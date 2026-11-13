use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
#[cfg(windows)]
use std::io::IsTerminal;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::{
    Config, NOTIFICATION_CONFIG_KEYS, config_path, load_notification_file,
    notification_config_path, power_is_off, save_notification_config, state_dir,
};
use crate::notify::{discover_telegram_chats, notify_telegram};
use crate::platform::federation_dir;
use crate::storage::{ensure_private_dir, write_atomic_json};

const HARD_CAP_KEY: &str = "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB";
fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn control_acknowledgement(result: &Value, action: &str, pid: u32) -> (i32, String, bool) {
    let Some(result) = result.as_object() else {
        return (
            2,
            "control acknowledgement unreadable: top-level value must be an object".to_owned(),
            true,
        );
    };
    if result.get("ok").and_then(Value::as_bool) == Some(true) {
        return (0, format!("completed: {action} pid={pid}"), false);
    }
    let error = result
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    if result.get("signal_completed").and_then(Value::as_bool) == Some(true) {
        return (
            2,
            format!(
                "signal completed but runtime finalization is unconfirmed: {action} pid={pid}: {error}; inspect memory-status and do not repeat blindly"
            ),
            true,
        );
    }
    (1, format!("rejected: {action} pid={pid}: {error}"), true)
}

fn submit_control(action: &str, pid: u32, timeout: f64) -> i32 {
    let directory = state_dir();
    let control = directory.join("control");
    let results = control.join("results");
    if let Err(error) = ensure_private_dir(&control).and_then(|_| ensure_private_dir(&results)) {
        eprintln!("control request directory unavailable: {error}");
        return 2;
    }
    let request_id = format!("{}-{}", now_nanos(), std::process::id());
    let request_path = control.join(format!("{request_id}.json"));
    let result_path = results.join(format!("{request_id}.json"));
    if let Err(error) = write_atomic_json(
        &request_path,
        &json!({"request_id": request_id, "action": action, "pid": pid}),
        0o600,
        true,
    ) {
        eprintln!("control request was not saved: {error}");
        return 2;
    }
    let started = Instant::now();
    while started.elapsed().as_secs_f64() < timeout {
        match fs::read(&result_path) {
            Ok(source) => {
                let _ = fs::remove_file(&result_path);
                let result = match serde_json::from_slice::<Value>(&source) {
                    Ok(result) => result,
                    Err(error) => {
                        eprintln!("control acknowledgement unreadable: {error}");
                        return 2;
                    }
                };
                let (code, message, stderr) = control_acknowledgement(&result, action, pid);
                if stderr {
                    eprintln!("{message}");
                } else {
                    println!("{message}");
                }
                return code;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                eprintln!("control acknowledgement unreadable: {error}");
                return 2;
            }
        }
    }
    eprintln!(
        "unconfirmed after {timeout}s: {action} pid={pid}; inspect memory-status before retrying"
    );
    2
}

#[cfg(unix)]
struct EchoGuard(bool);

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        if self.0 {
            let _ = Command::new("stty")
                .arg("echo")
                .stdin(Stdio::inherit())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            eprintln!();
        }
    }
}

#[cfg(unix)]
fn prompt_secret(label: &str) -> Result<String, String> {
    eprint!("{label} (hidden): ");
    io::stderr().flush().map_err(|error| error.to_string())?;
    let hidden = Command::new("stty")
        .arg("-echo")
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let _guard = EchoGuard(hidden);
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| format!("secret input failed: {error}"))?;
    validate_secret(value.trim())
}

#[cfg(windows)]
fn prompt_secret(label: &str) -> Result<String, String> {
    if !io::stdin().is_terminal() {
        let mut value = String::new();
        io::stdin()
            .read_line(&mut value)
            .map_err(|error| format!("secret input failed: {error}"))?;
        return validate_secret(value.trim());
    }
    let script = concat!(
        "$s=Read-Host $env:MEMORY_SUPERVISOR_SECRET_LABEL -AsSecureString;",
        "$b=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($s);",
        "try {[Console]::Out.Write([Runtime.InteropServices.Marshal]::PtrToStringBSTR($b))} ",
        "finally {[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($b)}"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .env("MEMORY_SUPERVISOR_SECRET_LABEL", label)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("secret input failed: {error}"))?;
    if !output.status.success() {
        return Err("secret input was cancelled".to_owned());
    }
    validate_secret(String::from_utf8_lossy(&output.stdout).trim())
}

fn validate_secret(value: &str) -> Result<String, String> {
    if value.is_empty() || value.contains(['\r', '\n']) {
        Err("secret cannot be empty and must fit on one line".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

fn telegram(arguments: &[String]) -> i32 {
    let path = notification_config_path();
    let mut values = load_notification_file(&path);
    if arguments.first().map(String::as_str) == Some("off") {
        for key in NOTIFICATION_CONFIG_KEYS {
            values.remove(key);
        }
        if let Err(error) = save_notification_config(&path, &values) {
            eprintln!("Telegram settings were not saved: {error}");
            return 1;
        }
        println!("Telegram off. Check state any time with `calando status`.");
        return 0;
    }
    let explicit_chat = arguments.first().cloned();
    if arguments.len() > 1
        || explicit_chat.as_ref().is_some_and(|id| {
            let digits = id.strip_prefix('-').unwrap_or(id);
            digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        eprintln!("usage: calando telegram [CHAT_ID] | calando telegram off");
        return 2;
    }
    let token = match prompt_secret("Telegram bot token from @BotFather") {
        Ok(token) => token,
        Err(error) => {
            eprintln!("{error}; nothing was saved");
            return 1;
        }
    };
    let chat_id = match explicit_chat {
        Some(chat_id) => chat_id,
        None => {
            let mut chats = match discover_telegram_chats(&token, 0) {
                Ok(chats) => chats,
                Err(error) => return telegram_discovery_error(&error),
            };
            if chats.is_empty() {
                println!(
                    "Open your bot in Telegram and send it any message. Waiting 120 seconds..."
                );
                chats = match discover_telegram_chats(&token, 120) {
                    Ok(chats) => chats,
                    Err(error) => return telegram_discovery_error(&error),
                };
            }
            match chats.len() {
                0 => {
                    eprintln!("No message reached the bot within 120 seconds. Nothing was saved.");
                    return 1;
                }
                1 => chats.pop().unwrap().0,
                _ => {
                    eprintln!("Several chats wrote to this bot:");
                    for (id, label) in chats {
                        eprintln!("  {id}  {label}");
                    }
                    eprintln!("Choose one: calando telegram <CHAT_ID>");
                    return 1;
                }
            }
        }
    };
    values.insert("MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN".to_owned(), token);
    values.insert("MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID".to_owned(), chat_id);
    if let Err(error) = save_notification_config(&path, &values) {
        eprintln!("Telegram settings were not saved: {error}");
        return 1;
    }
    match notify_telegram(
        "Telegram connected\nYou will get a message when Calando acts.",
        &values,
    )
    .as_str()
    {
        "delivered" => {
            println!("Telegram on. A test message was sent.");
            0
        }
        _ => {
            eprintln!("Saved, but the test message failed. Check the bot and chat, then retry.");
            1
        }
    }
}

fn telegram_discovery_error(error: &str) -> i32 {
    eprintln!("Telegram chat discovery failed: {error}");
    if error.contains("HTTP 401") {
        eprintln!("The token was rejected. Copy the current token from @BotFather and try again.");
    } else if error.contains("HTTP 409") {
        eprintln!(
            "This bot already has a webhook or another getUpdates client. Use a dedicated bot for Calando; no existing webhook was changed."
        );
    }
    eprintln!("Nothing was saved.");
    1
}

fn installation_root() -> PathBuf {
    if let Some(path) = env::var_os("MEMORY_SUPERVISOR_INSTALL_ROOT") {
        return PathBuf::from(path);
    }
    if let Ok(executable) = env::current_exe() {
        for ancestor in executable.ancestors().skip(1).take(5) {
            if ancestor.join("packaging").join("install.sh").is_file()
                || ancestor.join("packaging").join("install.ps1").is_file()
                || ancestor.join("install.sh").is_file()
                || ancestor.join("install.ps1").is_file()
            {
                return ancestor.to_path_buf();
            }
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Ok(value) = fs::read_to_string(crate::config::pointer_dir_in(&home).join("install-root"))
    {
        let value = value.trim();
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    crate::config::renamed(
        &home.join(".local").join("share"),
        "calando",
        "memory-supervisor",
    )
}

fn maintenance_script(root: &Path, name: &str) -> PathBuf {
    let structured = root.join("packaging").join(name);
    if structured.is_file() {
        structured
    } else {
        root.join(name)
    }
}

fn refresh_release_installation(root: &Path) -> io::Result<std::process::ExitStatus> {
    if let Some(script) = env::var_os("MEMORY_SUPERVISOR_BOOTSTRAP_FILE") {
        return if cfg!(windows) {
            Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
                .status()
        } else {
            Command::new("/bin/sh")
                .arg(script)
                .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
                .status()
        };
    }

    // Only files named in the signed release list are installed; the bootstrap script comes from
    // this installation, not from the network.
    let base = env::var("MEMORY_SUPERVISOR_RELEASE_BASE_URL").unwrap_or_else(|_| {
        "https://github.com/lssLab/Calando/releases/latest/download".to_owned()
    });
    let work = env::temp_dir().join(format!("calando-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let result = (|| {
        let verified =
            crate::release::fetch(base.trim_end_matches('/'), &work).map_err(io::Error::other)?;
        println!("Verified signed release {}.", verified.version);
        let script_name = if cfg!(windows) {
            "bootstrap.ps1"
        } else {
            "bootstrap.sh"
        };
        let script = work.join(script_name);
        fs::copy(root.join(script_name), &script)?;
        let mut command = if cfg!(windows) {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            command
        } else {
            Command::new("/bin/sh")
        };
        command
            .arg(&script)
            .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
            .env("MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE", &verified.source)
            .env(
                "MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE",
                &verified.source_checksum,
            );
        if let Some(binary) = &verified.binary {
            command.env("MEMORY_SUPERVISOR_BINARY_SOURCE", binary);
        }
        command.status()
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

fn maintain_from_copy(action: &str, root: &Path) -> i32 {
    let copy = env::temp_dir().join(format!("calando-maintain-{}.exe", std::process::id()));
    let status = env::current_exe()
        .and_then(|exe| fs::copy(exe, &copy))
        .and_then(|_| {
            Command::new(&copy)
                .args(["maintain", action])
                .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
                .status()
        });
    let _ = fs::remove_file(&copy);
    match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("maintenance failed: {error}");
            1
        }
    }
}

fn maintain_installation(action: &str) -> i32 {
    let root = installation_root();
    // Windows cannot replace or delete a running executable, so the installed binary hands
    // the work to a temporary copy of itself.
    if cfg!(windows) && crate::update::running_installed_binary() {
        return maintain_from_copy(action, &root);
    }
    let mut update_status = 0;
    if action == "update" {
        println!("Updating Calando source...");
        if root.join(".git").is_dir() {
            update_status = Command::new("git")
                .args(["-C"])
                .arg(&root)
                .args(["pull", "--ff-only"])
                .status()
                .map(|status| status.code().unwrap_or(1))
                .unwrap_or(1);
        } else {
            match refresh_release_installation(&root) {
                Ok(status) if status.success() => return 0,
                Ok(status) => update_status = status.code().unwrap_or(1),
                Err(error) => {
                    eprintln!("Release source update failed: {error}");
                    update_status = 1;
                }
            }
        }
        if update_status != 0 {
            eprintln!("Source update did not complete; reapplying the current revision.");
        }
    }
    let script = if cfg!(windows) {
        maintenance_script(
            &root,
            if action == "uninstall" {
                "uninstall.ps1"
            } else {
                "install.ps1"
            },
        )
    } else {
        maintenance_script(
            &root,
            if action == "uninstall" {
                "uninstall.sh"
            } else {
                "install.sh"
            },
        )
    };
    if !script.is_file() {
        eprintln!("maintenance script is missing: {}", script.display());
        return 1;
    }
    println!(
        "{}",
        if action == "uninstall" {
            "Removing Calando while preserving state and notification settings..."
        } else {
            "Reapplying the service and all detected CLI connections..."
        }
    );
    let mut command = if cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command
    } else {
        Command::new("/bin/sh")
    };
    command.arg(script);
    // Reapplying a release install reuses this binary: the installer would otherwise download
    // one from the release without the signed-list check.
    let reuse = (action != "uninstall"
        && !root.join(".git").is_dir()
        && env::var_os("MEMORY_SUPERVISOR_BINARY_SOURCE").is_none())
    .then(|| {
        let copy = env::temp_dir().join(format!(
            "calando-current-{}{}",
            std::process::id(),
            env::consts::EXE_SUFFIX
        ));
        fs::copy(env::current_exe().ok()?, &copy).ok()?;
        Some(copy)
    })
    .flatten();
    if let Some(copy) = &reuse {
        command.env("MEMORY_SUPERVISOR_BINARY_SOURCE", copy);
    }
    let status = command.status();
    if let Some(copy) = &reuse {
        let _ = fs::remove_file(copy);
    }
    match status {
        Ok(status) if status.success() && update_status == 0 => 0,
        Ok(_) => 1,
        Err(error) => {
            eprintln!("maintenance failed: {error}");
            1
        }
    }
}

fn power_off_preflight_at(path: &Path) -> Result<(), String> {
    let source = match fs::read(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot verify paused processes: {error}")),
    };
    let runtime: Value = serde_json::from_slice(&source)
        .map_err(|error| format!("cannot verify paused processes: {error}"))?;
    let stopped = runtime
        .get("stopped")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "cannot verify paused processes: runtime stopped ledger is invalid".to_owned()
        })?;
    let stopped: Vec<_> = stopped
        .keys()
        .map(|pid| {
            pid.parse::<u32>().map_err(|_| {
                "cannot verify paused processes: runtime stopped PID is invalid".to_owned()
            })
        })
        .collect::<Result<_, _>>()?;
    if !stopped.is_empty() {
        return Err(format!(
            "cannot turn off: paused PID(s) {} could not be resumed",
            stopped
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if runtime
        .get("pending_control")
        .is_some_and(|value| !value.is_null())
    {
        return Err(
            "cannot turn off while a process control action is pending; retry after it completes"
                .to_owned(),
        );
    }
    Ok(())
}

fn power_off_preflight() -> Result<(), String> {
    power_off_preflight_at(&state_dir().join("runtime.json"))
}

fn clear_local_publication() {
    let directory = state_dir();
    let _ = fs::remove_file(directory.join("admission-green.lease"));
    let instance = fs::read(directory.join("state.json"))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .and_then(|state| {
            state
                .get("instance")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    if let Some(instance) = instance.filter(|instance| {
        !instance.is_empty()
            && instance.len() <= 128
            && instance.chars().all(|character| {
                character.is_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
    }) {
        let _ = fs::remove_file(federation_dir().join(format!("{instance}.json")));
    }
}

fn run_power_script(script: &Path, action: &str) -> io::Result<std::process::ExitStatus> {
    let binary = env::current_exe().ok();
    let mut command = if cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .arg(action);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.arg(script).arg(action);
        command
    };
    if let Some(binary) = binary {
        command.env("MEMORY_SUPERVISOR_BINARY", binary);
    }
    command.status()
}

/// `off` first lets the running daemon resume every process it paused, so turning
/// Calando off never leaves work frozen.
fn release_paused_processes() {
    let stopped: Vec<u32> = read_json_file(&state_dir().join("runtime.json"))
        .and_then(|runtime| {
            runtime
                .get("stopped")?
                .as_object()
                .map(|stopped| stopped.keys().filter_map(|pid| pid.parse().ok()).collect())
        })
        .unwrap_or_default();
    for pid in stopped {
        submit_control("resume", pid, 5.0);
    }
}

/// Paused processes as `(pid, label)`, oldest first. The PID is the paused CLI process inside
/// a terminal (Claude Code or Codex), not the terminal.
fn paused_processes() -> Vec<(u32, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or_default();
    let mut paused: Vec<(f64, u32, String)> = read_json_file(&state_dir().join("runtime.json"))
        .and_then(|runtime| runtime.get("stopped")?.as_object().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|(pid, record)| {
            let pid: u32 = pid.parse().ok()?;
            let text = |key: &str| record.get(key).and_then(Value::as_str).unwrap_or("?");
            let since = record
                .get("suspended_at")
                .and_then(Value::as_f64)
                .unwrap_or(now);
            let gb = record
                .get("anon_mb")
                .and_then(Value::as_f64)
                .map(|mb| format!(" · {:.1} GB", mb / 1024.0))
                .unwrap_or_default();
            let minutes = ((now - since) / 60.0).max(0.0).round();
            Some((
                since,
                pid,
                format!(
                    "{} ({}){gb} · paused {minutes:.0} min ago",
                    text("name"),
                    text("role")
                ),
            ))
        })
        .collect();
    paused.sort_by(|left, right| left.0.total_cmp(&right.0));
    paused
        .into_iter()
        .map(|(_, pid, label)| (pid, label))
        .collect()
}

fn resume(arguments: &[String]) -> i32 {
    let paused = paused_processes();
    if paused.is_empty() {
        println!("Nothing is paused.");
        return 0;
    }
    let chosen: Vec<u32> = match arguments.first().map(String::as_str) {
        Some("all") => paused.iter().map(|(pid, _)| *pid).collect(),
        Some(pid) => match pid.parse::<u32>() {
            Ok(pid) if paused.iter().any(|(paused, _)| *paused == pid) => vec![pid],
            _ => {
                eprintln!("PID {pid} is not paused by Calando. Run `calando resume` to list them.");
                return 2;
            }
        },
        None => {
            println!("Paused sessions:");
            let labels: Vec<String> = paused
                .iter()
                .map(|(pid, label)| format!("PID {pid}  {label}"))
                .collect();
            match crate::picker::pick(&labels) {
                Some(crate::picker::Pick::One(index)) => vec![paused[index].0],
                Some(crate::picker::Pick::All) => paused.iter().map(|(pid, _)| *pid).collect(),
                Some(crate::picker::Pick::Cancel) => {
                    println!("Cancelled; nothing changed.");
                    return 0;
                }
                None => {
                    for (index, label) in labels.iter().enumerate() {
                        println!("  {}) {label}", index + 1);
                    }
                    println!("Run `calando resume <pid>` or `calando resume all`.");
                    return 1;
                }
            }
        }
    };
    let mut code = 0;
    for pid in chosen {
        code = code.max(submit_control("resume", pid, 5.0));
    }
    code
}

fn lock(arguments: &[String]) -> i32 {
    let path = crate::config::lock_path();
    let result = match arguments.first().map(String::as_str) {
        Some("on") => path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| {
                fs::write(
                    &path,
                    "Calando cannot be turned off while this file exists.\n",
                )
            }),
        Some("off") => match fs::remove_file(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
        _ => {
            println!(
                "Lock: {}\nUsage: calando lock on | off (administrator)",
                if crate::config::locked() { "on" } else { "off" }
            );
            return 0;
        }
    };
    match result {
        Ok(()) => {
            println!(
                "Lock {}. {}",
                arguments[0],
                if arguments[0] == "on" {
                    "Calando cannot be turned off from a terminal or a session until an administrator runs `calando lock off`."
                } else {
                    "Calando can be turned off again."
                }
            );
            0
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!(
                "The lock needs administrator rights: {}",
                if cfg!(windows) {
                    "run this in an Administrator terminal"
                } else {
                    "run `sudo \"$(command -v calando)\" lock ...`"
                }
            );
            1
        }
        Err(error) => {
            eprintln!("lock was not changed: {error}");
            1
        }
    }
}

fn maintain_power(action: &str) -> i32 {
    if matches!(action, "off" | "uninstall") && crate::config::locked() {
        eprintln!(
            "Calando is locked on by an administrator; `calando lock off` (as administrator) first."
        );
        return 1;
    }
    if action == "off" {
        release_paused_processes();
    }
    if action == "off"
        && let Err(error) = power_off_preflight()
    {
        eprintln!("{error}; supervisor remains on");
        return 1;
    }
    let root = installation_root();
    let script = maintenance_script(
        &root,
        if cfg!(windows) {
            "power.ps1"
        } else {
            "power.sh"
        },
    );
    if !script.is_file() {
        eprintln!("power control script is missing: {}", script.display());
        return 1;
    }
    match run_power_script(&script, action) {
        Ok(status) if status.success() => {
            if action == "off" {
                if let Err(error) = power_off_preflight() {
                    let restored =
                        run_power_script(&script, "on").is_ok_and(|status| status.success());
                    if !restored && power_is_off() {
                        clear_local_publication();
                    }
                    eprintln!(
                        "{error}; off was rolled back and supervisor is {}",
                        if restored { "on" } else { "not confirmed on" }
                    );
                    return 1;
                }
                clear_local_publication();
            }
            0
        }
        Ok(_) => {
            if power_is_off() {
                clear_local_publication();
            }
            1
        }
        Err(error) => {
            if power_is_off() {
                clear_local_publication();
            }
            eprintln!("power control failed: {error}");
            1
        }
    }
}

fn hard_cap(arguments: &[String]) -> i32 {
    let Some(action) = arguments.first().map(String::as_str) else {
        eprintln!("hard-cap requires show, set MB, or off");
        return 2;
    };
    let config = Config::load(&config_path());
    if let Some(error) = config.configuration_error() {
        eprintln!("configuration is unreadable; nothing changed: {error}");
        return 1;
    }
    let persisted = config.values().get(HARD_CAP_KEY).cloned();
    let override_value = env::var(HARD_CAP_KEY).ok();
    if action == "show" {
        if let Some(value) = override_value
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .or(persisted)
            .filter(|value| !value.is_null() && value.as_str() != Some(""))
        {
            println!(
                "local combined Claude Code/Codex memory limit: {} MB ({})",
                match value {
                    Value::String(value) => value,
                    value => value.to_string(),
                },
                if override_value.is_some() {
                    "environment override".to_owned()
                } else {
                    config_path().display().to_string()
                }
            );
        } else {
            println!("local combined Claude Code/Codex memory limit: OFF (automatic mode)");
        }
        println!(
            "scope: all visible Claude Code and Codex programs for this user in the local process space"
        );
        println!(
            "Windows, each WSL distribution, VM guest, and PID-isolated container are configured separately"
        );
        return 0;
    }
    if override_value.is_some() {
        eprintln!(
            "{HARD_CAP_KEY} is set in the environment and overrides the config file; unset it first; nothing changed"
        );
        return 1;
    }
    let mut updated = config.values().clone();
    let message = match action {
        "set" => {
            let Some(raw) = arguments.get(1) else {
                eprintln!("hard-cap set requires MB");
                return 2;
            };
            let Ok(number) = raw.parse::<f64>() else {
                eprintln!("hard cap must be a finite number of MB >= 1");
                return 2;
            };
            if !number.is_finite() || number < 1.0 {
                eprintln!("hard cap must be a finite number of MB >= 1");
                return 2;
            }
            updated.insert(HARD_CAP_KEY.to_owned(), json!(number));
            format!("set local combined Claude Code/Codex memory limit to {number} MB")
        }
        "off" => {
            updated.remove(HARD_CAP_KEY);
            "disabled local combined Claude Code/Codex memory limit (automatic mode)".to_owned()
        }
        _ => {
            eprintln!("unknown hard-cap subcommand: {action}");
            return 2;
        }
    };
    if let Err(error) = write_atomic_json(&config_path(), &updated, 0o600, true) {
        eprintln!("hard-cap configuration was not saved: {error}");
        return 1;
    }
    println!("{message}; preserved other settings");
    let status = maintain_installation("apply");
    if status != 0 {
        eprintln!("hard-cap setting was saved but service reload failed; run `calando update`");
    }
    status
}

pub fn usage() -> &'static str {
    "Calando - memory protection for Claude Code and Codex

Usage: calando <command>

  status [--json]        Show memory, protection and agents
  on | off               Turn protection on or off for this environment
  resume [<pid> | all]   Continue a paused session (lists them when no PID is given)
  telegram [CHAT_ID]     Send notices to Telegram (asks for the bot token)
  telegram off           Stop Telegram notices
  cap [<GB> | off]       Advanced: hard memory cap for all agents here (asks to confirm)
  update                 Update now (automatic by default)
  update auto on | off   Turn automatic updates on or off
  uninstall              Remove Calando, keeping its settings
  lock on | off          Administrator: stop anyone, including agents, from turning Calando off
  help                   Show this help

Inside a Claude Code or Codex session, /calando (Codex: $calando) affects only that session."
}

struct BudgetPeer {
    instance: String,
    capacity_mb: Option<f64>,
    cap_mb: Option<f64>,
    age_s: f64,
}

impl BudgetPeer {
    fn stale(&self) -> bool {
        !(-5.0..=10.0).contains(&self.age_s)
    }

    fn stale_note(&self) -> &'static str {
        if self.stale() {
            " [stale snapshot — verify that kernel's supervisor]"
        } else {
            ""
        }
    }
}

struct BudgetView {
    own_capacity_mb: f64,
    physical_mb: f64,
    peers: Vec<BudgetPeer>,
    own_cap_mb: Option<f64>,
}

impl BudgetView {
    fn other_claims_mb(&self) -> f64 {
        self.peers
            .iter()
            .filter_map(|peer| peer.cap_mb)
            .fold(0.0, |total, cap| total + cap)
    }

    fn current_mb(&self) -> f64 {
        (self.physical_mb - self.other_claims_mb())
            .min(self.own_capacity_mb)
            .max(0.0)
    }
}

fn parse_budget_amount(raw: &str) -> Result<f64, String> {
    let lower = raw.trim().to_lowercase();
    let (number, multiplier) = if let Some(number) = lower
        .strip_suffix("mib")
        .or_else(|| lower.strip_suffix("mb"))
        .or_else(|| lower.strip_suffix('m'))
    {
        (number, 1.0)
    } else if let Some(number) = lower
        .strip_suffix("gib")
        .or_else(|| lower.strip_suffix("gb"))
        .or_else(|| lower.strip_suffix('g'))
    {
        (number, 1024.0)
    } else {
        (lower.as_str(), 1024.0)
    };
    let number: f64 = number
        .trim()
        .parse()
        .map_err(|_| format!("budget must be a number of GiB, or use an MB/GB suffix: {raw}"))?;
    let mb = number * multiplier;
    if !mb.is_finite() || mb < 1.0 {
        return Err("budget must be at least 1 MB".to_owned());
    }
    Ok(mb)
}

fn mb_gib(mb: f64) -> String {
    format!("{:.0} MB ({:.1} GiB)", mb, mb / 1024.0)
}

fn format_mb_argument(mb: f64) -> String {
    if mb.fract() == 0.0 {
        format!("{}", mb as u64)
    } else {
        format!("{mb}")
    }
}

fn read_json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn collect_budget_view(now: f64) -> Result<BudgetView, String> {
    let state = read_json_file(&state_dir().join("state.json")).ok_or(
        "local supervisor state is unavailable; start or repair the daemon with \
         `calando update`",
    )?;
    let age = now - state.get("ts").and_then(Value::as_f64).unwrap_or_default();
    if !(-5.0..=10.0).contains(&age) {
        return Err(format!(
            "local supervisor state is stale ({age:.0}s old); cross-kernel budget accounting \
             needs the live daemon"
        ));
    }
    let own_instance = state
        .get("instance")
        .and_then(Value::as_str)
        .unwrap_or("local")
        .to_owned();
    let own_capacity_mb = state
        .get("memory_capacity_mb")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or("local capacity is unknown; budget validation is unavailable")?;
    let own_cap_mb = state
        .get("cli_hard_cap_mb")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0);

    let mut peers: BTreeMap<String, BudgetPeer> = BTreeMap::new();
    let federation = federation_dir();
    if crate::topology::channel_is_host_local(&federation)
        && let Ok(entries) = fs::read_dir(federation)
    {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|value| value != "json") {
                continue;
            }
            let Some(snapshot) = read_json_file(&path) else {
                continue;
            };
            let instance = snapshot
                .get("instance")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if instance.is_empty() || instance == own_instance {
                continue;
            }
            let peer = BudgetPeer {
                instance: instance.clone(),
                capacity_mb: snapshot
                    .get("memory_capacity_mb")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0),
                cap_mb: snapshot
                    .get("cli_hard_cap_mb")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0),
                age_s: now
                    - snapshot
                        .get("ts")
                        .and_then(Value::as_f64)
                        .unwrap_or_default(),
            };
            match peers.get(&instance) {
                Some(existing) if existing.age_s <= peer.age_s => {}
                _ => {
                    peers.insert(instance, peer);
                }
            }
        }
    }
    let peers: Vec<_> = peers.into_values().collect();
    let physical_mb = peers
        .iter()
        .filter_map(|peer| peer.capacity_mb)
        .fold(own_capacity_mb, f64::max);
    Ok(BudgetView {
        own_capacity_mb,
        physical_mb,
        peers,
        own_cap_mb,
    })
}

fn show_budget(view: &BudgetView) -> i32 {
    println!(
        "Memory cap here: {}",
        view.own_cap_mb
            .map(mb_gib)
            .unwrap_or_else(|| "off (Calando protects memory automatically)".to_owned())
    );
    println!("Largest cap possible now: {}", mb_gib(view.current_mb()));
    for peer in view.peers.iter().filter(|peer| peer.cap_mb.is_some()) {
        println!(
            "Cap in {}: {}{}",
            peer.instance,
            peer.cap_mb.map(mb_gib).unwrap_or_default(),
            peer.stale_note()
        );
    }
    let total = view.other_claims_mb() + view.own_cap_mb.unwrap_or_default();
    if total > view.physical_mb {
        println!(
            "Warning: caps across environments total {}, more than this machine's {}.",
            mb_gib(total),
            mb_gib(view.physical_mb)
        );
    }
    println!("Set: calando cap <GB>    Remove: calando cap off");
    0
}

fn confirm_on_stdin() -> bool {
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => {
            eprintln!();
            false
        }
        Ok(_) => matches!(line.trim().to_lowercase().as_str(), "y" | "yes"),
    }
}

pub(crate) const CAP_EXPLAINER: &str = "Memory cap (advanced)
Not recommended for normal use: Calando already protects memory automatically.
Use it only when you need a strict limit, such as research or benchmark runs.
Effect: new heavy agent work is held near the cap; above it, one growing subagent is paused.
Limit: a pause cannot take back memory already used, and use can briefly pass the cap between checks.";

fn cap(arguments: &[String]) -> i32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let assume_yes = arguments.iter().any(|argument| argument == "--yes");
    let values: Vec<_> = arguments
        .iter()
        .filter(|argument| *argument != "--yes")
        .collect();
    let raw = match values.as_slice() {
        [] => {
            return match collect_budget_view(now) {
                Ok(view) => show_budget(&view),
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        [value] if value.as_str() == "off" => return hard_cap(&["off".to_owned()]),
        [value] => value.as_str(),
        _ => {
            eprintln!("usage: calando cap [<GB> [--yes] | off]");
            return 2;
        }
    };
    let requested_mb = match parse_budget_amount(raw) {
        Ok(requested_mb) => requested_mb,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    if env::var_os(HARD_CAP_KEY).is_some() {
        eprintln!(
            "{HARD_CAP_KEY} is set in the environment and overrides the config file; unset it first; nothing changed"
        );
        return 1;
    }
    let view = match collect_budget_view(now) {
        Ok(view) => view,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if requested_mb > view.current_mb() {
        eprintln!(
            "{} is more than this environment can use now ({}); nothing changed",
            mb_gib(requested_mb),
            mb_gib(view.current_mb())
        );
        return 1;
    }
    if !assume_yes {
        eprintln!("{CAP_EXPLAINER}\n");
        eprint!(
            "Apply a cap of {} for all agents here? [y/N]: ",
            mb_gib(requested_mb)
        );
        if !confirm_on_stdin() {
            eprintln!("nothing changed (non-interactive callers add --yes)");
            return 1;
        }
    }
    apply_budget(requested_mb, &view)
}

fn apply_budget(requested_mb: f64, view: &BudgetView) -> i32 {
    let status = hard_cap(&["set".to_owned(), format_mb_argument(requested_mb)]);
    if status == 0 {
        println!(
            "this kernel's CLI memory budget: {} of the {} currently possible; explicit budgets \
             across kernels now total {} of the {} physical estimate",
            mb_gib(requested_mb),
            mb_gib(view.current_mb()),
            mb_gib(view.other_claims_mb() + requested_mb),
            mb_gib(view.physical_mb)
        );
    }
    status
}

pub fn run_control(arguments: &[OsString]) -> i32 {
    let arguments: Vec<String> = match arguments
        .iter()
        .map(|value| value.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
    {
        Some(arguments) => arguments,
        None => {
            eprintln!("arguments must be valid Unicode\n{}", usage());
            return 2;
        }
    };
    let Some(action) = arguments.first().map(String::as_str) else {
        println!("{}", usage());
        return 2;
    };
    match action {
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            0
        }
        "telegram" => telegram(&arguments[1..]),
        "resume" => resume(&arguments[1..]),
        "cap" => cap(&arguments[1..]),
        "on" | "off" if arguments.len() == 1 => maintain_power(action),
        "lock" => lock(&arguments[1..]),
        "uninstall" if arguments.len() == 1 && crate::config::locked() => maintain_power(action),
        "update" if arguments.get(1).map(String::as_str) == Some("auto") => {
            let enabled = match arguments.get(2).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => {
                    eprintln!("usage: calando update auto on|off");
                    return 2;
                }
            };
            match crate::update::set_auto(enabled) {
                Ok(()) => {
                    println!(
                        "Auto update {}.",
                        if enabled {
                            "on"
                        } else {
                            "off; `calando status` and Telegram will tell you when an update is available"
                        }
                    );
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            }
        }
        "update" | "uninstall" if arguments.len() == 1 => maintain_installation(action),
        // Internal: the temporary copy a Windows maintenance run hands its work to.
        "maintain" if arguments.len() == 2 => maintain_installation(&arguments[1]),
        _ => {
            eprintln!("unknown command: {}\n\n{}", arguments.join(" "), usage());
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_paths_prefer_the_structured_layout_and_accept_v020_flat_layout() {
        let root = env::temp_dir().join(format!(
            "memory-supervisor-maintenance-path-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        fs::create_dir_all(root.join("packaging")).unwrap();

        let legacy = root.join("install.sh");
        fs::write(&legacy, "legacy").unwrap();
        assert_eq!(maintenance_script(&root, "install.sh"), legacy);

        let structured = root.join("packaging").join("install.sh");
        fs::write(&structured, "structured").unwrap();
        assert_eq!(maintenance_script(&root, "install.sh"), structured);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn power_off_refuses_to_strand_paused_or_pending_process_control() {
        let root = env::temp_dir().join(format!(
            "memory-supervisor-power-preflight-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("runtime.json");
        fs::write(&path, br#"{"stopped":{},"pending_control":null}"#).unwrap();
        assert!(power_off_preflight_at(&path).is_ok());
        fs::write(
            &path,
            br#"{"stopped":{"42":"42:start"},"pending_control":null}"#,
        )
        .unwrap();
        assert!(power_off_preflight_at(&path).unwrap_err().contains("42"));
        fs::write(
            &path,
            br#"{"stopped":{},"pending_control":{"action":"resume"}}"#,
        )
        .unwrap();
        assert!(
            power_off_preflight_at(&path)
                .unwrap_err()
                .contains("pending")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_acknowledgements_distinguish_rejection_from_uncommitted_signal() {
        assert_eq!(
            control_acknowledgement(&json!({"ok":true}), "resume", 23),
            (0, "completed: resume pid=23".to_owned(), false)
        );
        let rejected = control_acknowledgement(
            &json!({"ok":false,"signal_completed":false,"error":"not managed"}),
            "resume",
            23,
        );
        assert_eq!(rejected.0, 1);
        assert!(rejected.1.contains("rejected"));
        let uncommitted = control_acknowledgement(
            &json!({"ok":false,"signal_completed":true,"error":"disk full"}),
            "resume",
            23,
        );
        assert_eq!(uncommitted.0, 2);
        assert!(uncommitted.1.contains("do not repeat blindly"));
        let malformed = control_acknowledgement(&json!([]), "resume", 23);
        assert_eq!(malformed.0, 2);
        assert!(malformed.1.contains("top-level value must be an object"));
    }

    fn view(own_capacity: f64, physical_peer: Option<f64>, peer_caps: &[f64]) -> BudgetView {
        let mut peers: Vec<BudgetPeer> = peer_caps
            .iter()
            .enumerate()
            .map(|(index, cap)| BudgetPeer {
                instance: format!("peer-{index}"),
                capacity_mb: None,
                cap_mb: Some(*cap),
                age_s: 1.0,
            })
            .collect();
        if let Some(capacity) = physical_peer {
            peers.push(BudgetPeer {
                instance: "host".to_owned(),
                capacity_mb: Some(capacity),
                cap_mb: None,
                age_s: 1.0,
            });
        }
        let physical_mb = peers
            .iter()
            .filter_map(|peer| peer.capacity_mb)
            .fold(own_capacity, f64::max);
        BudgetView {
            own_capacity_mb: own_capacity,
            physical_mb,
            peers,
            own_cap_mb: None,
        }
    }

    #[test]
    fn budget_amounts_default_to_gib_and_accept_unit_suffixes() {
        assert_eq!(parse_budget_amount("6").unwrap(), 6144.0);
        assert_eq!(parse_budget_amount("6.5").unwrap(), 6656.0);
        assert_eq!(parse_budget_amount("512mb").unwrap(), 512.0);
        assert_eq!(parse_budget_amount("512MB").unwrap(), 512.0);
        assert_eq!(parse_budget_amount("0.5G").unwrap(), 512.0);
        assert_eq!(parse_budget_amount(" 2 GB ").unwrap(), 2048.0);
        assert_eq!(parse_budget_amount("6GiB").unwrap(), 6144.0);
        assert_eq!(parse_budget_amount("512MiB").unwrap(), 512.0);
        assert!(parse_budget_amount("abc").is_err());
        assert!(parse_budget_amount("0").is_err());
        assert!(parse_budget_amount("-4").is_err());
        assert!(parse_budget_amount("g").is_err());
    }

    #[test]
    fn budget_totals_separate_theoretical_from_currently_possible() {
        let unconstrained = view(7941.0, Some(16108.0), &[]);
        assert_eq!(unconstrained.current_mb(), 7941.0);

        let host_claimed = view(7941.0, Some(16108.0), &[12288.0]);
        assert_eq!(host_claimed.current_mb(), 16108.0 - 12288.0);

        let small_claims = view(7941.0, Some(16108.0), &[4000.0, 2000.0]);
        assert_eq!(small_claims.current_mb(), 7941.0);

        let over_committed = view(7941.0, Some(16108.0), &[17000.0]);
        assert_eq!(over_committed.current_mb(), 0.0);

        let alone = view(7941.0, None, &[]);
        assert_eq!(alone.physical_mb, 7941.0);
        assert_eq!(alone.current_mb(), 7941.0);
    }
}
