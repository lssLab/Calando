use serde_json::Value;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::codex_app::is_codex_app_server;
use crate::platform::{platform_name, process_by_pid, resume_process, suspend_process};
use crate::policy::process_identity;
use crate::runtime::unique_nonce;
use crate::storage::{ensure_private_dir, write_atomic_json};

#[cfg(windows)]
pub fn controller_process_token(_platform: &str, pid: u32) -> Option<String> {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    // SAFETY: the process handle is opened read-only, all FILETIME buffers are initialized and
    // writable for the call, and the handle is closed on every path after a successful open.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        let succeeded =
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        CloseHandle(handle);
        succeeded.then(|| format!("{:08x}{:08x}", creation.high, creation.low))
    }
}

#[cfg(not(windows))]
pub fn controller_process_token(platform: &str, pid: u32) -> Option<String> {
    process_by_pid(platform, pid).map(|process| process_identity(&process))
}

pub fn phase_path(base: &Path, phase: &str) -> PathBuf {
    base.with_extension(format!("{phase}.json"))
}

fn guard_command(binary: &Path, arguments: &[OsString]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
fn spawn_detached_unix_guard(mut command: Command) -> io::Result<()> {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn().map(|_| ())
}

#[cfg(not(unix))]
fn spawn_detached_unix_guard(_command: Command) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "detached Unix guard is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn spawn_detached_windows_guard(mut command: Command) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    // The Task Scheduler may place the daemon in a job. BREAKAWAY makes spawn fail closed when
    // that job does not permit an independently recoverable child.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    command.spawn().map(|_| ())
}

#[cfg(not(windows))]
fn spawn_detached_windows_guard(_command: Command) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "detached Windows guard is unavailable on this platform",
    ))
}

fn linux_daemon_is_service_managed() -> bool {
    env::var_os("INVOCATION_ID").is_some()
        || fs::read_to_string("/proc/self/cgroup")
            .ok()
            .is_some_and(|cgroups| cgroups.lines().any(|line| line.contains(".service")))
}

/// Starts the one-shot guard outside the daemon's service lifetime. Failure is returned to the
/// caller before the two-phase signal token can be committed.
pub fn launch_independent(
    binary: &Path,
    arguments: &[OsString],
    platform: &str,
    target_pid: u32,
) -> io::Result<()> {
    match platform {
        "windows" => spawn_detached_windows_guard(guard_command(binary, arguments)),
        "darwin" | "macos" => {
            let label = format!(
                "io.github.lsslab.memory-supervisor.guard.{target_pid}.{}",
                unique_nonce()
            );
            Command::new("launchctl")
                .arg("submit")
                .arg("-l")
                .arg(label)
                .arg("--")
                .arg(binary)
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .and_then(|status| {
                    status
                        .success()
                        .then_some(())
                        .ok_or_else(|| io::Error::other(format!("launchctl exited with {status}")))
                })
        }
        "linux" | "wsl" => {
            let unit = format!(
                "memory-supervisor-app-guard-{target_pid}-{}",
                unique_nonce()
            );
            let service = Command::new("systemd-run")
                .arg("--user")
                .arg("--quiet")
                .arg("--collect")
                .arg("--service-type=exec")
                .arg(format!("--unit={unit}"))
                .arg(binary)
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match service {
                Ok(status) if status.success() => Ok(()),
                // A daemon owned by systemd must never fall back into its own kill domain.
                // The fallback daemon has no service cgroup, so a new session is independent
                // from its parent process and terminal.
                failure if !linux_daemon_is_service_managed() => {
                    spawn_detached_unix_guard(guard_command(binary, arguments)).map_err(
                        |fallback| {
                            io::Error::other(format!(
                                "systemd transient unit unavailable ({failure:?}); detached fallback failed: {fallback}"
                            ))
                        },
                    )
                }
                Ok(status) => Err(io::Error::other(format!(
                    "systemd-run exited with {status}"
                ))),
                Err(error) => Err(error),
            }
        }
        platform => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("independent guard is unavailable on {platform}"),
        )),
    }
}

fn write_phase(base: &Path, phase: &str, pid: u32, incident_id: &str, detail: &str) -> bool {
    let controller_pid = std::process::id();
    let controller_identity =
        controller_process_token(&platform_name(), controller_pid).unwrap_or_default();
    write_atomic_json(
        &phase_path(base, phase),
        &serde_json::json!({
            "phase": phase,
            "pid": pid,
            "controller_pid": controller_pid,
            "controller_identity": controller_identity,
            "incident_id": incident_id,
            "detail": detail,
        }),
        0o600,
        true,
    )
    .is_ok()
}

fn transition_control_phase(
    base: &Path,
    from: &str,
    to: &str,
    pid: u32,
    incident_id: &str,
    detail: &str,
) -> bool {
    let destination = phase_path(base, to);
    if fs::rename(phase_path(base, from), &destination).is_err() && !destination.is_file() {
        return false;
    }
    // The rename revokes the signalling token atomically. Replacing its content with the exact
    // terminal phase is best effort; the controller relies on the destination's existence.
    let _ = write_phase(base, to, pid, incident_id, detail);
    true
}

fn matching_pause_state(
    state: &Value,
    pid: u32,
    identity: &str,
    incident_id: &str,
    scope: &str,
) -> bool {
    let still_managed = state
        .get("stopped")
        .and_then(|stopped| stopped.get(pid.to_string()))
        .and_then(Value::as_str)
        == Some(identity);
    still_managed
        && state
            .get("incidents")
            .and_then(Value::as_array)
            .is_some_and(|incidents| {
                incidents.iter().any(|incident| {
                    incident.get("id").and_then(Value::as_str) == Some(incident_id)
                        && incident.get("status").and_then(Value::as_str) == Some("suspended")
                        && incident.get("pid").and_then(Value::as_u64) == Some(u64::from(pid))
                        && incident.get("identity").and_then(Value::as_str) == Some(identity)
                        && incident.get("app_control_scope").and_then(Value::as_str) == Some(scope)
                        && matches!(
                            incident.get("control_phase").and_then(Value::as_str),
                            Some("prepared" | "committed" | "active")
                        )
                })
            })
}

fn exact_shared_host(platform: &str, pid: u32, identity: &str, app_server_pid: u32) -> bool {
    if pid != app_server_pid {
        return false;
    }
    process_by_pid(platform, pid).is_some_and(|process| {
        process_identity(&process) == identity && is_codex_app_server(&process)
    })
}

/// Hidden one-shot controller for the shared app-server brake. A platform service manager launches
/// it outside the daemon's service lifetime. It owns both the suspend and the timed resume signal,
/// so there is no interval in which the daemon can issue a late pause after its guard has exited.
pub fn run_resume_guard(arguments: &[OsString]) -> i32 {
    if arguments.len() != 9 {
        return 2;
    }
    let Some(pid) = arguments[0]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 1)
    else {
        return 2;
    };
    let Some(identity) = arguments[1].to_str().map(str::to_owned) else {
        return 2;
    };
    let Some(incident_id) = arguments[2]
        .to_str()
        .map(str::to_owned)
        .filter(|value| !value.is_empty() && value.len() <= 512)
    else {
        return 2;
    };
    let Some(delay_ms) = arguments[3]
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|delay| (250..=300_000).contains(delay))
    else {
        return 2;
    };
    let runtime = PathBuf::from(&arguments[4]);
    let Some(platform) = arguments[5].to_str().map(str::to_owned) else {
        return 2;
    };
    let Some(app_server_pid) = arguments[6]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 1)
    else {
        return 2;
    };
    let Some(scope) = arguments[7]
        .to_str()
        .filter(|scope| *scope == "shared-host")
        .map(str::to_owned)
    else {
        return 2;
    };
    let control_base = PathBuf::from(&arguments[8]);
    let Some(runtime_parent) = runtime.parent() else {
        return 2;
    };
    let control_directory = runtime_parent.join("app-guards");
    if control_base.parent() != Some(control_directory.as_path())
        || ensure_private_dir(&control_directory).is_err()
    {
        return 2;
    }

    let state: Value = match fs::read(&runtime)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(state) => state,
        None => return 3,
    };
    if !matching_pause_state(&state, pid, &identity, &incident_id, &scope)
        || !exact_shared_host(&platform, pid, &identity, app_server_pid)
    {
        return 0;
    }
    if !write_phase(&control_base, "armed", pid, &incident_id, "ready") {
        return 3;
    }

    let arm_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if phase_path(&control_base, "committed").is_file() {
            break;
        }
        if ["cancelled", "expired", "error", "completed"]
            .into_iter()
            .any(|phase| phase_path(&control_base, phase).is_file())
        {
            return 0;
        }
        let current_state = fs::read(&runtime)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if current_state
            .as_ref()
            .is_none_or(|state| !matching_pause_state(state, pid, &identity, &incident_id, &scope))
        {
            if !transition_control_phase(
                &control_base,
                "armed",
                "cancelled",
                pid,
                &incident_id,
                "state-revoked",
            ) {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            return 0;
        }
        if Instant::now() >= arm_deadline {
            let expired = phase_path(&control_base, "expired");
            if fs::rename(phase_path(&control_base, "armed"), &expired).is_ok() {
                return 0;
            }
            if phase_path(&control_base, "committed").is_file() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }

    let committed_state = fs::read(&runtime)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if committed_state
        .as_ref()
        .is_none_or(|state| !matching_pause_state(state, pid, &identity, &incident_id, &scope))
        || !exact_shared_host(&platform, pid, &identity, app_server_pid)
    {
        let _ = transition_control_phase(
            &control_base,
            "committed",
            "cancelled",
            pid,
            &incident_id,
            "identity-or-scope-changed",
        );
        return 0;
    }
    if fs::rename(
        phase_path(&control_base, "committed"),
        phase_path(&control_base, "signalling"),
    )
    .is_err()
    {
        // The daemon or owner won the token and cancelled this episode. Without the signalling
        // token this controller has no authority to touch the process.
        return 0;
    }
    let _ = write_phase(
        &control_base,
        "signalling",
        pid,
        &incident_id,
        "signal-token-owned",
    );
    if let Err(error) = suspend_process(&platform, pid) {
        let _ = transition_control_phase(
            &control_base,
            "signalling",
            "error",
            pid,
            &incident_id,
            &format!("suspend failed: {error}"),
        );
        return 4;
    }
    if !transition_control_phase(
        &control_base,
        "signalling",
        "suspended",
        pid,
        &incident_id,
        "signal-complete",
    ) {
        // This process owns the signal. If acknowledgement cannot be made durable, release the
        // exact process immediately rather than leave the daemon guessing.
        let release = resume_process(&platform, pid)
            .map(|_| "suspend acknowledgement failed; process released")
            .unwrap_or("suspend acknowledgement and immediate release both failed");
        let _ = transition_control_phase(
            &control_base,
            "signalling",
            "error",
            pid,
            &incident_id,
            release,
        );
        return 3;
    }

    thread::sleep(Duration::from_millis(delay_ms));
    let state = fs::read(&runtime)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let same_episode = state
        .as_ref()
        .is_none_or(|state| matching_pause_state(state, pid, &identity, &incident_id, &scope));
    if !same_episode {
        // A guard from an earlier pause must never release a later pause of the same long-lived
        // App Server identity.
        let _ = write_phase(
            &control_base,
            "completed",
            pid,
            &incident_id,
            "episode-already-ended",
        );
        return 0;
    }
    let Some(process) = process_by_pid(&platform, pid) else {
        let _ = write_phase(
            &control_base,
            "completed",
            pid,
            &incident_id,
            "process-exited",
        );
        return 0;
    };
    if process_identity(&process) != identity {
        let _ = write_phase(
            &control_base,
            "completed",
            pid,
            &incident_id,
            "identity-changed",
        );
        return 0;
    }
    match resume_process(&platform, pid) {
        Ok(()) => {
            let _ = write_phase(&control_base, "resumed", pid, &incident_id, "timeout");
            0
        }
        Err(error) => {
            let _ = write_phase(
                &control_base,
                "error",
                pid,
                &incident_id,
                &format!("resume failed: {error}"),
            );
            4
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::platform::{list_processes, platform_name, process_state, terminate_process};
    use crate::policy::{ProcessInfo, process_identity};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TemporaryRoot(PathBuf);

    impl Drop for TemporaryRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct AppServerFixture {
        child: Child,
        platform: String,
        target_pid: Option<u32>,
    }

    impl Drop for AppServerFixture {
        fn drop(&mut self) {
            let launcher_pid = self.child.id();
            let processes = list_processes(&self.platform);
            let mut owned: Vec<_> = processes
                .keys()
                .copied()
                .filter(|pid| ancestry_depth(*pid, launcher_pid, &processes).is_some())
                .collect();
            if let Some(pid) = self.target_pid
                && !owned.contains(&pid)
            {
                owned.push(pid);
            }
            owned.sort_by_key(|pid| {
                std::cmp::Reverse(ancestry_depth(*pid, launcher_pid, &processes).unwrap_or(0))
            });
            for pid in owned {
                let _ = resume_process(&self.platform, pid);
                let _ = terminate_process(&self.platform, pid, true);
            }
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(_)) | Err(_) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(20)),
                }
            }
            let _ = terminate_process(&self.platform, launcher_pid, true);
        }
    }

    fn command_on_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
        })
    }

    fn ancestry_depth(
        mut pid: u32,
        root_pid: u32,
        processes: &BTreeMap<u32, ProcessInfo>,
    ) -> Option<usize> {
        let mut depth = 0;
        loop {
            if pid == root_pid {
                return Some(depth);
            }
            let process = processes.get(&pid)?;
            if process.ppid <= 1 || depth >= 64 {
                return None;
            }
            pid = process.ppid;
            depth += 1;
        }
    }

    fn wait_for_app_server(fixture: &mut AppServerFixture) -> ProcessInfo {
        let launcher_pid = fixture.child.id();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stable_identity = String::new();
        let mut stable_samples = 0;
        while Instant::now() < deadline {
            if let Some(status) = fixture.child.try_wait().unwrap() {
                panic!("Codex App Server fixture exited early with {status}");
            }
            let processes = list_processes(&fixture.platform);
            let candidate = processes
                .values()
                .filter_map(|process| {
                    ancestry_depth(process.pid, launcher_pid, &processes)
                        .filter(|_| is_codex_app_server(process))
                        .map(|depth| (depth, process))
                })
                .max_by_key(|(depth, _)| *depth)
                .map(|(_, process)| process.clone());
            if let Some(process) = candidate {
                let identity = process_identity(&process);
                if identity == stable_identity {
                    stable_samples += 1;
                } else {
                    stable_identity = identity;
                    stable_samples = 1;
                }
                if stable_samples >= 3 {
                    fixture.target_pid = Some(process.pid);
                    return process;
                }
            } else {
                stable_identity.clear();
                stable_samples = 0;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("native process inventory did not find a stable Codex App Server fixture");
    }

    fn spawn_app_server(platform: &str) -> Option<(AppServerFixture, ProcessInfo)> {
        let require_native =
            std::env::var("MEMORY_SUPERVISOR_NATIVE_CODEX_SMOKE").as_deref() == Ok("1");
        let native_command = command_on_path("codex");
        let child = if require_native || platform == "darwin" {
            let Some(command) = native_command else {
                if require_native {
                    panic!("official Codex CLI is required for the native App Server smoke test");
                }
                return None;
            };
            Command::new(command)
                .arg("app-server")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            // Linux developer tests remain self-contained. CI and macOS use the actual Codex App
            // Server so native process discovery, identity, and signals are exercised together.
            // Keep the classified host in Bash: Python version managers may replace argv[0] when
            // their shim launches the interpreter, making a valid fixture look like plain Python.
            Command::new("bash")
                .args([
                    "-c",
                    r#"exec -a codex bash -c 'while :; do sleep 60; done' app-server"#,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        };
        let mut fixture = AppServerFixture {
            child,
            platform: platform.to_owned(),
            target_pid: None,
        };
        let process = wait_for_app_server(&mut fixture);
        Some((fixture, process))
    }

    fn wait_for_phase(base: &Path, phase: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if phase_path(base, phase).is_file() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("guard did not publish its {phase} phase before the test deadline");
    }

    fn wait_for_stopped_process(platform: &str, pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if process_state(platform, pid).starts_with('T') {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("guard did not stop PID {pid} before its recovery window");
    }

    fn wait_for_running_process(platform: &str, pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            let state = process_state(platform, pid);
            if !state.starts_with('T') && !matches!(state.as_str(), "missing" | "unknown") {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("PID {pid} did not resume before the test deadline");
    }

    fn start_guard(arguments: [OsString; 9]) -> Receiver<i32> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(run_resume_guard(&arguments));
        });
        receiver
    }

    fn wait_for_guard(receiver: Receiver<i32>, base: &Path) -> i32 {
        receiver
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or_else(|error| {
                let phases = [
                    "armed",
                    "committed",
                    "signalling",
                    "suspended",
                    "resumed",
                    "completed",
                    "cancelled",
                    "expired",
                    "error",
                ]
                .into_iter()
                .filter(|phase| phase_path(base, phase).is_file())
                .collect::<Vec<_>>()
                .join(",");
                panic!("guard did not finish before its test deadline ({error}); phases={phases}")
            })
    }

    #[test]
    fn independent_guard_resumes_only_the_matching_persisted_pause_episode() {
        let root = std::env::temp_dir().join(format!(
            "memory-supervisor-app-guard-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("app-guards")).unwrap();
        let _temporary_root = TemporaryRoot(root.clone());
        let platform = platform_name();
        let Some((_fixture, process)) = spawn_app_server(&platform) else {
            return;
        };
        let pid = process.pid;
        let identity = process_identity(&process);
        if platform == "darwin" {
            // The durable phase protocol and delayed recovery are exercised below on Linux. On
            // macOS, validate native inventory and signals directly against the official App
            // Server; combining artificial delayed episodes with the npm-launched CI canary can
            // leave the harness waiting on a deliberately stopped child.
            suspend_process(&platform, pid).unwrap();
            wait_for_stopped_process(&platform, pid);
            resume_process(&platform, pid).unwrap();
            wait_for_running_process(&platform, pid);
            return;
        }
        let runtime = root.join("runtime.json");
        let write_episode = |incident_id: &str| {
            fs::write(
                &runtime,
                serde_json::to_vec(&json!({
                    "stopped": {pid.to_string(): identity.clone()},
                    "incidents": [{
                        "id": incident_id,
                        "status": "suspended",
                        "pid": pid,
                        "identity": identity.clone(),
                        "app_control_scope": "shared-host",
                        "control_phase": "prepared"
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
        };
        write_episode("new-pause");
        let stale_base = root.join("app-guards/stale");
        let stale_result = run_resume_guard(&[
            OsString::from(pid.to_string()),
            OsString::from(identity.clone()),
            OsString::from("old-pause"),
            OsString::from("250"),
            runtime.as_os_str().to_owned(),
            OsString::from(platform.clone()),
            OsString::from(pid.to_string()),
            OsString::from("shared-host"),
            stale_base.as_os_str().to_owned(),
        ]);
        assert_eq!(stale_result, 0);
        assert!(!process_state(&platform, pid).starts_with('T'));

        let control_base = root.join("app-guards/matching");
        let arguments = [
            OsString::from(pid.to_string()),
            OsString::from(identity.clone()),
            OsString::from("new-pause"),
            OsString::from("5000"),
            runtime.as_os_str().to_owned(),
            OsString::from(platform.clone()),
            OsString::from(pid.to_string()),
            OsString::from("shared-host"),
            control_base.as_os_str().to_owned(),
        ];
        let guard = start_guard(arguments);
        wait_for_phase(&control_base, "armed");
        fs::rename(
            phase_path(&control_base, "armed"),
            phase_path(&control_base, "committed"),
        )
        .unwrap();
        wait_for_stopped_process(&platform, pid);
        assert_eq!(wait_for_guard(guard, &control_base), 0);
        assert!(!process_state(&platform, pid).starts_with('T'));

        write_episode("second-pause");
        let second_base = root.join("app-guards/second");
        let arguments = [
            OsString::from(pid.to_string()),
            OsString::from(identity.clone()),
            OsString::from("second-pause"),
            OsString::from("5000"),
            runtime.as_os_str().to_owned(),
            OsString::from(platform.clone()),
            OsString::from(pid.to_string()),
            OsString::from("shared-host"),
            second_base.as_os_str().to_owned(),
        ];
        let second_guard = start_guard(arguments);
        wait_for_phase(&second_base, "armed");
        fs::rename(
            phase_path(&second_base, "armed"),
            phase_path(&second_base, "committed"),
        )
        .unwrap();
        wait_for_stopped_process(&platform, pid);
        write_episode("newer-pause");
        assert_eq!(wait_for_guard(second_guard, &second_base), 0);
        assert!(
            process_state(&platform, pid).starts_with('T'),
            "a stale guard must not release a newer pause of the same process"
        );
        resume_process(&platform, pid).unwrap();

        write_episode("cancel-before-commit");
        let cancelled_base = root.join("app-guards/cancelled");
        let arguments = [
            OsString::from(pid.to_string()),
            OsString::from(identity.clone()),
            OsString::from("cancel-before-commit"),
            OsString::from("250"),
            runtime.as_os_str().to_owned(),
            OsString::from(platform.clone()),
            OsString::from(pid.to_string()),
            OsString::from("shared-host"),
            cancelled_base.as_os_str().to_owned(),
        ];
        let cancelled_guard = start_guard(arguments);
        wait_for_phase(&cancelled_base, "armed");
        fs::rename(
            phase_path(&cancelled_base, "armed"),
            phase_path(&cancelled_base, "cancelled"),
        )
        .unwrap();
        assert_eq!(wait_for_guard(cancelled_guard, &cancelled_base), 0);
        assert!(!process_state(&platform, pid).starts_with('T'));
    }
}
