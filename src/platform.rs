use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::codex_app::APP_SERVER_SURFACE;
#[cfg(not(windows))]
use crate::codex_app::is_codex_app_server;
use crate::config::{Config, expand_user};
use crate::policy::{Level, MemorySnapshot, NativePressure, ProcessInfo, target_provider};

#[cfg(target_os = "macos")]
const DARWIN_SYSCTL: &str = "/usr/sbin/sysctl";
#[cfg(not(target_os = "macos"))]
const DARWIN_SYSCTL: &str = "sysctl";

fn darwin_sysctl() -> &'static str {
    if env::var_os("MEMORY_SUPERVISOR_FORCE_PLATFORM").is_some() {
        "sysctl"
    } else {
        DARWIN_SYSCTL
    }
}

#[derive(Debug, Default, Clone)]
pub struct SensorHealth {
    pub errors: BTreeMap<String, String>,
    pub last_process_scan_ts: Option<f64>,
}

impl SensorHealth {
    pub fn sensor_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

static SENSOR_HEALTH: OnceLock<Mutex<SensorHealth>> = OnceLock::new();

#[derive(Default)]
struct WindowsProcessCache {
    processes: BTreeMap<u32, ProcessInfo>,
    last_attempt: Option<Instant>,
}

static WINDOWS_PROCESS_CACHE: OnceLock<Mutex<WindowsProcessCache>> = OnceLock::new();

fn health() -> &'static Mutex<SensorHealth> {
    SENSOR_HEALTH.get_or_init(|| Mutex::new(SensorHealth::default()))
}

fn sensor_success(name: &str) {
    if let Ok(mut health) = health().lock() {
        health.errors.remove(name);
    }
}

fn sensor_failure(name: &str, error: impl std::fmt::Display) {
    if let Ok(mut health) = health().lock() {
        health.errors.insert(
            name.to_owned(),
            error.to_string().chars().take(240).collect(),
        );
    }
}

pub fn sensor_health() -> SensorHealth {
    health()
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn platform_name() -> String {
    if let Ok(forced) = env::var("MEMORY_SUPERVISOR_FORCE_PLATFORM")
        && !forced.is_empty()
    {
        return forced.to_lowercase();
    }
    if cfg!(target_os = "macos") {
        return "darwin".to_owned();
    }
    if cfg!(windows) {
        return "windows".to_owned();
    }
    if fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .to_lowercase()
        .contains("microsoft")
    {
        "wsl".to_owned()
    } else {
        "linux".to_owned()
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn command_output(
    program: impl AsRef<OsStr>,
    arguments: &[&str],
    timeout_s: u64,
) -> io::Result<String> {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("command stdout unavailable"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("command stderr unavailable"))?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = stdout_reader
                .join()
                .map_err(|_| io::Error::other("command stdout reader panicked"))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| io::Error::other("command stderr reader panicked"))??;
            if !status.success() {
                return Err(io::Error::other(String::from_utf8_lossy(&stderr)));
            }
            return Ok(String::from_utf8_lossy(&stdout).into_owned());
        }
        if started.elapsed() >= Duration::from_secs(timeout_s) {
            let _ = child.kill();
            let _ = child.wait();
            // A descendant can inherit a pipe after its parent is killed. Joining either reader
            // here would turn a bounded sensor timeout into an unbounded daemon hang.
            drop(stdout_reader);
            drop(stderr_reader);
            return Err(io::Error::new(io::ErrorKind::TimedOut, "command timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wsl_windows_home() -> Option<PathBuf> {
    if platform_name() != "wsl" {
        return None;
    }
    let mut candidates = fs::read_dir("/mnt/c/Users")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(".memory-supervisor").join("instances").is_dir());
    let first = candidates.next();
    if first.is_some() && candidates.next().is_none() {
        return first;
    }
    let output = command_output(
        "/mnt/c/Windows/System32/cmd.exe",
        &["/c", "echo %USERPROFILE%"],
        5,
    )
    .ok()?;
    let raw = output.trim().trim_matches('"');
    let bytes = raw.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'\\' {
        Some(PathBuf::from(format!(
            "/mnt/{}/{}",
            (bytes[0] as char).to_ascii_lowercase(),
            raw[3..].replace('\\', "/")
        )))
    } else {
        None
    }
}

pub fn federation_dir() -> PathBuf {
    if let Some(path) = env::var_os("MEMORY_SUPERVISOR_FEDERATION_DIR") {
        return expand_user(PathBuf::from(path));
    }
    if let Ok(value) =
        fs::read_to_string(home_dir().join(".memory-supervisor").join("federation-dir"))
    {
        let value = value.trim();
        if !value.is_empty() {
            return expand_user(value);
        }
    }
    if let Some(home) = wsl_windows_home() {
        return home.join(".memory-supervisor").join("instances");
    }
    if crate::topology::detect() == crate::topology::Topology::VmDynamic
        && let Some(shared) = vm_shared_folder()
    {
        return shared.join(".memory-supervisor").join("instances");
    }
    home_dir().join(".memory-supervisor").join("instances")
}

/// A dynamic VM federates with its host over the hypervisor's shared folder. Probe the common
/// Parallels mount: `/media/psf/Home` is the host user's home shared into the guest, so it resolves
/// to the same physical `~/.memory-supervisor/instances` the host daemon uses. VMware HGFS and
/// VirtualBox vboxsf name their shares per-config, so those set `MEMORY_SUPERVISOR_FEDERATION_DIR`;
/// absent any channel the daemon stays self-only (safe — no wrong cross-kernel action).
fn vm_shared_folder() -> Option<PathBuf> {
    ["/media/psf/Home", "/media/psf"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
}

pub fn fresh_federated_states(max_age_s: f64) -> Vec<Value> {
    fresh_federated_states_at(&federation_dir(), now_epoch(), max_age_s)
}

fn fresh_federated_states_at(directory: &Path, now: f64, max_age_s: f64) -> Vec<Value> {
    // Co-residency guard: a network-mounted rendezvous directory is not proof that peers share this
    // machine's physical RAM, so never federate across it. WSL2's 9p `/mnt/c` is host-local and
    // passes; NFS/SMB does not. Failing to stat resolves to host-local, so a legitimate peer is
    // never silently dropped.
    federated_states_from(
        directory,
        now,
        max_age_s,
        crate::topology::channel_is_host_local(directory),
    )
}

fn federated_states_from(
    directory: &Path,
    now: f64,
    max_age_s: f64,
    host_local: bool,
) -> Vec<Value> {
    if !host_local {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut states: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|value| value == "json")
        })
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|source| serde_json::from_slice::<Value>(&source).ok())
        .filter(|state| {
            let Some(object) = state.as_object() else {
                return false;
            };
            let age = now - object.get("ts").and_then(Value::as_f64).unwrap_or_default();
            (-5.0..=max_age_s).contains(&age)
                && object.get("error").is_none_or(Value::is_null)
                && matches!(
                    object.get("level").and_then(Value::as_str),
                    Some("GREEN" | "YELLOW" | "ORANGE" | "RED")
                )
        })
        .collect();
    states.sort_by(|left, right| {
        let left = left.get("ts").and_then(Value::as_f64).unwrap_or_default();
        let right = right.get("ts").and_then(Value::as_f64).unwrap_or_default();
        right.total_cmp(&left)
    });
    states.truncate(64);
    states
}

fn parse_level(value: Option<&str>) -> Option<Level> {
    match value {
        Some("GREEN") => Some(Level::Green),
        Some("YELLOW") => Some(Level::Yellow),
        Some("ORANGE") => Some(Level::Orange),
        Some("RED") => Some(Level::Red),
        _ => None,
    }
}

fn action_as_level(action: Option<&str>) -> Option<Level> {
    match action {
        Some("allow") => Some(Level::Green),
        Some("observe") => Some(Level::Yellow),
        Some("hold") => Some(Level::Orange),
        Some("drain") => Some(Level::Red),
        _ => None,
    }
}

pub fn admission_level_for_state(state: &Value) -> Level {
    let mut level = parse_level(state.get("admission_level").and_then(Value::as_str))
        .or_else(|| parse_level(state.get("level").and_then(Value::as_str)))
        .unwrap_or(Level::Green);
    if let Some(action_level) = action_as_level(state.get("action").and_then(Value::as_str)) {
        level = level.max(action_level);
    }
    // A paused PID or probation is already priced into the measurements and reported through
    // incidents; it no longer pins admission by itself. Degraded protection still does, because
    // an unmeasurable machine cannot certify safe expansion.
    if state
        .get("protection_degraded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        level = level.max(Level::Orange);
    }
    level
}

/// Federation shares pressure, not a kernel's opt-in budget: when a peer's elevation is driven
/// only by its own CLI cap (`cli_hard_cap_driving`), its underlying adaptive action is used so
/// cap proximity in one kernel cannot close fan-out everywhere else.
pub fn admission_level_for_peer(state: &Value) -> Level {
    if !state
        .get("cli_hard_cap_driving")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return admission_level_for_state(state);
    }
    let mut level = action_as_level(
        state
            .get("adaptive_action")
            .and_then(Value::as_str)
            .or_else(|| state.get("action").and_then(Value::as_str)),
    )
    .unwrap_or(Level::Green);
    if state
        .get("protection_degraded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        level = level.max(Level::Orange);
    }
    level
}

pub fn merge_federated_incidents(states: &[Value]) -> Vec<Value> {
    let mut merged = BTreeMap::<(String, String), Value>::new();
    for state in states {
        let source = state
            .get("instance")
            .or_else(|| state.get("platform"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let Some(incidents) = state.get("recent_incidents").and_then(Value::as_array) else {
            continue;
        };
        for raw in incidents {
            let Some(mut incident) = raw.as_object().cloned() else {
                continue;
            };
            let Some(id) = incident
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            incident
                .entry("source".to_owned())
                .or_insert_with(|| Value::String(source.to_owned()));
            let incident_source = incident["source"].as_str().unwrap_or(source).to_owned();
            let candidate = Value::Object(incident);
            let key = (incident_source, id);
            let candidate_at = candidate
                .get("updated_at")
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let replace = merged.get(&key).is_none_or(|previous| {
                candidate_at
                    >= previous
                        .get("updated_at")
                        .and_then(Value::as_f64)
                        .unwrap_or_default()
            });
            if replace {
                merged.insert(key, candidate);
            }
        }
    }
    let mut incidents: Vec<_> = merged.into_values().collect();
    incidents.sort_by(|left, right| {
        left.get("updated_at")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            .total_cmp(
                &right
                    .get("updated_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
            )
    });
    if incidents.len() > 128 {
        incidents.drain(..incidents.len() - 128);
    }
    incidents
}

fn capacity_override(
    config: &mut Config,
    available_mb: u64,
    capacity_mb: u64,
    source: impl Into<String>,
) -> MemorySnapshot {
    let name = "MEMORY_SUPERVISOR_CAPACITY_MB";
    let mut capacity = capacity_mb.max(1);
    let mut source = source.into();
    if config
        .setting(name)
        .is_some_and(|value| value.as_str() != Some(""))
    {
        let parsed = config.validated_number(name, capacity as f64, Some(1.0), None);
        if !config.has_validation_error(name) {
            capacity = parsed as u64;
            source = "override:MEMORY_SUPERVISOR_CAPACITY_MB".to_owned();
            sensor_success("capacity");
        } else {
            sensor_failure("capacity", "invalid MEMORY_SUPERVISOR_CAPACITY_MB");
        }
    } else {
        config.clear_validation_error(name);
    }
    MemorySnapshot {
        available_mb: available_mb.min(capacity),
        capacity_mb: capacity,
        capacity_source: source,
    }
}

pub fn memory_snapshot(platform: &str, config: &mut Config) -> MemorySnapshot {
    match platform {
        "windows" => windows_memory_snapshot(config),
        "darwin" => darwin_memory_snapshot(config),
        _ => linux_memory_snapshot(config),
    }
}

fn linux_memory_snapshot(config: &mut Config) -> MemorySnapshot {
    let source = match fs::read_to_string("/proc/meminfo") {
        Ok(source) => source,
        Err(error) => {
            sensor_failure("memory", error);
            return capacity_override(config, 0, 8192, "fallback:8GiB");
        }
    };
    let mut values = BTreeMap::new();
    for line in source.lines() {
        if let Some((key, value)) = line.split_once(':')
            && matches!(key, "MemAvailable" | "MemTotal")
            && let Some(value) = value.split_whitespace().next()
            && let Ok(value) = value.parse::<u64>()
        {
            values.insert(key, value / 1024);
        }
    }
    let (Some(&mut_available), Some(&mut_capacity)) =
        (values.get("MemAvailable"), values.get("MemTotal"))
    else {
        sensor_failure(
            "memory",
            "MemAvailable or MemTotal missing from /proc/meminfo",
        );
        return capacity_override(config, 0, 8192, "fallback:8GiB");
    };
    let mut available = mut_available;
    let mut capacity = mut_capacity;
    let mut source = "/proc/meminfo:MemTotal".to_owned();
    if let Some((limit, remaining, version)) = linux_cgroup_memory(
        capacity,
        Path::new("/proc/self/cgroup"),
        Path::new("/proc/self/mountinfo"),
    ) {
        capacity = capacity.min(limit);
        available = available.min(remaining);
        source = format!("cgroup-{version}:memory.limit");
    }
    sensor_success("memory");
    sensor_success("capacity");
    capacity_override(config, available, capacity, source)
}

fn unescape_mount_path(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn cgroup_directory(mount_root: &str, mount_point: &str, group_path: &str) -> Option<PathBuf> {
    let root = unescape_mount_path(mount_root);
    let root = root.trim_end_matches('/');
    let root = if root.is_empty() { "/" } else { root };
    let group = group_path.trim_end_matches('/');
    let group = if group.is_empty() { "/" } else { group };
    let relative = if group == root {
        ""
    } else if root == "/" {
        group.trim_start_matches('/')
    } else {
        group.strip_prefix(&format!("{root}/"))?
    };
    Some(Path::new(&unescape_mount_path(mount_point)).join(relative))
}

fn linux_cgroup_memory(
    physical_mb: u64,
    membership_path: &Path,
    mountinfo_path: &Path,
) -> Option<(u64, u64, &'static str)> {
    let memberships = fs::read_to_string(membership_path).ok()?;
    let mountinfo = fs::read_to_string(mountinfo_path).ok()?;
    let mut candidates = Vec::new();
    for membership in memberships.lines() {
        let parts: Vec<_> = membership.splitn(3, ':').collect();
        if parts.len() != 3 {
            continue;
        }
        let controllers: Vec<_> = parts[1].split(',').collect();
        if parts[0] == "0" && parts[1].is_empty() {
            candidates.push(("2", parts[2], "memory.max", "memory.current"));
        } else if controllers.contains(&"memory") {
            candidates.push((
                "1",
                parts[2],
                "memory.limit_in_bytes",
                "memory.usage_in_bytes",
            ));
        }
    }
    let physical_bytes = physical_mb.saturating_mul(1024 * 1024);
    for (version, group_path, limit_name, usage_name) in candidates {
        for line in mountinfo.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            let Some(separator) = fields.iter().position(|value| *value == "-") else {
                continue;
            };
            let fs_type = fields.get(separator + 1).copied().unwrap_or_default();
            let super_options = fields
                .get(separator + 3)
                .copied()
                .unwrap_or_default()
                .split(',');
            if (version == "2" && fs_type != "cgroup2")
                || (version == "1"
                    && (fs_type != "cgroup" || !super_options.clone().any(|v| v == "memory")))
            {
                continue;
            }
            let (Some(root), Some(mount_point)) = (fields.get(3), fields.get(4)) else {
                continue;
            };
            let Some(mut directory) = cgroup_directory(root, mount_point, group_path) else {
                continue;
            };
            let mount_point = PathBuf::from(unescape_mount_path(mount_point));
            let mut limits = Vec::new();
            loop {
                if let (Ok(raw_limit), Ok(raw_usage)) = (
                    fs::read_to_string(directory.join(limit_name)),
                    fs::read_to_string(directory.join(usage_name)),
                ) && raw_limit.trim() != "max"
                    && let (Ok(limit), Ok(usage)) = (
                        raw_limit.trim().parse::<u64>(),
                        raw_usage.trim().parse::<u64>(),
                    )
                    && limit > 0
                    && limit < physical_bytes
                {
                    limits.push((limit, limit.saturating_sub(usage)));
                }
                if directory == mount_point || !directory.starts_with(&mount_point) {
                    break;
                }
                let Some(parent) = directory.parent() else {
                    break;
                };
                directory = parent.to_path_buf();
            }
            if !limits.is_empty() {
                return Some((
                    limits.iter().map(|value| value.0).min()? / (1024 * 1024),
                    limits.iter().map(|value| value.1).min()? / (1024 * 1024),
                    version,
                ));
            }
        }
    }
    None
}

fn parse_vm_stat(output: &str) -> BTreeMap<String, u64> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            value
                .trim()
                .trim_end_matches('.')
                .parse()
                .ok()
                .map(|value| (key.trim().to_owned(), value))
        })
        .collect()
}

fn vm_page_size(output: &str) -> u64 {
    output
        .split_once("page size of")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or(4096)
}

fn darwin_memory_snapshot(config: &mut Config) -> MemorySnapshot {
    let available = match command_output("vm_stat", &[], 3) {
        Ok(output) => {
            let values = parse_vm_stat(&output);
            let pages: u64 = ["Pages free", "Pages inactive", "Pages purgeable"]
                .into_iter()
                .map(|key| values.get(key).copied().unwrap_or_default())
                .sum();
            sensor_success("memory");
            pages.saturating_mul(vm_page_size(&output)) / (1024 * 1024)
        }
        Err(error) => {
            sensor_failure("memory", error);
            0
        }
    };
    match command_output(darwin_sysctl(), &["-n", "hw.memsize"], 3)
        .and_then(|value| value.trim().parse::<u64>().map_err(io::Error::other))
    {
        Ok(bytes) if bytes > 0 => {
            sensor_success("capacity");
            capacity_override(
                config,
                available,
                bytes / (1024 * 1024),
                "sysctl:hw.memsize",
            )
        }
        Ok(_) => {
            sensor_failure("capacity", "hw.memsize was not positive");
            capacity_override(config, available, 8192, "fallback:8GiB")
        }
        Err(error) => {
            sensor_failure("capacity", error);
            capacity_override(config, available, 8192, "fallback:8GiB")
        }
    }
}

pub fn native_pressure(platform: &str) -> NativePressure {
    match platform {
        "windows" => windows_native_pressure(),
        "darwin" => darwin_native_pressure(),
        _ => linux_native_pressure(),
    }
}

fn linux_native_pressure() -> NativePressure {
    let result = (|| -> io::Result<NativePressure> {
        let source = fs::read_to_string("/proc/pressure/memory")?;
        let mut some = 0.0;
        let mut full = 0.0;
        for line in source.lines() {
            let destination = if line.starts_with("some") {
                &mut some
            } else if line.starts_with("full") {
                &mut full
            } else {
                continue;
            };
            if let Some(value) = line
                .split_whitespace()
                .find_map(|field| field.strip_prefix("avg10="))
            {
                *destination = value.parse().map_err(io::Error::other)?;
            }
        }
        let (reclaim, swap, oom) = linux_vm_counters(Path::new("/proc/vmstat"));
        Ok(NativePressure {
            some_avg10: some,
            full_avg10: full,
            reclaim_total: reclaim,
            swap_total: swap,
            oom_total: oom,
            ..NativePressure::default()
        })
    })();
    match result {
        Ok(pressure) => {
            sensor_success("pressure");
            pressure
        }
        Err(error) => {
            sensor_failure("pressure", error);
            NativePressure {
                state: "unknown".to_owned(),
                confidence: "low".to_owned(),
                ..NativePressure::default()
            }
        }
    }
}

fn linux_vm_counters(path: &Path) -> (f64, f64, u64) {
    let mut values = BTreeMap::new();
    if let Ok(source) = fs::read_to_string(path) {
        for line in source.lines() {
            let mut fields = line.split_whitespace();
            let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
                continue;
            };
            if matches!(
                key,
                "pgscan_kswapd" | "pgscan_direct" | "pswpin" | "pswpout" | "oom_kill"
            ) && let Ok(value) = value.parse::<u64>()
            {
                values.insert(key.to_owned(), value);
            }
        }
    }
    #[cfg(unix)]
    let page_bytes = {
        // SAFETY: sysconf is thread-safe and has no pointer arguments.
        let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if value > 0 { value as f64 } else { 4096.0 }
    };
    #[cfg(not(unix))]
    let page_bytes = 4096.0;
    let page_mb = page_bytes / (1024.0 * 1024.0);
    (
        (values.get("pgscan_kswapd").copied().unwrap_or_default()
            + values.get("pgscan_direct").copied().unwrap_or_default()) as f64
            * page_mb,
        (values.get("pswpin").copied().unwrap_or_default()
            + values.get("pswpout").copied().unwrap_or_default()) as f64
            * page_mb,
        values.get("oom_kill").copied().unwrap_or_default(),
    )
}

fn darwin_native_pressure() -> NativePressure {
    match command_output("vm_stat", &[], 3) {
        Ok(output) => match command_output(
            darwin_sysctl(),
            &["-n", "kern.memorystatus_vm_pressure_level"],
            3,
        ) {
            Ok(raw) => {
                sensor_success("pressure");
                darwin_pressure_from_outputs(&raw, &output)
            }
            Err(error) => {
                sensor_failure("pressure", error);
                darwin_pressure_from_outputs("", &output)
            }
        },
        Err(error) => {
            sensor_failure("pressure", error);
            NativePressure {
                state: "unknown".to_owned(),
                confidence: "low".to_owned(),
                ..NativePressure::default()
            }
        }
    }
}

fn darwin_pressure_from_outputs(raw: &str, output: &str) -> NativePressure {
    let (state, pressure, confidence) = match raw.trim() {
        "1" => ("normal", 0.0, "high"),
        "2" => ("warning", 20.0, "high"),
        "4" => ("critical", 60.0, "high"),
        _ => ("unknown", 0.0, "low"),
    };
    let values = parse_vm_stat(output);
    let page_mb = vm_page_size(output) as f64 / (1024.0 * 1024.0);
    let reclaim = ["Pageouts", "Pages purged", "Compressions", "Decompressions"]
        .into_iter()
        .map(|key| values.get(key).copied().unwrap_or_default())
        .sum::<u64>() as f64
        * page_mb;
    let swap = ["Swapins", "Swapouts"]
        .into_iter()
        .map(|key| values.get(key).copied().unwrap_or_default())
        .sum::<u64>() as f64
        * page_mb;
    NativePressure {
        some_avg10: pressure,
        full_avg10: pressure,
        state: state.to_owned(),
        reclaim_total: reclaim,
        swap_total: swap,
        confidence: confidence.to_owned(),
        ..NativePressure::default()
    }
}

pub fn list_processes(platform: &str) -> BTreeMap<u32, ProcessInfo> {
    if platform == "windows" {
        return cached_windows_processes();
    }
    let result = match platform {
        "darwin" => darwin_processes(None),
        _ => linux_processes(),
    };
    if let Ok(mut health) = health().lock() {
        health.last_process_scan_ts = Some(now_epoch());
    }
    if result.is_empty() {
        sensor_failure(
            "processes",
            format!("{platform} process inventory was empty"),
        );
    } else {
        sensor_success("processes");
    }
    result
}

fn refresh_windows_cache(
    cache: &mut WindowsProcessCache,
    now: Instant,
    interval: Duration,
    scan: impl FnOnce() -> BTreeMap<u32, ProcessInfo>,
) -> (BTreeMap<u32, ProcessInfo>, bool, bool) {
    if cache
        .last_attempt
        .is_some_and(|last| now.saturating_duration_since(last) < interval)
    {
        return (cache.processes.clone(), false, !cache.processes.is_empty());
    }
    cache.last_attempt = Some(now);
    let processes = scan();
    if processes.is_empty() {
        return (cache.processes.clone(), true, false);
    }
    cache.processes = processes;
    (cache.processes.clone(), true, true)
}

fn cached_windows_processes() -> BTreeMap<u32, ProcessInfo> {
    let mut config = Config::current();
    let interval = Duration::from_secs_f64(config.validated_number(
        "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S",
        3.0,
        Some(0.25),
        Some(300.0),
    ));
    let (processes, attempted, success) = WINDOWS_PROCESS_CACHE
        .get_or_init(|| Mutex::new(WindowsProcessCache::default()))
        .lock()
        .map(|mut cache| {
            refresh_windows_cache(&mut cache, Instant::now(), interval, || {
                windows_processes(None)
            })
        })
        .unwrap_or_else(|_| (BTreeMap::new(), true, false));
    if attempted {
        if let Ok(mut health) = health().lock() {
            health.last_process_scan_ts = Some(now_epoch());
        }
        if success {
            sensor_success("processes");
        } else {
            sensor_failure("processes", "windows process inventory refresh was empty");
        }
    }
    processes
}

pub fn process_by_pid(platform: &str, pid: u32) -> Option<ProcessInfo> {
    if pid <= 1 {
        return None;
    }
    match platform {
        "windows" => windows_processes(Some(pid)).remove(&pid),
        "darwin" => darwin_processes(Some(pid)).remove(&pid),
        _ => linux_process(pid),
    }
}

#[cfg(windows)]
fn lightweight_process_tree() -> BTreeMap<u32, ProcessInfo> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: Toolhelp returns a read-only snapshot handle owned and closed in this function.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return BTreeMap::new();
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    let mut result = BTreeMap::new();
    // SAFETY: entry has the required size and remains valid for each enumeration call.
    let mut available = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while available {
        let end = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        result.insert(
            entry.th32ProcessID,
            ProcessInfo {
                pid: entry.th32ProcessID,
                ppid: entry.th32ParentProcessID,
                name,
                rss_mb: 0,
                anon_mb: 0,
                args: Vec::new(),
                start_token: String::new(),
                terminal: "console".to_owned(),
                terminal_identity: String::new(),
            },
        );
        // SAFETY: the same live snapshot and initialized entry are reused.
        available = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot is a live handle returned by CreateToolhelp32Snapshot.
    unsafe { CloseHandle(snapshot) };
    result
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderContext {
    pub pid: u32,
    pub surface: String,
    pub descendant_pids: Vec<u32>,
}

fn descendant_pids(processes: &BTreeMap<u32, ProcessInfo>, root_pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    for pid in processes.keys().copied().filter(|pid| *pid != root_pid) {
        let mut current = pid;
        let mut visited = 0;
        while current > 1 && visited < 128 {
            let Some(process) = processes.get(&current) else {
                break;
            };
            current = process.ppid;
            if current == root_pid {
                result.push(pid);
                break;
            }
            visited += 1;
        }
    }
    result.sort_unstable();
    result
}

#[cfg(windows)]
fn published_codex_app_pid(pid: u32) -> bool {
    fs::read(crate::config::state_dir().join("state.json"))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .and_then(|state| {
            state
                .get("codex_app")?
                .get("app_servers")?
                .as_array()
                .cloned()
        })
        .is_some_and(|servers| {
            servers
                .iter()
                .any(|server| server.get("pid").and_then(Value::as_u64) == Some(pid as u64))
        })
}

/// Returns the nearest provider process that owns the current hook command and identifies
/// `codex app-server` without treating an ordinary Codex CLI process as an App surface.
/// This is identity evidence only; the daemon re-reads the PID/start token before any OS action.
pub fn current_provider_context(provider: &str) -> Option<ProviderContext> {
    #[cfg(windows)]
    let processes = lightweight_process_tree();
    #[cfg(not(windows))]
    let processes = list_processes(&platform_name());
    let mut current = processes.get(&std::process::id())?.ppid;
    let mut visited = 0;
    while current > 1 && visited < 64 {
        let process = processes.get(&current)?;
        if target_provider(process) == Some(provider) {
            #[cfg(windows)]
            let is_app_server = provider == "codex" && published_codex_app_pid(current);
            #[cfg(not(windows))]
            let is_app_server = provider == "codex" && is_codex_app_server(process);
            let surface = if is_app_server {
                APP_SERVER_SURFACE.to_owned()
            } else {
                "cli".to_owned()
            };
            return Some(ProviderContext {
                pid: current,
                descendant_pids: if surface == APP_SERVER_SURFACE {
                    descendant_pids(&processes, current)
                } else {
                    Vec::new()
                },
                surface,
            });
        }
        current = process.ppid;
        visited += 1;
    }
    None
}

pub fn current_provider_pid(provider: &str) -> Option<u32> {
    current_provider_context(provider).map(|context| context.pid)
}

fn linux_processes() -> BTreeMap<u32, ProcessInfo> {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter_map(|pid| linux_process(pid).map(|process| (pid, process)))
        .collect()
}

fn stat_tail(stat: &str) -> Option<Vec<&str>> {
    Some(stat.rsplit_once(')')?.1.split_whitespace().collect())
}

fn terminal_identity(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(metadata) = fs::metadata(path) else {
            return String::new();
        };
        format!("{}:{}:{}", metadata.dev(), metadata.ino(), metadata.rdev())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        String::new()
    }
}

fn linux_process(pid: u32) -> Option<ProcessInfo> {
    let base = PathBuf::from(format!("/proc/{pid}"));
    let status = fs::read_to_string(base.join("status")).ok()?;
    let mut name = String::new();
    let mut ppid = 0;
    let mut rss_mb = 0;
    let mut anon_mb = 0;
    for line in status.lines() {
        let mut fields = line.split_whitespace();
        match fields.next()? {
            "Name:" => name = fields.next().unwrap_or_default().to_owned(),
            "PPid:" => ppid = fields.next()?.parse().ok()?,
            "VmRSS:" => rss_mb = fields.next()?.parse::<u64>().ok()? / 1024,
            "RssAnon:" => anon_mb = fields.next()?.parse::<u64>().ok()? / 1024,
            _ => {}
        }
    }
    let args = fs::read(base.join("cmdline"))
        .unwrap_or_default()
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .collect();
    let stat = fs::read_to_string(base.join("stat")).ok()?;
    let fields = stat_tail(&stat)?;
    let tty_number = fields.get(4)?.parse::<u64>().ok()?;
    #[cfg(not(unix))]
    let _ = tty_number;
    let start_token = fields.get(19)?.to_string();
    let mut terminal = String::new();
    let mut terminal_id = String::new();
    for fd in [1, 2, 0] {
        let Ok(endpoint) = fs::read_link(base.join(format!("fd/{fd}"))) else {
            continue;
        };
        let text = endpoint.to_string_lossy();
        let identity = terminal_identity(&endpoint);
        #[cfg(unix)]
        let device_matches = {
            use std::os::unix::fs::MetadataExt;
            tty_number == 0
                || fs::metadata(&endpoint)
                    .map(|metadata| metadata.rdev() == tty_number)
                    .unwrap_or(false)
        };
        #[cfg(not(unix))]
        let device_matches = true;
        if (text.starts_with("/dev/pts/") || text.starts_with("/dev/tty"))
            && !identity.is_empty()
            && device_matches
        {
            terminal = text.into_owned();
            terminal_id = identity;
            break;
        }
    }
    Some(ProcessInfo {
        pid,
        ppid,
        name,
        rss_mb,
        anon_mb,
        args,
        start_token,
        terminal,
        terminal_identity: terminal_id,
    })
}

fn darwin_processes(pid: Option<u32>) -> BTreeMap<u32, ProcessInfo> {
    let pid_string = pid.map(|value| value.to_string());
    let mut arguments = if pid.is_some() {
        vec!["-p", pid_string.as_deref().unwrap_or_default(), "-o"]
    } else {
        vec!["-axo"]
    };
    arguments.push("pid=,ppid=,rss=,lstart=,tty=,comm=,args=");
    let output = match command_output("ps", &arguments, 5) {
        Ok(output) => output,
        Err(error) => {
            sensor_failure("processes", error);
            return BTreeMap::new();
        }
    };
    parse_darwin_processes(&output, terminal_identity)
}

fn parse_darwin_processes(
    output: &str,
    identity: impl Fn(&Path) -> String,
) -> BTreeMap<u32, ProcessInfo> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 10 {
                return None;
            }
            let pid = fields[0].parse().ok()?;
            let tty = fields[8];
            let terminal = if matches!(tty, "??" | "?" | "-") {
                String::new()
            } else if tty.starts_with("/dev/") {
                tty.to_owned()
            } else {
                format!("/dev/{tty}")
            };
            let path = Path::new(&terminal);
            Some((
                pid,
                ProcessInfo {
                    pid,
                    ppid: fields[1].parse().ok()?,
                    name: Path::new(fields[9])
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or(fields[9])
                        .to_owned(),
                    rss_mb: fields[2].parse::<u64>().ok()? / 1024,
                    anon_mb: fields[2].parse::<u64>().ok()? / 1024,
                    args: fields
                        .get(10..)
                        .unwrap_or_default()
                        .iter()
                        .map(|v| (*v).to_owned())
                        .collect(),
                    start_token: fields[3..8].join(" "),
                    terminal_identity: identity(path),
                    terminal,
                },
            ))
        })
        .collect()
}

#[cfg(any(windows, test))]
fn windows_command_args(command: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in command.chars() {
        match character {
            '"' => quoted = !quoted,
            value if value.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            value => current.push(value),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn windows_processes(pid: Option<u32>) -> BTreeMap<u32, ProcessInfo> {
    #[cfg(windows)]
    {
        let query = pid.map_or_else(
            || "Get-CimInstance Win32_Process".to_owned(),
            |pid| format!("Get-CimInstance Win32_Process -Filter 'ProcessId={pid}'"),
        );
        let script = format!(
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new();$ErrorActionPreference='Stop';{query} | Select-Object ProcessId,ParentProcessId,Name,WorkingSetSize,CommandLine,CreationDate | ConvertTo-Json -Compress"
        );
        let output = match command_output(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
            8,
        ) {
            Ok(output) => output,
            Err(error) => {
                sensor_failure("processes", error);
                return BTreeMap::new();
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&output) else {
            sensor_failure("processes", "PowerShell process JSON was invalid");
            return BTreeMap::new();
        };
        let rows = value.as_array().cloned().unwrap_or_else(|| vec![value]);
        rows.into_iter()
            .filter_map(|row| {
                let pid = row.get("ProcessId")?.as_u64()? as u32;
                let command = row
                    .get("CommandLine")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let rss_mb = row
                    .get("WorkingSetSize")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    / (1024 * 1024);
                Some((
                    pid,
                    ProcessInfo {
                        pid,
                        ppid: row
                            .get("ParentProcessId")
                            .and_then(Value::as_u64)
                            .unwrap_or_default() as u32,
                        name: row
                            .get("Name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        rss_mb,
                        anon_mb: rss_mb,
                        args: windows_command_args(command),
                        start_token: row
                            .get("CreationDate")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        terminal: "console".to_owned(),
                        terminal_identity: String::new(),
                    },
                ))
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        BTreeMap::new()
    }
}

pub fn process_state(platform: &str, pid: u32) -> String {
    match platform {
        "windows" => windows_process_state(pid),
        "darwin" => command_output("ps", &["-o", "state=", "-p", &pid.to_string()], 3)
            .map(|value| {
                let value = value.trim();
                if value.is_empty() { "missing" } else { value }.to_owned()
            })
            .unwrap_or_else(|_| "missing".to_owned()),
        _ => fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|source| {
                source
                    .lines()
                    .find(|line| line.starts_with("State:"))
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                if Path::new(&format!("/proc/{pid}")).exists() {
                    "unknown"
                } else {
                    "missing"
                }
                .to_owned()
            }),
    }
}

#[cfg(unix)]
fn unix_signal(pid: u32, signal: i32) -> io::Result<()> {
    // SAFETY: kill takes scalar values and does not retain pointers.
    if unsafe { libc::kill(pid as i32, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn suspend_process(platform: &str, pid: u32) -> io::Result<()> {
    if platform == "windows" {
        windows_suspend_resume(pid, true)
    } else {
        #[cfg(unix)]
        return unix_signal(pid, libc::SIGSTOP);
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "SIGSTOP unavailable",
        ))
    }
}

pub fn resume_process(platform: &str, pid: u32) -> io::Result<()> {
    if platform == "windows" {
        windows_suspend_resume(pid, false)
    } else {
        #[cfg(unix)]
        return unix_signal(pid, libc::SIGCONT);
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "SIGCONT unavailable",
        ))
    }
}

pub fn terminate_process(platform: &str, pid: u32, force: bool) -> io::Result<()> {
    if platform == "windows" {
        #[cfg(windows)]
        let _ = force;
        windows_terminate(pid)
    } else {
        #[cfg(unix)]
        return unix_signal(pid, if force { libc::SIGKILL } else { libc::SIGTERM });
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "signals unavailable",
        ))
    }
}

#[cfg(windows)]
mod windows_native {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    #[repr(C)]
    pub struct MemoryStatusEx {
        pub length: u32,
        pub memory_load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub total_page_file: u64,
        pub avail_page_file: u64,
        pub total_virtual: u64,
        pub avail_virtual: u64,
        pub avail_extended_virtual: u64,
    }

    #[repr(C)]
    pub struct PerformanceInformation {
        pub cb: u32,
        pub commit_total: usize,
        pub commit_limit: usize,
        pub commit_peak: usize,
        pub physical_total: usize,
        pub physical_available: usize,
        pub system_cache: usize,
        pub kernel_total: usize,
        pub kernel_paged: usize,
        pub kernel_nonpaged: usize,
        pub page_size: usize,
        pub handle_count: u32,
        pub process_count: u32,
        pub thread_count: u32,
    }

    #[repr(C)]
    pub struct ThreadEntry32 {
        pub size: u32,
        pub usage: u32,
        pub thread_id: u32,
        pub owner_process_id: u32,
        pub base_priority: i32,
        pub delta_priority: i32,
        pub flags: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        pub fn CloseHandle(handle: Handle) -> i32;
        pub fn TerminateProcess(handle: Handle, exit_code: u32) -> i32;
        pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        pub fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry32) -> i32;
        pub fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry32) -> i32;
        pub fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> Handle;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        pub fn GetPerformanceInfo(info: *mut PerformanceInformation, size: u32) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        pub fn NtSuspendProcess(handle: Handle) -> i32;
        pub fn NtResumeProcess(handle: Handle) -> i32;
        pub fn NtQueryInformationThread(
            handle: Handle,
            class: i32,
            information: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }
}

fn windows_memory_snapshot(config: &mut Config) -> MemorySnapshot {
    #[cfg(windows)]
    {
        use windows_native::*;
        let mut status: MemoryStatusEx = unsafe { std::mem::zeroed() };
        status.length = std::mem::size_of::<MemoryStatusEx>() as u32;
        // SAFETY: status is a valid writable structure with the required length field.
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            sensor_success("memory");
            sensor_success("capacity");
            return capacity_override(
                config,
                status.avail_phys / (1024 * 1024),
                status.total_phys / (1024 * 1024),
                "GlobalMemoryStatusEx",
            );
        }
        sensor_failure("memory", io::Error::last_os_error());
    }
    capacity_override(config, 0, 8192, "fallback:8GiB")
}

fn windows_native_pressure() -> NativePressure {
    #[cfg(windows)]
    {
        use windows_native::*;
        let mut info: PerformanceInformation = unsafe { std::mem::zeroed() };
        info.cb = std::mem::size_of::<PerformanceInformation>() as u32;
        // SAFETY: info is a valid writable structure and size matches its layout.
        if unsafe { GetPerformanceInfo(&mut info, info.cb) } != 0 {
            sensor_success("pressure");
            return NativePressure {
                commit_remaining_mb: Some(windows_commit_remaining_mb(
                    info.commit_limit,
                    info.commit_total,
                    info.page_size,
                )),
                ..NativePressure::default()
            };
        }
        sensor_failure("pressure", io::Error::last_os_error());
    }
    NativePressure {
        state: "unknown".to_owned(),
        confidence: "low".to_owned(),
        ..NativePressure::default()
    }
}

#[cfg(any(windows, test))]
fn windows_commit_remaining_mb(limit: usize, total: usize, page_size: usize) -> u64 {
    (limit.saturating_sub(total) * page_size / (1024 * 1024)) as u64
}

fn windows_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows_native::*;
        // SAFETY: OpenProcess/CloseHandle use an integer pid and owned handle.
        let handle = unsafe { OpenProcess(0x1000, 0, pid) };
        if handle.is_null() {
            return false;
        }
        unsafe { CloseHandle(handle) };
        true
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

fn windows_thread_suspend_counts(pid: u32) -> Option<Vec<u32>> {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use windows_native::*;
        const INVALID_HANDLE: Handle = -1_isize as Handle;
        // SAFETY: ToolHelp returns an owned snapshot handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(0x00000004, 0) };
        if snapshot.is_null() || snapshot == INVALID_HANDLE {
            return None;
        }
        let mut entry: ThreadEntry32 = unsafe { std::mem::zeroed() };
        entry.size = std::mem::size_of::<ThreadEntry32>() as u32;
        let mut counts = Vec::new();
        let mut failed = false;
        // SAFETY: entry is correctly sized and snapshot remains open through iteration.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.owner_process_id == pid {
                // THREAD_QUERY_INFORMATION
                let thread = unsafe { OpenThread(0x0040, 0, entry.thread_id) };
                if thread.is_null() {
                    failed = true;
                } else {
                    let mut count = 0_u32;
                    // ThreadSuspendCount = 35.
                    let status = unsafe {
                        NtQueryInformationThread(
                            thread,
                            35,
                            (&mut count as *mut u32).cast::<c_void>(),
                            std::mem::size_of::<u32>() as u32,
                            std::ptr::null_mut(),
                        )
                    };
                    unsafe { CloseHandle(thread) };
                    if status == 0 {
                        counts.push(count);
                    } else {
                        failed = true;
                    }
                }
            }
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        (!failed).then_some(counts)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

fn windows_process_state(pid: u32) -> String {
    windows_process_state_from(
        windows_thread_suspend_counts(pid),
        windows_process_alive(pid),
    )
}

fn windows_process_state_from(counts: Option<Vec<u32>>, alive: bool) -> String {
    if let Some(counts) = counts
        && !counts.is_empty()
    {
        return if counts.iter().all(|count| *count > 0) {
            "T"
        } else {
            "R"
        }
        .to_owned();
    }
    if alive { "unknown" } else { "missing" }.to_owned()
}

fn windows_suspend_resume(pid: u32, suspend: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_native::*;
        // SAFETY: OpenProcess returns an owned handle, closed before return.
        let handle = unsafe { OpenProcess(0x0800, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let status = unsafe {
            if suspend {
                NtSuspendProcess(handle)
            } else {
                NtResumeProcess(handle)
            }
        };
        unsafe { CloseHandle(handle) };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!("NT status {status:#x}")))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, suspend);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows process control unavailable",
        ))
    }
}

fn windows_terminate(pid: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_native::*;
        // SAFETY: OpenProcess returns an owned handle, closed before return.
        let handle = unsafe { OpenProcess(0x0001, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = unsafe { TerminateProcess(handle, 1) };
        unsafe { CloseHandle(handle) };
        if result != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows process control unavailable",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temporary_directory(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "memory-supervisor-platform-{label}-{}-{}",
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
    fn linux_stat_parser_allows_spaces_and_closing_parentheses() {
        let stat = "42 (name with ) paren) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 999 20";
        let fields = stat_tail(stat).unwrap();
        assert_eq!(fields[19], "999");
        assert_eq!(fields[4], "4");
    }

    #[cfg(unix)]
    #[test]
    fn command_output_drains_more_than_a_pipe_buffer() {
        let output = command_output(
            "/bin/sh",
            &["-c", "dd if=/dev/zero bs=131072 count=1 2>/dev/null"],
            2,
        )
        .unwrap();
        assert_eq!(output.len(), 131_072);
    }

    #[cfg(unix)]
    #[test]
    fn command_timeout_does_not_wait_for_a_descendant_inheriting_the_pipe() {
        let started = Instant::now();
        let error = command_output("/bin/sh", &["-c", "(sleep 4) & sleep 30"], 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn windows_quotes_keep_executable_together() {
        assert_eq!(
            windows_command_args(r#""C:\Program Files\node.exe" C:\cli\claude.js --flag"#),
            [
                "C:\\Program Files\\node.exe",
                "C:\\cli\\claude.js",
                "--flag"
            ]
        );
    }

    #[test]
    fn incidents_no_longer_pin_admission_but_degraded_protection_does() {
        assert_eq!(
            admission_level_for_state(&serde_json::json!({
                "level": "GREEN",
                "action": "allow",
                "probation": {"pid": 42},
                "stopped_pids": [42]
            })),
            Level::Green
        );
        assert_eq!(
            admission_level_for_state(&serde_json::json!({
                "level": "GREEN",
                "action": "allow",
                "protection_degraded": true
            })),
            Level::Orange
        );
    }

    #[test]
    fn peer_cap_proximity_stays_local_while_pressure_still_federates() {
        let cap_driven = serde_json::json!({
            "level": "GREEN",
            "action": "hold",
            "adaptive_action": "allow",
            "cli_hard_cap_driving": true
        });
        assert_eq!(admission_level_for_state(&cap_driven), Level::Orange);
        assert_eq!(admission_level_for_peer(&cap_driven), Level::Green);
        let pressure_driven = serde_json::json!({
            "level": "ORANGE",
            "action": "hold",
            "adaptive_action": "hold",
            "cli_hard_cap_driving": false
        });
        assert_eq!(admission_level_for_peer(&pressure_driven), Level::Orange);
        let cap_driven_but_degraded = serde_json::json!({
            "action": "drain",
            "adaptive_action": "allow",
            "cli_hard_cap_driving": true,
            "protection_degraded": true
        });
        assert_eq!(
            admission_level_for_peer(&cap_driven_but_degraded),
            Level::Orange
        );
    }

    #[test]
    fn co_residency_guard_scopes_a_non_host_local_rendezvous_to_self() {
        let dir = std::env::temp_dir().join(format!("ms-coresidency-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("peer-a.json"),
            br#"{"ts": 1000.0, "level": "GREEN", "instance": "peer-a"}"#,
        )
        .unwrap();
        // Host-local rendezvous: the fresh peer federates.
        assert_eq!(federated_states_from(&dir, 1000.0, 10.0, true).len(), 1);
        // A network rendezvous is not co-residency proof, so peers are scoped out — self only.
        assert!(federated_states_from(&dir, 1000.0, 10.0, false).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn federation_reader_is_platform_agnostic_inside_one_local_memory_domain() {
        let dir = temporary_directory("federation-platforms");
        for (index, platform) in ["windows", "wsl", "linux", "darwin"].iter().enumerate() {
            std::fs::write(
                dir.join(format!("peer-{index}.json")),
                serde_json::to_vec(&serde_json::json!({
                    "ts": 1000.0,
                    "level": "GREEN",
                    "instance": format!("{platform}-peer"),
                    "platform": platform
                }))
                .unwrap(),
            )
            .unwrap();
        }
        let peers = federated_states_from(&dir, 1000.0, 10.0, true);
        assert_eq!(peers.len(), 4);
        assert_eq!(
            peers
                .iter()
                .filter_map(|peer| peer.get("platform").and_then(Value::as_str))
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["darwin", "linux", "windows", "wsl"])
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn windows_inventory_cache_retries_on_interval_and_preserves_last_success() {
        fn process(pid: u32) -> ProcessInfo {
            ProcessInfo {
                pid,
                ppid: 0,
                name: "codex.exe".to_owned(),
                rss_mb: 0,
                anon_mb: 0,
                args: Vec::new(),
                start_token: String::new(),
                terminal: String::new(),
                terminal_identity: String::new(),
            }
        }
        let start = Instant::now();
        let interval = Duration::from_secs(3);
        let mut cache = WindowsProcessCache::default();
        let scans = Cell::new(0);
        let (first, attempted, success) =
            refresh_windows_cache(&mut cache, start, interval, || {
                scans.set(scans.get() + 1);
                BTreeMap::from([(1, process(1))])
            });
        assert_eq!((first.len(), attempted, success), (1, true, true));

        let (cached, attempted, _) =
            refresh_windows_cache(&mut cache, start + Duration::from_secs(1), interval, || {
                scans.set(scans.get() + 1);
                BTreeMap::new()
            });
        assert_eq!((cached.len(), attempted, scans.get()), (1, false, 1));

        let (preserved, attempted, success) =
            refresh_windows_cache(&mut cache, start + Duration::from_secs(4), interval, || {
                scans.set(scans.get() + 1);
                BTreeMap::new()
            });
        assert_eq!((preserved.len(), attempted, success), (1, true, false));
        let (second, attempted, success) =
            refresh_windows_cache(&mut cache, start + Duration::from_secs(7), interval, || {
                scans.set(scans.get() + 1);
                BTreeMap::from([(2, process(2))])
            });
        assert_eq!(
            (second.contains_key(&2), attempted, success, scans.get()),
            (true, true, true, 3)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_suspend_and_resume_use_the_exact_canary_pid() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >NUL"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let result = std::panic::catch_unwind(|| {
            let process = (0..100)
                .find_map(|_| {
                    let process = process_by_pid("windows", pid);
                    if process.is_none() {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    process
                })
                .expect("Windows canary was not observable");
            assert!(!process.start_token.is_empty());

            suspend_process("windows", pid).unwrap();
            assert!((0..100).any(|_| {
                let stopped = process_state("windows", pid) == "T";
                if !stopped {
                    std::thread::sleep(Duration::from_millis(20));
                }
                stopped
            }));

            resume_process("windows", pid).unwrap();
            assert!((0..100).any(|_| {
                let running = process_state("windows", pid) == "R";
                if !running {
                    std::thread::sleep(Duration::from_millis(20));
                }
                running
            }));
        });
        let _ = resume_process("windows", pid);
        let _ = child.kill();
        let _ = child.wait();
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn native_counter_and_state_parsers_match_reference_cases() {
        let root = temporary_directory("native");
        let vmstat = root.join("vmstat");
        fs::write(
            &vmstat,
            "pgscan_kswapd 256\npgscan_direct 256\npswpin 128\npswpout 128\noom_kill 2\n",
        )
        .unwrap();
        let (reclaim, swap, oom) = linux_vm_counters(&vmstat);
        #[cfg(unix)]
        let page_mb = {
            // SAFETY: sysconf reads a process-wide constant and retains no pointers.
            let bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            (if bytes > 0 { bytes as f64 } else { 4096.0 }) / (1024.0 * 1024.0)
        };
        #[cfg(not(unix))]
        let page_mb = 4096.0 / (1024.0 * 1024.0);
        assert!((reclaim - 512.0 * page_mb).abs() < 0.001);
        assert!((swap - 256.0 * page_mb).abs() < 0.001);
        assert_eq!(oom, 2);

        let darwin = darwin_pressure_from_outputs(
            "2\n",
            "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPageouts: 256.\nPages purged: 256.\nCompressions: 256.\nDecompressions: 256.\nSwapins: 128.\nSwapouts: 128.\n",
        );
        assert_eq!(
            (darwin.state.as_str(), darwin.confidence.as_str()),
            ("warning", "high")
        );
        assert!((darwin.reclaim_total - 4.0).abs() < 0.001);
        assert!((darwin.swap_total - 1.0).abs() < 0.001);

        let darwin_without_optional_level = darwin_pressure_from_outputs(
            "",
            "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPageouts: 256.\nSwapouts: 128.\n",
        );
        assert_eq!(
            (
                darwin_without_optional_level.state.as_str(),
                darwin_without_optional_level.confidence.as_str(),
            ),
            ("unknown", "low")
        );
        assert!((darwin_without_optional_level.reclaim_total - 1.0).abs() < 0.001);
        assert!((darwin_without_optional_level.swap_total - 0.5).abs() < 0.001);

        assert_eq!(windows_commit_remaining_mb(3000, 1000, 4096), 7);
        for (counts, alive, expected) in [
            (Some(vec![1, 2]), true, "T"),
            (Some(vec![1, 0]), true, "R"),
            (None, true, "unknown"),
            (Some(vec![]), false, "missing"),
        ] {
            assert_eq!(windows_process_state_from(counts, alive), expected);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn darwin_process_and_capacity_override_parsers_preserve_identity() {
        let parsed = parse_darwin_processes(
            "42 1 2048 Mon Jul 20 12:34:56 2026 ttys003 /usr/local/bin/codex codex --flag\n",
            |_| "terminal-id".to_owned(),
        );
        let process = &parsed[&42];
        assert_eq!(process.start_token, "Mon Jul 20 12:34:56 2026");
        assert_eq!(process.terminal, "/dev/ttys003");
        assert_eq!(process.terminal_identity, "terminal-id");
        assert_eq!(process.args, ["codex", "--flag"]);

        let root = temporary_directory("capacity");
        let config_path = root.join("config.json");
        fs::write(&config_path, r#"{"MEMORY_SUPERVISOR_CAPACITY_MB":4096}"#).unwrap();
        let mut config = Config::load(&config_path);
        let snapshot = capacity_override(&mut config, 12_000, 16_384, "test");
        assert_eq!(snapshot.available_mb, 4096);
        assert_eq!(snapshot.capacity_mb, 4096);
        assert_eq!(
            snapshot.capacity_source,
            "override:MEMORY_SUPERVISOR_CAPACITY_MB"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cgroup_parent_limits_and_federation_filtering_match_reference_cases() {
        let root = temporary_directory("cgroup");
        let mount = root.join("cgroup");
        let leaf = mount.join("parent/leaf");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("memory.max"), "max\n").unwrap();
        fs::write(leaf.join("memory.current"), (256 * 1024 * 1024).to_string()).unwrap();
        fs::write(
            leaf.parent().unwrap().join("memory.max"),
            (2_u64 * 1024 * 1024 * 1024).to_string(),
        )
        .unwrap();
        fs::write(
            leaf.parent().unwrap().join("memory.current"),
            (1536_u64 * 1024 * 1024).to_string(),
        )
        .unwrap();
        let membership = root.join("membership");
        let mountinfo = root.join("mountinfo");
        fs::write(&membership, "0::/parent/leaf\n").unwrap();
        fs::write(
            &mountinfo,
            format!("29 23 0:26 / {} rw - cgroup2 cgroup rw\n", mount.display()),
        )
        .unwrap();
        assert_eq!(
            linux_cgroup_memory(16384, &membership, &mountinfo),
            Some((2048, 512, "2"))
        );

        let v1_mount = root.join("memory");
        let v1_group = v1_mount.join("slice");
        fs::create_dir_all(&v1_group).unwrap();
        fs::write(
            v1_group.join("memory.limit_in_bytes"),
            (4_u64 * 1024 * 1024 * 1024).to_string(),
        )
        .unwrap();
        fs::write(
            v1_group.join("memory.usage_in_bytes"),
            (3_u64 * 1024 * 1024 * 1024).to_string(),
        )
        .unwrap();
        fs::write(&membership, "5:memory:/slice\n").unwrap();
        fs::write(
            &mountinfo,
            format!(
                "30 23 0:27 / {} rw - cgroup cgroup rw,memory\n",
                v1_mount.display()
            ),
        )
        .unwrap();
        assert_eq!(
            linux_cgroup_memory(16384, &membership, &mountinfo),
            Some((4096, 1024, "1"))
        );

        let federation = root.join("federation");
        fs::create_dir(&federation).unwrap();
        for index in 0..65 {
            fs::write(
                federation.join(format!("a-{index:02}.json")),
                serde_json::to_vec(&serde_json::json!({"ts": 940.0, "level":"GREEN"})).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            federation.join("z-fresh.json"),
            serde_json::to_vec(
                &serde_json::json!({"ts":1000.0, "level":"RED", "instance":"fresh"}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(federation.join("array.json"), "[]").unwrap();
        let states = fresh_federated_states_at(&federation, 1000.0, 10.0);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0]["instance"], "fresh");
        fs::remove_dir_all(root).unwrap();
    }
}
