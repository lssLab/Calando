use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::storage::{ensure_private_dir, write_atomic_json, write_atomic_text};

const CLAUDE_MINIMUM: (u64, u64, u64) = (2, 1, 217);
const CODEX_MINIMUM: (u64, u64, u64) = (0, 145, 0);

#[derive(Clone, Debug, Serialize)]
pub struct ClaudeInstallation {
    pub path: PathBuf,
    pub version: Option<String>,
    pub supported: bool,
    pub detail: String,
    pub on_path: bool,
    #[serde(skip)]
    version_parts: Option<(u64, u64, u64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClaudeDiscovery {
    pub selected: Option<ClaudeInstallation>,
    pub installations: Vec<ClaudeInstallation>,
}

impl ClaudeDiscovery {
    pub fn detected(&self) -> bool {
        !self.installations.is_empty()
    }

    pub fn failure_summary(&self) -> String {
        if self.installations.is_empty() {
            return "Claude Code executable was not found".to_owned();
        }
        self.installations
            .iter()
            .map(|candidate| format!("{} ({})", candidate.path.display(), candidate.detail))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

const CODEX_HOOK_SPECS: [(&str, &str, u64, Option<&str>); 7] = [
    ("SessionStart", "session_start", 5, None),
    ("SubagentStart", "subagent_start", 30, Some(".*")),
    ("SubagentStop", "subagent_stop", 5, Some(".*")),
    ("UserPromptSubmit", "user_prompt_submit", 5, None),
    ("Stop", "stop", 5, None),
    ("PreToolUse", "pre_tool_use", 20, Some(".*")),
    ("PostToolUse", "post_tool_use", 15, Some(".*")),
];

fn parse_version(output: &str) -> Option<(u64, u64, u64)> {
    let bytes = output.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit()
            || start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.')
        {
            continue;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let candidate = &output[start..end];
        let parts: Vec<_> = candidate.split('.').collect();
        if parts.len() >= 3
            && let (Ok(major), Ok(minor), Ok(patch)) =
                (parts[0].parse(), parts[1].parse(), parts[2].parse())
        {
            return Some((major, minor, patch));
        }
    }
    None
}

pub fn claude_contract(version_output: &str) -> (bool, String) {
    let Some(version) = parse_version(version_output) else {
        return (false, "could not parse Claude Code version".to_owned());
    };
    let current = format!("{}.{}.{}", version.0, version.1, version.2);
    if version < CLAUDE_MINIMUM {
        (
            false,
            format!("Claude Code {current} is older than 2.1.217"),
        )
    } else {
        (
            true,
            format!("Claude Code {current}: required hooks supported"),
        )
    }
}

pub fn codex_contract(version_output: &str, features_output: &str) -> (bool, String) {
    let Some(version) = parse_version(version_output) else {
        return (false, "could not parse Codex version".to_owned());
    };
    let current = format!("{}.{}.{}", version.0, version.1, version.2);
    if version < CODEX_MINIMUM {
        return (false, format!("Codex {current} is older than 0.145.0"));
    }
    for line in features_output.lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.first() == Some(&"hooks") {
            return if columns.get(1..) == Some(&["stable", "true"][..]) {
                (true, format!("Codex {current}: stable hooks enabled"))
            } else {
                (
                    false,
                    format!(
                        "Codex hooks are not stable and enabled: {}",
                        columns.get(1..).unwrap_or_default().join(" ")
                    ),
                )
            };
        }
    }
    (false, "Codex did not report the hooks feature".to_owned())
}

fn find_command(command: &OsStr) -> PathBuf {
    let path = PathBuf::from(command);
    if path.components().count() > 1 || path.is_file() {
        return path;
    }
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".ps1"]
    } else {
        &[""]
    };
    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            for extension in extensions {
                let candidate = directory.join(format!("{}{extension}", command.to_string_lossy()));
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    path
}

fn provider_command(command: &OsStr, arguments: &[&str]) -> Command {
    let executable = find_command(command);
    let extension = executable
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_lowercase();
    if cfg!(windows) && matches!(extension.as_str(), "cmd" | "bat") {
        let mut invocation =
            Command::new(env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
        invocation
            .args(["/d", "/c"])
            .arg(executable)
            .args(arguments);
        invocation
    } else if cfg!(windows) && extension == "ps1" {
        let mut invocation = Command::new("powershell.exe");
        invocation
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(executable)
            .args(arguments);
        invocation
    } else {
        let mut invocation = Command::new(executable);
        invocation.args(arguments);
        invocation
    }
}

fn output_with_timeout(mut command: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "command timed out"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn inspect_claude_installation(path: &Path, on_path: bool) -> ClaudeInstallation {
    match output_with_timeout(
        provider_command(path.as_os_str(), &["--version"]),
        Duration::from_secs(10),
    ) {
        Ok(output) if output.status.success() => {
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let version_parts = parse_version(&output);
            let (supported, detail) = claude_contract(&output);
            ClaudeInstallation {
                path: path.to_path_buf(),
                version: version_parts
                    .map(|version| format!("{}.{}.{}", version.0, version.1, version.2)),
                supported,
                detail,
                on_path,
                version_parts,
            }
        }
        Ok(output) => ClaudeInstallation {
            path: path.to_path_buf(),
            version: None,
            supported: false,
            detail: format!(
                "Claude Code inspection failed: exit {}",
                output.status.code().unwrap_or(1)
            ),
            on_path,
            version_parts: None,
        },
        Err(error) => ClaudeInstallation {
            path: path.to_path_buf(),
            version: None,
            supported: false,
            detail: format!("Claude Code inspection failed: {error}"),
            on_path,
            version_parts: None,
        },
    }
}

pub fn inspect_claude(command: &OsStr) -> (bool, String) {
    let installation = inspect_claude_installation(Path::new(command), true);
    (installation.supported, installation.detail)
}

fn push_claude_candidate(
    paths: &mut Vec<(PathBuf, bool)>,
    seen: &mut BTreeSet<PathBuf>,
    candidate: PathBuf,
    on_path: bool,
) {
    if !candidate.is_file() {
        return;
    }
    let identity = fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
    if seen.insert(identity) {
        paths.push((candidate, on_path));
    }
}

fn push_named_claude(
    paths: &mut Vec<(PathBuf, bool)>,
    seen: &mut BTreeSet<PathBuf>,
    directory: &Path,
    on_path: bool,
) {
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".ps1"]
    } else {
        &[""]
    };
    for extension in extensions {
        push_claude_candidate(
            paths,
            seen,
            directory.join(format!("claude{extension}")),
            on_path,
        );
    }
}

fn child_directories(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn claude_candidate_paths(home: &Path, search_path: Option<&OsStr>) -> Vec<(PathBuf, bool)> {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(search_path) = search_path {
        for directory in env::split_paths(search_path) {
            push_named_claude(&mut paths, &mut seen, &directory, true);
        }
    }

    for directory in [
        home.join(".local/bin"),
        home.join(".claude/local"),
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        home.join("AppData/Roaming/npm"),
    ] {
        push_named_claude(&mut paths, &mut seen, &directory, false);
    }

    for node in child_directories(&home.join(".nvm/versions/node")) {
        push_named_claude(&mut paths, &mut seen, &node.join("bin"), false);
    }
    for node in child_directories(&home.join(".fnm/node-versions")) {
        push_named_claude(&mut paths, &mut seen, &node.join("installation/bin"), false);
    }
    for version in child_directories(&home.join(".asdf/installs/nodejs")) {
        push_named_claude(&mut paths, &mut seen, &version.join("bin"), false);
    }

    let native_versions = home.join(".local/share/claude/versions");
    if let Ok(entries) = fs::read_dir(native_versions) {
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path
                .file_name()
                .and_then(OsStr::to_str)
                .and_then(parse_version)
                .is_some()
            {
                push_claude_candidate(&mut paths, &mut seen, path, false);
            }
        }
    }

    for variable in ["NVM_SYMLINK", "VOLTA_HOME"] {
        if let Some(root) = env::var_os(variable) {
            let root = PathBuf::from(root);
            let directory = if variable == "VOLTA_HOME" {
                root.join("bin")
            } else {
                root
            };
            push_named_claude(&mut paths, &mut seen, &directory, false);
        }
    }
    if let Some(root) = env::var_os("NVM_DIR") {
        for node in child_directories(&PathBuf::from(root).join("versions/node")) {
            push_named_claude(&mut paths, &mut seen, &node.join("bin"), false);
        }
    }

    paths
}

pub(crate) fn discover_claude_with_path(
    home: &Path,
    search_path: Option<&OsStr>,
) -> ClaudeDiscovery {
    let installations: Vec<_> = claude_candidate_paths(home, search_path)
        .into_iter()
        .map(|(path, on_path)| inspect_claude_installation(&path, on_path))
        .collect();
    let mut selected: Option<&ClaudeInstallation> = None;
    for installation in installations.iter().filter(|candidate| candidate.supported) {
        if selected.is_none_or(|current| installation.version_parts > current.version_parts) {
            selected = Some(installation);
        }
    }
    ClaudeDiscovery {
        selected: selected.cloned(),
        installations,
    }
}

pub fn discover_claude(home: &Path) -> ClaudeDiscovery {
    discover_claude_with_path(home, env::var_os("PATH").as_deref())
}

pub fn inspect_codex(command: &OsStr) -> (bool, String) {
    let version = output_with_timeout(
        provider_command(command, &["--version"]),
        Duration::from_secs(10),
    );
    let features = output_with_timeout(
        provider_command(command, &["features", "list"]),
        Duration::from_secs(10),
    );
    match (version, features) {
        (Ok(version), Ok(features)) if version.status.success() && features.status.success() => {
            codex_contract(
                &String::from_utf8_lossy(&version.stdout),
                &String::from_utf8_lossy(&features.stdout),
            )
        }
        (Ok(version), Ok(features)) => (
            false,
            format!(
                "Codex inspection failed: version exit {}, features exit {}",
                version.status.code().unwrap_or(1),
                features.status.code().unwrap_or(1)
            ),
        ),
        (Err(error), _) | (_, Err(error)) => (false, format!("Codex inspection failed: {error}")),
    }
}

fn is_supervisor_hook(value: &Value) -> bool {
    let Some(hook) = value.as_object() else {
        return false;
    };
    ["command", "commandWindows", "command_windows"]
        .into_iter()
        .filter_map(|key| hook.get(key).and_then(Value::as_str))
        .any(|command| {
            let command = command.to_lowercase().replace('\\', "/");
            command.contains("memory-supervisor") || command.contains("hooks/gate")
        })
}

fn group_is_owned(value: &Value) -> bool {
    value
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_supervisor_hook))
}

fn without_owned_hooks(group: &Value) -> (Option<Value>, bool) {
    let Some(object) = group.as_object() else {
        return (Some(group.clone()), false);
    };
    let Some(hooks) = object.get("hooks").and_then(Value::as_array) else {
        return (Some(group.clone()), false);
    };
    let kept: Vec<_> = hooks
        .iter()
        .filter(|hook| !is_supervisor_hook(hook))
        .cloned()
        .collect();
    if kept.len() == hooks.len() {
        return (Some(group.clone()), false);
    }
    if kept.is_empty() {
        return (None, true);
    }
    let mut updated = object.clone();
    updated.insert("hooks".to_owned(), Value::Array(kept));
    (Some(Value::Object(updated)), true)
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn powershell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

fn codex_route_argument(target: &Path, windows: bool) -> String {
    if windows {
        format!(" --hook-source {}", powershell_quote(target))
    } else {
        format!(" --hook-source {}", shell_quote(target))
    }
}

fn codex_windows_command(binary: &Path, event: &str, target: &Path) -> String {
    // Native Codex runs hooks through the selected Windows shell. Its normal
    // Windows environment is PowerShell, where a quoted executable path is a
    // string expression rather than a command. Use the call operator so paths
    // with spaces execute instead of producing a hook exit-code 1.
    format!(
        "& {} gate codex {event}{}",
        powershell_quote(binary),
        codex_route_argument(target, true)
    )
}

fn command_hook(binary: &Path, provider: &str, event: &str, timeout: u64, target: &Path) -> Value {
    let command_windows = if provider == "codex" {
        if cfg!(windows) {
            codex_windows_command(binary, event, target)
        } else {
            "exit 0".to_owned()
        }
    } else {
        format!("\"{}\" gate {provider} {event}", binary.display())
    };
    let route = if provider == "codex" {
        codex_route_argument(target, false)
    } else {
        String::new()
    };
    let command = if provider == "codex" && cfg!(windows) {
        "true".to_owned()
    } else {
        format!("{} gate {provider} {event}{route}", shell_quote(binary))
    };
    json!({
        "type": "command",
        "command": command,
        "commandWindows": command_windows,
        "timeout": timeout,
    })
}

fn existing_app_route(
    config: &Map<String, Value>,
    event: &str,
) -> (Option<String>, Option<String>) {
    config
        .get("hooks")
        .and_then(Value::as_object)
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .find(|hook| is_supervisor_hook(hook))
        .map(|hook| {
            (
                hook.get("command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                hook.get("commandWindows")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )
        })
        .unwrap_or_default()
}

fn codex_app_command_hook(
    event: &str,
    timeout: u64,
    binary: &Path,
    target: &Path,
    platform: &str,
    existing: (Option<String>, Option<String>),
) -> Result<Value, String> {
    let (mut command, mut command_windows) = existing;
    match platform {
        "windows" => {
            let routed = codex_windows_command(binary, event, target);
            command_windows = Some(routed.clone());
            // `command` is required by the schema. Preserve an existing POSIX route on a shared
            // home; otherwise use a valid no-op so accidental project discovery cannot run a
            // PowerShell command through a POSIX shell.
            command.get_or_insert_with(|| "true".to_owned());
        }
        "linux" | "wsl" | "darwin" | "macos" => {
            command = Some(format!(
                "{} gate codex {event}{}",
                shell_quote(binary),
                codex_route_argument(target, false)
            ));
            command_windows.get_or_insert_with(|| "exit 0".to_owned());
        }
        _ => return Err(format!("unsupported app hook platform: {platform}")),
    }
    let mut hook = Map::from_iter([
        ("type".to_owned(), Value::String("command".to_owned())),
        (
            "command".to_owned(),
            Value::String(command.unwrap_or_default()),
        ),
        ("timeout".to_owned(), json!(timeout)),
    ]);
    if let Some(command_windows) = command_windows {
        hook.insert("commandWindows".to_owned(), Value::String(command_windows));
    }
    Ok(Value::Object(hook))
}

fn desired_codex_app_hooks(
    binary: &Path,
    target: &Path,
    platform: &str,
    existing_config: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut desired = Map::new();
    for (event, _, timeout, matcher) in CODEX_HOOK_SPECS {
        let hook = codex_app_command_hook(
            event,
            timeout,
            binary,
            target,
            platform,
            existing_app_route(existing_config, event),
        )?;
        let mut group = Map::from_iter([("hooks".to_owned(), Value::Array(vec![hook]))]);
        if let Some(matcher) = matcher {
            group.insert("matcher".to_owned(), Value::String(matcher.to_owned()));
        }
        desired.insert(event.to_owned(), Value::Array(vec![Value::Object(group)]));
    }
    Ok(desired)
}

pub fn desired_hooks(
    provider: &str,
    binary: &Path,
    target: &Path,
) -> Result<Map<String, Value>, String> {
    let specs: &[(&str, u64, Option<&str>)] = match provider {
        "claude" => &[
            ("SessionStart", 5, None),
            ("SubagentStart", 30, Some(".*")),
            ("SubagentStop", 5, Some(".*")),
            ("PostToolBatch", 15, None),
            ("UserPromptSubmit", 5, None),
            ("Stop", 5, None),
            ("SessionEnd", 5, Some(".*")),
            ("PreToolUse", 20, Some(".*")),
            ("PostToolUse", 15, Some(".*")),
        ],
        "codex" => &[
            ("SessionStart", 5, None),
            ("SubagentStart", 30, Some(".*")),
            ("SubagentStop", 5, Some(".*")),
            ("UserPromptSubmit", 5, None),
            ("Stop", 5, None),
            ("PreToolUse", 20, Some(".*")),
            ("PostToolUse", 15, Some(".*")),
        ],
        _ => return Err(format!("unsupported provider: {provider}")),
    };
    let mut desired = Map::new();
    for (event, timeout, matcher) in specs {
        let mut group = Map::from_iter([(
            "hooks".to_owned(),
            Value::Array(vec![command_hook(
                binary, provider, event, *timeout, target,
            )]),
        )]);
        if let Some(matcher) = matcher {
            group.insert("matcher".to_owned(), Value::String((*matcher).to_owned()));
        }
        desired.insert(
            (*event).to_owned(),
            Value::Array(vec![Value::Object(group)]),
        );
    }
    Ok(desired)
}

fn load_object(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read(path) {
        Ok(source) => match serde_json::from_slice::<Value>(&source) {
            Ok(Value::Object(value)) => Ok(value),
            Ok(_) => Err(format!(
                "refusing to overwrite non-object JSON: {}",
                path.display()
            )),
            Err(error) => Err(format!(
                "refusing to overwrite malformed JSON {}: {error}",
                path.display()
            )),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(format!(
            "refusing to overwrite unreadable config {}: {error}",
            path.display()
        )),
    }
}

fn resolved_target(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path);
    match metadata {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path).map_err(|_| {
            format!(
                "refusing to create target through dangling config link: {}",
                path.display()
            )
        }),
        _ => Ok(path.to_path_buf()),
    }
}

fn backup(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    fs::copy(
        path,
        path.with_file_name(format!(
            "{}.bak-{timestamp}",
            path.file_name().unwrap_or_default().to_string_lossy()
        )),
    )?;
    Ok(())
}

fn save_object(path: &Path, value: &Map<String, Value>) -> Result<(), String> {
    let destination = resolved_target(path)?;
    let parent = destination
        .parent()
        .ok_or_else(|| "hook config has no parent".to_owned())?;
    let parent_existed = parent.exists();
    if parent_existed {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    } else {
        ensure_private_dir(parent).map_err(|error| error.to_string())?;
    }
    #[cfg(unix)]
    let existing_mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&destination)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o600)
    };
    backup(&destination).map_err(|error| format!("hook backup failed: {error}"))?;
    write_atomic_json(&destination, &Value::Object(value.clone()), 0o600, false)
        .map_err(|error| format!("hook config write failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(existing_mode))
            .map_err(|error| format!("hook mode restore failed: {error}"))?;
    }
    Ok(())
}

pub fn update_hooks(
    path: &Path,
    provider: &str,
    binary: &Path,
    remove: bool,
) -> Result<bool, String> {
    if provider == "codex" {
        // Codex CLI and Desktop App can intentionally share one CODEX_HOME. Use the same locked,
        // platform-aware writer for both surfaces so installing or updating one side never erases
        // the executable route used by the other side.
        return update_codex_app_hooks(path, binary, &crate::platform::platform_name(), remove);
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && fs::metadata(path).is_err()
    {
        return Err(format!(
            "refusing to create target through dangling config link: {}",
            path.display()
        ));
    }
    let mut config = load_object(path)?;
    let original = config.clone();
    if let Some(hooks) = config.get_mut("hooks") {
        let Some(hooks) = hooks.as_object_mut() else {
            return Err(format!(
                "refusing to overwrite non-object hooks in {}",
                path.display()
            ));
        };
        for groups in hooks.values_mut() {
            let Some(groups) = groups.as_array_mut() else {
                continue;
            };
            let mut kept = Vec::new();
            for group in groups.iter() {
                if let (Some(updated), _) = without_owned_hooks(group) {
                    kept.push(updated);
                }
            }
            *groups = kept;
        }
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
    }
    if !remove {
        let desired = desired_hooks(provider, binary, path)?;
        let hooks = config
            .entry("hooks".to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| {
                format!(
                    "refusing to overwrite non-object hooks in {}",
                    path.display()
                )
            })?;
        for (event, desired_groups) in desired {
            let destination = hooks
                .entry(event.clone())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| {
                    format!(
                        "refusing to overwrite non-list hook event {event} in {}",
                        path.display()
                    )
                })?;
            destination.extend(desired_groups.as_array().unwrap().iter().cloned());
        }
    }
    if config
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        config.remove("hooks");
    }
    let changed = config != original;
    if changed {
        save_object(path, &config)?;
    }
    Ok(changed)
}

/// Updates only the native Codex command field for the current OS. The other OS route is retained
/// so Codex CLI and Desktop App can safely share a Windows/WSL CODEX_HOME.
pub fn update_codex_app_hooks(
    path: &Path,
    binary: &Path,
    platform: &str,
    remove: bool,
) -> Result<bool, String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && fs::metadata(path).is_err()
    {
        return Err(format!(
            "refusing to create target through dangling config link: {}",
            path.display()
        ));
    }
    let lock_target = resolved_target(path)?;
    let _lock = AppHookLock::acquire(&lock_target)?;
    let mut config = load_object(path)?;
    let original = config.clone();
    let desired = (!remove).then(|| desired_codex_app_hooks(binary, path, platform, &config));
    if let Some(hooks) = config.get_mut("hooks") {
        let Some(hooks) = hooks.as_object_mut() else {
            return Err(format!(
                "refusing to overwrite non-object hooks in {}",
                path.display()
            ));
        };
        for groups in hooks.values_mut() {
            let Some(groups) = groups.as_array_mut() else {
                continue;
            };
            *groups = groups
                .iter()
                .filter_map(|group| without_owned_hooks(group).0)
                .collect();
        }
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
    }
    if let Some(desired) = desired {
        let desired = desired?;
        let hooks = config
            .entry("hooks".to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| {
                format!(
                    "refusing to overwrite non-object hooks in {}",
                    path.display()
                )
            })?;
        for (event, groups) in desired {
            let destination = hooks
                .entry(event.clone())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| {
                    format!(
                        "refusing to overwrite non-list hook event {event} in {}",
                        path.display()
                    )
                })?;
            destination.extend(groups.as_array().unwrap().iter().cloned());
        }
    }
    if config
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        config.remove("hooks");
    }
    let changed = config != original;
    if changed {
        save_object(path, &config)?;
    }
    Ok(changed)
}

struct AppHookLock {
    path: PathBuf,
    _file: File,
}

impl AppHookLock {
    fn acquire(target: &Path) -> Result<Self, String> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        if parent.exists() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        } else {
            ensure_private_dir(parent).map_err(|error| error.to_string())?;
        }
        let name = target
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("hooks.json");
        let path = parent.join(format!(".{name}.memory-supervisor.lock"));
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(Self { path, _file: file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&path)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_some_and(|age| age >= Duration::from_secs(60));
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() >= Duration::from_secs(2) {
                        return Err(format!(
                            "timed out waiting for shared Codex App hook lock: {}",
                            path.display()
                        ));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    return Err(format!(
                        "could not lock shared Codex App hooks {}: {error}",
                        path.display()
                    ));
                }
            }
        }
    }
}

impl Drop for AppHookLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn verify_codex_app_hooks(path: &Path, binary: &Path, platform: &str) -> Result<bool, String> {
    let config = load_object(path)?;
    let Some(hooks) = config.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };
    let mut found = Map::new();
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        let owned: Vec<_> = groups
            .iter()
            .filter(|group| group_is_owned(group))
            .cloned()
            .collect();
        if !owned.is_empty() {
            found.insert(event.clone(), Value::Array(owned));
        }
    }
    Ok(found == desired_codex_app_hooks(binary, path, platform, &config)?)
}

pub fn verify_hooks(path: &Path, provider: &str, binary: &Path) -> Result<bool, String> {
    if provider == "codex" {
        return verify_codex_app_hooks(path, binary, &crate::platform::platform_name());
    }
    let config = load_object(path)?;
    let Some(hooks) = config.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };
    let mut found = Map::new();
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        let owned: Vec<_> = groups
            .iter()
            .filter(|group| group_is_owned(group))
            .cloned()
            .collect();
        if !owned.is_empty() {
            found.insert(event.clone(), Value::Array(owned));
        }
    }
    Ok(found == desired_hooks(provider, binary, path)?)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexHookHealth {
    pub target: String,
    pub status: String,
    pub definition_valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untrusted_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_enable_state_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexHookSurface {
    Cli,
    App,
}

impl CodexHookHealth {
    pub fn ready(&self) -> bool {
        self.status == "HEALTHY"
    }

    pub fn summary(&self) -> String {
        if self.ready() {
            return "all seven Memory Supervisor hooks are present, enabled, and trusted"
                .to_owned();
        }
        let mut issues = Vec::new();
        for (label, events) in [
            ("missing", &self.missing_events),
            ("stale", &self.stale_events),
            ("duplicated", &self.duplicate_events),
            ("disabled", &self.disabled_events),
            ("not trusted", &self.untrusted_events),
            ("changed since approval", &self.modified_events),
        ] {
            if !events.is_empty() {
                issues.push(format!("{label}: {}", events.join(", ")));
            }
        }
        if let Some(error) = &self.config_error {
            issues.push(format!("config unreadable: {error}"));
        }
        if issues.is_empty() {
            "Codex hook readiness could not be proved".to_owned()
        } else {
            issues.join("; ")
        }
    }

    fn review_action(&self, surface: CodexHookSurface) -> String {
        let disabled = !self.disabled_events.is_empty();
        let needs_trust = !self.untrusted_events.is_empty() || !self.modified_events.is_empty();
        let definitions_need_repair = !self.missing_events.is_empty()
            || !self.stale_events.is_empty()
            || !self.duplicate_events.is_empty();
        let location = match surface {
            CodexHookSurface::Cli => "open `/hooks` in this Codex CLI".to_owned(),
            CodexHookSurface::App => format!(
                "open Settings → Hooks in Codex App and select source `{}`",
                self.target
            ),
        };
        let disabled_list = self.disabled_events.join(", ");
        let action = match (disabled, needs_trust, definitions_need_repair) {
            (true, false, _) => format!(
                "already trusted; the user must personally {location} and turn on {disabled_list}; `Trust all` does not change an off switch"
            ),
            (true, true, _) => format!(
                "the user must personally {location}, trust new or changed Memory Supervisor entries, and separately turn on {disabled_list}; trust and on/off are separate settings"
            ),
            (false, true, _) => format!(
                "the user must personally {location} and trust new or changed Memory Supervisor entries"
            ),
            (false, false, true) => format!(
                "after repair, the user must personally {location} and review entries Codex marks as new or changed"
            ),
            (false, false, false) => format!(
                "the user must personally {location} and verify all Memory Supervisor entries are on and trusted"
            ),
        };
        let split = if self.split_enable_state_events.is_empty() {
            String::new()
        } else {
            format!(
                "; the same file's equivalent path is on, but on/off is separate per runtime path, so this route remains off for {}",
                self.split_enable_state_events.join(", ")
            )
        };
        let reload = match surface {
            CodexHookSurface::Cli => {
                "restarting cannot change trust or off state; when the check is complete, close `/hooks` and continue the current work"
            }
            CodexHookSurface::App => {
                "restarting cannot change trust or off state; after applying the needed changes, continue an existing App task"
            }
        };
        format!("{action}{split}; {reload}")
    }

    pub fn remediation(&self, surface: CodexHookSurface) -> String {
        let definitions = !self.missing_events.is_empty()
            || !self.stale_events.is_empty()
            || !self.duplicate_events.is_empty();
        let state = !self.disabled_events.is_empty()
            || !self.untrusted_events.is_empty()
            || !self.modified_events.is_empty();
        let review = self.review_action(surface);
        match (definitions, state, self.config_error.is_some()) {
            (_, _, true) => format!(
                "repair the malformed Codex config beside {}, run `memory-supervisor update`, then {review}",
                self.target,
            ),
            (true, _, false) => format!(
                "run `memory-supervisor update`, then {review}; confirm with `memory-status --connections`"
            ),
            (false, true, false) => {
                format!("{review}; confirm with `memory-status --connections`")
            }
            (false, false, false) => "run `memory-status --connections` again".to_owned(),
        }
    }

    pub fn session_start_notice(&self, surface: CodexHookSurface) -> Option<(String, String)> {
        if self.ready() {
            return None;
        }
        let summary = self.summary();
        let remediation = self.remediation(surface);
        let context = format!(
            "[memory-supervisor] Protection setup needs attention. SessionStart reached Memory Supervisor, but the full hook set is not ready ({summary}). The daemon still measures memory, but any affected admission, agent tracking, tool control, or lead handoff must be treated as unavailable and fail-open. Tell the user now. Fix: {remediation}."
        );
        let user = format!(
            "[Memory Supervisor] PROTECTION SETUP NEEDS ATTENTION\nConnected: this SessionStart hook reached Memory Supervisor.\nIncomplete: {summary}.\nEffect: memory measurement continues, but the affected hook-based protections cannot be guaranteed.\nNext: {remediation}."
        );
        Some((context, user))
    }
}

fn normalize_hook_path(path: &Path) -> String {
    let mut value = path
        .display()
        .to_string()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned();
    if let Some(stripped) = value.strip_prefix("//?/") {
        value = stripped.to_owned();
    }
    // Codex can persist the same source with harmless `.` segments (for example after a UI
    // round-trip). Do this lexical reduction before platform alias comparison because Windows
    // canonicalization is not guaranteed to accept every mixed-separator spelling.
    while value.contains("/./") {
        value = value.replace("/./", "/");
    }
    if let Some(stripped) = value.strip_suffix("/.") {
        value = stripped.to_owned();
    }
    let bytes = value.as_bytes();
    if value.starts_with("/mnt/")
        && bytes.get(5).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(6) == Some(&b'/')
    {
        value = format!(
            "{}:/{}",
            (bytes[5] as char).to_ascii_lowercase(),
            &value[7..]
        );
    }
    let bytes = value.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') {
        value.make_ascii_lowercase();
    }
    value
}

fn same_hook_path(left: &Path, right: &Path) -> bool {
    if normalize_hook_path(left) == normalize_hook_path(right) {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => normalize_hook_path(&left) == normalize_hook_path(&right),
        _ => false,
    }
}

pub fn current_codex_hook_target() -> Option<PathBuf> {
    if let Some(home) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home).join("hooks.json"));
    }
    let home = if cfg!(windows) {
        env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"))
    } else {
        env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))
    }?;
    Some(PathBuf::from(home).join(".codex").join("hooks.json"))
}

/// A Memory Supervisor user hook can also be rediscovered as a project hook when another Codex
/// home opens that user's home directory. Only the hook belonging to the current CODEX_HOME may
/// make decisions; the other route is deliberately a no-op.
pub fn codex_hook_source_is_authoritative(source: &Path) -> bool {
    current_codex_hook_target().is_some_and(|target| same_hook_path(source, &target))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let mut canonical = Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&object[key]));
            }
            Value::Object(canonical)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn codex_hook_hash(
    event: &str,
    event_key: &str,
    group: &Value,
    hook: &Value,
    platform: &str,
) -> Option<String> {
    if hook.get("type").and_then(Value::as_str) != Some("command") {
        return None;
    }
    let command = if platform == "windows" {
        hook.get("commandWindows")
            .or_else(|| hook.get("command_windows"))
            .or_else(|| hook.get("command"))
    } else {
        hook.get("command")
    }
    .and_then(Value::as_str)
    .filter(|command| !command.trim().is_empty())?;
    let timeout = hook
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(600)
        .max(1);
    let mut normalized_hook = Map::from_iter([
        ("type".to_owned(), Value::String("command".to_owned())),
        ("command".to_owned(), Value::String(command.to_owned())),
        ("timeout".to_owned(), json!(timeout)),
        (
            "async".to_owned(),
            Value::Bool(hook.get("async").and_then(Value::as_bool).unwrap_or(false)),
        ),
    ]);
    if let Some(message) = hook.get("statusMessage").and_then(Value::as_str) {
        normalized_hook.insert(
            "statusMessage".to_owned(),
            Value::String(message.to_owned()),
        );
    }
    let context_event = matches!(
        event,
        "PreToolUse" | "PostToolUse" | "SessionStart" | "UserPromptSubmit" | "SubagentStart"
    );
    if context_event
        && let Some(limit) = hook
            .get("additionalContextLimit")
            .and_then(Value::as_u64)
            .filter(|limit| *limit != 2_500)
    {
        normalized_hook.insert("additionalContextLimit".to_owned(), json!(limit));
    }
    let mut identity = Map::from_iter([
        ("event_name".to_owned(), Value::String(event_key.to_owned())),
        (
            "hooks".to_owned(),
            Value::Array(vec![Value::Object(normalized_hook)]),
        ),
    ]);
    if !matches!(event, "UserPromptSubmit" | "Stop")
        && let Some(matcher) = group.get("matcher").and_then(Value::as_str)
    {
        identity.insert("matcher".to_owned(), Value::String(matcher.to_owned()));
    }
    let serialized = serde_json::to_vec(&canonical_json(&Value::Object(identity))).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(serialized)))
}

fn same_hook_state_path(left: &Path, right: &Path, platform: &str) -> bool {
    if platform == "windows" {
        let normalize = |path: &Path| {
            path.display()
                .to_string()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_ascii_lowercase()
        };
        return normalize(left) == normalize(right);
    }
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn codex_hook_state<'a>(
    states: &'a toml::map::Map<String, toml::Value>,
    target: &Path,
    event_key: &str,
    group_index: usize,
    handler_index: usize,
    platform: &str,
) -> Option<&'a toml::value::Table> {
    let suffix = format!(":{event_key}:{group_index}:{handler_index}");
    let exact = format!("{}{suffix}", target.display());
    if let Some(state) = states.get(&exact).and_then(toml::Value::as_table) {
        return Some(state);
    }
    states.iter().find_map(|(key, state)| {
        let source = key.strip_suffix(&suffix)?;
        // Codex stores trust under the source spelling seen by that process. A shared Windows/WSL
        // file therefore has separate `C:\...` and `/mnt/c/...` records, which must not be treated
        // as interchangeable even though both names resolve to the same hooks.json.
        same_hook_state_path(Path::new(source), target, platform)
            .then(|| state.as_table())
            .flatten()
    })
}

fn codex_hook_has_enabled_path_alias(
    states: &toml::map::Map<String, toml::Value>,
    target: &Path,
    event_key: &str,
    group_index: usize,
    handler_index: usize,
) -> bool {
    let suffix = format!(":{event_key}:{group_index}:{handler_index}");
    let exact = format!("{}{suffix}", target.display());
    states.iter().any(|(key, state)| {
        if key == &exact {
            return false;
        }
        let Some(source) = key.strip_suffix(&suffix) else {
            return false;
        };
        same_hook_path(Path::new(source), target)
            && state.as_table().is_some_and(|state| {
                state.get("enabled").and_then(toml::Value::as_bool) != Some(false)
            })
    })
}

fn codex_hook_states(path: &Path) -> Result<toml::map::Map<String, toml::Value>, String> {
    let config_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("config.toml");
    let source = match fs::read_to_string(&config_path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => {
            return Err(format!("{}: {error}", config_path.display()));
        }
    };
    let config: toml::Value = source
        .parse()
        .map_err(|error| format!("{}: {error}", config_path.display()))?;
    Ok(config
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default())
}

fn audit_codex_hooks_impl(
    path: &Path,
    binary: &Path,
    platform: &str,
    app_route: bool,
) -> CodexHookHealth {
    let mut health = CodexHookHealth {
        target: path.display().to_string(),
        ..CodexHookHealth::default()
    };
    let config = match load_object(path) {
        Ok(config) => config,
        Err(error) => {
            health.status = "NEEDS ATTENTION".to_owned();
            health.config_error = Some(error);
            return health;
        }
    };
    let desired = match if app_route {
        desired_codex_app_hooks(binary, path, platform, &config)
    } else {
        desired_hooks("codex", binary, path)
    } {
        Ok(desired) => desired,
        Err(error) => {
            health.status = "NEEDS ATTENTION".to_owned();
            health.config_error = Some(error);
            return health;
        }
    };
    let hooks = config.get("hooks").and_then(Value::as_object);
    let states = match codex_hook_states(path) {
        Ok(states) => states,
        Err(error) => {
            health.config_error = Some(error);
            Default::default()
        }
    };
    let expected_events: BTreeSet<_> = CODEX_HOOK_SPECS
        .iter()
        .map(|(event, _, _, _)| *event)
        .collect();
    if let Some(hooks) = hooks {
        for (event, groups) in hooks {
            if expected_events.contains(event.as_str()) || !groups.is_array() {
                continue;
            }
            if groups
                .as_array()
                .is_some_and(|groups| groups.iter().any(group_is_owned))
            {
                health.stale_events.push(event.clone());
            }
        }
    }
    for (event, event_key, _, _) in CODEX_HOOK_SPECS {
        let expected = desired.get(event);
        let groups = hooks
            .and_then(|hooks| hooks.get(event))
            .and_then(Value::as_array);
        let owned_positions: Vec<_> = groups
            .into_iter()
            .flatten()
            .enumerate()
            .flat_map(|(group_index, group)| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(move |(handler_index, hook)| {
                        is_supervisor_hook(hook).then_some((
                            group_index,
                            handler_index,
                            group,
                            hook,
                        ))
                    })
            })
            .collect();
        if owned_positions.is_empty() {
            health.missing_events.push(event.to_owned());
            continue;
        }
        if owned_positions.len() > 1 {
            health.duplicate_events.push(event.to_owned());
            continue;
        }
        let (group_index, handler_index, group, hook) = owned_positions[0];
        let Some(current_hash) = codex_hook_hash(event, event_key, group, hook, platform) else {
            health.stale_events.push(event.to_owned());
            continue;
        };
        let expected_hash = expected
            .and_then(Value::as_array)
            .and_then(|groups| groups.first())
            .and_then(|group| {
                let hook = group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .and_then(|hooks| hooks.first())?;
                codex_hook_hash(event, event_key, group, hook, platform)
            });
        if expected_hash.as_deref() != Some(current_hash.as_str()) {
            health.stale_events.push(event.to_owned());
            continue;
        }
        let Some(state) = codex_hook_state(
            &states,
            path,
            event_key,
            group_index,
            handler_index,
            platform,
        ) else {
            health.untrusted_events.push(event.to_owned());
            continue;
        };
        if state.get("enabled").and_then(toml::Value::as_bool) == Some(false) {
            health.disabled_events.push(event.to_owned());
            if app_route
                && codex_hook_has_enabled_path_alias(
                    &states,
                    path,
                    event_key,
                    group_index,
                    handler_index,
                )
            {
                health.split_enable_state_events.push(event.to_owned());
            }
        }
        match state.get("trusted_hash").and_then(toml::Value::as_str) {
            None => health.untrusted_events.push(event.to_owned()),
            Some(trusted) if trusted != current_hash => {
                health.modified_events.push(event.to_owned())
            }
            Some(_) => {}
        }
    }
    for events in [
        &mut health.missing_events,
        &mut health.stale_events,
        &mut health.duplicate_events,
        &mut health.disabled_events,
        &mut health.untrusted_events,
        &mut health.modified_events,
        &mut health.split_enable_state_events,
    ] {
        events.sort();
        events.dedup();
    }
    health.definition_valid = health.missing_events.is_empty()
        && health.stale_events.is_empty()
        && health.duplicate_events.is_empty();
    health.status = if health.definition_valid
        && health.disabled_events.is_empty()
        && health.untrusted_events.is_empty()
        && health.modified_events.is_empty()
        && health.config_error.is_none()
    {
        "HEALTHY"
    } else if health.definition_valid
        && health.disabled_events.is_empty()
        && health.config_error.is_none()
        && (!health.untrusted_events.is_empty() || !health.modified_events.is_empty())
    {
        "NEEDS TRUST"
    } else {
        "NEEDS ATTENTION"
    }
    .to_owned();
    health
}

pub fn audit_codex_hooks(path: &Path, binary: &Path, platform: &str) -> CodexHookHealth {
    audit_codex_hooks_impl(path, binary, platform, false)
}

pub fn audit_codex_app_hooks(path: &Path, binary: &Path, platform: &str) -> CodexHookHealth {
    audit_codex_hooks_impl(path, binary, platform, true)
}

fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn copy_path(old: &Path, new: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(old)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to migrate symlink {}", old.display()),
        ));
    }
    if metadata.is_file() {
        fs::copy(old, new)?;
        fs::set_permissions(new, metadata.permissions())?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to migrate special file {}", old.display()),
        ));
    }
    fs::create_dir(new)?;
    for entry in fs::read_dir(old)? {
        let entry = entry?;
        copy_path(&entry.path(), &new.join(entry.file_name()))?;
    }
    fs::set_permissions(new, metadata.permissions())
}

fn copy_then_remove(old: &Path, new: &Path) -> io::Result<()> {
    let parent = new.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("migration target has no parent: {}", new.display()),
        )
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = new.file_name().unwrap_or_else(|| OsStr::new("state"));
    let temporary = parent.join(format!(
        ".{}.memory-supervisor-migrate-{}-{stamp}",
        name.to_string_lossy(),
        std::process::id()
    ));
    if temporary.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("migration staging path exists: {}", temporary.display()),
        ));
    }
    if let Err(error) = copy_path(old, &temporary) {
        let _ = remove_path(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, new) {
        let _ = remove_path(&temporary);
        return Err(error);
    }
    if let Err(error) = remove_path(old) {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "migration copied {} to {} but could not remove the source; the complete destination was retained: {error}",
                old.display(),
                new.display()
            ),
        ));
    }
    Ok(())
}

fn move_if_absent_with<F>(old: &Path, new: &Path, rename: F) -> io::Result<bool>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    if !old.exists() || new.exists() {
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        fs::create_dir_all(parent)?;
    }
    match rename(old, new) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_then_remove(old, new)?;
        }
        Err(error) => return Err(error),
    }
    Ok(true)
}

fn move_if_absent(old: &Path, new: &Path) -> io::Result<bool> {
    move_if_absent_with(old, new, |old, new| fs::rename(old, new))
}

fn move_known(old: &Path, new: &Path, names: &[(&str, &str)]) -> io::Result<()> {
    if !old.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(new)?;
    for (old_name, new_name) in names {
        let _ = move_if_absent(&old.join(old_name), &new.join(new_name))?;
    }
    let _ = fs::remove_dir(old);
    Ok(())
}

fn rewrite_pointer(path: &Path, replacements: &[(PathBuf, PathBuf)]) -> io::Result<()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let target = PathBuf::from(raw.trim());
    for (old, new) in replacements {
        if &target != old {
            continue;
        }
        let _ = move_if_absent(old, new)?;
        fs::create_dir_all(new)?;
        write_atomic_text(path, &format!("{}\n", new.display()), 0o600)?;
        if let Some(parent) = old.parent() {
            let _ = fs::remove_dir(parent);
        }
        break;
    }
    Ok(())
}

fn renamed_config_key(key: &str) -> Option<String> {
    let special = match key {
        "MEMORY_GOVERNOR_NOTIFY_CHANNELS" | "CMG_NOTIFY_CHANNELS" => {
            Some("MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS")
        }
        "MEMORY_GOVERNOR_NOTIFY_CONFIG" | "CMG_NOTIFY_CONFIG" => {
            Some("MEMORY_SUPERVISOR_NOTIFICATION_CONFIG")
        }
        "MEMORY_GOVERNOR_NOTIFY_MIN_LEVEL"
        | "MEMORY_GOVERNOR_NOTIFICATION_MIN_LEVEL"
        | "CMG_NOTIFY_MIN_LEVEL" => Some("MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL"),
        "MEMORY_SUPERVISOR_NOTIFY_CHANNELS" => Some("MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS"),
        "MEMORY_SUPERVISOR_NOTIFY_CONFIG" => Some("MEMORY_SUPERVISOR_NOTIFICATION_CONFIG"),
        "MEMORY_SUPERVISOR_NOTIFY_MIN_LEVEL" | "MEMORY_SUPERVISOR_NOTIFICATION_MIN_LEVEL" => {
            Some("MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL")
        }
        _ => None,
    };
    if let Some(value) = special {
        return Some(value.to_owned());
    }
    key.strip_prefix("MEMORY_GOVERNOR_")
        .or_else(|| key.strip_prefix("CMG_"))
        .map(|suffix| format!("MEMORY_SUPERVISOR_{suffix}"))
}

fn removed_legacy_key(key: &str) -> bool {
    matches!(
        key,
        "MEMORY_GOVERNOR_AGGREGATE_ACTION"
            | "MEMORY_GOVERNOR_STOP_SCOPE"
            | "MEMORY_GOVERNOR_TARGETS"
            | "MEMORY_SUPERVISOR_AGGREGATE_ACTION"
            | "MEMORY_SUPERVISOR_HOLD_S"
            | "MEMORY_SUPERVISOR_STOP_SCOPE"
            | "MEMORY_SUPERVISOR_TARGETS"
            | "CMG_AGGREGATE_ACTION"
            | "CMG_HOLD_S"
            | "CMG_STOP_SCOPE"
            | "CMG_TARGETS"
    )
}

fn rewrite_legacy_json(path: &Path) -> Result<(), String> {
    let source = match fs::read(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let value: Value = serde_json::from_slice(&source)
        .map_err(|error| format!("legacy config left unchanged: {error}"))?;
    let object = value.as_object().ok_or_else(|| {
        format!(
            "legacy config left unchanged: {} is not an object",
            path.display()
        )
    })?;
    let mut current = Map::new();
    for (key, value) in object {
        if key.starts_with("MEMORY_GOVERNOR_") || key.starts_with("CMG_") || removed_legacy_key(key)
        {
            continue;
        }
        let key = renamed_config_key(key).unwrap_or_else(|| key.clone());
        current.entry(key).or_insert_with(|| value.clone());
    }
    for (key, value) in object {
        if (key.starts_with("MEMORY_GOVERNOR_") || key.starts_with("CMG_"))
            && !removed_legacy_key(key)
            && let Some(key) = renamed_config_key(key)
        {
            current.entry(key).or_insert_with(|| value.clone());
        }
    }
    if &current != object {
        write_atomic_json(path, &Value::Object(current), 0o600, true)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn rewrite_legacy_notifications(path: &Path) -> io::Result<()> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let updated = source
        .replace("MEMORY_GOVERNOR_", "MEMORY_SUPERVISOR_")
        .replace("CMG_", "MEMORY_SUPERVISOR_")
        .replace(
            "MEMORY_SUPERVISOR_NOTIFY_CHANNELS",
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS",
        )
        .replace(
            "MEMORY_SUPERVISOR_NOTIFY_CONFIG",
            "MEMORY_SUPERVISOR_NOTIFICATION_CONFIG",
        )
        .replace(
            "MEMORY_SUPERVISOR_NOTIFY_MIN_LEVEL",
            "MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL",
        )
        .replace(
            "MEMORY_SUPERVISOR_NOTIFICATION_MIN_LEVEL",
            "MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL",
        );
    if updated != source {
        write_atomic_text(path, &updated, 0o600)?;
    }
    Ok(())
}

pub fn migrate_install_names(home: &Path) -> Result<(), String> {
    let pointers = home.join(".memory-supervisor");
    for legacy in [home.join(".memory-governor"), home.join(".cmg")] {
        move_known(
            &legacy,
            &pointers,
            &[
                ("state-dir", "state-dir"),
                ("federation-dir", "federation-dir"),
                ("instances", "instances"),
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    let cache = home.join(".cache");
    for name in ["claude-governor", "claude-memory-governor", "cmg"] {
        let _ = move_if_absent(&cache.join(name), &cache.join("memory-supervisor"))
            .map_err(|error| error.to_string())?;
    }
    let replacements: Vec<_> = ["claude-governor", "claude-memory-governor", "cmg"]
        .into_iter()
        .map(|name| (cache.join(name), cache.join("memory-supervisor")))
        .collect();
    rewrite_pointer(&pointers.join("state-dir"), &replacements)
        .map_err(|error| error.to_string())?;
    if let Ok(raw) = fs::read_to_string(pointers.join("federation-dir")) {
        let target = PathBuf::from(raw.trim());
        if target.file_name() == Some(OsStr::new("instances"))
            && target
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == ".cmg" || name == ".memory-governor")
            && let Some(base) = target.parent().and_then(Path::parent)
        {
            let new = base.join(".memory-supervisor").join("instances");
            rewrite_pointer(&pointers.join("federation-dir"), &[(target, new)])
                .map_err(|error| error.to_string())?;
        }
    }
    let config_root = home.join(".config");
    let current = config_root.join("memory-supervisor");
    let legacy = config_root.join("claude-governor");
    let _ = move_if_absent(&legacy, &current).map_err(|error| error.to_string())?;
    move_known(
        &legacy,
        &current,
        &[
            ("config.json", "config.json"),
            ("notify.conf", "notifications.conf"),
            (".dm_channel", ".dm_channel"),
        ],
    )
    .map_err(|error| error.to_string())?;
    let _ = move_if_absent(
        &current.join("notify.conf"),
        &current.join("notifications.conf"),
    )
    .map_err(|error| error.to_string())?;
    move_known(
        &config_root.join("cmg"),
        &current,
        &[
            ("config.json", "config.json"),
            ("notify.conf", "notifications.conf"),
            (".dm_channel", ".dm_channel"),
        ],
    )
    .map_err(|error| error.to_string())?;
    rewrite_legacy_json(&current.join("config.json"))?;
    rewrite_legacy_notifications(&current.join("notifications.conf"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn integration_usage() -> &'static str {
    "USAGE:\n  memory-supervisor integration hooks --target PATH --provider claude|codex --binary PATH [--remove|--check]\n  memory-supervisor integration app-hooks --target PATH --binary PATH [--platform windows|wsl|linux|darwin] [--remove|--check]\n  memory-supervisor integration resolve-claude [--home PATH]\n  memory-supervisor integration check-claude [--command PATH]\n  memory-supervisor integration check-codex [--command PATH]\n  memory-supervisor integration migrate-names [--home PATH]\n  memory-supervisor integration path state|federation"
}

fn resolve_claude_command(arguments: &[String]) -> i32 {
    let home = if arguments.is_empty() {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else if arguments.len() == 2 && arguments[0] == "--home" {
        PathBuf::from(&arguments[1])
    } else {
        eprintln!(
            "resolve-claude accepts only --home PATH\n{}",
            integration_usage()
        );
        return 2;
    };
    let discovery = discover_claude(&home);
    if let Some(selected) = discovery.selected {
        println!("{}", selected.path.display());
        return 0;
    }
    eprintln!("{}", discovery.failure_summary());
    1
}

fn hooks_command(arguments: &[String]) -> i32 {
    let mut target = None;
    let mut provider = None;
    let mut binary = None;
    let mut remove = false;
    let mut check = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--target" | "--provider" | "--binary" | "--root" => {
                let Some(value) = arguments.get(index + 1) else {
                    eprintln!("{} requires a value", arguments[index]);
                    return 2;
                };
                match arguments[index].as_str() {
                    "--target" => target = Some(PathBuf::from(value)),
                    "--provider" => provider = Some(value.clone()),
                    "--binary" => binary = Some(PathBuf::from(value)),
                    "--root" => {
                        let root = PathBuf::from(value);
                        binary = Some(root.join("bin").join(if cfg!(windows) {
                            "memory-supervisor.exe"
                        } else {
                            "memory-supervisor"
                        }));
                    }
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--remove" => {
                remove = true;
                index += 1;
            }
            "--check" => {
                check = true;
                index += 1;
            }
            option => {
                eprintln!("unknown hooks option: {option}\n{}", integration_usage());
                return 2;
            }
        }
    }
    let (Some(target), Some(provider), Some(binary)) = (target, provider, binary) else {
        eprintln!(
            "hooks requires --target, --provider, and --binary\n{}",
            integration_usage()
        );
        return 2;
    };
    if !matches!(provider.as_str(), "claude" | "codex") {
        eprintln!("unsupported provider: {provider}");
        return 2;
    }
    if check {
        match verify_hooks(&target, &provider, &binary) {
            Ok(valid) => {
                println!(
                    "{}: {}",
                    if valid { "valid" } else { "invalid" },
                    target.display()
                );
                if valid { 0 } else { 1 }
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    } else {
        match update_hooks(&target, &provider, &binary, remove) {
            Ok(changed) => {
                println!(
                    "{}: {}",
                    if changed { "updated" } else { "unchanged" },
                    target.display()
                );
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    }
}

fn app_hooks_command(arguments: &[String]) -> i32 {
    let mut target = None;
    let mut binary = None;
    let mut platform = crate::platform::platform_name();
    let mut remove = false;
    let mut check = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--target" | "--binary" | "--platform" => {
                let Some(value) = arguments.get(index + 1) else {
                    eprintln!("{} requires a value", arguments[index]);
                    return 2;
                };
                match arguments[index].as_str() {
                    "--target" => target = Some(PathBuf::from(value)),
                    "--binary" => binary = Some(PathBuf::from(value)),
                    "--platform" => platform = value.to_lowercase(),
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--remove" => {
                remove = true;
                index += 1;
            }
            "--check" => {
                check = true;
                index += 1;
            }
            option => {
                eprintln!(
                    "unknown app-hooks option: {option}\n{}",
                    integration_usage()
                );
                return 2;
            }
        }
    }
    let (Some(target), Some(binary)) = (target, binary) else {
        eprintln!(
            "app-hooks requires --target and --binary\n{}",
            integration_usage()
        );
        return 2;
    };
    let result = if check {
        verify_codex_app_hooks(&target, &binary, &platform).map(|valid| {
            println!(
                "{}: {}",
                if valid { "valid" } else { "invalid" },
                target.display()
            );
            if valid { 0 } else { 1 }
        })
    } else {
        update_codex_app_hooks(&target, &binary, &platform, remove).map(|changed| {
            println!(
                "{}: {}",
                if changed { "updated" } else { "unchanged" },
                target.display()
            );
            0
        })
    };
    result.unwrap_or_else(|error| {
        eprintln!("{error}");
        1
    })
}

pub fn run_integration(arguments: &[OsString]) -> i32 {
    let arguments: Vec<String> = match arguments
        .iter()
        .map(|value| value.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
    {
        Some(arguments) => arguments,
        None => {
            eprintln!("arguments must be valid Unicode\n{}", integration_usage());
            return 2;
        }
    };
    let Some(action) = arguments.first().map(String::as_str) else {
        println!("{}", integration_usage());
        return 2;
    };
    match action {
        "--help" | "-h" | "help" => {
            println!("{}", integration_usage());
            0
        }
        "hooks" => hooks_command(&arguments[1..]),
        "app-hooks" => app_hooks_command(&arguments[1..]),
        "resolve-claude" => resolve_claude_command(&arguments[1..]),
        "path" => {
            if arguments.len() != 2 {
                eprintln!("path requires state or federation\n{}", integration_usage());
                return 2;
            }
            match arguments[1].as_str() {
                "state" => println!("{}", crate::config::state_dir().display()),
                "federation" => println!("{}", crate::platform::federation_dir().display()),
                value => {
                    eprintln!("unknown path: {value}; expected state or federation");
                    return 2;
                }
            }
            0
        }
        "migrate-names" => {
            let home = if arguments.len() == 1 {
                env::var_os("HOME")
                    .or_else(|| env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            } else if arguments.len() == 3 && arguments[1] == "--home" {
                PathBuf::from(&arguments[2])
            } else {
                eprintln!(
                    "migrate-names accepts only --home PATH\n{}",
                    integration_usage()
                );
                return 2;
            };
            match migrate_install_names(&home) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("install-name migration failed: {error}");
                    1
                }
            }
        }
        "check-claude" | "check-codex" => {
            let mut command = if action == "check-claude" {
                OsString::from("claude")
            } else {
                OsString::from("codex")
            };
            if !arguments[1..].is_empty() {
                if arguments.len() != 3 || arguments[1] != "--command" {
                    eprintln!(
                        "{action} accepts only --command PATH\n{}",
                        integration_usage()
                    );
                    return 2;
                }
                command = OsString::from(&arguments[2]);
            }
            let (supported, reason) = if action == "check-claude" {
                inspect_claude(&command)
            } else {
                inspect_codex(&command)
            };
            println!("{reason}");
            if supported { 0 } else { 1 }
        }
        _ => {
            eprintln!(
                "unknown integration action: {action}\n{}",
                integration_usage()
            );
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_directory() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "memory-supervisor-integration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn provider_contract_versions_are_exact() {
        assert!(claude_contract("Claude Code 2.1.217").0);
        assert!(!claude_contract("Claude Code 2.1.216").0);
        assert!(!claude_contract("Claude Code unknown").0);
        assert!(codex_contract("codex-cli 0.145.0", "hooks stable true\n").0);
        assert!(!codex_contract("codex-cli 0.144.9", "hooks stable true\n").0);
        assert!(!codex_contract("codex-cli 0.145.0", "hooks stable false\n").0);
        assert!(!codex_contract("codex-cli 0.145.0", "hooks experimental true\n").0);
    }

    #[test]
    fn claude_discovery_uses_supported_nvm_install_when_path_starts_with_old_global() {
        let root = temp_directory();
        let path_bin = root.join("path-bin");
        let nvm_bin = root.join(".nvm/versions/node/v24.16.0/bin");
        fs::create_dir_all(&path_bin).unwrap();
        fs::create_dir_all(&nvm_bin).unwrap();
        let write_claude = |path: &Path, version: &str| {
            let source = if cfg!(windows) {
                format!("@echo {version} (Claude Code)\r\n")
            } else {
                format!("#!/bin/sh\necho '{version} (Claude Code)'\n")
            };
            fs::write(path, source).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        };
        let executable = if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        };
        write_claude(&path_bin.join(executable), "2.1.142");
        write_claude(&nvm_bin.join(executable), "2.1.220");
        let search_path = env::join_paths([&path_bin]).unwrap();

        let discovery = discover_claude_with_path(&root, Some(&search_path));
        let selected = discovery.selected.unwrap();
        assert_eq!(selected.version.as_deref(), Some("2.1.220"));
        assert_eq!(selected.path, nvm_bin.join(executable));
        assert!(!selected.on_path);
        assert!(discovery.installations.iter().any(|candidate| {
            candidate.path == path_bin.join(executable) && !candidate.supported && candidate.on_path
        }));
        fs::remove_dir_all(root).unwrap();
    }

    fn write_codex_trust_state(
        target: &Path,
        platform: &str,
        disabled_event: Option<&str>,
        modified_event: Option<&str>,
    ) {
        let config: Value = serde_json::from_slice(&fs::read(target).unwrap()).unwrap();
        let hooks = config["hooks"].as_object().unwrap();
        let mut output = "[hooks.state]\n".to_owned();
        for (event, event_key, _, _) in CODEX_HOOK_SPECS {
            let groups = hooks[event].as_array().unwrap();
            let (group_index, group) = groups
                .iter()
                .enumerate()
                .find(|(_, group)| group_is_owned(group))
                .unwrap();
            let (handler_index, hook) = group["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .find(|(_, hook)| is_supervisor_hook(hook))
                .unwrap();
            let mut hash = codex_hook_hash(event, event_key, group, hook, platform).unwrap();
            if modified_event == Some(event) {
                hash = "sha256:not-current".to_owned();
            }
            let key = format!(
                "{}:{event_key}:{group_index}:{handler_index}",
                target.display()
            )
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
            output.push_str(&format!(
                "\n[hooks.state.\"{key}\"]\ntrusted_hash = \"{hash}\"\n"
            ));
            if disabled_event == Some(event) {
                output.push_str("enabled = false\n");
            }
        }
        fs::write(target.parent().unwrap().join("config.toml"), output).unwrap();
    }

    #[test]
    fn codex_trust_hash_matches_the_normalized_hook_contract() {
        let group = json!({
            "matcher":".*",
            "hooks":[{
                "type":"command",
                "command":"'/home/owner/.local/lib/memory-supervisor/memory-supervisor' gate codex PreToolUse",
                "timeout":20
            }]
        });
        assert_eq!(
            codex_hook_hash(
                "PreToolUse",
                "pre_tool_use",
                &group,
                &group["hooks"][0],
                "wsl"
            )
            .as_deref(),
            Some("sha256:00c945a66a9b5fb79cfa3eeafbfc54af495d7852045a403693f725623a85e3bc")
        );
    }

    #[test]
    fn codex_hook_health_requires_every_event_to_be_enabled_and_currently_trusted() {
        let root = temp_directory();
        let target = root.join("hooks.json");
        let binary = root.join("memory-supervisor");
        let platform = crate::platform::platform_name();
        update_codex_app_hooks(&target, &binary, &platform, false).unwrap();

        write_codex_trust_state(&target, &platform, None, None);
        let healthy = audit_codex_hooks(&target, &binary, &platform);
        assert!(healthy.ready(), "{}", healthy.summary());

        write_codex_trust_state(&target, &platform, Some("SubagentStop"), None);
        let disabled = audit_codex_hooks(&target, &binary, &platform);
        assert_eq!(disabled.status, "NEEDS ATTENTION");
        assert_eq!(disabled.disabled_events, ["SubagentStop"]);
        let (lead, user) = disabled
            .session_start_notice(CodexHookSurface::Cli)
            .unwrap();
        assert!(lead.contains("Tell the user now"));
        assert!(user.contains("PROTECTION SETUP NEEDS ATTENTION"));
        assert!(user.contains("open `/hooks`"));
        assert!(user.contains("must personally"));
        assert!(user.contains("already trusted"));
        assert!(user.contains("Trust all"));
        assert!(user.contains("restarting cannot change trust"));
        assert!(!user.contains("Settings → Hooks"));

        let app = disabled.remediation(CodexHookSurface::App);
        assert!(app.contains("Settings → Hooks"));
        assert!(app.contains("already trusted"));
        assert!(app.contains("Trust all"));
        assert!(app.contains(&target.display().to_string()));
        assert!(app.contains("continue an existing App task"));
        assert!(app.contains("must personally"));
        assert!(app.contains("restarting cannot change trust"));
        assert!(!app.contains("open `/hooks`"));

        let config_path = target.parent().unwrap().join("config.toml");
        let alias = format!(
            "{}/./hooks.json:subagent_stop:0:0",
            target.parent().unwrap().display()
        )
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
        let mut source = fs::read_to_string(&config_path).unwrap();
        source.push_str(&format!(
            "\n[hooks.state.\"{alias}\"]\ntrusted_hash = \"sha256:alias\"\n"
        ));
        fs::write(&config_path, source).unwrap();
        let split = audit_codex_app_hooks(&target, &binary, &platform);
        assert_eq!(split.split_enable_state_events, ["SubagentStop"]);
        assert!(
            split
                .remediation(CodexHookSurface::App)
                .contains("on/off is separate per runtime path")
        );

        write_codex_trust_state(&target, &platform, None, Some("PreToolUse"));
        let modified = audit_codex_hooks(&target, &binary, &platform);
        assert_eq!(modified.status, "NEEDS TRUST");
        assert_eq!(modified.modified_events, ["PreToolUse"]);

        let mut config: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        config["hooks"].as_object_mut().unwrap().remove("Stop");
        fs::write(&target, serde_json::to_vec(&config).unwrap()).unwrap();
        let missing = audit_codex_hooks(&target, &binary, &platform);
        assert_eq!(missing.status, "NEEDS ATTENTION");
        assert_eq!(missing.missing_events, ["Stop"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_app_reports_split_enable_state_for_the_same_windows_wsl_file() {
        let source = r#"
[hooks.state]

[hooks.state.'/mnt/c/Users/owner/.codex/hooks.json:subagent_stop:1:0']
enabled = false
trusted_hash = 'sha256:wsl'

[hooks.state.'C:\Users\owner\.codex\hooks.json:subagent_stop:1:0']
trusted_hash = 'sha256:windows'
"#;
        let config: toml::Value = source.parse().unwrap();
        let states = config["hooks"]["state"].as_table().unwrap();
        assert!(codex_hook_has_enabled_path_alias(
            states,
            Path::new("/mnt/c/Users/owner/.codex/hooks.json"),
            "subagent_stop",
            1,
            0,
        ));

        let health = CodexHookHealth {
            target: "/mnt/c/Users/owner/.codex/hooks.json".to_owned(),
            status: "NEEDS ATTENTION".to_owned(),
            definition_valid: true,
            disabled_events: vec!["SubagentStop".to_owned()],
            split_enable_state_events: vec!["SubagentStop".to_owned()],
            ..CodexHookHealth::default()
        };
        let app = health.remediation(CodexHookSurface::App);
        assert!(app.contains("Settings → Hooks"));
        assert!(app.contains("on/off is separate per runtime path"));
        assert!(app.contains("this route remains off"));
        assert!(!app.contains("open `/hooks`"));
        assert!(app.chars().count() <= 500, "{app}");
    }

    #[test]
    fn codex_hook_paths_match_windows_and_wsl_aliases_but_not_other_homes() {
        assert!(same_hook_path(
            Path::new("/mnt/c/Users/owner/.codex/hooks.json"),
            Path::new(r"C:\Users\Owner\.codex\hooks.json")
        ));
        assert!(same_hook_path(
            Path::new(r"C:\Users\owner\.codex\.\hooks.json"),
            Path::new(r"c:/users/owner/.codex/hooks.json")
        ));
        assert!(!same_hook_path(
            Path::new("/home/owner/.codex/hooks.json"),
            Path::new("/mnt/c/Users/owner/.codex/hooks.json")
        ));
        assert!(!same_hook_state_path(
            Path::new("/mnt/c/Users/owner/.codex/hooks.json"),
            Path::new(r"C:\Users\owner\.codex\hooks.json"),
            "wsl"
        ));
        assert!(same_hook_state_path(
            Path::new(r"C:\Users\OWNER\.codex\hooks.json"),
            Path::new(r"c:/users/owner/.codex/hooks.json"),
            "windows"
        ));
    }

    #[test]
    fn hook_merge_is_idempotent_and_preserves_foreign_handlers() {
        let root = temp_directory();
        let target = root.join("settings.json");
        let binary = root.join("memory-supervisor");
        fs::write(
            &target,
            serde_json::to_vec(&json!({
                "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "foreign"}]}]},
                "keep": true
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(update_hooks(&target, "claude", &binary, false).unwrap());
        assert!(verify_hooks(&target, "claude", &binary).unwrap());
        assert!(!update_hooks(&target, "claude", &binary, false).unwrap());
        assert!(update_hooks(&target, "claude", &binary, true).unwrap());
        let value: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(value["keep"], true);
        assert_eq!(value["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "foreign"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_name_migration_preserves_current_keys_and_removes_dead_policy() {
        let home = temp_directory();
        let old_config = home.join(".config/claude-governor");
        fs::create_dir_all(&old_config).unwrap();
        fs::write(
            old_config.join("config.json"),
            serde_json::to_vec(&json!({
                "MEMORY_GOVERNOR_CLI_HARD_CAP_MB": 4096,
                "MEMORY_GOVERNOR_NOTIFY_MIN_LEVEL": "orange",
                "MEMORY_GOVERNOR_STOP_SCOPE": "all",
                "MEMORY_SUPERVISOR_NOTIFICATION_MIN_LEVEL": "red",
                "MEMORY_SUPERVISOR_TARGETS": "claude,codex,other",
                "CMG_HOLD_S": 12,
                "CMG_TARGETS": "claude,codex,gemini,agy"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            old_config.join("notify.conf"),
            "MEMORY_GOVERNOR_NOTIFY_CHANNELS=\"telegram\"\nCMG_TELEGRAM_BOT_TOKEN=\"secret\"\n",
        )
        .unwrap();
        let old_pointer = home.join(".memory-governor");
        fs::create_dir(&old_pointer).unwrap();
        fs::write(old_pointer.join("state-dir"), "/tmp/state\n").unwrap();
        let old_state = home.join(".cache").join("claude-governor");
        fs::create_dir_all(&old_state).unwrap();
        fs::write(old_state.join("runtime.json"), "{}").unwrap();

        migrate_install_names(&home).unwrap();

        let config: Value = serde_json::from_slice(
            &fs::read(home.join(".config/memory-supervisor/config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config,
            json!({
                "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB": 4096,
                "MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL": "red"
            })
        );
        assert_eq!(
            fs::read_to_string(home.join(".config/memory-supervisor/notifications.conf"))
                .unwrap()
                .trim(),
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS=\"telegram\"\nMEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN=\"secret\""
        );
        assert!(
            home.join(".cache")
                .join("memory-supervisor")
                .join("runtime.json")
                .is_file()
        );
        assert!(home.join(".memory-supervisor/state-dir").is_file());
        assert!(!old_config.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn install_name_migration_rewrites_default_state_and_federation_pointers() {
        let home = temp_directory();
        let old_state = home.join(".cache").join("claude-governor");
        fs::create_dir_all(&old_state).unwrap();
        fs::write(old_state.join("runtime.json"), "{}").unwrap();
        let old_federation = home.join(".cmg/instances");
        fs::create_dir_all(&old_federation).unwrap();
        fs::write(old_federation.join("peer.json"), "{}").unwrap();
        let old_pointers = home.join(".memory-governor");
        fs::create_dir(&old_pointers).unwrap();
        fs::write(
            old_pointers.join("state-dir"),
            format!("{}\n", old_state.display()),
        )
        .unwrap();
        fs::write(
            old_pointers.join("federation-dir"),
            format!("{}\n", old_federation.display()),
        )
        .unwrap();

        migrate_install_names(&home).unwrap();

        let current = home.join(".memory-supervisor");
        let new_state = home.join(".cache").join("memory-supervisor");
        let new_federation = current.join("instances");
        assert_eq!(
            fs::read_to_string(current.join("state-dir"))
                .unwrap()
                .trim(),
            new_state.to_string_lossy()
        );
        assert_eq!(
            fs::read_to_string(current.join("federation-dir"))
                .unwrap()
                .trim(),
            new_federation.to_string_lossy()
        );
        assert!(new_state.join("runtime.json").is_file());
        assert!(new_federation.join("peer.json").is_file());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn install_name_migration_keeps_external_federation_on_its_volume() {
        let home = temp_directory();
        let external = temp_directory();
        let old_federation = external.join("user").join(".cmg").join("instances");
        fs::create_dir_all(&old_federation).unwrap();
        fs::write(old_federation.join("peer.json"), "{}").unwrap();
        let old_pointers = home.join(".cmg");
        fs::create_dir(&old_pointers).unwrap();
        fs::write(
            old_pointers.join("federation-dir"),
            format!("{}\n", old_federation.display()),
        )
        .unwrap();

        migrate_install_names(&home).unwrap();

        let new_federation = external
            .join("user")
            .join(".memory-supervisor")
            .join("instances");
        assert_eq!(
            fs::read_to_string(home.join(".memory-supervisor/federation-dir"))
                .unwrap()
                .trim(),
            new_federation.to_string_lossy()
        );
        assert!(new_federation.join("peer.json").is_file());
        assert!(!old_federation.exists());
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn install_name_migration_falls_back_to_copy_across_devices() {
        let root = temp_directory();
        let old = root.join("old");
        let new = root.join("new");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("peer.json"), "state").unwrap();

        let moved = move_if_absent_with(&old, &new, |_, _| {
            Err(io::Error::new(
                io::ErrorKind::CrossesDevices,
                "simulated cross-device move",
            ))
        })
        .unwrap();

        assert!(moved);
        assert_eq!(fs::read_to_string(new.join("peer.json")).unwrap(), "state");
        assert!(!old.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_cleanup_failure_keeps_the_complete_destination() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_directory();
        let protected = root.join("protected");
        let old = protected.join("old");
        let new = root.join("new");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("peer.json"), "state").unwrap();
        fs::set_permissions(&protected, fs::Permissions::from_mode(0o500)).unwrap();

        let error = move_if_absent_with(&old, &new, |_, _| {
            Err(io::Error::new(
                io::ErrorKind::CrossesDevices,
                "simulated cross-device move",
            ))
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("complete destination was retained")
        );
        assert_eq!(fs::read_to_string(new.join("peer.json")).unwrap(), "state");
        fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_config_symlink_is_refused() {
        use std::os::unix::fs::symlink;
        let root = temp_directory();
        let target = root.join("settings.json");
        symlink(root.join("missing.json"), &target).unwrap();
        let error =
            update_hooks(&target, "codex", &root.join("memory-supervisor"), false).unwrap_err();
        assert!(error.contains("dangling"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixed_hook_groups_and_malformed_containers_are_handled_exactly() {
        let root = temp_directory();
        let target = root.join("settings.json");
        let binary = root.join("memory-supervisor");
        fs::write(
            &target,
            serde_json::to_vec(&json!({
                "hooks": {
                    "SessionStart": [{
                        "matcher": "keep-me",
                        "hooks": [
                            {"type":"command","command":"foreign"},
                            {"type":"command","command":"/old/memory-supervisor gate SessionStart"}
                        ]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(update_hooks(&target, "claude", &binary, true).unwrap());
        let value: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(value["hooks"]["SessionStart"][0]["matcher"], "keep-me");
        assert_eq!(
            value["hooks"]["SessionStart"][0]["hooks"],
            json!([{"type":"command","command":"foreign"}])
        );

        fs::write(&target, br#"{"hooks":[]}"#).unwrap();
        assert!(
            update_hooks(&target, "claude", &binary, false)
                .unwrap_err()
                .contains("non-object hooks")
        );
        fs::write(&target, br#"{"hooks":{"SessionStart":{}}}"#).unwrap();
        assert!(
            update_hooks(&target, "claude", &binary, false)
                .unwrap_err()
                .contains("non-list hook event")
        );

        fs::write(&target, b"{}").unwrap();
        update_hooks(&target, "codex", &binary, false).unwrap();
        let mut duplicate: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        let extra = duplicate["hooks"]["SessionStart"][0].clone();
        duplicate["hooks"]["SessionStart"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        fs::write(&target, serde_json::to_vec(&duplicate).unwrap()).unwrap();
        assert!(!verify_hooks(&target, "codex", &binary).unwrap());

        let desired = desired_hooks("codex", &binary, &target).unwrap();
        assert_eq!(desired["PreToolUse"][0]["matcher"], ".*");
        let command_field = if cfg!(windows) {
            "commandWindows"
        } else {
            "command"
        };
        assert!(
            desired["PreToolUse"][0]["hooks"][0][command_field]
                .as_str()
                .unwrap()
                .contains("gate codex PreToolUse")
        );
        assert!(desired.values().all(|groups| {
            groups.as_array().is_some_and(|groups| {
                groups.iter().all(|group| {
                    group["hooks"].as_array().is_some_and(|hooks| {
                        hooks.iter().all(|hook| hook.get("statusMessage").is_none())
                    })
                })
            })
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_app_hook_routes_merge_wsl_and_windows_without_cross_overwrite() {
        let root = temp_directory();
        let target = root.join("hooks.json");
        let wsl_binary = root.join("wsl/memory-supervisor");
        let windows_binary = PathBuf::from(r"C:\Users\owner\memory-supervisor.exe");
        fs::write(
            &target,
            serde_json::to_vec(&json!({
                "keep": true,
                "hooks": {
                    "SessionStart": [{"hooks": [{"type":"command","command":"foreign"}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(update_codex_app_hooks(&target, &wsl_binary, "wsl", false).unwrap());
        assert!(verify_codex_app_hooks(&target, &wsl_binary, "wsl").unwrap());
        assert!(update_codex_app_hooks(&target, &windows_binary, "windows", false).unwrap());
        assert!(verify_codex_app_hooks(&target, &windows_binary, "windows").unwrap());

        let value: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        let owned = &value["hooks"]["PreToolUse"][0]["hooks"][0];
        assert_eq!(
            owned["command"],
            format!(
                "'{}' gate codex PreToolUse --hook-source '{}'",
                wsl_binary.display(),
                target.display()
            )
        );
        assert_eq!(
            owned["commandWindows"],
            format!(
                r#"& 'C:\Users\owner\memory-supervisor.exe' gate codex PreToolUse --hook-source '{}'"#,
                target.display()
            )
        );
        assert_eq!(value["keep"], true);
        assert_eq!(
            value["hooks"]["SessionStart"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|group| group["hooks"][0]["command"] == "foreign")
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_app_hook_writer_supports_every_native_platform_family() {
        for (platform, binary_name) in [
            ("linux", "memory-supervisor-linux"),
            ("wsl", "memory-supervisor-wsl"),
            ("darwin", "memory-supervisor-macos"),
            ("macos", "memory-supervisor-macos-alias"),
        ] {
            let root = temp_directory();
            let target = root.join("codex-home/hooks.json");
            let binary = root.join(binary_name);
            assert!(update_codex_app_hooks(&target, &binary, platform, false).unwrap());
            assert!(verify_codex_app_hooks(&target, &binary, platform).unwrap());
            let value: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
            let hook = &value["hooks"]["UserPromptSubmit"][0]["hooks"][0];
            assert!(
                hook["command"]
                    .as_str()
                    .is_some_and(|command| command.contains(binary_name))
            );
            assert_eq!(hook["commandWindows"], "exit 0");
            fs::remove_dir_all(root).unwrap();
        }

        let root = temp_directory();
        let target = root.join("codex-home/hooks.json");
        let binary = PathBuf::from(r"C:\Users\owner\memory-supervisor.exe");
        assert!(update_codex_app_hooks(&target, &binary, "windows", false).unwrap());
        assert!(verify_codex_app_hooks(&target, &binary, "windows").unwrap());
        let value: Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        let hook = &value["hooks"]["UserPromptSubmit"][0]["hooks"][0];
        assert_eq!(hook["command"], "true");
        assert!(
            hook["commandWindows"]
                .as_str()
                .is_some_and(|command| command.contains("memory-supervisor.exe"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_cli_hook_contract_carries_its_authoritative_source() {
        let binary = Path::new("/opt/memory-supervisor");
        let target = Path::new("/home/owner/.codex/hooks.json");
        let desired = desired_hooks("codex", binary, target).unwrap();
        let expected = if cfg!(windows) {
            json!({
                "type":"command",
                "command":"true",
                "commandWindows":"& '/opt/memory-supervisor' gate codex PreToolUse --hook-source '/home/owner/.codex/hooks.json'",
                "timeout":20
            })
        } else {
            json!({
                "type":"command",
                "command":"'/opt/memory-supervisor' gate codex PreToolUse --hook-source '/home/owner/.codex/hooks.json'",
                "commandWindows":"exit 0",
                "timeout":20
            })
        };
        assert_eq!(desired["PreToolUse"][0]["hooks"][0], expected);
    }

    #[test]
    fn codex_windows_hook_uses_the_powershell_call_operator_and_escapes_paths() {
        let binary = Path::new(r"C:\Users\O'Owner\Memory Supervisor\memory-supervisor.exe");
        let target = Path::new(r"C:\Users\O'Owner\.codex\hooks.json");
        assert_eq!(
            codex_windows_command(binary, "SessionStart", target),
            r"& 'C:\Users\O''Owner\Memory Supervisor\memory-supervisor.exe' gate codex SessionStart --hook-source 'C:\Users\O''Owner\.codex\hooks.json'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hook_update_preserves_symlink_target_and_existing_modes() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = temp_directory();
        let target_parent = root.join("private");
        fs::create_dir(&target_parent).unwrap();
        fs::set_permissions(&target_parent, fs::Permissions::from_mode(0o750)).unwrap();
        let target = target_parent.join("settings.json");
        fs::write(&target, b"{}").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let link = root.join("settings-link.json");
        symlink(&target, &link).unwrap();

        update_hooks(&link, "claude", &root.join("memory-supervisor"), false).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            fs::metadata(&target_parent).unwrap().permissions().mode() & 0o777,
            0o750
        );
        fs::remove_dir_all(root).unwrap();
    }
}
