use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
#[cfg(all(test, target_os = "linux"))]
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::codex_app::{
    APP_SERVER_SURFACE, CodexAppControlPlan, CodexAppHookRoute, CodexAppLogicalCandidate,
    CodexAppPlanInput, CodexAppPressureProfile, CodexAppSnapshot, hook_targets,
    marker_reader_capability, plan_app_control, process_descends_from,
};
use crate::config::{
    Config, PRETOOL_HOLD_DEFAULT_S, load_notification_config, notification_channels,
    notification_config_path, power_is_off, state_dir,
};
use crate::containment::{
    HookObservation, LogicalAgent, LogicalState, RunawayConfirmation, RunawayInputs, ToolClass,
    evaluate_runaway, newest_first,
};
use crate::events::{
    event_should_notify, make_event, pending_acknowledgements, pending_events, queue_event,
};
use crate::integration::{CodexHookSurface, audit_codex_app_hooks, update_codex_app_hooks};
use crate::notify::spawn_dispatcher;
use crate::platform::{
    admission_level_for_peer, admission_level_for_state, federation_dir, fresh_federated_states,
    list_processes, memory_snapshot, merge_federated_incidents, native_pressure, platform_name,
    process_by_pid, process_state, resume_process, sensor_health, suspend_process,
    terminate_process,
};
use crate::policy::{
    Action, Assessment, HistorySample, Level, ProcessInfo, ResolvedPolicy, TrackedProcess,
    apply_cli_hard_cap, assess_pressure, level_from, process_identity, resolve_policy,
    tracked_processes,
};
use crate::runtime::{
    INCIDENT_RETENTION_S, PendingControl, Probation, RuntimeLedger, incident_updated_at, rounded,
    unique_nonce,
};
use crate::storage::{append_bounded, ensure_private_dir, write_atomic_json, write_atomic_text};
use crate::terminal;

const CODEX_APP_HOOK_ROUTE_FRESH_S: f64 = 120.0;

const INCIDENT_NOTICE_S: f64 = 3600.0;
const TARGETS: [&str; 2] = ["claude", "codex"];
const MAX_TICK_S: f64 = 5.0;
const PEER_FRESH_CHECKPOINT_S: f64 = 30.0;

fn codex_app_hook_route_status(
    environment_resolved: bool,
    verified: bool,
    last_observed_at: Option<f64>,
    route_modified_at: Option<f64>,
    now: f64,
) -> (&'static str, bool) {
    let receipt_matches_current_route = last_observed_at
        .zip(route_modified_at)
        .is_some_and(|(observed, modified)| observed + 0.001 >= modified);
    let active = environment_resolved
        && verified
        && receipt_matches_current_route
        && last_observed_at.is_some_and(|observed| {
            observed <= now + 1.0 && now - observed <= CODEX_APP_HOOK_ROUTE_FRESH_S
        });
    let status = if !environment_resolved {
        "UNRESOLVED"
    } else if !verified {
        "NEEDS ATTENTION"
    } else if active {
        "ACTIVE"
    } else if receipt_matches_current_route && last_observed_at.is_some() {
        "STALE"
    } else {
        "CONFIGURED"
    };
    (status, active)
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn installed_binary_matches(binary: &Path) -> bool {
    let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) else {
        return false;
    };
    installed_binary_matches_at(binary, &PathBuf::from(home))
}

fn installed_binary_matches_at(binary: &Path, home: &Path) -> bool {
    let Ok(pointer) = fs::read_to_string(home.join(".memory-supervisor/binary")) else {
        return false;
    };
    let installed = PathBuf::from(pointer.trim());
    if installed.as_os_str().is_empty() {
        return false;
    }
    match (fs::canonicalize(binary), fs::canonicalize(&installed)) {
        (Ok(binary), Ok(installed)) => binary == installed,
        _ => binary == installed,
    }
}

#[derive(Clone, Copy, Default)]
struct GrowthMetrics {
    sample_count: usize,
    span_s: f64,
    delta_mb: f64,
    long_slope_mb_s: f64,
    recent_slope_mb_s: f64,
    monotonicity: f64,
}

#[derive(Clone, Copy)]
struct CodexAppPauseScope<'a> {
    affected_sessions: &'a [String],
    app_server_pid: u32,
    shared_host: bool,
}

#[derive(Clone, Default)]
struct CodexAppBackstopPlan {
    kind: String,
    required_keys: BTreeSet<String>,
    blind: bool,
}

impl CodexAppBackstopPlan {
    fn available(&self) -> bool {
        !self.kind.is_empty() && !self.required_keys.is_empty()
    }
}

fn growth_metrics(history: &[(f64, u64)]) -> GrowthMetrics {
    let Some((first, last)) = history.first().zip(history.last()) else {
        return GrowthMetrics::default();
    };
    let span = (last.0 - first.0).max(0.0);
    let delta = last.1 as f64 - first.1 as f64;
    let long_slope = if span > 0.0 { delta / span } else { 0.0 };
    let (nonnegative, intervals) = history.windows(2).fold((0, 0), |(good, count), pair| {
        (good + usize::from(pair[1].1 >= pair[0].1), count + 1)
    });
    let recent = &history[history.len().saturating_sub(5)..];
    let recent_slope = recent
        .first()
        .zip(recent.last())
        .filter(|(first, last)| last.0 > first.0)
        .map(|(first, last)| (last.1 as f64 - first.1 as f64) / (last.0 - first.0))
        .unwrap_or_default();
    GrowthMetrics {
        sample_count: history.len(),
        span_s: span,
        delta_mb: delta,
        long_slope_mb_s: long_slope,
        recent_slope_mb_s: recent_slope,
        monotonicity: if intervals == 0 {
            0.0
        } else {
            nonnegative as f64 / intervals as f64
        },
    }
}

fn hostname() -> String {
    env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            fs::read_to_string("/etc/hostname")
                .ok()
                .map(|value| value.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn default_instance_name(platform: &str, distro: Option<&str>, host: &str) -> String {
    match (platform, distro.filter(|value| !value.trim().is_empty())) {
        ("wsl", Some(distro)) => format!("{platform}-{distro}-{host}"),
        _ => format!("{platform}-{host}"),
    }
}

pub fn instance_name(platform: &str, config: &Config) -> String {
    let raw = config
        .setting("MEMORY_SUPERVISOR_INSTANCE")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            let distro = env::var("WSL_DISTRO_NAME").ok();
            default_instance_name(platform, distro.as_deref(), &hostname())
        });
    let value: String = raw
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let value = value.trim_matches(['.', '_', '-']);
    let value: String = value.chars().take(128).collect();
    if value.is_empty() {
        format!("{platform}-unknown")
    } else {
        value
    }
}

fn numeric(config: &mut Config, name: &str, default: f64, minimum: f64, maximum: f64) -> f64 {
    config.validated_number(name, default, Some(minimum), Some(maximum))
}

fn hard_cap(config: &mut Config) -> Option<f64> {
    let name = "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB";
    let configured = config.setting(name).is_some_and(|value| match value {
        Value::String(value) => !value.is_empty(),
        _ => true,
    });
    if !configured {
        config.clear_validation_error(name);
        return None;
    }
    let value = config.validated_number(name, 0.0, Some(1.0), None);
    (!config.has_validation_error(name)).then_some(value)
}

fn log_event(directory: &Path, message: &str) {
    let line = format!("{:.3} {message}\n", now_epoch());
    let _ = append_bounded(&directory.join("events.log"), &line, 5 * 1024 * 1024);
}

struct AppGuardInvocation<'a> {
    pid: u32,
    identity: &'a str,
    incident_id: &'a str,
    delay_s: f64,
    runtime_path: &'a Path,
    platform: &'a str,
    app_server_pid: u32,
    control_base: &'a Path,
}

fn guard_argument_values(invocation: &AppGuardInvocation<'_>) -> Vec<std::ffi::OsString> {
    vec![
        "app-resume-guard".into(),
        invocation.pid.to_string().into(),
        invocation.identity.into(),
        invocation.incident_id.into(),
        ((invocation.delay_s.clamp(0.25, 300.0) * 1000.0)
            .round()
            .to_string())
        .into(),
        invocation.runtime_path.as_os_str().to_owned(),
        invocation.platform.into(),
        invocation.app_server_pid.to_string().into(),
        "shared-host".into(),
        invocation.control_base.as_os_str().to_owned(),
    ]
}

fn app_guard_controller_state(platform: &str, control_base: &Path, phase: &str) -> Option<bool> {
    let receipt = fs::read(crate::app_guard::phase_path(control_base, phase))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())?;
    let controller_pid = receipt
        .get("controller_pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 1)?;
    let controller_identity = receipt
        .get("controller_identity")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !controller_identity.is_empty()
        && let Some(current_identity) =
            crate::app_guard::controller_process_token(platform, controller_pid)
    {
        return Some(current_identity == controller_identity);
    }
    if process_by_pid(platform, controller_pid).is_some() {
        return controller_identity.is_empty().then_some(true);
    }
    #[cfg(unix)]
    if platform != "windows" {
        // SAFETY: signal 0 performs no process action; it only checks this numeric PID.
        if unsafe { libc::kill(controller_pid as i32, 0) } == 0 {
            return controller_identity.is_empty().then_some(true);
        }
        return match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(false),
            Some(libc::EPERM) => Some(true),
            _ => None,
        };
    }
    #[cfg(windows)]
    if platform == "windows" {
        use std::ffi::c_void;

        type Handle = *mut c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
            fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
            fn GetLastError() -> u32;
            fn CloseHandle(handle: Handle) -> i32;
        }
        const SYNCHRONIZE: u32 = 0x0010_0000;
        const ERROR_INVALID_PARAMETER: u32 = 87;
        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_TIMEOUT: u32 = 258;
        // SAFETY: the handle is opened read-only for synchronization and closed on every path.
        unsafe {
            let handle = OpenProcess(SYNCHRONIZE, 0, controller_pid);
            if handle.is_null() {
                return (GetLastError() == ERROR_INVALID_PARAMETER).then_some(false);
            }
            let wait = WaitForSingleObject(handle, 0);
            CloseHandle(handle);
            return match wait {
                WAIT_OBJECT_0 => Some(false),
                WAIT_TIMEOUT => controller_identity.is_empty().then_some(true),
                _ => None,
            };
        }
    }
    None
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn value_u32(value: &Value, key: &str) -> u32 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default() as u32
}

fn value_f64(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_default()
}

fn incident_is_codex_app_physical(incident: &Value) -> bool {
    incident
        .get("reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| reason.starts_with("app-"))
        || incident.get("app_control_scope").and_then(Value::as_str)
            == Some("thread-confirmed-child")
}

fn extend_incident_audience(fields: &mut Map<String, Value>, incident: &Value) {
    if !incident_is_codex_app_physical(incident) {
        return;
    }
    fields.insert(
        "surface".to_owned(),
        Value::String(APP_SERVER_SURFACE.to_owned()),
    );
    for key in ["audience_provider", "audience_sessions"] {
        if let Some(value) = incident.get(key) {
            fields.insert(key.to_owned(), value.clone());
        }
    }
}

fn incident_notice(incident: &Value, phase: &str, transition_source: &str) -> String {
    let pid = value_u32(incident, "pid");
    let name = incident
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("process");
    let role = incident
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("process");
    let reason = incident
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("memory-incident");
    let attribution = incident
        .get("attribution")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let attribution_text = match attribution {
        "agent" => "agent activity likely dominates the machine-level headroom loss",
        "mixed" => "agent and external activity both contribute to machine-level headroom loss",
        "external" => "external activity likely dominates the machine-level headroom loss",
        _ => "the machine-level cause is not attributable from current evidence",
    };
    let evidence = match reason {
        "runaway-memory" => {
            let time_to_reserve = incident
                .get("process_time_to_reserve_s")
                .and_then(Value::as_f64)
                .map(|seconds| format!("; projected time to the recovery reserve was {seconds}s"))
                .unwrap_or_default();
            format!(
                "direct process evidence: {name} reached {} MiB and kept growing at an observed average of {} MiB/s across the configured {}s window{time_to_reserve}",
                value_u32(incident, "anon_mb"),
                value_f64(incident, "slope_mb_s"),
                value_f64(incident, "observation_window_s")
            )
        }
        "pressure-pause" | "pressure-lead-last-resort" => {
            "machine-level evidence: recoverable headroom was close to exhaustion; one exact PID was paused as the minimum containment step".to_owned()
        }
        "hard-cap-pause" | "hard-cap-lead-last-resort" => {
            format!(
                "explicit policy evidence: tracked CLI memory reached the configured {} MiB hard cap",
                value_f64(incident, "cli_hard_cap_mb")
            )
        }
        _ => format!("recorded reason: {reason}"),
    };
    let recovery = incident
        .get("recovery_policy")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (label, state, next) = match phase {
        "suspended" => (
            "PROCESS PAUSED",
            "The PID and in-memory session are preserved. This is not a crash, and pausing does not immediately release its RAM.".to_owned(),
            match recovery {
                "lead-probation" => "After sustained machine recovery, the supervisor will attempt one guarded automatic resume and watch the same PID. Run `memory-status` from another terminal for the live decision.".to_owned(),
                "automatic-pressure-recovery" => "The supervisor will resume one paused process at a time after sustained recovery. Run `memory-status` from another terminal for the live decision.".to_owned(),
                _ => format!("Run `memory-status` from another terminal. Resume only after reviewing the cause: `memory-supervisor resume {pid}`."),
            },
        ),
        "probation" => (
            "GUARDED RESUME",
            "The same PID is running under one-time probation.".to_owned(),
            "Stable recovery completes automatically; renewed growth or pressure pauses the same PID again.".to_owned(),
        ),
        "probation_failed" => (
            "PROCESS PAUSED AGAIN",
            "The guarded resume relapsed, so the same PID was reversibly paused again.".to_owned(),
            format!("Run `memory-status`, preserve any available work, and resume manually only if you accept another recurrence: `memory-supervisor resume {pid}`."),
        ),
        "pressure_resumed" | "recovery_confirmed" => (
            "PROCESS RESUMED",
            "Sustained recovery was confirmed and the same PID is running again.".to_owned(),
            "No manual recovery action is required. New fan-out still follows the current admission state.".to_owned(),
        ),
        "manual_resumed" | "external_resumed" => (
            "PROCESS RESUMED",
            "The same PID was resumed outside automatic recovery.".to_owned(),
            "Run `memory-status` before new fan-out; the supervisor will continue enforcing the current policy.".to_owned(),
        ),
        _ => (
            "MEMORY EVENT",
            "A supervised process changed state.".to_owned(),
            "Run `memory-status` for the current machine decision.".to_owned(),
        ),
    };
    let transition = if transition_source.is_empty() {
        String::new()
    } else {
        format!(" ({transition_source})")
    };
    format!(
        "[Memory Supervisor] {label}{transition}\nTarget: {name} ({role}), PID {pid}\nWhy: {evidence}.\nAttribution estimate: {attribution_text}.\nState: {state}\nNext: {next}\nAgent context: delivered once at the next provider hook boundary; terminal, OS, and configured remote routes are attempted immediately."
    )
}

fn pressure_episode_component(event_type: &str, status: &str) -> bool {
    matches!(
        event_type,
        "pressure-action"
            | "logical-containment"
            | "process-pause"
            | "process-control"
            | "codex-app-surface-gate"
            | "codex-app-process-pause"
    ) || (event_type == "lead-probation" && matches!(status, "monitoring" | "resumed"))
}

fn codex_app_recovery_safe(assessment: &Assessment, profile: &CodexAppPressureProfile) -> bool {
    if profile.causal || assessment.native_confidence == "low" {
        return false;
    }
    let outside_braking_distance = !assessment.collapse_imminent
        && assessment
            .time_to_recovery_reserve_s
            .is_none_or(|tte| tte > assessment.reaction_s * 2.0);
    assessment.attribution == "external" || outside_braking_distance
}

fn pressure_action_notice(
    assessment: &Assessment,
    previous: Option<Action>,
    psi_full_avg10: f64,
) -> String {
    let (title, effect) = match assessment.action {
        Action::Allow => (
            "ADMISSION OPEN",
            "New fan-out is allowed again; running work was not interrupted.",
        ),
        Action::Observe
            if previous
                .is_some_and(|previous| matches!(previous, Action::Hold | Action::Drain)) =>
        {
            (
                "ADMISSION REOPENED — OBSERVATION CONTINUES",
                "New fan-out is allowed again; no existing work was paused by this transition.",
            )
        }
        Action::Observe => (
            "PRESSURE OBSERVED",
            "No work is blocked or paused; the supervisor is collecting more evidence.",
        ),
        Action::Hold => (
            "NEW FAN-OUT HELD",
            "Only new fan-out is blocked. Existing work continues, and admission reopens automatically after sustained recovery.",
        ),
        Action::Drain => (
            "DRAIN MODE ACTIVE",
            "New fan-out is blocked. Existing work continues unless a separate exact-PID pause event reports a last-resort containment action.",
        ),
    };
    let evidence = if assessment.cli_hard_cap_driving {
        format!(
            "tracked CLI memory is {} MiB against the explicit {} MiB hard cap",
            assessment.cli_memory_used_mb.unwrap_or_default(),
            assessment.cli_hard_cap_mb.unwrap_or_default()
        )
    } else {
        let tte = assessment
            .trajectory_confirmed
            .then_some(assessment.time_to_recovery_reserve_s)
            .flatten()
            .map(|seconds| {
                format!(
                    "; if the sustained recent rate continued, the recovery reserve would be reached in {seconds}s"
                )
            })
            .unwrap_or_default();
        format!(
            "{} MiB of {} MiB remains; adaptive recovery reserve is {} MiB; new-fan-out floor is {} MiB; trajectory confirmation={}{tte}; native pressure is {}; PSI full avg10 is {psi_full_avg10}",
            assessment.mem_available_mb,
            assessment.memory_capacity_mb,
            assessment.automatic_reserve_mb,
            assessment.new_fanout_floor_mb,
            assessment.trajectory_confirmed,
            assessment.native_state,
        )
    };
    let attribution = match assessment.attribution.as_str() {
        "agent" => "agent activity likely dominates",
        "mixed" => "agent and external activity both contribute",
        "external" => "external activity likely dominates",
        _ => "the cause is not attributable from current evidence",
    };
    format!(
        "[Memory Supervisor] {title}\nWhy: {evidence}.\nEffect: {effect}\nAttribution estimate: {attribution}.\nNext: `memory-status` shows the live decision and evidence."
    )
}

pub struct Supervisor {
    pub platform: String,
    pub instance: String,
    pub directory: PathBuf,
    pub runtime_path: PathBuf,
    pub ledger: RuntimeLedger,
    pub runtime_error: Option<String>,
    pub notification_error: Option<String>,
    pub policy: ResolvedPolicy,
    pub config: Config,
    pub level: Level,
    level_since: f64,
    better_since: Option<f64>,
    process_history: BTreeMap<String, Vec<(f64, u64)>>,
    system_history: Vec<HistorySample>,
    warned: BTreeSet<String>,
    recovery_since: Option<f64>,
    critical_since: Option<f64>,
    assessment_better_since: Option<f64>,
    assessment_better_target: Option<Action>,
    last_cleanup: f64,
    tick_s: f64,
    leak_window_s: f64,
    hysteresis_s: f64,
    resume_cooldown_s: f64,
    leak_action: String,
    cli_hard_cap_mb: Option<f64>,
    logical_recovery_since: Option<f64>,
    codex_app_snapshot: CodexAppSnapshot,
    last_codex_app_hook_sync_at: f64,
}

impl Supervisor {
    pub fn new(runtime_path: Option<PathBuf>) -> Self {
        let mut config = Config::current();
        let platform = platform_name();
        let instance = instance_name(&platform, &config);
        let directory = runtime_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(state_dir);
        let runtime_path = runtime_path.unwrap_or_else(|| directory.join("runtime.json"));
        let (ledger, runtime_error) = RuntimeLedger::load(&runtime_path, &instance, now_epoch());
        let tick_s = numeric(
            &mut config,
            "MEMORY_SUPERVISOR_TICK_S",
            1.0,
            0.25,
            MAX_TICK_S,
        );
        let leak_window_s = numeric(
            &mut config,
            "MEMORY_SUPERVISOR_LEAK_WINDOW_S",
            30.0,
            5.0,
            3600.0,
        );
        let hysteresis_s = numeric(
            &mut config,
            "MEMORY_SUPERVISOR_HYSTERESIS_S",
            10.0,
            0.0,
            3600.0,
        );
        let resume_cooldown_s = numeric(
            &mut config,
            "MEMORY_SUPERVISOR_RESUME_COOLDOWN_S",
            60.0,
            0.0,
            86_400.0,
        );
        // Validate hook and sensor timing here so status exposes mistakes immediately.
        numeric(
            &mut config,
            "MEMORY_SUPERVISOR_PRETOOL_HOLD_S",
            PRETOOL_HOLD_DEFAULT_S,
            0.0,
            300.0,
        );
        numeric(
            &mut config,
            "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S",
            3.0,
            0.25,
            300.0,
        );
        let leak_action =
            config.validated_choice("MEMORY_SUPERVISOR_LEAK_ACTION", "stop", &["none", "stop"]);
        let cli_hard_cap_mb = hard_cap(&mut config);
        let policy = resolve_policy(&mut config, 8192);
        let level = ledger.level;
        let level_since = ledger.level_since;
        Self {
            platform,
            instance,
            directory,
            runtime_path,
            ledger,
            runtime_error,
            notification_error: None,
            policy,
            config,
            level,
            level_since,
            better_since: None,
            process_history: BTreeMap::new(),
            system_history: Vec::new(),
            warned: BTreeSet::new(),
            recovery_since: None,
            critical_since: None,
            assessment_better_since: None,
            assessment_better_target: None,
            last_cleanup: 0.0,
            tick_s,
            leak_window_s,
            hysteresis_s,
            resume_cooldown_s,
            leak_action,
            cli_hard_cap_mb,
            logical_recovery_since: None,
            codex_app_snapshot: CodexAppSnapshot::default(),
            last_codex_app_hook_sync_at: 0.0,
        }
    }

    pub fn tick_interval(&self) -> f64 {
        self.tick_s
    }

    fn persist_runtime(&mut self, now: f64) -> bool {
        if self.runtime_error.is_some() {
            return false;
        }
        self.ledger.level = self.level;
        self.ledger.level_since = self.level_since;
        match self.ledger.persist(&self.runtime_path, now) {
            Ok(()) => true,
            Err(error) => {
                log_event(
                    &self.directory,
                    &format!("RUNTIME_WRITE_ERROR error={error}"),
                );
                false
            }
        }
    }

    fn refresh_codex_app_adapter(&mut self, now: f64, processes: &BTreeMap<u32, ProcessInfo>) {
        let previous_adapter = self.ledger.codex_app.clone();
        let (mut snapshot, changed) = self.ledger.codex_app.reconcile(now, processes);
        if changed && !self.persist_runtime(now) {
            self.ledger.codex_app = previous_adapter;
            let mut scratch = self.ledger.codex_app.clone();
            snapshot = scratch.reconcile(now, processes).0;
        }
        if snapshot.detected {
            snapshot.ownership_capability = marker_reader_capability().to_owned();
        }
        let targets = hook_targets(&self.platform, processes);
        if !targets.is_empty() {
            let binary = env::current_exe().unwrap_or_else(|_| {
                PathBuf::from(if cfg!(windows) {
                    "memory-supervisor.exe"
                } else {
                    "memory-supervisor"
                })
            });
            let installed_binary = installed_binary_matches(&binary);
            let should_sync = installed_binary
                && now - self.last_codex_app_hook_sync_at >= 60.0
                && self.runtime_error.is_none();
            let mut sync_attempted = false;
            for target in targets {
                let sync = if should_sync && target.environment_resolved {
                    sync_attempted = true;
                    update_codex_app_hooks(&target.path, &binary, &self.platform, false).map(|_| ())
                } else {
                    Ok(())
                };
                let hook_health = sync
                    .as_ref()
                    .ok()
                    .map(|_| audit_codex_app_hooks(&target.path, &binary, &self.platform));
                let verified = hook_health.as_ref().is_some_and(|health| health.ready());
                let last_observed_at = self
                    .ledger
                    .codex_app
                    .threads
                    .values()
                    .filter(|thread| {
                        thread.app_server_pid == target.app_server_pid
                            && (thread.app_server_identity.is_empty()
                                || thread.app_server_identity == target.app_server_identity)
                    })
                    .map(|thread| thread.last_seen_at)
                    .filter(|observed| observed.is_finite() && *observed <= now + 1.0)
                    .max_by(f64::total_cmp);
                let route_modified_at = fs::metadata(&target.path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs_f64());
                let receipt_matches_current_route = last_observed_at
                    .zip(route_modified_at)
                    .is_some_and(|(observed, modified)| observed + 0.001 >= modified);
                let (status, active) = codex_app_hook_route_status(
                    target.environment_resolved,
                    verified,
                    last_observed_at,
                    route_modified_at,
                    now,
                );
                snapshot.hook_routes.push(CodexAppHookRoute {
                    app_server_pid: target.app_server_pid,
                    app_server_identity: target.app_server_identity,
                    path: target.path.display().to_string(),
                    platform: self.platform.clone(),
                    status: status.to_owned(),
                    detail: sync.err().unwrap_or_else(|| {
                        if !target.environment_resolved {
                            return "the active App Server CODEX_HOME could not be read; the fallback route is non-authoritative"
                                .to_owned();
                        }
                        if let Some(health) = hook_health.as_ref().filter(|health| !health.ready()) {
                            return format!(
                                "{}; next: {}",
                                health.summary(),
                                health.remediation(CodexHookSurface::App)
                            );
                        }
                        if active {
                            "the route matches and a recent App hook receipt proves that Codex is executing it"
                                .to_owned()
                        } else if verified
                                && last_observed_at.is_some()
                                && !receipt_matches_current_route
                        {
                                "the route is ready, but the latest App hook receipt predates the current hooks file; continue any existing App task with its next request to prove this version (no App restart or new task is required)"
                                    .to_owned()
                        } else if verified {
                                "the route is ready, but no recent App hook receipt proves execution; continue any existing App task with its next request (no App restart or new task is required)"
                                    .to_owned()
                        } else if !installed_binary {
                                "running binary is not an installed memory-supervisor".to_owned()
                        } else {
                                "native App hook route is missing or stale".to_owned()
                        }
                    })
                        .chars()
                        .take(500)
                        .collect(),
                    last_observed_at,
                });
            }
            if sync_attempted {
                self.last_codex_app_hook_sync_at = now;
            }
        }
        self.codex_app_snapshot = snapshot;
    }

    fn cleanup_artifacts(&mut self, now: f64) {
        if now - self.last_cleanup < 3600.0 {
            return;
        }
        self.last_cleanup = now;
        for (relative, retention) in [
            ("control/results", 86_400.0),
            ("notification-events/pending", 86_400.0),
            ("notification-events/results", 86_400.0),
            ("notification-events/acks", 86_400.0),
            ("hook-observations/pending", 86_400.0),
            ("app-guards", 86_400.0),
        ] {
            let Ok(entries) = fs::read_dir(self.directory.join(relative)) else {
                continue;
            };
            for entry in entries.flatten() {
                let active_guard_artifact = relative == "app-guards"
                    && self
                        .ledger
                        .codex_app
                        .control
                        .pending_physical
                        .as_ref()
                        .filter(|pending| !pending.guard_control_id.is_empty())
                        .is_some_and(|pending| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with(&format!("{}.", pending.guard_control_id))
                        });
                if active_guard_artifact {
                    continue;
                }
                let old = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age.as_secs_f64() > retention);
                if old {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        // Bound the logical roster: prune ended or long-silent sessions so it
        // cannot grow without limit.  A returning session re-registers on its
        // next hook.  Never prune a restricted agent (non-ACTIVE) or one whose
        // PID is currently paused.
        const LOGICAL_AGENT_RETENTION_S: f64 = 3600.0;
        let stopped: BTreeSet<String> = self.ledger.stopped.keys().cloned().collect();
        let app_working: BTreeSet<_> = self
            .ledger
            .codex_app
            .invocations
            .values()
            .filter(|invocation| invocation.ended_at.is_none())
            .map(|invocation| invocation.logical_key.clone())
            .chain(
                self.ledger
                    .codex_app
                    .process_owners
                    .values()
                    .map(|owner| owner.logical_key.clone()),
            )
            .collect();
        let before = self.ledger.logical_agents.len();
        self.ledger.logical_agents.retain(|_, agent| {
            agent.state != LogicalState::Active
                || now - agent.last_seen_at <= LOGICAL_AGENT_RETENTION_S
                || app_working.contains(&agent.key)
                || agent
                    .process_pid
                    .is_some_and(|pid| stopped.contains(&pid.to_string()))
        });
        if self.ledger.logical_agents.len() != before {
            let _ = self.persist_runtime(now);
        }
    }

    fn bump_logical_epoch(&mut self) -> u64 {
        self.ledger.logical_epoch = self.ledger.logical_epoch.saturating_add(1).max(1);
        self.ledger.logical_epoch
    }

    fn apply_hook_observation(&mut self, observation: HookObservation) -> bool {
        if !matches!(observation.provider.as_str(), "claude" | "codex")
            || observation.session_id.is_empty()
            || (observation.surface == APP_SERVER_SURFACE && observation.session_id == "nosid")
            || !observation.observed_at.is_finite()
        {
            return false;
        }
        self.ledger.codex_app.observe(&observation);
        let key = observation.key();
        let role = if observation.agent_id.is_some() {
            "subagent"
        } else {
            "lead"
        };
        let lifecycle_change = matches!(
            observation.event.as_str(),
            "SessionStart" | "SessionEnd" | "SubagentStart" | "SubagentStop"
        );
        let epoch = self.ledger.logical_epoch;
        let agent = self
            .ledger
            .logical_agents
            .entry(key.clone())
            .or_insert_with(|| LogicalAgent {
                key: key.clone(),
                provider: observation.provider.clone(),
                session_id: observation.session_id.clone(),
                agent_id: observation.agent_id.clone(),
                agent_type: observation
                    .agent_type
                    .clone()
                    .unwrap_or_else(|| role.to_owned()),
                role: role.to_owned(),
                process_pid: (observation.surface != crate::codex_app::APP_SERVER_SURFACE)
                    .then_some(observation.process_pid)
                    .flatten(),
                surface: observation.surface.clone(),
                epoch,
                state_since: observation.observed_at,
                started_at: observation.observed_at,
                last_seen_at: observation.observed_at,
                last_progress_at: observation.observed_at,
                ..LogicalAgent::default()
            });
        if observation.observed_at + 0.001 < agent.last_seen_at {
            return false;
        }
        let previous_state = agent.state;
        agent.last_seen_at = observation.observed_at;
        if observation.surface == crate::codex_app::APP_SERVER_SURFACE {
            // One app-server PID hosts many independent App leads. It is host inventory, never
            // the process identity of a logical lead or subagent.
            agent.process_pid = None;
            if let (Some(epoch), Some(state)) = (
                observation.observed_control_epoch,
                observation.observed_logical_state,
            ) {
                agent.last_hook_receipt_at = Some(observation.observed_at);
                agent.last_hook_receipt_epoch = Some(epoch);
                agent.last_hook_receipt_state = Some(state);
            }
        } else if let Some(pid) = observation.process_pid {
            agent.process_pid = Some(pid);
        }
        if !observation.surface.is_empty() {
            agent.surface = observation.surface.clone();
        }
        if let Some(agent_type) = observation.agent_type {
            agent.agent_type = agent_type;
        }
        match observation.event.as_str() {
            "SessionStart" | "SubagentStart" => {
                agent.active = true;
                agent.state = LogicalState::Active;
                agent.reason.clear();
                agent.evidence_stage.clear();
                agent.state_since = observation.observed_at;
                agent.started_at = observation.observed_at;
                agent.last_progress_at = observation.observed_at;
                agent.idle_since = None;
                agent.in_flight_tool_class = None;
                agent.last_blocked_at = None;
                agent.last_blocked_tool = None;
                agent.last_blocked_reason = None;
                agent.last_blocked_epoch = None;
                agent.last_hook_receipt_at = observation
                    .observed_control_epoch
                    .map(|_| observation.observed_at);
                agent.last_hook_receipt_epoch = observation.observed_control_epoch;
                agent.last_hook_receipt_state = observation.observed_logical_state;
            }
            "SessionEnd" | "SubagentStop" => {
                agent.active = false;
                agent.state = LogicalState::Active;
                agent.reason = if agent
                    .last_blocked_at
                    .is_some_and(|at| at >= agent.started_at)
                {
                    format!(
                        "provider lifecycle ended after memory-supervisor blocked {}",
                        agent.last_blocked_tool.as_deref().unwrap_or("a tool")
                    )
                } else {
                    "provider lifecycle completed".to_owned()
                };
                agent.state_since = observation.observed_at;
                agent.last_progress_at = observation.observed_at;
                agent.idle_since = Some(observation.observed_at);
                agent.in_flight_tool_class = None;
            }
            "UserPromptSubmit" | "BeforeAgent" => {
                agent.active = true;
                agent.idle_since = None;
                agent.last_progress_at = observation.observed_at;
            }
            "PreToolUse" => {
                agent.active = true;
                agent.idle_since = None;
                agent.last_tool_class = observation.tool_class;
                if observation.blocked {
                    agent.in_flight_tool_class = None;
                    agent.last_blocked_at = Some(observation.observed_at);
                    agent.last_blocked_tool = observation.tool_name;
                    agent.last_blocked_reason = observation
                        .block_reason
                        .or_else(|| (!agent.reason.is_empty()).then(|| agent.reason.clone()));
                    agent.last_blocked_epoch = Some(agent.epoch);
                } else {
                    agent.in_flight_tool_class = observation.tool_class;
                    if matches!(
                        observation.tool_class,
                        Some(ToolClass::Expansion | ToolClass::HighMemoryStart)
                    ) {
                        agent.last_heavy_at = Some(observation.observed_at);
                    }
                }
            }
            "PostToolUse" | "AfterTool" | "PostToolBatch" => {
                agent.last_progress_at = observation.observed_at;
                agent.last_tool_class = observation.tool_class.or(agent.last_tool_class);
                agent.in_flight_tool_class = None;
            }
            "Stop" => {
                agent.last_progress_at = observation.observed_at;
                agent.idle_since = Some(observation.observed_at);
                agent.in_flight_tool_class = None;
                agent.completed_turns = agent.completed_turns.saturating_add(1);
            }
            _ => {}
        }
        // Normal provider lifecycle changes are inventory, not user-visible
        // containment epochs.  Advance the global epoch only when lifecycle
        // completion/start actually removes a restricted state from the roster.
        // Otherwise every short probe or ordinary session close makes unrelated
        // leads receive a spurious "ACTIVE" control notice.
        if lifecycle_change && previous_state != LogicalState::Active {
            let epoch = self.bump_logical_epoch();
            if let Some(agent) = self.ledger.logical_agents.get_mut(&key) {
                agent.epoch = epoch;
            }
        }
        true
    }

    fn drain_hook_observations(&mut self, now: f64) {
        let pending = self.directory.join("hook-observations").join("pending");
        let _ = ensure_private_dir(&pending);
        let Ok(entries) = fs::read_dir(&pending) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"))
            .collect();
        paths.sort();
        paths.truncate(4096);
        if paths.is_empty() {
            return;
        }
        let previous = self.ledger.clone();
        let mut consumed = Vec::new();
        let mut changed = false;
        for path in paths {
            let observation = fs::read(&path)
                .ok()
                .and_then(|source| serde_json::from_slice::<HookObservation>(&source).ok());
            let Some(observation) = observation else {
                consumed.push(path);
                continue;
            };
            if now - observation.observed_at > 86_400.0 || observation.observed_at - now > 30.0 {
                consumed.push(path);
                continue;
            }
            changed |= self.apply_hook_observation(observation);
            consumed.push(path);
        }
        if changed && !self.persist_runtime(now) {
            self.ledger = previous;
            return;
        }
        for path in consumed {
            let _ = fs::remove_file(path);
        }
    }

    fn notification_event(
        &self,
        event_type: &str,
        status: &str,
        message: &str,
        dedupe_key: &str,
        fields: Map<String, Value>,
    ) -> Value {
        let mut fields = fields;
        // These are evidence and actuator steps inside one protection episode.
        // The episode edge is the only normal user notification; exact terminal
        // pause/resume notices are delivered separately at the actuator.
        if pressure_episode_component(event_type, status) {
            fields.insert("importance".to_owned(), Value::String("detail".to_owned()));
        }
        fields.insert("platform".to_owned(), Value::String(self.platform.clone()));
        make_event(
            event_type,
            status,
            message,
            &self.instance,
            dedupe_key,
            fields,
        )
    }

    fn queue_notification_event(&mut self, event: &Value) -> bool {
        let known: BTreeSet<_> = self
            .ledger
            .notification_events
            .iter()
            .filter_map(|event| event.get("event_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        match queue_event(&self.directory, event, &known) {
            Ok(_) => {
                self.notification_error = None;
                true
            }
            Err(error) => {
                self.notification_error = Some(error.to_string());
                log_event(
                    &self.directory,
                    &format!("NOTIFICATION_QUEUE_ERROR error={error}"),
                );
                false
            }
        }
    }

    fn emit_event(
        &mut self,
        event_type: &str,
        status: &str,
        message: &str,
        dedupe_key: &str,
        fields: Map<String, Value>,
    ) {
        let event = self.notification_event(event_type, status, message, dedupe_key, fields);
        self.queue_notification_event(&event);
    }

    fn flush_pending_pressure_episode_event(&mut self, now: f64) {
        let Some(event) = self.ledger.pending_pressure_episode_event.clone() else {
            return;
        };
        if !self.queue_notification_event(&event) {
            return;
        }
        self.ledger.pending_pressure_episode_event = None;
        if !self.persist_runtime(now) {
            self.ledger.pending_pressure_episode_event = Some(event);
        }
    }

    fn update_pressure_episode(&mut self, assessment: &Assessment, now: f64) {
        self.flush_pending_pressure_episode_event(now);
        if self.ledger.pending_pressure_episode_event.is_some() {
            return;
        }
        let restricted = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| agent.active && agent.state != LogicalState::Active)
            .count();
        let active = assessment.action >= Action::Hold
            || restricted > 0
            || self.ledger.codex_app.control.surface_gate
            || self.ledger.codex_app.control.pending_physical.is_some()
            || !self.ledger.stopped.is_empty()
            || self.ledger.probation.is_some();
        let previous_started_at = self.ledger.pressure_episode_started_at;
        if active == previous_started_at.is_some() {
            return;
        }

        let episode_started_at = previous_started_at.unwrap_or(now);
        let paused = self.ledger.stopped.len();
        let tte = assessment
            .time_to_recovery_reserve_s
            .filter(|value| value.is_finite())
            .map(|value| format!("; projected time to the recovery reserve is {value:.1}s"))
            .unwrap_or_default();
        let lost_pids: BTreeSet<_> = self
            .ledger
            .incidents
            .iter()
            .filter(|incident| incident.get("status").and_then(Value::as_str) == Some("gone"))
            .filter(|incident| {
                incident
                    .get("gone_at")
                    .and_then(Value::as_f64)
                    .is_some_and(|gone_at| gone_at >= episode_started_at && gone_at <= now)
            })
            .map(|incident| value_u32(incident, "pid"))
            .filter(|pid| *pid > 1)
            .collect();
        let (status, action, severity, recovery, message) = if active {
            (
                "active",
                "protect",
                "critical",
                "pending",
                format!(
                    "[Memory Supervisor] MEMORY PROTECTION ACTIVE\nWhy: {} MiB of {} MiB remains{tte}; distress is {}; attribution is {}.\nEffect: the calculated braking boundary is active. New-work admission, logical cushions, and exact-PID pauses may proceed only as required; their individual steps are recorded without sending repeated remote alerts.\nCurrent containment: {restricted} live logical agents restricted; {paused} exact PIDs paused.\nNext: preserve work and use `memory-status` for live evidence. Recovery is announced only after the braking decision, logical restrictions, managed pauses, and probation are all clear.",
                    assessment.mem_available_mb,
                    assessment.memory_capacity_mb,
                    assessment.distress,
                    assessment.attribution,
                ),
            )
        } else if !lost_pids.is_empty() {
            let pids = lost_pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            (
                "ended-with-loss",
                "review",
                "warning",
                "workers-gone",
                format!(
                    "[Memory Supervisor] MEMORY PROTECTION ENDED — WORKER LOSS DETECTED\nWhy: HOLD/DRAIN and all protection actuators are clear, but paused worker PIDs {pids} disappeared before a confirmed resume.\nEffect: machine protection has ended, but workload recovery is incomplete; affected subagent results may be missing or partial.\nNext: inspect the lead handoff and incident ledger, then deliberately resume the provider workflow or rerun only the incomplete tasks."
                ),
            )
        } else {
            (
                "recovered",
                "recovered",
                "info",
                "clean",
                "[Memory Supervisor] MEMORY PROTECTION RECOVERED\nWhy: the calculated braking decision is clear, every live logical agent is ACTIVE, and no App surface gate, supervisor-managed PID, or probation remains.\nEffect: the protection episode is complete; component actions remain in the event ledger, and normal admission may continue under the current decision.\nNext: no recovery command is required; check `memory-status` before new fan-out if the workload is still memory-heavy.".to_owned(),
            )
        };
        let event = self.notification_event(
            "pressure-episode",
            status,
            &message,
            &format!("episode:{episode_started_at:.3}"),
            Map::from_iter([
                ("severity".to_owned(), Value::String(severity.to_owned())),
                (
                    "cause".to_owned(),
                    Value::String("pressure-episode-edge".to_owned()),
                ),
                ("action".to_owned(), Value::String(action.to_owned())),
                ("recovery".to_owned(), Value::String(recovery.to_owned())),
                ("headroom_mb".to_owned(), json!(assessment.mem_available_mb)),
                (
                    "capacity_mb".to_owned(),
                    json!(assessment.memory_capacity_mb),
                ),
                ("tte_s".to_owned(), json!(assessment.time_to_exhaustion_s)),
                (
                    "reserve_mb".to_owned(),
                    json!(assessment.automatic_reserve_mb),
                ),
                (
                    "attribution".to_owned(),
                    Value::String(assessment.attribution.clone()),
                ),
                (
                    "distress".to_owned(),
                    Value::String(assessment.distress.clone()),
                ),
            ]),
        );
        self.ledger.pressure_episode_started_at = active.then_some(now);
        self.ledger.pending_pressure_episode_event = Some(event);
        if !self.persist_runtime(now) {
            self.ledger.pressure_episode_started_at = previous_started_at;
            self.ledger.pending_pressure_episode_event = None;
            return;
        }
        self.flush_pending_pressure_episode_event(now);
    }

    fn drain_notification_events(&mut self, now: f64) {
        let config = load_notification_config(&notification_config_path());
        let channels = notification_channels(&config);
        self.drain_notification_events_with(now, &channels, spawn_dispatcher);
    }

    fn drain_notification_events_with<F>(
        &mut self,
        now: f64,
        channels: &BTreeSet<String>,
        mut dispatch: F,
    ) where
        F: FnMut(&Value, &Path) -> io::Result<()>,
    {
        let results_dir = self.directory.join("notification-events/results");
        let mut changed = false;
        let mut consumed_paths = Vec::new();
        let result_paths = fs::read_dir(&results_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"));
        for path in result_paths {
            let Some(result) = fs::read(&path)
                .ok()
                .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
                .and_then(|value| value.as_object().cloned())
            else {
                continue;
            };
            let Some(event_id) = result.get("event_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(deliveries) = result.get("deliveries").and_then(Value::as_object) else {
                continue;
            };
            let Some(event) = self
                .ledger
                .notification_events
                .iter_mut()
                .find(|event| event.get("event_id").and_then(Value::as_str) == Some(event_id))
            else {
                continue;
            };
            let object = event.as_object_mut().expect("event object");
            let current = object
                .entry("deliveries".to_owned())
                .or_insert_with(|| json!({}));
            let current = current.as_object_mut().expect("deliveries object");
            for transport in ["os", "discord", "telegram"] {
                if let Some(status) = deliveries.get(transport).and_then(Value::as_str)
                    && matches!(status, "delivered" | "failed" | "skipped" | "unavailable")
                    && current.get(transport).and_then(Value::as_str) != Some("delivered")
                {
                    current.insert(transport.to_owned(), Value::String(status.to_owned()));
                }
            }
            if let Some(details) = result.get("delivery_details").and_then(Value::as_object) {
                object.insert(
                    "delivery_details".to_owned(),
                    Value::Object(Map::from_iter(details.iter().map(|(key, value)| {
                        (
                            key.chars().take(64).collect(),
                            Value::String(
                                value
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| value.to_string())
                                    .chars()
                                    .take(256)
                                    .collect(),
                            ),
                        )
                    }))),
                );
            }
            object.insert("delivered_at".to_owned(), json!(rounded(now, 3)));
            consumed_paths.push(path);
            changed = true;
        }

        let known: BTreeSet<_> = self
            .ledger
            .notification_events
            .iter()
            .filter_map(|event| event.get("event_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        let pending = pending_events(&self.directory, &known);
        let mut consumed_pending = Vec::new();
        for mut event in pending {
            let notify = event_should_notify(&event);
            let terminal_status = event
                .get("terminal")
                .and_then(Value::as_str)
                .filter(|value| {
                    matches!(*value, "delivered" | "failed" | "skipped" | "unavailable")
                })
                .unwrap_or("skipped")
                .to_owned();
            event.as_object_mut().expect("event object").insert(
                "deliveries".to_owned(),
                json!({
                    "hook": if notify { "pending" } else { "skipped" },
                    "terminal": terminal_status,
                    "os": if notify && channels.contains("os") { "pending" } else { "skipped" },
                    "discord": if notify && channels.contains("discord") { "pending" } else { "skipped" },
                    "telegram": if notify && channels.contains("telegram") { "pending" } else { "skipped" },
                }),
            );
            if let Some(id) = event.get("event_id").and_then(Value::as_str) {
                consumed_pending.push(id.to_owned());
            }
            self.ledger.notification_events.push(event);
            changed = true;
        }
        for (path, acknowledgement) in pending_acknowledgements(&self.directory) {
            let event_id = value_string(&acknowledgement, "event_id");
            let transport = value_string(&acknowledgement, "transport");
            let status = value_string(&acknowledgement, "status");
            if let Some(event) = self.ledger.notification_events.iter_mut().find(|event| {
                event.get("event_id").and_then(Value::as_str) == Some(event_id.as_str())
            }) {
                let object = event.as_object_mut().expect("event object");
                let deliveries = object
                    .entry("deliveries".to_owned())
                    .or_insert_with(|| json!({}));
                if let Some(deliveries) = deliveries.as_object_mut()
                    && deliveries.get(&transport).and_then(Value::as_str) != Some("delivered")
                {
                    deliveries.insert(transport, Value::String(status));
                }
                consumed_paths.push(path);
                changed = true;
            }
        }

        for event in &mut self.ledger.notification_events {
            let started = event
                .get("dispatch_started_at")
                .and_then(Value::as_f64)
                .unwrap_or(now);
            let Some(deliveries) = event.get_mut("deliveries").and_then(Value::as_object_mut)
            else {
                continue;
            };
            if now - started < 30.0 {
                continue;
            }
            let mut expired = false;
            for transport in ["os", "discord", "telegram"] {
                if deliveries.get(transport).and_then(Value::as_str) == Some("pending") {
                    deliveries.insert(transport.to_owned(), Value::String("failed".to_owned()));
                    expired = true;
                }
            }
            if expired {
                event
                    .as_object_mut()
                    .expect("event object")
                    .insert("delivery_timeout_at".to_owned(), json!(rounded(now, 3)));
                changed = true;
            }
        }
        let cutoff = now - INCIDENT_RETENTION_S;
        let before = self.ledger.notification_events.len();
        self.ledger.notification_events.retain(|event| {
            event
                .get("created_at")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or_default()
                >= cutoff
        });
        if self.ledger.notification_events.len() > 128 {
            let excess = self.ledger.notification_events.len() - 128;
            self.ledger.notification_events.drain(..excess);
        }
        changed |= before != self.ledger.notification_events.len();

        if changed {
            if !self.persist_runtime(now) {
                self.notification_error =
                    Some("notification ledger could not be persisted".to_owned());
                let new_ids: BTreeSet<_> = consumed_pending.iter().cloned().collect();
                self.ledger.notification_events.retain(|event| {
                    event
                        .get("event_id")
                        .and_then(Value::as_str)
                        .is_none_or(|event_id| !new_ids.contains(event_id))
                });
                return;
            }
            self.notification_error = None;
            for event_id in &consumed_pending {
                let _ = fs::remove_file(
                    self.directory
                        .join("notification-events/pending")
                        .join(format!("{event_id}.json")),
                );
            }
            for path in consumed_paths {
                let _ = fs::remove_file(path);
            }
        }

        let mut dispatch_changed = false;
        for event in &mut self.ledger.notification_events {
            let pending = event
                .get("deliveries")
                .and_then(Value::as_object)
                .is_some_and(|deliveries| {
                    ["os", "discord", "telegram"].into_iter().any(|transport| {
                        deliveries.get(transport).and_then(Value::as_str) == Some("pending")
                    })
                });
            if !pending || event.get("dispatch_started_at").is_some() {
                continue;
            }
            let Some(event_id) = event
                .get("event_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            let result_path = results_dir.join(format!("{event_id}.json"));
            let dispatch_result = dispatch(event, &result_path);
            let object = event.as_object_mut().expect("event object");
            if let Err(error) = dispatch_result {
                self.notification_error = Some(format!("dispatcher start failed: {error}"));
                log_event(
                    &self.directory,
                    &format!("NOTIFICATION_DISPATCH_ERROR error={error}"),
                );
                if let Some(deliveries) =
                    object.get_mut("deliveries").and_then(Value::as_object_mut)
                {
                    for transport in ["os", "discord", "telegram"] {
                        if deliveries.get(transport).and_then(Value::as_str) == Some("pending") {
                            deliveries
                                .insert(transport.to_owned(), Value::String("failed".to_owned()));
                        }
                    }
                }
            } else {
                object.insert("dispatch_started_at".to_owned(), json!(rounded(now, 3)));
            }
            dispatch_changed = true;
        }
        if dispatch_changed {
            if !self.persist_runtime(now) {
                self.notification_error =
                    Some("notification dispatch state could not be persisted".to_owned());
            } else {
                self.notification_error = None;
            }
        }
    }

    fn update_level(&mut self, raw: Level, now: f64, mem: u64, psi: f64) -> Option<(Level, Level)> {
        if raw > self.level {
            let previous = self.level;
            self.level = raw;
            self.level_since = now;
            self.better_since = None;
            log_event(
                &self.directory,
                &format!("ESCALATE {:?} mem={mem} psi={psi}", self.level),
            );
            return Some((previous, self.level));
        } else if raw < self.level {
            match self.better_since {
                None => self.better_since = Some(now),
                Some(since) if now - since >= self.hysteresis_s => {
                    let previous = self.level;
                    self.level = match self.level {
                        Level::Red => Level::Orange,
                        Level::Orange => Level::Yellow,
                        Level::Yellow | Level::Green => Level::Green,
                    };
                    self.level_since = now;
                    self.better_since = Some(now);
                    log_event(
                        &self.directory,
                        &format!("RELAX {:?} mem={mem} psi={psi}", self.level),
                    );
                    return Some((previous, self.level));
                }
                _ => {}
            }
        } else {
            self.better_since = None;
        }
        None
    }

    fn record_transition(
        &mut self,
        previous: Level,
        current: Level,
        mem: u64,
        psi: f64,
        assessment: &Assessment,
    ) {
        let floor = self.config.validated_choice(
            "MEMORY_SUPERVISOR_LEDGER_MIN_LEVEL",
            "yellow",
            &["green", "yellow", "orange", "red"],
        );
        let floor = match floor.as_str() {
            "green" => Level::Green,
            "orange" => Level::Orange,
            "red" => Level::Red,
            _ => Level::Yellow,
        };
        if previous.max(current) < floor {
            return;
        }
        let name = |level: Level| match level {
            Level::Green => "GREEN",
            Level::Yellow => "YELLOW",
            Level::Orange => "ORANGE",
            Level::Red => "RED",
        };
        let direction = if current > previous {
            "worsened"
        } else {
            "recovered"
        };
        let message = format!(
            "[{}] {} -> {} ({direction}); MemAvailable {mem}MB; PSI {psi}; adaptive action={:?} (`memory-status`).",
            self.platform,
            name(previous),
            name(current),
            assessment.action
        );
        self.emit_event(
            "utilization-transition",
            &name(current).to_lowercase(),
            &message,
            &format!(
                "{}:{}:{:.3}",
                name(previous),
                name(current),
                self.level_since
            ),
            Map::from_iter([
                (
                    "severity".to_owned(),
                    Value::String(
                        if assessment.action == Action::Drain {
                            "critical"
                        } else if assessment.action == Action::Hold {
                            "warning"
                        } else {
                            "info"
                        }
                        .to_owned(),
                    ),
                ),
                ("headroom_mb".to_owned(), json!(mem)),
                (
                    "cause".to_owned(),
                    Value::String("utilization-threshold".to_owned()),
                ),
                (
                    "action".to_owned(),
                    serde_json::to_value(assessment.action).unwrap(),
                ),
                ("importance".to_owned(), Value::String("detail".to_owned())),
            ]),
        );
    }

    pub fn stabilize_assessment(&mut self, assessment: &mut Assessment, now: f64) -> bool {
        let proposed = assessment.action;
        let previous = self.ledger.last_assessment_action;
        let mut changed = false;
        if previous.is_none_or(|previous| proposed > previous) {
            self.ledger.last_assessment_action = Some(proposed);
            self.ledger.action_since = now;
            self.assessment_better_since = None;
            self.assessment_better_target = None;
            changed = true;
        } else if previous.is_some_and(|previous| proposed < previous) {
            // Time the window on "any proposal below the held level" and adopt the most
            // conservative proposal seen in it: an Allow/Observe alternation below a held
            // Hold must not reset recovery, or the hold outlives every measurement.
            if self.assessment_better_since.is_none() {
                self.assessment_better_since = Some(now);
                self.assessment_better_target = Some(proposed);
            } else if self
                .assessment_better_target
                .is_none_or(|target| proposed > target)
            {
                self.assessment_better_target = Some(proposed);
            }
            let recovery_window = self.hysteresis_s.max(assessment.reaction_s * 2.0);
            if now - self.assessment_better_since.unwrap_or(now) >= recovery_window {
                let adopted = self
                    .assessment_better_target
                    .unwrap_or(proposed)
                    .max(proposed);
                self.ledger.last_assessment_action = Some(adopted);
                self.ledger.action_since = now;
                self.assessment_better_since = None;
                self.assessment_better_target = None;
                changed = true;
            }
        } else {
            self.assessment_better_since = None;
            self.assessment_better_target = None;
        }
        let effective = self.ledger.last_assessment_action.unwrap_or(proposed);
        assessment.action = effective;
        assessment.admission_level = effective.level();
        changed
    }

    pub fn analyze_processes(
        &mut self,
        now: f64,
        mut tracked: Vec<TrackedProcess>,
        assessment: &Assessment,
    ) -> (Vec<TrackedProcess>, Vec<TrackedProcess>) {
        let live: BTreeSet<_> = tracked
            .iter()
            .map(|process| process.identity.clone())
            .collect();
        let warn_mb = self.policy.value("MEMORY_SUPERVISOR_LEAK_RSS_MB");
        for process in &mut tracked {
            let history = self
                .process_history
                .entry(process.identity.clone())
                .or_default();
            history.push((now, process.anon_mb));
            history.retain(|(timestamp, _)| now - timestamp <= self.leak_window_s);
            let metrics = growth_metrics(history);
            process.slope_mb_s = rounded(metrics.long_slope_mb_s, 2);
            process.recent_slope_mb_s = rounded(metrics.recent_slope_mb_s, 2);
            process.monotonicity = rounded(metrics.monotonicity, 3);
            process.growth_delta_mb = rounded(metrics.delta_mb, 1);
            process.observation_span_s = rounded(metrics.span_s, 1);
        }
        self.process_history
            .retain(|identity, _| live.contains(identity));
        self.ledger
            .runaway_confirmations
            .retain(|identity, _| live.contains(identity));
        self.warned.retain(|identity| live.contains(identity));
        let total_positive_growth: f64 = tracked
            .iter()
            .map(|process| process.slope_mb_s.max(0.0))
            .sum();
        let physical_or_commit = assessment
            .commit_remaining_mb
            .map(|value| value.min(assessment.mem_available_mb) as f64)
            .unwrap_or(assessment.mem_available_mb as f64);
        let usable_headroom = (physical_or_commit - assessment.automatic_reserve_mb).max(0.0);
        let mut leaks = Vec::new();
        for index in 0..tracked.len() {
            let process_pid = tracked[index].pid;
            let work_mismatch_since = self.process_work_mismatch_since(process_pid);
            let evidence: Vec<_> = self
                .process_history
                .get(&tracked[index].identity)
                .into_iter()
                .flatten()
                .filter(|(timestamp, _)| {
                    work_mismatch_since.is_none_or(|since| *timestamp >= since)
                })
                .copied()
                .collect();
            let evidence_metrics = growth_metrics(&evidence);
            let same_role_peer_slopes = tracked
                .iter()
                .enumerate()
                .filter(|(peer_index, peer)| {
                    *peer_index != index && peer.role == tracked[index].role
                })
                .map(|(_, peer)| peer.slope_mb_s.max(0.0))
                .collect();
            let mut verdict = evaluate_runaway(&RunawayInputs {
                identity_reliable: tracked[index].identity_reliable,
                owned_mb: tracked[index].anon_mb as f64,
                warning_mb: warn_mb,
                delta_mb: evidence_metrics.delta_mb,
                sample_count: evidence_metrics.sample_count,
                observation_span_s: evidence_metrics.span_s,
                observation_s: self.leak_window_s,
                monotonicity: evidence_metrics.monotonicity,
                long_slope_mb_s: evidence_metrics.long_slope_mb_s,
                recent_slope_mb_s: evidence_metrics.recent_slope_mb_s,
                usable_headroom_mb: usable_headroom,
                automatic_reserve_mb: assessment.automatic_reserve_mb,
                reaction_s: assessment.reaction_s,
                native_confidence: assessment.native_confidence.clone(),
                attribution: assessment.attribution.clone(),
                work_mismatch: work_mismatch_since.is_some(),
                headroom_fall_mb_s: assessment.headroom_fall_mb_s,
                total_positive_growth_mb_s: total_positive_growth,
                same_role_peer_slopes_mb_s: same_role_peer_slopes,
            });
            let identity = tracked[index].identity.clone();
            let verified = if verdict.gates.complete() {
                let confirmation = self
                    .ledger
                    .runaway_confirmations
                    .entry(identity.clone())
                    .or_insert(RunawayConfirmation {
                        first_complete_at: now,
                        last_complete_at: now,
                    });
                confirmation.last_complete_at = now;
                now - confirmation.first_complete_at >= assessment.reaction_s
            } else {
                self.ledger.runaway_confirmations.remove(&identity);
                false
            };
            if verified {
                verdict.stage = "RUNAWAY_VERIFIED".to_owned();
            }
            tracked[index].runaway_verified = verified;
            tracked[index].strong_leak = verified;
            tracked[index].runaway = Some(verdict.clone());
            if matches!(
                verdict.stage.as_str(),
                "RUNAWAY_SUSPECT" | "RUNAWAY_VERIFIED"
            ) {
                leaks.push(tracked[index].clone());
            }
        }
        (tracked, leaks)
    }

    fn process_work_mismatch_since(&self, pid: u32) -> Option<f64> {
        let agents = self.logical_agents_for_pid(pid, false);
        if agents.is_empty()
            || agents.iter().any(|agent| {
                agent.in_flight_tool_class.is_some() || (agent.active && agent.idle_since.is_none())
            })
        {
            return None;
        }
        agents
            .iter()
            .map(|agent| agent.idle_since)
            .collect::<Option<Vec<_>>>()
            .and_then(|values| values.into_iter().max_by(f64::total_cmp))
    }

    fn logical_agents_for_pid(&self, pid: u32, active_only: bool) -> Vec<&LogicalAgent> {
        let owner_key = self
            .ledger
            .codex_app
            .owner_for_pid(pid)
            .map(|owner| owner.logical_key.as_str());
        self.ledger
            .logical_agents
            .values()
            .filter(|agent| {
                (!active_only || agent.active)
                    && (owner_key == Some(agent.key.as_str()) || agent.process_pid == Some(pid))
            })
            .collect()
    }

    fn process_runaway(tracked: &[TrackedProcess], pid: u32) -> Option<&TrackedProcess> {
        tracked.iter().find(|process| process.pid == pid)
    }

    fn logical_processes<'a>(
        &self,
        agent: &LogicalAgent,
        tracked: &'a [TrackedProcess],
    ) -> Vec<&'a TrackedProcess> {
        if agent.surface == crate::codex_app::APP_SERVER_SURFACE {
            let owned: BTreeSet<_> = self
                .ledger
                .codex_app
                .owned_pids_for_logical(&agent.key)
                .into_iter()
                .collect();
            tracked
                .iter()
                .filter(|process| owned.contains(&process.pid))
                .collect()
        } else {
            agent
                .process_pid
                .and_then(|pid| Self::process_runaway(tracked, pid))
                .into_iter()
                .collect()
        }
    }

    fn codex_app_shared_host(&self, pid: u32) -> bool {
        self.ledger.codex_app.is_shared_host(pid)
            || self
                .codex_app_snapshot
                .app_servers
                .iter()
                .any(|server| server.pid == pid)
    }

    fn codex_app_physical_control_forbidden(&self, pid: u32) -> bool {
        self.codex_app_shared_host(pid)
            || self
                .ledger
                .codex_app
                .owner_for_pid(pid)
                .is_some_and(|owner| !owner.evidence.control_safe())
            || self
                .codex_app_snapshot
                .app_servers
                .iter()
                .any(|server| server.unattributed_pids.contains(&pid))
    }

    fn codex_app_hook_active_for(&self, app_server_pid: u32) -> bool {
        let Some(server_identity) = self
            .codex_app_snapshot
            .app_servers
            .iter()
            .find(|server| server.pid == app_server_pid)
            .map(|server| server.identity.as_str())
        else {
            return false;
        };
        self.codex_app_snapshot.hook_routes.iter().any(|route| {
            route.app_server_pid == app_server_pid
                && route.app_server_identity == server_identity
                && route.status == "ACTIVE"
        })
    }

    fn codex_app_hooks_active_for_all_servers(&self) -> bool {
        !self.codex_app_snapshot.app_servers.is_empty()
            && self
                .codex_app_snapshot
                .app_servers
                .iter()
                .all(|server| self.codex_app_hook_active_for(server.pid))
    }

    fn codex_app_blind_target_scope(
        &self,
    ) -> (BTreeMap<u32, u32>, BTreeMap<u32, BTreeSet<String>>) {
        let mut target_to_server = BTreeMap::<u32, u32>::new();
        let mut related_by_pid = BTreeMap::<u32, BTreeSet<String>>::new();
        for server in &self.codex_app_snapshot.app_servers {
            for pid in &server.blind_control_pids {
                target_to_server.insert(*pid, server.pid);
                related_by_pid.insert(
                    *pid,
                    server
                        .blind_candidate_keys
                        .get(&pid.to_string())
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                );
            }
        }
        for owner in self
            .ledger
            .codex_app
            .process_owners
            .values()
            .filter(|owner| !owner.evidence.control_safe())
        {
            target_to_server.insert(owner.pid, owner.app_server_pid);
            // Estimated ownership can rank a logical target, but physical authority remains
            // server-wide and therefore waits for every effective key on that server.
            related_by_pid.insert(
                owner.pid,
                self.active_app_keys_for_server(owner.app_server_pid),
            );
        }
        (target_to_server, related_by_pid)
    }

    fn app_physical_identity_available(&self, process: &TrackedProcess) -> bool {
        process.identity_reliable
            && process.pid != std::process::id()
            && self.ledger.stopped_identity(process.pid).is_none()
            && !self.ledger.resume_cooldown.contains_key(&process.identity)
    }

    fn app_physical_growth_ready(
        &self,
        process: &TrackedProcess,
        app_growth_mb_s: f64,
        minimum_span: f64,
    ) -> bool {
        self.app_physical_identity_available(process)
            && process.slope_mb_s >= 1.0
            && process.recent_slope_mb_s >= 1.0
            && process.monotonicity >= 0.8
            && process.observation_span_s >= minimum_span
            && process.growth_delta_mb >= 128.0
            && process.slope_mb_s >= (app_growth_mb_s * 0.25).max(1.0)
    }

    fn app_effective_backstop_growth_ready(
        &self,
        process: &TrackedProcess,
        app_growth_mb_s: f64,
        minimum_span: f64,
    ) -> bool {
        self.app_physical_growth_ready(process, app_growth_mb_s, minimum_span)
            && process.slope_mb_s >= (app_growth_mb_s * 0.50).max(1.0)
    }

    fn pressure_target_effective_or_non_app(&self, process: &TrackedProcess) -> bool {
        if self
            .ledger
            .codex_app
            .control_safe_owner_for_pid(process.pid)
            .is_none()
        {
            return true;
        }
        let profile = &self.codex_app_snapshot.pressure;
        let minimum_span = (self.leak_window_s * 0.8).max(5.0);
        profile.causal
            && self.app_physical_growth_ready(process, profile.app_growth_mb_s, minimum_span)
    }

    fn app_backstop_keys_reachable(
        &self,
        keys: &BTreeSet<String>,
        candidate_keys: &BTreeSet<String>,
        now: f64,
        reaction_s: f64,
    ) -> bool {
        if keys.is_empty() {
            return false;
        }
        if keys.iter().all(|key| {
            self.ledger
                .logical_agents
                .get(key)
                .is_some_and(|agent| self.app_agent_handoff_receipt_current(agent, now, reaction_s))
        }) {
            return true;
        }
        // A stopped/idle App lead cannot acknowledge a future control epoch until the user starts
        // another turn. Counting its old receipt while other keys still have a ladder to traverse
        // makes the receipt expire before the physical brake is reachable. Either the whole scope
        // is ready now, or every required key must still have a live hook boundary ahead of it.
        keys.iter().all(|key| {
            self.ledger.logical_agents.get(key).is_some_and(|agent| {
                agent.active
                    && agent.surface == APP_SERVER_SURFACE
                    && candidate_keys.contains(key)
                    && agent.idle_since.is_none()
            })
        })
    }

    fn app_backstop_receipt_budget_s(
        &self,
        backstop: &CodexAppBackstopPlan,
        now: f64,
        reaction_s: f64,
    ) -> Option<f64> {
        if !backstop.available() {
            return None;
        }
        backstop
            .required_keys
            .iter()
            .map(|key| {
                let agent = self.ledger.logical_agents.get(key)?;
                if self.app_agent_handoff_receipt_current(agent, now, reaction_s) {
                    let age = now - agent.last_hook_receipt_at?;
                    Some((CODEX_APP_HOOK_ROUTE_FRESH_S - age).max(0.0))
                } else {
                    // A live, non-idle candidate will earn its receipt after transition; the
                    // whole same-generation hook freshness window is then available.
                    Some(CODEX_APP_HOOK_ROUTE_FRESH_S)
                }
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .min_by(f64::total_cmp)
    }

    fn app_host_growth_ready(
        &self,
        process: &TrackedProcess,
        app_growth_mb_s: f64,
        child_growth_mb_s: f64,
        minimum_span: f64,
    ) -> bool {
        self.app_physical_identity_available(process)
            && process.slope_mb_s >= 4.0
            && process.recent_slope_mb_s >= 1.0
            && process.monotonicity >= 0.8
            && process.observation_span_s >= minimum_span
            && process.slope_mb_s >= app_growth_mb_s * 0.50
            && process.slope_mb_s >= child_growth_mb_s
            && !self.active_app_keys_for_server(process.pid).is_empty()
    }

    fn codex_app_profile_and_candidates(
        &self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> (
        CodexAppPressureProfile,
        Vec<CodexAppLogicalCandidate>,
        CodexAppBackstopPlan,
    ) {
        let slope = |pid: u32| {
            tracked
                .iter()
                .find(|process| process.pid == pid)
                .map(|process| process.slope_mb_s.max(0.0))
                .unwrap_or_default()
        };
        let confirmed_pids: BTreeSet<_> = self
            .codex_app_snapshot
            .threads
            .iter()
            .flat_map(|thread| thread.confirmed_pids.iter().copied())
            .collect();
        let estimated_pids: BTreeSet<_> = self
            .codex_app_snapshot
            .threads
            .iter()
            .flat_map(|thread| thread.estimated_pids.iter().copied())
            .collect();
        let unattributed_pids: BTreeSet<_> = self
            .codex_app_snapshot
            .app_servers
            .iter()
            .flat_map(|server| server.unattributed_pids.iter().copied())
            .collect();
        let host_pids: BTreeSet<_> = self
            .codex_app_snapshot
            .app_servers
            .iter()
            .map(|server| server.pid)
            .collect();
        let confirmed_growth: f64 = confirmed_pids.iter().map(|pid| slope(*pid)).sum();
        let estimated_growth: f64 = estimated_pids.iter().map(|pid| slope(*pid)).sum();
        let blind_child_growth: f64 = unattributed_pids.iter().map(|pid| slope(*pid)).sum();
        let shared_host_growth: f64 = host_pids.iter().map(|pid| slope(*pid)).sum();
        let app_growth =
            confirmed_growth + estimated_growth + blind_child_growth + shared_host_growth;
        let blind_growth = estimated_growth + blind_child_growth + shared_host_growth;
        let blind_ratio = if app_growth >= 1.0 {
            (blind_growth / app_growth).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let total_positive: f64 = tracked
            .iter()
            .map(|process| process.slope_mb_s.max(0.0))
            .sum();
        let headroom_share = if assessment.headroom_fall_mb_s >= 1.0 {
            app_growth / assessment.headroom_fall_mb_s
        } else {
            0.0
        };
        let tracked_share = if total_positive >= 1.0 {
            app_growth / total_positive
        } else {
            0.0
        };
        let collision = !self.codex_app_snapshot.identity_collisions.is_empty();
        let causal = assessment.trajectory_confirmed
            && assessment.native_confidence != "low"
            && matches!(assessment.attribution.as_str(), "agent" | "mixed")
            && app_growth >= 4.0
            && headroom_share >= 0.25
            && tracked_share >= 0.40;

        let mut blind_keys = BTreeSet::new();
        for server in &self.codex_app_snapshot.app_servers {
            for keys in server.blind_candidate_keys.values() {
                blind_keys.extend(keys.iter().cloned());
            }
        }
        let collided_sessions: BTreeSet<_> = self
            .codex_app_snapshot
            .identity_collisions
            .keys()
            .filter_map(|key| self.ledger.codex_app.threads.get(key))
            .map(|thread| thread.session_id.clone())
            .collect();
        let mut candidates = Vec::new();
        for agent in self.ledger.logical_agents.values().filter(|agent| {
            agent.active
                && agent.surface == APP_SERVER_SURFACE
                && !collided_sessions.contains(&agent.session_id)
        }) {
            let owned = self.ledger.codex_app.owned_pids_for_logical(&agent.key);
            let growth: f64 = owned.iter().map(|pid| slope(*pid)).sum();
            let confirmed = owned.iter().any(|pid| confirmed_pids.contains(pid));
            let estimated = owned.iter().any(|pid| estimated_pids.contains(pid));
            let heavy_or_in_flight = agent.in_flight_tool_class.is_some()
                || matches!(
                    agent.last_tool_class,
                    Some(ToolClass::Expansion | ToolClass::HighMemoryStart)
                );
            let blind_possible = blind_keys.contains(&agent.key)
                || estimated
                || (blind_ratio > 0.0 && agent.in_flight_tool_class.is_some());
            let work_bearing = agent.state != LogicalState::Active
                || growth >= 1.0
                || agent.in_flight_tool_class.is_some()
                || agent.idle_since.is_none();
            if work_bearing {
                candidates.push(CodexAppLogicalCandidate {
                    key: agent.key.clone(),
                    role: agent.role.clone(),
                    state: agent.state,
                    state_since: agent.state_since,
                    growth_mb_s: growth,
                    confirmed,
                    blind_possible,
                    heavy_or_in_flight,
                    newest_at: agent.last_progress_at.max(agent.started_at),
                });
            }
        }
        let physical_enabled = causal && self.leak_action == "stop" && self.runtime_error.is_none();
        let minimum_span = (self.leak_window_s * 0.8).max(5.0);
        let candidate_keys: BTreeSet<_> = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect();
        let confirmed_target = physical_enabled
            .then(|| {
                confirmed_pids
                    .iter()
                    .filter_map(|pid| {
                        let owner = self.ledger.codex_app.control_safe_owner_for_pid(*pid)?;
                        let process = tracked.iter().find(|process| process.pid == *pid)?;
                        let keys = BTreeSet::from([owner.logical_key.clone()]);
                        (self.codex_app_hook_active_for(owner.app_server_pid)
                            && self.app_effective_backstop_growth_ready(
                                process,
                                app_growth,
                                minimum_span,
                            )
                            && self.app_backstop_keys_reachable(
                                &keys,
                                &candidate_keys,
                                now,
                                assessment.reaction_s,
                            ))
                        .then_some((process.slope_mb_s, keys))
                    })
                    .max_by(|left, right| left.0.total_cmp(&right.0))
            })
            .flatten();
        let (blind_to_server, related_by_pid) = self.codex_app_blind_target_scope();
        let blind_target = physical_enabled
            .then(|| {
                blind_to_server
                    .iter()
                    .filter_map(|(pid, server)| {
                        let keys = related_by_pid.get(pid)?.clone();
                        let process = tracked.iter().find(|process| process.pid == *pid)?;
                        (self.codex_app_hook_active_for(*server)
                            && self.app_effective_backstop_growth_ready(
                                process,
                                app_growth,
                                minimum_span,
                            )
                            && self.app_backstop_keys_reachable(
                                &keys,
                                &candidate_keys,
                                now,
                                assessment.reaction_s,
                            ))
                        .then_some((process.slope_mb_s, keys))
                    })
                    .max_by(|left, right| left.0.total_cmp(&right.0))
            })
            .flatten();
        let child_growth: f64 = tracked
            .iter()
            .filter(|process| !host_pids.contains(&process.pid))
            .filter(|process| {
                self.ledger.codex_app.owner_for_pid(process.pid).is_some()
                    || unattributed_pids.contains(&process.pid)
            })
            .map(|process| process.slope_mb_s.max(0.0))
            .sum();
        let host_target = physical_enabled
            .then(|| {
                host_pids
                    .iter()
                    .filter_map(|pid| {
                        let process = tracked.iter().find(|process| process.pid == *pid)?;
                        let keys = self.active_app_keys_for_server(*pid);
                        (self.codex_app_hook_active_for(*pid)
                            && self.app_host_growth_ready(
                                process,
                                app_growth,
                                child_growth,
                                minimum_span,
                            )
                            && self.app_backstop_keys_reachable(
                                &keys,
                                &candidate_keys,
                                now,
                                assessment.reaction_s,
                            ))
                        .then_some((process.slope_mb_s, keys))
                    })
                    .max_by(|left, right| left.0.total_cmp(&right.0))
            })
            .flatten();
        let backstop = if let Some((_, required_keys)) = confirmed_target {
            CodexAppBackstopPlan {
                kind: "confirmed-child".to_owned(),
                required_keys,
                blind: false,
            }
        } else if let Some((_, required_keys)) = blind_target {
            CodexAppBackstopPlan {
                kind: "blind-child".to_owned(),
                required_keys,
                blind: true,
            }
        } else if let Some((_, required_keys)) = host_target {
            CodexAppBackstopPlan {
                kind: "shared-host".to_owned(),
                required_keys,
                blind: true,
            }
        } else {
            CodexAppBackstopPlan::default()
        };
        if backstop.blind {
            for candidate in &mut candidates {
                if backstop.required_keys.contains(&candidate.key) {
                    candidate.blind_possible = true;
                }
            }
        }
        let mode = if collision {
            "IDENTITY_COLLISION"
        } else if causal && !self.codex_app_hooks_active_for_all_servers() {
            "DEGRADED_BLIND"
        } else if !causal {
            "OPEN"
        } else if blind_ratio >= 0.75 {
            "BLIND"
        } else if blind_ratio > 0.0 {
            "MIXED"
        } else {
            "CONFIRMED"
        };
        let profile = CodexAppPressureProfile {
            causal,
            mode: mode.to_owned(),
            app_growth_mb_s: rounded(app_growth, 2),
            confirmed_growth_mb_s: rounded(confirmed_growth, 2),
            estimated_growth_mb_s: rounded(estimated_growth, 2),
            blind_child_growth_mb_s: rounded(blind_child_growth, 2),
            shared_host_growth_mb_s: rounded(shared_host_growth, 2),
            blind_ratio: rounded(blind_ratio, 3),
            reserve_tte_s: assessment.time_to_recovery_reserve_s,
            backstop: if backstop.available() {
                backstop.kind.clone()
            } else {
                "none".to_owned()
            },
            reason: if causal {
                format!(
                    "App growth {:.2} MiB/s explains {:.0}% of current headroom fall; blind share {:.0}%",
                    app_growth,
                    headroom_share.min(1.0) * 100.0,
                    blind_ratio * 100.0
                )
            } else {
                "App-specific sustained causal pressure is not confirmed".to_owned()
            },
            ..CodexAppPressureProfile::default()
        };
        (profile, candidates, backstop)
    }

    fn set_codex_app_surface_gate(&mut self, enabled: bool, reason: &str, now: f64) -> bool {
        if self.ledger.codex_app.control.surface_gate == enabled || self.runtime_error.is_some() {
            return false;
        }
        let previous = self.ledger.codex_app.control.clone();
        self.ledger.codex_app.control.surface_gate = enabled;
        self.ledger.codex_app.control.surface_gate_since = enabled.then_some(now);
        self.ledger.codex_app.control.reason = reason.to_owned();
        self.ledger.codex_app.control.last_action_at = now;
        self.ledger.codex_app.control.mode = if enabled { "BLIND_GATE" } else { "OPEN" }.to_owned();
        if !self.persist_runtime(now) {
            self.ledger.codex_app.control = previous;
            return false;
        }
        let message = if enabled {
            format!(
                "[Memory Supervisor] CODEX APP CUSHION ENABLED\nWhy: {reason}.\nEffect: only new high-memory App tool starts are held; running work, results, messages, status and recovery remain available."
            )
        } else {
            "[Memory Supervisor] CODEX APP CUSHION RELEASED\nEffect: new high-memory App tool starts are available again after sustained recovery."
                .to_owned()
        };
        let audience_sessions: Vec<_> = self
            .ledger
            .codex_app
            .threads
            .values()
            .map(|thread| thread.session_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.emit_event(
            "codex-app-surface-gate",
            if enabled { "restricted" } else { "recovered" },
            &message,
            &format!("app-surface:{enabled}:{now:.3}"),
            Map::from_iter([
                (
                    "severity".to_owned(),
                    Value::String(if enabled { "warning" } else { "info" }.to_owned()),
                ),
                ("cause".to_owned(), Value::String(reason.to_owned())),
                (
                    "surface".to_owned(),
                    Value::String(APP_SERVER_SURFACE.to_owned()),
                ),
                (
                    "audience_provider".to_owned(),
                    Value::String("codex".to_owned()),
                ),
                ("audience_sessions".to_owned(), json!(audience_sessions)),
                (
                    "action".to_owned(),
                    Value::String(if enabled { "cushion" } else { "reopen" }.to_owned()),
                ),
            ]),
        );
        true
    }

    fn app_physical_pause_active(&self) -> bool {
        self.ledger.incidents.iter().any(|incident| {
            incident.get("status").and_then(Value::as_str) == Some("suspended")
                && incident_is_codex_app_physical(incident)
        })
    }

    fn manage_codex_app_containment(
        &mut self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        if !self.codex_app_snapshot.detected
            && !self
                .ledger
                .logical_agents
                .values()
                .any(|agent| agent.surface == APP_SERVER_SURFACE)
        {
            return false;
        }
        let (mut profile, candidates, backstop) =
            self.codex_app_profile_and_candidates(assessment, tracked, now);
        let backstop_receipt_budget_s =
            self.app_backstop_receipt_budget_s(&backstop, now, assessment.reaction_s);
        let plan: CodexAppControlPlan = plan_app_control(CodexAppPlanInput {
            now,
            tick_s: self.tick_s,
            reaction_s: assessment.reaction_s,
            reserve_tte_s: assessment.time_to_recovery_reserve_s,
            collapse_imminent: assessment.collapse_imminent,
            causal: profile.causal,
            app_growth_mb_s: profile.app_growth_mb_s,
            blind_ratio: profile.blind_ratio,
            has_physical_backstop: backstop.available(),
            backstop_required_keys: backstop.required_keys.iter().cloned().collect(),
            backstop_blind: backstop.blind,
            backstop_receipt_budget_s,
            surface_gate_active: self.ledger.codex_app.control.surface_gate,
            last_blind_target_at: self.ledger.codex_app.control.last_blind_target_at,
            candidates: candidates.clone(),
        });
        if backstop.available() && !plan.physical_backstop_reachable {
            profile.backstop = "none".to_owned();
        }
        profile.control_horizon_s = rounded(plan.horizon_s, 2);
        profile.remaining_steps = plan.remaining_steps;
        profile.due_steps_now = plan.due_steps_now;
        profile.selected_keys = plan.selected_keys.clone();
        let app_braking_due = profile.causal
            && (assessment.collapse_imminent
                || assessment
                    .time_to_recovery_reserve_s
                    .is_some_and(|tte| tte <= plan.horizon_s));
        // Native distress corroborates a real trajectory but does not pin an App cushion by
        // itself. Once the current causal trajectory is outside the App's calculated stopping
        // distance, preserving the restriction would recreate the early-throttling failure even
        // though the machine policy has returned to observation-only.
        let globally_safe =
            matches!(assessment.action, Action::Allow | Action::Observe) && !app_braking_due;
        let safe = globally_safe || codex_app_recovery_safe(assessment, &profile);
        if safe {
            if !self.ledger.codex_app.control.surface_gate
                && !self.ledger.logical_agents.values().any(|agent| {
                    agent.active
                        && agent.surface == APP_SERVER_SURFACE
                        && agent.state != LogicalState::Active
                })
            {
                self.ledger.codex_app.control.recovery_since = None;
                self.ledger.codex_app.control.mode = "OPEN".to_owned();
                self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
                self.codex_app_snapshot.pressure = profile;
                return false;
            }
            let since = *self
                .ledger
                .codex_app
                .control
                .recovery_since
                .get_or_insert(now);
            if self.app_physical_pause_active()
                || now - since < assessment.reaction_s * 2.0
                || now - self.ledger.codex_app.control.last_action_at < self.tick_s.max(0.25)
            {
                self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
                self.codex_app_snapshot.pressure = profile;
                return false;
            }
            let mut restricted: Vec<_> = self
                .ledger
                .logical_agents
                .values()
                .filter(|agent| {
                    agent.active
                        && agent.surface == APP_SERVER_SURFACE
                        && agent.state != LogicalState::Active
                })
                .collect();
            restricted.sort_by(|left, right| {
                (right.role == "lead")
                    .cmp(&(left.role == "lead"))
                    .then_with(|| right.state.cmp(&left.state))
                    .then_with(|| right.last_progress_at.total_cmp(&left.last_progress_at))
            });
            if let Some(agent) = restricted.first() {
                let key = agent.key.clone();
                let target = agent.state.relax();
                if self.change_logical_batch(
                    vec![(key, target)],
                    "sustained Codex App recovery",
                    false,
                    tracked,
                    now,
                ) {
                    self.ledger.codex_app.control.last_action_at = now;
                    self.ledger.codex_app.control.mode = "RECOVERING".to_owned();
                    let _ = self.persist_runtime(now);
                    self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
                    self.codex_app_snapshot.pressure = profile;
                    return true;
                }
            }
            let changed =
                self.set_codex_app_surface_gate(false, "sustained Codex App recovery", now);
            self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
            self.codex_app_snapshot.pressure = profile;
            return changed;
        }
        self.ledger.codex_app.control.recovery_since = None;
        self.codex_app_snapshot.pressure = profile.clone();
        self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
        if plan.surface_gate
            && !self.ledger.codex_app.control.surface_gate
            && self.set_codex_app_surface_gate(true, &profile.reason, now)
        {
            self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
            return true;
        }
        if plan.targets.is_empty()
            || now - self.ledger.codex_app.control.last_action_at < self.tick_s.max(0.25)
        {
            return false;
        }
        let starts_blind = plan.targets.iter().any(|(key, _)| {
            candidates.iter().any(|candidate| {
                candidate.key == *key
                    && candidate.blind_possible
                    && candidate.state == LogicalState::Active
            })
        });
        let reason = format!(
            "Codex App adaptive stopping distance: {} step(s) are due now; {}",
            plan.due_steps_now, profile.reason
        );
        if self.change_logical_batch(plan.targets, &reason, true, tracked, now) {
            self.ledger.codex_app.control.last_action_at = now;
            self.ledger.codex_app.control.mode = profile.mode;
            self.ledger.codex_app.control.reason = reason;
            if starts_blind {
                self.ledger.codex_app.control.last_blind_target_at = now;
            }
            let _ = self.persist_runtime(now);
            self.codex_app_snapshot.control = self.ledger.codex_app.control.clone();
            return true;
        }
        false
    }

    fn total_lead_growth(&self, tracked: &[TrackedProcess]) -> f64 {
        tracked
            .iter()
            .filter(|process| process.role == "lead" && !self.codex_app_shared_host(process.pid))
            .map(|process| process.slope_mb_s.max(0.0))
            .sum()
    }

    fn logical_steps_remaining(state: LogicalState) -> u32 {
        match state {
            LogicalState::Active => 3,
            LogicalState::NoExpansion => 2,
            LogicalState::LightWorkOnly => 1,
            LogicalState::HandoffOnly => 0,
        }
    }

    fn direct_lead_override(
        &self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
    ) -> Option<String> {
        let subordinate_steps: u32 = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active && agent.role == "subagent" && agent.surface != APP_SERVER_SURFACE
            })
            .map(|agent| Self::logical_steps_remaining(agent.state))
            .sum();
        let drain_horizon = assessment.reaction_s * subordinate_steps.max(2) as f64;
        let total_lead_growth = self.total_lead_growth(tracked);
        self.ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active && agent.role == "lead" && agent.surface != APP_SERVER_SURFACE
            })
            .filter_map(|agent| {
                let process = self
                    .logical_processes(agent, tracked)
                    .into_iter()
                    .filter(|process| process.runaway_verified)
                    .min_by(|left, right| {
                        let tte = |process: &&TrackedProcess| {
                            process
                                .runaway
                                .as_ref()
                                .and_then(|verdict| verdict.candidate_tte_s)
                                .unwrap_or(f64::INFINITY)
                        };
                        tte(left).total_cmp(&tte(right))
                    })?;
                let verdict = process.runaway.as_ref()?;
                let tte = verdict.candidate_tte_s?;
                (process.runaway_verified
                    && process.slope_mb_s >= total_lead_growth * 0.5
                    && tte <= drain_horizon
                    && self.direct_process_risk(process, assessment))
                .then_some((agent.key.clone(), tte, agent.started_at))
            })
            .min_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| right.2.total_cmp(&left.2))
            })
            .map(|value| value.0)
    }

    fn all_subordinates_exhausted(&self) -> bool {
        self.ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active && agent.role == "subagent" && agent.surface != APP_SERVER_SURFACE
            })
            .all(|agent| agent.state == LogicalState::HandoffOnly)
    }

    fn logical_batch_budget(&self, role: &str, assessment: &Assessment) -> usize {
        let remaining: usize = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active && agent.role == role && agent.surface != APP_SERVER_SURFACE
            })
            .map(|agent| Self::logical_steps_remaining(agent.state) as usize)
            .sum();
        if remaining == 0 {
            return 0;
        }
        let time_left = assessment
            .time_to_recovery_reserve_s
            .unwrap_or(assessment.reaction_s)
            .max(self.tick_s);
        let ticks_left = (time_left / self.tick_s.max(0.25)).floor().max(1.0) as usize;
        remaining.div_ceil(ticks_left)
    }

    fn logical_batch_targets(
        &self,
        role: &str,
        mut step_budget: usize,
        tracked: &[TrackedProcess],
    ) -> Vec<(String, LogicalState)> {
        let mut candidates: Vec<_> = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active
                    && agent.role == role
                    && agent.surface != APP_SERVER_SURFACE
                    && agent.state != LogicalState::HandoffOnly
            })
            .collect();
        candidates.sort_by(|left, right| {
            let risk = |agent: &LogicalAgent| {
                let processes = self.logical_processes(agent, tracked);
                let verified = processes.iter().any(|process| process.runaway_verified);
                let heavy = matches!(
                    agent.in_flight_tool_class.or(agent.last_tool_class),
                    Some(ToolClass::Expansion | ToolClass::HighMemoryStart)
                );
                (verified, heavy, agent.state)
            };
            risk(left)
                .cmp(&risk(right))
                .then_with(|| {
                    let tte = |agent: &LogicalAgent| {
                        self.logical_processes(agent, tracked)
                            .into_iter()
                            .filter_map(|process| process.runaway.as_ref())
                            .filter_map(|verdict| verdict.candidate_tte_s)
                            .min_by(f64::total_cmp)
                            .unwrap_or(f64::INFINITY)
                    };
                    tte(right).total_cmp(&tte(left))
                })
                .then_with(|| newest_first(left, right))
        });

        let mut targets = Vec::new();
        while step_budget > 0 {
            let Some(agent) = candidates.pop() else {
                break;
            };
            let available = Self::logical_steps_remaining(agent.state) as usize;
            let steps = available.min(step_budget);
            let mut target = agent.state;
            for _ in 0..steps {
                target = target.tighten();
            }
            targets.push((agent.key.clone(), target));
            step_budget -= steps;
        }
        targets
    }

    fn change_logical_batch(
        &mut self,
        targets: Vec<(String, LogicalState)>,
        reason: &str,
        tighten: bool,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(key, target)| {
                let agent = self.ledger.logical_agents.get(&key)?;
                let moves = if tighten {
                    target > agent.state
                } else {
                    target < agent.state
                };
                moves.then_some((
                    key,
                    agent.state,
                    target,
                    agent.process_pid,
                    agent.role.clone(),
                    agent.provider.clone(),
                ))
            })
            .collect();
        if changes.is_empty() || self.runtime_error.is_some() {
            return false;
        }

        let previous = self.ledger.clone();
        let epoch = self.bump_logical_epoch();
        let applied_steps: u32 = changes
            .iter()
            .map(|(_, from, to, _, _, _)| {
                Self::logical_steps_remaining(*from).abs_diff(Self::logical_steps_remaining(*to))
            })
            .sum();
        for (key, _, target, _, _, _) in &changes {
            let Some(agent) = self.ledger.logical_agents.get_mut(key) else {
                self.ledger = previous;
                return false;
            };
            agent.state = *target;
            agent.epoch = epoch;
            agent.reason = reason.to_owned();
            agent.evidence_stage = if tighten {
                "LOGICAL_ACTION_ELIGIBLE"
            } else {
                "RECOVERY_CONFIRMED"
            }
            .to_owned();
            agent.state_since = now;
            agent.next_check = now + self.tick_s;
        }
        // Coalesce oscillation to material edges: notify only when this batch
        // crosses the restricted boundary — the onset of a restricted regime, or
        // the return to a fully recovered one.  A "live restricted agent" is
        // active and not ACTIVE, so idle or ended roster entries never pin the
        // regime; the persisted roster carries the regime across restarts, so no
        // separate flag can desync.  Steps inside an already-restricted regime,
        // where the boundary does not move, stay detail-only.
        let was_restricted = previous
            .logical_agents
            .values()
            .any(|agent| agent.active && agent.state != LogicalState::Active);
        let now_restricted = self
            .ledger
            .logical_agents
            .values()
            .any(|agent| agent.active && agent.state != LogicalState::Active);
        let material_edge = was_restricted != now_restricted;
        self.ledger.last_logical_action_at = now;
        if !self.persist_runtime(now) {
            self.ledger = previous;
            return false;
        }

        let target_summary = changes
            .iter()
            .map(|(key, from, to, _, role, provider)| {
                format!("{provider} {role} {key}: {from:?}->{to:?}")
            })
            .collect::<Vec<_>>();
        let message = if tighten {
            format!(
                "[Memory Supervisor] ADAPTIVE CUSHION BATCH APPLIED\nEpoch: {epoch}\nTargets: {} logical agents, {applied_steps} minimum steps\nWhy: {reason}.\nEffect: only the named future-work classes are restricted; running work and result, message, status, stop, and recovery paths stay open.\nCoordination: every lead receives epoch {epoch} and the exact current roster at its next hook boundary.",
                changes.len()
            )
        } else {
            format!(
                "[Memory Supervisor] ADAPTIVE CUSHION BATCH RELAXED\nEpoch: {epoch}\nTargets: {} logical agents, {applied_steps} steps\nWhy: {reason}.\nEffect: the named future-work classes reopen on the recovery schedule while safety holds; remaining restrictions lift the same way.\nCoordination: every lead receives epoch {epoch} and the exact current roster at its next hook boundary.",
                changes.len()
            )
        };
        // On a material edge only, write one coalesced line to each affected
        // terminal so the human sees a restriction start or a recovery in real
        // time — even while the lead is idle-waiting on a fan-out. Deduplicated
        // per terminal and gated to the edge, so the per-batch raw writes that
        // corrupted the TUI cannot return.
        if material_edge {
            let mut delivered = BTreeSet::new();
            for pid in changes.iter().filter_map(|(_, _, _, pid, _, _)| *pid) {
                let Some(process) = tracked.iter().find(|process| process.pid == pid) else {
                    continue;
                };
                if process.terminal.is_empty() || !delivered.insert(process.terminal.clone()) {
                    continue;
                }
                terminal::write(
                    &self.platform,
                    process.pid,
                    &process.terminal,
                    &message,
                    &process.terminal_identity,
                );
            }
        }
        self.emit_event(
            "logical-containment",
            if tighten { "restricted" } else { "relaxed" },
            &message,
            &format!("batch:{epoch}"),
            Map::from_iter([
                (
                    "severity".to_owned(),
                    Value::String(if tighten { "warning" } else { "info" }.to_owned()),
                ),
                ("cause".to_owned(), Value::String(reason.to_owned())),
                ("logical_epoch".to_owned(), json!(epoch)),
                ("logical_steps".to_owned(), json!(applied_steps)),
                ("targets".to_owned(), json!(target_summary)),
                (
                    "action".to_owned(),
                    Value::String(if tighten { "cushion" } else { "reopen" }.to_owned()),
                ),
            ]),
        );
        true
    }

    fn manage_cli_logical_containment(
        &mut self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        let safe = matches!(assessment.action, Action::Allow | Action::Observe)
            && assessment.distress == "normal"
            && assessment.cli_hard_cap_status != "exceeded";
        if safe {
            let Some(since) = self.logical_recovery_since else {
                self.logical_recovery_since = Some(now);
                return false;
            };
            if now - since < assessment.reaction_s * 2.0
                || now - self.ledger.last_logical_action_at < self.tick_s.max(0.25)
            {
                return false;
            }
            // Reopening mirrors the tightening schedule: the same minimum-batch formula against
            // a bounded recovery deadline. The serial one-step-per-reaction ladder returned
            // capability minutes after the measurements already allowed it.
            let mut candidates: Vec<_> = self
                .ledger
                .logical_agents
                .values()
                .filter(|agent| {
                    agent.active
                        && agent.surface != APP_SERVER_SURFACE
                        && agent.state != LogicalState::Active
                })
                .collect();
            if candidates.is_empty() {
                return false;
            }
            candidates.sort_by(|left, right| {
                (right.role == "lead")
                    .cmp(&(left.role == "lead"))
                    .then_with(|| right.state.cmp(&left.state))
                    .then_with(|| newest_first(right, left))
            });
            let remaining: usize = candidates
                .iter()
                .map(|agent| (3 - Self::logical_steps_remaining(agent.state)) as usize)
                .sum();
            let relax_deadline = since + assessment.reaction_s * 4.0;
            let ticks_left = ((relax_deadline - now) / self.tick_s.max(0.25))
                .floor()
                .max(1.0) as usize;
            let budget = remaining.div_ceil(ticks_left).min(candidates.len());
            let targets: Vec<_> = candidates
                .into_iter()
                .take(budget)
                .map(|agent| (agent.key.clone(), agent.state.relax()))
                .collect();
            return self.change_logical_batch(
                targets,
                "sustained adaptive recovery",
                false,
                tracked,
                now,
            );
        }
        self.logical_recovery_since = None;
        let cap_exceeded = assessment.cli_hard_cap_status == "exceeded";
        let attributable = matches!(assessment.attribution.as_str(), "agent" | "mixed")
            && assessment.native_confidence != "low";
        if assessment.action != Action::Drain && !cap_exceeded {
            return false;
        }
        if !cap_exceeded && !attributable {
            return false;
        }
        if now - self.ledger.last_logical_action_at < self.tick_s.max(0.25) {
            return false;
        }
        if let Some(key) = self.direct_lead_override(assessment, tracked) {
            let target = self
                .ledger
                .logical_agents
                .get(&key)
                .map(|agent| agent.state.tighten())
                .unwrap_or(LogicalState::Active);
            return self.change_logical_batch(
                vec![(key, target)],
                "lead_dominant_override: verified idle/finished-work growth reaches the recovery boundary before subordinate cushioning can help",
                true,
                tracked,
                now,
            );
        }
        let subagent_budget = self.logical_batch_budget("subagent", assessment);
        if subagent_budget > 0 {
            let targets = self.logical_batch_targets("subagent", subagent_budget, tracked);
            let reason = format!(
                "adaptive stopping distance: apply {subagent_budget} subordinate steps this tick so the remaining ladder fits before the recovery reserve; verified abnormality, active heavy work, and newest age determine order{}",
                if cap_exceeded {
                    "; explicit CLI hard cap is the driving boundary"
                } else {
                    ""
                }
            );
            return self.change_logical_batch(targets, &reason, true, tracked, now);
        }
        if !self.all_subordinates_exhausted() {
            return false;
        }
        let near_final_boundary = assessment.collapse_imminent
            || assessment
                .time_to_exhaustion_s
                .is_some_and(|tte| tte <= assessment.reaction_s * 6.0)
            || cap_exceeded;
        if !near_final_boundary {
            return false;
        }
        let lead_budget = self.logical_batch_budget("lead", assessment);
        let targets = self.logical_batch_targets("lead", lead_budget, tracked);
        let reason = format!(
            "all subordinates are exhausted and boundary danger remains: apply {lead_budget} lead steps this tick so the last-resort ladder fits before the recovery reserve; verified abnormality and newest age determine order"
        );
        self.change_logical_batch(targets, &reason, true, tracked, now)
    }

    #[cfg(test)]
    fn manage_logical_containment(
        &mut self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        self.manage_cli_logical_containment(assessment, tracked, now)
    }

    fn logical_process_exhausted(&self, pid: u32, now: f64, reaction_s: f64) -> bool {
        let agents = self.logical_agents_for_pid(pid, true);
        if let Some(owner) = self.ledger.codex_app.control_safe_owner_for_pid(pid) {
            return self.codex_app_hook_active_for(owner.app_server_pid)
                && !agents.is_empty()
                && agents.iter().all(|agent| {
                    agent.surface == APP_SERVER_SURFACE
                        && self.app_agent_handoff_observed(agent, now, reaction_s)
                });
        }
        agents.is_empty()
            || agents.iter().all(|agent| {
                agent.state == LogicalState::HandoffOnly && now - agent.state_since >= reaction_s
            })
    }

    fn lead_pause_authorized(
        &self,
        candidate: &TrackedProcess,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
    ) -> bool {
        !self.codex_app_physical_control_forbidden(candidate.pid)
            && (self.all_subordinates_exhausted()
                || self
                    .direct_lead_override(assessment, tracked)
                    .and_then(|key| self.ledger.logical_agents.get(&key))
                    .is_some_and(|agent| agent.process_pid == Some(candidate.pid)))
    }

    fn record_suspension(
        &mut self,
        candidate: &TrackedProcess,
        assessment: &Assessment,
        now: f64,
        reason: &str,
        probe: &terminal::Delivery,
    ) -> Value {
        let app_confirmed_session = self
            .ledger
            .codex_app
            .control_safe_owner_for_pid(candidate.pid)
            .and_then(|owner| self.ledger.codex_app.threads.get(&owner.thread_key))
            .map(|thread| thread.session_id.clone());
        let recovery = if app_confirmed_session.is_some() {
            "automatic-pressure-recovery"
        } else if reason == "runaway-memory" && candidate.role == "lead" {
            "lead-probation"
        } else if matches!(
            reason,
            "pressure-pause"
                | "pressure-lead-last-resort"
                | "hard-cap-pause"
                | "hard-cap-lead-last-resort"
                | "app-blind-child-last-resort"
                | "app-shared-host-last-resort"
        ) {
            "automatic-pressure-recovery"
        } else {
            "lead-or-owner"
        };
        let mut incident = json!({
            "id": format!("{}-{}-{}", self.instance, candidate.pid, unique_nonce()),
            "status": "suspended",
            "source": self.instance,
            "platform": self.platform,
            "pid": candidate.pid,
            "identity": candidate.identity,
            "start_token": candidate.start_token,
            "name": candidate.name,
            "via": candidate.via,
            "role": candidate.role,
            "reason": reason,
            "anon_mb": candidate.anon_mb,
            "slope_mb_s": candidate.slope_mb_s,
            "observation_window_s": if reason == "runaway-memory" { Some(self.leak_window_s) } else { None },
            "process_time_to_reserve_s": if reason == "runaway-memory" && candidate.slope_mb_s >= 1.0 {
                Some(rounded(
                    (assessment.mem_available_mb as f64 - assessment.automatic_reserve_mb).max(0.0)
                        / candidate.slope_mb_s,
                    1,
                ))
            } else {
                None
            },
            "suspended_at": rounded(now, 3),
            "updated_at": rounded(now, 3),
            "terminal": candidate.terminal,
            "terminal_identity": probe.identity,
            "terminal_probe": probe.status,
            "terminal_probe_reason": probe.reason,
            "terminal_notice": if candidate.role == "lead" { "pending" } else { "skipped" },
            "recovery_policy": recovery,
            "distress": assessment.distress,
            "attribution": assessment.attribution,
            "time_to_exhaustion_s": assessment.time_to_exhaustion_s,
            "time_to_recovery_reserve_s": assessment.time_to_recovery_reserve_s,
            "automatic_reserve_mb": assessment.automatic_reserve_mb,
            "cli_hard_cap_mb": assessment.cli_hard_cap_mb,
            "cli_memory_used_mb": assessment.cli_memory_used_mb,
            "cli_hard_cap_remaining_mb": assessment.cli_hard_cap_remaining_mb,
            "cli_hard_cap_status": assessment.cli_hard_cap_status,
        });
        if let Some(session_id) = app_confirmed_session
            && let Some(object) = incident.as_object_mut()
        {
            object.insert(
                "app_control_scope".to_owned(),
                Value::String("thread-confirmed-child".to_owned()),
            );
            object.insert(
                "thread_attribution".to_owned(),
                Value::String("confirmed".to_owned()),
            );
            object.insert(
                "claimed_thread".to_owned(),
                Value::String(session_id.clone()),
            );
            object.insert(
                "audience_provider".to_owned(),
                Value::String("codex".to_owned()),
            );
            object.insert("audience_sessions".to_owned(), json!([session_id]));
        }
        self.ledger.incidents.push(incident.clone());
        incident
    }

    fn record_terminal_delivery(&mut self, incident_id: &str, delivery: &terminal::Delivery) {
        if let Some(incident) = self
            .ledger
            .incidents
            .iter_mut()
            .rev()
            .find(|incident| incident.get("id").and_then(Value::as_str) == Some(incident_id))
            .and_then(Value::as_object_mut)
        {
            incident.insert(
                "last_terminal_notice".to_owned(),
                Value::String(delivery.status.clone()),
            );
            incident.insert(
                "last_terminal_notice_reason".to_owned(),
                Value::String(delivery.reason.clone()),
            );
        }
    }

    fn exact_process(&self, candidate: &TrackedProcess) -> Option<ProcessInfo> {
        let current = process_by_pid(&self.platform, candidate.pid)?;
        (!current.start_token.is_empty() && process_identity(&current) == candidate.identity)
            .then_some(current)
    }

    fn suspend_candidate(
        &mut self,
        candidate: &TrackedProcess,
        assessment: &Assessment,
        now: f64,
        reason: &str,
    ) -> bool {
        if self.runtime_error.is_some() || self.codex_app_physical_control_forbidden(candidate.pid)
        {
            return false;
        }
        let Some(current) = self.exact_process(candidate) else {
            return false;
        };
        let probe = terminal::probe(
            &self.platform,
            candidate.pid,
            &current.terminal,
            &current.terminal_identity,
        );
        if candidate.role == "lead" && probe.status != "delivered" {
            return false;
        }
        if let Err(error) = suspend_process(&self.platform, candidate.pid) {
            log_event(
                &self.directory,
                &format!("SUSPEND_ERROR pid={} error={error}", candidate.pid),
            );
            return false;
        }
        self.ledger
            .mark_stopped(candidate.pid, candidate.identity.clone());
        let incident = self.record_suspension(candidate, assessment, now, reason, &probe);
        if !self.persist_runtime(now) {
            if resume_process(&self.platform, candidate.pid).is_err() {
                self.runtime_error =
                    Some("suspension state could not be persisted or rolled back".to_owned());
                return false;
            }
            self.ledger.clear_stopped(candidate.pid);
            self.ledger.transition_incident(
                &candidate.identity,
                "resumed",
                now,
                "persistence-rollback",
                Map::new(),
            );
            return false;
        }
        let message = incident_notice(&incident, "suspended", "");
        let delivery = terminal::write(
            &self.platform,
            candidate.pid,
            &current.terminal,
            &message,
            if probe.identity.is_empty() {
                &current.terminal_identity
            } else {
                &probe.identity
            },
        );
        if candidate.role == "lead" && delivery.status != "delivered" {
            let _ = resume_process(&self.platform, candidate.pid);
            self.ledger.clear_stopped(candidate.pid);
            self.ledger.transition_incident(
                &candidate.identity,
                "resumed",
                now,
                "terminal-visibility-rollback",
                Map::from_iter([(
                    "recovery_visibility".to_owned(),
                    Value::String("unavailable".to_owned()),
                )]),
            );
            let _ = self.persist_runtime(now);
            return false;
        }
        if let Some(stored) = self
            .ledger
            .incidents
            .iter_mut()
            .rev()
            .find(|value| value.get("id") == incident.get("id"))
            .and_then(Value::as_object_mut)
        {
            stored.insert(
                "terminal_notice".to_owned(),
                Value::String(delivery.status.clone()),
            );
            stored.insert(
                "terminal_notice_reason".to_owned(),
                Value::String(delivery.reason.clone()),
            );
        }
        let _ = self.persist_runtime(now);
        let mut event_fields = Map::from_iter([
            ("severity".to_owned(), Value::String("critical".to_owned())),
            ("pid".to_owned(), json!(candidate.pid)),
            ("role".to_owned(), Value::String(candidate.role.clone())),
            ("cause".to_owned(), Value::String(reason.to_owned())),
            ("action".to_owned(), Value::String("paused".to_owned())),
            ("terminal".to_owned(), Value::String(delivery.status)),
            (
                "attribution".to_owned(),
                Value::String(assessment.attribution.clone()),
            ),
            (
                "distress".to_owned(),
                Value::String(assessment.distress.clone()),
            ),
        ]);
        extend_incident_audience(&mut event_fields, &incident);
        self.emit_event(
            "process-pause",
            "suspended",
            &message,
            incident
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            event_fields,
        );
        true
    }

    fn spawn_codex_app_resume_guard(
        &self,
        pid: u32,
        identity: &str,
        incident_id: &str,
        delay_s: f64,
        app_server_pid: u32,
        control_base: &Path,
    ) -> Result<(), String> {
        let binary = env::current_exe().map_err(|error| error.to_string())?;
        let control_directory = control_base
            .parent()
            .ok_or_else(|| "guard control directory is missing".to_owned())?;
        ensure_private_dir(control_directory).map_err(|error| error.to_string())?;
        let arguments = guard_argument_values(&AppGuardInvocation {
            pid,
            identity,
            incident_id,
            delay_s,
            runtime_path: &self.runtime_path,
            platform: &self.platform,
            app_server_pid,
            control_base,
        });
        crate::app_guard::launch_independent(&binary, &arguments, &self.platform, pid)
            .map_err(|error| error.to_string())
    }

    fn wait_codex_app_guard(
        &self,
        control_base: &Path,
        phases: &[&str],
        timeout: Duration,
    ) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(phase) = phases
                .iter()
                .find(|phase| crate::app_guard::phase_path(control_base, phase).is_file())
            {
                return Some((*phase).to_owned());
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn cancel_codex_app_pause(
        &mut self,
        previous: RuntimeLedger,
        candidate: &TrackedProcess,
        affected_sessions: &[String],
        incident_id: &str,
        now: f64,
        reason: (&str, &str),
    ) {
        let (cause, explanation) = reason;
        self.ledger = previous;
        if !self.persist_runtime(now) {
            self.runtime_error = Some(
                "Codex App pause was cancelled but its prepared runtime state could not be rolled back"
                    .to_owned(),
            );
        }
        let message = format!(
            "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE CANCELLED\n{explanation}\nEffect: no process remains paused by this attempt."
        );
        self.emit_event(
            "codex-app-process-pause",
            "cancelled",
            &message,
            &format!("app-pause-cancelled:{incident_id}"),
            Map::from_iter([
                ("severity".to_owned(), Value::String("warning".to_owned())),
                ("pid".to_owned(), json!(candidate.pid)),
                ("cause".to_owned(), Value::String(cause.to_owned())),
                (
                    "surface".to_owned(),
                    Value::String(APP_SERVER_SURFACE.to_owned()),
                ),
                (
                    "audience_provider".to_owned(),
                    Value::String("codex".to_owned()),
                ),
                ("audience_sessions".to_owned(), json!(affected_sessions)),
                ("action".to_owned(), Value::String("cancelled".to_owned())),
            ]),
        );
    }

    fn suspend_codex_app_last_resort(
        &mut self,
        candidate: &TrackedProcess,
        assessment: &Assessment,
        now: f64,
        reason: &str,
        scope: CodexAppPauseScope<'_>,
    ) -> bool {
        let CodexAppPauseScope {
            affected_sessions,
            app_server_pid,
            shared_host,
        } = scope;
        if self.runtime_error.is_some()
            || self.ledger.codex_app.control.pending_physical.is_some()
            || self.ledger.stopped_identity(candidate.pid).is_some()
            || self
                .ledger
                .resume_cooldown
                .contains_key(&candidate.identity)
            || candidate.pid == std::process::id()
        {
            return false;
        }
        let Some(current) = self.exact_process(candidate) else {
            return false;
        };
        let guard_delay_s = (assessment.reaction_s * 2.0).clamp(10.0, 60.0);
        let guard_deadline = shared_host.then_some(now + guard_delay_s);
        let guard_control_id = if shared_host {
            format!("{}-{}", candidate.pid, unique_nonce())
        } else {
            String::new()
        };
        let guard_control_base =
            shared_host.then(|| self.directory.join("app-guards").join(&guard_control_id));
        let previous = self.ledger.clone();
        self.ledger
            .mark_stopped(candidate.pid, candidate.identity.clone());
        let probe = terminal::Delivery {
            status: "unavailable".to_owned(),
            identity: String::new(),
            reason: "Codex App incidents are delivered through hooks and the event ledger"
                .to_owned(),
        };
        let incident = self.record_suspension(candidate, assessment, now, reason, &probe);
        if let Some(stored) = self
            .ledger
            .incidents
            .iter_mut()
            .rev()
            .find(|value| value.get("id") == incident.get("id"))
            .and_then(Value::as_object_mut)
        {
            stored.insert(
                "control_phase".to_owned(),
                Value::String("prepared".to_owned()),
            );
            stored.insert(
                "app_control_scope".to_owned(),
                Value::String(
                    if shared_host {
                        "shared-host"
                    } else {
                        "blind-child"
                    }
                    .to_owned(),
                ),
            );
            stored.insert(
                "thread_attribution".to_owned(),
                Value::String("blind".to_owned()),
            );
            stored.insert("claimed_thread".to_owned(), Value::Null);
            stored.insert("affected_sessions".to_owned(), json!(affected_sessions));
            stored.insert(
                "audience_provider".to_owned(),
                Value::String("codex".to_owned()),
            );
            stored.insert("audience_sessions".to_owned(), json!(affected_sessions));
            stored.insert("guard_deadline".to_owned(), json!(guard_deadline));
            if !guard_control_id.is_empty() {
                stored.insert(
                    "guard_control_id".to_owned(),
                    Value::String(guard_control_id.clone()),
                );
            }
        }
        self.ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: candidate.pid,
                identity: candidate.identity.clone(),
                scope: if shared_host {
                    "shared-host"
                } else {
                    "blind-child"
                }
                .to_owned(),
                prepared_at: now,
                guard_deadline,
                guard_control_id: guard_control_id.clone(),
            });
        // Persist the exact target and recovery route before a signal can stop hook delivery.
        if !self.persist_runtime(now) {
            self.ledger = previous;
            return false;
        }
        let incident_id = incident
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let live_processes = list_processes(&self.platform);
        let identity_still_exact = live_processes
            .get(&candidate.pid)
            .is_some_and(|process| process_identity(process) == candidate.identity);
        let scope_still_exact = if shared_host {
            candidate.pid == app_server_pid
                && live_processes
                    .get(&app_server_pid)
                    .is_some_and(crate::codex_app::is_codex_app_server)
        } else {
            live_processes
                .get(&app_server_pid)
                .is_some_and(crate::codex_app::is_codex_app_server)
                && process_descends_from(candidate.pid, app_server_pid, &live_processes)
        };
        if !identity_still_exact
            || !scope_still_exact
            || process_identity(&current) != candidate.identity
        {
            self.cancel_codex_app_pause(
                previous,
                candidate,
                affected_sessions,
                incident_id,
                now,
                (
                    "target-changed-before-arming",
                    "The exact App process identity or its App Server relationship changed before the brake was armed.",
                ),
            );
            return false;
        }
        let prepared_message = format!(
            "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE PREPARED\nTarget PID: {}.\nScope: {}.\nEffect: the exact process is about to be paused reversibly after all smaller controls completed; recovery state is already durable.",
            candidate.pid,
            if shared_host {
                "all threads in the shared App Server"
            } else {
                "one App child with unknown thread ownership"
            }
        );
        let prepared_event = self.notification_event(
            "codex-app-process-pause",
            "prepared",
            &prepared_message,
            &format!("app-pause-prepared:{incident_id}"),
            Map::from_iter([
                ("severity".to_owned(), Value::String("critical".to_owned())),
                ("pid".to_owned(), json!(candidate.pid)),
                ("cause".to_owned(), Value::String(reason.to_owned())),
                (
                    "surface".to_owned(),
                    Value::String(APP_SERVER_SURFACE.to_owned()),
                ),
                (
                    "audience_provider".to_owned(),
                    Value::String("codex".to_owned()),
                ),
                ("audience_sessions".to_owned(), json!(affected_sessions)),
                (
                    "action".to_owned(),
                    Value::String("pause-prepared".to_owned()),
                ),
            ]),
        );
        if !self.queue_notification_event(&prepared_event) && shared_host {
            self.cancel_codex_app_pause(
                previous,
                candidate,
                affected_sessions,
                incident_id,
                now,
                (
                    "prepared-notice-unavailable",
                    "The durable App notice could not be queued before a shared App Server brake.",
                ),
            );
            return false;
        }
        if shared_host {
            let Some(control_base) = guard_control_base.as_ref() else {
                self.cancel_codex_app_pause(
                    previous,
                    candidate,
                    affected_sessions,
                    incident_id,
                    now,
                    (
                        "resume-guard-path-unavailable",
                        "The independent recovery controller did not have a valid control path.",
                    ),
                );
                return false;
            };
            if let Err(error) = self.spawn_codex_app_resume_guard(
                candidate.pid,
                &candidate.identity,
                incident_id,
                guard_delay_s,
                app_server_pid,
                control_base,
            ) {
                log_event(
                    &self.directory,
                    &format!("APP_GUARD_START_ERROR pid={} error={error}", candidate.pid),
                );
                self.cancel_codex_app_pause(
                    previous,
                    candidate,
                    affected_sessions,
                    incident_id,
                    now,
                    (
                        "resume-guard-unavailable",
                        "The independent recovery controller could not be started outside the daemon's lifetime.",
                    ),
                );
                return false;
            }
            let phase = self.wait_codex_app_guard(
                control_base,
                &["armed", "error", "cancelled", "expired"],
                Duration::from_secs(10),
            );
            if phase.as_deref() != Some("armed") {
                self.cancel_codex_app_pause(
                    previous,
                    candidate,
                    affected_sessions,
                    incident_id,
                    now,
                    (
                        "resume-guard-not-armed",
                        "The independent recovery controller did not prove that it was ready; the pause was not authorized.",
                    ),
                );
                return false;
            }
        }
        // Identity and scope are deliberately checked again after the guard is armed and
        // immediately before the controller receives authority to signal the process.
        let final_scope_exact = if shared_host {
            process_by_pid(&self.platform, candidate.pid).is_some_and(|process| {
                process_identity(&process) == candidate.identity
                    && candidate.pid == app_server_pid
                    && crate::codex_app::is_codex_app_server(&process)
            })
        } else {
            let final_processes = list_processes(&self.platform);
            final_processes
                .get(&candidate.pid)
                .is_some_and(|process| process_identity(process) == candidate.identity)
                && final_processes
                    .get(&app_server_pid)
                    .is_some_and(crate::codex_app::is_codex_app_server)
                && process_descends_from(candidate.pid, app_server_pid, &final_processes)
        };
        if !final_scope_exact {
            self.cancel_codex_app_pause(
                previous,
                candidate,
                affected_sessions,
                incident_id,
                now,
                (
                    "target-changed-before-commit",
                    "The exact App process identity or its App Server relationship changed during the final safety check.",
                ),
            );
            return false;
        }
        if shared_host {
            let control_base = guard_control_base.as_ref().unwrap();
            if fs::rename(
                crate::app_guard::phase_path(control_base, "armed"),
                crate::app_guard::phase_path(control_base, "committed"),
            )
            .is_err()
            {
                self.cancel_codex_app_pause(
                    previous,
                    candidate,
                    affected_sessions,
                    incident_id,
                    now,
                    (
                        "resume-guard-commit-failed",
                        "The recovery controller's ready record could not be committed atomically, so it never received permission to pause the App Server.",
                    ),
                );
                return false;
            }
            let phase = self.wait_codex_app_guard(
                control_base,
                &["suspended", "error", "cancelled", "expired"],
                Duration::from_secs(5),
            );
            match phase.as_deref() {
                Some("suspended") => {}
                Some("error" | "cancelled" | "expired") => {
                    self.cancel_codex_app_pause(
                        previous,
                        candidate,
                        affected_sessions,
                        incident_id,
                        now,
                        (
                            "resume-guard-signal-failed",
                            "The independent recovery controller declined or failed the OS pause signal.",
                        ),
                    );
                    return false;
                }
                _ => {
                    if let Some(stored) = self
                        .ledger
                        .incidents
                        .iter_mut()
                        .rev()
                        .find(|value| value.get("id") == incident.get("id"))
                        .and_then(Value::as_object_mut)
                    {
                        stored.insert(
                            "control_phase".to_owned(),
                            Value::String("committed".to_owned()),
                        );
                        stored.insert("updated_at".to_owned(), json!(rounded(now, 3)));
                    }
                    self.ledger.last_pressure_action_at = now;
                    if !self.persist_runtime(now) {
                        self.runtime_error = Some(
                            "the independent App brake owns the signal, but its committed state could not be refreshed"
                                .to_owned(),
                        );
                    }
                    self.emit_event(
                        "codex-app-process-pause",
                        "committed",
                        "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE COMMITTED\nThe independent recovery controller owns the pause and timed resume. Its OS signal acknowledgement is still being reconciled; no second pause signal will be issued.",
                        &format!("app-pause-committed:{incident_id}"),
                        Map::from_iter([
                            ("severity".to_owned(), Value::String("warning".to_owned())),
                            ("pid".to_owned(), json!(candidate.pid)),
                            ("cause".to_owned(), Value::String(reason.to_owned())),
                            ("surface".to_owned(), Value::String(APP_SERVER_SURFACE.to_owned())),
                            ("audience_provider".to_owned(), Value::String("codex".to_owned())),
                            ("audience_sessions".to_owned(), json!(affected_sessions)),
                            ("action".to_owned(), Value::String("pause-committed".to_owned())),
                        ]),
                    );
                    return true;
                }
            }
        } else if let Err(error) = suspend_process(&self.platform, candidate.pid) {
            log_event(
                &self.directory,
                &format!("APP_SUSPEND_ERROR pid={} error={error}", candidate.pid),
            );
            self.cancel_codex_app_pause(
                previous,
                candidate,
                affected_sessions,
                incident_id,
                now,
                (
                    "signal-failed",
                    "The OS rejected the exact blind-child pause signal.",
                ),
            );
            return false;
        }
        self.ledger.codex_app.control.pending_physical = None;
        if let Some(stored) = self
            .ledger
            .incidents
            .iter_mut()
            .rev()
            .find(|value| value.get("id") == incident.get("id"))
            .and_then(Value::as_object_mut)
        {
            stored.insert(
                "control_phase".to_owned(),
                Value::String("active".to_owned()),
            );
            stored.insert("updated_at".to_owned(), json!(rounded(now, 3)));
        }
        self.ledger.last_pressure_action_at = now;
        if !self.persist_runtime(now) {
            let resumed = resume_process(&self.platform, candidate.pid).is_ok();
            if resumed {
                self.cancel_codex_app_pause(
                    previous,
                    candidate,
                    affected_sessions,
                    incident_id,
                    now,
                    (
                        "final-state-persistence-failed",
                        "The pause signal completed, but its final state could not be saved, so the exact process was released immediately.",
                    ),
                );
            } else {
                self.runtime_error = Some(
                    "Codex App pause completed, but final persistence and immediate release both failed; the durable recovery record remains prepared"
                        .to_owned(),
                );
                self.emit_event(
                    "codex-app-process-pause",
                    "recovery-pending",
                    "[Memory Supervisor] CODEX APP BRAKE RECOVERY PENDING\nThe exact process was paused, but the final state write and immediate release failed. The durable prepared record remains available for automatic or owner recovery.",
                    &format!("app-pause-recovery-pending:{incident_id}"),
                    Map::from_iter([
                        ("severity".to_owned(), Value::String("critical".to_owned())),
                        ("pid".to_owned(), json!(candidate.pid)),
                        (
                            "cause".to_owned(),
                            Value::String("persistence-and-release-failed".to_owned()),
                        ),
                        (
                            "surface".to_owned(),
                            Value::String(APP_SERVER_SURFACE.to_owned()),
                        ),
                        (
                            "audience_provider".to_owned(),
                            Value::String("codex".to_owned()),
                        ),
                        ("audience_sessions".to_owned(), json!(affected_sessions)),
                        (
                            "action".to_owned(),
                            Value::String("recovery-pending".to_owned()),
                        ),
                    ]),
                );
            }
            // A failed release means the physical brake really did take effect. Report an action
            // to the scheduler so it cannot select another target in the same pressure tick.
            return !resumed;
        }
        let scope = if shared_host {
            "the shared Codex App server"
        } else {
            "one App-started child whose thread owner is unknown"
        };
        let message = format!(
            "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE\nTarget: {scope}, PID {}.\nWhy: all smaller effective controls completed and sustained App growth still approached the recovery boundary.\nEffect: the exact OS process is paused reversibly; no individual thread is blamed. All App leads receive recovery state at their next hook.",
            candidate.pid
        );
        self.emit_event(
            "codex-app-process-pause",
            "suspended",
            &message,
            incident
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            Map::from_iter([
                ("severity".to_owned(), Value::String("critical".to_owned())),
                ("pid".to_owned(), json!(candidate.pid)),
                ("cause".to_owned(), Value::String(reason.to_owned())),
                (
                    "surface".to_owned(),
                    Value::String(APP_SERVER_SURFACE.to_owned()),
                ),
                (
                    "thread_attribution".to_owned(),
                    Value::String("blind".to_owned()),
                ),
                ("affected_sessions".to_owned(), json!(affected_sessions)),
                (
                    "audience_provider".to_owned(),
                    Value::String("codex".to_owned()),
                ),
                ("audience_sessions".to_owned(), json!(affected_sessions)),
                ("action".to_owned(), Value::String("paused".to_owned())),
            ]),
        );
        true
    }

    fn alert_and_suspend(
        &mut self,
        leaks: &[TrackedProcess],
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        let active: BTreeSet<_> = leaks.iter().map(|leak| leak.identity.clone()).collect();
        self.warned.retain(|identity| active.contains(identity));
        let mut eligible = Vec::new();
        for leak in leaks {
            if self.warned.insert(leak.identity.clone()) {
                let message = format!(
                    "[Memory Supervisor] PROCESS GROWTH OBSERVED\nTarget: {} ({}), PID {}\nEvidence: {} MiB at an observed average of {} MiB/s.\nEffect: observation only; no process was paused. Run `memory-status` for current evidence.",
                    leak.name, leak.role, leak.pid, leak.anon_mb, leak.slope_mb_s
                );
                self.emit_event(
                    "leak-suspect",
                    "detected",
                    &message,
                    &leak.identity,
                    Map::from_iter([
                        ("severity".to_owned(), Value::String("warning".to_owned())),
                        ("pid".to_owned(), json!(leak.pid)),
                        ("role".to_owned(), Value::String(leak.role.clone())),
                        (
                            "cause".to_owned(),
                            Value::String("material-process-growth-observation".to_owned()),
                        ),
                        ("importance".to_owned(), Value::String("detail".to_owned())),
                    ]),
                );
            }
            if self.leak_action == "stop"
                && leak.runaway_verified
                && self.direct_process_risk(leak, assessment)
                && !self.codex_app_physical_control_forbidden(leak.pid)
                && self.logical_process_exhausted(leak.pid, now, assessment.reaction_s)
                && (leak.anon_mb as f64 >= self.policy.value("MEMORY_SUPERVISOR_LEAK_STOP_MB")
                    || assessment.collapse_imminent)
                && (leak.role != "lead" || self.lead_pause_authorized(leak, assessment, tracked))
                && self.ledger.stopped_identity(leak.pid).is_none()
                && !self.ledger.resume_cooldown.contains_key(&leak.identity)
                && leak.identity_reliable
                && leak.pid != std::process::id()
                && self.runtime_error.is_none()
                && self
                    .ledger
                    .probation
                    .as_ref()
                    .is_none_or(|probation| probation.identity != leak.identity)
            {
                eligible.push(leak);
            }
        }
        let priority = |role: &str| match role {
            "worker" => 2,
            "support" => 1,
            _ => 0,
        };
        let Some(candidate) = eligible.into_iter().max_by(|left, right| {
            let left_tte = left
                .runaway
                .as_ref()
                .and_then(|value| value.candidate_tte_s)
                .unwrap_or(f64::INFINITY);
            let right_tte = right
                .runaway
                .as_ref()
                .and_then(|value| value.candidate_tte_s)
                .unwrap_or(f64::INFINITY);
            right_tte
                .total_cmp(&left_tte)
                .then_with(|| priority(&left.role).cmp(&priority(&right.role)))
                .then_with(|| left.slope_mb_s.total_cmp(&right.slope_mb_s))
                .then_with(|| left.anon_mb.cmp(&right.anon_mb))
        }) else {
            return false;
        };
        self.suspend_candidate(candidate, assessment, now, "runaway-memory")
    }

    fn direct_process_risk(&self, process: &TrackedProcess, assessment: &Assessment) -> bool {
        if !matches!(assessment.action, Action::Hold | Action::Drain)
            || assessment.native_confidence == "low"
            || process.slope_mb_s < 1.0
        {
            return false;
        }
        let usable_headroom =
            (assessment.mem_available_mb as f64 - assessment.automatic_reserve_mb).max(0.0);
        let process_tte = usable_headroom / process.slope_mb_s;
        let intervention_horizon = (self.leak_window_s * 4.0).max(assessment.reaction_s * 12.0);
        assessment.collapse_imminent
            || assessment
                .time_to_exhaustion_s
                .is_some_and(|time| time <= assessment.reaction_s * 6.0)
            || process_tte <= intervention_horizon
    }

    fn pressure_incidents(&self) -> Vec<Value> {
        self.ledger
            .incidents
            .iter()
            .filter(|incident| {
                incident.get("status").and_then(Value::as_str) == Some("suspended")
                    && (matches!(
                        incident.get("reason").and_then(Value::as_str),
                        Some(
                            "pressure-pause"
                                | "pressure-lead-last-resort"
                                | "hard-cap-pause"
                                | "hard-cap-lead-last-resort"
                        )
                    ) || incident_is_codex_app_physical(incident))
                    && self
                        .ledger
                        .stopped_identity(value_u32(incident, "pid"))
                        .is_some()
            })
            .cloned()
            .collect()
    }

    fn apply_pressure_actions(
        &mut self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        if self.ledger.probation.is_some() {
            return false;
        }
        let globally_safe = matches!(assessment.action, Action::Allow | Action::Observe)
            && assessment.distress == "normal";
        let app_recovery_safe =
            codex_app_recovery_safe(assessment, &self.codex_app_snapshot.pressure);
        let safe = globally_safe || app_recovery_safe;
        if safe {
            self.critical_since = None;
            let Some(recovery_since) = self.recovery_since else {
                self.recovery_since = Some(now);
                return false;
            };
            if now - recovery_since < assessment.reaction_s * 2.0
                || now - self.ledger.last_pressure_action_at < assessment.reaction_s
            {
                return false;
            }
            let Some(incident) = self
                .pressure_incidents()
                .into_iter()
                .filter(|incident| globally_safe || incident_is_codex_app_physical(incident))
                .min_by_key(|incident| {
                    let reason = incident
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    (
                        match reason {
                            "app-shared-host-last-resort" => 0,
                            "app-blind-child-last-resort" => 1,
                            _ => 2,
                        },
                        incident.get("role").and_then(Value::as_str) == Some("lead"),
                        incident
                            .get("anon_mb")
                            .and_then(Value::as_u64)
                            .unwrap_or_default(),
                    )
                })
            else {
                return false;
            };
            let pid = value_u32(&incident, "pid");
            let identity = value_string(&incident, "identity");
            let Some(process) = process_by_pid(&self.platform, pid) else {
                return false;
            };
            if process_identity(&process) != identity
                || resume_process(&self.platform, pid).is_err()
            {
                return false;
            }
            self.ledger.clear_stopped(pid);
            let incident_reason = value_string(&incident, "reason");
            let app_physical = incident_is_codex_app_physical(&incident);
            let source = if app_physical {
                "codex-app-pressure-recovery"
            } else if incident_reason.starts_with("hard-cap-") {
                "hard-cap-recovery"
            } else {
                "pressure-recovery"
            };
            let transitioned = self
                .ledger
                .transition_incident(&identity, "resumed", now, source, Map::new())
                .clone();
            self.ledger.last_pressure_action_at = now;
            if app_physical {
                // Physical recovery is followed by a fresh, live observation window. Time spent
                // while the process was stopped never counts toward logical reopening.
                self.ledger.codex_app.control.recovery_since = Some(now);
                self.ledger.codex_app.control.last_action_at = now;
                self.ledger.codex_app.control.mode = "PHYSICAL_PROBATION".to_owned();
            }
            let message = incident_notice(&transitioned, "pressure_resumed", source);
            let delivery = terminal::write(
                &self.platform,
                pid,
                &process.terminal,
                &message,
                &process.terminal_identity,
            );
            if !self.persist_runtime(now) {
                self.runtime_error = Some(
                    "automatic pressure resume completed but runtime state was not persisted"
                        .to_owned(),
                );
            }
            let mut event_fields = Map::from_iter([
                ("pid".to_owned(), json!(pid)),
                ("cause".to_owned(), Value::String(source.to_owned())),
                ("action".to_owned(), Value::String("resumed".to_owned())),
                ("terminal".to_owned(), Value::String(delivery.status)),
            ]);
            extend_incident_audience(&mut event_fields, &transitioned);
            self.emit_event(
                "process-pause",
                "resumed",
                &message,
                transitioned
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                event_fields,
            );
            return true;
        }

        self.recovery_since = None;
        let cap_exceeded = assessment.cli_hard_cap_status == "exceeded";
        if assessment.collapse_imminent || cap_exceeded {
            self.critical_since.get_or_insert(now);
        } else {
            self.critical_since = None;
        }
        let runaway_active = self.ledger.incidents.iter().any(|incident| {
            incident.get("status").and_then(Value::as_str) == Some("suspended")
                && incident.get("reason").and_then(Value::as_str) == Some("runaway-memory")
        });
        let attributed_collapse = assessment.collapse_imminent
            && matches!(assessment.attribution.as_str(), "agent" | "mixed")
            && assessment.native_confidence != "low";
        if (!cap_exceeded && !attributed_collapse)
            || now - self.ledger.last_pressure_action_at < assessment.reaction_s
            || self.runtime_error.is_some()
            || runaway_active
        {
            return false;
        }
        // This branch runs only under an exceeded cap or attributed collapse, and a growing
        // support child (build, test, node) is then the most common real allocator — leaving it
        // untouchable made the kernel OOM killer the actual backstop for admitted work.
        let mut candidates: Vec<_> = tracked
            .iter()
            .filter(|process| {
                (process.role == "worker" || process.role == "support")
                    && process.slope_mb_s >= 1.0
                    && process.identity_reliable
                    && !self.codex_app_physical_control_forbidden(process.pid)
                    && self.pressure_target_effective_or_non_app(process)
                    && self.logical_process_exhausted(process.pid, now, assessment.reaction_s)
                    && self.ledger.stopped_identity(process.pid).is_none()
                    && !self.ledger.resume_cooldown.contains_key(&process.identity)
            })
            .collect();
        candidates.sort_by(|left, right| {
            left.runaway_verified
                .cmp(&right.runaway_verified)
                .then_with(|| {
                    let tte = |process: &TrackedProcess| {
                        process
                            .runaway
                            .as_ref()
                            .and_then(|verdict| verdict.candidate_tte_s)
                            .unwrap_or(f64::INFINITY)
                    };
                    tte(right).total_cmp(&tte(left))
                })
                .then_with(|| left.slope_mb_s.total_cmp(&right.slope_mb_s))
                .then_with(|| left.anon_mb.cmp(&right.anon_mb))
        });
        if let Some(candidate) = candidates.last()
            && self.suspend_candidate(
                candidate,
                assessment,
                now,
                if cap_exceeded {
                    "hard-cap-pause"
                } else {
                    "pressure-pause"
                },
            )
        {
            self.ledger.last_pressure_action_at = now;
            let _ = self.persist_runtime(now);
            return true;
        }
        let critical_long_enough = self
            .critical_since
            .is_some_and(|since| now - since >= assessment.reaction_s * 2.0);
        if !critical_long_enough {
            return false;
        }
        let reason = if cap_exceeded {
            "hard-cap-lead-last-resort"
        } else {
            "pressure-lead-last-resort"
        };
        let candidate = tracked
            .iter()
            .filter(|process| {
                process.role == "lead"
                    && process.slope_mb_s >= 1.0
                    && process.identity_reliable
                    && self.pressure_target_effective_or_non_app(process)
                    && self.logical_process_exhausted(process.pid, now, assessment.reaction_s)
                    && self.lead_pause_authorized(process, assessment, tracked)
                    && self.ledger.stopped_identity(process.pid).is_none()
                    && !self.ledger.resume_cooldown.contains_key(&process.identity)
                    && !self.ledger.incidents.iter().any(|incident| {
                        incident.get("identity").and_then(Value::as_str)
                            == Some(process.identity.as_str())
                            && incident.get("reason").and_then(Value::as_str) == Some(reason)
                    })
            })
            .max_by(|left, right| {
                left.runaway_verified
                    .cmp(&right.runaway_verified)
                    .then_with(|| {
                        let tte = |process: &TrackedProcess| {
                            process
                                .runaway
                                .as_ref()
                                .and_then(|verdict| verdict.candidate_tte_s)
                                .or(assessment.time_to_exhaustion_s)
                                .unwrap_or(f64::INFINITY)
                        };
                        tte(right).total_cmp(&tte(left))
                    })
                    .then_with(|| {
                        let started = |process: &TrackedProcess| {
                            self.ledger
                                .logical_agents
                                .values()
                                .filter(|agent| agent.process_pid == Some(process.pid))
                                .map(|agent| agent.started_at)
                                .max_by(f64::total_cmp)
                                .unwrap_or_default()
                        };
                        started(left).total_cmp(&started(right))
                    })
            });
        if let Some(candidate) = candidate
            && self.suspend_candidate(candidate, assessment, now, reason)
        {
            self.ledger.last_pressure_action_at = now;
            let _ = self.persist_runtime(now);
            return true;
        }
        false
    }

    fn app_agent_handoff_receipt_current(
        &self,
        agent: &LogicalAgent,
        now: f64,
        _reaction_s: f64,
    ) -> bool {
        // A receipt is bound to the exact control epoch, logical state, transition time, and App
        // Server generation. Its useful lifetime therefore matches the already-bounded live hook
        // route rather than an arbitrary four-reaction window that can expire mid-way through a
        // valid multi-owner stopping schedule.
        let receipt_fresh_s = CODEX_APP_HOOK_ROUTE_FRESH_S;
        agent.active
            && agent.state == LogicalState::HandoffOnly
            && agent.last_hook_receipt_epoch == Some(agent.epoch)
            && agent.last_hook_receipt_state == Some(LogicalState::HandoffOnly)
            && agent.last_hook_receipt_at.is_some_and(|receipt| {
                receipt + 0.001 >= agent.state_since && now - receipt <= receipt_fresh_s
            })
    }

    fn app_agent_handoff_observed(&self, agent: &LogicalAgent, now: f64, reaction_s: f64) -> bool {
        now - agent.state_since >= reaction_s
            && self.app_agent_handoff_receipt_current(agent, now, reaction_s)
    }

    fn app_keys_handoff_for(&self, keys: &BTreeSet<String>, now: f64, reaction_s: f64) -> bool {
        !keys.is_empty()
            && keys.iter().all(|key| {
                self.ledger
                    .logical_agents
                    .get(key)
                    .is_some_and(|agent| self.app_agent_handoff_observed(agent, now, reaction_s))
            })
    }

    fn app_server_sessions(&self, app_server_pid: u32) -> Vec<String> {
        let mut sessions: Vec<_> = self
            .ledger
            .codex_app
            .threads
            .values()
            .filter(|thread| thread.app_server_pid == app_server_pid)
            .map(|thread| thread.session_id.clone())
            .collect();
        sessions.sort();
        sessions.dedup();
        sessions
    }

    fn active_app_keys_for_server(&self, app_server_pid: u32) -> BTreeSet<String> {
        let sessions: BTreeSet<_> = self
            .ledger
            .codex_app
            .threads
            .values()
            .filter(|thread| thread.app_server_pid == app_server_pid)
            .map(|thread| thread.session_id.clone())
            .collect();
        self.ledger
            .logical_agents
            .values()
            .filter(|agent| {
                agent.active
                    && agent.surface == APP_SERVER_SURFACE
                    && sessions.contains(&agent.session_id)
            })
            .map(|agent| agent.key.clone())
            .collect()
    }

    fn apply_codex_app_blind_backstop(
        &mut self,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        now: f64,
    ) -> bool {
        let profile = self.codex_app_snapshot.pressure.clone();
        if self.leak_action != "stop"
            || !profile.causal
            || !assessment.collapse_imminent
            || !self.ledger.codex_app.control.surface_gate
            || self.runtime_error.is_some()
            || self.ledger.codex_app.control.pending_physical.is_some()
            || now - self.ledger.codex_app.control.last_action_at < assessment.reaction_s
            || now - self.ledger.last_pressure_action_at < assessment.reaction_s
        {
            return false;
        }
        let (blind_to_server, related_by_pid) = self.codex_app_blind_target_scope();
        let minimum_span = (self.leak_window_s * 0.8).max(5.0);
        let mut blind_candidates: Vec<_> = tracked
            .iter()
            .filter(|process| blind_to_server.contains_key(&process.pid))
            .filter(|process| {
                blind_to_server
                    .get(&process.pid)
                    .is_some_and(|server| self.codex_app_hook_active_for(*server))
                    && self.app_physical_growth_ready(
                        process,
                        profile.app_growth_mb_s,
                        minimum_span,
                    )
                    && related_by_pid.get(&process.pid).is_some_and(|keys| {
                        self.app_keys_handoff_for(keys, now, assessment.reaction_s)
                    })
            })
            .collect();
        blind_candidates.sort_by(|left, right| {
            left.slope_mb_s
                .total_cmp(&right.slope_mb_s)
                .then_with(|| left.anon_mb.cmp(&right.anon_mb))
        });
        if let Some(candidate) = blind_candidates.last() {
            let server_pid = blind_to_server[&candidate.pid];
            let sessions = self.app_server_sessions(server_pid);
            return self.suspend_codex_app_last_resort(
                candidate,
                assessment,
                now,
                "app-blind-child-last-resort",
                CodexAppPauseScope {
                    affected_sessions: &sessions,
                    app_server_pid: server_pid,
                    shared_host: false,
                },
            );
        }

        let critical_long_enough = self
            .critical_since
            .is_some_and(|since| now - since >= assessment.reaction_s * 2.0);
        if !critical_long_enough {
            return false;
        }
        let actionable_confirmed_child = self
            .ledger
            .codex_app
            .process_owners
            .values()
            .filter(|owner| owner.evidence.control_safe())
            .any(|owner| {
                tracked.iter().any(|process| {
                    process.pid == owner.pid
                        && self.app_physical_growth_ready(
                            process,
                            profile.app_growth_mb_s,
                            minimum_span,
                        )
                        && self.logical_process_exhausted(process.pid, now, assessment.reaction_s)
                })
            });
        if actionable_confirmed_child {
            // A smaller exact target is ready now. The shared host can never skip it.
            return false;
        }
        let child_growth: f64 = tracked
            .iter()
            .filter(|process| !self.codex_app_shared_host(process.pid))
            .filter(|process| {
                self.ledger.codex_app.owner_for_pid(process.pid).is_some()
                    || self
                        .codex_app_snapshot
                        .app_servers
                        .iter()
                        .any(|server| server.unattributed_pids.contains(&process.pid))
            })
            .map(|process| process.slope_mb_s.max(0.0))
            .sum();
        let host = tracked
            .iter()
            .filter(|process| self.codex_app_shared_host(process.pid))
            .filter(|process| {
                self.codex_app_hook_active_for(process.pid)
                    && self.app_host_growth_ready(
                        process,
                        profile.app_growth_mb_s,
                        child_growth,
                        minimum_span,
                    )
                    && {
                        let keys = self.active_app_keys_for_server(process.pid);
                        self.app_keys_handoff_for(&keys, now, assessment.reaction_s)
                    }
            })
            .max_by(|left, right| left.slope_mb_s.total_cmp(&right.slope_mb_s));
        let Some(host) = host else {
            return false;
        };
        let sessions = self.app_server_sessions(host.pid);
        self.suspend_codex_app_last_resort(
            host,
            assessment,
            now,
            "app-shared-host-last-resort",
            CodexAppPauseScope {
                affected_sessions: &sessions,
                app_server_pid: host.pid,
                shared_host: true,
            },
        )
    }

    fn manage_lead_probation(&mut self, assessment: &Assessment, now: f64) {
        if self.runtime_error.is_some()
            || self
                .ledger
                .probation
                .as_ref()
                .is_some_and(|probation| probation.status == "failed")
        {
            return;
        }
        if let Some(mut probation) = self.ledger.probation.clone() {
            let Some(process) = process_by_pid(&self.platform, probation.pid) else {
                self.ledger.transition_incident(
                    &probation.identity,
                    "gone",
                    now,
                    "probation",
                    Map::new(),
                );
                self.ledger.probation = None;
                let _ = self.persist_runtime(now);
                return;
            };
            if process_identity(&process) != probation.identity {
                return;
            }
            if !probation.signal_sent {
                if resume_process(&self.platform, probation.pid).is_err() {
                    return;
                }
                self.ledger.clear_stopped(probation.pid);
                probation.signal_sent = true;
                probation.baseline_mb = Some(process.anon_mb);
                probation.started_at = Some(now);
                probation.deadline = Some(now + self.leak_window_s);
                self.ledger.probation = Some(probation.clone());
                let incident = self
                    .ledger
                    .transition_incident(
                        &probation.identity,
                        "probation",
                        now,
                        "probation-resume",
                        Map::from_iter([("probation_attempted".to_owned(), Value::Bool(true))]),
                    )
                    .clone();
                if !self.persist_runtime(now) {
                    if suspend_process(&self.platform, probation.pid).is_err() {
                        self.runtime_error = Some(
                            "lead probation resumed but could not persist or roll back".to_owned(),
                        );
                        return;
                    }
                    self.ledger
                        .mark_stopped(probation.pid, probation.identity.clone());
                    probation.status = "failed".to_owned();
                    self.ledger.probation = Some(probation.clone());
                    self.ledger.transition_incident(
                        &probation.identity,
                        "probation_failed",
                        now,
                        "persistence-rollback",
                        Map::from_iter([("probation_attempted".to_owned(), Value::Bool(true))]),
                    );
                    if !self.persist_runtime(now) {
                        self.runtime_error =
                            Some("lead probation rollback could not be persisted".to_owned());
                    }
                    return;
                }
                let message = incident_notice(&incident, "probation", "automatic-one-shot");
                let delivery = terminal::write(
                    &self.platform,
                    process.pid,
                    &process.terminal,
                    &message,
                    &process.terminal_identity,
                );
                self.record_terminal_delivery(&value_string(&incident, "id"), &delivery);
                if !self.persist_runtime(now) {
                    log_event(
                        &self.directory,
                        &format!(
                            "RUNTIME_WRITE_RETRY lead probation terminal metadata pid={}",
                            probation.pid
                        ),
                    );
                }
                self.emit_event(
                    "lead-probation",
                    "monitoring",
                    &message,
                    &probation.identity,
                    Map::from_iter([
                        ("pid".to_owned(), json!(probation.pid)),
                        ("role".to_owned(), Value::String("lead".to_owned())),
                        ("action".to_owned(), Value::String("probation".to_owned())),
                        ("terminal".to_owned(), Value::String(delivery.status)),
                    ]),
                );
                return;
            }
            let elapsed = (now - probation.started_at.unwrap_or(now)).max(0.001);
            let growth =
                process.anon_mb as i64 - probation.baseline_mb.unwrap_or(process.anon_mb) as i64;
            let growth_rate = growth as f64 / elapsed;
            let relapse = assessment.distress == "critical"
                || (growth >= 32
                    && growth_rate
                        >= (self.policy.value("MEMORY_SUPERVISOR_LEAK_SLOPE_MBS") * 0.25).max(4.0));
            if relapse {
                if suspend_process(&self.platform, probation.pid).is_err() {
                    return;
                }
                self.ledger
                    .mark_stopped(probation.pid, probation.identity.clone());
                probation.status = "failed".to_owned();
                probation.failed_at = Some(now);
                probation.growth_mb_s = Some(rounded(growth_rate, 1));
                self.ledger.probation = Some(probation.clone());
                let incident = self
                    .ledger
                    .transition_incident(
                        &probation.identity,
                        "probation_failed",
                        now,
                        "probation",
                        Map::from_iter([
                            ("probation_attempted".to_owned(), Value::Bool(true)),
                            ("growth_mb_s".to_owned(), json!(rounded(growth_rate, 1))),
                        ]),
                    )
                    .clone();
                if !self.persist_runtime(now) {
                    if resume_process(&self.platform, probation.pid).is_err() {
                        self.runtime_error = Some(
                            "probation relapse was paused but could not be persisted or rolled back"
                                .to_owned(),
                        );
                        self.emit_event(
                            "protection-degraded",
                            "critical",
                            &format!(
                                "CRITICAL: lead pid={} relapsed and was paused, but the pause could not be persisted or rolled back. Run memory-status.",
                                probation.pid
                            ),
                            &format!("probation-untracked:{}:{now:.3}", probation.identity),
                            Map::from_iter([
                                ("severity".to_owned(), Value::String("critical".to_owned())),
                                ("pid".to_owned(), json!(probation.pid)),
                                ("role".to_owned(), Value::String("lead".to_owned())),
                                (
                                    "cause".to_owned(),
                                    Value::String("runtime-persistence-failure".to_owned()),
                                ),
                                (
                                    "action".to_owned(),
                                    Value::String("manual-choice".to_owned()),
                                ),
                            ]),
                        );
                        return;
                    }
                    self.ledger.clear_stopped(probation.pid);
                    self.ledger.transition_incident(
                        &probation.identity,
                        "resumed",
                        now,
                        "persistence-rollback",
                        Map::from_iter([
                            ("probation_attempted".to_owned(), Value::Bool(true)),
                            (
                                "recovery_visibility".to_owned(),
                                Value::String("degraded".to_owned()),
                            ),
                        ]),
                    );
                    let rollback_persisted = self.persist_runtime(now);
                    self.runtime_error = Some(
                        if rollback_persisted {
                            "probation relapse pause was rolled back because runtime state was not persisted"
                        } else {
                            "probation relapse pause was rolled back, but rollback state was not persisted"
                        }
                        .to_owned(),
                    );
                    self.emit_event(
                        "protection-degraded",
                        "critical",
                        &format!(
                            "Lead pid={} probation relapse pause was rolled back after a runtime persistence failure. Save work and run memory-status.",
                            probation.pid
                        ),
                        &format!("probation-persistence:{}:{now:.3}", probation.identity),
                        Map::from_iter([
                            ("severity".to_owned(), Value::String("critical".to_owned())),
                            ("pid".to_owned(), json!(probation.pid)),
                            ("role".to_owned(), Value::String("lead".to_owned())),
                            (
                                "cause".to_owned(),
                                Value::String("runtime-persistence-failure".to_owned()),
                            ),
                            ("action".to_owned(), Value::String("hold".to_owned())),
                        ]),
                    );
                    return;
                }
                let message = incident_notice(&incident, "probation_failed", "exact-pid-watchdog");
                let delivery = terminal::write(
                    &self.platform,
                    process.pid,
                    &process.terminal,
                    &message,
                    &process.terminal_identity,
                );
                self.record_terminal_delivery(&value_string(&incident, "id"), &delivery);
                if !self.persist_runtime(now) {
                    log_event(
                        &self.directory,
                        &format!(
                            "RUNTIME_WRITE_RETRY probation failure terminal metadata pid={}",
                            probation.pid
                        ),
                    );
                }
                self.emit_event(
                    "lead-probation",
                    "failed",
                    &message,
                    &probation.identity,
                    Map::from_iter([
                        ("pid".to_owned(), json!(probation.pid)),
                        ("role".to_owned(), Value::String("lead".to_owned())),
                        (
                            "action".to_owned(),
                            Value::String("manual-choice".to_owned()),
                        ),
                        ("terminal".to_owned(), Value::String(delivery.status)),
                    ]),
                );
                return;
            }
            if now >= probation.deadline.unwrap_or(f64::INFINITY) {
                let previous = self.ledger.clone();
                let incident = self
                    .ledger
                    .transition_incident(
                        &probation.identity,
                        "resumed",
                        now,
                        "probation",
                        Map::from_iter([("probation_attempted".to_owned(), Value::Bool(true))]),
                    )
                    .clone();
                self.ledger.probation = None;
                if !self.persist_runtime(now) {
                    self.ledger = previous;
                    return;
                }
                let message = incident_notice(&incident, "recovery_confirmed", "probation");
                let delivery = terminal::write(
                    &self.platform,
                    process.pid,
                    &process.terminal,
                    &message,
                    &process.terminal_identity,
                );
                self.record_terminal_delivery(&value_string(&incident, "id"), &delivery);
                if !self.persist_runtime(now) {
                    log_event(
                        &self.directory,
                        &format!(
                            "RUNTIME_WRITE_RETRY probation success terminal metadata pid={}",
                            process.pid
                        ),
                    );
                }
                self.emit_event(
                    "lead-probation",
                    "resumed",
                    &message,
                    &value_string(&incident, "identity"),
                    Map::from_iter([
                        ("pid".to_owned(), json!(process.pid)),
                        ("action".to_owned(), Value::String("resumed".to_owned())),
                        ("terminal".to_owned(), Value::String(delivery.status)),
                    ]),
                );
            }
            return;
        }

        let candidate = self.ledger.incidents.iter().rev().find(|incident| {
            incident.get("status").and_then(Value::as_str) == Some("suspended")
                && incident.get("reason").and_then(Value::as_str) == Some("runaway-memory")
                && incident.get("role").and_then(Value::as_str) == Some("lead")
                && incident.get("recovery_policy").and_then(Value::as_str) == Some("lead-probation")
                && !incident
                    .get("probation_attempted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                && self
                    .ledger
                    .stopped_identity(value_u32(incident, "pid"))
                    .is_some()
        });
        let Some(candidate) = candidate.cloned() else {
            return;
        };
        if assessment.distress != "normal"
            || !matches!(assessment.action, Action::Allow | Action::Observe)
        {
            return;
        }
        let required = assessment.automatic_reserve_mb
            + value_f64(&candidate, "slope_mb_s").max(0.0) * assessment.reaction_s;
        if (assessment.mem_available_mb as f64) < required {
            return;
        }
        let pid = value_u32(&candidate, "pid");
        let identity = value_string(&candidate, "identity");
        let Some(process) = process_by_pid(&self.platform, pid) else {
            return;
        };
        if process_identity(&process) != identity {
            return;
        }
        let previous = self.ledger.clone();
        self.ledger.probation = Some(Probation {
            status: "monitoring".to_owned(),
            pid,
            identity: identity.clone(),
            signal_sent: false,
            prepared_at: Some(now),
            baseline_mb: None,
            started_at: None,
            deadline: None,
            failed_at: None,
            growth_mb_s: None,
        });
        self.ledger.transition_incident(
            &identity,
            "probation",
            now,
            "probation",
            Map::from_iter([("probation_attempted".to_owned(), Value::Bool(true))]),
        );
        if self.persist_runtime(now) {
            self.manage_lead_probation(assessment, now);
        } else {
            self.ledger = previous;
        }
    }

    fn revoke_codex_app_generation(&mut self, app_server_pid: u32, now: f64) {
        let sessions: BTreeSet<_> = self
            .ledger
            .codex_app
            .threads
            .values()
            .filter(|thread| thread.app_server_pid == app_server_pid)
            .map(|thread| thread.session_id.clone())
            .collect();
        let owned_identities: Vec<_> = self
            .ledger
            .codex_app
            .process_owners
            .iter()
            .filter(|(_, owner)| owner.app_server_pid == app_server_pid)
            .map(|(identity, _)| identity.clone())
            .collect();
        self.ledger
            .codex_app
            .threads
            .retain(|_, thread| thread.app_server_pid != app_server_pid);
        self.ledger
            .codex_app
            .invocations
            .retain(|_, invocation| invocation.app_server_pid != app_server_pid);
        self.ledger
            .codex_app
            .process_owners
            .retain(|_, owner| owner.app_server_pid != app_server_pid);
        for generations in self.ledger.codex_app.identity_collisions.values_mut() {
            generations.remove(&app_server_pid);
        }
        self.ledger
            .codex_app
            .identity_collisions
            .retain(|_, generations| generations.len() > 1);
        for identity in owned_identities {
            self.process_history.remove(&identity);
            self.ledger.runaway_confirmations.remove(&identity);
            self.warned.remove(&identity);
        }
        for agent in self.ledger.logical_agents.values_mut().filter(|agent| {
            agent.surface == APP_SERVER_SURFACE && sessions.contains(&agent.session_id)
        }) {
            // A new server generation must earn fresh hook/ownership evidence. Never transfer an
            // old generation's targeted state merely because the visible session id is reused.
            agent.active = false;
            agent.state = LogicalState::Active;
            agent.state_since = now;
            agent.in_flight_tool_class = None;
            agent.idle_since = Some(now);
            agent.last_hook_receipt_at = None;
            agent.last_hook_receipt_epoch = None;
            agent.last_hook_receipt_state = None;
            agent.reason =
                "Codex App server generation changed; awaiting a fresh hook boundary".to_owned();
        }
        let control = &mut self.ledger.codex_app.control;
        control.pending_physical = None;
        control.last_blind_target_at = 0.0;
        control.last_action_at = now;
        control.recovery_since = Some(now);
        control.mode = "SERVER_REPLACED".to_owned();
        control.reason =
            "the stopped App Server was resumed after a new server generation appeared".to_owned();
    }

    fn reconcile_stopped(&mut self, now: f64, processes: &BTreeMap<u32, ProcessInfo>) {
        let entries: Vec<_> = self
            .ledger
            .stopped
            .iter()
            .filter_map(|(pid, identity)| pid.parse().ok().map(|pid| (pid, identity.clone())))
            .collect();
        let mut changed = false;
        let mut refresh_app = false;
        for (pid, identity) in entries {
            let process = processes.get(&pid);
            if process.is_some_and(|process| process.start_token.is_empty()) {
                self.runtime_error = Some(format!(
                    "cannot verify supervisor-managed pid {pid}: process start identity missing"
                ));
                continue;
            }
            let pending_physical = self
                .ledger
                .codex_app
                .control
                .pending_physical
                .as_ref()
                .filter(|pending| pending.pid == pid && pending.identity == identity)
                .cloned();
            let guard_control_base = pending_physical.as_ref().and_then(|pending| {
                (!pending.guard_control_id.is_empty()).then(|| {
                    self.directory
                        .join("app-guards")
                        .join(&pending.guard_control_id)
                })
            });
            let guard_phase = |phase: &str| {
                guard_control_base
                    .as_ref()
                    .is_some_and(|base| crate::app_guard::phase_path(base, phase).is_file())
            };
            let guard_token_phase = if guard_phase("committed") {
                Some("committed")
            } else if guard_phase("signalling") {
                Some("signalling")
            } else {
                None
            };
            let guard_committed_inflight = pending_physical
                .as_ref()
                .is_some_and(|pending| pending.scope == "shared-host")
                && guard_token_phase.is_some_and(|phase| {
                    guard_control_base.as_ref().is_some_and(|base| {
                        app_guard_controller_state(&self.platform, base, phase) != Some(false)
                    })
                })
                && ![
                    "suspended",
                    "resumed",
                    "completed",
                    "error",
                    "cancelled",
                    "expired",
                ]
                .into_iter()
                .any(guard_phase);
            let shared_host_incident = self
                .ledger
                .incidents
                .iter()
                .rev()
                .find(|incident| {
                    incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                        && incident.get("status").and_then(Value::as_str) == Some("suspended")
                        && incident.get("app_control_scope").and_then(Value::as_str)
                            == Some("shared-host")
                })
                .cloned();
            let replacement_pid = shared_host_incident.as_ref().and_then(|_| {
                crate::codex_app::app_server_pids(processes)
                    .into_iter()
                    .find(|candidate| *candidate != pid)
            });
            if let (Some(original), Some(replacement_pid), Some(process)) =
                (shared_host_incident, replacement_pid, process)
                && process_identity(process) == identity
                && !guard_committed_inflight
                && resume_process(&self.platform, pid).is_ok()
            {
                self.ledger.clear_stopped(pid);
                self.ledger
                    .resume_cooldown
                    .insert(identity.clone(), now + self.resume_cooldown_s);
                let transitioned = self
                    .ledger
                    .transition_incident(
                        &identity,
                        "resumed",
                        now,
                        "codex-app-server-replacement",
                        Map::from_iter([("replacement_pid".to_owned(), json!(replacement_pid))]),
                    )
                    .clone();
                self.revoke_codex_app_generation(pid, now);
                self.ledger.last_pressure_action_at = now;
                let message = format!(
                    "[Memory Supervisor] CODEX APP SERVER RESUMED\nThe previously paused App Server PID {pid} was resumed immediately because a new App Server generation (PID {replacement_pid}) appeared. Old thread ownership was discarded; the new generation must establish fresh hook evidence."
                );
                let mut fields = Map::from_iter([
                    ("pid".to_owned(), json!(pid)),
                    ("replacement_pid".to_owned(), json!(replacement_pid)),
                    (
                        "cause".to_owned(),
                        Value::String("app-server-generation-changed".to_owned()),
                    ),
                    ("action".to_owned(), Value::String("resumed".to_owned())),
                ]);
                extend_incident_audience(&mut fields, &original);
                self.emit_event(
                    "codex-app-process-pause",
                    "resumed",
                    &message,
                    transitioned
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    fields,
                );
                changed = true;
                refresh_app = true;
                continue;
            }
            if process.is_none_or(|process| process_identity(process) != identity) {
                self.ledger.clear_stopped(pid);
                if self
                    .ledger
                    .probation
                    .as_ref()
                    .is_some_and(|probation| probation.identity == identity)
                {
                    self.ledger.probation = None;
                }
                let pending_action = self
                    .ledger
                    .pending_control
                    .as_ref()
                    .filter(|pending| pending.pid == pid)
                    .map(|pending| pending.action.clone());
                let status = match pending_action.as_deref() {
                    Some("terminate") => "terminated",
                    Some("kill") => "killed",
                    _ => "gone",
                };
                self.ledger
                    .transition_incident(&identity, status, now, "reconcile", Map::new());
                if pending_action.is_some() {
                    self.ledger.pending_control = None;
                }
                if self
                    .ledger
                    .codex_app
                    .control
                    .pending_physical
                    .as_ref()
                    .is_some_and(|pending| pending.pid == pid)
                {
                    self.ledger.codex_app.control.pending_physical = None;
                }
                changed = true;
                continue;
            }
            let state = process_state(&self.platform, pid);
            let externally_resumed = if self.platform == "windows" {
                state == "R"
            } else {
                !matches!(state.as_str(), "missing" | "unknown") && !state.starts_with('T')
            };
            let guard_recovery_lost = pending_physical
                .as_ref()
                .is_some_and(|pending| pending.scope == "shared-host")
                && (guard_phase("error")
                    || ["committed", "signalling", "suspended"]
                        .into_iter()
                        .any(|phase| {
                            guard_phase(phase)
                                && guard_control_base.as_ref().is_some_and(|base| {
                                    app_guard_controller_state(&self.platform, base, phase)
                                        == Some(false)
                                })
                        }));
            if state.starts_with('T') && guard_recovery_lost {
                match resume_process(&self.platform, pid) {
                    Ok(()) => {
                        self.ledger.clear_stopped(pid);
                        self.ledger.codex_app.control.pending_physical = None;
                        self.ledger
                            .resume_cooldown
                            .insert(identity.clone(), now + self.resume_cooldown_s);
                        let incident = self
                            .ledger
                            .transition_incident(
                                &identity,
                                "resumed",
                                now,
                                "app-guard-recovery-takeover",
                                Map::from_iter([(
                                    "control_phase".to_owned(),
                                    Value::String("recovered".to_owned()),
                                )]),
                            )
                            .clone();
                        self.ledger.codex_app.control.recovery_since = Some(now);
                        self.ledger.codex_app.control.last_action_at = now;
                        self.ledger.codex_app.control.mode = "PHYSICAL_PROBATION".to_owned();
                        self.ledger.last_pressure_action_at = now;
                        let mut fields = Map::from_iter([
                            ("severity".to_owned(), Value::String("warning".to_owned())),
                            ("pid".to_owned(), json!(pid)),
                            (
                                "cause".to_owned(),
                                Value::String("independent-controller-lost".to_owned()),
                            ),
                            ("action".to_owned(), Value::String("resumed".to_owned())),
                        ]);
                        extend_incident_audience(&mut fields, &incident);
                        self.emit_event(
                            "codex-app-process-pause",
                            "resumed",
                            "[Memory Supervisor] CODEX APP SERVER RESUMED\nThe independent pause controller ended before it could prove recovery, so the daemon immediately released the exact App Server and started a fresh live recovery window.",
                            &format!("app-guard-takeover:{}", value_string(&incident, "id")),
                            fields,
                        );
                        changed = true;
                        continue;
                    }
                    Err(error) => {
                        self.ledger.last_pressure_action_at = now;
                        log_event(
                            &self.directory,
                            &format!("APP_GUARD_RECOVERY_ERROR pid={pid} error={error}"),
                        );
                        continue;
                    }
                }
            }
            if externally_resumed && guard_committed_inflight {
                // Atomic commit transfers the signal to the independent controller. Until it
                // produces a terminal receipt, clearing this durable episode could let an old
                // controller pause the same App Server after a newer decision has begun.
                continue;
            }
            if externally_resumed
                && let Some(pending) = pending_physical.as_ref()
                && (pending.scope != "shared-host"
                    || !guard_phase("suspended") && !guard_phase("resumed"))
            {
                self.ledger.clear_stopped(pid);
                self.ledger.codex_app.control.pending_physical = None;
                let incident = self
                    .ledger
                    .transition_incident(
                        &identity,
                        "cancelled",
                        now,
                        "reconcile-uncommitted-app-pause",
                        Map::from_iter([(
                            "control_phase".to_owned(),
                            Value::String("cancelled".to_owned()),
                        )]),
                    )
                    .clone();
                let mut fields = Map::from_iter([
                    ("severity".to_owned(), Value::String("warning".to_owned())),
                    ("pid".to_owned(), json!(pid)),
                    (
                        "cause".to_owned(),
                        Value::String("prepared-pause-not-committed".to_owned()),
                    ),
                    ("action".to_owned(), Value::String("cancelled".to_owned())),
                ]);
                extend_incident_audience(&mut fields, &incident);
                self.emit_event(
                    "codex-app-process-pause",
                    "cancelled",
                    "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE CANCELLED\nA prepared App pause was found without a completed OS pause or a live committed controller. The running process was left untouched.",
                    &format!("app-pause-reconcile-cancelled:{}", value_string(&incident, "id")),
                    fields,
                );
                changed = true;
                continue;
            }
            if externally_resumed {
                let app_physical = self
                    .ledger
                    .incidents
                    .iter()
                    .rev()
                    .find(|incident| {
                        incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                            && incident.get("status").and_then(Value::as_str) == Some("suspended")
                    })
                    .is_some_and(incident_is_codex_app_physical);
                self.ledger.clear_stopped(pid);
                if self
                    .ledger
                    .probation
                    .as_ref()
                    .is_some_and(|probation| probation.identity == identity)
                {
                    self.ledger.probation = None;
                }
                self.ledger
                    .resume_cooldown
                    .insert(identity.clone(), now + self.resume_cooldown_s);
                let incident = self
                    .ledger
                    .transition_incident(&identity, "resumed", now, "external", Map::new())
                    .clone();
                if app_physical {
                    // Guard/manual recovery starts a new live probation window just like daemon
                    // recovery; stopped time can never unlock logical work.
                    self.ledger.codex_app.control.recovery_since = Some(now);
                    self.ledger.codex_app.control.last_action_at = now;
                    self.ledger.codex_app.control.mode = "PHYSICAL_PROBATION".to_owned();
                }
                let message = incident_notice(&incident, "external_resumed", "external-signal");
                // The event must survive even when the resumed PID is no longer resolvable in
                // this tick's inventory (it may have exited immediately); only the exact
                // terminal write depends on a resolved process.
                let terminal_status = if let Some(process) = process {
                    terminal::write(
                        &self.platform,
                        pid,
                        &process.terminal,
                        &message,
                        &process.terminal_identity,
                    )
                    .status
                } else {
                    "unavailable".to_owned()
                };
                let mut event_fields = Map::from_iter([
                    ("pid".to_owned(), json!(pid)),
                    (
                        "cause".to_owned(),
                        Value::String("external-resume".to_owned()),
                    ),
                    ("action".to_owned(), Value::String("resumed".to_owned())),
                    ("terminal".to_owned(), Value::String(terminal_status)),
                ]);
                extend_incident_audience(&mut event_fields, &incident);
                self.emit_event(
                    "process-control",
                    "resumed",
                    &message,
                    &format!("external-resume:{}:{now:.3}", value_string(&incident, "id")),
                    event_fields,
                );
                if self
                    .ledger
                    .pending_control
                    .as_ref()
                    .is_some_and(|pending| pending.pid == pid)
                {
                    self.ledger.pending_control = None;
                }
                if self
                    .ledger
                    .codex_app
                    .control
                    .pending_physical
                    .as_ref()
                    .is_some_and(|pending| pending.pid == pid)
                {
                    self.ledger.codex_app.control.pending_physical = None;
                }
                changed = true;
            } else if state.starts_with('T')
                && self
                    .ledger
                    .codex_app
                    .control
                    .pending_physical
                    .as_ref()
                    .is_some_and(|pending| pending.pid == pid && pending.identity == identity)
            {
                self.ledger.codex_app.control.pending_physical = None;
                let mut activated_incident = None;
                if let Some(incident) = self
                    .ledger
                    .incidents
                    .iter_mut()
                    .rev()
                    .find(|incident| {
                        incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                            && incident.get("status").and_then(Value::as_str) == Some("suspended")
                    })
                    .and_then(Value::as_object_mut)
                {
                    let was_active =
                        incident.get("control_phase").and_then(Value::as_str) == Some("active");
                    incident.insert(
                        "control_phase".to_owned(),
                        Value::String("active".to_owned()),
                    );
                    incident.insert("updated_at".to_owned(), json!(rounded(now, 3)));
                    if !was_active {
                        activated_incident = Some(Value::Object(incident.clone()));
                    }
                }
                if let Some(incident) = activated_incident {
                    let message = format!(
                        "[Memory Supervisor] CODEX APP LAST-RESORT BRAKE\nTarget PID: {pid}.\nWhy: all smaller effective controls completed and sustained App growth still approached the recovery boundary.\nEffect: the independent controller confirmed the reversible OS pause; all App leads receive recovery state at their next hook."
                    );
                    let mut fields = Map::from_iter([
                        ("severity".to_owned(), Value::String("critical".to_owned())),
                        ("pid".to_owned(), json!(pid)),
                        (
                            "cause".to_owned(),
                            Value::String(value_string(&incident, "reason")),
                        ),
                        (
                            "surface".to_owned(),
                            Value::String(APP_SERVER_SURFACE.to_owned()),
                        ),
                        ("action".to_owned(), Value::String("paused".to_owned())),
                    ]);
                    extend_incident_audience(&mut fields, &incident);
                    self.emit_event(
                        "codex-app-process-pause",
                        "suspended",
                        &message,
                        &value_string(&incident, "id"),
                        fields,
                    );
                }
                changed = true;
            }
        }
        let cooldowns = self.ledger.resume_cooldown.len();
        self.ledger
            .resume_cooldown
            .retain(|_, expires_at| *expires_at > now);
        changed |= cooldowns != self.ledger.resume_cooldown.len();
        if changed {
            if !self.persist_runtime(now) {
                self.runtime_error =
                    Some("reconciled runtime state could not be persisted".to_owned());
            } else if refresh_app {
                self.refresh_codex_app_adapter(now, processes);
            }
        }
    }

    fn process_control_requests(&mut self, now: f64, processes: &BTreeMap<u32, ProcessInfo>) {
        let control = self.directory.join("control");
        let results = control.join("results");
        let _ = ensure_private_dir(&control);
        let _ = ensure_private_dir(&results);
        let Ok(entries) = fs::read_dir(&control) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let request = fs::read(&path)
                .ok()
                .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
                .unwrap_or_else(|| json!({}));
            let request_id = request
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("request")
                });
            let request_id: String = request_id
                .chars()
                .filter(|character| character.is_alphanumeric() || matches!(*character, '-' | '_'))
                .take(128)
                .collect();
            let action = value_string(&request, "action").to_lowercase();
            let pid = value_u32(&request, "pid");
            let mut ok = false;
            let mut signal_completed = false;
            let mut guard_cancelled_before_signal = false;
            let result = (|| -> Result<(), String> {
                if self.runtime_error.is_some() {
                    return Err("runtime protection is degraded".to_owned());
                }
                if !matches!(action.as_str(), "resume" | "terminate" | "kill") {
                    return Err(format!("unsupported control action: {action}"));
                }
                let identity = self
                    .ledger
                    .stopped_identity(pid)
                    .ok_or_else(|| "target is not supervisor-managed".to_owned())?
                    .to_owned();
                let process = processes
                    .get(&pid)
                    .filter(|process| {
                        !process.start_token.is_empty() && process_identity(process) == identity
                    })
                    .ok_or_else(|| {
                        "target is not the same supervisor-managed suspended process".to_owned()
                    })?
                    .clone();
                let app_physical = self
                    .ledger
                    .incidents
                    .iter()
                    .rev()
                    .find(|incident| {
                        incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                            && incident.get("status").and_then(Value::as_str) == Some("suspended")
                    })
                    .is_some_and(incident_is_codex_app_physical);
                let app_guard_control_id = self
                    .ledger
                    .codex_app
                    .control
                    .pending_physical
                    .as_ref()
                    .filter(|pending| {
                        pending.pid == pid
                            && pending.identity == identity
                            && pending.scope == "shared-host"
                            && !pending.guard_control_id.is_empty()
                    })
                    .map(|pending| pending.guard_control_id.clone());
                if let Some(control_id) = app_guard_control_id {
                    let control_base = self.directory.join("app-guards").join(control_id);
                    let committed = crate::app_guard::phase_path(&control_base, "committed");
                    let signalling = crate::app_guard::phase_path(&control_base, "signalling");
                    let cancellable_token = if committed.is_file() {
                        Some(committed.clone())
                    } else if signalling.is_file()
                        && app_guard_controller_state(&self.platform, &control_base, "signalling")
                            == Some(false)
                    {
                        Some(signalling.clone())
                    } else {
                        None
                    };
                    if let Some(token) = cancellable_token {
                        let cancelled = crate::app_guard::phase_path(&control_base, "cancelled");
                        if fs::rename(&token, &cancelled).is_ok() {
                            guard_cancelled_before_signal = true;
                            let incident_id = self
                                .ledger
                                .incidents
                                .iter()
                                .rev()
                                .find(|incident| {
                                    incident.get("identity").and_then(Value::as_str)
                                        == Some(identity.as_str())
                                })
                                .and_then(|incident| incident.get("id"))
                                .cloned()
                                .unwrap_or(Value::Null);
                            let _ = write_atomic_json(
                                &cancelled,
                                &json!({
                                    "phase":"cancelled",
                                    "pid":pid,
                                    "incident_id":incident_id,
                                    "detail":"owner control won the signal token before the OS pause"
                                }),
                                0o600,
                                true,
                            );
                        } else if signalling.is_file() {
                            return Err(
                                "the independent App recovery controller owns an in-flight OS signal; retry after its pause receipt"
                                    .to_owned(),
                            );
                        } else {
                            return Err(
                                "the independent App recovery token changed during owner control; retry after reconciliation"
                                    .to_owned(),
                            );
                        }
                    } else if committed.is_file() || signalling.is_file() {
                        return Err(
                            "the independent App recovery controller owns an in-flight OS signal; retry after its pause receipt"
                                .to_owned(),
                        );
                    }
                }
                if let Some(pending) = &self.ledger.pending_control
                    && (pending.pid != pid
                        || pending.action != action
                        || pending.identity != identity)
                {
                    return Err(
                        "another persisted control operation needs resolution first".to_owned()
                    );
                }
                if self.ledger.pending_control.is_none() {
                    self.ledger.pending_control = Some(PendingControl {
                        action: action.clone(),
                        pid,
                        identity: identity.clone(),
                        requested_at: rounded(now, 3),
                        phase: None,
                    });
                    if !self.persist_runtime(now) {
                        self.ledger.pending_control = None;
                        return Err("could not persist control intent; no signal sent".to_owned());
                    }
                }
                match action.as_str() {
                    "resume" => resume_process(&self.platform, pid).map_err(|e| e.to_string())?,
                    "terminate" => {
                        terminate_process(&self.platform, pid, false).map_err(|e| e.to_string())?;
                        if self.platform != "windows" {
                            let _ = resume_process(&self.platform, pid);
                        }
                    }
                    "kill" => {
                        terminate_process(&self.platform, pid, true).map_err(|e| e.to_string())?
                    }
                    _ => unreachable!(),
                }
                signal_completed = true;
                self.ledger.clear_stopped(pid);
                if action == "resume" && !guard_cancelled_before_signal {
                    self.ledger
                        .resume_cooldown
                        .insert(identity.clone(), now + self.resume_cooldown_s);
                    if app_physical {
                        self.ledger.codex_app.control.recovery_since = Some(now);
                        self.ledger.codex_app.control.last_action_at = now;
                        self.ledger.codex_app.control.mode = "PHYSICAL_PROBATION".to_owned();
                    }
                }
                let status = match action.as_str() {
                    "resume" if guard_cancelled_before_signal => "cancelled",
                    "resume" => "resumed",
                    "terminate" => "terminated",
                    _ => "killed",
                };
                let incident = self
                    .ledger
                    .transition_incident(&identity, status, now, "memory-supervisor", Map::new())
                    .clone();
                self.ledger.pending_control = None;
                if self
                    .ledger
                    .codex_app
                    .control
                    .pending_physical
                    .as_ref()
                    .is_some_and(|pending| pending.pid == pid)
                {
                    self.ledger.codex_app.control.pending_physical = None;
                }
                if !self.persist_runtime(now) {
                    self.runtime_error = Some(
                        "control signal completed but final runtime state was not persisted"
                            .to_owned(),
                    );
                    return Err(self.runtime_error.clone().unwrap());
                }
                if action == "resume" {
                    let message = if guard_cancelled_before_signal {
                        "[Memory Supervisor] CODEX APP BRAKE CANCELLED\nOwner recovery cancelled the committed controller token before the OS pause. The exact App Server remained running."
                            .to_owned()
                    } else {
                        incident_notice(&incident, "manual_resumed", "memory-supervisor")
                    };
                    let delivery = terminal::write(
                        &self.platform,
                        pid,
                        &process.terminal,
                        &message,
                        &process.terminal_identity,
                    );
                    let mut event_fields = Map::from_iter([
                        ("pid".to_owned(), json!(pid)),
                        (
                            "cause".to_owned(),
                            Value::String("memory-supervisor".to_owned()),
                        ),
                        (
                            "action".to_owned(),
                            Value::String(
                                if guard_cancelled_before_signal {
                                    "cancelled"
                                } else {
                                    action.as_str()
                                }
                                .to_owned(),
                            ),
                        ),
                        ("terminal".to_owned(), Value::String(delivery.status)),
                    ]);
                    extend_incident_audience(&mut event_fields, &incident);
                    self.emit_event(
                        if guard_cancelled_before_signal {
                            "codex-app-process-pause"
                        } else {
                            "process-control"
                        },
                        if guard_cancelled_before_signal {
                            "cancelled"
                        } else {
                            "resumed"
                        },
                        &message,
                        request_id.as_str(),
                        event_fields,
                    );
                }
                ok = true;
                Ok(())
            })();
            let error = result.err();
            let _ = write_atomic_json(
                &results.join(format!("{request_id}.json")),
                &json!({
                    "request_id": request_id,
                    "ok": ok,
                    "signal_completed": signal_completed,
                    "action": action,
                    "pid": pid,
                    "completed_at": rounded(now_epoch(), 3),
                    "error": error,
                }),
                0o600,
                true,
            );
            let _ = fs::remove_file(path);
        }
    }

    pub fn tick(&mut self) -> Value {
        let now = now_epoch();
        self.cleanup_artifacts(now);
        self.drain_notification_events(now);
        self.drain_hook_observations(now);
        let snapshot = memory_snapshot(&self.platform, &mut self.config);
        self.policy = resolve_policy(&mut self.config, snapshot.capacity_mb);
        self.cli_hard_cap_mb = hard_cap(&mut self.config);
        let pressure = native_pressure(&self.platform);
        let raw_level = level_from(snapshot.available_mb, pressure.some_avg10, &self.policy);
        let utilization_transition =
            self.update_level(raw_level, now, snapshot.available_mb, pressure.some_avg10);
        let process_map = list_processes(&self.platform);
        self.refresh_codex_app_adapter(now, &process_map);
        let initial_tracked = tracked_processes(&process_map);
        self.system_history.push(HistorySample {
            ts: now,
            available: Some(snapshot.available_mb as f64),
            tracked: Some(initial_tracked.iter().map(|item| item.anon_mb).sum::<u64>() as f64),
            worker: Some(
                initial_tracked
                    .iter()
                    .filter(|item| item.role == "worker")
                    .map(|item| item.anon_mb)
                    .sum::<u64>() as f64,
            ),
            reclaim: Some(pressure.reclaim_total),
            swap: Some(pressure.swap_total),
            oom: Some(pressure.oom_total as f64),
            commit_remaining: pressure.commit_remaining_mb.map(|value| value as f64),
        });
        let long_window = 120.0_f64.max(self.leak_window_s * 3.0);
        self.system_history
            .retain(|sample| now - sample.ts <= long_window);
        let health = sensor_health();
        let runtime_degraded = self.runtime_error.is_some()
            || self.ledger.pending_control.is_some()
            || !health.sensor_ok();
        let mut assessment = assess_pressure(
            &self.platform,
            &snapshot,
            &pressure,
            &self.system_history,
            self.tick_s,
            self.leak_window_s,
            runtime_degraded,
        );
        apply_cli_hard_cap(
            &mut assessment,
            &initial_tracked,
            self.cli_hard_cap_mb,
            !health.errors.contains_key("processes"),
        );
        let previous_action = self.ledger.last_assessment_action;
        let action_changed = self.stabilize_assessment(&mut assessment, now);
        if let Some((previous, current)) = utilization_transition {
            self.record_transition(
                previous,
                current,
                snapshot.available_mb,
                pressure.some_avg10,
                &assessment,
            );
        }
        if action_changed && (previous_action.is_some() || assessment.action != Action::Allow) {
            let message = pressure_action_notice(&assessment, previous_action, pressure.full_avg10);
            self.emit_event(
                "pressure-action",
                &format!("{:?}", assessment.action).to_lowercase(),
                &message,
                &format!("{:?}:{:.3}", assessment.action, self.ledger.action_since),
                Map::from_iter([
                    (
                        "severity".to_owned(),
                        Value::String(
                            if assessment.action == Action::Drain {
                                "critical"
                            } else if assessment.action == Action::Hold {
                                "warning"
                            } else {
                                "info"
                            }
                            .to_owned(),
                        ),
                    ),
                    (
                        "cause".to_owned(),
                        Value::String(
                            if assessment.cli_hard_cap_driving {
                                "cli-hard-cap"
                            } else {
                                "adaptive-pressure-assessment"
                            }
                            .to_owned(),
                        ),
                    ),
                    (
                        "action".to_owned(),
                        serde_json::to_value(assessment.action).unwrap(),
                    ),
                    ("distress".to_owned(), json!(assessment.distress)),
                    ("attribution".to_owned(), json!(assessment.attribution)),
                    ("headroom_mb".to_owned(), json!(assessment.mem_available_mb)),
                    (
                        "capacity_mb".to_owned(),
                        json!(assessment.memory_capacity_mb),
                    ),
                    ("tte_s".to_owned(), json!(assessment.time_to_exhaustion_s)),
                    (
                        "reserve_mb".to_owned(),
                        json!(assessment.automatic_reserve_mb),
                    ),
                    (
                        "new_fanout_floor_mb".to_owned(),
                        json!(assessment.new_fanout_floor_mb),
                    ),
                    ("native_state".to_owned(), json!(assessment.native_state)),
                    ("psi_full_avg10".to_owned(), json!(pressure.full_avg10)),
                    (
                        "reclaim_rate_s".to_owned(),
                        json!(assessment.reclaim_rate_s),
                    ),
                    ("swap_rate_s".to_owned(), json!(assessment.swap_rate_s)),
                ]),
            );
        }
        if health.errors.contains_key("processes") {
            let tracked = initial_tracked;
            self.update_pressure_episode(&assessment, now);
            return self.build_state(
                now,
                raw_level,
                &snapshot,
                &pressure,
                &assessment,
                &tracked,
                &[],
                &health,
            );
        }
        self.reconcile_stopped(now, &process_map);
        self.process_control_requests(now, &process_map);
        let (tracked, leaks) = self.analyze_processes(now, initial_tracked, &assessment);
        let app_logical_action = self.manage_codex_app_containment(&assessment, &tracked, now);
        let cli_logical_action =
            !app_logical_action && self.manage_cli_logical_containment(&assessment, &tracked, now);
        let logical_action = app_logical_action || cli_logical_action;
        let leak_action =
            !logical_action && self.alert_and_suspend(&leaks, &assessment, &tracked, now);
        self.manage_lead_probation(&assessment, now);
        let pressure_action = !logical_action
            && !leak_action
            && self.apply_pressure_actions(&assessment, &tracked, now);
        if !logical_action && !leak_action && !pressure_action {
            self.apply_codex_app_blind_backstop(&assessment, &tracked, now);
        }
        self.update_pressure_episode(&assessment, now);
        self.track_peer_freshness(now);
        self.build_state(
            now,
            raw_level,
            &snapshot,
            &pressure,
            &assessment,
            &tracked,
            &leaks,
            &health,
        )
    }

    /// A surviving daemon is the one component positioned to say that a previously fresh
    /// federation peer stopped publishing; silence here hid a dying peer kernel entirely.
    fn track_peer_freshness(&mut self, now: f64) {
        if !crate::topology::channel_is_host_local(&federation_dir()) {
            return;
        }
        self.track_peer_freshness_from(now, fresh_federated_states(86_400.0));
    }

    fn track_peer_freshness_from(&mut self, now: f64, peers: Vec<Value>) {
        let mut current = BTreeMap::new();
        for peer in peers {
            let Some(instance) = peer
                .get("instance")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            if instance == self.instance {
                continue;
            }
            let age = now - peer.get("ts").and_then(Value::as_f64).unwrap_or_default();
            current.insert(instance, age);
        }

        let mut changed = false;
        for (instance, age) in &current {
            if *age <= 10.0 {
                if let Some(episode) = self.ledger.federation_peer_stale_since.remove(instance) {
                    changed = true;
                    let message = format!(
                        "[Memory Supervisor] FEDERATION PEER RECOVERED\nPeer: {instance} is publishing fresh snapshots again.\nEffect: its pressure participates in shared admission once more."
                    );
                    self.emit_event(
                        "federation-peer",
                        "recovered",
                        &message,
                        &format!("peer:{instance}:{episode:.3}"),
                        Map::from_iter([
                            ("severity".to_owned(), Value::String("info".to_owned())),
                            ("peer".to_owned(), Value::String(instance.clone())),
                            (
                                "action".to_owned(),
                                Value::String("peer-recovered".to_owned()),
                            ),
                        ]),
                    );
                }
                if self
                    .ledger
                    .federation_peer_last_fresh
                    .get(instance)
                    .is_none_or(|last| now - last >= PEER_FRESH_CHECKPOINT_S)
                {
                    self.ledger
                        .federation_peer_last_fresh
                        .insert(instance.clone(), now);
                    changed = true;
                }
            } else if *age > 60.0
                && let Some(last_fresh) = self
                    .ledger
                    .federation_peer_last_fresh
                    .get(instance)
                    .copied()
            {
                changed |= self.mark_peer_stale(instance, *age, last_fresh);
            }
        }
        let missing: Vec<_> = self
            .ledger
            .federation_peer_last_fresh
            .iter()
            .filter(|(instance, _)| !current.contains_key(*instance))
            .map(|(instance, last_fresh)| (instance.clone(), *last_fresh))
            .collect();
        for (instance, last_fresh) in missing {
            let age = now - last_fresh;
            if age > 60.0 {
                changed |= self.mark_peer_stale(&instance, age, last_fresh);
            }
        }
        if changed {
            let _ = self.persist_runtime(now);
        }
    }

    fn mark_peer_stale(&mut self, instance: &str, age: f64, last_fresh: f64) -> bool {
        if self
            .ledger
            .federation_peer_stale_since
            .contains_key(instance)
        {
            return false;
        }
        self.ledger
            .federation_peer_stale_since
            .insert(instance.to_owned(), last_fresh);
        let message = format!(
            "[Memory Supervisor] FEDERATION PEER STALE\nPeer: {instance} last published {age:.0}s ago.\nEffect: its pressure no longer participates in shared admission, and that kernel's own CLI sessions may be unprotected.\nNext: verify that kernel's supervisor service; the handoff lists the restart commands."
        );
        self.emit_event(
            "federation-peer",
            "stale",
            &message,
            &format!("peer:{instance}:{last_fresh:.3}"),
            Map::from_iter([
                ("severity".to_owned(), Value::String("warning".to_owned())),
                ("peer".to_owned(), Value::String(instance.to_owned())),
                ("age_s".to_owned(), json!(age.round())),
                ("action".to_owned(), Value::String("peer-stale".to_owned())),
            ]),
        );
        true
    }

    fn recent_incidents(&self, now: f64) -> Vec<Value> {
        self.ledger
            .incidents
            .iter()
            .filter(|incident| {
                incident.get("status").and_then(Value::as_str) == Some("suspended")
                    || now - incident_updated_at(incident) <= INCIDENT_NOTICE_S
            })
            .rev()
            .take(128)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn build_state(
        &self,
        now: f64,
        raw_level: Level,
        snapshot: &crate::policy::MemorySnapshot,
        pressure: &crate::policy::NativePressure,
        assessment: &Assessment,
        tracked: &[TrackedProcess],
        leaks: &[TrackedProcess],
        health: &crate::platform::SensorHealth,
    ) -> Value {
        let mut state = serde_json::to_value(assessment)
            .unwrap()
            .as_object()
            .cloned()
            .unwrap();
        let thresholds: Map<_, _> = self
            .policy
            .values
            .iter()
            .map(|(key, value)| (key.clone(), json!(rounded(*value, 1))))
            .collect();
        let subordinate_steps: usize = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| agent.active && agent.role == "subagent")
            .map(|agent| Self::logical_steps_remaining(agent.state) as usize)
            .sum();
        let lead_steps: usize = self
            .ledger
            .logical_agents
            .values()
            .filter(|agent| agent.active && agent.role == "lead")
            .map(|agent| Self::logical_steps_remaining(agent.state) as usize)
            .sum();
        let braking_role = if subordinate_steps > 0 {
            "subagent"
        } else {
            "lead"
        };
        let next_batch = if assessment.action == Action::Drain
            && (matches!(assessment.attribution.as_str(), "agent" | "mixed")
                || assessment.cli_hard_cap_status == "exceeded")
        {
            self.logical_batch_budget(braking_role, assessment)
        } else {
            0
        };
        state.extend(Map::from_iter([
            ("schema_version".to_owned(), json!(6)),
            ("ts".to_owned(), json!(rounded(now, 1))),
            (
                "level".to_owned(),
                serde_json::to_value(self.level).unwrap(),
            ),
            (
                "utilization".to_owned(),
                serde_json::to_value(raw_level).unwrap(),
            ),
            (
                "memory_capacity_source".to_owned(),
                Value::String(snapshot.capacity_source.clone()),
            ),
            (
                "policy_mode".to_owned(),
                Value::String(self.policy.mode.clone()),
            ),
            (
                "policy_profile".to_owned(),
                Value::String(self.policy.profile.clone()),
            ),
            ("resolved_thresholds".to_owned(), Value::Object(thresholds)),
            (
                "threshold_overrides".to_owned(),
                json!(self.policy.overrides),
            ),
            ("psi_some_avg10".to_owned(), json!(pressure.some_avg10)),
            ("psi_full_avg10".to_owned(), json!(pressure.full_avg10)),
            ("platform".to_owned(), Value::String(self.platform.clone())),
            ("instance".to_owned(), Value::String(self.instance.clone())),
            ("targets".to_owned(), json!(TARGETS)),
            (
                "tracked_roots".to_owned(),
                json!(tracked.iter().filter(|item| item.via == "root").count()),
            ),
            (
                "tracked_children".to_owned(),
                json!(tracked.iter().filter(|item| item.via == "child").count()),
            ),
            (
                "tracked_total_rss_mb".to_owned(),
                json!(tracked.iter().map(|item| item.rss_mb).sum::<u64>()),
            ),
            ("tracked_processes".to_owned(), json!(tracked)),
            ("leak_suspects".to_owned(), json!(leaks)),
            (
                "runaway_verified_count".to_owned(),
                json!(
                    tracked
                        .iter()
                        .filter(|process| process.runaway_verified)
                        .count()
                ),
            ),
            ("logical_epoch".to_owned(), json!(self.ledger.logical_epoch)),
            (
                "logical_agents".to_owned(),
                serde_json::to_value(&self.ledger.logical_agents).unwrap(),
            ),
            (
                "codex_app".to_owned(),
                serde_json::to_value(&self.codex_app_snapshot).unwrap(),
            ),
            (
                "logical_restricted_count".to_owned(),
                json!(
                    self.ledger
                        .logical_agents
                        .values()
                        .filter(|agent| agent.active && agent.state != LogicalState::Active)
                        .count()
                ),
            ),
            ("logical_control_tick_s".to_owned(), json!(self.tick_s)),
            (
                "logical_last_action_at".to_owned(),
                json!(self.ledger.last_logical_action_at),
            ),
            (
                "logical_subagent_steps_remaining".to_owned(),
                json!(subordinate_steps),
            ),
            ("logical_lead_steps_remaining".to_owned(), json!(lead_steps)),
            (
                "logical_next_batch_role".to_owned(),
                Value::String(braking_role.to_owned()),
            ),
            ("logical_next_batch_steps".to_owned(), json!(next_batch)),
            ("stopped_pids".to_owned(), json!(self.ledger.stopped_pids())),
            (
                "pressure_episode_active".to_owned(),
                Value::Bool(self.ledger.pressure_episode_started_at.is_some()),
            ),
            (
                "pressure_episode_started_at".to_owned(),
                json!(self.ledger.pressure_episode_started_at),
            ),
            (
                "recent_incidents".to_owned(),
                Value::Array(self.recent_incidents(now)),
            ),
            (
                "suspend_scope".to_owned(),
                Value::String("exact-process".to_owned()),
            ),
            (
                "leak_action".to_owned(),
                Value::String(self.leak_action.clone()),
            ),
            (
                "runtime_error".to_owned(),
                self.runtime_error
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            (
                "pending_control".to_owned(),
                serde_json::to_value(&self.ledger.pending_control).unwrap(),
            ),
            (
                "probation".to_owned(),
                serde_json::to_value(&self.ledger.probation).unwrap(),
            ),
            (
                "action_since".to_owned(),
                json!(rounded(self.ledger.action_since, 3)),
            ),
            (
                "notification_events".to_owned(),
                Value::Array(
                    self.ledger
                        .notification_events
                        .iter()
                        .rev()
                        .take(64)
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect(),
                ),
            ),
            (
                "notification_error".to_owned(),
                self.notification_error
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            ("sensor_ok".to_owned(), Value::Bool(health.sensor_ok())),
            ("sensor_errors".to_owned(), json!(health.errors)),
            (
                "last_process_scan_ts".to_owned(),
                health
                    .last_process_scan_ts
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
            ),
            (
                "protection_degraded".to_owned(),
                Value::Bool(self.runtime_error.is_some() || self.ledger.pending_control.is_some()),
            ),
            (
                "control_command".to_owned(),
                Value::String("memory-supervisor".to_owned()),
            ),
        ]));
        if let Some(error) = self.config.configuration_error() {
            state.insert("configuration_error".to_owned(), Value::String(error));
        }
        Value::Object(state)
    }
}

fn admission_rank(state: &Value, is_peer: bool) -> (Level, i64) {
    let level = if is_peer {
        admission_level_for_peer(state)
    } else {
        admission_level_for_state(state)
    };
    let available = state
        .get("admission_mem_available_mb")
        .or_else(|| state.get("mem_available_mb"))
        .and_then(Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let capacity = state
        .get("admission_capacity_mb")
        .or_else(|| state.get("memory_capacity_mb"))
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let ratio = if available.is_finite() && capacity.is_finite() && capacity > 0.0 {
        available / capacity
    } else {
        f64::INFINITY
    };
    (level, -(ratio * 1_000_000.0) as i64)
}

pub fn admission_snapshot(local: &Value, peers: Option<Vec<Value>>, tick_s: f64) -> Value {
    let mut result = local.as_object().cloned().unwrap_or_default();
    let mut candidates = vec![local.clone()];
    candidates.extend(peers.unwrap_or_else(|| fresh_federated_states(10.0)));
    let mut worst_index = 0;
    for index in 1..candidates.len() {
        if admission_rank(&candidates[index], true)
            > admission_rank(&candidates[worst_index], worst_index > 0)
        {
            worst_index = index;
        }
    }
    let worst = &candidates[worst_index];
    let local_level = admission_level_for_state(local);
    let worst_level = if worst_index == 0 {
        local_level
    } else {
        admission_level_for_peer(worst)
    };
    result.insert(
        "local_utilization".to_owned(),
        local
            .get("utilization")
            .cloned()
            .unwrap_or_else(|| json!("GREEN")),
    );
    result.insert(
        "local_admission_level".to_owned(),
        serde_json::to_value(local_level).unwrap(),
    );
    result.insert(
        "local_level".to_owned(),
        serde_json::to_value(local_level).unwrap(),
    );
    result.insert(
        "admission_level".to_owned(),
        serde_json::to_value(worst_level).unwrap(),
    );
    result.insert(
        "admission_source".to_owned(),
        if worst_index == 0 {
            Value::String("local".to_owned())
        } else {
            worst
                .get("instance")
                .or_else(|| worst.get("platform"))
                .cloned()
                .unwrap_or_else(|| Value::String("peer".to_owned()))
        },
    );
    for (target, source) in [
        ("admission_mem_available_mb", "mem_available_mb"),
        ("admission_capacity_mb", "memory_capacity_mb"),
        ("admission_action", "action"),
        ("admission_distress", "distress"),
        ("admission_attribution", "attribution"),
        ("admission_action_since", "action_since"),
        ("admission_time_to_exhaustion_s", "time_to_exhaustion_s"),
        (
            "admission_time_to_recovery_reserve_s",
            "time_to_recovery_reserve_s",
        ),
    ] {
        result.insert(
            target.to_owned(),
            worst.get(source).cloned().unwrap_or(Value::Null),
        );
    }
    for key in [
        "cli_hard_cap_mb",
        "cli_memory_used_mb",
        "cli_hard_cap_remaining_mb",
        "cli_hard_cap_status",
        "cli_hard_cap_driving",
    ] {
        result.insert(
            format!("admission_{key}"),
            worst.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    result.insert(
        "recent_incidents".to_owned(),
        Value::Array(merge_federated_incidents(&candidates)),
    );
    result.insert(
        "admission_valid_until_epoch".to_owned(),
        json!((now_epoch() + (tick_s * 1.5).clamp(1.0, 5.0)) as u64),
    );
    Value::Object(result)
}

pub fn update_fast_path_lease(directory: &Path, state: &Value) {
    let safe = state.get("admission_level").and_then(Value::as_str) == Some("GREEN")
        && state
            .get("local_admission_level")
            .or_else(|| state.get("local_level"))
            .and_then(Value::as_str)
            == Some("GREEN")
        && state
            .get("stopped_pids")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        && state
            .get("recent_incidents")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        && state.get("error").is_none()
        && state.get("configuration_error").is_none()
        && state
            .get("sensor_ok")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && !state
            .get("protection_degraded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let target = directory.join("admission-green.lease");
    if !safe {
        let _ = fs::remove_file(target);
        return;
    }
    let federation = federation_dir().to_string_lossy().into_owned();
    if federation.contains(['\n', '\r']) {
        let _ = fs::remove_file(target);
        return;
    }
    let expires = state
        .get("admission_valid_until_epoch")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let _ = write_atomic_text(&target, &format!("1\n{expires}\n{federation}\n"), 0o600);
}

pub fn publish_federation(state: &Value) {
    let Some(instance) = state.get("instance").and_then(Value::as_str) else {
        return;
    };
    let _ = write_atomic_json(
        &federation_dir().join(format!("{instance}.json")),
        state,
        0o600,
        false,
    );
}

fn clear_power_publication(directory: &Path, instance: &str) {
    let _ = fs::remove_file(directory.join("admission-green.lease"));
    let _ = fs::remove_file(federation_dir().join(format!("{instance}.json")));
}

pub fn write_state(directory: &Path, state: &Value) -> io::Result<()> {
    write_atomic_json(&directory.join("state.json"), state, 0o600, true)
}

pub struct InstanceLock {
    file: File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "lock has no parent"))?;
        ensure_private_dir(parent)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(1)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: flock only uses the live file descriptor and stores no pointer.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("supervisor already owns {}", path.display()),
                ));
            }
        }
        #[cfg(windows)]
        {
            use std::ffi::c_void;
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn LockFile(
                    file: *mut c_void,
                    offset_low: u32,
                    offset_high: u32,
                    bytes_low: u32,
                    bytes_high: u32,
                ) -> i32;
            }
            // SAFETY: the handle remains owned by `file` for the lock lifetime.
            if unsafe { LockFile(file.as_raw_handle(), 0, 0, 1, 0) } == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("supervisor already owns {}", path.display()),
                ));
            }
        }
        Ok(Self { file })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: the descriptor is still live while Drop runs.
            unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
        #[cfg(windows)]
        {
            use std::ffi::c_void;
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn UnlockFile(
                    file: *mut c_void,
                    offset_low: u32,
                    offset_high: u32,
                    bytes_low: u32,
                    bytes_high: u32,
                ) -> i32;
            }
            // SAFETY: the handle is live and the range matches acquisition.
            unsafe { UnlockFile(self.file.as_raw_handle(), 0, 0, 1, 0) };
        }
    }
}

pub fn run_daemon(arguments: &[std::ffi::OsString]) -> i32 {
    let once = arguments.iter().any(|argument| argument == "--once");
    if arguments.iter().any(|argument| {
        argument != "--once" && argument != "--foreground" && argument != "--detach-console"
    }) {
        eprintln!("usage: memory-supervisor daemon [--once|--foreground|--detach-console]");
        return 2;
    }
    #[cfg(windows)]
    if arguments
        .iter()
        .any(|argument| argument == "--detach-console")
    {
        terminal::detach_private_console();
    }
    let directory = state_dir();
    if let Err(error) =
        ensure_private_dir(&directory).and_then(|()| ensure_private_dir(&directory.join("control")))
    {
        eprintln!("cannot prepare state directory: {error}");
        return 1;
    }
    let _lock = match InstanceLock::acquire(&directory.join("supervisor.lock")) {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let mut supervisor = Supervisor::new(None);
    if power_is_off() {
        clear_power_publication(&directory, &supervisor.instance);
        return 0;
    }
    loop {
        if power_is_off() {
            clear_power_publication(&directory, &supervisor.instance);
            break;
        }
        let state = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| supervisor.tick()))
            .unwrap_or_else(|_| {
                log_event(&directory, "TICK_ERROR panic in supervisor tick");
                json!({
                    "schema_version": 6,
                    "ts": rounded(now_epoch(), 1),
                    "level": "GREEN",
                    "utilization": "GREEN",
                    "distress": "unknown",
                    "attribution": "unknown",
                    "action": "hold",
                    "admission_level": "ORANGE",
                    "platform": supervisor.platform,
                    "instance": supervisor.instance,
                    "error": "panic in supervisor tick",
                    "fail_open": true,
                    "protection_degraded": true,
                })
            });
        // Local protection first: the federation directory can live on a slow or host-side
        // filesystem (WSL writes to /mnt/c), and a stall there must not stale the local
        // state/lease that every hook reads.
        let snapshot = admission_snapshot(&state, None, supervisor.tick_interval());
        if let Err(error) = write_state(&directory, &snapshot) {
            log_event(&directory, &format!("STATE_WRITE_ERROR error={error}"));
        }
        update_fast_path_lease(&directory, &snapshot);
        publish_federation(&state);
        if once {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(
            supervisor.tick_interval(),
        ));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temp_directory(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "memory-supervisor-supervisor-{label}-{}-{}",
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
    fn wsl_default_instance_names_are_unique_per_distribution() {
        assert_eq!(
            default_instance_name("wsl", Some("Debian"), "shared-host"),
            "wsl-Debian-shared-host"
        );
        assert_eq!(
            default_instance_name("wsl", Some("Ubuntu"), "shared-host"),
            "wsl-Ubuntu-shared-host"
        );
        assert_eq!(
            default_instance_name("windows", Some("Debian"), "shared-host"),
            "windows-shared-host"
        );
    }

    #[test]
    fn tick_interval_cannot_outlive_the_fresh_state_contract() {
        let root = temp_directory("tick-freshness");
        let path = root.join("config.json");
        fs::write(&path, r#"{"MEMORY_SUPERVISOR_TICK_S":5}"#).unwrap();
        let mut config = Config::load(&path);
        assert_eq!(
            numeric(
                &mut config,
                "MEMORY_SUPERVISOR_TICK_S",
                1.0,
                0.25,
                MAX_TICK_S
            ),
            5.0
        );
        assert!(config.configuration_error().is_none());

        fs::write(&path, r#"{"MEMORY_SUPERVISOR_TICK_S":60}"#).unwrap();
        let mut config = Config::load(&path);
        assert_eq!(
            numeric(
                &mut config,
                "MEMORY_SUPERVISOR_TICK_S",
                1.0,
                0.25,
                MAX_TICK_S
            ),
            1.0
        );
        assert!(
            config
                .configuration_error()
                .as_deref()
                .is_some_and(|error| error.contains("must be <= 5"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_hook_route_requires_a_fresh_receipt_from_the_current_file_version() {
        assert_eq!(
            codex_app_hook_route_status(true, true, None, Some(100.0), 110.0),
            ("CONFIGURED", false)
        );
        assert_eq!(
            codex_app_hook_route_status(true, true, Some(99.0), Some(100.0), 110.0),
            ("CONFIGURED", false),
            "a receipt from the previous hooks file cannot prove current trust"
        );
        assert_eq!(
            codex_app_hook_route_status(true, true, Some(101.0), Some(100.0), 110.0),
            ("ACTIVE", true)
        );
        assert_eq!(
            codex_app_hook_route_status(true, true, Some(101.0), Some(100.0), 230.0),
            ("STALE", false)
        );
        assert_eq!(
            codex_app_hook_route_status(false, true, Some(110.0), Some(100.0), 110.0),
            ("UNRESOLVED", false)
        );
        assert_eq!(
            codex_app_hook_route_status(true, false, Some(110.0), Some(100.0), 110.0),
            ("NEEDS ATTENTION", false)
        );
    }

    #[test]
    fn only_the_owner_registered_binary_may_repair_app_hooks() {
        let root = temp_directory("registered-binary");
        let pointer = root.join(".memory-supervisor");
        fs::create_dir(&pointer).unwrap();
        let installed = root.join("installed/memory-supervisor");
        let development = root.join("target/debug/memory-supervisor");
        fs::write(pointer.join("binary"), format!("{}\n", installed.display())).unwrap();
        assert!(installed_binary_matches_at(&installed, &root));
        assert!(!installed_binary_matches_at(&development, &root));
        fs::remove_dir_all(root).unwrap();
    }

    fn notification_channels_for_test() -> BTreeSet<String> {
        ["hook", "terminal", "os", "discord", "telegram"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn assessment_for(action: Action, attribution: &str) -> Assessment {
        let mut assessment = assess_pressure(
            "linux",
            &crate::policy::MemorySnapshot {
                available_mb: 4096,
                capacity_mb: 8192,
                capacity_source: "test".to_owned(),
            },
            &crate::policy::NativePressure::default(),
            &[],
            1.0,
            30.0,
            false,
        );
        assessment.action = action;
        assessment.admission_level = action.level();
        assessment.attribution = attribution.to_owned();
        assessment.distress = if action == Action::Drain {
            "critical"
        } else {
            "normal"
        }
        .to_owned();
        assessment.collapse_imminent = action == Action::Drain;
        assessment.native_confidence = "high".to_owned();
        assessment
    }

    fn tracked_sample(anon_mb: u64) -> TrackedProcess {
        TrackedProcess {
            pid: 42,
            name: "provider-helper".to_owned(),
            rss_mb: anon_mb,
            anon_mb,
            via: "child".to_owned(),
            role: "worker".to_owned(),
            root_pid: 10,
            tree_rss_mb: anon_mb,
            tree_anon_mb: anon_mb,
            identity: "42:start".to_owned(),
            identity_reliable: true,
            start_token: "start".to_owned(),
            terminal: String::new(),
            terminal_identity: String::new(),
            slope_mb_s: 0.0,
            monotonicity: 0.0,
            strong_leak: false,
            recent_slope_mb_s: 0.0,
            growth_delta_mb: 0.0,
            observation_span_s: 0.0,
            runaway_verified: false,
            runaway: None,
        }
    }

    #[cfg(unix)]
    struct Canary(std::process::Child);

    #[cfg(unix)]
    impl Canary {
        fn spawn() -> Self {
            use std::os::unix::process::CommandExt;
            use std::process::{Command, Stdio};
            let mut command = Command::new("/bin/sleep");
            command
                .arg("60")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            Self(command.spawn().unwrap())
        }

        #[cfg(target_os = "linux")]
        fn spawn_app_server() -> Self {
            use std::os::unix::process::CommandExt;
            use std::process::{Command, Stdio};
            let mut command = Command::new("bash");
            command
                .args([
                    "-c",
                    r#"exec -a codex bash -c 'while :; do sleep 60; done' app-server"#,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            Self(command.spawn().unwrap())
        }

        fn pid(&self) -> u32 {
            self.0.id()
        }
    }

    #[cfg(unix)]
    impl Drop for Canary {
        fn drop(&mut self) {
            let pid = self.0.id();
            let _ = resume_process(&platform_name(), pid);
            if let Ok(process_group) = i32::try_from(pid) {
                // Every canary starts a new session. Kill the complete test-only process group so
                // the shell-backed App Server fixture cannot leave its sleeping child behind.
                unsafe {
                    libc::kill(-process_group, libc::SIGCONT);
                    libc::kill(-process_group, libc::SIGKILL);
                }
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[cfg(unix)]
    fn canary_process(platform: &str, pid: u32) -> ProcessInfo {
        for _ in 0..100 {
            if let Some(process) = process_by_pid(platform, pid)
                && !process.start_token.is_empty()
            {
                return process;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("canary process {pid} did not become observable");
    }

    #[cfg(target_os = "linux")]
    fn canary_app_server_process(platform: &str, pid: u32) -> ProcessInfo {
        for _ in 0..100 {
            if let Some(process) = process_by_pid(platform, pid)
                && !process.start_token.is_empty()
                && crate::codex_app::is_codex_app_server(&process)
            {
                return process;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("canary App Server process {pid} did not become observable");
    }

    #[cfg(unix)]
    fn canary_candidate(process: &ProcessInfo, role: &str, slope: f64) -> TrackedProcess {
        TrackedProcess {
            pid: process.pid,
            name: process.name.clone(),
            rss_mb: process.rss_mb,
            anon_mb: process.anon_mb,
            via: "child".to_owned(),
            role: role.to_owned(),
            root_pid: process.ppid,
            tree_rss_mb: process.rss_mb,
            tree_anon_mb: process.anon_mb,
            identity: process_identity(process),
            identity_reliable: true,
            start_token: process.start_token.clone(),
            terminal: process.terminal.clone(),
            terminal_identity: process.terminal_identity.clone(),
            slope_mb_s: slope,
            monotonicity: 1.0,
            strong_leak: true,
            recent_slope_mb_s: slope,
            growth_delta_mb: 1024.0,
            observation_span_s: 30.0,
            runaway_verified: true,
            runaway: None,
        }
    }

    fn strict_analysis_assessment() -> Assessment {
        let mut assessment = assessment_for(Action::Drain, "agent");
        assessment.mem_available_mb = 900;
        assessment.automatic_reserve_mb = 500.0;
        assessment.headroom_fall_mb_s = 80.0;
        assessment.tracked_growth_mb_s = 70.0;
        assessment.time_to_exhaustion_s = Some(10.0);
        assessment
    }

    fn seed_idle_agent(supervisor: &mut Supervisor, pid: u32, idle_since: f64) {
        let key = format!("claude:test:agent-{pid}");
        supervisor.ledger.logical_agents.insert(
            key.clone(),
            LogicalAgent {
                key,
                provider: "claude".to_owned(),
                session_id: "test".to_owned(),
                agent_id: Some(format!("agent-{pid}")),
                agent_type: "test".to_owned(),
                role: "subagent".to_owned(),
                process_pid: Some(pid),
                idle_since: Some(idle_since),
                started_at: idle_since,
                last_seen_at: idle_since,
                last_progress_at: idle_since,
                active: true,
                ..LogicalAgent::default()
            },
        );
    }

    fn verified_process(mut process: TrackedProcess, tte: f64) -> TrackedProcess {
        process.slope_mb_s = 50.0;
        process.recent_slope_mb_s = 55.0;
        process.monotonicity = 1.0;
        process.growth_delta_mb = 1500.0;
        process.observation_span_s = 30.0;
        process.strong_leak = true;
        process.runaway_verified = true;
        process.runaway = Some(crate::containment::RunawayVerdict {
            stage: "RUNAWAY_VERIFIED".to_owned(),
            gates: crate::containment::RunawayGates {
                identity: true,
                materiality: true,
                persistence: true,
                machine_corroboration: true,
                work_mismatch: true,
                causal_dominance: true,
            },
            required_growth_mb: 256.0,
            candidate_tte_s: Some(tte),
            growth_contribution: 0.8,
            headroom_share: 0.8,
            peer_outlier: true,
        });
        process
    }

    fn active_agent(key: &str, role: &str, pid: u32, started_at: f64) -> LogicalAgent {
        let (_, session_agent) = key.split_once(':').unwrap_or(("test", key));
        LogicalAgent {
            key: key.to_owned(),
            provider: "claude".to_owned(),
            session_id: "test".to_owned(),
            agent_id: (role == "subagent").then(|| session_agent.to_owned()),
            agent_type: "test".to_owned(),
            role: role.to_owned(),
            process_pid: Some(pid),
            started_at,
            last_seen_at: started_at,
            last_progress_at: started_at,
            active: true,
            ..LogicalAgent::default()
        }
    }

    #[cfg(unix)]
    fn wait_for_process_state(platform: &str, pid: u32, stopped: bool) -> String {
        for _ in 0..100 {
            let state = process_state(platform, pid);
            if state != "unknown" && state != "missing" && (state.starts_with('T') == stopped) {
                return state;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("canary {pid} did not reach expected stopped={stopped}");
    }

    #[cfg(unix)]
    fn seed_suspended_lead(supervisor: &mut Supervisor, process: &ProcessInfo, now: f64) -> String {
        let identity = process_identity(process);
        suspend_process(&supervisor.platform, process.pid).unwrap();
        wait_for_process_state(&supervisor.platform, process.pid, true);
        supervisor
            .ledger
            .mark_stopped(process.pid, identity.clone());
        supervisor.ledger.incidents.push(json!({
            "id":format!("lead-{}", process.pid),
            "status":"suspended",
            "source":supervisor.instance,
            "platform":supervisor.platform,
            "pid":process.pid,
            "identity":identity,
            "name":process.name,
            "role":"lead",
            "reason":"runaway-memory",
            "attribution":"agent",
            "distress":"critical",
            "recovery_policy":"lead-probation",
            "anon_mb":process.anon_mb,
            "slope_mb_s":10.0,
            "updated_at":now,
            "suspended_at":now
        }));
        assert!(supervisor.persist_runtime(now));
        identity
    }

    #[test]
    fn admission_uses_worst_peer_and_active_recovery_holds() {
        let local = json!({
            "level":"GREEN", "utilization":"GREEN", "action":"allow",
            "mem_available_mb":3000, "memory_capacity_mb":8000,
            "stopped_pids":[], "recent_incidents":[], "sensor_ok":true
        });
        let peer = json!({
            "level":"ORANGE", "action":"hold", "instance":"windows-host",
            "mem_available_mb":1000, "memory_capacity_mb":16000,
            "recent_incidents":[]
        });
        let state = admission_snapshot(&local, Some(vec![peer]), 1.0);
        assert_eq!(state["admission_level"], "ORANGE");
        assert_eq!(state["admission_source"], "windows-host");

        let recovering = json!({"level":"GREEN", "action":"allow", "probation":{"pid":42}});
        assert_eq!(admission_level_for_state(&recovering), Level::Green);

        let cap_driven_peer = json!({
            "level":"GREEN", "action":"hold", "adaptive_action":"allow",
            "cli_hard_cap_driving":true, "instance":"capped-host",
            "mem_available_mb":100, "memory_capacity_mb":16000,
            "recent_incidents":[]
        });
        let uncoupled = admission_snapshot(&local, Some(vec![cap_driven_peer]), 1.0);
        assert_eq!(uncoupled["admission_level"], "GREEN");
    }

    #[test]
    fn high_memory_process_needs_growth_and_pressure_before_pause_eligibility() {
        let mut supervisor = Supervisor::new(Some(
            std::env::temp_dir()
                .join(format!("memory-supervisor-analysis-{}", std::process::id()))
                .join("runtime.json"),
        ));
        let process = tracked_sample(5000);
        let assessment = strict_analysis_assessment();
        for timestamp in 0..5 {
            let (tracked, leaks) =
                supervisor.analyze_processes(timestamp as f64, vec![process.clone()], &assessment);
            assert_eq!(tracked[0].slope_mb_s, 0.0);
            assert!(leaks.is_empty());
        }
    }

    #[test]
    fn transient_small_burst_never_becomes_a_process_pause_candidate() {
        let root = temp_directory("transient-burst");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let assessment = strict_analysis_assessment();
        let samples = [50, 120, 190, 260, 334];
        for (timestamp, anon_mb) in samples.into_iter().enumerate() {
            let (tracked, _leaks) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(anon_mb)],
                &assessment,
            );
            assert!(!tracked[0].strong_leak);
        }
        for timestamp in 5..=35 {
            let (tracked, _leaks) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(334)],
                &assessment,
            );
            assert!(!tracked[0].strong_leak);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sustained_material_growth_must_cross_the_adaptive_stop_size() {
        let root = temp_directory("sustained-material-growth");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let stop_mb = supervisor.policy.value("MEMORY_SUPERVISOR_LEAK_STOP_MB");
        seed_idle_agent(&mut supervisor, 42, 0.0);
        let assessment = strict_analysis_assessment();
        let mut became_strong_at = None;
        for timestamp in 0..=30 {
            let anon_mb = 3500 + timestamp * 70;
            let (tracked, leaks) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(anon_mb as u64)],
                &assessment,
            );
            if tracked[0].strong_leak {
                assert!(tracked[0].anon_mb as f64 > stop_mb);
                assert!(!leaks.is_empty());
                became_strong_at = Some(timestamp);
                break;
            }
        }
        assert!(became_strong_at.is_some_and(|timestamp| timestamp >= 24));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn derivative_alone_has_no_pause_authority_even_during_global_distress() {
        let root = temp_directory("derivative-no-authority");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let assessment = strict_analysis_assessment();
        let mut leaks = Vec::new();
        for timestamp in 0..5 {
            let (_, current) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(50 + timestamp * 71)],
                &assessment,
            );
            leaks = current;
        }
        assert!(leaks.iter().all(|process| !process.runaway_verified));
        let tracked = leaks.clone();
        assert!(!supervisor.alert_and_suspend(&leaks, &assessment, &tracked, 5.0,));
        assert!(supervisor.ledger.stopped.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn useful_large_growth_is_not_called_a_runaway() {
        let root = temp_directory("useful-large-growth");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.ledger.logical_agents.insert(
            "claude:test:agent-42".to_owned(),
            active_agent("claude:test:agent-42", "subagent", 42, 0.0),
        );
        let assessment = strict_analysis_assessment();
        let mut last = tracked_sample(0);
        for timestamp in 0..=40 {
            let (tracked, suspects) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(3500 + timestamp * 100)],
                &assessment,
            );
            last = tracked[0].clone();
            assert!(suspects.is_empty());
        }
        assert!(last.anon_mb > 7000);
        assert_eq!(last.runaway.unwrap().stage, "GROWTH_OBSERVED");
        assert!(!last.runaway_verified);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_abnormal_evidence_requires_fresh_reconfirmation() {
        let root = temp_directory("strict-reconfirmation");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        seed_idle_agent(&mut supervisor, 42, 0.0);
        let assessment = strict_analysis_assessment();
        let mut first_complete = None;
        let mut verified = None;
        for timestamp in 0..=40 {
            let (tracked, _) = supervisor.analyze_processes(
                timestamp as f64,
                vec![tracked_sample(3500 + timestamp * 100)],
                &assessment,
            );
            let process = &tracked[0];
            if process
                .runaway
                .as_ref()
                .is_some_and(|verdict| verdict.gates.complete())
                && first_complete.is_none()
            {
                first_complete = Some(timestamp);
                assert!(!process.runaway_verified);
            }
            if process.runaway_verified {
                verified = Some(timestamp);
                break;
            }
        }
        assert!(first_complete.is_some());
        assert!(verified.unwrap() as f64 - first_complete.unwrap() as f64 >= assessment.reaction_s);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logical_containment_prioritizes_verified_abnormality_over_recency() {
        let root = temp_directory("logical-priority");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let old_key = "claude:test:old";
        let new_key = "claude:test:new";
        supervisor.ledger.logical_agents.insert(
            old_key.to_owned(),
            active_agent(old_key, "subagent", 42, 1.0),
        );
        supervisor.ledger.logical_agents.insert(
            new_key.to_owned(),
            active_agent(new_key, "subagent", 43, 9.0),
        );
        supervisor.ledger.action_since = 0.0;
        let abnormal = verified_process(tracked_sample(6000), 8.0);
        let mut normal = tracked_sample(5000);
        normal.pid = 43;
        normal.identity = "43:start".to_owned();
        let assessment = strict_analysis_assessment();
        assert!(supervisor.manage_logical_containment(&assessment, &[abnormal, normal], 10.0));
        assert_eq!(
            supervisor.ledger.logical_agents[old_key].state,
            LogicalState::LightWorkOnly
        );
        assert_eq!(
            supervisor.ledger.logical_agents[new_key].state,
            LogicalState::Active
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adaptive_stopping_distance_scales_from_eight_workers_to_hundreds_of_leads() {
        let exercise = |role: &str, count: u32, label: &str| {
            let root = temp_directory(label);
            let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
            for index in 0..count {
                let key = format!("claude:test:{role}-{index:03}");
                supervisor.ledger.logical_agents.insert(
                    key.clone(),
                    active_agent(&key, role, 10_000 + index, index as f64),
                );
            }
            supervisor.ledger.action_since = 10.0;
            let mut assessment = assessment_for(Action::Drain, "agent");
            for tick in 0..5 {
                assessment.time_to_recovery_reserve_s = Some((5 - tick) as f64);
                let before: usize = supervisor
                    .ledger
                    .logical_agents
                    .values()
                    .map(|agent| Supervisor::logical_steps_remaining(agent.state) as usize)
                    .sum();
                let ticks_left = 5 - tick as usize;
                let expected_budget = before.div_ceil(ticks_left);
                assert!(supervisor.manage_logical_containment(
                    &assessment,
                    &[],
                    10.0 + tick as f64
                ));
                let after: usize = supervisor
                    .ledger
                    .logical_agents
                    .values()
                    .map(|agent| Supervisor::logical_steps_remaining(agent.state) as usize)
                    .sum();
                assert_eq!(before - after, expected_budget);
            }
            assert!(
                supervisor
                    .ledger
                    .logical_agents
                    .values()
                    .all(|agent| agent.state == LogicalState::HandoffOnly)
            );
            assert_eq!(supervisor.ledger.logical_epoch, 5);
            fs::remove_dir_all(root).unwrap();
        };

        exercise("subagent", 8, "adaptive-eight-workers");
        exercise("lead", 300, "adaptive-three-hundred-leads");
    }

    #[test]
    fn sustained_recovery_reopens_in_batches_within_the_recovery_deadline() {
        let root = temp_directory("batched-relax");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        for index in 0..8 {
            let key = format!("claude:test:worker-{index:03}");
            supervisor.ledger.logical_agents.insert(
                key.clone(),
                active_agent(&key, "subagent", 20_000 + index, index as f64),
            );
        }
        supervisor.ledger.action_since = 10.0;
        let mut danger = assessment_for(Action::Drain, "agent");
        for tick in 0..5 {
            danger.time_to_recovery_reserve_s = Some((5 - tick) as f64);
            assert!(supervisor.manage_logical_containment(&danger, &[], 10.0 + tick as f64));
        }
        assert!(
            supervisor
                .ledger
                .logical_agents
                .values()
                .all(|agent| agent.state == LogicalState::HandoffOnly)
        );

        let safe = assessment_for(Action::Allow, "unknown");
        let reaction = safe.reaction_s;
        assert!(!supervisor.manage_logical_containment(&safe, &[], 100.0));
        let mut now = 100.0 + reaction * 2.0;
        let mut calls = 0usize;
        while supervisor
            .ledger
            .logical_agents
            .values()
            .any(|agent| agent.state != LogicalState::Active)
        {
            assert!(
                calls <= (reaction * 2.0) as usize + 1,
                "reopening exceeded the recovery deadline"
            );
            supervisor.manage_logical_containment(&safe, &[], now);
            now += 1.0;
            calls += 1;
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hold_is_one_pressure_episode_with_one_start_and_one_recovery() {
        let root = temp_directory("hold-episode-notify");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let hold = assessment_for(Action::Hold, "unknown");

        supervisor.update_pressure_episode(&hold, 10.0);
        supervisor.update_pressure_episode(&hold, 11.0);
        assert_eq!(supervisor.ledger.pressure_episode_started_at, Some(10.0));

        let observe = assessment_for(Action::Observe, "unknown");
        supervisor.update_pressure_episode(&observe, 20.0);
        assert!(supervisor.ledger.pressure_episode_started_at.is_none());

        let notified: Vec<_> = pending_events(&root, &BTreeSet::new())
            .into_iter()
            .filter(event_should_notify)
            .collect();
        assert_eq!(notified.len(), 2);
        assert_eq!(notified[0]["status"], "active");
        assert_eq!(notified[1]["status"], "recovered");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ample_critical_observation_sends_no_protection_alert() {
        let root = temp_directory("critical-observation-no-alert");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let mut observation = assessment_for(Action::Observe, "unknown");
        observation.mem_available_mb = 8854;
        observation.memory_capacity_mb = 9945;
        observation.distress = "critical".to_owned();
        observation.time_to_recovery_reserve_s = Some(1380.8);
        observation.trajectory_confirmed = false;
        observation.collapse_imminent = false;

        supervisor.update_pressure_episode(&observation, 10.0);

        assert!(supervisor.ledger.pressure_episode_started_at.is_none());
        assert!(pending_events(&root, &BTreeSet::new()).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_cushion_is_one_pressure_episode_not_a_second_remote_alert() {
        let root = temp_directory("app-cushion-one-alert");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        assert!(supervisor.set_codex_app_surface_gate(
            true,
            "calculated App stopping distance reached",
            10.0,
        ));
        let observation = assessment_for(Action::Observe, "agent");
        supervisor.update_pressure_episode(&observation, 10.0);

        let events = pending_events(&root, &BTreeSet::new());
        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| event_should_notify(event))
                .count(),
            1
        );
        assert!(events.iter().any(|event| {
            event["type"] == "codex-app-surface-gate" && event["importance"] == "detail"
        }));
        assert!(
            events.iter().any(|event| {
                event["type"] == "pressure-episode" && event["status"] == "active"
            })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_pressure_episode_edge_recovers_both_crash_windows() {
        let root = temp_directory("episode-durable-outbox");
        let runtime = root.join("runtime.json");
        let mut supervisor = Supervisor::new(Some(runtime.clone()));
        let event = supervisor.notification_event(
            "pressure-episode",
            "active",
            "[Memory Supervisor] MEMORY PROTECTION ACTIVE",
            "episode:10.000",
            Map::from_iter([(
                "cause".to_owned(),
                Value::String("pressure-episode-edge".to_owned()),
            )]),
        );
        let event_id = event["event_id"].as_str().unwrap().to_owned();
        supervisor.ledger.pressure_episode_started_at = Some(10.0);
        supervisor.ledger.pending_pressure_episode_event = Some(event.clone());
        assert!(supervisor.persist_runtime(10.0));
        assert!(pending_events(&root, &BTreeSet::new()).is_empty());

        let danger = assessment_for(Action::Drain, "agent");
        let mut before_queue_restart = Supervisor::new(Some(runtime.clone()));
        before_queue_restart.update_pressure_episode(&danger, 11.0);
        assert!(
            before_queue_restart
                .ledger
                .pending_pressure_episode_event
                .is_none()
        );
        let queued = pending_events(&root, &BTreeSet::new());
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0]["event_id"], event_id);

        before_queue_restart.ledger.pending_pressure_episode_event = Some(event);
        assert!(before_queue_restart.persist_runtime(12.0));
        let mut after_queue_restart = Supervisor::new(Some(runtime.clone()));
        after_queue_restart.update_pressure_episode(&danger, 13.0);
        assert!(
            after_queue_restart
                .ledger
                .pending_pressure_episode_event
                .is_none()
        );
        assert_eq!(pending_events(&root, &BTreeSet::new()).len(), 1);
        let (persisted, error) = RuntimeLedger::load(&runtime, &after_queue_restart.instance, 14.0);
        assert!(error.is_none());
        assert!(persisted.pending_pressure_episode_event.is_none());
        assert_eq!(persisted.pressure_episode_started_at, Some(10.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn federation_peer_loss_survives_restart_and_realerts_after_recovery() {
        let root = temp_directory("federation-peer-durable");
        let runtime = root.join("runtime.json");
        let peer = |timestamp| json!({"instance":"peer-a","ts":timestamp,"level":"GREEN"});

        let mut supervisor = Supervisor::new(Some(runtime.clone()));
        supervisor.track_peer_freshness_from(100.0, vec![peer(100.0)]);
        assert_eq!(
            supervisor.ledger.federation_peer_last_fresh["peer-a"],
            100.0
        );

        let mut restarted = Supervisor::new(Some(runtime.clone()));
        restarted.track_peer_freshness_from(161.0, Vec::new());
        assert_eq!(
            restarted.ledger.federation_peer_stale_since["peer-a"],
            100.0
        );
        let first_stale_id = pending_events(&root, &BTreeSet::new())
            .into_iter()
            .find(|event| event["status"] == "stale")
            .unwrap()["event_id"]
            .as_str()
            .unwrap()
            .to_owned();

        let mut recovered = Supervisor::new(Some(runtime.clone()));
        recovered.track_peer_freshness_from(162.0, vec![peer(162.0)]);
        assert!(recovered.ledger.federation_peer_stale_since.is_empty());
        assert_eq!(recovered.ledger.federation_peer_last_fresh["peer-a"], 162.0);

        let mut failed_again = Supervisor::new(Some(runtime));
        failed_again.track_peer_freshness_from(223.0, Vec::new());
        let events = pending_events(&root, &BTreeSet::new());
        assert_eq!(
            events
                .iter()
                .filter(|event| event["status"] == "stale")
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["status"] == "recovered")
                .count(),
            1
        );
        assert!(events.iter().any(|event| {
            event["status"] == "stale"
                && event["event_id"]
                    .as_str()
                    .is_some_and(|id| id != first_stale_id)
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pressure_episode_coalesces_cross_emitter_actions_to_start_and_recovery() {
        let root = temp_directory("edge-coalesced-notify");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        for index in 0..8 {
            let key = format!("claude:test:worker-{index:03}");
            supervisor.ledger.logical_agents.insert(
                key.clone(),
                active_agent(&key, "subagent", 30_000 + index, index as f64),
            );
        }
        supervisor.ledger.action_since = 10.0;
        let mut danger = assessment_for(Action::Drain, "agent");
        supervisor.emit_event(
            "pressure-action",
            "drain",
            "drain",
            "drain:10",
            Map::from_iter([(
                "importance".to_owned(),
                Value::String("important".to_owned()),
            )]),
        );
        for tick in 0..5 {
            danger.time_to_recovery_reserve_s = Some((5 - tick) as f64);
            let now = 10.0 + tick as f64;
            supervisor.manage_logical_containment(&danger, &[], now);
            if tick < 3 {
                supervisor.emit_event(
                    "process-pause",
                    "suspended",
                    "paused",
                    &format!("pid:{}", 40_000 + tick),
                    Map::from_iter([
                        ("pid".to_owned(), json!(40_000 + tick)),
                        (
                            "importance".to_owned(),
                            Value::String("important".to_owned()),
                        ),
                    ]),
                );
            }
            supervisor.update_pressure_episode(&danger, now);
        }
        supervisor.emit_event(
            "process-control",
            "resumed",
            "manually resumed",
            "resume:40000",
            Map::from_iter([(
                "importance".to_owned(),
                Value::String("important".to_owned()),
            )]),
        );
        assert!(
            supervisor
                .ledger
                .logical_agents
                .values()
                .all(|agent| agent.state == LogicalState::HandoffOnly)
        );
        let safe = assessment_for(Action::Allow, "unknown");
        let reaction = safe.reaction_s;
        supervisor.emit_event(
            "pressure-action",
            "observe",
            "recovered admission",
            "observe:100",
            Map::from_iter([(
                "importance".to_owned(),
                Value::String("important".to_owned()),
            )]),
        );
        assert!(!supervisor.manage_logical_containment(&safe, &[], 100.0));
        supervisor.update_pressure_episode(&safe, 100.0);
        let mut now = 100.0 + reaction * 2.0;
        while supervisor
            .ledger
            .logical_agents
            .values()
            .any(|agent| agent.state != LogicalState::Active)
        {
            supervisor.manage_logical_containment(&safe, &[], now);
            supervisor.update_pressure_episode(&safe, now);
            now += 1.0;
        }

        let events = pending_events(&root, &BTreeSet::new());
        let components: Vec<_> = events
            .iter()
            .filter(|event| {
                matches!(
                    event["type"].as_str(),
                    Some(
                        "pressure-action"
                            | "logical-containment"
                            | "process-pause"
                            | "process-control"
                    )
                )
            })
            .collect();
        assert!(
            components.len() >= 8,
            "the regression must reproduce the cross-emitter components"
        );
        assert!(
            components
                .iter()
                .all(|event| !event_should_notify(event) && event["importance"] == "detail"),
            "component actions stay in the ledger but do not notify"
        );
        let notified: Vec<_> = events
            .iter()
            .filter(|event| event_should_notify(event))
            .collect();
        assert_eq!(
            notified.len(),
            2,
            "one pressure episode may notify only on start and full recovery"
        );
        assert_eq!(
            notified
                .iter()
                .filter(|event| event["type"] == "pressure-episode")
                .count(),
            2
        );
        assert_eq!(
            notified
                .iter()
                .filter(|event| event["status"] == "active")
                .count(),
            1,
            "protection onset notifies exactly once"
        );
        assert_eq!(
            notified
                .iter()
                .filter(|event| event["status"] == "recovered")
                .count(),
            1,
            "full recovery notifies exactly once"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_prunes_ended_sessions_but_keeps_restricted_paused_and_recent() {
        let root = temp_directory("roster-prune");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        // Old, silent, ACTIVE (an ended or dead session) -> pruned.
        supervisor.ledger.logical_agents.insert(
            "claude:s:ended".to_owned(),
            active_agent("claude:s:ended", "subagent", 100, 0.0),
        );
        // Seen within the retention window -> kept.
        supervisor.ledger.logical_agents.insert(
            "claude:s:recent".to_owned(),
            active_agent("claude:s:recent", "subagent", 101, 9000.0),
        );
        // Old but still restricted (non-ACTIVE) -> kept.
        let mut restricted = active_agent("claude:s:restricted", "subagent", 102, 0.0);
        restricted.state = LogicalState::LightWorkOnly;
        supervisor
            .ledger
            .logical_agents
            .insert("claude:s:restricted".to_owned(), restricted);
        // Old and ACTIVE, but its PID is currently paused -> kept.
        supervisor.ledger.logical_agents.insert(
            "claude:s:paused".to_owned(),
            active_agent("claude:s:paused", "subagent", 103, 0.0),
        );
        supervisor
            .ledger
            .stopped
            .insert("103".to_owned(), "103:token".to_owned());

        supervisor.last_cleanup = 0.0;
        supervisor.cleanup_artifacts(10_000.0);

        let present = |key: &str| supervisor.ledger.logical_agents.contains_key(key);
        assert!(
            !present("claude:s:ended"),
            "an ended silent session must prune"
        );
        assert!(
            present("claude:s:recent"),
            "a recently seen session is kept"
        );
        assert!(
            present("claude:s:restricted"),
            "a restricted agent is never pruned"
        );
        assert!(
            present("claude:s:paused"),
            "a paused PID's agent is never pruned"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn attributed_collapse_can_pause_a_growing_support_process_without_a_cap() {
        let root = temp_directory("support-pause");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let candidate = canary_candidate(&process, "support", 2.0);
        let danger = assessment_for(Action::Drain, "agent");
        assert_ne!(danger.cli_hard_cap_status, "exceeded");
        let tracked = [candidate];
        supervisor.apply_pressure_actions(&danger, &tracked, 100.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        assert_eq!(
            supervisor.ledger.stopped_identity(canary.pid()),
            Some(tracked[0].identity.as_str())
        );
        resume_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_lifecycle_does_not_advance_a_user_visible_control_epoch() {
        let root = temp_directory("lifecycle-is-not-control");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let payload = json!({"session_id":"s1","agent_id":"a1","agent_type":"worker"});
        let start = HookObservation::from_payload(
            "start".to_owned(),
            1.0,
            "claude",
            "SubagentStart",
            &payload,
            Some(42),
            false,
        );
        assert!(supervisor.apply_hook_observation(start));
        assert_eq!(supervisor.ledger.logical_epoch, 0);

        let stop = HookObservation::from_payload(
            "stop".to_owned(),
            2.0,
            "claude",
            "SubagentStop",
            &payload,
            Some(42),
            false,
        );
        assert!(supervisor.apply_hook_observation(stop.clone()));
        assert_eq!(supervisor.ledger.logical_epoch, 0);

        let key = "claude:s1:a1";
        let agent = supervisor.ledger.logical_agents.get_mut(key).unwrap();
        agent.active = true;
        agent.state = LogicalState::NoExpansion;
        let mut restricted_stop = stop;
        restricted_stop.id = "restricted-stop".to_owned();
        restricted_stop.observed_at = 3.0;
        assert!(supervisor.apply_hook_observation(restricted_stop));
        assert_eq!(supervisor.ledger.logical_epoch, 1);
        assert_eq!(
            supervisor.ledger.logical_agents[key].state,
            LogicalState::Active
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn blocked_subagent_lifecycle_preserves_the_supervisor_reason() {
        let root = temp_directory("blocked-subagent-lifecycle");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let payload = json!({
            "session_id":"s1", "agent_id":"a1", "agent_type":"worker",
            "tool_name":"Bash", "tool_input":{"command":"cargo test"}
        });
        let start = HookObservation::from_payload(
            "start".to_owned(),
            1.0,
            "claude",
            "SubagentStart",
            &payload,
            Some(42),
            false,
        );
        assert!(supervisor.apply_hook_observation(start));
        let agent = supervisor
            .ledger
            .logical_agents
            .get_mut("claude:s1:a1")
            .unwrap();
        agent.state = LogicalState::HandoffOnly;
        agent.epoch = 7;
        agent.reason = "attributed memory boundary risk".to_owned();

        let mut blocked = HookObservation::from_payload(
            "blocked".to_owned(),
            2.0,
            "claude",
            "PreToolUse",
            &payload,
            Some(42),
            true,
        );
        blocked.block_reason = Some("HANDOFF_ONLY denied future work".to_owned());
        assert!(supervisor.apply_hook_observation(blocked));

        let stop = HookObservation::from_payload(
            "stop".to_owned(),
            3.0,
            "claude",
            "SubagentStop",
            &payload,
            Some(42),
            false,
        );
        assert!(supervisor.apply_hook_observation(stop));
        let agent = &supervisor.ledger.logical_agents["claude:s1:a1"];
        assert_eq!(agent.last_blocked_tool.as_deref(), Some("bash"));
        assert_eq!(agent.last_blocked_epoch, Some(7));
        assert_eq!(
            agent.last_blocked_reason.as_deref(),
            Some("HANDOFF_ONLY denied future work")
        );
        assert!(
            agent
                .reason
                .contains("after memory-supervisor blocked bash")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_pressure_and_safe_high_use_do_not_cushion_existing_agents() {
        let root = temp_directory("logical-no-false-throttle");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = "claude:test:worker";
        supervisor
            .ledger
            .logical_agents
            .insert(key.to_owned(), active_agent(key, "subagent", 42, 1.0));
        supervisor.ledger.action_since = 0.0;
        let external = assessment_for(Action::Drain, "external");
        assert!(!supervisor.manage_logical_containment(&external, &[], 10.0));
        assert_eq!(
            supervisor.ledger.logical_agents[key].state,
            LogicalState::Active
        );
        let mut safe = assessment_for(Action::Allow, "unknown");
        safe.mem_available_mb = 100;
        safe.memory_capacity_mb = 128;
        assert!(!supervisor.manage_logical_containment(&safe, &[], 11.0));
        assert_eq!(
            supervisor.ledger.logical_agents[key].state,
            LogicalState::Active
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_verified_lead_override_precedes_unrelated_newer_subagent() {
        let root = temp_directory("lead-direct-override");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let lead_key = "claude:test:root";
        let child_key = "claude:test:new-child";
        supervisor
            .ledger
            .logical_agents
            .insert(lead_key.to_owned(), active_agent(lead_key, "lead", 42, 1.0));
        supervisor.ledger.logical_agents.insert(
            child_key.to_owned(),
            active_agent(child_key, "subagent", 43, 9.0),
        );
        supervisor.ledger.action_since = 0.0;
        let mut lead = verified_process(tracked_sample(6000), 4.0);
        lead.role = "lead".to_owned();
        let mut child = tracked_sample(2000);
        child.pid = 43;
        child.identity = "43:start".to_owned();
        let assessment = strict_analysis_assessment();
        assert!(supervisor.manage_logical_containment(&assessment, &[lead, child], 10.0));
        assert_eq!(
            supervisor.ledger.logical_agents[lead_key].state,
            LogicalState::NoExpansion
        );
        assert_eq!(
            supervisor.ledger.logical_agents[child_key].state,
            LogicalState::Active
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_process_risk_scales_by_time_to_reserve_not_machine_size_ratio() {
        let root = temp_directory("time-to-reserve");
        let supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let mut process = tracked_sample(25_000);
        process.slope_mb_s = 50.0;
        process.strong_leak = true;

        let mut large_machine = assessment_for(Action::Observe, "agent");
        large_machine.mem_available_mb = 75_000;
        large_machine.memory_capacity_mb = 102_400;
        large_machine.automatic_reserve_mb = 1024.0;
        large_machine.distress = "elevated".to_owned();
        large_machine.collapse_imminent = false;
        large_machine.time_to_exhaustion_s = None;
        assert!(!supervisor.direct_process_risk(&process, &large_machine));

        let mut near_exhaustion = large_machine;
        near_exhaustion.mem_available_mb = 6000;
        near_exhaustion.automatic_reserve_mb = 1024.0;
        assert!(!supervisor.direct_process_risk(&process, &near_exhaustion));
        near_exhaustion.action = Action::Hold;
        near_exhaustion.admission_level = Level::Orange;
        assert!(supervisor.direct_process_risk(&process, &near_exhaustion));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_notices_keep_evidence_actions_and_estimates_separate() {
        let mut assessment = assessment_for(Action::Hold, "unknown");
        assessment.time_to_exhaustion_s = None;
        assessment.mem_available_mb = 4096;
        assessment.memory_capacity_mb = 8192;
        let pressure = pressure_action_notice(&assessment, None, 0.0);
        assert!(pressure.contains("NEW FAN-OUT HELD"));
        assert!(pressure.contains("Only new fan-out is blocked"));
        assert!(!pressure.contains("Some("));
        assert!(!pressure.contains("None"));

        let incident = json!({
            "pid":42, "name":"provider-helper", "role":"support",
            "reason":"runaway-memory", "anon_mb":5200, "slope_mb_s":50.0,
            "observation_window_s":30.0, "process_time_to_reserve_s":65.0,
            "attribution":"agent", "recovery_policy":"lead-or-owner"
        });
        let pause = incident_notice(&incident, "suspended", "");
        assert!(pause.contains("direct process evidence"));
        assert!(pause.contains("Attribution estimate:"));
        assert!(pause.contains("projected time to the recovery reserve was 65s"));
        assert!(!pause.contains("confirmed"));
    }

    #[test]
    fn notification_dispatch_waits_for_a_durable_ledger() {
        let root = temp_directory("notification-failure");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let event = make_event(
            "process-pause",
            "suspended",
            "pause notice",
            "linux-test",
            "incident-1",
            Map::new(),
        );
        let event_id = value_string(&event, "event_id");
        assert!(queue_event(&root, &event, &BTreeSet::new()).unwrap());

        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();
        supervisor.runtime_path = blocker.join("runtime.json");
        let calls = Cell::new(0);
        supervisor.drain_notification_events_with(
            now_epoch(),
            &notification_channels_for_test(),
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(calls.get(), 0);
        assert!(
            root.join("notification-events/pending")
                .join(format!("{event_id}.json"))
                .is_file()
        );
        assert!(supervisor.ledger.notification_events.is_empty());
        assert_eq!(
            supervisor.notification_error.as_deref(),
            Some("notification ledger could not be persisted")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn durable_notification_results_acknowledgements_and_timeouts_are_merged() {
        let root = temp_directory("notification-success");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let now = now_epoch();
        let event = make_event(
            "process-pause",
            "suspended",
            "pause notice",
            "linux-test",
            "incident-2",
            Map::new(),
        );
        let event_id = value_string(&event, "event_id");
        assert!(queue_event(&root, &event, &BTreeSet::new()).unwrap());
        let calls = Cell::new(0);
        supervisor.drain_notification_events_with(
            now,
            &notification_channels_for_test(),
            |stored, result_path| {
                calls.set(calls.get() + 1);
                let runtime: Value =
                    serde_json::from_slice(&fs::read(root.join("runtime.json")).unwrap()).unwrap();
                assert!(
                    runtime["notification_events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item.get("event_id").and_then(Value::as_str)
                            == stored.get("event_id").and_then(Value::as_str))
                );
                write_atomic_json(
                    result_path,
                    &json!({
                        "event_id": event_id,
                        "deliveries": {
                            "os": "delivered",
                            "discord": "failed",
                            "telegram": "skipped"
                        },
                        "delivery_details": {"os_route": "linux-desktop-user-bus"}
                    }),
                    0o600,
                    true,
                )
            },
        );
        assert_eq!(calls.get(), 1);
        assert!(
            !root
                .join("notification-events/pending")
                .join(format!("{event_id}.json"))
                .exists()
        );
        crate::events::acknowledge_event(&root, &event_id, "os", "failed", "late-failure").unwrap();
        supervisor.drain_notification_events_with(
            now + 1.0,
            &notification_channels_for_test(),
            |_, _| panic!("a completed event must not be dispatched again"),
        );
        let stored = supervisor
            .ledger
            .notification_events
            .iter()
            .find(|item| item.get("event_id").and_then(Value::as_str) == Some(&event_id))
            .unwrap();
        assert_eq!(stored["deliveries"]["os"], "delivered");
        assert_eq!(stored["deliveries"]["discord"], "failed");
        assert_eq!(
            stored["delivery_details"]["os_route"],
            "linux-desktop-user-bus"
        );

        supervisor.ledger.notification_events.push(json!({
            "event_id":"timeout",
            "created_at":now,
            "dispatch_started_at":now-31.0,
            "deliveries":{"os":"pending","discord":"pending","telegram":"skipped"}
        }));
        supervisor.ledger.notification_events.push(json!({
            "event_id":"bad-time",
            "created_at":"not-a-number",
            "deliveries":{}
        }));
        supervisor.drain_notification_events_with(
            now,
            &notification_channels_for_test(),
            |_, _| panic!("timed-out events must not be dispatched again"),
        );
        let timeout = supervisor
            .ledger
            .notification_events
            .iter()
            .find(|item| item["event_id"] == "timeout")
            .unwrap();
        assert_eq!(timeout["deliveries"]["os"], "failed");
        assert!(timeout.get("delivery_timeout_at").is_some());
        assert!(
            !supervisor
                .ledger
                .notification_events
                .iter()
                .any(|item| item["event_id"] == "bad-time")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exact_pid_pause_resume_and_persistence_rollback_use_real_signals() {
        let root = temp_directory("signal");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let candidate = canary_candidate(&process, "worker", 20.0);
        let danger = assessment_for(Action::Drain, "agent");
        assert!(supervisor.suspend_candidate(&candidate, &danger, now_epoch(), "pressure-pause"));
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        assert_eq!(
            supervisor.ledger.stopped_identity(canary.pid()),
            Some(candidate.identity.as_str())
        );
        resume_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), false);

        let rollback_canary = Canary::spawn();
        let rollback_process = canary_process(&supervisor.platform, rollback_canary.pid());
        let rollback_candidate = canary_candidate(&rollback_process, "worker", 25.0);
        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();
        supervisor.runtime_path = blocker.join("runtime.json");
        assert!(!supervisor.suspend_candidate(
            &rollback_candidate,
            &danger,
            now_epoch(),
            "pressure-pause"
        ));
        wait_for_process_state(&supervisor.platform, rollback_canary.pid(), false);
        assert!(
            supervisor
                .ledger
                .stopped_identity(rollback_canary.pid())
                .is_none()
        );
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str)
                == Some(rollback_candidate.identity.as_str())
                && incident.get("transition_source").and_then(Value::as_str)
                    == Some("persistence-rollback")
        }));
        drop(rollback_canary);
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn pressure_policy_pauses_at_most_one_and_never_blames_external_pressure() {
        let root = temp_directory("pressure-actions");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let first = Canary::spawn();
        let second = Canary::spawn();
        let first_process = canary_process(&supervisor.platform, first.pid());
        let second_process = canary_process(&supervisor.platform, second.pid());
        let first_candidate = canary_candidate(&first_process, "worker", 20.0);
        let second_candidate = canary_candidate(&second_process, "worker", 10.0);

        let external = assessment_for(Action::Drain, "external");
        supervisor.apply_pressure_actions(
            &external,
            &[first_candidate.clone(), second_candidate.clone()],
            100.0,
        );
        assert!(supervisor.ledger.stopped.is_empty());
        wait_for_process_state(&supervisor.platform, first.pid(), false);
        wait_for_process_state(&supervisor.platform, second.pid(), false);

        let danger = assessment_for(Action::Drain, "mixed");
        supervisor.apply_pressure_actions(
            &danger,
            &[first_candidate.clone(), second_candidate.clone()],
            100.0,
        );
        assert_eq!(supervisor.ledger.stopped.len(), 1);
        assert_eq!(
            supervisor.ledger.stopped_identity(first.pid()),
            Some(first_candidate.identity.as_str())
        );
        wait_for_process_state(&supervisor.platform, first.pid(), true);
        wait_for_process_state(&supervisor.platform, second.pid(), false);

        let safe = assessment_for(Action::Allow, "unknown");
        supervisor.apply_pressure_actions(&safe, &[], 110.0);
        supervisor.apply_pressure_actions(&safe, &[], 121.0);
        wait_for_process_state(&supervisor.platform, first.pid(), false);
        assert!(supervisor.ledger.stopped.is_empty());
        drop(second);
        drop(first);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn control_intent_is_durable_before_a_real_resume_signal() {
        let root = temp_directory("control");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = process_identity(&process);
        suspend_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        supervisor
            .ledger
            .mark_stopped(canary.pid(), identity.clone());
        supervisor
            .ledger
            .incidents
            .push(json!({"id":"control-incident","status":"suspended","pid":canary.pid(),"identity":identity}));
        supervisor.persist_runtime(90.0);
        ensure_private_dir(&root.join("control")).unwrap();
        fs::write(
            root.join("control/resume.json"),
            serde_json::to_vec(
                &json!({"request_id":"resume-canary","action":"resume","pid":canary.pid()}),
            )
            .unwrap(),
        )
        .unwrap();
        supervisor
            .process_control_requests(100.0, &BTreeMap::from([(canary.pid(), process.clone())]));
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        let result: Value = serde_json::from_slice(
            &fs::read(root.join("control/results/resume-canary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["signal_completed"], true);
        assert!(supervisor.ledger.pending_control.is_none());
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_none());
        assert!(supervisor.ledger.resume_cooldown.contains_key(&identity));
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn owner_resume_atomically_cancels_a_committed_app_guard_before_its_signal() {
        let root = temp_directory("owner-cancels-app-guard");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.platform = "linux".to_owned();
        let host = Canary::spawn_app_server();
        let process = canary_app_server_process("linux", host.pid());
        let identity = process_identity(&process);
        let control_id = "owner-cancel-test";
        let control_base = root.join("app-guards").join(control_id);
        ensure_private_dir(control_base.parent().unwrap()).unwrap();
        write_atomic_json(
            &crate::app_guard::phase_path(&control_base, "committed"),
            &json!({
                "phase":"committed",
                "controller_pid":std::process::id(),
                "incident_id":"owner-cancel-incident"
            }),
            0o600,
            true,
        )
        .unwrap();
        supervisor.ledger.mark_stopped(host.pid(), identity.clone());
        supervisor.ledger.incidents.push(json!({
            "id":"owner-cancel-incident",
            "status":"suspended",
            "pid":host.pid(),
            "identity":identity,
            "reason":"app-shared-host-last-resort",
            "app_control_scope":"shared-host",
            "control_phase":"committed"
        }));
        supervisor.ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: host.pid(),
                identity: identity.clone(),
                scope: "shared-host".to_owned(),
                prepared_at: 10.0,
                guard_deadline: Some(20.0),
                guard_control_id: control_id.to_owned(),
            });
        assert!(supervisor.persist_runtime(10.0));
        ensure_private_dir(&root.join("control")).unwrap();
        write_atomic_json(
            &root.join("control/resume-app.json"),
            &json!({
                "request_id":"resume-app",
                "action":"resume",
                "pid":host.pid()
            }),
            0o600,
            true,
        )
        .unwrap();

        supervisor.process_control_requests(11.0, &BTreeMap::from([(host.pid(), process.clone())]));
        let result: Value = serde_json::from_slice(
            &fs::read(root.join("control/results/resume-app.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["signal_completed"], true);
        assert!(crate::app_guard::phase_path(&control_base, "cancelled").is_file());
        assert!(!crate::app_guard::phase_path(&control_base, "committed").exists());
        assert!(!process_state("linux", host.pid()).starts_with('T'));
        assert!(supervisor.ledger.stopped_identity(host.pid()).is_none());
        assert!(
            supervisor
                .ledger
                .codex_app
                .control
                .pending_physical
                .is_none()
        );
        assert!(!supervisor.ledger.resume_cooldown.contains_key(&identity));
        assert_eq!(
            supervisor.ledger.incidents.last().unwrap()["status"],
            "cancelled"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn app_guard_controller_receipt_rejects_a_reused_controller_pid() {
        let root = temp_directory("app-guard-controller-identity");
        let control_base = root.join("app-guards/controller");
        write_atomic_json(
            &crate::app_guard::phase_path(&control_base, "signalling"),
            &json!({
                "phase":"signalling",
                "controller_pid":std::process::id(),
                "controller_identity":"different-process-generation"
            }),
            0o600,
            true,
        )
        .unwrap();
        assert_eq!(
            app_guard_controller_state(&platform_name(), &control_base, "signalling"),
            Some(false)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn failed_control_intent_sends_no_signal_and_uncommitted_completion_is_explicit() {
        let root = temp_directory("control-failures");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = process_identity(&process);
        suspend_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        supervisor
            .ledger
            .mark_stopped(canary.pid(), identity.clone());
        ensure_private_dir(&root.join("control")).unwrap();
        fs::write(
            root.join("control/intent-failure.json"),
            serde_json::to_vec(
                &json!({"request_id":"intent-failure","action":"resume","pid":canary.pid()}),
            )
            .unwrap(),
        )
        .unwrap();
        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();
        supervisor.runtime_path = blocker.join("runtime.json");
        supervisor
            .process_control_requests(100.0, &BTreeMap::from([(canary.pid(), process.clone())]));
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        let rejected: Value = serde_json::from_slice(
            &fs::read(root.join("control/results/intent-failure.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(rejected["signal_completed"], false);
        assert!(
            rejected["error"]
                .as_str()
                .unwrap()
                .contains("no signal sent")
        );

        supervisor.ledger.pending_control = Some(PendingControl {
            action: "resume".to_owned(),
            pid: canary.pid(),
            identity: identity.clone(),
            requested_at: 100.0,
            phase: None,
        });
        fs::write(
            root.join("control/final-failure.json"),
            serde_json::to_vec(
                &json!({"request_id":"final-failure","action":"resume","pid":canary.pid()}),
            )
            .unwrap(),
        )
        .unwrap();
        supervisor.process_control_requests(101.0, &BTreeMap::from([(canary.pid(), process)]));
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        let uncommitted: Value = serde_json::from_slice(
            &fs::read(root.join("control/results/final-failure.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(uncommitted["signal_completed"], true);
        assert_eq!(uncommitted["ok"], false);
        assert!(
            uncommitted["error"]
                .as_str()
                .unwrap()
                .contains("final runtime state was not persisted")
        );
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn lead_probation_is_one_shot_and_stable_recovery_is_durable() {
        let root = temp_directory("lead-probation");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = seed_suspended_lead(&mut supervisor, &process, 90.0);
        let safe = assessment_for(Action::Allow, "unknown");

        supervisor.manage_lead_probation(&safe, 100.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        assert!(
            supervisor
                .ledger
                .probation
                .as_ref()
                .is_some_and(|probation| probation.signal_sent)
        );
        supervisor.manage_lead_probation(&safe, 131.0);
        assert!(supervisor.ledger.probation.is_none());
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_none());
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                && incident.get("status").and_then(Value::as_str) == Some("resumed")
                && incident.get("last_terminal_notice").is_some()
        }));

        let persisted: RuntimeLedger =
            serde_json::from_slice(&fs::read(root.join("runtime.json")).unwrap()).unwrap();
        assert!(persisted.probation.is_none());
        assert!(persisted.stopped_identity(canary.pid()).is_none());
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn lead_probation_relapse_repauses_once_and_then_requires_manual_choice() {
        let root = temp_directory("lead-relapse");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        seed_suspended_lead(&mut supervisor, &process, 90.0);
        let safe = assessment_for(Action::Allow, "unknown");
        supervisor.manage_lead_probation(&safe, 100.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), false);

        let danger = assessment_for(Action::Drain, "agent");
        supervisor.manage_lead_probation(&danger, 101.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        assert!(
            supervisor
                .ledger
                .probation
                .as_ref()
                .is_some_and(|probation| probation.status == "failed")
        );
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_some());
        let updated_at = supervisor.ledger.updated_at;
        supervisor.manage_lead_probation(&safe, 200.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        assert_eq!(supervisor.ledger.updated_at, updated_at);
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unpersisted_probation_completion_restores_the_previous_incident_for_retry() {
        let root = temp_directory("probation-commit");
        let valid_runtime = root.join("runtime.json");
        let mut supervisor = Supervisor::new(Some(valid_runtime.clone()));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = seed_suspended_lead(&mut supervisor, &process, 90.0);
        let safe = assessment_for(Action::Allow, "unknown");
        supervisor.manage_lead_probation(&safe, 100.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), false);

        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();
        supervisor.runtime_path = blocker.join("runtime.json");
        supervisor.manage_lead_probation(&safe, 131.0);
        assert!(
            supervisor
                .ledger
                .probation
                .as_ref()
                .is_some_and(|probation| probation.status == "monitoring")
        );
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                && incident.get("status").and_then(Value::as_str) == Some("probation")
        }));

        supervisor.runtime_path = valid_runtime;
        supervisor.manage_lead_probation(&safe, 132.0);
        assert!(supervisor.ledger.probation.is_none());
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                && incident.get("status").and_then(Value::as_str) == Some("resumed")
        }));
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unpersisted_probation_relapse_is_resumed_and_marked_degraded() {
        let root = temp_directory("probation-rollback");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = seed_suspended_lead(&mut supervisor, &process, 90.0);
        let safe = assessment_for(Action::Allow, "unknown");
        supervisor.manage_lead_probation(&safe, 100.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), false);

        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();
        supervisor.runtime_path = blocker.join("runtime.json");
        supervisor.manage_lead_probation(&assessment_for(Action::Drain, "agent"), 101.0);
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_none());
        assert!(
            supervisor
                .runtime_error
                .as_deref()
                .is_some_and(|error| error.contains("rolled back"))
        );
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                && incident.get("status").and_then(Value::as_str) == Some("resumed")
                && incident.get("transition_source").and_then(Value::as_str)
                    == Some("persistence-rollback")
        }));
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_detects_external_resume_clears_probation_and_expires_cooldown() {
        let root = temp_directory("reconcile");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = process_identity(&process);
        suspend_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        supervisor
            .ledger
            .mark_stopped(canary.pid(), identity.clone());
        supervisor.ledger.probation = Some(Probation {
            status: "failed".to_owned(),
            pid: canary.pid(),
            identity: identity.clone(),
            signal_sent: true,
            prepared_at: Some(90.0),
            baseline_mb: Some(process.anon_mb),
            started_at: Some(90.0),
            deadline: Some(120.0),
            failed_at: Some(95.0),
            growth_mb_s: Some(10.0),
        });
        supervisor.ledger.incidents.push(json!({
            "id":"external-resume", "status":"probation_failed", "pid":canary.pid(),
            "identity":identity, "reason":"runaway-memory", "role":"lead"
        }));
        resume_process(&supervisor.platform, canary.pid()).unwrap();
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        supervisor.reconcile_stopped(100.0, &BTreeMap::from([(canary.pid(), process.clone())]));
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_none());
        assert!(supervisor.ledger.probation.is_none());
        assert!(supervisor.ledger.resume_cooldown.contains_key(&identity));
        assert!(supervisor.ledger.incidents.iter().any(|incident| {
            incident.get("identity").and_then(Value::as_str) == Some(identity.as_str())
                && incident.get("transition_source").and_then(Value::as_str) == Some("external")
        }));
        supervisor.reconcile_stopped(100.0 + supervisor.resume_cooldown_s + 1.0, &BTreeMap::new());
        assert!(!supervisor.ledger.resume_cooldown.contains_key(&identity));
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconcile_finishes_pending_termination_when_the_exact_pid_is_gone() {
        let root = temp_directory("reconcile-gone");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor
            .ledger
            .mark_stopped(424_242, "424242:start".to_owned());
        supervisor.ledger.incidents.push(json!({
            "id":"terminated-gone", "status":"suspended", "pid":424242,
            "identity":"424242:start", "reason":"pressure-pause"
        }));
        supervisor.ledger.pending_control = Some(PendingControl {
            action: "terminate".to_owned(),
            pid: 424_242,
            identity: "424242:start".to_owned(),
            requested_at: 90.0,
            phase: None,
        });
        supervisor.reconcile_stopped(100.0, &BTreeMap::new());
        assert!(supervisor.ledger.pending_control.is_none());
        assert!(supervisor.ledger.stopped.is_empty());
        assert_eq!(
            supervisor.ledger.incidents.last().unwrap()["status"],
            "terminated"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn lead_pause_is_refused_without_a_verified_exact_terminal() {
        let root = temp_directory("lead-terminal");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        assert!(process.terminal.is_empty());
        let candidate = canary_candidate(&process, "lead", 20.0);
        assert!(!supervisor.suspend_candidate(
            &candidate,
            &assessment_for(Action::Drain, "agent"),
            100.0,
            "pressure-lead-last-resort"
        ));
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        assert!(supervisor.ledger.stopped.is_empty());
        drop(canary);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn action_hysteresis_and_utilization_events_preserve_performance_first_policy() {
        let root = temp_directory("hysteresis");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let mut safe = assessment_for(Action::Allow, "unknown");
        assert!(supervisor.stabilize_assessment(&mut safe, 0.0));
        let mut danger = assessment_for(Action::Drain, "agent");
        assert!(supervisor.stabilize_assessment(&mut danger, 1.0));
        let mut early = assessment_for(Action::Allow, "unknown");
        assert!(!supervisor.stabilize_assessment(&mut early, 2.0));
        assert_eq!(early.action, Action::Drain);
        let mut recovered = assessment_for(Action::Allow, "unknown");
        assert!(supervisor.stabilize_assessment(&mut recovered, 13.0));
        assert_eq!(recovered.action, Action::Allow);

        supervisor.record_transition(Level::Yellow, Level::Orange, 700, 2.0, &danger);
        let events = pending_events(&root, &BTreeSet::new());
        let transition = events
            .iter()
            .find(|event| event["type"] == "utilization-transition")
            .unwrap();
        assert_eq!(transition["importance"], "detail");
        assert!(!event_should_notify(transition));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_window_survives_alternation_below_the_held_level() {
        let root = temp_directory("hysteresis-alternation");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let mut danger = assessment_for(Action::Drain, "agent");
        assert!(supervisor.stabilize_assessment(&mut danger, 0.0));
        let mut observe = assessment_for(Action::Observe, "unknown");
        assert!(!supervisor.stabilize_assessment(&mut observe, 1.0));
        let mut allow = assessment_for(Action::Allow, "unknown");
        assert!(!supervisor.stabilize_assessment(&mut allow, 4.0));
        let mut observe_again = assessment_for(Action::Observe, "unknown");
        assert!(!supervisor.stabilize_assessment(&mut observe_again, 8.0));
        assert_eq!(observe_again.action, Action::Drain);
        let mut adopted = assessment_for(Action::Allow, "unknown");
        assert!(supervisor.stabilize_assessment(&mut adopted, 11.5));
        assert_eq!(adopted.action, Action::Observe);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pressure_episode_recovery_waits_for_every_actuator() {
        let root = temp_directory("episode-full-recovery");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let danger = assessment_for(Action::Drain, "agent");
        supervisor.update_pressure_episode(&danger, 10.0);
        let (ledger, error) =
            RuntimeLedger::load(&root.join("runtime.json"), &supervisor.instance, 11.0);
        assert!(error.is_none());
        supervisor.ledger = ledger;
        assert_eq!(supervisor.ledger.pressure_episode_started_at, Some(10.0));

        let safe = assessment_for(Action::Allow, "unknown");
        supervisor
            .ledger
            .mark_stopped(42, "42:start-token".to_owned());
        supervisor.update_pressure_episode(&safe, 20.0);
        assert_eq!(supervisor.ledger.pressure_episode_started_at, Some(10.0));

        supervisor.ledger.clear_stopped(42);
        supervisor.ledger.probation = Some(Probation {
            status: "monitoring".to_owned(),
            pid: 42,
            identity: "42:start-token".to_owned(),
            signal_sent: true,
            prepared_at: Some(20.0),
            baseline_mb: Some(100),
            started_at: Some(20.0),
            deadline: Some(30.0),
            failed_at: None,
            growth_mb_s: None,
        });
        supervisor.update_pressure_episode(&safe, 21.0);
        assert_eq!(supervisor.ledger.pressure_episode_started_at, Some(10.0));

        supervisor.ledger.probation = None;
        supervisor.update_pressure_episode(&safe, 31.0);
        assert!(supervisor.ledger.pressure_episode_started_at.is_none());
        let events = pending_events(&root, &BTreeSet::new());
        assert_eq!(
            events
                .iter()
                .filter(|event| event_should_notify(event))
                .count(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pressure_episode_does_not_call_a_gone_worker_clean_recovery() {
        let root = temp_directory("episode-worker-loss");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor
            .ledger
            .mark_stopped(42, "42:start-token".to_owned());
        supervisor.ledger.incidents.push(json!({
            "id":"incident-42", "identity":"42:start-token", "pid":42,
            "status":"suspended", "suspended_at":10.0, "updated_at":10.0
        }));
        let danger = assessment_for(Action::Drain, "agent");
        supervisor.update_pressure_episode(&danger, 10.0);

        supervisor.reconcile_stopped(20.0, &BTreeMap::new());
        let safe = assessment_for(Action::Allow, "unknown");
        supervisor.update_pressure_episode(&safe, 21.0);

        let events = pending_events(&root, &BTreeSet::new());
        let notified: Vec<_> = events
            .iter()
            .filter(|event| event_should_notify(event))
            .collect();
        assert_eq!(notified.len(), 2);
        let final_event = notified
            .iter()
            .find(|event| event["status"] == "ended-with-loss")
            .expect("worker loss must be the final episode edge");
        assert_eq!(final_event["recovery"], "workers-gone");
        assert!(
            final_event["message"]
                .as_str()
                .unwrap()
                .contains("PIDs 42 disappeared before a confirmed resume")
        );
        assert!(notified.iter().all(|event| event["status"] != "recovered"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn singleton_lock_releases_cleanly() {
        let root = temp_directory("lock");
        let path = root.join("supervisor.lock");
        let first = InstanceLock::acquire(&path).unwrap();
        assert_eq!(
            InstanceLock::acquire(&path).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        drop(first);
        drop(InstanceLock::acquire(&path).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detected_codex_app_server_is_protected_before_first_hook() {
        let root = temp_directory("codex-app-pre-hook-protection");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));

        let mut shared = tracked_sample(256);
        shared.pid = 100;
        shared.identity = "100:app-server".to_owned();
        shared.role = "lead".to_owned();
        shared.via = "root".to_owned();
        shared.slope_mb_s = 12.5;

        // Before App detection, an ordinary CLI lead keeps the original growth calculation.
        assert_eq!(supervisor.total_lead_growth(&[shared.clone()]), 12.5);
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:app-server".to_owned(),
                unattributed_pids: vec![101],
                ..Default::default()
            });
        let danger = assessment_for(Action::Drain, "agent");

        assert!(supervisor.codex_app_shared_host(100));
        assert!(supervisor.codex_app_physical_control_forbidden(100));
        assert!(supervisor.codex_app_physical_control_forbidden(101));
        assert!(!supervisor.codex_app_physical_control_forbidden(102));
        assert_eq!(supervisor.total_lead_growth(&[shared.clone()]), 0.0);
        assert!(!supervisor.lead_pause_authorized(&shared, &danger, &[shared.clone()]));
        fs::remove_dir_all(root).unwrap();
    }

    fn app_pressure_assessment(tte: f64) -> Assessment {
        let mut assessment = assessment_for(Action::Hold, "agent");
        assessment.trajectory_confirmed = true;
        assessment.native_confidence = "high".to_owned();
        assessment.headroom_fall_mb_s = 80.0;
        assessment.tracked_growth_mb_s = 80.0;
        assessment.time_to_recovery_reserve_s = Some(tte);
        assessment.reaction_s = 5.0;
        assessment
    }

    fn app_logical_agent(key: &str, session: &str) -> LogicalAgent {
        LogicalAgent {
            key: key.to_owned(),
            provider: "codex".to_owned(),
            session_id: session.to_owned(),
            role: "subagent".to_owned(),
            surface: APP_SERVER_SURFACE.to_owned(),
            active: true,
            last_seen_at: 10.0,
            last_progress_at: 10.0,
            started_at: 10.0,
            state_since: 10.0,
            in_flight_tool_class: Some(ToolClass::HighMemoryStart),
            ..LogicalAgent::default()
        }
    }

    #[test]
    fn app_physical_backstop_requires_every_handoff_to_be_observed_by_a_current_hook() {
        let root = temp_directory("codex-app-handoff-receipt");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", None);
        let mut agent = app_logical_agent(&key, "app-one");
        agent.role = "lead".to_owned();
        agent.state = LogicalState::HandoffOnly;
        agent.state_since = 10.0;
        agent.epoch = 7;
        supervisor.ledger.logical_agents.insert(key.clone(), agent);
        let keys = BTreeSet::from([key.clone()]);

        assert!(!supervisor.app_keys_handoff_for(&keys, 20.0, 5.0));
        let agent = supervisor.ledger.logical_agents.get_mut(&key).unwrap();
        agent.last_hook_receipt_at = Some(15.0);
        agent.last_hook_receipt_epoch = Some(6);
        agent.last_hook_receipt_state = Some(LogicalState::HandoffOnly);
        assert!(!supervisor.app_keys_handoff_for(&keys, 20.0, 5.0));

        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .last_hook_receipt_epoch = Some(7);
        assert!(supervisor.app_keys_handoff_for(&keys, 20.0, 5.0));

        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .last_hook_receipt_at = Some(9.0);
        assert!(!supervisor.app_keys_handoff_for(&keys, 20.0, 5.0));
        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .last_hook_receipt_at = Some(15.0);
        assert!(supervisor.app_keys_handoff_for(&keys, 36.0, 5.0));
        assert!(!supervisor.app_keys_handoff_for(&keys, 136.0, 5.0));
        fs::remove_dir_all(root).unwrap();
    }

    fn seed_app_thread(supervisor: &mut Supervisor, key: &str, session: &str, server_pid: u32) {
        let thread_key = crate::containment::logical_key("codex", session, None);
        supervisor.ledger.codex_app.threads.insert(
            thread_key.clone(),
            crate::codex_app::CodexAppThread {
                key: thread_key,
                session_id: session.to_owned(),
                app_server_pid: server_pid,
                app_server_identity: format!("{server_pid}:server"),
                active: true,
                started_at: 10.0,
                last_seen_at: 10.0,
            },
        );
        supervisor
            .ledger
            .logical_agents
            .insert(key.to_owned(), app_logical_agent(key, session));
    }

    fn connect_app_hook(supervisor: &mut Supervisor) {
        supervisor
            .codex_app_snapshot
            .hook_routes
            .push(CodexAppHookRoute {
                app_server_pid: 100,
                app_server_identity: "100:server".to_owned(),
                path: "/test/.codex/hooks.json".to_owned(),
                platform: "test".to_owned(),
                status: "ACTIVE".to_owned(),
                detail: "test route".to_owned(),
                last_observed_at: Some(20.0),
            });
    }

    #[test]
    fn one_light_app_session_stays_open_during_an_ample_pressure_observation() {
        let root = temp_directory("one-light-app-session");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", None);
        let mut lead = app_logical_agent(&key, "app-one");
        lead.role = "lead".to_owned();
        lead.in_flight_tool_class = None;
        lead.last_tool_class = None;
        supervisor.ledger.logical_agents.insert(key.clone(), lead);

        let mut observation = assessment_for(Action::Observe, "unknown");
        observation.mem_available_mb = 8854;
        observation.memory_capacity_mb = 9945;
        observation.distress = "critical".to_owned();
        observation.time_to_recovery_reserve_s = Some(1380.8);
        observation.trajectory_confirmed = false;
        observation.collapse_imminent = false;

        assert!(!supervisor.manage_codex_app_containment(&observation, &[], 20.0));
        supervisor.update_pressure_episode(&observation, 20.0);
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::Active
        );
        assert!(!supervisor.ledger.codex_app.control.surface_gate);
        assert!(!supervisor.codex_app_snapshot.pressure.causal);
        assert!(pending_events(&root, &BTreeSet::new()).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confirmed_app_child_requires_current_route_and_handoff_receipt() {
        let root = temp_directory("codex-app-confirmed-receipt");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        connect_app_hook(&mut supervisor);
        let thread_key = crate::containment::logical_key("codex", "app-one", None);
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key,
                logical_key: key.clone(),
                invocation_id: "confirmed".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        let agent = supervisor.ledger.logical_agents.get_mut(&key).unwrap();
        agent.state = LogicalState::HandoffOnly;
        agent.state_since = 10.0;
        agent.epoch = 7;
        agent.in_flight_tool_class = None;

        assert!(!supervisor.logical_process_exhausted(110, 20.0, 5.0));
        let agent = supervisor.ledger.logical_agents.get_mut(&key).unwrap();
        agent.last_hook_receipt_at = Some(15.0);
        agent.last_hook_receipt_epoch = Some(7);
        agent.last_hook_receipt_state = Some(LogicalState::HandoffOnly);
        assert!(supervisor.logical_process_exhausted(110, 20.0, 5.0));

        supervisor.codex_app_snapshot.hook_routes[0].status = "STALE".to_owned();
        assert!(!supervisor.logical_process_exhausted(110, 20.0, 5.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_host_handoff_scope_includes_idle_active_app_leads() {
        let root = temp_directory("codex-app-idle-active-handoff");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", None);
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        let agent = supervisor.ledger.logical_agents.get_mut(&key).unwrap();
        agent.role = "lead".to_owned();
        agent.in_flight_tool_class = None;
        agent.idle_since = Some(15.0);
        agent.state = LogicalState::Active;

        assert_eq!(
            supervisor.active_app_keys_for_server(100),
            BTreeSet::from([key])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn idle_app_window_makes_a_shared_host_brake_unreachable_not_early() {
        let root = temp_directory("codex-app-idle-host-unreachable");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.leak_action = "stop".to_owned();
        let working = crate::containment::logical_key("codex", "working", None);
        let idle = crate::containment::logical_key("codex", "idle", None);
        seed_app_thread(&mut supervisor, &working, "working", 100);
        seed_app_thread(&mut supervisor, &idle, "idle", 100);
        let idle_agent = supervisor.ledger.logical_agents.get_mut(&idle).unwrap();
        idle_agent.role = "lead".to_owned();
        idle_agent.in_flight_tool_class = None;
        idle_agent.idle_since = Some(15.0);
        connect_app_hook(&mut supervisor);
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        let mut host = tracked_sample(512);
        host.pid = 100;
        host.identity = "100:server".to_owned();
        host.role = "lead".to_owned();
        host.via = "root".to_owned();
        host.slope_mb_s = 60.0;
        host.recent_slope_mb_s = 60.0;
        host.monotonicity = 1.0;
        host.observation_span_s = 30.0;

        let (profile, candidates, backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(20.0),
            &[host.clone()],
            20.0,
        );

        assert_eq!(profile.backstop, "none");
        assert!(!backstop.available());
        assert!(candidates.iter().any(|candidate| candidate.key == working));
        assert!(candidates.iter().all(|candidate| candidate.key != idle));

        for key in [&working, &idle] {
            let agent = supervisor.ledger.logical_agents.get_mut(key).unwrap();
            agent.state = LogicalState::HandoffOnly;
            agent.state_since = 10.0;
            agent.epoch = 7;
            agent.last_hook_receipt_at = Some(15.0);
            agent.last_hook_receipt_epoch = Some(7);
            agent.last_hook_receipt_state = Some(LogicalState::HandoffOnly);
        }
        let (ready_profile, _, ready_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(20.0),
            &[host],
            20.0,
        );
        assert_eq!(ready_profile.backstop, "shared-host");
        assert!(ready_backstop.available());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn estimated_child_is_not_a_backstop_when_an_idle_lead_cannot_acknowledge_it() {
        let root = temp_directory("codex-app-estimated-idle-scope");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.leak_action = "stop".to_owned();
        let lead_key = crate::containment::logical_key("codex", "app-one", None);
        let worker_key = crate::containment::logical_key("codex", "app-one", Some("worker"));
        seed_app_thread(&mut supervisor, &worker_key, "app-one", 100);
        let mut lead = app_logical_agent(&lead_key, "app-one");
        lead.role = "lead".to_owned();
        lead.in_flight_tool_class = None;
        lead.idle_since = Some(15.0);
        supervisor
            .ledger
            .logical_agents
            .insert(lead_key.clone(), lead);
        connect_app_hook(&mut supervisor);
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key: lead_key.clone(),
                logical_key: worker_key,
                invocation_id: "estimated".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadEstimated,
                assigned_at: 10.0,
            },
        );
        supervisor
            .codex_app_snapshot
            .threads
            .push(crate::codex_app::CodexAppThreadMemory {
                key: lead_key,
                session_id: "app-one".to_owned(),
                app_server_pid: 100,
                estimated_pids: vec![110],
                ..Default::default()
            });
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        let mut child = tracked_sample(512);
        child.pid = 110;
        child.identity = "110:start".to_owned();
        child.slope_mb_s = 40.0;
        child.recent_slope_mb_s = 40.0;
        child.monotonicity = 1.0;
        child.observation_span_s = 30.0;
        child.growth_delta_mb = 512.0;

        let (profile, _, backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(20.0),
            &[child],
            20.0,
        );
        assert_eq!(profile.backstop, "none");
        assert!(!backstop.available());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_dominant_exact_app_growth_is_not_advertised_as_the_final_backstop() {
        let root = temp_directory("codex-app-small-exact-not-backstop");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", None);
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key: key.clone(),
                logical_key: key,
                invocation_id: "confirmed".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        supervisor.codex_app_snapshot.pressure = CodexAppPressureProfile {
            causal: true,
            app_growth_mb_s: 100.0,
            ..Default::default()
        };
        let mut child = tracked_sample(512);
        child.pid = 110;
        child.identity = "110:start".to_owned();
        child.slope_mb_s = 10.0;
        child.recent_slope_mb_s = 10.0;
        child.monotonicity = 1.0;
        child.observation_span_s = 30.0;
        child.growth_delta_mb = 512.0;

        assert!(!supervisor.pressure_target_effective_or_non_app(&child));
        child.slope_mb_s = 30.0;
        child.recent_slope_mb_s = 30.0;
        assert!(supervisor.app_physical_growth_ready(&child, 100.0, 24.0));
        assert!(!supervisor.app_effective_backstop_growth_ready(&child, 100.0, 24.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_backstop_is_counted_only_when_the_physical_target_is_really_usable() {
        let root = temp_directory("codex-app-real-backstop");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.leak_action = "stop".to_owned();
        let key = crate::containment::logical_key("codex", "app-one", None);
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        connect_app_hook(&mut supervisor);
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        let mut host = tracked_sample(512);
        host.pid = 100;
        host.identity = "100:server".to_owned();
        host.role = "lead".to_owned();
        host.via = "root".to_owned();

        let (_, _, stable_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[host.clone()],
            20.0,
        );
        assert!(
            !stable_backstop.available(),
            "a merely live host is not a usable brake"
        );

        host.slope_mb_s = 60.0;
        host.recent_slope_mb_s = 60.0;
        host.monotonicity = 1.0;
        host.observation_span_s = 30.0;
        let (profile, _, usable_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[host.clone()],
            20.0,
        );
        assert!(usable_backstop.available());
        assert_eq!(profile.backstop, "shared-host");

        supervisor.codex_app_snapshot.hook_routes[0].app_server_identity =
            "100:previous-generation".to_owned();
        let (_, _, stale_generation_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[host.clone()],
            20.0,
        );
        assert!(
            !stale_generation_backstop.available(),
            "an ACTIVE receipt from an older PID generation is not current evidence"
        );
        supervisor.codex_app_snapshot.hook_routes[0].app_server_identity = "100:server".to_owned();

        supervisor.codex_app_snapshot.hook_routes.clear();
        let (profile, _, offline_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[host.clone()],
            20.0,
        );
        assert!(!offline_backstop.available());
        assert_eq!(profile.backstop, "none");
        assert_eq!(profile.mode, "DEGRADED_BLIND");

        connect_app_hook(&mut supervisor);
        supervisor.leak_action = "warn".to_owned();
        let (profile, _, disabled_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[host],
            20.0,
        );
        assert!(!disabled_backstop.available());
        assert_eq!(profile.backstop, "none");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn estimated_app_child_counts_only_as_a_blind_backstop() {
        let root = temp_directory("codex-app-estimated-backstop");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.leak_action = "stop".to_owned();
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        connect_app_hook(&mut supervisor);
        let thread_key = crate::containment::logical_key("codex", "app-one", None);
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key: thread_key.clone(),
                logical_key: key,
                invocation_id: "estimated".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadEstimated,
                assigned_at: 10.0,
            },
        );
        supervisor
            .codex_app_snapshot
            .threads
            .push(crate::codex_app::CodexAppThreadMemory {
                key: thread_key,
                session_id: "app-one".to_owned(),
                app_server_pid: 100,
                estimated_pids: vec![110],
                ..Default::default()
            });
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        let mut child = tracked_sample(512);
        child.pid = 110;
        child.identity = "110:start".to_owned();
        child.slope_mb_s = 40.0;
        child.recent_slope_mb_s = 40.0;
        child.monotonicity = 1.0;
        child.observation_span_s = 30.0;
        child.growth_delta_mb = 512.0;

        let (profile, _, has_backstop) = supervisor.codex_app_profile_and_candidates(
            &app_pressure_assessment(10.0),
            &[child],
            20.0,
        );
        assert!(has_backstop.available());
        assert_eq!(profile.backstop, "blind-child");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confirmed_app_runaway_uses_app_scoped_automatic_recovery() {
        let root = temp_directory("codex-app-confirmed-recovery");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        connect_app_hook(&mut supervisor);
        let thread_key = crate::containment::logical_key("codex", "app-one", None);
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key,
                logical_key: key,
                invocation_id: "confirmed".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        let mut candidate = tracked_sample(512);
        candidate.pid = 110;
        candidate.identity = "110:start".to_owned();
        candidate.role = "lead".to_owned();
        candidate.slope_mb_s = 20.0;
        let incident = supervisor.record_suspension(
            &candidate,
            &app_pressure_assessment(1.0),
            20.0,
            "runaway-memory",
            &terminal::Delivery {
                status: "unavailable".to_owned(),
                identity: String::new(),
                reason: "App hook delivery".to_owned(),
            },
        );
        assert_eq!(incident["recovery_policy"], "automatic-pressure-recovery");
        assert_eq!(incident["app_control_scope"], "thread-confirmed-child");
        assert_eq!(incident["audience_sessions"], json!(["app-one"]));
        assert!(incident_is_codex_app_physical(&incident));
        supervisor.ledger.mark_stopped(110, "110:start".to_owned());
        assert_eq!(supervisor.pressure_incidents().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn app_physical_resume_starts_a_fresh_live_probation_window() {
        let root = temp_directory("codex-app-physical-probation");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let canary = Canary::spawn();
        let process = canary_process(&supervisor.platform, canary.pid());
        let identity = process_identity(&process);
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        let thread_key = crate::containment::logical_key("codex", "app-one", None);
        supervisor.ledger.codex_app.process_owners.insert(
            identity.clone(),
            crate::codex_app::CodexAppProcessOwner {
                identity: identity.clone(),
                pid: canary.pid(),
                app_server_pid: 100,
                thread_key,
                logical_key: key,
                invocation_id: "confirmed".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        let candidate = canary_candidate(&process, "support", 20.0);
        assert!(supervisor.suspend_candidate(
            &candidate,
            &assessment_for(Action::Drain, "agent"),
            20.0,
            "runaway-memory",
        ));
        wait_for_process_state(&supervisor.platform, canary.pid(), true);
        supervisor.recovery_since = Some(0.0);
        supervisor.ledger.last_pressure_action_at = 0.0;
        let safe = assessment_for(Action::Allow, "unknown");
        assert!(supervisor.apply_pressure_actions(&safe, &[], 100.0));
        wait_for_process_state(&supervisor.platform, canary.pid(), false);
        assert_eq!(
            supervisor.ledger.codex_app.control.recovery_since,
            Some(100.0)
        );
        assert_eq!(
            supervisor.ledger.codex_app.control.mode,
            "PHYSICAL_PROBATION"
        );
        assert!(supervisor.ledger.stopped_identity(canary.pid()).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_host_never_skips_an_actionable_confirmed_child() {
        let root = temp_directory("codex-app-smaller-first");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.leak_action = "stop".to_owned();
        supervisor.critical_since = Some(0.0);
        supervisor.ledger.codex_app.control.surface_gate = true;
        supervisor.codex_app_snapshot.pressure = CodexAppPressureProfile {
            causal: true,
            app_growth_mb_s: 60.0,
            ..Default::default()
        };
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        seed_app_thread(&mut supervisor, &key, "app-one", 100);
        connect_app_hook(&mut supervisor);
        let thread_key = crate::containment::logical_key("codex", "app-one", None);
        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .state = LogicalState::HandoffOnly;
        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .state_since = 0.0;
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key,
                logical_key: key,
                invocation_id: "confirmed".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                ..Default::default()
            });
        let mut child = tracked_sample(256);
        child.pid = 110;
        child.identity = "110:start".to_owned();
        child.slope_mb_s = 10.0;
        let mut host = tracked_sample(512);
        host.pid = 100;
        host.identity = "100:server".to_owned();
        host.slope_mb_s = 50.0;
        host.recent_slope_mb_s = 50.0;
        host.monotonicity = 1.0;
        host.observation_span_s = 30.0;
        let mut danger = app_pressure_assessment(1.0);
        danger.action = Action::Drain;
        danger.collapse_imminent = true;

        assert!(!supervisor.apply_codex_app_blind_backstop(&danger, &[child, host], 30.0,));
        assert!(supervisor.ledger.stopped.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_app_guard_cannot_be_reconciled_away_before_its_terminal_receipt() {
        let root = temp_directory("codex-app-committed-guard");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.platform = "linux".to_owned();
        let host = Canary::spawn_app_server();
        let process = canary_app_server_process("linux", host.pid());
        let identity = process_identity(&process);
        let control_id = "committed-guard-test";
        let control_base = root.join("app-guards").join(control_id);
        ensure_private_dir(control_base.parent().unwrap()).unwrap();
        write_atomic_json(
            &crate::app_guard::phase_path(&control_base, "committed"),
            &json!({"phase":"committed"}),
            0o600,
            true,
        )
        .unwrap();
        supervisor.ledger.mark_stopped(host.pid(), identity.clone());
        supervisor.ledger.incidents.push(json!({
            "id":"committed-app-pause",
            "status":"suspended",
            "pid":host.pid(),
            "identity":identity,
            "reason":"app-shared-host-last-resort",
            "app_control_scope":"shared-host",
            "control_phase":"committed",
            "audience_provider":"codex",
            "audience_sessions":["app-one"]
        }));
        supervisor.ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: host.pid(),
                identity: identity.clone(),
                scope: "shared-host".to_owned(),
                prepared_at: 10.0,
                guard_deadline: Some(20.0),
                guard_control_id: control_id.to_owned(),
            });
        let processes = BTreeMap::from([(host.pid(), process)]);

        supervisor.reconcile_stopped(11.0, &processes);
        assert_eq!(
            supervisor.ledger.stopped_identity(host.pid()),
            Some(identity.as_str()),
            "a running snapshot cannot cancel a controller that already owns the signal"
        );
        assert!(
            supervisor
                .ledger
                .codex_app
                .control
                .pending_physical
                .is_some()
        );

        write_atomic_json(
            &crate::app_guard::phase_path(&control_base, "error"),
            &json!({"phase":"error"}),
            0o600,
            true,
        )
        .unwrap();
        supervisor.reconcile_stopped(12.0, &processes);
        assert!(supervisor.ledger.stopped_identity(host.pid()).is_none());
        assert!(
            supervisor
                .ledger
                .codex_app
                .control
                .pending_physical
                .is_none()
        );
        assert_eq!(
            supervisor.ledger.incidents.last().unwrap()["status"],
            "cancelled"
        );
        let events = pending_events(&root, &BTreeSet::new());
        assert!(events.iter().any(|event| {
            event["type"] == "codex-app-process-pause" && event["status"] == "cancelled"
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn daemon_immediately_recovers_a_pause_when_the_app_guard_loses_recovery() {
        let root = temp_directory("codex-app-guard-recovery-takeover");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.platform = "linux".to_owned();
        let host = Canary::spawn_app_server();
        let process = canary_app_server_process("linux", host.pid());
        let identity = process_identity(&process);
        let control_id = "failed-guard-test";
        let control_base = root.join("app-guards").join(control_id);
        write_atomic_json(
            &crate::app_guard::phase_path(&control_base, "error"),
            &json!({"phase":"error","detail":"timed resume failed"}),
            0o600,
            true,
        )
        .unwrap();
        supervisor.ledger.mark_stopped(host.pid(), identity.clone());
        supervisor.ledger.incidents.push(json!({
            "id":"guard-recovery-takeover",
            "status":"suspended",
            "pid":host.pid(),
            "identity":identity,
            "reason":"app-shared-host-last-resort",
            "app_control_scope":"shared-host",
            "control_phase":"active",
            "audience_provider":"codex",
            "audience_sessions":["app-one"]
        }));
        supervisor.ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: host.pid(),
                identity: identity.clone(),
                scope: "shared-host".to_owned(),
                prepared_at: 10.0,
                guard_deadline: Some(20.0),
                guard_control_id: control_id.to_owned(),
            });
        suspend_process("linux", host.pid()).unwrap();
        wait_for_process_state("linux", host.pid(), true);

        supervisor.reconcile_stopped(30.0, &BTreeMap::from([(host.pid(), process)]));
        wait_for_process_state("linux", host.pid(), false);
        assert!(supervisor.ledger.stopped_identity(host.pid()).is_none());
        assert!(
            supervisor
                .ledger
                .codex_app
                .control
                .pending_physical
                .is_none()
        );
        assert_eq!(
            supervisor.ledger.incidents.last().unwrap()["status"],
            "resumed"
        );
        assert_eq!(
            supervisor.ledger.codex_app.control.mode,
            "PHYSICAL_PROBATION"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn new_app_server_generation_immediately_releases_the_stopped_old_host() {
        let root = temp_directory("codex-app-host-replacement");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.platform = "linux".to_owned();
        let old_host = Canary::spawn_app_server();
        let old_process = canary_app_server_process("linux", old_host.pid());
        let old_identity = process_identity(&old_process);
        let key = crate::containment::logical_key("codex", "app-one", None);
        seed_app_thread(&mut supervisor, &key, "app-one", old_host.pid());
        supervisor
            .ledger
            .logical_agents
            .get_mut(&key)
            .unwrap()
            .state = LogicalState::HandoffOnly;
        supervisor.ledger.codex_app.invocations.insert(
            "old-invocation".to_owned(),
            crate::codex_app::CodexAppInvocation {
                id: "old-invocation".to_owned(),
                thread_key: key.clone(),
                logical_key: key.clone(),
                app_server_pid: old_host.pid(),
                started_at: 10.0,
                ..Default::default()
            },
        );
        supervisor.ledger.codex_app.process_owners.insert(
            "424242:old".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "424242:old".to_owned(),
                pid: 424_242,
                app_server_pid: old_host.pid(),
                thread_key: key.clone(),
                logical_key: key.clone(),
                invocation_id: "old-invocation".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        supervisor
            .ledger
            .mark_stopped(old_host.pid(), old_identity.clone());
        supervisor.ledger.incidents.push(json!({
            "id":"old-host-pause",
            "status":"suspended",
            "pid":old_host.pid(),
            "identity":old_identity,
            "reason":"app-shared-host-last-resort",
            "app_control_scope":"shared-host",
            "audience_provider":"codex",
            "audience_sessions":["app-one"]
        }));
        supervisor.ledger.codex_app.control.surface_gate = true;
        supervisor.ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: old_host.pid(),
                identity: old_identity.clone(),
                scope: "shared-host".to_owned(),
                prepared_at: 20.0,
                guard_deadline: Some(50.0),
                guard_control_id: String::new(),
            });
        assert!(supervisor.persist_runtime(20.0));
        suspend_process("linux", old_host.pid()).unwrap();
        wait_for_process_state("linux", old_host.pid(), true);

        let replacement = Canary::spawn_app_server();
        let replacement_process = canary_app_server_process("linux", replacement.pid());
        let processes = BTreeMap::from([
            (old_host.pid(), old_process),
            (replacement.pid(), replacement_process),
        ]);
        assert_eq!(crate::codex_app::app_server_pids(&processes).len(), 2);

        supervisor.reconcile_stopped(30.0, &processes);
        wait_for_process_state("linux", old_host.pid(), false);
        assert!(supervisor.ledger.stopped_identity(old_host.pid()).is_none());
        assert!(
            supervisor
                .ledger
                .codex_app
                .control
                .pending_physical
                .is_none()
        );
        assert_eq!(
            supervisor.ledger.codex_app.control.recovery_since,
            Some(30.0)
        );
        assert_eq!(supervisor.ledger.codex_app.control.mode, "SERVER_REPLACED");
        assert!(
            supervisor
                .ledger
                .codex_app
                .threads
                .values()
                .all(|thread| thread.app_server_pid != old_host.pid())
        );
        assert!(
            supervisor
                .ledger
                .codex_app
                .process_owners
                .values()
                .all(|owner| owner.app_server_pid != old_host.pid())
        );
        assert!(!supervisor.ledger.logical_agents[&key].active);
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::Active
        );
        let incident = supervisor
            .ledger
            .incidents
            .iter()
            .find(|incident| incident["id"] == "old-host-pause")
            .unwrap();
        assert_eq!(incident["status"], "resumed");
        assert_eq!(
            incident["transition_source"],
            "codex-app-server-replacement"
        );
        assert_eq!(incident["replacement_pid"], replacement.pid());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_confirmed_planner_tightens_only_the_causal_app_agent() {
        let root = temp_directory("codex-app-confirmed-planner");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        supervisor
            .ledger
            .logical_agents
            .insert(key.clone(), app_logical_agent(&key, "app-one"));
        supervisor.ledger.logical_agents.insert(
            "codex:cli:root".to_owned(),
            LogicalAgent {
                key: "codex:cli:root".to_owned(),
                provider: "codex".to_owned(),
                session_id: "cli".to_owned(),
                active: true,
                ..LogicalAgent::default()
            },
        );
        supervisor.ledger.codex_app.process_owners.insert(
            "110:start".to_owned(),
            crate::codex_app::CodexAppProcessOwner {
                identity: "110:start".to_owned(),
                pid: 110,
                app_server_pid: 100,
                thread_key: crate::containment::logical_key("codex", "app-one", None),
                logical_key: key.clone(),
                invocation_id: "invocation".to_owned(),
                evidence: crate::codex_app::CodexAppOwnershipEvidence::ThreadConfirmed,
                assigned_at: 10.0,
            },
        );
        supervisor
            .codex_app_snapshot
            .threads
            .push(crate::codex_app::CodexAppThreadMemory {
                key: crate::containment::logical_key("codex", "app-one", None),
                session_id: "app-one".to_owned(),
                app_server_pid: 100,
                confirmed_pids: vec![110],
                ..Default::default()
            });
        let mut process = tracked_sample(512);
        process.pid = 110;
        process.identity = "110:start".to_owned();
        process.slope_mb_s = 80.0;
        process.recent_slope_mb_s = 80.0;
        assert!(supervisor.manage_codex_app_containment(
            &app_pressure_assessment(3.0),
            &[process],
            20.0
        ));
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::NoExpansion
        );
        assert_eq!(
            supervisor.ledger.logical_agents["codex:cli:root"].state,
            LogicalState::Active
        );
        assert!(!supervisor.ledger.codex_app.control.surface_gate);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn full_blind_app_path_closes_the_surface_before_targeting_one_agent() {
        let root = temp_directory("codex-app-blind-planner");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        supervisor
            .ledger
            .logical_agents
            .insert(key.clone(), app_logical_agent(&key, "app-one"));
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                unattributed_pids: vec![110],
                blind_control_pids: vec![110],
                blind_candidate_keys: BTreeMap::from([("110".to_owned(), vec![key.clone()])]),
                ..Default::default()
            });
        let mut process = tracked_sample(512);
        process.pid = 110;
        process.identity = "110:start".to_owned();
        process.slope_mb_s = 80.0;
        process.recent_slope_mb_s = 80.0;
        let mut early_braking = app_pressure_assessment(9.0);
        early_braking.action = Action::Observe;
        early_braking.admission_level = Action::Observe.level();
        early_braking.distress = "normal".to_owned();
        assert!(supervisor.manage_codex_app_containment(&early_braking, &[process.clone()], 20.0));
        assert!(supervisor.ledger.codex_app.control.surface_gate);
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::Active
        );
        assert!(supervisor.manage_codex_app_containment(
            &app_pressure_assessment(8.0),
            &[process],
            21.0
        ));
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::NoExpansion
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn causal_blind_app_work_keeps_full_performance_outside_its_stopping_distance() {
        let root = temp_directory("codex-app-outside-stopping-distance");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        supervisor
            .ledger
            .logical_agents
            .insert(key.clone(), app_logical_agent(&key, "app-one"));
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                unattributed_pids: vec![110],
                blind_control_pids: vec![110],
                blind_candidate_keys: BTreeMap::from([("110".to_owned(), vec![key.clone()])]),
                ..Default::default()
            });
        let mut process = tracked_sample(512);
        process.pid = 110;
        process.identity = "110:start".to_owned();
        process.slope_mb_s = 80.0;
        process.recent_slope_mb_s = 80.0;
        let mut outside = app_pressure_assessment(30.0);
        outside.action = Action::Observe;
        outside.admission_level = Action::Observe.level();
        outside.distress = "critical".to_owned();

        assert!(!supervisor.manage_codex_app_containment(&outside, &[process], 20.0));
        assert!(!supervisor.ledger.codex_app.control.surface_gate);
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::Active
        );
        assert!(supervisor.codex_app_snapshot.pressure.causal);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn causal_app_work_reopens_after_it_moves_outside_the_braking_distance() {
        let root = temp_directory("codex-app-causal-recovery");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        let key = crate::containment::logical_key("codex", "app-one", Some("worker-one"));
        supervisor
            .ledger
            .logical_agents
            .insert(key.clone(), app_logical_agent(&key, "app-one"));
        let agent = supervisor.ledger.logical_agents.get_mut(&key).unwrap();
        agent.state = LogicalState::NoExpansion;
        agent.state_since = 10.0;
        supervisor.ledger.codex_app.control.surface_gate = true;
        supervisor.ledger.codex_app.control.last_action_at = 10.0;
        supervisor
            .codex_app_snapshot
            .app_servers
            .push(crate::codex_app::CodexAppServerMemory {
                pid: 100,
                identity: "100:server".to_owned(),
                unattributed_pids: vec![110],
                blind_control_pids: vec![110],
                blind_candidate_keys: BTreeMap::from([("110".to_owned(), vec![key.clone()])]),
                ..Default::default()
            });
        let mut process = tracked_sample(512);
        process.pid = 110;
        process.identity = "110:start".to_owned();
        process.slope_mb_s = 80.0;
        process.recent_slope_mb_s = 80.0;
        let mut recovered_distance = app_pressure_assessment(30.0);
        recovered_distance.action = Action::Observe;
        recovered_distance.admission_level = Action::Observe.level();
        recovered_distance.distress = "critical".to_owned();

        assert!(!supervisor.manage_codex_app_containment(
            &recovered_distance,
            &[process.clone()],
            20.0,
        ));
        assert_eq!(
            supervisor.ledger.codex_app.control.recovery_since,
            Some(20.0)
        );
        assert!(supervisor.manage_codex_app_containment(
            &recovered_distance,
            &[process.clone()],
            31.0,
        ));
        assert_eq!(
            supervisor.ledger.logical_agents[&key].state,
            LogicalState::Active
        );
        assert!(supervisor.manage_codex_app_containment(&recovered_distance, &[process], 32.0,));
        assert!(!supervisor.ledger.codex_app.control.surface_gate);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn blind_app_pause_persists_scope_before_stopping_one_exact_descendant() {
        let root = temp_directory("codex-app-blind-real-signal");
        let mut app_server = ProcessCommand::new("bash")
            .args([
                "-c",
                r#"exec -a codex bash -c 'python3 -c "import time; x=bytearray(64*1024*1024); time.sleep(30)" & wait' app-server"#,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let app_server_pid = app_server.id();
        let mut child = None;
        for _ in 0..100 {
            let processes = list_processes("linux");
            child = processes
                .values()
                .find(|process| {
                    process.pid != app_server_pid
                        && process_descends_from(process.pid, app_server_pid, &processes)
                        && process.name.contains("python")
                        && process.anon_mb >= 32
                })
                .cloned();
            if child.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let child = child.expect("memory-bearing app-server descendant");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        supervisor.platform = "linux".to_owned();
        let mut candidate = tracked_sample(child.anon_mb);
        candidate.pid = child.pid;
        candidate.name = child.name.clone();
        candidate.identity = process_identity(&child);
        candidate.start_token = child.start_token.clone();
        candidate.identity_reliable = true;
        candidate.role = "support".to_owned();
        candidate.slope_mb_s = 64.0;
        let mut assessment = app_pressure_assessment(1.0);
        assessment.action = Action::Drain;
        assessment.collapse_imminent = true;
        assert!(supervisor.suspend_codex_app_last_resort(
            &candidate,
            &assessment,
            now_epoch(),
            "app-blind-child-last-resort",
            CodexAppPauseScope {
                affected_sessions: &["thread-one".to_owned(), "thread-two".to_owned()],
                app_server_pid,
                shared_host: false,
            },
        ));
        for _ in 0..50 {
            if process_state("linux", child.pid).starts_with('T') {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(process_state("linux", child.pid).starts_with('T'));
        assert_eq!(
            supervisor.ledger.stopped_identity(child.pid),
            Some(candidate.identity.as_str())
        );
        let incident = supervisor.ledger.incidents.last().unwrap();
        assert_eq!(incident["app_control_scope"], "blind-child");
        assert_eq!(incident["thread_attribution"], "blind");
        assert!(incident["claimed_thread"].is_null());
        resume_process("linux", child.pid).unwrap();
        let _ = terminate_process("linux", child.pid, true);
        let _ = app_server.kill();
        let _ = app_server.wait();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_app_threads_are_independent_and_never_own_the_shared_host_pid() {
        let root = temp_directory("codex-app-isolation");
        let mut supervisor = Supervisor::new(Some(root.join("runtime.json")));
        for session in ["thread-one", "thread-two"] {
            let mut observation = HookObservation::from_payload(
                format!("start-{session}"),
                1.0,
                "codex",
                "SessionStart",
                &json!({"session_id":session}),
                Some(100),
                false,
            );
            observation.mark_codex_app(100);
            assert!(supervisor.apply_hook_observation(observation));
        }
        let one = crate::containment::logical_key("codex", "thread-one", None);
        let two = crate::containment::logical_key("codex", "thread-two", None);
        supervisor
            .ledger
            .logical_agents
            .get_mut(&one)
            .unwrap()
            .state = LogicalState::NoExpansion;
        assert_eq!(
            supervisor.ledger.logical_agents[&two].state,
            LogicalState::Active
        );
        assert_eq!(supervisor.ledger.logical_agents[&one].process_pid, None);
        assert_eq!(supervisor.ledger.logical_agents[&two].process_pid, None);

        let mut shared = tracked_sample(256);
        shared.pid = 100;
        shared.identity = "100:app-server".to_owned();
        shared.role = "lead".to_owned();
        shared.via = "root".to_owned();
        let danger = assessment_for(Action::Drain, "agent");
        assert!(!supervisor.lead_pause_authorized(&shared, &danger, &[shared.clone()]));
        assert!(!supervisor.suspend_candidate(&shared, &danger, 2.0, "pressure-pause"));
        fs::remove_dir_all(root).unwrap();
    }
}
