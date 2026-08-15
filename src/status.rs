use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{
    load_notification_config, notification_channels, notification_config_path, power_is_off,
    state_dir,
};
use crate::events::{event_should_notify, user_message};
use crate::integration::{
    CodexHookSurface, audit_codex_hooks, current_codex_hook_target, discover_claude_with_path,
    verify_hooks,
};
use crate::platform::{
    admission_level_for_peer, admission_level_for_state, federation_dir, fresh_federated_states,
    platform_name, process_state,
};
use crate::policy::Level;

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Green => "GREEN",
        Level::Yellow => "YELLOW",
        Level::Orange => "ORANGE",
        Level::Red => "RED",
    }
}

fn string(value: &Value, key: &str, default: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn restricted_logical_agents(value: &Value) -> Vec<&Value> {
    let mut agents: Vec<_> = value
        .get("logical_agents")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|agents| agents.values())
        .filter(|agent| {
            agent.get("active").and_then(Value::as_bool) == Some(true)
                && !matches!(
                    agent.get("state").and_then(Value::as_str),
                    None | Some("ACTIVE")
                )
        })
        .collect();
    agents.sort_by_key(|agent| display(agent.get("key"), "?"));
    agents
}

fn positive_pid(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|pid| *pid > 1)
}

fn rank(state: &Value, is_peer: bool) -> (Level, i64) {
    let available = finite_number(
        state
            .get("admission_mem_available_mb")
            .or_else(|| state.get("mem_available_mb")),
    );
    let capacity = finite_number(
        state
            .get("admission_capacity_mb")
            .or_else(|| state.get("memory_capacity_mb")),
    );
    let headroom = match (available, capacity) {
        (Some(available), Some(capacity)) if capacity > 0.0 => available / capacity,
        _ => f64::INFINITY,
    };
    let scaled = if headroom.is_finite() {
        -(headroom * 1_000_000.0).round() as i64
    } else {
        i64::MIN
    };
    let level = if is_peer {
        admission_level_for_peer(state)
    } else {
        admission_level_for_state(state)
    };
    (level, scaled)
}

fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

fn powered_off_status(path: &Path) -> Value {
    let notification_config = load_notification_config(&notification_config_path());
    json!({
        "ok": true,
        "running": false,
        "fresh": false,
        "power": "off",
        "enabled": false,
        "level": "GREEN",
        "admission_level": "GREEN",
        "action": "allow",
        "state_path": path.display().to_string(),
        "notification_channels": notification_channels(&notification_config),
        "stopped_pids": [],
        "recent_incidents": [],
    })
}

pub fn read_status_at(path: &Path, peers: Vec<Value>, now: f64) -> Value {
    let mut result = Map::from_iter([
        ("ok".to_owned(), Value::Bool(false)),
        ("running".to_owned(), Value::Bool(false)),
        ("fresh".to_owned(), Value::Bool(false)),
        (
            "state_path".to_owned(),
            Value::String(path.display().to_string()),
        ),
    ]);
    let source = match fs::read(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            result.insert(
                "error".to_owned(),
                Value::String("state file not found".to_owned()),
            );
            return Value::Object(result);
        }
        Err(error) => {
            result.insert(
                "error".to_owned(),
                Value::String(format!("state unreadable: {error}")),
            );
            return Value::Object(result);
        }
    };
    let state: Value = match serde_json::from_slice(&source) {
        Ok(Value::Object(state)) => Value::Object(state),
        Ok(_) => {
            result.insert(
                "error".to_owned(),
                Value::String(
                    "state unreadable: top-level JSON value must be an object".to_owned(),
                ),
            );
            return Value::Object(result);
        }
        Err(error) => {
            result.insert(
                "error".to_owned(),
                Value::String(format!("state unreadable: {error}")),
            );
            return Value::Object(result);
        }
    };
    let Some(timestamp) = finite_number(state.get("ts")) else {
        result.insert(
            "error".to_owned(),
            Value::String("state unreadable: invalid timestamp".to_owned()),
        );
        return Value::Object(result);
    };
    let age = now - timestamp;
    let fresh = (-5.0..=10.0).contains(&age);
    result.extend(state.as_object().unwrap().clone());
    let healthy = fresh
        && state.get("error").is_none_or(Value::is_null)
        && state.get("sensor_ok").and_then(Value::as_bool) != Some(false)
        && state.get("configuration_error").is_none_or(Value::is_null)
        && !state
            .get("protection_degraded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    result.insert("ok".to_owned(), Value::Bool(healthy));
    result.insert("running".to_owned(), Value::Bool(fresh));
    result.insert("fresh".to_owned(), Value::Bool(fresh));
    result.insert("age_s".to_owned(), json!((age * 10.0).round() / 10.0));
    result.insert(
        "state_path".to_owned(),
        Value::String(path.display().to_string()),
    );

    let mut candidates = Vec::with_capacity(peers.len() + 1);
    candidates.push(state.clone());
    candidates.extend(peers);
    let worst_index = candidates
        .iter()
        .enumerate()
        .max_by(|(left_index, left), (right_index, right)| {
            rank(left, *left_index > 0)
                .cmp(&rank(right, *right_index > 0))
                .then_with(|| right_index.cmp(left_index))
        })
        .map(|(index, _)| index)
        .unwrap_or_default();
    let worst = &candidates[worst_index];
    let cached = worst_index == 0;
    let admission_level = if cached {
        admission_level_for_state(worst)
    } else {
        admission_level_for_peer(worst)
    };
    result.insert(
        "admission_level".to_owned(),
        Value::String(level_name(admission_level).to_owned()),
    );
    let source_name = if cached {
        string(&state, "admission_source", "local")
    } else {
        string(worst, "instance", &string(worst, "platform", "peer"))
    };
    result.insert("admission_source".to_owned(), Value::String(source_name));
    for (output, cached_key, fallback) in [
        (
            "admission_mem_available_mb",
            "admission_mem_available_mb",
            "mem_available_mb",
        ),
        (
            "admission_capacity_mb",
            "admission_capacity_mb",
            "memory_capacity_mb",
        ),
        ("admission_action", "admission_action", "action"),
        ("admission_distress", "admission_distress", "distress"),
        (
            "admission_attribution",
            "admission_attribution",
            "attribution",
        ),
        (
            "admission_time_to_exhaustion_s",
            "admission_time_to_exhaustion_s",
            "time_to_exhaustion_s",
        ),
        (
            "admission_time_to_recovery_reserve_s",
            "admission_time_to_recovery_reserve_s",
            "time_to_recovery_reserve_s",
        ),
    ] {
        let value = if cached {
            worst
                .get(cached_key)
                .or_else(|| worst.get(fallback))
                .cloned()
        } else {
            worst.get(fallback).cloned()
        };
        result.insert(output.to_owned(), value.unwrap_or(Value::Null));
    }
    for key in [
        "cli_hard_cap_mb",
        "cli_memory_used_mb",
        "cli_hard_cap_remaining_mb",
        "cli_hard_cap_status",
        "cli_hard_cap_driving",
    ] {
        let value = if cached {
            worst
                .get(format!("admission_{key}"))
                .or_else(|| worst.get(key))
                .cloned()
        } else {
            worst.get(key).cloned()
        };
        result.insert(format!("admission_{key}"), value.unwrap_or(Value::Null));
    }
    let platform = string(&state, "platform", &platform_name());
    let stopped_processes: Vec<_> = array(&state, "stopped_pids")
        .iter()
        .filter_map(positive_pid)
        .map(|pid| json!({"pid": pid, "state": process_state(&platform, pid)}))
        .collect();
    result.insert("stopped_processes".to_owned(), json!(stopped_processes));
    let notification_config = load_notification_config(&notification_config_path());
    result.insert(
        "notification_channels".to_owned(),
        json!(notification_channels(&notification_config)),
    );
    Value::Object(result)
}

pub fn read_status() -> Value {
    let path = state_path();
    if power_is_off() {
        powered_off_status(&path)
    } else {
        read_status_at(&path, fresh_federated_states(10.0), now_epoch())
    }
}

fn incident_for_pid(incidents: &[Value], pid: u32) -> Option<&Value> {
    incidents
        .iter()
        .rev()
        .find(|incident| incident.get("pid").and_then(positive_pid) == Some(pid))
}

fn display(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => default.to_owned(),
        Some(value) => value.to_string(),
    }
}

pub fn human(status: &Value) -> String {
    if status.get("power").and_then(Value::as_str) == Some("off") {
        return "memory-supervisor: OFF\nprotection: intentionally disabled; connected AI CLI hooks pass through without restricting work\nnext: memory-supervisor on".to_owned();
    }
    if status.get("running").and_then(Value::as_bool) != Some(true) {
        return format!(
            "memory-supervisor: NOT RUNNING\nstate: {}\nreason: {}",
            display(status.get("state_path"), "unknown"),
            display(status.get("error"), "state is stale")
        );
    }
    let stopped: Vec<_> = array(status, "stopped_pids")
        .iter()
        .filter_map(positive_pid)
        .collect();
    let incidents: Vec<_> = array(status, "recent_incidents")
        .iter()
        .filter(|item| item.is_object())
        .cloned()
        .collect();
    let events: Vec<_> = array(status, "notification_events")
        .iter()
        .filter(|item| item.is_object())
        .cloned()
        .collect();
    let probation = status.get("probation").filter(|value| value.is_object());
    let mut lines = Vec::new();
    if !stopped.is_empty() {
        lines.push("[Memory Supervisor] PROCESS PAUSED".to_owned());
        lines.push(format!(
            "Meaning: {} CLI process(es) are reversibly paused. This is not a terminal crash; each PID and in-memory session is preserved.",
            stopped.len()
        ));
        for pid in &stopped {
            let incident = incident_for_pid(&incidents, *pid).unwrap_or(&Value::Null);
            let role = display(incident.get("role"), "process");
            let reason_key = display(incident.get("reason"), "unknown");
            let reason = match reason_key.as_str() {
                "runaway-memory" => {
                    "material process memory kept growing across the full observation window and crossed the adaptive process threshold"
                }
                "pressure-pause" => {
                    "machine headroom approached exhaustion; one worker was selected for minimum containment"
                }
                "pressure-lead-last-resort" => {
                    "machine headroom approached exhaustion and the lead was the last eligible containment target"
                }
                "hard-cap-pause" => "tracked CLI memory reached the explicit hard cap",
                "hard-cap-lead-last-resort" => {
                    "tracked CLI memory reached the explicit hard cap and the lead was the last eligible target"
                }
                _ => reason_key.as_str(),
            };
            let attribution_key = incident
                .get("attribution")
                .or_else(|| status.get("attribution"));
            let attribution = match attribution_key.and_then(Value::as_str).unwrap_or("unknown") {
                "agent" => "agent activity likely dominates",
                "external" => "external activity likely dominates",
                "mixed" => "agent and external activity both contribute",
                _ => "not attributable from current evidence",
            };
            let recovery_key = display(incident.get("recovery_policy"), "manual");
            let recovery = if probation.is_some_and(|probation| {
                probation.get("pid").and_then(positive_pid) == Some(*pid)
                    && probation.get("status").and_then(Value::as_str) == Some("failed")
            }) {
                "Guarded resume failed; manual owner decision required".to_owned()
            } else {
                match recovery_key.as_str() {
                    "lead-probation" => {
                        "one guarded automatic resume after sustained recovery".to_owned()
                    }
                    "automatic-pressure-recovery" => {
                        "automatic one-at-a-time resume after sustained recovery".to_owned()
                    }
                    "lead-or-owner" => {
                        "manual resume after the lead or owner reviews the cause".to_owned()
                    }
                    _ => recovery_key,
                }
            };
            let tte = incident
                .get("time_to_exhaustion_s")
                .or_else(|| status.get("time_to_exhaustion_s"));
            lines.push(format!(
                "Target: {} ({role}), PID {}",
                display(incident.get("name"), "?"),
                pid
            ));
            let process_projection = incident
                .get("process_time_to_reserve_s")
                .filter(|value| !value.is_null())
                .map(|value| {
                    format!(
                        " Projected time to the automatic recovery reserve: {} seconds.",
                        display(Some(value), "?")
                    )
                })
                .unwrap_or_default();
            lines.push(format!("Why: {reason}.{process_projection}"));
            lines.push(format!(
                "Machine attribution estimate: {attribution}. Exhaustion estimate: {}.",
                tte.map(|value| format!("{} seconds", display(Some(value), "?")))
                    .unwrap_or_else(|| "not currently calculable".to_owned())
            ));
            lines.push(format!("Recovery: {recovery}."));
            lines.push(format!("Manual command: memory-supervisor resume {pid}"));
            lines.push(format!(
                "Terminal delivery: {} ({})",
                display(
                    incident
                        .get("last_terminal_notice")
                        .or_else(|| incident.get("terminal_notice")),
                    "unknown"
                ),
                display(
                    incident
                        .get("last_terminal_notice_reason")
                        .or_else(|| incident.get("terminal_notice_reason")),
                    "no-detail"
                )
            ));
        }
        if stopped.len() == 1 {
            lines.push(
                "Shortcut: memory-supervisor resume (selects the only paused PID)".to_owned(),
            );
        }
        lines.push(String::new());
    } else if probation.and_then(|value| value.get("status").and_then(Value::as_str))
        == Some("monitoring")
    {
        let probation = probation.unwrap();
        lines.extend([
            "[Memory Supervisor] GUARDED RESUME".to_owned(),
            format!(
                "State: lead PID {} is running under one-time probation; new fan-out remains held.",
                display(probation.get("pid"), "?")
            ),
            "Next: preserve work and avoid expansion. Renewed growth or pressure pauses the same PID again."
                .to_owned(),
            String::new(),
        ]);
    } else if let Some(event) = events.iter().rev().find(|event| {
        let age = now_epoch() - finite_number(event.get("created_at")).unwrap_or(f64::NEG_INFINITY);
        (-5.0..=3600.0).contains(&age)
            && event.get("role").and_then(Value::as_str) == Some("lead")
            && matches!(
                event.get("status").and_then(Value::as_str),
                Some("monitoring" | "resumed")
            )
            && event.get("message").and_then(Value::as_str).is_some()
    }) {
        lines.push("[Memory Supervisor] LATEST LEAD RECOVERY EVENT".to_owned());
        lines.extend(user_message(event).lines().map(str::to_owned));
        lines.push(String::new());
    } else if !restricted_logical_agents(status).is_empty() {
        let restricted = restricted_logical_agents(status);
        lines.extend([
            "[Memory Supervisor] LOGICAL CUSHION ACTIVE".to_owned(),
            format!(
                "Meaning: {} logical agent(s) have only named future work restricted; their CLI process is still running and result delivery remains open.",
                restricted.len()
            ),
            format!(
                "Authoritative epoch: {}. The lead receives this same roster at its next hook boundary.",
                display(status.get("logical_epoch"), "0")
            ),
        ]);
        for agent in restricted {
            lines.push(format!(
                "Target: {} | state={} | role={} | reason={}",
                display(agent.get("key"), "?"),
                display(agent.get("state"), "?"),
                display(agent.get("role"), "?"),
                display(agent.get("reason"), "attributed boundary risk")
            ));
        }
        lines.extend([
            "Next: continue allowed work and hand off results; the supervisor remeasures before any further step and reopens one step at a time after recovery."
                .to_owned(),
            String::new(),
        ]);
    } else if matches!(
        status
            .get("admission_cli_hard_cap_status")
            .and_then(Value::as_str),
        Some("near" | "exceeded")
    ) {
        let exceeded = status["admission_cli_hard_cap_status"] == "exceeded";
        lines.extend([
            if exceeded {
                "[Memory Supervisor] EXPLICIT CLI HARD CAP REACHED"
            } else {
                "[Memory Supervisor] EXPLICIT CLI HARD CAP NEAR"
            }
            .to_owned(),
            format!(
                "Evidence: tracked CLI memory={} MiB; hard cap={} MiB; source={}.",
                display(status.get("admission_cli_memory_used_mb"), "?"),
                display(status.get("admission_cli_hard_cap_mb"), "?"),
                display(status.get("admission_source"), "local")
            ),
            if exceeded {
                "Effect: new fan-out is blocked; any exact-PID pause is reported separately. Admission reopens after sustained recovery."
            } else {
                "Effect: new fan-out is held before the predicted burst crosses the cap; existing work continues."
            }
                .to_owned(),
            String::new(),
        ]);
    } else if matches!(
        status.get("admission_level").and_then(Value::as_str),
        Some("ORANGE" | "RED")
    ) {
        lines.extend([
            "[Memory Supervisor] NEW FAN-OUT HELD".to_owned(),
            format!(
                "Decision: action={} · distress={} · attribution estimate={}.",
                display(
                    status
                        .get("admission_action")
                        .or_else(|| status.get("action")),
                    "?"
                ),
                display(
                    status
                        .get("admission_distress")
                        .or_else(|| status.get("distress")),
                    "?"
                ),
                display(
                    status
                        .get("admission_attribution")
                        .or_else(|| status.get("attribution")),
                    "?"
                )
            ),
            "Effect: existing work continues. A process pause, if any, is a separate reported action."
                .to_owned(),
            String::new(),
        ]);
    } else {
        lines.extend([
            "[Memory Supervisor] NORMAL".to_owned(),
            "Effect: existing work and new fan-out are allowed within the current adaptive safety margin."
                .to_owned(),
            String::new(),
        ]);
    }

    if status.get("admission_source").and_then(Value::as_str) != Some("local") {
        lines.push(format!(
            "Shared admission: {} (source: {})",
            display(status.get("admission_level"), "?"),
            display(status.get("admission_source"), "?")
        ));
    }
    lines.extend([
        format!(
            "State: utilization={} | admission={} | platform={}",
            display(
                status.get("utilization").or_else(|| status.get("level")),
                "UNKNOWN"
            ),
            display(
                status
                    .get("admission_level")
                    .or_else(|| status.get("level")),
                "UNKNOWN"
            ),
            display(status.get("platform"), "?")
        ),
        {
            let topology = crate::topology::detect();
            format!("Co-tenancy: {} — {}", topology.name(), topology.describe())
        },
        format!(
            "Memory: available={} MiB / capacity={} MiB ({}) | sample age={}s",
            display(status.get("mem_available_mb"), "?"),
            display(status.get("memory_capacity_mb"), "?"),
            display(status.get("memory_capacity_source"), "?"),
            display(status.get("age_s"), "?")
        ),
        format!(
            "Tracked CLI processes: roots={} children={} RSS={} MiB",
            display(status.get("tracked_roots"), "0"),
            display(status.get("tracked_children"), "0"),
            display(status.get("tracked_total_rss_mb"), "0")
        ),
        format!(
            "Abnormal-growth evidence: suspects={} verified={} (large useful memory alone is neither)",
            array(status, "leak_suspects").len(),
            display(status.get("runaway_verified_count"), "0")
        ),
        {
            let restricted = restricted_logical_agents(status).len();
            if restricted == 0 {
                "Logical control: no active restrictions".to_owned()
            } else {
                format!("Logical control: {restricted} agent(s) restricted")
            }
        },
        format!(
            "Adaptive stopping distance: control-tick={}s remaining-steps(sub/lead)={}/{} next-batch={} {} step(s)",
            display(status.get("logical_control_tick_s"), "?"),
            display(status.get("logical_subagent_steps_remaining"), "0"),
            display(status.get("logical_lead_steps_remaining"), "0"),
            display(status.get("logical_next_batch_role"), "none"),
            display(status.get("logical_next_batch_steps"), "0")
        ),
        format!(
            "Supervisor-paused PIDs: {}",
            serde_json::to_string(array(status, "stopped_pids"))
                .unwrap_or_else(|_| "[]".to_owned())
        ),
        format!("Recent process incidents: {}", incidents.len()),
        format!(
            "Action notifications: {}",
            array(status, "notification_channels")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        ),
        format!(
            "Adaptive decision: action={} distress={} attribution={} recovery-reserve={} MiB new-fan-out-floor={} MiB trajectory-confirmed={} projected-reserve-boundary={}",
            display(status.get("action"), "?"),
            display(status.get("distress"), "?"),
            display(status.get("attribution"), "?"),
            display(status.get("automatic_reserve_mb"), "?"),
            display(status.get("new_fanout_floor_mb"), "?"),
            display(status.get("trajectory_confirmed"), "false"),
            status
                .get("time_to_recovery_reserve_s")
                .filter(|value| !value.is_null())
                .map(|value| format!("{}s", display(Some(value), "?")))
                .unwrap_or_else(|| "not currently calculable".to_owned())
        ),
    ]);
    if let Some(app) = status.get("codex_app")
        && app.get("detected").and_then(Value::as_bool) == Some(true)
    {
        let control = app.get("control").unwrap_or(&Value::Null);
        let pressure = app.get("pressure").unwrap_or(&Value::Null);
        let app_hook_active = app
            .get("hook_routes")
            .and_then(Value::as_array)
            .is_some_and(|routes| {
                !routes.is_empty()
                    && routes
                        .iter()
                        .all(|route| route.get("status").and_then(Value::as_str) == Some("ACTIVE"))
            });
        let heavy_work_state = if control.get("surface_gate").and_then(Value::as_bool) == Some(true)
        {
            if app_hook_active {
                "HELD"
            } else {
                "REQUESTED (HOOK OFFLINE)"
            }
        } else {
            "OPEN"
        };
        lines.push(format!(
            "Codex App control: mode={} | ownership={} | new-heavy-work={} | App growth={} MiB/s (confirmed={} estimated={} blind-child={} shared-host={}) | blind={} | usable-last-brake={} | control horizon={}s | selected={}",
            display(pressure.get("mode"), "OPEN"),
            display(app.get("ownership_capability"), "blind"),
            heavy_work_state,
            display(pressure.get("app_growth_mb_s"), "0"),
            display(pressure.get("confirmed_growth_mb_s"), "0"),
            display(pressure.get("estimated_growth_mb_s"), "0"),
            display(pressure.get("blind_child_growth_mb_s"), "0"),
            display(pressure.get("shared_host_growth_mb_s"), "0"),
            pressure
                .get("blind_ratio")
                .and_then(Value::as_f64)
                .map(|value| format!("{:.0}%", value * 100.0))
                .unwrap_or_else(|| "0%".to_owned()),
            display(pressure.get("backstop"), "none"),
            display(pressure.get("control_horizon_s"), "0"),
            serde_json::to_string(
                pressure
                    .get("selected_keys")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            )
            .unwrap_or_else(|_| "[]".to_owned())
        ));
        if pressure.get("mode").and_then(Value::as_str) == Some("DEGRADED_BLIND") {
            lines.push(
                "Codex App protection is degraded: the App process tree and machine pressure are still observed, but thread-by-thread control is unavailable until the native App hook route reconnects."
                    .to_owned(),
            );
        }
        if !app
            .get("identity_collisions")
            .and_then(Value::as_object)
            .is_none_or(|collisions| collisions.is_empty())
        {
            lines.push(
                "Codex App identity: overlapping app-server generations detected; targeted control is disabled for those sessions and blind protection remains active."
                    .to_owned(),
            );
        }
    }
    if status.get("cli_hard_cap_mb").is_none_or(Value::is_null) {
        lines.push("Local CLI hard cap: OFF (adaptive policy only)".to_owned());
    } else {
        lines.push(format!(
            "Local CLI hard cap: {}/{} MiB | status={} | remaining={} MiB",
            display(status.get("cli_memory_used_mb"), "?"),
            display(status.get("cli_hard_cap_mb"), "?"),
            display(status.get("cli_hard_cap_status"), "unknown"),
            display(status.get("cli_hard_cap_remaining_mb"), "?")
        ));
    }
    for incident in incidents.iter().rev().take(3).rev() {
        lines.push(format!(
            "Incident record: {} | source={} | PID={} | reason={} | id={}",
            display(incident.get("status"), "?").to_uppercase(),
            display(incident.get("source"), "?"),
            display(incident.get("pid"), "?"),
            display(incident.get("reason"), "?"),
            display(incident.get("id"), "?")
        ));
    }
    if let Some(latest) = events.iter().rev().find(|event| {
        event_should_notify(event)
            && now_epoch() - finite_number(event.get("created_at")).unwrap_or_default() <= 3600.0
    }) {
        let rendered = user_message(latest);
        let title = rendered
            .lines()
            .next()
            .unwrap_or("[Memory Supervisor] ACTION RECORDED")
            .trim_start_matches("[Memory Supervisor] ");
        lines.push(format!("Recent action notice: {title}"));
        if let Some(deliveries) = latest.get("deliveries").and_then(Value::as_object) {
            let summary = ["hook", "terminal", "os", "discord", "telegram"]
                .into_iter()
                .filter_map(|route| {
                    deliveries
                        .get(route)
                        .and_then(Value::as_str)
                        .map(|status| format!("{route}={status}"))
                })
                .collect::<Vec<_>>()
                .join(", ");
            if !summary.is_empty() {
                lines.push(format!("Notice delivery: {summary}"));
            }
        }
    }
    for (key, label) in [
        ("error", "daemon error"),
        ("configuration_error", "configuration error"),
        ("runtime_error", "runtime error"),
        ("notification_error", "notification error"),
    ] {
        if status.get(key).is_some_and(|value| !value.is_null()) {
            lines.push(format!("{label}: {}", display(status.get(key), "?")));
        }
    }
    if let Some(errors) = status.get("sensor_errors").and_then(Value::as_object)
        && !errors.is_empty()
    {
        lines.push(format!(
            "sensor error: {}",
            errors
                .iter()
                .map(|(name, error)| format!("{name}={}", display(Some(error), "?")))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if status
        .get("pending_control")
        .is_some_and(|value| !value.is_null())
    {
        lines.push(format!(
            "pending control recovery: {} — verify the PID identity, then retry the same memory-supervisor action in the owning OS",
            display(status.get("pending_control"), "?")
        ));
    }
    if status
        .get("protection_degraded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        lines.push("protection degraded: new admission is held at ORANGE or stricter".to_owned());
    }
    if matches!(
        status.get("admission_level").and_then(Value::as_str),
        Some("ORANGE" | "RED")
    ) {
        lines.push(
            "Next: do not fan out; finish or drain workers and use sequential low-memory work"
                .to_owned(),
        );
    }
    lines.join("\n")
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn path_command_exists(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            let direct = directory.join(name);
            direct.is_file()
                || (cfg!(windows) && directory.join(format!("{name}.exe")).is_file())
                || (cfg!(windows) && directory.join(format!("{name}.cmd")).is_file())
        })
    })
}

fn provider_status_with_claude_path(
    home: &Path,
    binary: &Path,
    name: &str,
    claude_search_path: Option<&OsStr>,
) -> Value {
    let (provider, command, detect, skill, hooks, requirement, next) = if name == "Claude Code" {
        (
            "claude",
            "claude",
            ".claude",
            ".claude/skills/memory-supervisor/SKILL.md",
            ".claude/settings.json",
            "Claude Code 2.1.217+",
            "the supported Claude Code version, skill, and user hook are connected; interactive workspace trust is required before settings-file hooks run and is not verified here",
        )
    } else {
        (
            "codex",
            "codex",
            ".codex",
            ".agents/skills/memory-supervisor/SKILL.md",
            ".codex/hooks.json",
            "Codex 0.145.0+ with hooks stable true",
            "all seven hooks are present, enabled, and trusted",
        )
    };
    let hook_path = if provider == "codex" {
        current_codex_hook_target().unwrap_or_else(|| home.join(hooks))
    } else {
        home.join(hooks)
    };
    let claude =
        (provider == "claude").then(|| discover_claude_with_path(home, claude_search_path));
    let detected = if provider == "codex" {
        hook_path.parent().is_some_and(Path::is_dir)
    } else {
        claude.as_ref().is_some_and(|value| value.detected()) || home.join(detect).is_dir()
    };
    if provider != "claude" && !path_command_exists(command) && !detected {
        return json!({"name": name, "status": "NOT DETECTED"});
    }
    if provider == "claude" && !detected {
        return json!({"name": name, "status": "NOT DETECTED"});
    }
    let skill_ready = home.join(skill).is_file();
    let version_ready = claude
        .as_ref()
        .is_none_or(|discovery| discovery.selected.is_some());
    let codex_health =
        (provider == "codex").then(|| audit_codex_hooks(&hook_path, binary, &platform_name()));
    let hook_ready = codex_health.as_ref().map_or_else(
        || verify_hooks(&hook_path, provider, binary).unwrap_or(false),
        |health| health.ready(),
    );
    let missing: Vec<_> = [
        ("supported version", version_ready),
        ("skill", skill_ready),
        ("hook", hook_ready),
    ]
    .into_iter()
    .filter_map(|(name, ready)| (!ready).then_some(name))
    .collect();
    let claude_selected = claude
        .as_ref()
        .and_then(|discovery| discovery.selected.as_ref());
    let claude_version = claude_selected.and_then(|selected| selected.version.clone());
    let claude_command = claude_selected.map(|selected| selected.path.display().to_string());
    let detail = if provider == "claude" {
        let skill_state = if skill_ready { "connected" } else { "missing" };
        let hook_state = if hook_ready {
            "connected"
        } else {
            "missing/stale"
        };
        if let Some(selected) = claude_selected {
            let identity = format!(
                "Claude Code {} at {}",
                selected.version.as_deref().unwrap_or("unknown"),
                selected.path.display()
            );
            if missing.is_empty() {
                format!(
                    "version: supported — {identity}; skill: {skill_state}; hook: {hook_state}; interactive workspace trust is required before settings-file hooks run and is not verified here"
                )
            } else {
                format!(
                    "version: supported — {identity}; skill: {skill_state}; hook: {hook_state}; missing/stale: {}; run `memory-supervisor update`",
                    missing.join(", ")
                )
            }
        } else {
            format!(
                "version: NEEDS ATTENTION — {}; skill: {skill_state}; hook: {}; update Claude Code, then run `memory-supervisor update`",
                claude
                    .as_ref()
                    .map(|discovery| discovery.failure_summary())
                    .unwrap_or_else(|| requirement.to_owned()),
                if hook_ready {
                    "connected and preserved"
                } else {
                    hook_state
                },
            )
        }
    } else if missing.is_empty() {
        next.to_owned()
    } else if let Some(health) = codex_health.as_ref().filter(|_| !hook_ready) {
        format!(
            "{}; next: {}",
            health.summary(),
            health.remediation(CodexHookSurface::Cli)
        )
    } else {
        format!(
            "missing/stale: {}; run `memory-supervisor update`; requires {requirement}",
            missing.join(", ")
        )
    };
    json!({
        "name": name,
        "status": if missing.is_empty() { "CONNECTED" } else { "NEEDS ATTENTION" },
        "version_supported": version_ready,
        "version": claude_version,
        "command": claude_command,
        "installations": claude.as_ref().map(|value| &value.installations),
        "skill": skill_ready,
        "hook": hook_ready,
        "hook_health": codex_health.as_ref().and_then(|health| serde_json::to_value(health).ok()),
        "detail": detail,
    })
}

fn provider_status(home: &Path, binary: &Path, name: &str) -> Value {
    let search_path = env::var_os("PATH");
    provider_status_with_claude_path(home, binary, name, search_path.as_deref())
}

pub fn connection_status(status: &Value) -> Value {
    let powered_off = status.get("power").and_then(Value::as_str) == Some("off");
    let daemon = if powered_off {
        "OFF"
    } else if status.get("ok").and_then(Value::as_bool) == Some(true) {
        "CONNECTED"
    } else if status.get("running").and_then(Value::as_bool) == Some(true) {
        "DEGRADED"
    } else {
        "NOT RUNNING"
    };
    let home = home_dir();
    let binary = env::current_exe().unwrap_or_else(|_| {
        home.join(".local")
            .join("lib")
            .join("memory-supervisor")
            .join(if cfg!(windows) {
                "memory-supervisor.exe"
            } else {
                "memory-supervisor"
            })
    });
    let providers = vec![
        provider_status(&home, &binary, "Claude Code"),
        provider_status(&home, &binary, "Codex"),
    ];
    let codex_app = status.get("codex_app").cloned().unwrap_or_else(
        || json!({"detected":false,"app_servers":[],"threads":[],"hook_routes":[]}),
    );
    let app_route_attention = codex_app.get("detected").and_then(Value::as_bool) == Some(true)
        && codex_app
            .get("hook_routes")
            .and_then(Value::as_array)
            .is_none_or(|routes| {
                routes.is_empty()
                    || routes
                        .iter()
                        .any(|route| route.get("status").and_then(Value::as_str) != Some("ACTIVE"))
            });
    let ready = !powered_off
        && daemon == "CONNECTED"
        && !app_route_attention
        && providers.iter().all(|provider| {
            provider.get("status").and_then(Value::as_str) != Some("NEEDS ATTENTION")
        });
    json!({
        "daemon": daemon,
        "power": if powered_off { "off" } else { "on" },
        "providers": providers,
        "codex_app": codex_app,
        "notifications": status.get("notification_channels").cloned().unwrap_or_else(|| json!([])),
        "ready": ready,
    })
}

pub fn human_connections(report: &Value) -> String {
    let mut lines = vec![
        "Memory Supervisor connections:".to_owned(),
        format!(
            "  {:14} {}",
            "Core daemon",
            display(report.get("daemon"), "?")
        ),
    ];
    for provider in array(report, "providers") {
        let detail = provider
            .get("detail")
            .and_then(Value::as_str)
            .map(|value| format!("  {value}"))
            .unwrap_or_default();
        lines.push(format!(
            "  {:14} {}{}",
            display(provider.get("name"), "?"),
            display(provider.get("status"), "?"),
            detail
        ));
    }
    if let Some(app) = report.get("codex_app")
        && app.get("detected").and_then(Value::as_bool) == Some(true)
    {
        let threads = array(app, "threads").len();
        let routes = array(app, "hook_routes");
        let route_status = if !routes.is_empty()
            && routes
                .iter()
                .all(|route| route.get("status").and_then(Value::as_str) == Some("ACTIVE"))
        {
            "ACTIVE"
        } else if routes
            .iter()
            .any(|route| route.get("status").and_then(Value::as_str) == Some("UNRESOLVED"))
        {
            "UNRESOLVED HOME"
        } else if routes
            .iter()
            .any(|route| route.get("status").and_then(Value::as_str) == Some("NEEDS ATTENTION"))
        {
            "NEEDS ATTENTION"
        } else if routes
            .iter()
            .any(|route| route.get("status").and_then(Value::as_str) == Some("STALE"))
        {
            "STALE"
        } else {
            "CONFIGURED"
        };
        lines.push(format!(
            "  {:14} {}  {} independent thread(s)",
            "Codex App", route_status, threads
        ));
        let details: BTreeSet<_> = routes
            .iter()
            .filter(|route| route.get("status").and_then(Value::as_str) != Some("ACTIVE"))
            .filter_map(|route| route.get("detail").and_then(Value::as_str))
            .collect();
        for detail in details {
            lines.push(format!("  {:14} {detail}", ""));
        }
    }
    lines.push(format!(
        "  {:14} {}",
        "Notif. routes",
        array(report, "notifications")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",")
    ));
    lines.push(
        if report.get("power").and_then(Value::as_str) == Some("off") {
            "Result: off by owner; connectors remain installed and pass through. Run `memory-supervisor on` to resume protection."
        } else if report.get("daemon").and_then(Value::as_str) != Some("CONNECTED") {
            "Result: core daemon needs attention."
        } else if report.get("ready").and_then(Value::as_bool) != Some(true) {
            "Result: core is running, but listed connectors need attention."
        } else {
            "Result: ready."
        }
        .to_owned(),
    );
    lines.join("\n")
}

pub fn all_statuses_at(directory: &Path, now: f64) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| serde_json::from_slice::<Value>(&fs::read(path).ok()?).ok())
        .filter_map(|mut state| {
            let timestamp = finite_number(state.get("ts"))?;
            let age = now - timestamp;
            let object = state.as_object_mut()?;
            object.insert("age_s".to_owned(), json!((age * 10.0).round() / 10.0));
            object.insert(
                "fresh".to_owned(),
                Value::Bool((-5.0..=10.0).contains(&age)),
            );
            Some(state)
        })
        .collect()
}

pub fn prune_stale_at(directory: &Path, hours: f64, now: f64) -> usize {
    let cutoff = now - hours * 3600.0;
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let mut removed = 0;
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
    {
        let modified = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs_f64())
            .unwrap_or_default();
        let timestamp = fs::read(&path)
            .ok()
            .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
            .and_then(|value| finite_number(value.get("ts")))
            .filter(|timestamp| *timestamp <= now + 5.0)
            .unwrap_or_default();
        if modified.max(timestamp) < cutoff && fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn human_all(statuses: &[Value], directory: &Path) -> String {
    if statuses.is_empty() {
        return format!(
            "federation: no instances published in {}\n각 환경(Windows/WSL/VM/컨테이너)에서 install을 실행하면 여기 나타납니다.",
            directory.display()
        );
    }
    let mut lines = vec![format!("federation ({}):", directory.display())];
    let fresh_healthy: Vec<_> = statuses
        .iter()
        .filter(|state| {
            state.get("fresh").and_then(Value::as_bool) == Some(true)
                && state.get("sensor_ok").and_then(Value::as_bool) != Some(false)
                && state.get("configuration_error").is_none_or(Value::is_null)
                && !state
                    .get("protection_degraded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .collect();
    if let Some(worst) = fresh_healthy
        .into_iter()
        .max_by_key(|state| rank(state, true))
    {
        lines.push(format!(
            "  effective admission: {} via {}",
            level_name(admission_level_for_peer(worst)),
            display(worst.get("instance"), "?")
        ));
    }
    for state in statuses {
        let cap = if state.get("cli_hard_cap_mb").is_none_or(Value::is_null) {
            "off".to_owned()
        } else {
            format!(
                "{}/{}MB",
                display(state.get("cli_memory_used_mb"), "?"),
                display(state.get("cli_hard_cap_mb"), "?")
            )
        };
        let stale = if state.get("fresh").and_then(Value::as_bool) == Some(true) {
            ""
        } else {
            "  [STALE — 해당 환경 supervisor 확인]"
        };
        lines.push(format!(
            "  {:28.28} util={:6.6} admission={:6.6} action={} mem={}/{}MB cap={} tracked={}+{} abnormal={}/{} logical={} paused={} incidents={} age={}s{}{}",
            display(state.get("instance"), "?"),
            display(state.get("utilization").or_else(|| state.get("level")), "?"),
            level_name(admission_level_for_state(state)),
            display(state.get("action"), "?"),
            display(state.get("mem_available_mb"), "?"),
            display(state.get("memory_capacity_mb"), "?"),
            cap,
            display(state.get("tracked_roots"), "0"),
            display(state.get("tracked_children"), "0"),
            array(state, "leak_suspects").len(),
            display(state.get("runaway_verified_count"), "0"),
            restricted_logical_agents(state).len(),
            array(state, "stopped_pids").len(),
            array(state, "recent_incidents").len(),
            display(state.get("age_s"), "?"),
            if state.get("sensor_ok").and_then(Value::as_bool) == Some(false) {
                " sensor-error"
            } else {
                ""
            },
            stale
        ));
    }
    lines.join("\n")
}

fn usage() -> &'static str {
    "USAGE: memory-status [--json] [--connections] [--all [--prune-stale-hours HOURS]]"
}

pub fn run_status(arguments: &[OsString]) -> i32 {
    let mut json_output = false;
    let mut connections = false;
    let mut all = false;
    let mut prune = None;
    let mut index = 0;
    while index < arguments.len() {
        let Some(argument) = arguments[index].to_str() else {
            eprintln!("arguments must be valid Unicode\n{}", usage());
            return 2;
        };
        match argument {
            "--json" => json_output = true,
            "--connections" => connections = true,
            "--all" => all = true,
            "--prune-stale-hours" => {
                let Some(value) = arguments.get(index + 1).and_then(|value| value.to_str()) else {
                    eprintln!("--prune-stale-hours requires HOURS\n{}", usage());
                    return 2;
                };
                prune = value.parse::<f64>().ok();
                if prune.is_none_or(|value| !value.is_finite() || value < 24.0) {
                    eprintln!("--prune-stale-hours must be at least 24\n{}", usage());
                    return 2;
                }
                index += 1;
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return 0;
            }
            value => {
                eprintln!("unknown option: {value}\n{}", usage());
                return 2;
            }
        }
        index += 1;
    }
    if connections && all {
        eprintln!("--connections and --all cannot be combined\n{}", usage());
        return 2;
    }
    if all && power_is_off() {
        let status = read_status();
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!([status])).unwrap()
            );
        } else {
            println!("{}", human(&status));
        }
        return 0;
    }
    if connections {
        let status = read_status();
        let report = connection_status(&status);
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            println!("{}", human_connections(&report));
        }
        return if status.get("ok").and_then(Value::as_bool) == Some(true) {
            0
        } else {
            1
        };
    }
    if all {
        let directory = federation_dir();
        if let Some(hours) = prune {
            prune_stale_at(&directory, hours, now_epoch());
        }
        let statuses = all_statuses_at(&directory, now_epoch());
        if json_output {
            println!("{}", serde_json::to_string_pretty(&statuses).unwrap());
        } else {
            println!("{}", human_all(&statuses, &directory));
        }
        return if statuses.iter().any(|state| {
            state.get("fresh").and_then(Value::as_bool) == Some(true)
                && state.get("sensor_ok").and_then(Value::as_bool) != Some(false)
                && !state
                    .get("protection_degraded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        }) {
            0
        } else {
            1
        };
    }
    if prune.is_some() {
        eprintln!("--prune-stale-hours requires --all\n{}", usage());
        return 2;
    }
    let status = read_status();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&status).unwrap());
    } else {
        println!("{}", human(&status));
    }
    if status.get("ok").and_then(Value::as_bool) == Some(true) {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temporary_directory() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "memory-supervisor-status-{}-{}",
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
    fn worst_peer_controls_json_admission_fields() {
        let root = temporary_directory();
        let path = root.join("state.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "ts": 1000.0,
                "level": "GREEN",
                "action": "allow",
                "mem_available_mb": 8000,
                "memory_capacity_mb": 16000
            }))
            .unwrap(),
        )
        .unwrap();
        let status = read_status_at(
            &path,
            vec![json!({
                "level": "ORANGE",
                "instance": "windows-host",
                "action": "hold",
                "distress": "elevated",
                "attribution": "external",
                "mem_available_mb": 600,
                "memory_capacity_mb": 32768,
                "cli_hard_cap_mb": 8192,
                "cli_hard_cap_status": "near"
            })],
            1001.0,
        );
        assert_eq!(status["admission_source"], "windows-host");
        assert_eq!(status["admission_level"], "ORANGE");
        assert_eq!(status["admission_cli_hard_cap_mb"], 8192);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn intentional_power_off_is_healthy_and_actionable() {
        let status = powered_off_status(Path::new("/tmp/state.json"));
        assert_eq!(status["ok"], true);
        assert_eq!(status["running"], false);
        assert_eq!(status["power"], "off");
        let output = human(&status);
        assert!(output.starts_with("memory-supervisor: OFF"));
        assert!(output.contains("memory-supervisor on"));
    }

    #[test]
    fn pause_block_leads_with_exact_recovery_command() {
        let output = human(&json!({
            "running": true,
            "level": "RED",
            "admission_level": "ORANGE",
            "platform": "linux",
            "stopped_pids": [42],
            "recent_incidents": [{
                "pid": 42,
                "name": "codex",
                "role": "lead",
                "reason": "runaway-memory",
                "recovery_policy": "lead-probation",
                "terminal_notice": "delivered",
                "terminal_notice_reason": "written"
            }]
        }));
        let first = output.lines().take(10).collect::<Vec<_>>().join("\n");
        assert!(first.contains("This is not a terminal crash"));
        assert!(first.contains("codex (lead), PID 42"));
        assert!(first.contains("memory-supervisor resume 42"));
    }

    #[test]
    fn logical_cushion_is_readable_and_never_described_as_a_process_pause() {
        let output = human(&json!({
            "running":true, "level":"RED", "admission_level":"RED", "platform":"linux",
            "logical_epoch":12, "runaway_verified_count":1,
            "logical_agents":{
                "claude:s1:a1":{
                    "key":"claude:s1:a1", "active":true, "state":"LIGHT_WORK_ONLY",
                    "role":"subagent", "reason":"verified abnormal growth reached the recovery boundary"
                },
                "claude:s1:root":{
                    "key":"claude:s1:root", "active":true, "state":"ACTIVE", "role":"lead"
                }
            },
            "leak_suspects":[{"pid":42}], "stopped_pids":[], "recent_incidents":[]
        }));
        assert!(output.starts_with("[Memory Supervisor] LOGICAL CUSHION ACTIVE"));
        assert!(output.contains("Authoritative epoch: 12"));
        assert!(output.contains("claude:s1:a1 | state=LIGHT_WORK_ONLY"));
        assert!(output.contains("CLI process is still running"));
        assert!(output.contains("suspects=1 verified=1"));
        assert!(!output.contains("PROCESS PAUSED"));
    }

    #[test]
    fn stale_prune_removes_old_malformed_artifact() {
        let root = temporary_directory();
        let path = root.join("old.json");
        fs::write(&path, b"not-json").unwrap();
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(prune_stale_at(&root, 24.0, now_epoch() + 172_800.0), 1);
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connection_status_rejects_a_stale_hook_binary() {
        let home = temporary_directory();
        let claude_bin = home.join(".local/bin");
        fs::create_dir_all(&claude_bin).unwrap();
        let claude = claude_bin.join(if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        });
        fs::write(
            &claude,
            if cfg!(windows) {
                "@echo 9.9.9 (Claude Code)\r\n"
            } else {
                "#!/bin/sh\necho '9.9.9 (Claude Code)'\n"
            },
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let skill = home.join(".claude/skills/memory-supervisor");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: memory-supervisor\n---\n",
        )
        .unwrap();
        let hooks = home.join(".claude/settings.json");
        let installed = home.join("installed-memory-supervisor");
        crate::integration::update_hooks(&hooks, "claude", &installed, false).unwrap();

        let search_path = env::join_paths([&claude_bin]).unwrap();
        let connected = provider_status_with_claude_path(
            &home,
            &installed,
            "Claude Code",
            Some(search_path.as_os_str()),
        );
        assert_eq!(connected["status"], "CONNECTED");
        assert!(
            connected["detail"]
                .as_str()
                .unwrap()
                .contains("workspace trust is required")
        );
        assert_eq!(connected["version"], "9.9.9");
        assert_eq!(
            provider_status_with_claude_path(
                &home,
                &home.join("new-memory-supervisor"),
                "Claude Code",
                Some(search_path.as_os_str()),
            )["status"],
            "NEEDS ATTENTION"
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn claude_version_failure_does_not_hide_an_existing_hook() {
        let home = temporary_directory();
        let claude_bin = home.join(".local/bin");
        fs::create_dir_all(&claude_bin).unwrap();
        let claude = claude_bin.join(if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        });
        fs::write(
            &claude,
            if cfg!(windows) {
                "@echo 2.1.142 (Claude Code)\r\n"
            } else {
                "#!/bin/sh\necho '2.1.142 (Claude Code)'\n"
            },
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let skill = home.join(".claude/skills/memory-supervisor");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: memory-supervisor\n---\n",
        )
        .unwrap();
        let hooks = home.join(".claude/settings.json");
        let installed = home.join("installed-memory-supervisor");
        crate::integration::update_hooks(&hooks, "claude", &installed, false).unwrap();

        let search_path = env::join_paths([&claude_bin]).unwrap();
        let status = provider_status_with_claude_path(
            &home,
            &installed,
            "Claude Code",
            Some(search_path.as_os_str()),
        );
        assert_eq!(status["status"], "NEEDS ATTENTION");
        assert_eq!(status["version_supported"], false);
        assert_eq!(status["hook"], true);
        assert!(
            status["detail"]
                .as_str()
                .unwrap()
                .contains("hook: connected and preserved")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn app_connection_failure_prints_app_settings_guidance() {
        let output = human_connections(&json!({
            "daemon":"CONNECTED",
            "power":"on",
            "providers":[],
            "codex_app":{
                "detected":true,
                "threads":[{"session_id":"existing-task"}],
                "hook_routes":[{
                    "status":"NEEDS ATTENTION",
                    "detail":"next: in Codex App, open Settings → Hooks, then continue any existing App task; no App restart or new task is required"
                }]
            },
            "notifications":[],
            "ready":false
        }));
        assert!(output.contains("Codex App      NEEDS ATTENTION  1 independent thread(s)"));
        assert!(output.contains("Settings → Hooks"));
        assert!(output.contains("continue any existing App task"));
        assert!(!output.contains("open `/hooks`"));
    }

    #[test]
    fn cached_peer_cap_and_health_failures_remain_explicit_in_json_and_human_output() {
        let root = temporary_directory();
        let path = root.join("state.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "ts":1000.0, "level":"GREEN", "utilization":"GREEN",
                "action":"allow", "distress":"normal", "attribution":"unknown",
                "mem_available_mb":8000, "memory_capacity_mb":16384,
                "local_admission_level":"GREEN", "admission_level":"RED",
                "admission_source":"windows-host", "admission_mem_available_mb":300,
                "admission_capacity_mb":32768, "admission_action":"drain",
                "admission_distress":"critical", "admission_attribution":"external",
                "admission_cli_hard_cap_mb":4096, "admission_cli_memory_used_mb":4200,
                "admission_cli_hard_cap_status":"exceeded", "sensor_ok":true,
                "stopped_pids":[], "recent_incidents":[]
            }))
            .unwrap(),
        )
        .unwrap();
        let status = read_status_at(&path, Vec::new(), 1001.0);
        assert_eq!(status["admission_source"], "windows-host");
        assert_eq!(status["admission_level"], "RED");
        assert_eq!(status["admission_cli_hard_cap_mb"], 4096);
        let output = human(&status);
        assert!(output.contains("EXPLICIT CLI HARD CAP REACHED"));
        assert!(output.contains("tracked CLI memory=4200 MiB; hard cap=4096 MiB"));

        let degraded = human(&json!({
            "running":true, "level":"GREEN", "admission_level":"ORANGE",
            "platform":"linux", "stopped_pids":"malformed", "recent_incidents":null,
            "notification_events":{}, "sensor_errors":{"processes":"inventory failed"},
            "configuration_error":"bad threshold", "runtime_error":"runtime corrupt",
            "notification_error":"delivery failed", "protection_degraded":true
        }));
        for expected in [
            "configuration error: bad threshold",
            "runtime error: runtime corrupt",
            "notification error: delivery failed",
            "sensor error: processes=inventory failed",
            "protection degraded",
        ] {
            assert!(
                degraded.contains(expected),
                "missing {expected}: {degraded}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn human_actions_list_every_exact_pid_and_recent_delivery_state() {
        let output = human(&json!({
            "running":true, "level":"RED", "admission_level":"RED", "platform":"windows",
            "stopped_pids":[42,"43",0,"bad"],
            "recent_incidents":[
                {"id":"i42","pid":42,"name":"codex","role":"lead","reason":"runaway-memory","recovery_policy":"lead-probation"},
                {"id":"i43","pid":43,"name":"worker","role":"worker","reason":"pressure-pause","recovery_policy":"automatic-pressure-recovery"}
            ],
            "notification_events":[{
                "type":"process-pause","status":"suspended","action":"paused",
                "cause":"pressure-pause","created_at":now_epoch(),
                "message":"legacy TTE=Some(4.0)s",
                "deliveries":{"hook":"delivered","os":"failed"},
                "delivery_details":{"os_route":"windows-balloon"}
            }]
        }));
        assert!(output.contains("memory-supervisor resume 42"));
        assert!(output.contains("memory-supervisor resume 43"));
        assert!(!output.contains("selects the only paused PID"));
        assert!(output.contains("Recent action notice: PROCESS PAUSED"));
        assert!(output.contains("Notice delivery: hook=delivered, os=failed"));
        assert!(!output.contains("Some("));
        assert!(!output.contains("windows-balloon"));

        let probation = human(&json!({
            "running":true, "level":"GREEN", "admission_level":"ORANGE", "platform":"linux",
            "stopped_pids":[], "probation":{"pid":42,"status":"monitoring"}
        }));
        assert!(probation.contains("GUARDED RESUME"));
        assert!(probation.contains("new fan-out remains held"));

        let lead_message = "[Memory Supervisor] PROCESS RESUMED\nAgent context: next hook boundary";
        let recent = human(&json!({
            "running":true, "level":"GREEN", "admission_level":"GREEN", "platform":"linux",
            "stopped_pids":[], "notification_events":[{
                "event_id":"lead","type":"lead-probation","status":"resumed","role":"lead",
                "message":lead_message,"created_at":now_epoch()
            }]
        }));
        assert!(recent.starts_with("[Memory Supervisor] LATEST LEAD RECOVERY EVENT"));
        assert!(recent.contains(lead_message));
    }

    #[test]
    fn invalid_timestamp_and_federation_health_are_reported_without_panics() {
        let root = temporary_directory();
        let path = root.join("state.json");
        fs::write(&path, br#"{"ts":"bad","level":"GREEN"}"#).unwrap();
        let status = read_status_at(&path, Vec::new(), 1000.0);
        assert_eq!(status["running"], false);
        assert!(
            status["error"]
                .as_str()
                .unwrap()
                .contains("invalid timestamp")
        );

        for (name, value) in [
            (
                "healthy.json",
                json!({"ts":1000.0,"level":"YELLOW","instance":"healthy","sensor_ok":true}),
            ),
            (
                "degraded.json",
                json!({"ts":1000.0,"level":"RED","instance":"degraded","protection_degraded":true}),
            ),
            (
                "stale.json",
                json!({"ts":900.0,"level":"RED","instance":"stale","sensor_ok":true}),
            ),
        ] {
            fs::write(root.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
        }
        fs::write(root.join("array.json"), b"[]").unwrap();
        let statuses = all_statuses_at(&root, 1001.0);
        assert_eq!(statuses.len(), 3);
        let report = human_all(&statuses, &root);
        assert!(report.contains("effective admission: YELLOW via healthy"));
        assert!(report.contains("[STALE"));
        assert!(!report.contains("effective admission: RED via degraded"));
        fs::remove_dir_all(root).unwrap();
    }
}
