#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SANDBOX: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    state: PathBuf,
    federation: PathBuf,
    home: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "memory-supervisor-mock-darwin-{}-{}",
            std::process::id(),
            NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed)
        ));
        let state = root.join("state");
        let federation = root.join("federation");
        let home = root.join("home");
        for path in [&state, &federation, &home] {
            fs::create_dir_all(path).unwrap();
        }
        Self {
            root,
            state,
            federation,
            home,
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn mocked_darwin_kernel_signals_publish_the_exact_native_snapshot() {
    let sandbox = Sandbox::new();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shims = manifest.join("tests/mock-darwin/shims");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![shims];
    paths.extend(std::env::split_paths(&inherited_path));
    let path = std::env::join_paths(paths).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_memory-supervisor"))
        .args(["daemon", "--once"])
        .env("PATH", path)
        .env("HOME", &sandbox.home)
        .env("MEMORY_SUPERVISOR_FORCE_PLATFORM", "darwin")
        .env("MEMORY_SUPERVISOR_DIR", &sandbox.state)
        .env("MEMORY_SUPERVISOR_FEDERATION_DIR", &sandbox.federation)
        .env("MEMORY_SUPERVISOR_INSTANCE", "darwin-mock")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state: Value =
        serde_json::from_slice(&fs::read(sandbox.state.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["platform"], "darwin");
    assert_eq!(state["instance"], "darwin-mock");
    assert_eq!(state["mem_available_mb"], 3804);
    assert_eq!(state["psi_some_avg10"], 20.0);
    assert_eq!(state["native_state"], "warning");
    assert_eq!(state["level"], "ORANGE");
    assert_eq!(state["action"], "observe");
    assert_eq!(state["tracked_roots"], 2);
    assert_eq!(state["tracked_children"], 0);
    assert_eq!(state["tracked_total_rss_mb"], 559);

    let terminals: std::collections::BTreeSet<_> = state["tracked_processes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|process| process["terminal"].as_str())
        .collect();
    assert_eq!(
        terminals,
        ["/dev/ttys001", "/dev/ttys003"].into_iter().collect()
    );

    let federation_files: Vec<_> = fs::read_dir(&sandbox.federation)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    assert_eq!(federation_files.len(), 1);
}

#[test]
fn mocked_darwin_pressure_sysctl_failure_degrades_protection() {
    let sandbox = Sandbox::new();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shims = manifest.join("tests/mock-darwin/shims");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![shims];
    paths.extend(std::env::split_paths(&inherited_path));
    let path = std::env::join_paths(paths).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_memory-supervisor"))
        .args(["daemon", "--once"])
        .env("PATH", path)
        .env("HOME", &sandbox.home)
        .env("MEMORY_SUPERVISOR_FORCE_PLATFORM", "darwin")
        .env("MEMORY_SUPERVISOR_DIR", &sandbox.state)
        .env("MEMORY_SUPERVISOR_FEDERATION_DIR", &sandbox.federation)
        .env("MEMORY_SUPERVISOR_INSTANCE", "darwin-pressure-failure")
        .env("MEMORY_SUPERVISOR_TEST_DARWIN_PRESSURE_FAILURE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state: Value =
        serde_json::from_slice(&fs::read(sandbox.state.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["native_state"], "unknown");
    assert_eq!(state["native_confidence"], "low");
    assert_eq!(state["sensor_ok"], false);
    assert!(state["sensor_errors"].get("pressure").is_some());
    assert_eq!(state["action"], "hold");
    assert_eq!(state["admission_level"], "ORANGE");
}
