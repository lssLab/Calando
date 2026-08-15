use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::{Config, PRETOOL_HOLD_DEFAULT_S, power_is_off, state_dir};
use crate::containment::{
    HookObservation, LogicalState, ToolClass, agent_id, classify_tool, logical_key,
};
use crate::events::{
    acknowledge_event, event_should_notify, make_event, queue_event, user_message,
};
use crate::integration::{
    CodexHookSurface, audit_codex_app_hooks, audit_codex_hooks, codex_hook_source_is_authoritative,
};
use crate::platform::{
    admission_level_for_peer, admission_level_for_state, current_provider_context,
    fresh_federated_states, merge_federated_incidents, platform_name,
};
use crate::policy::Level;
use crate::runtime::unique_nonce;
use crate::storage::{append_bounded, ensure_private_dir, write_atomic_json};

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn list(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn state_rank(state: &Value, is_peer: bool) -> (Level, i64) {
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
    let headroom = if available.is_finite() && capacity.is_finite() && capacity > 0.0 {
        available / capacity
    } else {
        f64::INFINITY
    };
    (level, -(headroom * 1_000_000.0) as i64)
}

fn level_json(level: Level) -> Value {
    serde_json::to_value(level).expect("level serializes")
}

pub fn effective_admission_state(local: &Value, peers: Option<Vec<Value>>) -> Value {
    let mut result = local.as_object().cloned().unwrap_or_default();
    let local_level = local
        .get("local_admission_level")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_value::<Level>(Value::String(value.to_owned())).ok())
        .unwrap_or_else(|| admission_level_for_state(local));
    result.insert(
        "local_utilization".to_owned(),
        local
            .get("utilization")
            .or_else(|| local.get("level"))
            .cloned()
            .unwrap_or_else(|| Value::String("GREEN".to_owned())),
    );
    result.insert("local_admission_level".to_owned(), level_json(local_level));
    result.insert("local_level".to_owned(), level_json(local_level));
    let mut candidates = vec![local.clone()];
    candidates.extend(peers.unwrap_or_else(|| fresh_federated_states(10.0)));
    let mut worst_index = 0;
    for index in 1..candidates.len() {
        if state_rank(&candidates[index], true)
            > state_rank(&candidates[worst_index], worst_index > 0)
        {
            worst_index = index;
        }
    }
    let worst = &candidates[worst_index];
    let cached = worst_index == 0;
    result.insert(
        "admission_level".to_owned(),
        level_json(if cached {
            admission_level_for_state(worst)
        } else {
            admission_level_for_peer(worst)
        }),
    );
    let fallback = |name: &str, default: Value| {
        if cached {
            worst
                .get(format!("admission_{name}"))
                .or_else(|| worst.get(name))
                .cloned()
                .unwrap_or(default)
        } else {
            worst.get(name).cloned().unwrap_or(default)
        }
    };
    result.insert(
        "admission_source".to_owned(),
        if cached {
            local
                .get("admission_source")
                .cloned()
                .unwrap_or_else(|| Value::String("local".to_owned()))
        } else {
            worst
                .get("instance")
                .or_else(|| worst.get("platform"))
                .cloned()
                .unwrap_or_else(|| Value::String("peer".to_owned()))
        },
    );
    for (target, source, default) in [
        ("admission_mem_available_mb", "mem_available_mb", json!("?")),
        ("admission_capacity_mb", "memory_capacity_mb", json!("?")),
        ("admission_action", "action", json!("unknown")),
        ("admission_distress", "distress", json!("unknown")),
        ("admission_attribution", "attribution", json!("unknown")),
        ("admission_action_since", "action_since", json!(0)),
        (
            "admission_time_to_exhaustion_s",
            "time_to_exhaustion_s",
            Value::Null,
        ),
        (
            "admission_time_to_recovery_reserve_s",
            "time_to_recovery_reserve_s",
            Value::Null,
        ),
    ] {
        result.insert(target.to_owned(), fallback(source, default));
    }
    for key in [
        "cli_hard_cap_mb",
        "cli_memory_used_mb",
        "cli_hard_cap_remaining_mb",
        "cli_hard_cap_status",
        "cli_hard_cap_driving",
    ] {
        result.insert(format!("admission_{key}"), fallback(key, Value::Null));
    }
    result.insert(
        "leak_suspects".to_owned(),
        worst
            .get("leak_suspects")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    result.insert(
        "stopped_pids".to_owned(),
        worst
            .get("stopped_pids")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    result.insert(
        "recent_incidents".to_owned(),
        Value::Array(merge_federated_incidents(&candidates)),
    );
    if !cached && result.get("fail_open").and_then(Value::as_bool) != Some(true) {
        result.remove("fail_open");
        result.remove("reason");
    }
    Value::Object(result)
}

pub fn safe_state(directory: &Path) -> Value {
    let state = fs::read(directory.join("state.json"))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .filter(Value::is_object)
        .filter(|state| {
            let age = now_epoch() - state.get("ts").and_then(Value::as_f64).unwrap_or_default();
            (-5.0..=10.0).contains(&age)
                && state.get("error").is_none()
                && matches!(
                    state.get("level").and_then(Value::as_str),
                    Some("GREEN" | "YELLOW" | "ORANGE" | "RED")
                )
        })
        .unwrap_or_else(
            || json!({"level":"GREEN", "fail_open":true, "reason":"missing-stale-or-invalid"}),
        );
    effective_admission_state(&state, None)
}

fn context(event: &str, message: &str, user_message: Option<&str>) -> Value {
    let mut result = Map::from_iter([(
        "hookSpecificOutput".to_owned(),
        json!({"hookEventName":event,"additionalContext":message}),
    )]);
    if let Some(message) = user_message {
        result.insert(
            "systemMessage".to_owned(),
            Value::String(message.to_owned()),
        );
    }
    Value::Object(result)
}

fn gate_level(state: &Value) -> Level {
    admission_level_for_state(state)
}

fn admission_is_braking(state: &Value) -> bool {
    match state
        .get("admission_action")
        .or_else(|| state.get("action"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("hold" | "drain") => true,
        Some("allow" | "observe") => false,
        _ => matches!(gate_level(state), Level::Orange | Level::Red),
    }
}

/// Rate-limited warning when the daemon state is missing or stale and admission is failing
/// open: silence here previously meant neither the agent nor the user learned protection was
/// gone until they asked.
fn fail_open_notice(directory: &Path, state: &Value) -> Option<String> {
    if state.get("fail_open").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let marker = directory.join("fail-open-notice");
    let now = now_epoch();
    if let Ok(source) = fs::read_to_string(&marker)
        && let Ok(previous) = source.trim().parse::<f64>()
        && now - previous < 600.0
    {
        return None;
    }
    let _ = fs::write(&marker, format!("{now}"));
    Some(
        "[Memory Supervisor] PROTECTION UNAVAILABLE\nThe supervisor daemon state is missing or stale, so admission is failing open and the exact-PID backstop is not running.\nNext: run `memory-status`, then `memory-supervisor update`; restart the service if needed."
            .to_owned(),
    )
}

fn hold_until_ok(directory: &Path, seconds: f64, allow_yellow: bool) -> (bool, Value) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
    let mut state = safe_state(directory);
    while Instant::now() < deadline {
        let level = gate_level(&state);
        if level == Level::Green || (allow_yellow && level == Level::Yellow) {
            return (true, state);
        }
        std::thread::sleep(Duration::from_secs(1));
        state = safe_state(directory);
    }
    (false, state)
}

fn session_key(payload: &Value) -> String {
    let raw = payload
        .get("session_id")
        .or_else(|| payload.get("conversationId"))
        .and_then(Value::as_str)
        .unwrap_or("nosid");
    let value: String = raw
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(*character, '-' | '_'))
        .take(128)
        .collect();
    if value.is_empty() {
        "nosid".to_owned()
    } else {
        value
    }
}

fn infer_provider(payload: &Value) -> &'static str {
    if payload.get("turn_id").is_some() {
        "codex"
    } else {
        "claude"
    }
}

fn hook_source_argument(arguments: &[OsString]) -> Option<PathBuf> {
    arguments.windows(2).find_map(|pair| {
        (pair[0].to_str() == Some("--hook-source"))
            .then(|| pair[1].to_str().map(PathBuf::from))
            .flatten()
    })
}

fn observation_queue(
    directory: &Path,
    provider: &str,
    event: &str,
    payload: &Value,
    state: &Value,
    blocked: bool,
    block_reason: Option<&str>,
) {
    let pending = directory.join("hook-observations").join("pending");
    if ensure_private_dir(&pending).is_err() {
        return;
    }
    let nonce = unique_nonce();
    let context = current_provider_context(provider);
    let mut observation = HookObservation::from_payload(
        format!("{}-{}-{nonce}", std::process::id(), event.to_lowercase()),
        now_epoch(),
        provider,
        event,
        payload,
        context.as_ref().map(|context| context.pid),
        blocked,
    );
    if let Some(context) = context
        && context.surface == crate::codex_app::APP_SERVER_SURFACE
    {
        observation.mark_codex_app(context.pid);
        observation.thread_marker = env::var("CODEX_THREAD_ID")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        observation.app_server_baseline_pids = context.descendant_pids;
        if let Some(agent) = logical_agent(state, provider, payload) {
            observation.observed_control_epoch = agent.get("epoch").and_then(Value::as_u64);
            observation.observed_logical_state = agent
                .get("state")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
        }
    }
    observation.block_reason = block_reason.map(|reason| reason.chars().take(2000).collect());
    let _ = write_atomic_json(
        &pending.join(format!("{nonce}-{}.json", std::process::id())),
        &observation,
        0o600,
        false,
    );
}

fn logical_agent<'a>(state: &'a Value, provider: &str, payload: &Value) -> Option<&'a Value> {
    let session = session_key(payload);
    let agent = agent_id(payload);
    let key = logical_key(provider, &session, agent.as_deref());
    state.get("logical_agents")?.get(key)
}

fn logical_state(agent: &Value) -> LogicalState {
    agent
        .get("state")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or(LogicalState::Active)
}

fn codex_app_surface_gate_for(state: &Value, provider: &str, app_surface: bool) -> bool {
    provider == "codex" && codex_app_gate_enabled(state) && app_surface
}

fn codex_app_gate_enabled(state: &Value) -> bool {
    state
        .get("codex_app")
        .and_then(|app| app.get("control"))
        .and_then(|control| control.get("surface_gate"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn hold_until_codex_app_open(directory: &Path, seconds: f64) -> (bool, Value) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
    let mut state = safe_state(directory);
    while Instant::now() < deadline {
        if !codex_app_gate_enabled(&state) {
            return (true, state);
        }
        std::thread::sleep(Duration::from_secs(1));
        state = safe_state(directory);
    }
    (!codex_app_gate_enabled(&state), state)
}

fn codex_app_surface_gate(state: &Value, provider: &str) -> bool {
    codex_app_surface_gate_for(
        state,
        provider,
        current_provider_context(provider)
            .is_some_and(|context| context.surface == crate::codex_app::APP_SERVER_SURFACE),
    )
}

fn logical_notice(
    directory: &Path,
    state: &Value,
    provider: &str,
    payload: &Value,
) -> Option<String> {
    let epoch = state
        .get("logical_epoch")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let restricted_count = state
        .get("logical_restricted_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let last_action_at = state
        .get("logical_last_action_at")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let session = session_key(payload);
    let payload_agent = agent_id(payload);
    let agent = payload_agent.clone().unwrap_or_else(|| "root".to_owned());
    let short = |value: &str| value.chars().take(48).collect::<String>();
    let marker = directory.join(format!(
        "logical-seen-{}-{}-{}.json",
        short(provider),
        short(&session),
        short(&agent)
    ));
    let previous = fs::read(&marker)
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .unwrap_or_else(|| json!({}));
    let previous_epoch = previous
        .get("epoch")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let previous_blocked_at = previous
        .get("blocked_at")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let mut blocked = if payload_agent.is_none() {
        state
            .get("logical_agents")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|agents| agents.values())
            .filter(|agent| {
                agent.get("provider").and_then(Value::as_str) == Some(provider)
                    && agent.get("session_id").and_then(Value::as_str) == Some(session.as_str())
                    && agent
                        .get("last_blocked_at")
                        .and_then(Value::as_f64)
                        .is_some_and(|at| at > previous_blocked_at)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    blocked.sort_by(|left, right| {
        left.get("last_blocked_at")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            .total_cmp(
                &right
                    .get("last_blocked_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
            )
    });
    blocked.truncate(12);
    // Lifecycle inventory alone has no user-facing control meaning. A recorded
    // supervisor denial is independently relevant to the lead handoff.
    let new_control =
        previous_epoch < epoch && epoch > 0 && (restricted_count > 0 || last_action_at > 0.0);
    if !new_control && blocked.is_empty() {
        return None;
    }
    let own = logical_agent(state, provider, payload);
    // A session created after a completed recovery must not inherit an old
    // "all clear" epoch.  Active restrictions remain globally relevant, but
    // a recovery-only notice is useful only to sessions that existed when the
    // action happened (including the same session resumed later).
    let stale_recovery = new_control
        && restricted_count == 0
        && own
            .and_then(|agent| agent.get("started_at"))
            .and_then(Value::as_f64)
            .is_none_or(|started_at| started_at > last_action_at);
    if stale_recovery && blocked.is_empty() {
        return None;
    }
    let own_state = own.map(logical_state).unwrap_or(LogicalState::Active);
    let own_reason = own
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty());
    let restricted = state
        .get("logical_agents")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|agents| agents.values())
        .filter(|agent| agent.get("active").and_then(Value::as_bool) != Some(false))
        .filter_map(|agent| {
            let state = logical_state(agent);
            (state != LogicalState::Active).then(|| {
                format!(
                    "{}={state:?}",
                    agent.get("key").and_then(Value::as_str).unwrap_or("agent")
                )
            })
        })
        .take(12)
        .collect::<Vec<_>>();
    let blocked_at = blocked
        .iter()
        .filter_map(|agent| agent.get("last_blocked_at").and_then(Value::as_f64))
        .fold(previous_blocked_at, f64::max);
    let _ = write_atomic_json(
        &marker,
        &json!({"epoch":epoch,"blocked_at":blocked_at,"updated_at":now_epoch()}),
        0o600,
        true,
    );
    let roster = if restricted.is_empty() {
        "none; all logical agents are ACTIVE".to_owned()
    } else {
        restricted.join(", ")
    };
    let reason = own_reason.unwrap_or(if restricted.is_empty() {
        "all logical cushions are released"
    } else {
        "another agent's control state changed"
    });
    let mut notices = Vec::new();
    if new_control && !stale_recovery {
        notices.push(format!(
            "[memory-supervisor] Logical control epoch {epoch}. Your state is {own_state:?} ({reason}). Current restricted roster: {roster}. Keep result/message/status/recovery paths open, do not recreate a restricted worker, and report this control change to the user."
        ));
    }
    if !blocked.is_empty() {
        let entries = blocked
            .iter()
            .map(|agent| {
                let id = agent
                    .get("agent_id")
                    .and_then(Value::as_str)
                    .unwrap_or("subagent");
                let tool = agent
                    .get("last_blocked_tool")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let epoch = agent
                    .get("last_blocked_epoch")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let reason = agent
                    .get("last_blocked_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("memory-supervisor policy");
                let reason: String = reason
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(240)
                    .collect();
                format!("{id}: {tool} denied at logical epoch {epoch} ({reason})")
            })
            .collect::<Vec<_>>()
            .join("; ");
        notices.push(format!(
            "[memory-supervisor] Lead handoff: {entries}. These subagent results may be partial; verify completion and retry only after the current admission and logical state permit it."
        ));
    }
    (!notices.is_empty()).then(|| notices.join(" "))
}

fn pressure_summary(state: &Value) -> String {
    let level = serde_json::to_value(gate_level(state))
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "GREEN".to_owned());
    let utilization = state
        .get("local_utilization")
        .or_else(|| state.get("utilization"))
        .or_else(|| state.get("level"))
        .and_then(Value::as_str)
        .unwrap_or("GREEN");
    let available = state
        .get("admission_mem_available_mb")
        .or_else(|| state.get("mem_available_mb"))
        .map(Value::to_string)
        .unwrap_or_else(|| "?".to_owned());
    let capacity = state
        .get("admission_capacity_mb")
        .or_else(|| state.get("memory_capacity_mb"))
        .map(Value::to_string)
        .unwrap_or_else(|| "?".to_owned());
    let mut summary = format!(
        "admission={level} utilization={utilization} MemAvailable={available}MB capacity={capacity}MB leak_suspects={}",
        list(state.get("leak_suspects")).len()
    );
    if let Some(cap) = state
        .get("admission_cli_hard_cap_mb")
        .or_else(|| state.get("cli_hard_cap_mb"))
        .filter(|value| !value.is_null())
    {
        let used = state
            .get("admission_cli_memory_used_mb")
            .or_else(|| state.get("cli_memory_used_mb"))
            .map(Value::to_string)
            .unwrap_or_else(|| "?".to_owned());
        let status = state
            .get("admission_cli_hard_cap_status")
            .or_else(|| state.get("cli_hard_cap_status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        summary.push_str(&format!(" cli_cap={used}/{cap}MB({status})"));
    }
    if let Some(source) = state.get("admission_source").and_then(Value::as_str)
        && source != "local"
    {
        summary.push_str(&format!(" source={source}"));
    }
    summary
}

fn incident_token(incident: &Value) -> String {
    ["source", "id", "status", "updated_at"]
        .into_iter()
        .map(|key| incident.get(key).map(Value::to_string).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(":")
}

fn incident_context(incident: &Value) -> String {
    let text = |key: &str, fallback: &str| {
        incident
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let pid = incident
        .get("pid")
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".to_owned());
    let evidence = if text("reason", "unknown") == "runaway-memory" {
        let reserve_tte = incident
            .get("process_time_to_reserve_s")
            .map(|value| format!("; projected recovery-reserve arrival={value}s"))
            .unwrap_or_default();
        format!(
            "; direct process evidence={} MiB at {} MiB/s over the configured observation window{}",
            incident
                .get("anon_mb")
                .map(Value::to_string)
                .unwrap_or_else(|| "unknown".to_owned()),
            incident
                .get("slope_mb_s")
                .map(Value::to_string)
                .unwrap_or_else(|| "unknown".to_owned()),
            reserve_tte
        )
    } else {
        String::new()
    };
    format!(
        "Process event: {} {} ({}), PID {}; reason={}{}.",
        text("status", "recorded"),
        text("name", "process"),
        text("role", "unknown role"),
        pid,
        text("reason", "unknown"),
        evidence
    )
}

fn notice_signature(state: &Value) -> String {
    let mut leaks: Vec<_> = list(state.get("leak_suspects"))
        .iter()
        .map(|item| {
            item.get("identity")
                .or_else(|| item.get("pid"))
                .map(Value::to_string)
                .unwrap_or_default()
        })
        .collect();
    leaks.sort();
    let mut stopped: Vec<_> = list(state.get("stopped_pids"))
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    stopped.sort_unstable();
    serde_json::to_string(&json!({"leaks":leaks,"stopped":stopped})).unwrap()
}

/// Returns `(additional_context, system_message)` for a bounded batch of
/// material events or incidents the session lead has not yet seen, or `None`.
/// Subagent hooks must not consume the lead's incident cursor.
fn consume_memory_notice(
    directory: &Path,
    provider: &str,
    payload: &Value,
    state: &Value,
    user_visible: bool,
) -> Option<(String, String)> {
    if agent_id(payload).is_some() {
        return None;
    }
    let session = session_key(payload);
    let all_incidents = list(state.get("recent_incidents"));
    let audience_scoped_incident_exists = all_incidents.iter().any(|item| {
        item.get("audience_provider").is_some() || item.get("audience_sessions").is_some()
    });
    let incidents: Vec<_> = all_incidents
        .iter()
        .filter(|item| {
            if item.get("id").is_none() {
                return false;
            }
            let provider_matches = item
                .get("audience_provider")
                .and_then(Value::as_str)
                .is_none_or(|audience| audience == provider);
            let session_matches = item
                .get("audience_sessions")
                .and_then(Value::as_array)
                .is_none_or(|sessions| {
                    sessions
                        .iter()
                        .any(|value| value.as_str() == Some(session.as_str()))
                });
            provider_matches && session_matches
        })
        .cloned()
        .collect();
    let now = now_epoch();
    let events: Vec<_> = list(state.get("notification_events"))
        .iter()
        .filter(|event| {
            let provider_matches = event
                .get("audience_provider")
                .and_then(Value::as_str)
                .is_none_or(|audience| audience == provider);
            let session_matches = event
                .get("audience_sessions")
                .and_then(Value::as_array)
                .is_none_or(|sessions| {
                    sessions
                        .iter()
                        .any(|value| value.as_str() == Some(session.as_str()))
                });
            event.get("event_id").is_some()
                && provider_matches
                && session_matches
                && now
                    - event
                        .get("created_at")
                        .and_then(Value::as_f64)
                        .unwrap_or(now)
                    <= 3600.0
        })
        .cloned()
        .collect();
    if incidents.is_empty()
        && events.is_empty()
        && list(state.get("leak_suspects")).is_empty()
        && list(state.get("stopped_pids")).is_empty()
    {
        return None;
    }
    let marker = directory.join(format!("seen-{session}.json"));
    let previous = fs::read(&marker)
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .unwrap_or_else(|| json!({}));
    let mut seen_incidents: BTreeSet<String> = list(previous.get("seen_incidents"))
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut seen_events: BTreeSet<String> = list(previous.get("seen_events"))
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let unseen_incidents: Vec<_> = incidents
        .iter()
        .filter(|incident| !seen_incidents.contains(&incident_token(incident)))
        .cloned()
        .collect();
    let all_unseen_events: Vec<_> = events
        .iter()
        .filter(|event| {
            event
                .get("event_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !seen_events.contains(id))
        })
        .cloned()
        .collect();
    let visible_events: Vec<_> = all_unseen_events
        .iter()
        .filter(|event| event_should_notify(event))
        .cloned()
        .collect();
    let selected_events: Vec<_> = visible_events.iter().take(4).collect();
    let selected_event_ids: BTreeSet<_> = selected_events
        .iter()
        .filter_map(|event| event.get("event_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let selected_incidents: Vec<_> = unseen_incidents.iter().take(4).collect();
    let legacy_signature = notice_signature(state);
    let legacy_changed = !list(state.get("stopped_pids")).is_empty()
        && (!incidents.is_empty() || !audience_scoped_incident_exists)
        && previous.get("legacy_signature").and_then(Value::as_str)
            != Some(legacy_signature.as_str());
    if unseen_incidents.is_empty() && all_unseen_events.is_empty() && !legacy_changed {
        return None;
    }
    seen_incidents.extend(
        selected_incidents
            .iter()
            .map(|incident| incident_token(incident)),
    );
    seen_events.extend(
        all_unseen_events
            .iter()
            .filter(|event| {
                !event_should_notify(event)
                    || event
                        .get("event_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| selected_event_ids.contains(id))
            })
            .filter_map(|event| event.get("event_id").and_then(Value::as_str))
            .map(str::to_owned),
    );
    let seen_incidents: Vec<_> = seen_incidents.into_iter().rev().take(256).collect();
    let seen_events: Vec<_> = seen_events.into_iter().rev().take(256).collect();
    let _ = write_atomic_json(
        &marker,
        &json!({
            "seen_incidents":seen_incidents,
            "seen_events":seen_events,
            "legacy_signature":legacy_signature,
            "updated_at":now,
        }),
        0o600,
        true,
    );
    for event in &all_unseen_events {
        if let Some(id) = event.get("event_id").and_then(Value::as_str) {
            let visible = event_should_notify(event);
            if visible && !selected_event_ids.contains(id) {
                continue;
            }
            let status = if visible && user_visible {
                "delivered"
            } else if visible {
                "unavailable"
            } else {
                "skipped"
            };
            let _ = acknowledge_event(directory, id, "hook", status, &session);
        }
    }
    if unseen_incidents.is_empty() && selected_events.is_empty() && !legacy_changed {
        return None;
    }
    let stopped = list(state.get("stopped_pids"))
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut details = selected_events
        .iter()
        .map(|event| {
            (
                event
                    .get("created_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                user_message(event),
            )
        })
        .chain(selected_incidents.iter().map(|incident| {
            (
                incident
                    .get("updated_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                incident_context(incident),
            )
        }))
        .collect::<Vec<_>>();
    if details.is_empty()
        && legacy_changed
        && let Some(incident) = incidents.last()
    {
        details.push((
            incident
                .get("updated_at")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            incident_context(incident),
        ));
    }
    details.sort_by(|left, right| left.0.total_cmp(&right.0));
    let detail = if details.is_empty() {
        "A supervised memory action changed state.".to_owned()
    } else {
        details
            .into_iter()
            .map(|(_, message)| message)
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let active = !list(state.get("stopped_pids")).is_empty()
        || incidents
            .iter()
            .any(|incident| incident.get("status").and_then(Value::as_str) == Some("suspended"));
    let guidance = if active {
        "Do not recreate existing workers. Inspect `memory-status`; resume a paused PID only with `memory-supervisor resume <pid>` in its owning OS."
    } else {
        "No supervisor-managed pause is active. Do not issue another resume; continue under the current admission decision."
    };
    let paused = if stopped.is_empty() { "none" } else { &stopped };
    let context = format!(
        "[memory-supervisor]\nEvent: {detail}\nCurrent: {}.\nPaused PIDs: {paused}.\nRequired behavior: {guidance} Report the action and recovery state to the user.",
        pressure_summary(state)
    );
    Some((context, detail))
}

fn queue_hook_event(
    directory: &Path,
    payload: &Value,
    state: &Value,
    event_type: &str,
    status: &str,
    message: &str,
    user_visible: bool,
) {
    let source = state
        .get("admission_source")
        .or_else(|| state.get("instance"))
        .and_then(Value::as_str)
        .unwrap_or("local");
    let session = session_key(payload);
    let epoch = state
        .get("admission_action_since")
        .or_else(|| state.get("action_since"))
        .or_else(|| state.get("level"))
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".to_owned());
    let event = make_event(
        event_type,
        status,
        message,
        source,
        &format!("{session}:{epoch}"),
        Map::from_iter([
            ("severity".to_owned(), Value::String("warning".to_owned())),
            (
                "cause".to_owned(),
                Value::String("memory-admission".to_owned()),
            ),
            ("importance".to_owned(), Value::String("detail".to_owned())),
        ]),
    );
    let known: BTreeSet<_> = list(state.get("notification_events"))
        .iter()
        .filter_map(|event| event.get("event_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    if queue_event(directory, &event, &known).is_ok()
        && let Some(id) = event.get("event_id").and_then(Value::as_str)
    {
        let _ = acknowledge_event(
            directory,
            id,
            "hook",
            if user_visible {
                "delivered"
            } else {
                "unavailable"
            },
            &session,
        );
    }
}

fn admission_deferred_reason(state: &Value) -> String {
    let level = state
        .get("admission_level")
        .or_else(|| state.get("level"))
        .and_then(Value::as_str)
        .unwrap_or("ORANGE");
    let available = state
        .get("admission_mem_available_mb")
        .or_else(|| state.get("mem_available_mb"))
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".to_owned());
    let capacity = state
        .get("admission_capacity_mb")
        .or_else(|| state.get("memory_capacity_mb"))
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".to_owned());
    let source = state
        .get("admission_source")
        .and_then(Value::as_str)
        .filter(|source| *source != "local")
        .map(|source| format!(" from {source}"))
        .unwrap_or_default();
    format!(
        "[Memory Supervisor] NEW FAN-OUT HELD — ADMISSION_DEFERRED\nWhy: shared admission is {level}{source}; available memory is {available}/{capacity} MiB.\nEffect: this new spawn is blocked, existing work continues, and no process was paused by this decision.\nNext: finish existing workers or use sequential work, then retry after `memory-status` shows GREEN or YELLOW."
    )
}

fn decide_with_session_health(
    provider: &str,
    event: &str,
    payload: &Value,
    state: &Value,
    directory: &Path,
    config: &mut Config,
    session_health: Option<(&str, &str)>,
) -> Option<Value> {
    if matches!(event, "SubagentStart" | "SubagentStop") {
        let line = format!(
            "{:.0} {}\n",
            now_epoch(),
            serde_json::to_string(&json!({
                "event":event,
                "provider":provider,
                "session_id":payload.get("session_id"),
                "agent_id":payload.get("agent_id"),
                "agent_type":payload.get("agent_type"),
            }))
            .unwrap()
        );
        let _ = append_bounded(
            &directory.join("agent-lifecycle.log"),
            &line,
            5 * 1024 * 1024,
        );
    }
    match event {
        "SessionStart" => {
            let notice = consume_memory_notice(directory, provider, payload, state, true);
            let notice_context = notice.as_ref().map(|(context, _)| context.as_str());
            let health_context = session_health.map(|(context, _)| context);
            let user_notice = [
                notice.as_ref().map(|(_, user)| user.as_str()),
                session_health.map(|(_, user)| user),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");
            let notice_user = (!user_notice.is_empty()).then_some(user_notice.as_str());
            let logical = logical_notice(directory, state, provider, payload);
            let source = payload
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !source.is_empty() && source != "startup" {
                let combined = [notice_context, logical.as_deref(), health_context]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                return (!combined.is_empty())
                    .then(|| context("SessionStart", &combined, notice_user));
            }
            let suffix = [notice_context, logical.as_deref(), health_context]
                .into_iter()
                .flatten()
                .map(|message| format!(" {message}"))
                .collect::<String>();
            Some(context(
                "SessionStart",
                &format!(
                    "[memory-supervisor] Active. All visible Claude Code and Codex sessions for this user in the same local process space share one new-work decision. Current: {}. High memory use is fine while useful work can continue safely. Only measured danger delays new work, and pausing an existing program requires stronger evidence that it is causing the problem. A program pause is a separate, clearly reported last resort. Use `memory-status` for details and `memory-supervisor resume <pid>` only for a program paused by Memory Supervisor.{suffix}",
                    pressure_summary(state)
                ),
                notice_user,
            ))
        }
        "PreToolUse" => {
            let class = classify_tool(payload);
            if matches!(class, ToolClass::Expansion | ToolClass::HighMemoryStart)
                && codex_app_surface_gate(state, provider)
            {
                let hold = config.validated_number(
                    "MEMORY_SUPERVISOR_PRETOOL_HOLD_S",
                    PRETOOL_HOLD_DEFAULT_S,
                    Some(0.0),
                    Some(300.0),
                );
                let (recovered, refreshed) = hold_until_codex_app_open(directory, hold);
                if !recovered && codex_app_surface_gate(&refreshed, provider) {
                    let message = "[Memory Supervisor] CODEX APP NEW HEAVY WORK HELD\nWhy: shared App memory is still approaching its calculated recovery boundary.\nEffect: this new high-memory or fan-out start was not run; work already running, results, messages, status, stop and recovery remain available.\nNext: continue delivering the current result, then retry after the App cushion reopens.";
                    queue_hook_event(
                        directory,
                        payload,
                        &refreshed,
                        "codex-app-surface-denial",
                        "blocked",
                        message,
                        true,
                    );
                    return Some(json!({
                        "systemMessage":message,
                        "hookSpecificOutput":{
                            "hookEventName":"PreToolUse",
                            "permissionDecision":"deny",
                            "permissionDecisionReason":message,
                        }
                    }));
                }
            }
            if class == ToolClass::Expansion
                && !matches!(gate_level(state), Level::Green | Level::Yellow)
            {
                let hold = config.validated_number(
                    "MEMORY_SUPERVISOR_PRETOOL_HOLD_S",
                    PRETOOL_HOLD_DEFAULT_S,
                    Some(0.0),
                    Some(300.0),
                );
                let (recovered, refreshed) = hold_until_ok(directory, hold, true);
                if !recovered {
                    let message = "[Memory Supervisor] NEW FAN-OUT HELD\nEffect: This spawn was blocked; existing work continues and no process was paused by this decision.\nNext: Run `memory-status` and retry after admission returns to GREEN or YELLOW.";
                    queue_hook_event(
                        directory,
                        payload,
                        &refreshed,
                        "spawn-denial",
                        "blocked",
                        message,
                        true,
                    );
                    return Some(json!({
                        "systemMessage":message,
                        "hookSpecificOutput":{
                            "hookEventName":"PreToolUse",
                            "permissionDecision":"deny",
                            "permissionDecisionReason":admission_deferred_reason(&refreshed),
                        }
                    }));
                }
            }
            if class == ToolClass::HighMemoryStart && admission_is_braking(state) {
                let hold = config.validated_number(
                    "MEMORY_SUPERVISOR_PRETOOL_HOLD_S",
                    PRETOOL_HOLD_DEFAULT_S,
                    Some(0.0),
                    Some(300.0),
                );
                let (recovered, refreshed) = hold_until_ok(directory, hold, true);
                if !recovered {
                    let message = "[Memory Supervisor] HEAVY START HELD\nEffect: the calculated memory braking boundary is active, so this new high-memory start was not run; existing work continues and no process was paused by this decision.\nNext: run `memory-status` and retry after admission reopens.";
                    queue_hook_event(
                        directory,
                        payload,
                        &refreshed,
                        "heavy-start-denial",
                        "blocked",
                        message,
                        true,
                    );
                    return Some(json!({
                        "systemMessage":message,
                        "hookSpecificOutput":{
                            "hookEventName":"PreToolUse",
                            "permissionDecision":"deny",
                            "permissionDecisionReason":"MEMORY_BRAKING_ACTIVE: the calculated memory boundary currently holds new high-memory work; existing work continues and this call may be retried after admission reopens.",
                        }
                    }));
                }
            }
            if let Some(agent) = logical_agent(state, provider, payload) {
                let logical_state = logical_state(agent);
                if !class.allowed_in(logical_state) {
                    let epoch = agent
                        .get("epoch")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    let reason = agent
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("attributed memory boundary risk");
                    let edit = class == ToolClass::Edit;
                    let message = format!(
                        "[Memory Supervisor] LOGICAL WORK CUSHION ACTIVE\nTarget state: {logical_state:?}, epoch {epoch}.\nWhy: {reason}.\nEffect: this {:?} start was not run; result, message, status, stop, and recovery paths remain open.{}\nNext: follow the current state, then retry only after a newer epoch reopens it.",
                        class,
                        if edit {
                            " The edit was not queued; reread and replan it after recovery."
                        } else {
                            ""
                        }
                    );
                    return Some(json!({
                        "systemMessage":message,
                        "hookSpecificOutput":{
                            "hookEventName":"PreToolUse",
                            "permissionDecision":"deny",
                            "permissionDecisionReason":message,
                        }
                    }));
                }
            }
            logical_notice(directory, state, provider, payload)
                .map(|notice| context("PreToolUse", &notice, None))
        }
        "SubagentStart" => {
            if gate_level(state) != Level::Red {
                return None;
            }
            let hold = config.validated_number(
                "MEMORY_SUPERVISOR_PRETOOL_HOLD_S",
                PRETOOL_HOLD_DEFAULT_S,
                Some(0.0),
                Some(300.0),
            );
            let (recovered, state) = hold_until_ok(directory, hold, true);
            if recovered {
                return None;
            }
            let message = "[Memory Supervisor] WORKER START DELAYED\nEffect: The RED fallback delayed this worker start; the lead remains running.\nNext: Run `memory-status` and avoid additional fan-out until recovery.";
            queue_hook_event(
                directory,
                payload,
                &state,
                "worker-start-delay",
                "delayed",
                message,
                true,
            );
            Some(context(
                "SubagentStart",
                &format!(
                    "[memory-supervisor] RED fallback delayed this worker start. Current: {}. Do not add recursive fan-out; finish or drain existing work first.",
                    pressure_summary(&state)
                ),
                Some(message),
            ))
        }
        "SubagentStop" | "Stop" | "SessionEnd" => None,
        "PostToolBatch" | "PostToolUse" | "AfterTool" => {
            let memory = consume_memory_notice(directory, provider, payload, state, true);
            let memory_context = memory.as_ref().map(|(context, _)| context.as_str());
            let memory_user = memory.as_ref().map(|(_, user)| user.as_str());
            let logical = logical_notice(directory, state, provider, payload);
            let notice = [memory_context, logical.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            (!notice.is_empty()).then(|| {
                context(
                    payload
                        .get("hook_event_name")
                        .and_then(Value::as_str)
                        .unwrap_or(event),
                    &notice,
                    memory_user,
                )
            })
        }
        "UserPromptSubmit" | "BeforeAgent" => {
            if let Some(warning) = fail_open_notice(directory, state) {
                return Some(context(event, &warning, Some(&warning)));
            }
            let memory = consume_memory_notice(directory, provider, payload, state, true);
            let memory_context = memory.as_ref().map(|(context, _)| context.as_str());
            let memory_user = memory.as_ref().map(|(_, user)| user.as_str());
            let logical = logical_notice(directory, state, provider, payload);
            let notice = [memory_context, logical.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            if !notice.is_empty() {
                return Some(context(event, &notice, memory_user));
            }
            if matches!(gate_level(state), Level::Green | Level::Yellow) {
                None
            } else {
                Some(context(
                    event,
                    &format!(
                        "[memory-supervisor] Current: {}. Existing work continues, but do not create new agent/workflow fan-out until admission returns to GREEN or YELLOW. Use `memory-status` for the evidence.",
                        pressure_summary(state)
                    ),
                    None,
                ))
            }
        }
        _ => None,
    }
}

pub fn decide(
    provider: &str,
    event: &str,
    payload: &Value,
    state: &Value,
    directory: &Path,
    config: &mut Config,
) -> Option<Value> {
    decide_with_session_health(provider, event, payload, state, directory, config, None)
}

pub fn run_gate(arguments: &[OsString]) -> i32 {
    if power_is_off() {
        return 0;
    }
    let first = arguments
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let second = arguments.get(1).and_then(|value| value.to_str());
    let directory = state_dir();
    let mut input = String::new();
    let payload = if io::stdin().read_to_string(&mut input).is_ok() {
        serde_json::from_str::<Value>(&input).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let (provider, event) = if matches!(first, "claude" | "codex") {
        (first, second.unwrap_or_default())
    } else {
        (infer_provider(&payload), first)
    };
    let hook_source = hook_source_argument(arguments);
    if provider == "codex"
        && hook_source
            .as_deref()
            .is_some_and(|source| !codex_hook_source_is_authoritative(source))
    {
        return 0;
    }
    let mut config = Config::current();
    let state = safe_state(&directory);
    let session_health = (provider == "codex" && event == "SessionStart")
        .then(|| {
            let source = hook_source.as_deref()?;
            let binary = env::current_exe().ok()?;
            let platform = platform_name();
            let app_route = current_provider_context(provider)
                .is_some_and(|context| context.surface == crate::codex_app::APP_SERVER_SURFACE);
            let health = if app_route {
                audit_codex_app_hooks(source, &binary, &platform)
            } else {
                audit_codex_hooks(source, &binary, &platform)
            };
            health.session_start_notice(if app_route {
                CodexHookSurface::App
            } else {
                CodexHookSurface::Cli
            })
        })
        .flatten();
    let session_health_ref = session_health
        .as_ref()
        .map(|(context, user)| (context.as_str(), user.as_str()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decide_with_session_health(
            provider,
            event,
            &payload,
            &state,
            &directory,
            &mut config,
            session_health_ref,
        )
    }));
    let blocked = result
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .is_some_and(|value| {
            value
                .pointer("/hookSpecificOutput/permissionDecision")
                .and_then(Value::as_str)
                == Some("deny")
        });
    if state.get("fail_open").and_then(Value::as_bool) != Some(true) {
        let block_reason = result
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .and_then(|value| {
                value
                    .pointer("/hookSpecificOutput/permissionDecisionReason")
                    .and_then(Value::as_str)
            });
        observation_queue(
            &directory,
            provider,
            event,
            &payload,
            &state,
            blocked,
            block_reason,
        );
    }
    match result {
        Ok(Some(value)) => println!("{}", serde_json::to_string(&value).unwrap()),
        Ok(None) => {}
        Err(_) => {
            let _ = append_bounded(
                &directory.join("hook-errors.log"),
                &format!("{:.0} panic in Rust gate\n", now_epoch()),
                5 * 1024 * 1024,
            );
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::pending_events;

    fn temp_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "memory-supervisor-gate-{}-{}",
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
    fn orange_spawn_is_denied_but_non_expansion_is_allowed() {
        let directory = temp_directory();
        let state = json!({
            "ts":now_epoch(), "level":"ORANGE", "mem_available_mb":700,
            "memory_capacity_mb":8192, "leak_suspects":[], "stopped_pids":[],
            "recent_incidents":[]
        });
        fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        fs::write(
            directory.join("config.json"),
            br#"{"MEMORY_SUPERVISOR_PRETOOL_HOLD_S":0}"#,
        )
        .unwrap();
        let mut config = Config::load(&directory.join("config.json"));
        let denied = decide(
            "codex",
            "PreToolUse",
            &json!({"tool_name":"spawn_agent","session_id":"s1"}),
            &state,
            &directory,
            &mut config,
        )
        .unwrap();
        assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
        let denied_again = decide(
            "codex",
            "PreToolUse",
            &json!({"tool_name":"spawn_agent","session_id":"s2"}),
            &state,
            &directory,
            &mut config,
        )
        .unwrap();
        assert_eq!(
            denied_again["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        let queued = pending_events(&directory, &BTreeSet::new());
        assert_eq!(queued.len(), 2);
        assert!(queued.iter().all(|event| {
            event["type"] == "spawn-denial"
                && event["importance"] == "detail"
                && !event_should_notify(event)
        }));
        assert!(
            decide(
                "codex",
                "PreToolUse",
                &json!({"tool_name":"apply_patch"}),
                &state,
                &directory,
                &mut config,
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn adaptive_braking_holds_new_high_memory_starts_but_not_observation_or_ordinary_work() {
        let directory = temp_directory();
        let state = json!({
            "ts":now_epoch(), "level":"RED", "distress":"critical",
            "action":"hold", "admission_action":"hold",
            "admission_distress":"critical", "mem_available_mb":400,
            "memory_capacity_mb":8192, "leak_suspects":[], "stopped_pids":[],
            "recent_incidents":[]
        });
        fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        fs::write(
            directory.join("config.json"),
            br#"{"MEMORY_SUPERVISOR_PRETOOL_HOLD_S":0}"#,
        )
        .unwrap();
        let mut config = Config::load(&directory.join("config.json"));
        let denied = decide(
            "claude",
            "PreToolUse",
            &json!({"tool_name":"Bash","tool_input":{"command":"cargo build --release"},"session_id":"s1"}),
            &state,
            &directory,
            &mut config,
        )
        .unwrap();
        assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(
            denied["systemMessage"]
                .as_str()
                .unwrap()
                .contains("HEAVY START HELD")
        );
        assert!(
            decide(
                "claude",
                "PreToolUse",
                &json!({"tool_name":"Bash","tool_input":{"command":"docker ps -a"},"session_id":"s1"}),
                &state,
                &directory,
                &mut config,
            )
            .is_none()
        );
        let critical_observation = json!({
            "ts":now_epoch(), "level":"YELLOW", "distress":"critical",
            "action":"observe", "admission_action":"observe",
            "admission_distress":"critical", "mem_available_mb":8854,
            "memory_capacity_mb":9945, "leak_suspects":[], "stopped_pids":[],
            "recent_incidents":[]
        });
        assert!(
            decide(
                "claude",
                "PreToolUse",
                &json!({"tool_name":"Bash","tool_input":{"command":"cargo build --release"},"session_id":"s1"}),
                &critical_observation,
                &directory,
                &mut config,
            )
            .is_none()
        );
        let elevated = json!({
            "ts":now_epoch(), "level":"ORANGE", "distress":"elevated",
            "action":"observe", "admission_action":"observe",
            "admission_distress":"elevated", "mem_available_mb":900,
            "memory_capacity_mb":8192, "leak_suspects":[], "stopped_pids":[],
            "recent_incidents":[]
        });
        assert!(
            decide(
                "claude",
                "PreToolUse",
                &json!({"tool_name":"Bash","tool_input":{"command":"cargo build"},"session_id":"s1"}),
                &elevated,
                &directory,
                &mut config,
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn codex_app_surface_gate_is_scoped_to_app_commands_only() {
        let state = json!({
            "codex_app": {"control": {"surface_gate": true}}
        });
        assert!(codex_app_surface_gate_for(&state, "codex", true));
        assert!(!codex_app_surface_gate_for(&state, "codex", false));
        assert!(!codex_app_surface_gate_for(&state, "claude", true));
        assert!(!codex_app_surface_gate_for(&json!({}), "codex", true));
    }

    #[test]
    fn blind_app_incident_reaches_only_the_affected_app_leads() {
        let directory = temp_directory();
        let state = json!({
            "level":"RED",
            "recent_incidents":[{
                "id":"app-blind-1", "status":"suspended", "source":"wsl",
                "pid":42, "identity":"42:start", "name":"worker",
                "reason":"app-blind-child-last-resort", "updated_at":now_epoch(),
                "audience_provider":"codex", "audience_sessions":["app-one","app-two"]
            }],
            "stopped_pids":[42]
        });
        assert!(
            consume_memory_notice(
                &directory,
                "codex",
                &json!({"session_id":"app-one"}),
                &state,
                true
            )
            .is_some()
        );
        assert!(
            consume_memory_notice(
                &directory,
                "codex",
                &json!({"session_id":"unrelated"}),
                &state,
                true
            )
            .is_none()
        );
        assert!(
            consume_memory_notice(
                &directory,
                "claude",
                &json!({"session_id":"app-two"}),
                &state,
                true
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_notice_is_injected_once() {
        let directory = temp_directory();
        let state = json!({
            "level":"GREEN", "recent_incidents":[{
                "id":"inc-1","status":"resumed","source":"wsl-host","pid":42,
                "name":"claude","reason":"runaway-memory","updated_at":now_epoch()
            }]
        });
        let payload = json!({"source":"resume","session_id":"lead-1"});
        let mut config = Config::default();
        let first = decide(
            "codex",
            "SessionStart",
            &payload,
            &state,
            &directory,
            &mut config,
        )
        .unwrap();
        let second = decide(
            "codex",
            "SessionStart",
            &payload,
            &state,
            &directory,
            &mut config,
        );
        assert!(
            first["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("Process event: resumed claude")
        );
        assert!(second.is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lead_receives_bounded_incident_batches_without_events_consuming_them() {
        let directory = temp_directory();
        let now = now_epoch();
        let incidents = (1..=5)
            .map(|index| {
                json!({
                    "id":format!("incident-{index}"),
                    "status":"gone",
                    "source":"test",
                    "pid":40 + index,
                    "name":format!("worker-{index}"),
                    "role":"worker",
                    "reason":"pressure-pause",
                    "updated_at":now - (5 - index) as f64,
                })
            })
            .collect::<Vec<_>>();
        let state = json!({
            "level":"GREEN",
            "recent_incidents":incidents,
            "stopped_pids":[],
            "notification_events":[{
                "event_id":"episode-start",
                "type":"pressure-episode",
                "status":"active",
                "importance":"important",
                "message":"[Memory Supervisor] MEMORY PROTECTION ACTIVE",
                "created_at":now - 10.0,
            }],
        });
        let payload = json!({"session_id":"lead-batched-incidents"});

        let first = consume_memory_notice(&directory, "codex", &payload, &state, true)
            .unwrap()
            .0;
        assert!(first.contains("MEMORY PROTECTION ACTIVE"));
        for index in 1..=4 {
            assert!(first.contains(&format!("worker-{index}")));
        }
        assert!(!first.contains("worker-5"));

        let second = consume_memory_notice(&directory, "codex", &payload, &state, true)
            .unwrap()
            .0;
        assert!(!second.contains("MEMORY PROTECTION ACTIVE"));
        assert!(!second.contains("worker-1"));
        assert!(second.contains("worker-5"));
        assert!(consume_memory_notice(&directory, "codex", &payload, &state, true).is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stale_red_state_fails_open() {
        let directory = temp_directory();
        fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&json!({"ts":now_epoch()-60.0,"level":"RED"})).unwrap(),
        )
        .unwrap();
        assert_eq!(safe_state(&directory)["level"], "GREEN");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn session_prompt_and_action_event_contracts_match_provider_boundaries() {
        let directory = temp_directory();
        let mut config = Config::default();
        let green = json!({
            "level":"GREEN", "mem_available_mb":4096, "memory_capacity_mb":8192,
            "leak_suspects":[], "stopped_pids":[], "recent_incidents":[]
        });
        let startup = decide(
            "claude",
            "SessionStart",
            &json!({"source":"startup","session_id":"quote-'\""}),
            &green,
            &directory,
            &mut config,
        )
        .unwrap();
        assert!(
            startup["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("memory-status")
        );
        let setup = decide_with_session_health(
            "codex",
            "SessionStart",
            &json!({"source":"startup","session_id":"setup-gap"}),
            &green,
            &directory,
            &mut config,
            Some((
                "Tell the user now: SubagentStop is disabled.",
                "[Memory Supervisor] PROTECTION SETUP NEEDS ATTENTION",
            )),
        )
        .unwrap();
        assert!(
            setup["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("SubagentStop is disabled")
        );
        assert_eq!(
            setup["systemMessage"],
            "[Memory Supervisor] PROTECTION SETUP NEEDS ATTENTION"
        );
        assert!(
            decide(
                "claude",
                "SessionStart",
                &json!({"source":"resume"}),
                &green,
                &directory,
                &mut config,
            )
            .is_none()
        );

        let yellow = json!({"level":"YELLOW"});
        assert!(
            decide(
                "claude",
                "UserPromptSubmit",
                &json!({}),
                &yellow,
                &directory,
                &mut config,
            )
            .is_none()
        );
        let orange = json!({"level":"ORANGE"});
        let guidance = decide(
            "claude",
            "UserPromptSubmit",
            &json!({}),
            &orange,
            &directory,
            &mut config,
        )
        .unwrap();
        assert!(
            guidance["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("GREEN or YELLOW")
        );

        let message = "[Memory Supervisor] GUARDED RESUME\nState: same PID under probation.";
        let event_state = json!({
            "level":"GREEN",
            "notification_events":[{
                "event_id":"event-1", "type":"lead-probation", "status":"monitoring",
                "message":message, "created_at":now_epoch()
            }]
        });
        let payload = json!({"session_id":"lead-event-session"});
        let first = decide(
            "claude",
            "UserPromptSubmit",
            &payload,
            &event_state,
            &directory,
            &mut config,
        )
        .unwrap();
        assert!(
            first["systemMessage"]
                .as_str()
                .unwrap()
                .contains("GUARDED RESUME")
        );
        assert!(
            first["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("State: same PID under probation")
        );
        assert!(
            !first["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("event_id")
        );
        assert!(
            decide(
                "claude",
                "UserPromptSubmit",
                &payload,
                &event_state,
                &directory,
                &mut config,
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn subagent_hooks_cannot_consume_the_leads_episode_notice() {
        let directory = temp_directory();
        let state = json!({
            "level":"GREEN", "recent_incidents":[], "stopped_pids":[],
            "logical_agents":{
                "claude:workflow-1:worker-1":{
                    "key":"claude:workflow-1:worker-1", "provider":"claude",
                    "session_id":"workflow-1", "agent_id":"worker-1",
                    "active":false, "state":"ACTIVE", "started_at":1.0,
                    "last_blocked_at":2.0, "last_blocked_tool":"bash",
                    "last_blocked_epoch":4,
                    "last_blocked_reason":"HANDOFF_ONLY denied future work"
                }
            },
            "notification_events":[
                {
                    "event_id":"episode-start", "type":"pressure-episode",
                    "status":"active", "importance":"important",
                    "message":"[Memory Supervisor] MEMORY PROTECTION ACTIVE\nWhy: test danger.",
                    "created_at":now_epoch()-2.0
                },
                {
                    "event_id":"episode-end", "type":"pressure-episode",
                    "status":"recovered", "importance":"important",
                    "message":"[Memory Supervisor] MEMORY PROTECTION RECOVERED\nWhy: test recovery.",
                    "created_at":now_epoch()-1.0
                }
            ]
        });
        let mut config = Config::default();
        let subagent = json!({
            "session_id":"workflow-1", "agent_id":"worker-1",
            "tool_name":"Read"
        });
        assert!(
            decide(
                "claude",
                "PostToolUse",
                &subagent,
                &state,
                &directory,
                &mut config,
            )
            .is_none()
        );
        assert!(!directory.join("seen-workflow-1.json").exists());

        let lead = decide(
            "claude",
            "UserPromptSubmit",
            &json!({"session_id":"workflow-1"}),
            &state,
            &directory,
            &mut config,
        )
        .unwrap();
        let context = lead["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("MEMORY PROTECTION ACTIVE"));
        assert!(context.contains("MEMORY PROTECTION RECOVERED"));
        assert!(context.contains("Lead handoff"));
        assert!(context.contains("worker-1: bash denied at logical epoch 4"));
        assert!(
            context.find("MEMORY PROTECTION ACTIVE") < context.find("MEMORY PROTECTION RECOVERED")
        );
        assert!(
            decide(
                "claude",
                "UserPromptSubmit",
                &json!({"session_id":"workflow-1"}),
                &state,
                &directory,
                &mut config,
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cached_peer_malformed_collections_and_leak_only_state_keep_admission_exact() {
        let cached = json!({
            "level":"GREEN", "action":"allow", "local_admission_level":"GREEN",
            "admission_level":"RED", "admission_source":"windows-host",
            "admission_mem_available_mb":300, "admission_capacity_mb":32768,
            "admission_action":"drain", "admission_distress":"critical",
            "admission_attribution":"external", "admission_cli_hard_cap_mb":4096,
            "admission_cli_memory_used_mb":4200, "admission_cli_hard_cap_status":"exceeded"
        });
        let effective = effective_admission_state(&cached, Some(Vec::new()));
        assert_eq!(effective["local_admission_level"], "GREEN");
        assert_eq!(effective["admission_source"], "windows-host");
        assert_eq!(effective["admission_action"], "drain");
        assert!(pressure_summary(&effective).contains("cli_cap=4200/4096MB(exceeded)"));

        let malformed_peer = json!({
            "level":"ORANGE", "instance":"malformed-peer",
            "leak_suspects":null, "stopped_pids":"unexpected", "recent_incidents":null
        });
        let effective =
            effective_admission_state(&json!({"level":"GREEN"}), Some(vec![malformed_peer]));
        assert_eq!(effective["admission_level"], "ORANGE");
        assert_eq!(effective["admission_source"], "malformed-peer");

        let fail_open = effective_admission_state(
            &json!({
                "level":"GREEN", "fail_open":true,
                "reason":"missing-stale-or-invalid"
            }),
            Some(vec![json!({
                "level":"GREEN", "instance":"healthy-peer",
                "mem_available_mb":1024, "memory_capacity_mb":8192
            })]),
        );
        assert_eq!(fail_open["admission_source"], "healthy-peer");
        assert_eq!(fail_open["fail_open"], true);
        assert_eq!(fail_open["reason"], "missing-stale-or-invalid");

        let leak_only = effective_admission_state(
            &json!({"level":"GREEN","leak_suspects":[{"pid":42,"identity":"42:x"}]}),
            Some(Vec::new()),
        );
        assert_eq!(leak_only["admission_level"], "GREEN");
    }

    #[test]
    fn red_start_fallback_and_lifecycle_are_bounded() {
        let directory = temp_directory();
        let config_path = directory.join("config.json");
        fs::write(&config_path, br#"{"MEMORY_SUPERVISOR_PRETOOL_HOLD_S":0}"#).unwrap();
        let mut config = Config::load(&config_path);
        let red = json!({
            "ts":now_epoch(), "level":"RED", "admission_attribution":"agent",
            "admission_action_since":125.0, "mem_available_mb":300,
            "memory_capacity_mb":8192, "recent_incidents":[]
        });
        fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&red).unwrap(),
        )
        .unwrap();
        let fallback = decide(
            "claude",
            "SubagentStart",
            &json!({"session_id":"red-worker","agent_id":"a1"}),
            &red,
            &directory,
            &mut config,
        )
        .unwrap();
        assert!(
            fallback["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("RED fallback delayed")
        );
        assert!(
            decide(
                "claude",
                "SubagentStop",
                &json!({"session_id":"s1","agent_id":"a1","agent_type":"worker"}),
                &red,
                &directory,
                &mut config,
            )
            .is_none()
        );
        assert!(
            fs::read_to_string(directory.join("agent-lifecycle.log"))
                .unwrap()
                .contains("\"agent_id\":\"a1\"")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn logical_ladder_blocks_only_the_named_future_work_and_never_queues_edits() {
        let directory = temp_directory();
        let key = "codex:s1:a1";
        let base = json!({
            "level":"GREEN",
            "logical_epoch":7,
            "logical_agents":{
                (key):{
                    "key":key,"provider":"codex","session_id":"s1","agent_id":"a1",
                    "active":true,"epoch":7,"reason":"test boundary",
                    "state":"LIGHT_WORK_ONLY"
                }
            }
        });
        let mut config = Config::default();
        let build = decide(
            "codex",
            "PreToolUse",
            &json!({
                "session_id":"s1","agent_id":"a1","tool_name":"exec_command",
                "tool_input":{"cmd":"cargo test --all-targets"}
            }),
            &base,
            &directory,
            &mut config,
        )
        .unwrap();
        assert_eq!(build["hookSpecificOutput"]["permissionDecision"], "deny");
        let edit = decide(
            "codex",
            "PreToolUse",
            &json!({"session_id":"s1","agent_id":"a1","tool_name":"apply_patch"}),
            &base,
            &directory,
            &mut config,
        );
        assert_ne!(
            edit.as_ref()
                .and_then(|value| value.pointer("/hookSpecificOutput/permissionDecision"))
                .and_then(Value::as_str),
            Some("deny")
        );

        let mut handoff = base;
        handoff["logical_epoch"] = json!(8);
        handoff["logical_agents"][key]["epoch"] = json!(8);
        handoff["logical_agents"][key]["state"] = json!("HANDOFF_ONLY");
        let denied_edit = decide(
            "codex",
            "PreToolUse",
            &json!({"session_id":"s1","agent_id":"a1","tool_name":"apply_patch"}),
            &handoff,
            &directory,
            &mut config,
        )
        .unwrap();
        assert!(
            denied_edit["systemMessage"]
                .as_str()
                .unwrap()
                .contains("not queued")
        );
        let message = decide(
            "codex",
            "PreToolUse",
            &json!({"session_id":"s1","agent_id":"a1","tool_name":"send_message"}),
            &handoff,
            &directory,
            &mut config,
        );
        assert_ne!(
            message
                .as_ref()
                .and_then(|value| value.pointer("/hookSpecificOutput/permissionDecision"))
                .and_then(Value::as_str),
            Some("deny")
        );
        let structured = decide(
            "codex",
            "PreToolUse",
            &json!({
                "session_id":"s1", "agent_id":"a1",
                "tool_name":"StructuredOutput"
            }),
            &handoff,
            &directory,
            &mut config,
        );
        assert_ne!(
            structured
                .as_ref()
                .and_then(|value| value.pointer("/hookSpecificOutput/permissionDecision"))
                .and_then(Value::as_str),
            Some("deny")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lead_receives_a_recorded_subagent_block_after_the_control_epoch() {
        let directory = temp_directory();
        let payload = json!({"session_id":"workflow-1"});
        let mut state = json!({
            "logical_epoch":7, "logical_restricted_count":0,
            "logical_last_action_at":2.0,
            "logical_agents":{
                "claude:workflow-1:root":{
                    "key":"claude:workflow-1:root", "provider":"claude",
                    "session_id":"workflow-1", "role":"lead", "active":true,
                    "state":"ACTIVE", "started_at":1.0, "reason":""
                }
            }
        });
        let first = logical_notice(&directory, &state, "claude", &payload).unwrap();
        assert!(first.contains("Logical control epoch 7"));

        state["logical_agents"]["claude:workflow-1:worker-1"] = json!({
            "key":"claude:workflow-1:worker-1", "provider":"claude",
            "session_id":"workflow-1", "agent_id":"worker-1", "role":"subagent",
            "active":false, "state":"ACTIVE", "started_at":1.5,
            "last_blocked_at":3.0, "last_blocked_tool":"bash",
            "last_blocked_epoch":7,
            "last_blocked_reason":"HANDOFF_ONLY denied future work"
        });
        let handoff = logical_notice(&directory, &state, "claude", &payload).unwrap();
        assert!(handoff.contains("Lead handoff"));
        assert!(handoff.contains("worker-1: bash denied at logical epoch 7"));
        assert!(handoff.contains("HANDOFF_ONLY denied future work"));
        assert!(logical_notice(&directory, &state, "claude", &payload).is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lifecycle_inventory_epoch_never_becomes_an_active_notice() {
        let directory = temp_directory();
        let payload = json!({"session_id":"current"});
        let lifecycle_only = json!({
            "logical_epoch":13,
            "logical_restricted_count":0,
            "logical_last_action_at":0.0,
            "logical_agents":{
                "codex:current:root":{
                    "key":"codex:current:root","provider":"codex","session_id":"current",
                    "active":true,"state":"ACTIVE","epoch":0,"reason":"","started_at":0.5
                }
            }
        });
        assert!(logical_notice(&directory, &lifecycle_only, "codex", &payload).is_none());

        let mut real_recovery = lifecycle_only;
        real_recovery["logical_epoch"] = json!(14);
        real_recovery["logical_last_action_at"] = json!(1.0);
        let notice = logical_notice(&directory, &real_recovery, "codex", &payload).unwrap();
        assert!(notice.contains("all logical cushions are released"));
        assert!(!notice.contains("Active ()"));
        assert!(
            logical_notice(
                &directory,
                &real_recovery,
                "codex",
                &json!({"session_id":"created-after-recovery"})
            )
            .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
