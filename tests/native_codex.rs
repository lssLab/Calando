use memory_supervisor::integration::inspect_codex;
use memory_supervisor::platform::{
    list_processes, platform_name, process_state, resume_process, suspend_process,
    terminate_process,
};
use memory_supervisor::policy::target_provider;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
#[cfg(windows)]
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn find_command(name: &str) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path).find_map(|directory| {
            extensions
                .iter()
                .map(|extension| directory.join(format!("{name}{extension}")))
                .find(|candidate| candidate.is_file())
        })
    })
}

fn spawn_codex(command: &OsStr) -> Child {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| OsString::from("cmd.exe")))
            .args(["/d", "/c"])
            .arg(command)
            .arg("mcp-server")
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
    #[cfg(not(windows))]
    {
        Command::new(command)
            .arg("mcp-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
}

fn ancestors(
    mut pid: u32,
    processes: &BTreeMap<u32, memory_supervisor::policy::ProcessInfo>,
) -> Vec<u32> {
    let mut result = Vec::new();
    while let Some(process) = processes.get(&pid) {
        if result.len() >= 64 || process.ppid <= 1 {
            break;
        }
        pid = process.ppid;
        result.push(pid);
    }
    result
}

fn expected_stopped(platform: &str, state: &str) -> bool {
    let _ = platform;
    state.to_uppercase().starts_with('T')
}

fn expected_running(platform: &str, state: &str) -> bool {
    if platform == "windows" {
        state == "R"
    } else {
        !expected_stopped(platform, state) && !matches!(state, "missing" | "unknown")
    }
}

fn process_diagnostic(
    baseline: &BTreeSet<u32>,
    launcher_pid: u32,
    processes: &BTreeMap<u32, memory_supervisor::policy::ProcessInfo>,
) -> String {
    processes
        .values()
        .filter(|process| {
            !baseline.contains(&process.pid)
                && (process.pid == launcher_pid
                    || ancestors(process.pid, processes).contains(&launcher_pid))
        })
        .map(|process| {
            format!(
                "pid={} ppid={} name={:?} args={:?}",
                process.pid, process.ppid, process.name, process.args
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn official_codex_process_is_detected_paused_and_resumed_natively() {
    if std::env::var("MEMORY_SUPERVISOR_NATIVE_CODEX_SMOKE").as_deref() != Ok("1") {
        return;
    }
    let command = find_command("codex").expect("official Codex CLI is not installed");
    let (supported, reason) = inspect_codex(command.as_os_str());
    assert!(supported, "{reason}");

    let platform = platform_name();
    let baseline: BTreeSet<_> = list_processes(&platform).into_keys().collect();
    let mut launcher = spawn_codex(command.as_os_str());
    let launcher_pid = launcher.id();
    let mut target_pid = None;
    let mut stopped = false;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            assert!(
                launcher.try_wait().unwrap().is_none(),
                "Codex MCP canary exited early"
            );
            let processes = list_processes(&platform);
            if let Some(process) = processes.values().find(|process| {
                !baseline.contains(&process.pid)
                    && target_provider(process) == Some("codex")
                    && (process.pid == launcher_pid
                        || ancestors(process.pid, &processes).contains(&launcher_pid))
            }) {
                target_pid = Some(process.pid);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let processes = list_processes(&platform);
        let pid = target_pid.unwrap_or_else(|| {
            panic!(
                "native sensor did not find the Codex MCP canary; descendants: {}",
                process_diagnostic(&baseline, launcher_pid, &processes)
            )
        });
        suspend_process(&platform, pid).unwrap();
        stopped = true;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && !expected_stopped(&platform, &process_state(&platform, pid))
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(expected_stopped(&platform, &process_state(&platform, pid)));

        resume_process(&platform, pid).unwrap();
        stopped = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && !expected_running(&platform, &process_state(&platform, pid))
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(expected_running(&platform, &process_state(&platform, pid)));
        assert!(
            launcher.try_wait().unwrap().is_none(),
            "Codex MCP canary exited after resume"
        );
    }));

    if let Some(pid) = target_pid {
        if stopped {
            let _ = resume_process(&platform, pid);
        }
        if pid != launcher_pid {
            let _ = terminate_process(&platform, pid, true);
        }
    }
    let _ = launcher.kill();
    let _ = launcher.wait();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}
