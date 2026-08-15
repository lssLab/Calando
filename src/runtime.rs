use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::codex_app::CodexAppLedger;
use crate::containment::{LogicalAgent, LogicalState, RunawayConfirmation};
use crate::policy::{Action, Level};
use crate::storage::write_atomic_json;

pub const INCIDENT_RETENTION_S: f64 = 86_400.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingControl {
    pub action: String,
    pub pid: u32,
    pub identity: String,
    pub requested_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Probation {
    pub status: String,
    pub pid: u32,
    pub identity: String,
    #[serde(default)]
    pub signal_sent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth_mb_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLedger {
    pub schema_version: u32,
    pub updated_at: f64,
    pub instance: String,
    pub stopped: BTreeMap<String, String>,
    pub resume_cooldown: BTreeMap<String, f64>,
    pub incidents: Vec<Value>,
    pub notification_events: Vec<Value>,
    pub level: Level,
    pub level_since: f64,
    pub last_assessment_action: Option<Action>,
    pub action_since: f64,
    pub pending_control: Option<PendingControl>,
    pub probation: Option<Probation>,
    pub last_pressure_action_at: f64,
    #[serde(default)]
    pub logical_epoch: u64,
    #[serde(default)]
    pub logical_agents: BTreeMap<String, LogicalAgent>,
    #[serde(default)]
    pub codex_app: CodexAppLedger,
    #[serde(default)]
    pub runaway_confirmations: BTreeMap<String, RunawayConfirmation>,
    #[serde(default)]
    pub last_logical_action_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_episode_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_pressure_episode_event: Option<Value>,
    #[serde(default)]
    pub federation_peer_last_fresh: BTreeMap<String, f64>,
    #[serde(default)]
    pub federation_peer_stale_since: BTreeMap<String, f64>,
}

impl RuntimeLedger {
    pub fn empty(instance: impl Into<String>) -> Self {
        Self {
            schema_version: 3,
            updated_at: 0.0,
            instance: instance.into(),
            stopped: BTreeMap::new(),
            resume_cooldown: BTreeMap::new(),
            incidents: Vec::new(),
            notification_events: Vec::new(),
            level: Level::Green,
            level_since: 0.0,
            last_assessment_action: None,
            action_since: 0.0,
            pending_control: None,
            probation: None,
            last_pressure_action_at: 0.0,
            logical_epoch: 0,
            logical_agents: BTreeMap::new(),
            codex_app: CodexAppLedger::default(),
            runaway_confirmations: BTreeMap::new(),
            last_logical_action_at: 0.0,
            pressure_episode_started_at: None,
            pending_pressure_episode_event: None,
            federation_peer_last_fresh: BTreeMap::new(),
            federation_peer_stale_since: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path, instance: &str, now: f64) -> (Self, Option<String>) {
        let source = match fs::read(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (Self::empty(instance), None);
            }
            Err(error) => {
                return (
                    Self::empty(instance),
                    Some(format!("{}: IO error: {error}", path.display())),
                );
            }
        };
        let mut value: Self = match serde_json::from_slice(&source) {
            Ok(value) => value,
            Err(error) => {
                return (
                    Self::empty(instance),
                    Some(format!("{}: JSON error: {error}", path.display())),
                );
            }
        };
        if let Err(error) = value.validate() {
            return (
                Self::empty(instance),
                Some(format!("{}: {error}", path.display())),
            );
        }
        value.instance = instance.to_owned();
        value
            .resume_cooldown
            .retain(|_, expires_at| expires_at.is_finite() && *expires_at > now);
        if value.incidents.len() > 128 {
            value.incidents.drain(..value.incidents.len() - 128);
        }
        if value.notification_events.len() > 128 {
            value
                .notification_events
                .drain(..value.notification_events.len() - 128);
        }
        // Development builds once advanced the global logical epoch for
        // ordinary session/subagent lifecycle.  With no recorded containment
        // action and no restricted agent, that number has no control meaning;
        // erase it so status and future hook context cannot expose stale noise.
        if value.last_logical_action_at <= 0.0
            && value
                .logical_agents
                .values()
                .all(|agent| agent.state == LogicalState::Active)
        {
            value.logical_epoch = 0;
            for agent in value.logical_agents.values_mut() {
                agent.epoch = 0;
            }
        }
        (value, None)
    }

    fn validate(&self) -> Result<(), String> {
        if !matches!(self.schema_version, 1..=3) {
            return Err("top-level object with schema_version=1|2|3 required".to_owned());
        }
        for (raw_pid, identity) in &self.stopped {
            let pid: u32 = raw_pid
                .parse()
                .map_err(|_| format!("invalid stopped pid: {raw_pid}"))?;
            if pid <= 1 || !identity.starts_with(&format!("{pid}:")) {
                return Err(format!("invalid stopped identity for pid {pid}"));
            }
        }
        if self.incidents.iter().any(|item| {
            item.as_object()
                .and_then(|object| object.get("id"))
                .is_none_or(Value::is_null)
        }) {
            return Err("incidents must be objects with ids".to_owned());
        }
        if self.notification_events.iter().any(|item| {
            let Some(object) = item.as_object() else {
                return true;
            };
            object
                .get("event_id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || object
                    .get("deliveries")
                    .is_some_and(|deliveries| !deliveries.is_object())
        }) {
            return Err(
                "notification_events must be objects with string event ids and object deliveries"
                    .to_owned(),
            );
        }
        if self
            .pending_pressure_episode_event
            .as_ref()
            .is_some_and(|event| {
                event.get("type").and_then(Value::as_str) != Some("pressure-episode")
                    || event
                        .get("event_id")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                    || event
                        .get("message")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                    || !matches!(
                        event.get("status").and_then(Value::as_str),
                        Some("active" | "recovered" | "ended-with-loss")
                    )
                    || (event.get("status").and_then(Value::as_str) == Some("active"))
                        != self.pressure_episode_started_at.is_some()
            })
        {
            return Err("pending pressure episode event is invalid".to_owned());
        }
        if self.federation_peer_last_fresh.len() > 128
            || self.federation_peer_stale_since.len() > 128
            || self
                .federation_peer_last_fresh
                .iter()
                .chain(&self.federation_peer_stale_since)
                .any(|(instance, timestamp)| {
                    instance.is_empty() || !timestamp.is_finite() || *timestamp < 0.0
                })
            || self
                .federation_peer_stale_since
                .keys()
                .any(|instance| !self.federation_peer_last_fresh.contains_key(instance))
        {
            return Err("federation peer ledger is invalid".to_owned());
        }
        if let Some(pending) = &self.pending_control
            && (!matches!(pending.action.as_str(), "resume" | "terminate" | "kill")
                || pending.pid <= 1
                || !pending.identity.starts_with(&format!("{}:", pending.pid))
                || self.stopped.get(&pending.pid.to_string()) != Some(&pending.identity))
        {
            return Err("pending_control is invalid".to_owned());
        }
        if let Some(probation) = &self.probation
            && (probation.pid <= 1
                || !probation
                    .identity
                    .starts_with(&format!("{}:", probation.pid))
                || !matches!(probation.status.as_str(), "monitoring" | "failed"))
        {
            return Err("probation is invalid".to_owned());
        }
        if !self.level_since.is_finite()
            || !self.action_since.is_finite()
            || !self.last_pressure_action_at.is_finite()
            || !self.last_logical_action_at.is_finite()
            || self
                .pressure_episode_started_at
                .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err("transition timestamps must be finite".to_owned());
        }
        if self.logical_agents.iter().any(|(key, agent)| {
            key != &agent.key
                || agent.session_id.is_empty()
                || agent.provider.is_empty()
                || !agent.state_since.is_finite()
                || !agent.started_at.is_finite()
                || !agent.last_seen_at.is_finite()
                || !agent.last_progress_at.is_finite()
                || !agent.next_check.is_finite()
                || agent.idle_since.is_some_and(|value| !value.is_finite())
                || agent.last_heavy_at.is_some_and(|value| !value.is_finite())
                || agent
                    .last_hook_receipt_at
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
        }) {
            return Err("logical agent ledger is invalid".to_owned());
        }
        if let Some(pending) = &self.codex_app.control.pending_physical
            && (pending.pid <= 1
                || !pending.identity.starts_with(&format!("{}:", pending.pid))
                || !matches!(pending.scope.as_str(), "blind-child" | "shared-host")
                || !pending.prepared_at.is_finite()
                || pending.guard_deadline.is_some_and(|deadline| {
                    !deadline.is_finite() || deadline < pending.prepared_at
                })
                || (!pending.guard_control_id.is_empty()
                    && (pending.guard_control_id.len() > 128
                        || !pending.guard_control_id.chars().all(|character| {
                            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                        })))
                || self.stopped.get(&pending.pid.to_string()) != Some(&pending.identity))
        {
            return Err("Codex App pending physical control is invalid".to_owned());
        }
        if self.runaway_confirmations.values().any(|confirmation| {
            !confirmation.first_complete_at.is_finite()
                || !confirmation.last_complete_at.is_finite()
                || confirmation.last_complete_at < confirmation.first_complete_at
        }) {
            return Err("runaway confirmation ledger is invalid".to_owned());
        }
        Ok(())
    }

    pub fn stopped_identity(&self, pid: u32) -> Option<&str> {
        self.stopped.get(&pid.to_string()).map(String::as_str)
    }

    pub fn mark_stopped(&mut self, pid: u32, identity: String) {
        self.stopped.insert(pid.to_string(), identity);
    }

    pub fn clear_stopped(&mut self, pid: u32) {
        self.stopped.remove(&pid.to_string());
    }

    pub fn stopped_pids(&self) -> Vec<u32> {
        self.stopped
            .keys()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }

    pub fn persist(&mut self, path: &Path, now: f64) -> io::Result<()> {
        self.schema_version = 3;
        self.updated_at = rounded(now, 3);
        self.incidents.retain(|incident| {
            incident.get("status").and_then(Value::as_str) == Some("suspended")
                || now - incident_updated_at(incident) <= INCIDENT_RETENTION_S
        });
        if self.incidents.len() > 128 {
            self.incidents.drain(..self.incidents.len() - 128);
        }
        if self.notification_events.len() > 128 {
            self.notification_events
                .drain(..self.notification_events.len() - 128);
        }
        if self.federation_peer_last_fresh.len() > 128 {
            let excess = self.federation_peer_last_fresh.len() - 128;
            let mut oldest: Vec<_> = self
                .federation_peer_last_fresh
                .iter()
                .map(|(instance, timestamp)| (instance.clone(), *timestamp))
                .collect();
            oldest.sort_by(|left, right| left.1.total_cmp(&right.1));
            for (instance, _) in oldest.into_iter().take(excess) {
                self.federation_peer_last_fresh.remove(&instance);
                self.federation_peer_stale_since.remove(&instance);
            }
        }
        let known_peers = &self.federation_peer_last_fresh;
        self.federation_peer_stale_since
            .retain(|instance, _| known_peers.contains_key(instance));
        self.logical_agents
            .retain(|_, agent| agent.active || now - agent.last_seen_at <= INCIDENT_RETENTION_S);
        write_atomic_json(path, self, 0o600, true)
    }

    pub fn transition_incident(
        &mut self,
        identity: &str,
        status: &str,
        now: f64,
        source: &str,
        updates: Map<String, Value>,
    ) -> &mut Value {
        let index = self
            .incidents
            .iter()
            .rposition(|incident| {
                incident.get("identity").and_then(Value::as_str) == Some(identity)
                    && matches!(
                        incident.get("status").and_then(Value::as_str),
                        Some("suspended" | "probation" | "probation_failed")
                    )
            })
            .unwrap_or_else(|| {
                let pid = identity.split_once(':').map(|value| value.0).unwrap_or("0");
                self.incidents.push(json!({
                    "id": format!("{}-{pid}-{}", self.instance, unique_nonce()),
                    "source": self.instance,
                    "pid": pid.parse::<u32>().unwrap_or_default(),
                    "identity": identity,
                    "reason": "recovered-runtime",
                    "suspended_at": rounded(now, 3),
                }));
                self.incidents.len() - 1
            });
        let incident = self.incidents[index]
            .as_object_mut()
            .expect("incident object");
        incident.insert("status".to_owned(), Value::String(status.to_owned()));
        incident.insert(
            "transition_source".to_owned(),
            Value::String(source.to_owned()),
        );
        incident.insert("updated_at".to_owned(), json!(rounded(now, 3)));
        incident.insert(format!("{status}_at"), json!(rounded(now, 3)));
        incident.extend(updates);
        &mut self.incidents[index]
    }
}

pub fn incident_updated_at(incident: &Value) -> f64 {
    incident
        .get("updated_at")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or_default()
}

pub fn rounded(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

pub fn unique_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "memory-supervisor-runtime-{}-{}",
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
    fn paused_identity_survives_restart_and_corruption_degrades_safely() {
        let directory = temp_directory();
        let path = directory.join("runtime.json");
        let mut ledger = RuntimeLedger::empty("linux-test");
        ledger.mark_stopped(42, "42:start-token".to_owned());
        ledger
            .incidents
            .push(json!({"id":"i1","status":"suspended"}));
        ledger.persist(&path, 100.0).unwrap();
        let (loaded, error) = RuntimeLedger::load(&path, "linux-test", 101.0);
        assert!(error.is_none());
        assert_eq!(loaded.stopped_identity(42), Some("42:start-token"));

        fs::write(&path, b"not-json").unwrap();
        let (degraded, error) = RuntimeLedger::load(&path, "linux-test", 102.0);
        assert!(degraded.stopped.is_empty());
        assert!(error.unwrap().contains("JSON error"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn lifecycle_only_epochs_are_removed_on_load() {
        let directory = temp_directory();
        let path = directory.join("runtime.json");
        let mut ledger = RuntimeLedger::empty("linux-test");
        ledger.logical_epoch = 15;
        ledger.logical_agents.insert(
            "codex:s1:root".to_owned(),
            LogicalAgent {
                key: "codex:s1:root".to_owned(),
                provider: "codex".to_owned(),
                session_id: "s1".to_owned(),
                agent_type: "lead".to_owned(),
                epoch: 15,
                ..LogicalAgent::default()
            },
        );
        ledger.persist(&path, 100.0).unwrap();

        let (loaded, error) = RuntimeLedger::load(&path, "linux-test", 101.0);
        assert!(error.is_none());
        assert_eq!(loaded.logical_epoch, 0);
        assert_eq!(loaded.logical_agents["codex:s1:root"].epoch, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn federation_peer_history_is_bounded_and_keeps_its_subset_invariant() {
        let directory = temp_directory();
        let path = directory.join("runtime.json");
        let mut ledger = RuntimeLedger::empty("linux-test");
        for index in 0..130 {
            let instance = format!("peer-{index:03}");
            ledger
                .federation_peer_last_fresh
                .insert(instance.clone(), index as f64);
            ledger
                .federation_peer_stale_since
                .insert(instance, index as f64);
        }
        ledger.persist(&path, 200.0).unwrap();
        assert_eq!(ledger.federation_peer_last_fresh.len(), 128);
        assert_eq!(ledger.federation_peer_stale_since.len(), 128);
        assert!(!ledger.federation_peer_last_fresh.contains_key("peer-000"));
        let (loaded, error) = RuntimeLedger::load(&path, "linux-test", 201.0);
        assert!(error.is_none());
        assert_eq!(loaded.federation_peer_last_fresh.len(), 128);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn malformed_notification_delivery_state_degrades_without_reaching_dispatch() {
        let directory = temp_directory();
        let path = directory.join("runtime.json");
        let mut ledger = RuntimeLedger::empty("linux-test");
        ledger.notification_events.push(json!({
            "event_id":"event-1",
            "deliveries":"not-an-object"
        }));
        fs::write(&path, serde_json::to_vec(&ledger).unwrap()).unwrap();

        let (loaded, error) = RuntimeLedger::load(&path, "linux-test", 101.0);
        assert!(loaded.notification_events.is_empty());
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.contains("object deliveries"))
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn codex_app_pending_pause_requires_a_durable_exact_recovery_record() {
        let mut ledger = RuntimeLedger::empty("linux-test");
        ledger.mark_stopped(42, "42:start-token".to_owned());
        ledger.codex_app.control.pending_physical =
            Some(crate::codex_app::CodexAppPendingPhysical {
                pid: 42,
                identity: "42:start-token".to_owned(),
                scope: "shared-host".to_owned(),
                prepared_at: 100.0,
                guard_deadline: Some(110.0),
                guard_control_id: "42-guard".to_owned(),
            });
        assert!(ledger.validate().is_ok());

        ledger.clear_stopped(42);
        assert_eq!(
            ledger.validate(),
            Err("Codex App pending physical control is invalid".to_owned())
        );

        ledger.mark_stopped(42, "42:start-token".to_owned());
        ledger
            .codex_app
            .control
            .pending_physical
            .as_mut()
            .unwrap()
            .guard_deadline = Some(99.0);
        assert_eq!(
            ledger.validate(),
            Err("Codex App pending physical control is invalid".to_owned())
        );
    }
}
