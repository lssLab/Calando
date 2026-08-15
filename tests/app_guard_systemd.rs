#![cfg(target_os = "linux")]

use memory_supervisor::app_guard::{launch_independent, phase_path};
use memory_supervisor::codex_app::is_codex_app_server;
use memory_supervisor::platform::{list_processes, process_by_pid, process_state, resume_process};
use memory_supervisor::policy::process_identity;
use memory_supervisor::storage::{ensure_private_dir, write_atomic_json};
use serde_json::json;
use std::ffi::OsString;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct ProcessGuard(Child);

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let pid = self.0.id();
        let _ = resume_process("linux", pid);
        if let Ok(process_group) = i32::try_from(pid) {
            // The fixture owns a separate test-only session, including its sleeping child.
            unsafe {
                libc::kill(-process_group, libc::SIGCONT);
                libc::kill(-process_group, libc::SIGKILL);
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn transient_systemd_guard_owns_the_pause_and_resume_outside_the_callers_cgroup() {
    let systemd_ready = Command::new("systemd-run")
        .args(["--user", "--quiet", "--wait", "--collect", "/bin/true"])
        .status()
        .is_ok_and(|status| status.success());
    if !systemd_ready {
        eprintln!("skipped: no user systemd manager is available on this Linux runner");
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "memory-supervisor-systemd-guard-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    ensure_private_dir(&root).unwrap();
    ensure_private_dir(&root.join("app-guards")).unwrap();
    let mut host_command = Command::new("bash");
    host_command
        .args([
            "-c",
            r#"exec -a codex bash -c 'while :; do sleep 60; done' app-server"#,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        host_command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut host = ProcessGuard(host_command.spawn().unwrap());
    let pid = host.0.id();
    assert!(wait_until(Duration::from_secs(2), || {
        process_by_pid("linux", pid)
            .as_ref()
            .is_some_and(is_codex_app_server)
    }));
    let identity = process_identity(&process_by_pid("linux", pid).unwrap());
    let incident_id = format!("systemd-guard-{pid}");
    let runtime = root.join("runtime.json");
    write_atomic_json(
        &runtime,
        &json!({
            "stopped": {pid.to_string(): identity},
            "incidents": [{
                "id": incident_id,
                "status": "suspended",
                "pid": pid,
                "identity": identity,
                "app_control_scope": "shared-host",
                "control_phase": "prepared"
            }]
        }),
        0o600,
        true,
    )
    .unwrap();
    let control_base = root.join("app-guards/integration");
    let arguments = [
        OsString::from("app-resume-guard"),
        OsString::from(pid.to_string()),
        OsString::from(identity.clone()),
        OsString::from(incident_id.clone()),
        OsString::from("500"),
        runtime.as_os_str().to_owned(),
        OsString::from("linux"),
        OsString::from(pid.to_string()),
        OsString::from("shared-host"),
        control_base.as_os_str().to_owned(),
    ];
    launch_independent(
        std::path::Path::new(env!("CARGO_BIN_EXE_memory-supervisor")),
        &arguments,
        "linux",
        pid,
    )
    .unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        phase_path(&control_base, "armed").is_file()
    }));

    let guard_pid = list_processes("linux")
        .values()
        .find(|process| process.args.iter().any(|argument| argument == &incident_id))
        .map(|process| process.pid)
        .expect("independent guard process");
    let caller_cgroup = fs::read_to_string("/proc/self/cgroup").unwrap();
    let guard_cgroup = fs::read_to_string(format!("/proc/{guard_pid}/cgroup")).unwrap();
    assert_ne!(caller_cgroup, guard_cgroup);
    assert!(guard_cgroup.contains("memory-supervisor-app-guard"));

    fs::rename(
        phase_path(&control_base, "armed"),
        phase_path(&control_base, "committed"),
    )
    .unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        process_state("linux", pid).starts_with('T')
    }));
    assert!(wait_until(Duration::from_secs(3), || {
        !process_state("linux", pid).starts_with('T')
            && phase_path(&control_base, "resumed").is_file()
    }));

    let _ = host.0.kill();
    let _ = host.0.wait();
    fs::remove_dir_all(root).unwrap();
}
