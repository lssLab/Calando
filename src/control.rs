use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
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
    CONFIGURABLE_NOTIFICATION_CHANNELS, Config, MANDATORY_NOTIFICATION_CHANNELS,
    NOTIFICATION_CONFIG_KEYS, config_path, load_notification_config, load_notification_file,
    notification_channels, notification_config_path, power_is_off, save_notification_config,
    state_dir,
};
use crate::notify::{
    discover_telegram_chats, notify_desktop_result, notify_discord, notify_telegram,
};
use crate::platform::federation_dir;
use crate::storage::{ensure_private_dir, write_atomic_json};

const HARD_CAP_KEY: &str = "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB";
const CHANNEL_ORDER: [&str; 5] = ["hook", "terminal", "os", "discord", "telegram"];
const OPTIONAL_CHANNEL_ORDER: [&str; 3] = ["os", "discord", "telegram"];
const DISCORD_KEYS: [&str; 4] = [
    "MEMORY_SUPERVISOR_DISCORD_WEBHOOK",
    "MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN",
    "MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID",
    "MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID",
];
const TELEGRAM_KEYS: [&str; 2] = [
    "MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN",
    "MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID",
];

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn positive_pid(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|pid| *pid > 1)
}

fn resolve_single_stopped_pid(directory: &Path) -> Result<u32, String> {
    let state = serde_json::from_slice::<Value>(
        &fs::read(directory.join("state.json"))
            .map_err(|error| format!("cannot read supervisor state: {error}"))?,
    )
    .map_err(|error| format!("cannot read supervisor state: {error}"))?;
    let mut pids: BTreeSet<_> = state
        .get("stopped_pids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(positive_pid)
        .collect();
    match pids.len() {
        0 => Err("no supervisor-managed paused process; no signal was sent".to_owned()),
        1 => Ok(pids.pop_first().unwrap()),
        _ => Err(format!(
            "multiple paused processes; choose one: {}; no signal was sent",
            pids.into_iter()
                .map(|pid| format!("memory-supervisor resume {pid}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
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

fn parse_routes(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_lowercase();
    if normalized == "all" {
        return Ok("all".to_owned());
    }
    if matches!(normalized.as_str(), "none" | "off") {
        return Ok("none".to_owned());
    }
    let selected: BTreeSet<_> = normalized
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    let mandatory: Vec<_> = selected
        .iter()
        .filter(|route| MANDATORY_NOTIFICATION_CHANNELS.contains(route))
        .copied()
        .collect();
    if !mandatory.is_empty() {
        return Err(format!(
            "{} is mandatory and cannot be configured; omit mandatory routes from the route list",
            mandatory.join(",")
        ));
    }
    let unknown: Vec<_> = selected
        .iter()
        .filter(|route| !CONFIGURABLE_NOTIFICATION_CHANNELS.contains(route))
        .copied()
        .collect();
    if selected.is_empty() || !unknown.is_empty() {
        return Err(format!(
            "invalid route(s): {}; expected all, none, or a comma-separated subset of os,discord,telegram",
            if unknown.is_empty() {
                value.to_owned()
            } else {
                unknown.join(",")
            }
        ));
    }
    Ok(OPTIONAL_CHANNEL_ORDER
        .into_iter()
        .filter(|route| selected.contains(route))
        .collect::<Vec<_>>()
        .join(","))
}

fn set_route(values: &mut BTreeMap<String, String>, route: &str, enabled: bool) {
    let mut selected: BTreeSet<_> = notification_channels(values)
        .into_iter()
        .filter(|channel| CONFIGURABLE_NOTIFICATION_CHANNELS.contains(&channel.as_str()))
        .collect();
    if enabled {
        selected.insert(route.to_owned());
    } else {
        selected.remove(route);
    }
    let serialized = if selected.len() == CONFIGURABLE_NOTIFICATION_CHANNELS.len() {
        "all".to_owned()
    } else if selected.is_empty() {
        "none".to_owned()
    } else {
        OPTIONAL_CHANNEL_ORDER
            .into_iter()
            .filter(|route| selected.contains(*route))
            .collect::<Vec<_>>()
            .join(",")
    };
    values.insert(
        "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(),
        serialized,
    );
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

fn save_and_report(values: &BTreeMap<String, String>, route: Option<&str>) -> i32 {
    let path = notification_config_path();
    if let Err(error) = save_notification_config(&path, values) {
        eprintln!("notification settings were not saved: {error}");
        return 1;
    }
    println!("saved private notification settings: {}", path.display());
    if let Some(route) = route {
        println!("enabled route: {route}");
    }
    let overrides: Vec<_> = NOTIFICATION_CONFIG_KEYS
        .into_iter()
        .filter(|key| env::var_os(key).is_some())
        .collect();
    if !overrides.is_empty() {
        eprintln!(
            "warning: environment override(s) still take precedence: {}",
            overrides.join(", ")
        );
    }
    0
}

fn show_notifications() -> i32 {
    let config = load_notification_config(&notification_config_path());
    let selected = notification_channels(&config);
    println!(
        "notification settings: {}",
        notification_config_path().display()
    );
    println!(
        "routes: {}",
        CHANNEL_ORDER
            .into_iter()
            .filter(|route| selected.contains(*route))
            .collect::<Vec<_>>()
            .join(",")
    );
    let discord = if config
        .get("MEMORY_SUPERVISOR_DISCORD_WEBHOOK")
        .is_some_and(|value| !value.is_empty())
    {
        "webhook configured (secret hidden)".to_owned()
    } else if config
        .get("MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN")
        .is_some_and(|value| !value.is_empty())
        && config
            .get("MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID")
            .is_some_and(|value| !value.is_empty())
    {
        format!(
            "bot configured for channel {} (token hidden)",
            config["MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID"]
        )
    } else if config
        .get("MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN")
        .is_some_and(|value| !value.is_empty())
        && config
            .get("MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID")
            .is_some_and(|value| !value.is_empty())
    {
        format!(
            "bot configured for DM user {} (token hidden)",
            config["MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID"]
        )
    } else if DISCORD_KEYS.iter().any(|key| config.contains_key(*key)) {
        "incomplete configuration".to_owned()
    } else {
        "not configured".to_owned()
    };
    let telegram = if config
        .get("MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN")
        .is_some_and(|value| !value.is_empty())
        && config
            .get("MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID")
            .is_some_and(|value| !value.is_empty())
    {
        format!(
            "bot configured for chat {} (token hidden)",
            config["MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID"]
        )
    } else if TELEGRAM_KEYS.iter().any(|key| config.contains_key(*key)) {
        "incomplete configuration".to_owned()
    } else {
        "not configured".to_owned()
    };
    println!("discord: {discord}");
    println!("telegram: {telegram}");
    0
}

fn test_notifications() -> i32 {
    let config = load_notification_config(&notification_config_path());
    let selected = notification_channels(&config);
    let message = "Notification connection test; no protective action occurred.";
    let desktop = if selected.contains("os") {
        notify_desktop_result(message)
    } else {
        crate::notify::DesktopResult {
            status: "disabled".to_owned(),
            route: "disabled-by-config".to_owned(),
        }
    };
    let discord_configured = config
        .get("MEMORY_SUPERVISOR_DISCORD_WEBHOOK")
        .is_some_and(|value| !value.is_empty())
        || (config
            .get("MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN")
            .is_some_and(|value| !value.is_empty())
            && [
                "MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID",
                "MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID",
            ]
            .into_iter()
            .any(|key| config.get(key).is_some_and(|value| !value.is_empty())));
    let telegram_configured = TELEGRAM_KEYS
        .into_iter()
        .all(|key| config.get(key).is_some_and(|value| !value.is_empty()));
    let discord = if !selected.contains("discord") {
        "disabled".to_owned()
    } else if !discord_configured {
        "not configured".to_owned()
    } else {
        notify_discord(message, &config)
    };
    let telegram = if !selected.contains("telegram") {
        "disabled".to_owned()
    } else if !telegram_configured {
        "not configured".to_owned()
    } else {
        notify_telegram(message, &config)
    };
    println!("os: {} ({})", desktop.status, desktop.route);
    println!("discord: {discord}");
    println!("telegram: {telegram}");
    println!("hook/terminal: tested only by a real supervisor action");
    if matches!(desktop.status.as_str(), "failed" | "unavailable")
        || matches!(discord.as_str(), "failed" | "skipped" | "not configured")
        || matches!(telegram.as_str(), "failed" | "skipped" | "not configured")
    {
        1
    } else {
        0
    }
}

fn configure_notifications(arguments: &[String]) -> i32 {
    let Some(action) = arguments.first().map(String::as_str) else {
        eprintln!("notifications requires a subcommand\n{}", usage());
        return 2;
    };
    if action == "show" {
        return show_notifications();
    }
    if action == "test" {
        return test_notifications();
    }
    let mut values = load_notification_file(&notification_config_path());
    match action {
        "routes" => {
            let Some(raw) = arguments.get(1) else {
                eprintln!("notifications routes requires ROUTES");
                return 2;
            };
            let routes = match parse_routes(raw) {
                Ok(routes) => routes,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            values.insert("MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(), routes);
            save_and_report(&values, None)
        }
        "discord-webhook" => {
            let secret = match prompt_secret("Discord webhook URL") {
                Ok(secret) => secret,
                Err(error) => {
                    eprintln!("{error}; nothing was saved");
                    return 1;
                }
            };
            let valid = secret
                .strip_prefix("https://discord.com/api/webhooks/")
                .and_then(|suffix| suffix.split_once('/'))
                .is_some_and(|(id, token)| {
                    !id.is_empty()
                        && id.bytes().all(|byte| byte.is_ascii_digit())
                        && !token.is_empty()
                });
            if !valid {
                eprintln!(
                    "Expected an https://discord.com/api/webhooks/... URL; nothing was saved."
                );
                return 1;
            }
            for key in DISCORD_KEYS {
                values.remove(key);
            }
            values.insert("MEMORY_SUPERVISOR_DISCORD_WEBHOOK".to_owned(), secret);
            set_route(&mut values, "discord", true);
            save_and_report(&values, Some("discord"))
        }
        "discord-channel" | "discord-dm" => {
            let Some(id) = arguments
                .get(1)
                .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                eprintln!("Discord IDs contain digits only");
                return 2;
            };
            let secret = match prompt_secret("Discord bot token") {
                Ok(secret) => secret,
                Err(error) => {
                    eprintln!("{error}; nothing was saved");
                    return 1;
                }
            };
            for key in DISCORD_KEYS {
                values.remove(key);
            }
            values.insert("MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN".to_owned(), secret);
            values.insert(
                if action == "discord-channel" {
                    "MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID"
                } else {
                    "MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID"
                }
                .to_owned(),
                id.clone(),
            );
            set_route(&mut values, "discord", true);
            let _ = fs::remove_file(
                notification_config_path()
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(".dm_channel"),
            );
            save_and_report(&values, Some("discord"))
        }
        "telegram" => {
            let explicit_chat = arguments.get(1).cloned();
            if explicit_chat.as_ref().is_some_and(|id| {
                let digits = id.strip_prefix('-').unwrap_or(id);
                digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                eprintln!("Telegram chat ID must be an integer");
                return 2;
            }
            let token = match prompt_secret("Telegram bot token") {
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
                            "No pending Telegram update yet. Open this exact bot and send /start or any new message now; waiting 120 seconds..."
                        );
                        chats = match discover_telegram_chats(&token, 120) {
                            Ok(chats) => chats,
                            Err(error) => return telegram_discovery_error(&error),
                        };
                    }
                    if chats.is_empty() {
                        eprintln!(
                            "No Telegram update arrived within 120 seconds. Confirm that you opened the bot matching this token, then run the command again and send a fresh /start or message while it waits. Nothing was saved."
                        );
                        return 1;
                    }
                    if chats.len() > 1 {
                        eprintln!("Multiple Telegram chats were found:");
                        for (id, label) in chats {
                            eprintln!("  {id}  {label}");
                        }
                        eprintln!("Run: memory-supervisor notifications telegram <CHAT_ID>");
                        return 1;
                    }
                    let (id, label) = chats.pop().unwrap();
                    println!("found Telegram chat: {id} ({label})");
                    id
                }
            };
            for key in TELEGRAM_KEYS {
                values.remove(key);
            }
            values.insert("MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN".to_owned(), token);
            values.insert("MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID".to_owned(), chat_id);
            set_route(&mut values, "telegram", true);
            save_and_report(&values, Some("telegram"))
        }
        "disable-discord" => {
            for key in DISCORD_KEYS {
                values.remove(key);
            }
            set_route(&mut values, "discord", false);
            let result = save_and_report(&values, None);
            if result == 0 {
                println!("discord credentials removed and route disabled");
            }
            result
        }
        "disable-telegram" => {
            for key in TELEGRAM_KEYS {
                values.remove(key);
            }
            set_route(&mut values, "telegram", false);
            let result = save_and_report(&values, None);
            if result == 0 {
                println!("telegram credentials removed and route disabled");
            }
            result
        }
        _ => {
            eprintln!("unknown notifications subcommand: {action}\n{}", usage());
            2
        }
    }
}

fn telegram_discovery_error(error: &str) -> i32 {
    eprintln!("Telegram chat discovery failed: {error}");
    if error.contains("HTTP 401") {
        eprintln!("The token was rejected. Copy the current token from @BotFather and try again.");
    } else if error.contains("HTTP 409") {
        eprintln!(
            "This bot already has a webhook or another getUpdates client. Use a dedicated bot for Memory Supervisor; no existing webhook was changed."
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
    if let Ok(value) = fs::read_to_string(home.join(".memory-supervisor").join("install-root")) {
        let value = value.trim();
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    home.join(".local").join("share").join("memory-supervisor")
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

    let url = env::var("MEMORY_SUPERVISOR_BOOTSTRAP_URL").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh".to_owned()
    });
    if cfg!(windows) {
        let windows_url = env::var("MEMORY_SUPERVISOR_BOOTSTRAP_URL").unwrap_or_else(|_| {
            "https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1".to_owned()
        });
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "$ErrorActionPreference='Stop'; Invoke-Expression (Invoke-RestMethod -Uri $env:MEMORY_SUPERVISOR_BOOTSTRAP_URL)",
            ])
            .env("MEMORY_SUPERVISOR_BOOTSTRAP_URL", windows_url)
            .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
            .status()
    } else {
        Command::new("/bin/sh")
            .args([
                "-c",
                "set -eu; source=$(curl --proto '=https' --tlsv1.2 -fsSL \"$MEMORY_SUPERVISOR_BOOTSTRAP_URL\"); [ -n \"$source\" ]; printf '%s\\n' \"$source\" | /bin/sh",
            ])
            .env("MEMORY_SUPERVISOR_BOOTSTRAP_URL", url)
            .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root)
            .status()
    }
}

fn maintain_installation(action: &str) -> i32 {
    let root = installation_root();
    let mut update_status = 0;
    if action == "update" {
        println!("Updating Memory Supervisor source...");
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
            "Removing Memory Supervisor while preserving state and notification settings..."
        } else {
            "Reapplying the service and all detected CLI connections..."
        }
    );
    let status = if cfg!(windows) {
        Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .status()
    } else {
        Command::new("/bin/sh").arg(script).status()
    };
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
            "cannot turn off while Memory Supervisor owns paused PID(s): {}; resume or terminate them first",
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

fn maintain_power(action: &str) -> i32 {
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
        eprintln!(
            "hard-cap setting was saved but service reload failed; run `memory-supervisor update`"
        );
    }
    status
}

fn usage() -> &'static str {
    "Memory Supervisor control\n\nUSAGE:\n  memory-supervisor on\n  memory-supervisor off\n  memory-supervisor resume [PID] [--timeout SECONDS]\n  memory-supervisor terminate PID [--timeout SECONDS]\n  memory-supervisor kill PID [--timeout SECONDS]\n  memory-supervisor budget\n  memory-supervisor budget set <GiB | N MB> [--yes]\n  memory-supervisor budget off\n  memory-supervisor hard-cap <show|set MB|off>\n  memory-supervisor notifications <show|routes ROUTES|discord-webhook|discord-channel ID|discord-dm ID|telegram [CHAT_ID]|disable-discord|disable-telegram|test>\n  memory-supervisor update\n  memory-supervisor uninstall"
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
    own_instance: String,
    own_capacity_mb: f64,
    physical_mb: f64,
    physical_source: String,
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

    fn theoretical_mb(&self) -> f64 {
        self.own_capacity_mb.min(self.physical_mb)
    }

    fn current_mb(&self) -> f64 {
        (self.physical_mb - self.other_claims_mb())
            .min(self.own_capacity_mb)
            .max(0.0)
    }
}

enum BudgetGate {
    Apply,
    Confirm { percent: f64, machine_percent: f64 },
    TooHigh { beyond_theoretical: bool },
}

fn budget_gate(requested_mb: f64, view: &BudgetView) -> BudgetGate {
    let current = view.current_mb();
    if requested_mb > current {
        return BudgetGate::TooHigh {
            beyond_theoretical: requested_mb > view.theoretical_mb(),
        };
    }
    // Two attention lines feed one confirmation: taking 90%+ of what is still possible in this
    // kernel, or the machine-wide explicit-budget total reaching 90%+ of the physical estimate.
    // The second closes the gradual case where several kernels each stay below 90% locally while
    // together committing nearly the whole machine.
    let machine_after_mb = view.other_claims_mb() + requested_mb;
    let machine_percent = 100.0 * machine_after_mb / view.physical_mb;
    if requested_mb >= 0.9 * current || machine_after_mb >= 0.9 * view.physical_mb {
        return BudgetGate::Confirm {
            percent: 100.0 * requested_mb / current,
            machine_percent,
        };
    }
    BudgetGate::Apply
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
         `memory-supervisor update` (unvalidated MB setting remains available with \
         `memory-supervisor hard-cap set <MB>`)",
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
    let (physical_mb, physical_source) = peers
        .iter()
        .filter_map(|peer| Some((peer.capacity_mb?, peer.instance.as_str())))
        .chain([(own_capacity_mb, own_instance.as_str())])
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(capacity, instance)| (capacity, instance.to_owned()))
        .unwrap_or((own_capacity_mb, own_instance.clone()));
    Ok(BudgetView {
        own_instance,
        own_capacity_mb,
        physical_mb,
        physical_source,
        peers,
        own_cap_mb,
    })
}

fn show_budget(view: &BudgetView) -> i32 {
    println!("CLI memory budget — kernel {}", view.own_instance);
    println!("  this kernel capacity: {}", mb_gib(view.own_capacity_mb));
    println!(
        "  physical machine estimate: {} (largest known kernel: {})",
        mb_gib(view.physical_mb),
        view.physical_source
    );
    if view.peers.is_empty() {
        println!(
            "  other kernels: none detected in the federation directory; install the supervisor \
             in every kernel (host, WSL, VM, isolated container) so budgets share one physical \
             total"
        );
    } else {
        println!("  other kernels' explicit budgets:");
        for peer in &view.peers {
            let budget = peer
                .cap_mb
                .map(mb_gib)
                .unwrap_or_else(|| "OFF (default kernel allocation is not a claim)".to_owned());
            let capacity = peer
                .capacity_mb
                .map(|value| format!(", capacity {}", mb_gib(value)))
                .unwrap_or_default();
            println!(
                "    {}: {budget}{capacity}{}",
                peer.instance,
                peer.stale_note()
            );
        }
    }
    println!("  budgeted elsewhere: {}", mb_gib(view.other_claims_mb()));
    println!(
        "  theoretical maximum here (if no other kernel kept a budget): {}",
        mb_gib(view.theoretical_mb())
    );
    println!("  currently possible here: {}", mb_gib(view.current_mb()));
    println!(
        "  this kernel's budget: {}",
        view.own_cap_mb
            .map(mb_gib)
            .unwrap_or_else(|| "OFF (adaptive policy only)".to_owned())
    );
    let total = view.other_claims_mb() + view.own_cap_mb.unwrap_or_default();
    println!(
        "  explicit budgets across kernels: {} of the {} physical estimate ({:.0}%)",
        mb_gib(total),
        mb_gib(view.physical_mb),
        100.0 * total / view.physical_mb
    );
    if total > view.physical_mb {
        println!(
            "  warning: explicit budgets across kernels total {} and exceed the physical machine \
             estimate {}; reduce one so the machine cannot be over-committed",
            mb_gib(total),
            mb_gib(view.physical_mb)
        );
    }
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

fn budget(arguments: &[String]) -> i32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    match arguments.first().map(String::as_str) {
        None | Some("show") => match collect_budget_view(now) {
            Ok(view) => show_budget(&view),
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        Some("off") => hard_cap(&["off".to_owned()]),
        Some("set") => {
            let mut assume_yes = false;
            let mut value = None;
            for argument in &arguments[1..] {
                if argument == "--yes" {
                    assume_yes = true;
                } else if value.is_none() {
                    value = Some(argument.clone());
                } else {
                    eprintln!("budget set accepts one amount\n{}", usage());
                    return 2;
                }
            }
            let Some(raw) = value else {
                eprintln!("budget set requires an amount in GiB (or with an MB/GB suffix)");
                return 2;
            };
            let requested_mb = match parse_budget_amount(&raw) {
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
            let current = view.current_mb();
            match budget_gate(requested_mb, &view) {
                BudgetGate::TooHigh { beyond_theoretical } => {
                    eprintln!(
                        "requested {} exceeds what is currently possible in this kernel: {}",
                        mb_gib(requested_mb),
                        mb_gib(current)
                    );
                    eprintln!(
                        "  physical machine estimate {}; this kernel capacity {}",
                        mb_gib(view.physical_mb),
                        mb_gib(view.own_capacity_mb)
                    );
                    let mut stale_claim = false;
                    for peer in &view.peers {
                        if let Some(cap) = peer.cap_mb {
                            stale_claim |= peer.stale();
                            eprintln!(
                                "  {}: {} explicit budget{}",
                                peer.instance,
                                mb_gib(cap),
                                peer.stale_note()
                            );
                        }
                    }
                    if stale_claim {
                        eprintln!(
                            "  a stale snapshot contributes to this total; if that kernel is gone, remove its old file with `memory-status --all --prune-stale-hours 24`"
                        );
                    }
                    if beyond_theoretical {
                        eprintln!(
                            "even with every other kernel's budget removed, the maximum possible \
                             here is {} (bounded by this kernel's capacity within the physical \
                             machine)",
                            mb_gib(view.theoretical_mb())
                        );
                    } else {
                        eprintln!(
                            "to budget {} here, reduce other kernels' budgets by at least {}",
                            mb_gib(requested_mb),
                            mb_gib(requested_mb - current)
                        );
                    }
                    eprintln!("nothing changed");
                    1
                }
                BudgetGate::Confirm {
                    percent,
                    machine_percent,
                } if !assume_yes => {
                    eprint!(
                        "requested {} is {percent:.0}% of the currently possible {}; explicit \
                         budgets across kernels would total {} of the {} physical estimate \
                         ({machine_percent:.0}%). Continue? [y/N]: ",
                        mb_gib(requested_mb),
                        mb_gib(current),
                        mb_gib(view.other_claims_mb() + requested_mb),
                        mb_gib(view.physical_mb)
                    );
                    if !confirm_on_stdin() {
                        eprintln!("nothing changed");
                        return 1;
                    }
                    apply_budget(requested_mb, &view)
                }
                _ => apply_budget(requested_mb, &view),
            }
        }
        Some(other) => {
            eprintln!("unknown budget subcommand: {other}\n{}", usage());
            2
        }
    }
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

fn parse_process(arguments: &[String], action: &str) -> Result<(u32, f64), String> {
    let mut pid = None;
    let mut timeout: f64 = 5.0;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--timeout" {
            timeout = arguments
                .get(index + 1)
                .ok_or_else(|| "--timeout requires SECONDS".to_owned())?
                .parse()
                .map_err(|_| "timeout must be a number".to_owned())?;
            index += 2;
        } else if arguments[index].starts_with('-') {
            return Err(format!("unknown option: {}", arguments[index]));
        } else if pid.is_none() {
            pid = Some(
                arguments[index]
                    .parse::<u32>()
                    .map_err(|_| "PID must be an integer > 1".to_owned())?,
            );
            index += 1;
        } else {
            return Err("only one PID is accepted".to_owned());
        }
    }
    if !(0.1..=300.0).contains(&timeout) || !timeout.is_finite() {
        return Err("timeout must be between 0.1 and 300 seconds".to_owned());
    }
    let pid = match pid {
        Some(pid) => pid,
        None if action == "resume" => resolve_single_stopped_pid(&state_dir())?,
        None => return Err("PID may be omitted only for resume".to_owned()),
    };
    if pid <= 1 {
        return Err("PID must be an integer > 1".to_owned());
    }
    Ok((pid, timeout))
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
        "notifications" => configure_notifications(&arguments[1..]),
        "budget" => budget(&arguments[1..]),
        "hard-cap" => hard_cap(&arguments[1..]),
        "on" | "off" if arguments.len() == 1 => maintain_power(action),
        "on" | "off" => {
            eprintln!("{action} accepts no arguments\n{}", usage());
            2
        }
        "update" | "uninstall" => maintain_installation(action),
        "resume" | "terminate" | "kill" => match parse_process(&arguments[1..], action) {
            Ok((pid, timeout)) => submit_control(action, pid, timeout),
            Err(error) => {
                eprintln!("{error}\n{}", usage());
                2
            }
        },
        _ => {
            eprintln!("unknown action: {action}\n{}", usage());
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
    fn mandatory_notification_routes_cannot_be_disabled_or_configured() {
        assert!(parse_routes("hook,os").unwrap_err().contains("mandatory"));
        let mut values = BTreeMap::from([(
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(),
            "none".to_owned(),
        )]);
        set_route(&mut values, "discord", true);
        assert_eq!(values["MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS"], "discord");
        assert!(notification_channels(&values).contains("hook"));
        assert!(notification_channels(&values).contains("terminal"));
    }

    #[test]
    fn resume_without_pid_selects_only_one_paused_process() {
        let root = env::temp_dir().join(format!(
            "memory-supervisor-control-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("state.json"), br#"{"stopped_pids":[42]}"#).unwrap();
        assert_eq!(resolve_single_stopped_pid(&root).unwrap(), 42);
        fs::write(root.join("state.json"), br#"{"stopped_pids":[42,43]}"#).unwrap();
        let error = resolve_single_stopped_pid(&root).unwrap_err();
        assert!(error.contains("memory-supervisor resume 42"));
        assert!(error.contains("memory-supervisor resume 43"));
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
            own_instance: "wsl".to_owned(),
            own_capacity_mb: own_capacity,
            physical_mb,
            physical_source: "host".to_owned(),
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
        assert_eq!(unconstrained.theoretical_mb(), 7941.0);
        assert_eq!(unconstrained.current_mb(), 7941.0);

        let host_claimed = view(7941.0, Some(16108.0), &[12288.0]);
        assert_eq!(host_claimed.theoretical_mb(), 7941.0);
        assert_eq!(host_claimed.current_mb(), 16108.0 - 12288.0);

        let small_claims = view(7941.0, Some(16108.0), &[4000.0, 2000.0]);
        assert_eq!(small_claims.current_mb(), 7941.0);

        let over_committed = view(7941.0, Some(16108.0), &[17000.0]);
        assert_eq!(over_committed.current_mb(), 0.0);

        let alone = view(7941.0, None, &[]);
        assert_eq!(alone.physical_mb, 7941.0);
        assert_eq!(alone.current_mb(), 7941.0);
    }

    #[test]
    fn budget_gate_orders_error_confirmation_and_plain_application() {
        let constrained = view(7941.0, Some(16108.0), &[12288.0]);
        let current = constrained.current_mb();
        assert!(matches!(
            budget_gate(current + 1.0, &constrained),
            BudgetGate::TooHigh {
                beyond_theoretical: false
            }
        ));
        assert!(matches!(
            budget_gate(8000.0, &constrained),
            BudgetGate::TooHigh {
                beyond_theoretical: true
            }
        ));
        assert!(matches!(
            budget_gate(current, &constrained),
            BudgetGate::Confirm { .. }
        ));
        assert!(matches!(
            budget_gate(0.9 * current, &constrained),
            BudgetGate::Confirm { .. }
        ));
        // Locally below 90%, but the machine-wide explicit total crosses 90% — still confirms.
        assert!(matches!(
            budget_gate(0.9 * current - 1.0, &constrained),
            BudgetGate::Confirm { .. }
        ));
        assert!(matches!(
            budget_gate(1000.0, &constrained),
            BudgetGate::Apply
        ));
        let over_committed = view(7941.0, Some(16108.0), &[17000.0]);
        assert!(matches!(
            budget_gate(1.0, &over_committed),
            BudgetGate::TooHigh { .. }
        ));
    }

    #[test]
    fn machine_level_commitment_also_requires_confirmation() {
        let host = view(16108.0, None, &[8000.0]);
        assert!(matches!(budget_gate(6000.0, &host), BudgetGate::Apply));
        match budget_gate(6600.0, &host) {
            BudgetGate::Confirm {
                percent,
                machine_percent,
            } => {
                assert!(percent < 90.0);
                assert!(machine_percent >= 90.0);
            }
            _ => panic!("expected machine-level confirmation"),
        }
    }

    #[test]
    fn route_parser_is_canonical_and_process_arguments_are_bounded() {
        assert_eq!(parse_routes("telegram,os").unwrap(), "os,telegram");
        assert_eq!(parse_routes("all").unwrap(), "all");
        assert_eq!(parse_routes("off").unwrap(), "none");
        assert!(
            parse_routes("unknown")
                .unwrap_err()
                .contains("invalid route")
        );
        assert_eq!(
            parse_process(
                &["42".to_owned(), "--timeout".to_owned(), "0.1".to_owned()],
                "resume"
            )
            .unwrap(),
            (42, 0.1)
        );
        assert!(parse_process(&["1".to_owned()], "resume").is_err());
        assert!(
            parse_process(
                &["42".to_owned(), "--timeout".to_owned(), "301".to_owned()],
                "resume"
            )
            .is_err()
        );
        assert!(parse_process(&[], "kill").is_err());
    }
}
