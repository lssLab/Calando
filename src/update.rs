//! Daily update check. Updates apply automatically unless the user turned that off; a release
//! that changes hook definitions is left for the user when Codex is connected, because Codex
//! would silently stop running a changed hook until the user trusts it again in `/hooks`.

use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{Config, config_path, load_notification_config, notification_config_path};
use crate::storage::write_atomic_json;

const LATEST_RELEASE: &str = "https://api.github.com/repos/lssLab/Calando/releases/latest";
const AUTO_KEY: &str = "MEMORY_SUPERVISOR_AUTO_UPDATE";
const CHECK_INTERVAL_S: f64 = 86_400.0;

pub fn auto_enabled(config: &Config) -> bool {
    config.values().get(AUTO_KEY).and_then(Value::as_bool) != Some(false)
}

fn record_path(directory: &Path) -> PathBuf {
    directory.join("update.json")
}

pub fn record(directory: &Path) -> Value {
    fs::read(record_path(directory))
        .ok()
        .and_then(|source| serde_json::from_slice(&source).ok())
        .unwrap_or_else(|| json!({}))
}

fn version_triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.split('-').next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((version_triple(candidate), version_triple(current)), (Some(a), Some(b)) if a > b)
}

/// A minor or major release may change hook definitions; a patch release never does
/// (`tests::hook_definitions_change_only_with_the_minor_version` enforces it).
fn changes_hooks(candidate: &str, current: &str) -> bool {
    match (version_triple(candidate), version_triple(current)) {
        (Some((a, b, _)), Some((c, d, _))) => (a, b) != (c, d),
        _ => true,
    }
}

/// The one line `calando status` and `/calando status` show, or `None` when nothing is pending.
pub fn status_line(directory: &Path) -> Option<String> {
    let record = record(directory);
    let latest = record.get("latest").and_then(Value::as_str)?;
    if !is_newer(latest, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    Some(match record.get("pending").and_then(Value::as_str) {
        Some("auto-off") => {
            format!("Update: {latest} available (auto update is off). Run `calando update`.")
        }
        Some("codex-trust") => format!(
            "Update: {latest} needs `calando update`, then trust the changed hooks in Codex `/hooks`."
        ),
        _ => format!("Update: {latest} will install automatically."),
    })
}

fn latest_version() -> Option<String> {
    let (status, body) = crate::notify::get_json(LATEST_RELEASE, 10).ok()?;
    (200..300).contains(&status).then_some(())?;
    body.get("tag_name")
        .and_then(Value::as_str)
        .map(|tag| tag.trim_start_matches('v').to_owned())
}

fn codex_connected() -> bool {
    let home = env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".codex"))
    });
    home.and_then(|home| fs::read_to_string(home.join("hooks.json")).ok())
        .is_some_and(|hooks| hooks.contains("calando") || hooks.contains("memory-supervisor"))
}

/// Free to update: nothing paused and no memory pressure right now.
fn quiet(directory: &Path) -> bool {
    let Some(state) = fs::read(directory.join("state.json"))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
    else {
        return false;
    };
    state
        .get("stopped_pids")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        && matches!(
            state.get("action").and_then(Value::as_str),
            Some("allow" | "observe")
        )
}

fn telegram_once(record: &mut Value, version: &str, message: &str) {
    if record.get("notified").and_then(Value::as_str) == Some(version) {
        return;
    }
    let config = load_notification_config(&notification_config_path());
    if crate::config::telegram_enabled(&config) {
        crate::notify::notify_telegram(message, &config);
    }
    record["notified"] = json!(version);
}

/// Runs `calando update` where stopping the Calando service cannot kill it.
fn spawn_detached_update() -> bool {
    let Ok(binary) = env::current_exe() else {
        return false;
    };
    let mut command = if cfg!(target_os = "linux")
        && Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "calando.service"])
            .status()
            .is_ok_and(|status| status.success())
    {
        let mut command = Command::new("systemd-run");
        command
            .args(["--user", "--collect", "--quiet"])
            .arg(&binary);
        command
    } else {
        Command::new(&binary)
    };
    command
        .arg("update")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A new session leaves the service's process group, which launchd and the fallback
        // daemon stop as a whole.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, plus CREATE_BREAKAWAY_FROM_JOB when the
        // task's job allows it, so stopping the scheduled task does not end the update.
        command.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0100_0000);
        if command.spawn().is_ok() {
            return true;
        }
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    command.spawn().is_ok()
}

/// Only the installed binary updates itself or repairs hooks; a test or development build must
/// neither reach the network nor rewire the user's CLIs.
fn running_installed_binary() -> bool {
    let installed = fs::read_to_string(crate::config::pointer_dir().join("binary"))
        .ok()
        .and_then(|path| fs::canonicalize(path.trim()).ok());
    installed.is_some()
        && installed
            == env::current_exe()
                .ok()
                .and_then(|exe| fs::canonicalize(exe).ok())
}

/// Called from the daemon each tick; does real work at most once a day.
pub fn daily_check(directory: &Path, now: f64) {
    if !running_installed_binary() {
        return;
    }
    let mut record = record(directory);
    if now
        - record
            .get("checked_at")
            .and_then(Value::as_f64)
            .unwrap_or_default()
        < CHECK_INTERVAL_S
    {
        return;
    }
    record["checked_at"] = json!(now);
    let _ = write_atomic_json(&record_path(directory), &record, 0o600, true);
    let directory = directory.to_path_buf();
    // The network call must never stall the daemon's measurement loop.
    let _ = std::thread::Builder::new()
        .name("calando-update".to_owned())
        .spawn(move || check(&directory, record));
}

/// Claude Code can rewrite settings.json from a stale in-memory copy and drop the hooks; put
/// them back once a day. Codex hooks are not rewritten here: they would need the user's trust.
fn repair_claude_hooks() {
    let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) else {
        return;
    };
    let settings = PathBuf::from(home).join(".claude").join("settings.json");
    let Ok(binary) = env::current_exe() else {
        return;
    };
    if settings.is_file()
        && crate::integration::verify_hooks(&settings, "claude", &binary) == Ok(false)
    {
        let _ = crate::integration::update_hooks(&settings, "claude", &binary, false);
    }
}

fn check(directory: &Path, mut record: Value) {
    repair_claude_hooks();
    let current = env!("CARGO_PKG_VERSION");
    let Some(latest) = latest_version() else {
        return;
    };
    record["latest"] = json!(latest);
    record["pending"] = Value::Null;
    if is_newer(&latest, current) {
        if !auto_enabled(&Config::load(&config_path())) {
            record["pending"] = json!("auto-off");
            telegram_once(
                &mut record,
                &latest,
                &format!(
                    "Update available\nVersion: {latest} (installed {current})\nAction: Auto update is off. Run `calando update`."
                ),
            );
        } else if changes_hooks(&latest, current) && codex_connected() {
            record["pending"] = json!("codex-trust");
            telegram_once(
                &mut record,
                &latest,
                &format!(
                    "Update needs you\nVersion: {latest} (installed {current})\nAction: It changes Codex hooks. Run `calando update`, then trust them in Codex /hooks."
                ),
            );
        } else if quiet(directory) && spawn_detached_update() {
            record["pending"] = json!("installing");
        } else {
            // Busy now; try again in an hour rather than tomorrow.
            record["checked_at"] = json!(
                record["checked_at"].as_f64().unwrap_or_default() - CHECK_INTERVAL_S + 3600.0
            );
        }
    }
    let _ = write_atomic_json(&record_path(directory), &record, 0o600, true);
}

/// `calando update auto on|off`.
pub fn set_auto(enabled: bool) -> Result<(), String> {
    let config = Config::load(&config_path());
    if let Some(error) = config.configuration_error() {
        return Err(format!(
            "configuration is unreadable; nothing changed: {error}"
        ));
    }
    let mut values = config.values().clone();
    if enabled {
        values.remove(AUTO_KEY);
    } else {
        values.insert(AUTO_KEY.to_owned(), Value::Bool(false));
    }
    write_atomic_json(&config_path(), &values, 0o600, true).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_only_minor_changes_touch_hooks() {
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
        assert!(!changes_hooks("0.3.5", "0.3.0"));
        assert!(changes_hooks("0.4.0", "0.3.9"));
    }

    // Hook commands embed the platform's shell form, so one platform holds the reference hash.
    #[cfg(target_os = "linux")]
    #[test]
    fn hook_definitions_change_only_with_the_minor_version() {
        // Changing any hook definition requires a minor bump: update this pair together.
        const HOOKS_FOR: &str = "0.3";
        const HOOKS_SHA256: &str =
            "c5fb7960182683e06482521c232e02197107ff68949cf46ece1746d442a3eaf6";
        let binary = Path::new("/home/owner/.local/lib/calando/calando");
        let mut combined = String::new();
        for (provider, target) in [
            ("claude", "/home/owner/.claude/settings.json"),
            ("codex", "/home/owner/.codex/hooks.json"),
        ] {
            let hooks =
                crate::integration::desired_hooks(provider, binary, Path::new(target)).unwrap();
            combined.push_str(&serde_json::to_string(&hooks).unwrap());
        }
        use sha2::Digest;
        let digest = format!("{:x}", sha2::Sha256::digest(combined.as_bytes()));
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            version.starts_with(HOOKS_FOR),
            "bump HOOKS_FOR with the minor version"
        );
        assert_eq!(
            digest, HOOKS_SHA256,
            "hooks changed: bump the minor version and this hash"
        );
    }
}
