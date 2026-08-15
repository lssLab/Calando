use serde_json::{Value, json};
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_SANDBOX: AtomicU64 = AtomicU64::new(0);

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    federation: PathBuf,
    config: PathBuf,
    notifications: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "memory-supervisor-{name}-{}-{}",
            std::process::id(),
            NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        let state = root.join("state");
        let federation = root.join("federation");
        let config = root.join("config.json");
        let notifications = root.join("notifications.conf");
        for path in [&home, &state, &federation] {
            fs::create_dir_all(path).unwrap();
        }
        Self {
            root,
            home,
            state,
            federation,
            config,
            notifications,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_memory-supervisor"));
        command
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("MEMORY_SUPERVISOR_DIR", &self.state)
            .env("MEMORY_SUPERVISOR_FEDERATION_DIR", &self.federation)
            .env("MEMORY_SUPERVISOR_CONFIG", &self.config)
            .env("MEMORY_SUPERVISOR_NOTIFICATION_CONFIG", &self.notifications)
            .env("MEMORY_SUPERVISOR_PRETOOL_HOLD_S", "0")
            .env_remove("MEMORY_SUPERVISOR_CLI_HARD_CAP_MB");
        for key in [
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS",
            "MEMORY_SUPERVISOR_DISCORD_WEBHOOK",
            "MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN",
            "MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID",
            "MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID",
            "MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN",
            "MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn run(&self, arguments: &[&str], input: Option<&str>) -> Output {
        let mut command = self.command();
        command
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn release_install_update_refreshes_through_the_public_bootstrap() {
    let sandbox = Sandbox::new("release-update");
    let install_root = sandbox.root.join("release-root");
    fs::create_dir_all(&install_root).unwrap();
    let script = sandbox.root.join(if cfg!(windows) {
        "bootstrap.ps1"
    } else {
        "bootstrap.sh"
    });
    if cfg!(windows) {
        fs::write(
            &script,
            "[IO.File]::WriteAllText((Join-Path $env:MEMORY_SUPERVISOR_INSTALL_ROOT 'refreshed'), 'yes')\n",
        )
        .unwrap();
    } else {
        fs::write(
            &script,
            "#!/bin/sh\nset -eu\nprintf yes > \"$MEMORY_SUPERVISOR_INSTALL_ROOT/refreshed\"\n",
        )
        .unwrap();
    }

    let output = sandbox
        .command()
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", &install_root)
        .env("MEMORY_SUPERVISOR_BOOTSTRAP_FILE", &script)
        .arg("update")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(install_root.join("refreshed")).unwrap(),
        "yes"
    );
}

#[test]
fn status_gate_and_notification_routes_work_through_the_real_binary() {
    let sandbox = Sandbox::new("cli");
    fs::write(
        sandbox.state.join("state.json"),
        serde_json::to_vec(&json!({
            "ts":now_epoch(), "level":"ORANGE", "utilization":"YELLOW",
            "local_admission_level":"ORANGE", "admission_level":"ORANGE",
            "action":"hold", "distress":"pressure", "attribution":"external",
            "mem_available_mb":512, "memory_capacity_mb":8192,
            "stopped_pids":[], "recent_incidents":[]
        }))
        .unwrap(),
    )
    .unwrap();

    let status = sandbox.run(&["status", "--json"], None);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["admission_level"], "ORANGE");

    let gate = sandbox.run(
        &["gate", "PreToolUse"],
        Some(r#"{"session_id":"blackbox","tool_name":"spawn_agent"}"#),
    );
    assert!(gate.status.success(), "{}", stderr(&gate));
    let gate_json: Value = serde_json::from_slice(&gate.stdout).unwrap();
    assert_eq!(
        gate_json["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert!(
        gate_json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("ADMISSION_DEFERRED")
    );
    let second_gate = sandbox.run(
        &["gate", "PreToolUse"],
        Some(r#"{"session_id":"blackbox-2","tool_name":"spawn_agent"}"#),
    );
    assert!(second_gate.status.success(), "{}", stderr(&second_gate));
    let events: Vec<Value> = fs::read_dir(sandbox.state.join("notification-events/pending"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| { event["type"] == "spawn-denial" && event["importance"] == "detail" })
    );

    let routes = sandbox.run(&["control", "notifications", "routes", "none"], None);
    assert!(routes.status.success(), "{}", stderr(&routes));
    let show = sandbox.run(&["control", "notifications", "show"], None);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("routes: hook,terminal"));
    assert!(
        fs::read_to_string(&sandbox.notifications)
            .unwrap()
            .contains("MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS=none")
    );
}

#[test]
fn slow_tick_is_rejected_by_the_real_daemon_freshness_contract() {
    let sandbox = Sandbox::new("slow-tick");
    fs::write(&sandbox.config, br#"{"MEMORY_SUPERVISOR_TICK_S":60}"#).unwrap();
    let daemon = sandbox.run(&["daemon", "--once"], None);
    assert!(daemon.status.success(), "{}", stderr(&daemon));
    let state: Value =
        serde_json::from_slice(&fs::read(sandbox.state.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["logical_control_tick_s"], 1.0);
    assert!(
        state["configuration_error"]
            .as_str()
            .is_some_and(|error| error.contains("must be <= 5"))
    );
}

#[test]
fn connection_status_reports_attention_without_failing_a_healthy_core() {
    let sandbox = Sandbox::new("connections");
    fs::create_dir(sandbox.home.join(".claude")).unwrap();
    fs::write(
        sandbox.state.join("state.json"),
        serde_json::to_vec(&json!({
            "ts":now_epoch(), "level":"GREEN", "sensor_ok":true,
            "stopped_pids":[], "recent_incidents":[]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = sandbox.run(&["status", "--connections", "--json"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["daemon"], "CONNECTED");
    assert_eq!(report["ready"], false);
}

#[test]
fn notification_setup_rejects_empty_ids_before_prompting_for_a_secret() {
    let sandbox = Sandbox::new("empty-notification-ids");
    for arguments in [
        &["control", "notifications", "discord-channel", ""][..],
        &["control", "notifications", "discord-dm", ""][..],
        &["control", "notifications", "telegram", "-"][..],
    ] {
        let output = sandbox.run(arguments, None);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    }
    let webhook = sandbox.run(
        &["control", "notifications", "discord-webhook"],
        Some("https://discord.com/api/webhooks/\n"),
    );
    assert_eq!(webhook.status.code(), Some(1), "{}", stderr(&webhook));
    assert!(!sandbox.notifications.exists());

    fs::write(
        &sandbox.notifications,
        "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS=discord\n",
    )
    .unwrap();
    let test = sandbox.run(&["control", "notifications", "test"], None);
    assert_eq!(test.status.code(), Some(1), "{}", stderr(&test));
    assert!(stdout(&test).contains("discord: not configured"));
}

#[test]
fn control_request_and_acknowledgement_complete_over_the_real_filesystem() {
    let sandbox = Sandbox::new("control");
    fs::write(
        sandbox.state.join("state.json"),
        br#"{"stopped_pids":[4242]}"#,
    )
    .unwrap();
    let control = sandbox.state.join("control");
    let responder = std::thread::spawn(move || {
        for _ in 0..200 {
            if let Ok(entries) = fs::read_dir(&control) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("json") {
                        continue;
                    }
                    let request: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    let request_id = request["request_id"].as_str().unwrap();
                    fs::create_dir_all(control.join("results")).unwrap();
                    fs::write(
                        control.join("results").join(format!("{request_id}.json")),
                        br#"{"ok":true}"#,
                    )
                    .unwrap();
                    return request;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("control request was not created")
    });
    let output = sandbox.run(&["control", "resume", "--timeout", "2"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("completed: resume pid=4242"));
    let request = responder.join().unwrap();
    assert_eq!(request["action"], "resume");
    assert_eq!(request["pid"], 4242);
}

#[cfg(unix)]
#[test]
fn hard_cap_and_secret_notification_setup_are_atomic_and_private() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let sandbox = Sandbox::new("settings");
    let install_root = sandbox.root.join("install-root");
    fs::create_dir(&install_root).unwrap();
    let marker = sandbox.root.join("installer-runs");
    let installer = install_root.join("install.sh");
    fs::write(
        &installer,
        "#!/bin/sh\nprintf 'apply\\n' >> \"$INSTALL_MARKER\"\n",
    )
    .unwrap();
    fs::set_permissions(&installer, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&sandbox.config, br#"{"KEEP":"yes"}"#).unwrap();

    let mut set = sandbox.command();
    let set = set
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", &install_root)
        .env("INSTALL_MARKER", &marker)
        .args(["control", "hard-cap", "set", "2048"])
        .output()
        .unwrap();
    assert!(set.status.success(), "{}", stderr(&set));
    let config: Value = serde_json::from_slice(&fs::read(&sandbox.config).unwrap()).unwrap();
    assert_eq!(config["KEEP"], "yes");
    assert_eq!(config["MEMORY_SUPERVISOR_CLI_HARD_CAP_MB"], 2048.0);

    let mut off = sandbox.command();
    let off = off
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", &install_root)
        .env("INSTALL_MARKER", &marker)
        .args(["control", "hard-cap", "off"])
        .output()
        .unwrap();
    assert!(off.status.success(), "{}", stderr(&off));
    let config: Value = serde_json::from_slice(&fs::read(&sandbox.config).unwrap()).unwrap();
    assert_eq!(config["KEEP"], "yes");
    assert!(config.get("MEMORY_SUPERVISOR_CLI_HARD_CAP_MB").is_none());
    assert_eq!(fs::read_to_string(&marker).unwrap(), "apply\napply\n");

    let webhook = "https://discord.com/api/webhooks/1/private-token";
    let setup = sandbox.run(
        &["control", "notifications", "discord-webhook"],
        Some(&format!("{webhook}\n")),
    );
    assert!(setup.status.success(), "{}", stderr(&setup));
    assert!(!stdout(&setup).contains("private-token"));
    assert!(!stderr(&setup).contains("private-token"));
    let source = fs::read_to_string(&sandbox.notifications).unwrap();
    assert!(source.contains(webhook));
    assert_eq!(
        fs::metadata(&sandbox.notifications).unwrap().mode() & 0o777,
        0o600
    );
    let show = sandbox.run(&["control", "notifications", "show"], None);
    assert!(stdout(&show).contains("webhook configured (secret hidden)"));
    assert!(!stdout(&show).contains("private-token"));

    let invalid = Sandbox::new("invalid-settings");
    fs::write(&invalid.config, "not-json").unwrap();
    let refused = invalid.run(&["control", "hard-cap", "set", "1024"], None);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("nothing changed"));
    assert_eq!(fs::read_to_string(&invalid.config).unwrap(), "not-json");

    fs::create_dir(install_root.join(".git")).unwrap();
    let fake_bin = sandbox.root.join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let git_marker = sandbox.root.join("git-runs");
    write_script(
        &fake_bin.join("git"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$GIT_MARKER\"\n",
    );
    let uninstaller = install_root.join("uninstall.sh");
    fs::write(
        &uninstaller,
        "#!/bin/sh\nprintf 'remove\\n' >> \"$INSTALL_MARKER\"\n",
    )
    .unwrap();
    fs::set_permissions(&uninstaller, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut update = sandbox.command();
    let update = update
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", &install_root)
        .env("INSTALL_MARKER", &marker)
        .env("GIT_MARKER", &git_marker)
        .env("PATH", &path)
        .args(["control", "update"])
        .output()
        .unwrap();
    assert!(update.status.success(), "{}", stderr(&update));
    assert!(
        fs::read_to_string(&git_marker)
            .unwrap()
            .contains("pull --ff-only")
    );

    let mut uninstall = sandbox.command();
    let uninstall = uninstall
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", &install_root)
        .env("INSTALL_MARKER", &marker)
        .args(["control", "uninstall"])
        .output()
        .unwrap();
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    assert!(
        fs::read_to_string(&marker)
            .unwrap()
            .ends_with("apply\nremove\n")
    );
}

#[cfg(unix)]
fn write_script(path: &Path, source: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn shell_green_lease_is_exact_and_every_invalid_lease_uses_rust() {
    let sandbox = Sandbox::new("lease");
    let fake = sandbox.root.join("fake-runtime");
    let marker = sandbox.root.join("slow-path");
    write_script(
        &fake,
        "#!/bin/sh\nprintf 'invoked\\n' >> \"$GATE_MARKER\"\nprintf '{\"slow\":true}'\n",
    );
    let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/gate.sh");
    let lease = sandbox.state.join("admission-green.lease");
    fs::write(
        &lease,
        format!(
            "1\n{}\n{}\n",
            now_epoch() as u64 + 60,
            sandbox.federation.display()
        ),
    )
    .unwrap();

    let invoke = |federation: &Path| {
        let mut command = Command::new("/bin/sh");
        command
            .arg(&hook)
            .arg("PreToolUse")
            .env("HOME", &sandbox.home)
            .env("MEMORY_SUPERVISOR_DIR", &sandbox.state)
            .env("MEMORY_SUPERVISOR_FEDERATION_DIR", federation)
            .env("MEMORY_SUPERVISOR_BINARY", &fake)
            .env("GATE_MARKER", &marker)
            .output()
            .unwrap()
    };
    let fast = invoke(&sandbox.federation);
    assert!(fast.status.success());
    assert!(fast.stdout.is_empty());
    assert!(!marker.exists());

    let wrong_federation = sandbox.root.join("other-federation");
    let slow = invoke(&wrong_federation);
    assert!(slow.status.success());
    assert_eq!(stdout(&slow), r#"{"slow":true}"#);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "invoked\n");

    fs::write(
        &lease,
        format!(
            "1\n{}\n{}\n",
            now_epoch() as u64,
            sandbox.federation.display()
        ),
    )
    .unwrap();
    let expired = invoke(&sandbox.federation);
    assert_eq!(stdout(&expired), r#"{"slow":true}"#);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "invoked\ninvoked\n");
}

#[cfg(unix)]
#[test]
fn expired_green_lease_cannot_hide_a_red_peer_from_the_real_rust_gate() {
    let sandbox = Sandbox::new("peer-lease");
    let pointer_directory = sandbox.home.join(".memory-supervisor");
    fs::create_dir_all(&pointer_directory).unwrap();
    fs::write(
        pointer_directory.join("federation-dir"),
        format!("{}\n", sandbox.federation.display()),
    )
    .unwrap();
    fs::write(
        sandbox.state.join("state.json"),
        serde_json::to_vec(&json!({
            "ts":now_epoch(), "level":"GREEN", "admission_level":"GREEN",
            "mem_available_mb":4096, "memory_capacity_mb":8192,
            "leak_suspects":[], "stopped_pids":[], "recent_incidents":[]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        sandbox.federation.join("pointer-peer.json"),
        serde_json::to_vec(&json!({
            "ts":now_epoch(), "instance":"pointer-peer", "level":"RED",
            "admission_level":"RED", "action":"drain", "mem_available_mb":256,
            "memory_capacity_mb":8192, "leak_suspects":[], "stopped_pids":[],
            "recent_incidents":[]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        sandbox.state.join("admission-green.lease"),
        format!(
            "1\n{}\n{}\n",
            now_epoch() as u64,
            sandbox.federation.display()
        ),
    )
    .unwrap();

    let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/gate.sh");
    let mut command = Command::new("/bin/sh");
    command
        .arg(hook)
        .arg("PreToolUse")
        .env("HOME", &sandbox.home)
        .env("MEMORY_SUPERVISOR_DIR", &sandbox.state)
        .env_remove("MEMORY_SUPERVISOR_FEDERATION_DIR")
        .env("MEMORY_SUPERVISOR_CONFIG", &sandbox.config)
        .env(
            "MEMORY_SUPERVISOR_BINARY",
            env!("CARGO_BIN_EXE_memory-supervisor"),
        )
        .env("MEMORY_SUPERVISOR_PRETOOL_HOLD_S", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_name":"spawn_agent","session_id":"peer-test"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let decision: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decision["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        decision["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("pointer-peer")
    );
}
