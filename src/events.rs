use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::write_atomic_json;

const ALLOWED_FIELDS: [&str; 25] = [
    "source",
    "platform",
    "severity",
    "type",
    "status",
    "pid",
    "role",
    "cause",
    "attribution",
    "distress",
    "headroom_mb",
    "capacity_mb",
    "tte_s",
    "reserve_mb",
    "native_state",
    "psi_full_avg10",
    "reclaim_rate_s",
    "swap_rate_s",
    "action",
    "recovery",
    "message",
    "terminal",
    "importance",
    "event_id",
    "created_at",
];
const TRANSPORTS: [&str; 5] = ["hook", "terminal", "os", "discord", "telegram"];
const DELIVERY_STATES: [&str; 4] = ["delivered", "failed", "skipped", "unavailable"];

fn text<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn number(event: &Value, key: &str) -> Option<f64> {
    event
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// Render the stable event schema into a user-facing action notice.
///
/// Current producers already store this format in `message`. The schema-based
/// fallback keeps runtime records written by older releases from leaking Rust
/// debug formatting or obsolete prose back through hooks and notification
/// transports after an upgrade.
pub fn user_message(event: &Value) -> String {
    let stored = text(event, "message").unwrap_or_default().trim();
    let obsolete_format = ["Some(", "Nones", "TTE=None", "TTE=Some("]
        .iter()
        .any(|token| stored.contains(token));
    if stored.starts_with("[Memory Supervisor]") && !obsolete_format {
        return truncate(stored, 2000);
    }

    let event_type = text(event, "type").unwrap_or("memory-action");
    let status = text(event, "status").unwrap_or("recorded");
    let action = text(event, "action").unwrap_or_default();
    let cause = text(event, "cause").unwrap_or("memory-policy");
    let pid = event
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid > 1);
    let role = text(event, "role").unwrap_or("CLI process");

    let (title, effect, next) = if action == "paused" || status == "suspended" {
        (
            "PROCESS PAUSED",
            "The exact PID and its in-memory session are preserved; this is not a crash.",
            pid.map(|pid| {
                format!(
                    "Review `memory-status`; resume only after reviewing the cause with `memory-supervisor resume {pid}`."
                )
            })
            .unwrap_or_else(|| "Review `memory-status` before resuming any process.".to_owned()),
        )
    } else if action == "resumed" || status == "resumed" {
        (
            "PROCESS RESUMED",
            "The preserved PID is running again under the current admission policy.",
            "No recovery command is required; check `memory-status` before new fan-out.".to_owned(),
        )
    } else if action == "manual-choice" || matches!(status, "critical" | "failed") {
        (
            "ACTION REQUIRED",
            "Automatic protection could not complete a safe state transition.",
            "Preserve work and run `memory-status` for the exact recovery instruction.".to_owned(),
        )
    } else if action == "hold" || status == "hold" {
        (
            "NEW FAN-OUT HELD",
            "Only new fan-out is blocked; existing work continues and no process was paused by admission.",
            "Wait for automatic recovery or use sequential work; `memory-status` shows the live decision."
                .to_owned(),
        )
    } else if action == "drain" || status == "drain" {
        (
            "DRAIN MODE ACTIVE",
            "New fan-out is blocked while existing work is contained under the current machine pressure.",
            "Preserve work and check `memory-status`; a process pause is reported as a separate action."
                .to_owned(),
        )
    } else if action == "probation" || status == "monitoring" {
        (
            "GUARDED RESUME",
            "The same PID is running under one-time recovery observation.",
            "Stable recovery completes automatically; renewed growth pauses the same PID again."
                .to_owned(),
        )
    } else {
        (
            "MEMORY ACTION RECORDED",
            "The supervisor recorded a memory-policy state change.",
            "Run `memory-status` for the current decision; no unreported process pause should be assumed."
                .to_owned(),
        )
    };

    let why = match cause {
        "runaway-memory" | "material-process-growth-observation" => {
            "Sustained process growth was observed across the configured evidence window".to_owned()
        }
        "pressure-pause" | "pressure-lead-last-resort" => {
            "Recoverable machine headroom approached exhaustion and the exact PID was selected as the minimum containment step".to_owned()
        }
        "hard-cap-pause" | "hard-cap-lead-last-resort" => {
            "Tracked CLI memory reached the explicit local hard cap".to_owned()
        }
        "adaptive-pressure-assessment" | "memory-admission" => {
            "The adaptive machine-pressure assessment changed the admission decision".to_owned()
        }
        "runtime-persistence-failure" => {
            "The runtime could not durably record a control transition".to_owned()
        }
        "external-resume" => "The PID was resumed outside automatic recovery".to_owned(),
        _ => format!("The supervisor recorded cause `{cause}` for event `{event_type}`"),
    };
    let headroom = match (number(event, "headroom_mb"), number(event, "capacity_mb")) {
        (Some(available), Some(capacity)) => {
            let tte = number(event, "tte_s")
                .map(|seconds| format!("; estimated exhaustion in {seconds:.1}s"))
                .unwrap_or_default();
            format!("; available headroom was {available:.0}/{capacity:.0} MiB{tte}")
        }
        _ => String::new(),
    };
    let attribution = match text(event, "attribution").unwrap_or("unknown") {
        "agent" => "agent activity likely dominated the machine-level headroom loss",
        "mixed" => "agent and external activity both contributed",
        "external" => "external activity likely dominated the machine-level headroom loss",
        _ => "the machine-level cause was not attributable from the recorded evidence",
    };
    let target = pid
        .map(|pid| format!("\nTarget: {role}, PID {pid}"))
        .unwrap_or_default();
    truncate(
        &format!(
            "[Memory Supervisor] {title}{target}\nWhy: {why}{headroom}.\nAttribution estimate: {attribution}.\nState: {effect}\nNext: {next}"
        ),
        2000,
    )
}

fn now() -> f64 {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    (value * 1000.0).round() / 1000.0
}

fn truncate(value: &str, characters: usize) -> String {
    value.chars().take(characters).collect()
}

fn digest(value: &str, characters: usize) -> String {
    let result = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(characters);
    for byte in result {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").unwrap();
        if encoded.len() >= characters {
            encoded.truncate(characters);
            break;
        }
    }
    encoded
}

pub fn make_event(
    event_type: &str,
    status: &str,
    message: &str,
    source: &str,
    dedupe_key: &str,
    mut fields: Map<String, Value>,
) -> Value {
    let identity = format!("{source}|{event_type}|{status}|{dedupe_key}");
    let importance = if fields
        .remove("importance")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "important".to_owned())
        .trim()
        .eq_ignore_ascii_case("detail")
    {
        "detail"
    } else {
        "important"
    };
    let mut event = Map::from_iter([
        ("event_id".to_owned(), Value::String(digest(&identity, 24))),
        ("source".to_owned(), Value::String(truncate(source, 128))),
        ("type".to_owned(), Value::String(truncate(event_type, 64))),
        ("status".to_owned(), Value::String(truncate(status, 64))),
        ("message".to_owned(), Value::String(truncate(message, 2000))),
        ("created_at".to_owned(), json!(now())),
        (
            "importance".to_owned(),
            Value::String(importance.to_owned()),
        ),
    ]);
    for (key, value) in fields {
        if ALLOWED_FIELDS.contains(&key.as_str()) && !value.is_null() {
            event.insert(key, value);
        }
    }
    Value::Object(event)
}

pub fn event_should_notify(event: &Value) -> bool {
    let importance = event
        .get("importance")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !importance.is_empty() {
        return importance != "detail";
    }
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(event_type, "utilization-transition" | "leak-suspect") {
        return false;
    }
    event_type != "pressure-action"
        || matches!(
            event.get("action").and_then(Value::as_str),
            Some("hold" | "drain")
        )
}

pub fn queue_event(
    directory: &Path,
    event: &Value,
    known_ids: &BTreeSet<String>,
) -> io::Result<bool> {
    let Some(event_id) = event.get("event_id").and_then(Value::as_str) else {
        return Ok(false);
    };
    if event_id.is_empty() || known_ids.contains(event_id) {
        return Ok(false);
    }
    let target = directory
        .join("notification-events/pending")
        .join(format!("{event_id}.json"));
    if target.exists() {
        return Ok(false);
    }
    let Some(object) = event.as_object() else {
        return Ok(false);
    };
    let mut sanitized: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| ALLOWED_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    sanitized.insert("event_id".to_owned(), Value::String(event_id.to_owned()));
    let message = sanitized
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    sanitized.insert("message".to_owned(), Value::String(truncate(message, 2000)));
    write_atomic_json(&target, &sanitized, 0o600, true)?;
    Ok(true)
}

fn sorted_json_files(directory: &Path, limit: usize) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .collect();
    paths.sort();
    paths.truncate(limit);
    paths
}

pub fn pending_events(directory: &Path, known_ids: &BTreeSet<String>) -> Vec<Value> {
    let mut events: Vec<_> =
        sorted_json_files(&directory.join("notification-events/pending"), usize::MAX)
            .into_iter()
            .filter_map(|path| fs::read(path).ok())
            .filter_map(|source| serde_json::from_slice::<Value>(&source).ok())
            .filter(|event| {
                let id = event
                    .get("event_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                !id.is_empty()
                    && !known_ids.contains(id)
                    && event
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.is_empty())
                    && event
                        .get("message")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.is_empty())
            })
            .collect();
    events.sort_by(|left, right| {
        left.get("created_at")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            .total_cmp(
                &right
                    .get("created_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
            )
            .then_with(|| {
                left.get("event_id")
                    .and_then(Value::as_str)
                    .cmp(&right.get("event_id").and_then(Value::as_str))
            })
    });
    events.truncate(128);
    events
}

pub fn acknowledge_event(
    directory: &Path,
    event_id: &str,
    transport: &str,
    status: &str,
    consumer: &str,
) -> io::Result<bool> {
    if event_id.is_empty() || !TRANSPORTS.contains(&transport) || !DELIVERY_STATES.contains(&status)
    {
        return Ok(false);
    }
    let suffix = digest(&format!("{event_id}|{transport}|{consumer}"), 16);
    let target = directory
        .join("notification-events/acks")
        .join(format!("{event_id}-{suffix}.json"));
    write_atomic_json(
        &target,
        &json!({
            "event_id": event_id,
            "transport": transport,
            "status": status,
            "acknowledged_at": now(),
        }),
        0o600,
        true,
    )?;
    Ok(true)
}

pub fn pending_acknowledgements(directory: &Path) -> Vec<(PathBuf, Value)> {
    sorted_json_files(&directory.join("notification-events/acks"), 256)
        .into_iter()
        .filter_map(|path| {
            let value = serde_json::from_slice::<Value>(&fs::read(&path).ok()?).ok()?;
            let valid = value
                .get("event_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
                && value
                    .get("transport")
                    .and_then(Value::as_str)
                    .is_some_and(|value| TRANSPORTS.contains(&value))
                && value
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|value| DELIVERY_STATES.contains(&value));
            valid.then_some((path, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "memory-supervisor-events-{}-{}",
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
    fn event_queue_dedupes_sanitizes_and_acknowledges() {
        let root = temporary_directory();
        let event = make_event(
            "spawn-denial",
            "blocked",
            "one spawn was blocked",
            "linux-test",
            "session-action",
            Map::from_iter([
                ("action".to_owned(), json!("hold")),
                ("prompt".to_owned(), json!("private prompt")),
            ]),
        );
        assert!(event_should_notify(&event));
        assert!(queue_event(&root, &event, &BTreeSet::new()).unwrap());
        assert!(!queue_event(&root, &event, &BTreeSet::new()).unwrap());
        let stored = pending_events(&root, &BTreeSet::new());
        assert_eq!(stored.len(), 1);
        assert!(stored[0].get("prompt").is_none());
        let event_id = event["event_id"].as_str().unwrap();
        assert!(acknowledge_event(&root, event_id, "hook", "delivered", "session-1").unwrap());
        assert_eq!(pending_acknowledgements(&root).len(), 1);

        let detail = make_event(
            "utilization-transition",
            "orange",
            "YELLOW to ORANGE",
            "linux-test",
            "transition",
            Map::from_iter([("importance".to_owned(), json!("detail"))]),
        );
        assert!(!event_should_notify(&detail));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_events_follow_creation_time_not_hash_filename() {
        let root = temporary_directory();
        let mut active = make_event(
            "pressure-episode",
            "active",
            "active",
            "test",
            "episode",
            Map::new(),
        );
        active["event_id"] = json!("z-earlier");
        active["created_at"] = json!(1.0);
        let mut recovered = make_event(
            "pressure-episode",
            "recovered",
            "recovered",
            "test",
            "episode",
            Map::new(),
        );
        recovered["event_id"] = json!("a-later");
        recovered["created_at"] = json!(2.0);
        assert!(queue_event(&root, &recovered, &BTreeSet::new()).unwrap());
        assert!(queue_event(&root, &active, &BTreeSet::new()).unwrap());

        let statuses: Vec<_> = pending_events(&root, &BTreeSet::new())
            .into_iter()
            .map(|event| event["status"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(statuses, ["active", "recovered"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_event_messages_are_normalized_at_the_user_boundary() {
        let event = json!({
            "type":"pressure-action",
            "status":"hold",
            "action":"hold",
            "cause":"adaptive-pressure-assessment",
            "attribution":"external",
            "headroom_mb":3838,
            "capacity_mb":16108,
            "tte_s":45.4,
            "message":"Memory action changed to Hold; TTE=Some(45.4)s."
        });
        let rendered = user_message(&event);
        assert!(rendered.starts_with("[Memory Supervisor] NEW FAN-OUT HELD"));
        assert!(rendered.contains("Only new fan-out is blocked"));
        assert!(rendered.contains("external activity likely dominated"));
        assert!(!rendered.contains("Some("));
        assert!(!rendered.contains("Nones"));

        let degraded = user_message(&json!({
            "type":"protection-degraded",
            "status":"critical",
            "action":"hold",
            "cause":"runtime-persistence-failure",
            "message":"legacy failure"
        }));
        assert!(degraded.starts_with("[Memory Supervisor] ACTION REQUIRED"));
    }
}
