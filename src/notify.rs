use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{
    load_notification_config, notification_channels, notification_config_path, state_dir,
};
use crate::events::{acknowledge_event, event_should_notify, user_message};
use crate::storage::{ensure_private_dir, write_atomic_json, write_atomic_text};

const USER_AGENT: &str = "claude-codex-memory-supervisor";
const TITLE: &str = "Claude Code & Codex CLI Memory Supervisor";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DesktopResult {
    pub status: String,
    pub route: String,
}

fn command_status(command: &mut Command, timeout: Duration) -> io::Result<bool> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "notification command timed out",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn windows_notification(message: &str) -> String {
    let script = concat!(
        "Add-Type -AssemblyName System.Windows.Forms;",
        "[System.Windows.Forms.NotifyIcon]$n=New-Object System.Windows.Forms.NotifyIcon;",
        "$n.Icon=[System.Drawing.SystemIcons]::Warning;",
        "$n.BalloonTipTitle='Claude Code & Codex CLI Memory Supervisor';",
        "$n.BalloonTipText=$env:MEMORY_SUPERVISOR_MESSAGE;",
        "$n.Visible=$true;$n.ShowBalloonTip(5000);Start-Sleep -Seconds 6;$n.Dispose()"
    );
    for executable in ["powershell.exe", "powershell", "pwsh"] {
        let mut command = Command::new(executable);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("MEMORY_SUPERVISOR_MESSAGE", message);
        match command_status(&mut command, Duration::from_secs(15)) {
            Ok(success) => {
                return if success { "delivered" } else { "failed" }.to_owned();
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return "failed".to_owned(),
        }
    }
    "unavailable".to_owned()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_wsl() -> bool {
    fs::read_to_string("/proc/version")
        .is_ok_and(|value| value.to_lowercase().contains("microsoft"))
}

pub fn notify_desktop_result(message: &str) -> DesktopResult {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("osascript");
        command.args([
            "-e",
            "on run argv",
            "-e",
            "display notification (item 1 of argv) with title \"Claude Code & Codex CLI Memory Supervisor\"",
            "-e",
            "end run",
            message,
        ]);
        DesktopResult {
            status: if command_status(&mut command, Duration::from_secs(8)).unwrap_or(false) {
                "delivered"
            } else {
                "failed"
            }
            .to_owned(),
            route: "macos-notification".to_owned(),
        }
    }
    #[cfg(windows)]
    {
        DesktopResult {
            status: windows_notification(message),
            route: "windows-balloon".to_owned(),
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut delivered = Vec::new();
        let mut failed = Vec::new();
        if is_wsl() {
            let route = "wsl-host-windows-balloon";
            match windows_notification(message).as_str() {
                "delivered" => delivered.push(route),
                "failed" => failed.push(route),
                _ => {}
            }
        }
        if env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some() {
            let route = "linux-desktop-user-bus";
            let mut command = Command::new("notify-send");
            command.args(["-u", "critical", TITLE, message]);
            match command_status(&mut command, Duration::from_secs(8)) {
                Ok(true) => delivered.push(route),
                Ok(_) => failed.push(route),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => failed.push(route),
            }
        }
        if !delivered.is_empty() {
            DesktopResult {
                status: "delivered".to_owned(),
                route: delivered.join(","),
            }
        } else if !failed.is_empty() {
            DesktopResult {
                status: "failed".to_owned(),
                route: failed.join(","),
            }
        } else {
            DesktopResult {
                status: "unavailable".to_owned(),
                route: "no-os-notification-route".to_owned(),
            }
        }
    }
}

fn curl_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn curl_config(
    method: &str,
    url: &str,
    payload: Option<&Value>,
    authorization: Option<&str>,
    timeout_seconds: u64,
) -> Result<String, String> {
    if !url.starts_with("https://") || url.contains(['\r', '\n']) {
        return Err("invalid HTTPS endpoint".to_owned());
    }
    let mut config = format!(
        "silent\nshow-error\nrequest = \"{}\"\nurl = \"{}\"\nheader = \"User-Agent: {}\"\nmax-time = {}\nconnect-timeout = 5\nwrite-out = \"\\n__MEMORY_SUPERVISOR_HTTP_STATUS__:%{{http_code}}\"\n",
        curl_quote(method),
        curl_quote(url),
        USER_AGENT,
        timeout_seconds.max(1),
    );
    if let Some(authorization) = authorization {
        if authorization.contains(['\r', '\n']) {
            return Err("invalid authorization value".to_owned());
        }
        config.push_str(&format!(
            "header = \"Authorization: {}\"\n",
            curl_quote(authorization)
        ));
    }
    if let Some(payload) = payload {
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!(
            "data = \"{}\"\n",
            curl_quote(&serde_json::to_string(payload).map_err(|error| error.to_string())?)
        ));
    }
    Ok(config)
}

fn parse_http_response(response: &str) -> Result<(u16, Value), String> {
    let marker = "\n__MEMORY_SUPERVISOR_HTTP_STATUS__:";
    let (body_text, status_text) = response
        .rsplit_once(marker)
        .ok_or_else(|| "connection failed or returned no HTTP status".to_owned())?;
    let status = status_text
        .trim()
        .parse::<u16>()
        .map_err(|_| "connection returned an invalid HTTP status".to_owned())?;
    let body = serde_json::from_str(body_text).unwrap_or(Value::Null);
    Ok((status, body))
}

fn request_json(
    method: &str,
    url: &str,
    payload: Option<&Value>,
    authorization: Option<&str>,
    timeout_seconds: u64,
) -> Result<(u16, Value), String> {
    let config = curl_config(method, url, payload, authorization, timeout_seconds)?;
    let mut child = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                "curl is unavailable; reinstall Memory Supervisor to repair prerequisites"
                    .to_owned()
            } else {
                "connection process could not start".to_owned()
            }
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| "connection input unavailable".to_owned())?
        .write_all(config.as_bytes())
        .map_err(|_| "connection input failed".to_owned())?;
    let output = child
        .wait_with_output()
        .map_err(|_| "connection failed or timed out".to_owned())?;
    let response = String::from_utf8_lossy(&output.stdout);
    parse_http_response(&response)
}

fn discord_cache_path() -> PathBuf {
    notification_config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".dm_channel")
}

pub fn notify_discord(message: &str, config: &BTreeMap<String, String>) -> String {
    let webhook = config
        .get("MEMORY_SUPERVISOR_DISCORD_WEBHOOK")
        .map(String::as_str)
        .unwrap_or_default();
    let token = config
        .get("MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN")
        .map(String::as_str)
        .unwrap_or_default();
    let mut channel = config
        .get("MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID")
        .cloned()
        .unwrap_or_default();
    let owner = config
        .get("MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID")
        .map(String::as_str)
        .unwrap_or_default();
    let (url, authorization) = if !webhook.is_empty() {
        if !webhook.starts_with("https://discord.com/api/webhooks/") {
            return "failed".to_owned();
        }
        (webhook.to_owned(), None)
    } else if !token.is_empty() && channel.is_empty() && !owner.is_empty() {
        channel = fs::read_to_string(discord_cache_path())
            .unwrap_or_default()
            .trim()
            .to_owned();
        if channel.is_empty() {
            let authorization = format!("Bot {token}");
            let Ok((status, body)) = request_json(
                "POST",
                "https://discord.com/api/v10/users/@me/channels",
                Some(&json!({"recipient_id": owner})),
                Some(&authorization),
                8,
            ) else {
                return "failed".to_owned();
            };
            if !(200..300).contains(&status) {
                return "failed".to_owned();
            }
            channel = body
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if channel.is_empty()
                || ensure_private_dir(
                    discord_cache_path()
                        .parent()
                        .unwrap_or_else(|| Path::new(".")),
                )
                .is_err()
                || write_atomic_text(&discord_cache_path(), &channel, 0o600).is_err()
            {
                return "failed".to_owned();
            }
        }
        (
            format!("https://discord.com/api/v10/channels/{channel}/messages"),
            Some(format!("Bot {token}")),
        )
    } else if !token.is_empty() && !channel.is_empty() {
        (
            format!("https://discord.com/api/v10/channels/{channel}/messages"),
            Some(format!("Bot {token}")),
        )
    } else {
        return "skipped".to_owned();
    };
    let payload = json!({
        "content": format!("**{TITLE}**\n{}", message.chars().take(1800).collect::<String>())
    });
    match request_json("POST", &url, Some(&payload), authorization.as_deref(), 8) {
        Ok((status, _)) if (200..300).contains(&status) => "delivered",
        _ => "failed",
    }
    .to_owned()
}

fn valid_telegram_token(token: &str) -> bool {
    let Some((prefix, secret)) = token.split_once(':') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.bytes().all(|byte| byte.is_ascii_digit())
        && secret.len() >= 16
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn notify_telegram(message: &str, config: &BTreeMap<String, String>) -> String {
    let token = config
        .get("MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN")
        .map(String::as_str)
        .unwrap_or_default();
    let chat_id = config
        .get("MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID")
        .map(String::as_str)
        .unwrap_or_default();
    if token.is_empty() || chat_id.is_empty() {
        return "skipped".to_owned();
    }
    if !valid_telegram_token(token) {
        return "failed".to_owned();
    }
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let payload = json!({
        "chat_id": chat_id,
        "text": format!("{TITLE}\n{}", message.chars().take(3900).collect::<String>()),
    });
    match request_json("POST", &url, Some(&payload), None, 8) {
        Ok((status, body)) if (200..300).contains(&status) && body["ok"] == Value::Bool(true) => {
            "delivered"
        }
        _ => "failed",
    }
    .to_owned()
}

fn safe_telegram_error(status: u16, body: &Value) -> String {
    let description = body
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("request rejected")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect::<String>();
    format!("Telegram API HTTP {status}: {description}")
}

pub fn discover_telegram_chats(
    token: &str,
    wait_seconds: u64,
) -> Result<Vec<(String, String)>, String> {
    if wait_seconds > 120 {
        return Err("wait_seconds must be between 0 and 120".to_owned());
    }
    if !valid_telegram_token(token) {
        return Err("Telegram bot token format is invalid".to_owned());
    }
    let query = if wait_seconds == 0 {
        String::new()
    } else {
        format!("?timeout={wait_seconds}")
    };
    let url = format!("https://api.telegram.org/bot{token}/getUpdates{query}");
    let (status, body) =
        request_json("GET", &url, None, None, wait_seconds.max(3) + 5).map_err(|_| {
            "Telegram API connection failed or timed out; check internet access and retry"
                .to_owned()
        })?;
    if !(200..300).contains(&status) || body.get("ok") != Some(&Value::Bool(true)) {
        return Err(safe_telegram_error(status, &body));
    }
    telegram_chats(&body)
}

fn telegram_chats(body: &Value) -> Result<Vec<(String, String)>, String> {
    let results = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| "Telegram API returned an unexpected update list".to_owned())?;
    let mut chats = BTreeMap::new();
    for update in results {
        for field in [
            "message",
            "edited_message",
            "channel_post",
            "my_chat_member",
        ] {
            let Some(chat) = update.get(field).and_then(|event| event.get("chat")) else {
                continue;
            };
            let Some(id) = chat.get("id").and_then(|id| match id {
                Value::Number(value) => Some(value.to_string()),
                Value::String(value) => Some(value.clone()),
                _ => None,
            }) else {
                continue;
            };
            let label = ["title", "username", "first_name", "type"]
                .into_iter()
                .find_map(|key| chat.get(key).and_then(Value::as_str))
                .unwrap_or("chat")
                .replace(['\n', '\r'], " ");
            chats.insert(id, label);
        }
    }
    Ok(chats.into_iter().collect())
}

pub fn dispatch_event(
    event: &Value,
    config: &BTreeMap<String, String>,
) -> (BTreeMap<String, String>, Map<String, Value>) {
    let selected = notification_channels(config);
    let should_notify = event_should_notify(event);
    let message = user_message(event);
    let desktop = if !should_notify {
        DesktopResult {
            status: "skipped".to_owned(),
            route: "action-only".to_owned(),
        }
    } else if selected.contains("os") {
        notify_desktop_result(&message)
    } else {
        DesktopResult {
            status: "skipped".to_owned(),
            route: "disabled-by-config".to_owned(),
        }
    };
    let deliveries = BTreeMap::from([
        ("os".to_owned(), desktop.status.clone()),
        (
            "discord".to_owned(),
            if should_notify && selected.contains("discord") {
                notify_discord(&message, config)
            } else {
                "skipped".to_owned()
            },
        ),
        (
            "telegram".to_owned(),
            if should_notify && selected.contains("telegram") {
                notify_telegram(&message, config)
            } else {
                "skipped".to_owned()
            },
        ),
    ]);
    (
        deliveries,
        Map::from_iter([("os_route".to_owned(), Value::String(desktop.route))]),
    )
}

fn spawn_reaped(command: &mut Command) -> io::Result<u32> {
    let (sender, receiver) = mpsc::sync_channel::<std::process::Child>(1);
    thread::Builder::new()
        .name("memory-notify-reaper".to_owned())
        .stack_size(64 * 1024)
        .spawn(move || {
            if let Ok(mut child) = receiver.recv() {
                let _ = child.wait();
            }
        })?;
    let child = command.spawn()?;
    let pid = child.id();
    if let Err(error) = sender.send(child) {
        let mut child = error.0;
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("notification reaper stopped unexpectedly"));
    }
    Ok(pid)
}

pub fn spawn_dispatcher(event: &Value, result_path: &Path) -> io::Result<()> {
    let executable = env::current_exe()?;
    let serialized = serde_json::to_string(event).map_err(io::Error::other)?;
    let mut command = Command::new(executable);
    command
        .arg("notify")
        .arg("--event-json")
        .arg(serialized)
        .arg("--result")
        .arg(result_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    spawn_reaped(&mut command).map(|_| ())
}

fn parse_event(arguments: &[OsString]) -> Result<(Value, Option<PathBuf>), String> {
    let mut event = Value::Object(Map::new());
    let mut result = None;
    let mut message = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index]
            .to_str()
            .ok_or_else(|| "notify arguments must be valid Unicode".to_owned())?;
        match argument {
            "--event-json" | "--event-file" | "--result" => {
                let value = arguments
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| format!("{argument} requires a value"))?;
                match argument {
                    "--event-json" => {
                        event = serde_json::from_str(value)
                            .map_err(|error| format!("invalid event JSON: {error}"))?;
                    }
                    "--event-file" => {
                        event = serde_json::from_slice(
                            &fs::read(value)
                                .map_err(|error| format!("cannot read event file: {error}"))?,
                        )
                        .map_err(|error| format!("invalid event file: {error}"))?;
                    }
                    "--result" => result = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--help" | "-h" => {
                return Err(
                    "USAGE: memory-supervisor notify [--event-json JSON|--event-file PATH] [--result PATH] [MESSAGE]"
                        .to_owned(),
                );
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            value => {
                if message.is_some() {
                    return Err("only one notification message is accepted".to_owned());
                }
                message = Some(value.to_owned());
                index += 1;
            }
        }
    }
    if !event.is_object() {
        event = Value::Object(Map::new());
    }
    if event.get("message").and_then(Value::as_str).is_none() {
        event.as_object_mut().unwrap().insert(
            "message".to_owned(),
            Value::String(
                message.unwrap_or_else(|| {
                    "Claude Code & Codex CLI Memory Supervisor alert".to_owned()
                }),
            ),
        );
    }
    Ok((event, result))
}

pub fn run_notify(arguments: &[OsString]) -> i32 {
    let (event, result_path) = match parse_event(arguments) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return if error.starts_with("USAGE:") { 0 } else { 2 };
        }
    };
    let config = load_notification_config(&notification_config_path());
    let (deliveries, details) = dispatch_event(&event, &config);
    if let Some(event_id) = event.get("event_id").and_then(Value::as_str) {
        for (transport, status) in &deliveries {
            let _ = acknowledge_event(&state_dir(), event_id, transport, status, "rust-dispatcher");
        }
    }
    if let Some(path) = result_path {
        let _ = write_atomic_json(
            &path,
            &json!({
                "event_id": event.get("event_id"),
                "deliveries": deliveries,
                "delivery_details": details,
            }),
            0o600,
            true,
        );
    }
    0
}

pub fn configured_optional_routes(config: &BTreeMap<String, String>) -> BTreeSet<String> {
    notification_channels(config)
        .into_iter()
        .filter(|route| matches!(route.as_str(), "os" | "discord" | "telegram"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_token_and_update_parsing_helpers_are_strict() {
        assert!(valid_telegram_token(
            "123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdef-1234"
        ));
        assert!(!valid_telegram_token("not-a-token"));
        assert!(!valid_telegram_token("123:short"));
        assert_eq!(
            discover_telegram_chats("not-a-token", 120).unwrap_err(),
            "Telegram bot token format is invalid"
        );
        assert_eq!(
            discover_telegram_chats("not-a-token", 121).unwrap_err(),
            "wait_seconds must be between 0 and 120"
        );

        let chats = telegram_chats(&json!({
            "result": [
                {"message":{"chat":{"id":-10042,"title":"Build room"}}},
                {"my_chat_member":{"chat":{"id":"7","first_name":"Ada\nLovelace"}}},
                {"edited_message":{"chat":{"id":-10042,"title":"duplicate"}}}
            ]
        }))
        .unwrap();
        assert_eq!(
            chats,
            [
                ("-10042".to_owned(), "duplicate".to_owned()),
                ("7".to_owned(), "Ada Lovelace".to_owned())
            ]
        );
        assert!(telegram_chats(&json!({"result":"bad"})).is_err());
    }

    #[test]
    fn http_request_contract_keeps_headers_payload_and_status_exact() {
        let config = curl_config(
            "POST",
            "https://discord.com/api/v10/channels/42/messages",
            Some(&json!({"content":"line one\n\"line two\""})),
            Some("Bot private-token"),
            8,
        )
        .unwrap();
        assert!(config.contains("User-Agent: claude-codex-memory-supervisor"));
        assert!(config.contains("Authorization: Bot private-token"));
        assert!(config.contains("Content-Type: application/json"));
        assert!(config.contains(r#"line one\\n\\\"line two\\\""#));
        assert!(curl_config("GET", "http://example.com", None, None, 1).is_err());
        assert!(curl_config("GET", "https://example.com", None, Some("bad\nheader"), 1).is_err());

        assert_eq!(
            parse_http_response("{\"ok\":true}\n__MEMORY_SUPERVISOR_HTTP_STATUS__:200").unwrap(),
            (200, json!({"ok":true}))
        );
        assert_eq!(
            parse_http_response("\n__MEMORY_SUPERVISOR_HTTP_STATUS__:204").unwrap(),
            (204, Value::Null)
        );
        assert!(parse_http_response("missing marker").is_err());
    }

    #[test]
    fn action_only_filter_skips_every_optional_transport() {
        let event = json!({
            "type": "utilization-transition",
            "importance": "detail",
            "message": "yellow to orange"
        });
        let config = BTreeMap::from([(
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(),
            "none".to_owned(),
        )]);
        let (deliveries, details) = dispatch_event(&event, &config);
        assert!(deliveries.values().all(|status| status == "skipped"));
        assert_eq!(details["os_route"], "action-only");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detached_notification_children_are_reaped() {
        let mut command = Command::new("/bin/true");
        let pid = spawn_reaped(&mut command).unwrap();
        let process = PathBuf::from(format!("/proc/{pid}"));
        for _ in 0..100 {
            if !process.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("notification child {pid} remained as an unreaped process");
    }
}
