use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};

use crate::containment::{HookObservation, LogicalState, logical_key};
use crate::policy::{ProcessInfo, process_identity, target_provider};

pub const APP_SERVER_SURFACE: &str = "app-server";
const FINISHED_TOOL_GRACE_S: f64 = 5.0;
const UNOWNED_TOOL_RETENTION_S: f64 = 300.0;
const THREAD_RETENTION_S: f64 = 3600.0;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodexAppOwnershipEvidence {
    /// `CODEX_THREAD_ID`, hook baseline, server generation and process identity agree.
    ThreadConfirmed,
    /// A child inherited a confirmed owner through an unbroken process tree.
    InheritedConfirmed,
    /// Timing/command/cwd identify one invocation, but no confirmed marker exists.
    #[default]
    ThreadEstimated,
}

impl CodexAppOwnershipEvidence {
    pub fn control_safe(self) -> bool {
        matches!(self, Self::ThreadConfirmed | Self::InheritedConfirmed)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodexAppThread {
    pub key: String,
    pub session_id: String,
    pub app_server_pid: u32,
    pub app_server_identity: String,
    pub active: bool,
    pub started_at: f64,
    pub last_seen_at: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodexAppInvocation {
    pub id: String,
    pub thread_key: String,
    pub logical_key: String,
    pub app_server_pid: u32,
    pub turn_id: Option<String>,
    pub tool_use_id: Option<String>,
    pub tool_name: Option<String>,
    pub command_hint: Option<String>,
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_marker: Option<String>,
    pub baseline_pids: Vec<u32>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodexAppProcessOwner {
    pub identity: String,
    pub pid: u32,
    pub app_server_pid: u32,
    pub thread_key: String,
    pub logical_key: String,
    pub invocation_id: String,
    pub evidence: CodexAppOwnershipEvidence,
    pub assigned_at: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodexAppLedger {
    pub threads: BTreeMap<String, CodexAppThread>,
    pub invocations: BTreeMap<String, CodexAppInvocation>,
    /// Exact process identity is the key. A PID by itself is never durable ownership evidence.
    pub process_owners: BTreeMap<String, CodexAppProcessOwner>,
    /// A session observed under multiple live app-server generations cannot be targeted safely.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity_collisions: BTreeMap<String, BTreeSet<u32>>,
    #[serde(default, skip_serializing_if = "CodexAppControlState::is_default")]
    pub control: CodexAppControlState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodexAppControlState {
    pub mode: String,
    pub surface_gate: bool,
    pub surface_gate_since: Option<f64>,
    pub reason: String,
    pub last_action_at: f64,
    pub last_blind_target_at: f64,
    pub recovery_since: Option<f64>,
    pub pending_physical: Option<CodexAppPendingPhysical>,
}

impl CodexAppControlState {
    fn is_default(value: &Self) -> bool {
        value == &Self::default()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppPendingPhysical {
    pub pid: u32,
    pub identity: String,
    pub scope: String,
    pub prepared_at: f64,
    pub guard_deadline: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard_control_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppPressureProfile {
    pub causal: bool,
    pub mode: String,
    pub app_growth_mb_s: f64,
    pub confirmed_growth_mb_s: f64,
    pub estimated_growth_mb_s: f64,
    pub blind_child_growth_mb_s: f64,
    pub shared_host_growth_mb_s: f64,
    pub blind_ratio: f64,
    pub control_horizon_s: f64,
    pub reserve_tte_s: Option<f64>,
    pub remaining_steps: usize,
    pub due_steps_now: usize,
    pub selected_keys: Vec<String>,
    pub backstop: String,
    pub reason: String,
}

impl CodexAppPressureProfile {
    fn is_default(value: &Self) -> bool {
        value == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexAppLogicalCandidate {
    pub key: String,
    pub role: String,
    pub state: LogicalState,
    pub state_since: f64,
    pub growth_mb_s: f64,
    pub confirmed: bool,
    pub blind_possible: bool,
    pub heavy_or_in_flight: bool,
    pub newest_at: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexAppPlanInput {
    pub now: f64,
    pub tick_s: f64,
    pub reaction_s: f64,
    pub reserve_tte_s: Option<f64>,
    pub collapse_imminent: bool,
    pub causal: bool,
    pub app_growth_mb_s: f64,
    pub blind_ratio: f64,
    pub has_physical_backstop: bool,
    /// Logical agents whose current hook acknowledgement is required before the selected
    /// physical App backstop can actually be used. An advertised backstop without a reachable
    /// acknowledgement route is deliberately treated as absent.
    pub backstop_required_keys: Vec<String>,
    /// Blind-child and shared-host brakes add possible owners one at a time and observe each
    /// choice for a reaction interval. Confirmed child brakes do not need that serial search.
    pub backstop_blind: bool,
    /// Time for which the earliest required lead acknowledgement can remain current. The planner
    /// must not advertise a physical brake whose serial logical route outlives that evidence.
    pub backstop_receipt_budget_s: Option<f64>,
    pub surface_gate_active: bool,
    pub last_blind_target_at: f64,
    pub candidates: Vec<CodexAppLogicalCandidate>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodexAppControlPlan {
    pub surface_gate: bool,
    pub physical_backstop_reachable: bool,
    pub horizon_s: f64,
    pub remaining_steps: usize,
    pub due_steps_now: usize,
    pub selected_keys: Vec<String>,
    pub targets: Vec<(String, LogicalState)>,
}

fn logical_steps_remaining(state: LogicalState) -> usize {
    match state {
        LogicalState::Active => 3,
        LogicalState::NoExpansion => 2,
        LogicalState::LightWorkOnly => 1,
        LogicalState::HandoffOnly => 0,
    }
}

/// Receding-horizon App planner. It chooses the smallest currently useful causal set, moves each
/// target by at most one state per normal tick, and leaves unrelated idle threads untouched.
pub fn plan_app_control(mut input: CodexAppPlanInput) -> CodexAppControlPlan {
    if !input.causal || input.candidates.is_empty() {
        return CodexAppControlPlan::default();
    }
    input.candidates.sort_by(|left, right| {
        right
            .confirmed
            .cmp(&left.confirmed)
            .then_with(|| right.growth_mb_s.total_cmp(&left.growth_mb_s))
            .then_with(|| right.heavy_or_in_flight.cmp(&left.heavy_or_in_flight))
            .then_with(|| (right.role == "subagent").cmp(&(left.role == "subagent")))
            .then_with(|| right.state.cmp(&left.state))
            .then_with(|| right.newest_at.total_cmp(&left.newest_at))
    });

    let mut selected = Vec::<CodexAppLogicalCandidate>::new();
    // Never abandon an App target in the middle of an episode.
    selected.extend(
        input
            .candidates
            .iter()
            .filter(|candidate| candidate.state != LogicalState::Active)
            .cloned(),
    );
    let required_growth = (input.app_growth_mb_s * 0.60).max(1.0);
    let mut covered_growth: f64 = selected
        .iter()
        .map(|candidate| candidate.growth_mb_s.max(0.0))
        .sum();
    let confirmed_growth: f64 = input
        .candidates
        .iter()
        .filter(|candidate| candidate.confirmed)
        .map(|candidate| candidate.growth_mb_s.max(0.0))
        .sum();
    // Exact ownership is useful evidence, but it is not causal evidence by itself. If confirmed
    // children collectively explain less than the same 25% floor used by the App causality gate,
    // walking every exact-but-stable thread would only reduce performance while blind/host growth
    // remains dominant. In that case the surface cushion and one blind candidate are the smaller
    // effective controls.
    if confirmed_growth >= (input.app_growth_mb_s * 0.25).max(1.0) {
        let confirmed_target = required_growth.min(confirmed_growth);
        for candidate in input
            .candidates
            .iter()
            .filter(|candidate| candidate.confirmed && candidate.growth_mb_s >= 1.0)
        {
            if covered_growth >= confirmed_target {
                break;
            }
            if !selected.iter().any(|value| value.key == candidate.key) {
                covered_growth += candidate.growth_mb_s.max(0.0);
                selected.push(candidate.clone());
            }
        }
    }
    if input.has_physical_backstop && !input.backstop_blind {
        for key in &input.backstop_required_keys {
            if selected.iter().any(|candidate| &candidate.key == key) {
                continue;
            }
            if let Some(candidate) = input
                .candidates
                .iter()
                .find(|candidate| &candidate.key == key)
            {
                selected.push(candidate.clone());
            }
        }
    }

    let blind_already_waiting = selected.iter().any(|candidate| {
        candidate.blind_possible
            && candidate.state == LogicalState::HandoffOnly
            && input.now - candidate.state_since < input.reaction_s
    });
    let blind_target_active = selected.iter().any(|candidate| candidate.blind_possible);
    let may_add_blind = !blind_already_waiting
        && (!blind_target_active
            || input.now - input.last_blind_target_at >= input.reaction_s.max(input.tick_s));
    if input.blind_ratio >= 0.20
        && may_add_blind
        && let Some(candidate) = input.candidates.iter().find(|candidate| {
            candidate.blind_possible
                && candidate.state != LogicalState::HandoffOnly
                && !selected.iter().any(|value| value.key == candidate.key)
        })
    {
        selected.push(candidate.clone());
    }
    if selected.is_empty()
        && let Some(candidate) = input.candidates.first()
    {
        selected.push(candidate.clone());
    }

    let current_remaining_steps: usize = selected
        .iter()
        .map(|candidate| logical_steps_remaining(candidate.state))
        .sum();
    let parallel = selected
        .iter()
        .filter(|candidate| candidate.state != LogicalState::HandoffOnly)
        .count()
        .max(1);
    let surface_step_s = if input.blind_ratio >= 0.20 && !input.surface_gate_active {
        input.tick_s.max(0.25)
    } else {
        0.0
    };
    let tick_s = input.tick_s.max(0.25);
    let ladder_s = current_remaining_steps.div_ceil(parallel) as f64 * tick_s + surface_step_s;
    let required_keys: BTreeSet<_> = input.backstop_required_keys.iter().cloned().collect();
    let mut backstop_reachable = input.has_physical_backstop
        && !required_keys.is_empty()
        && required_keys.iter().all(|key| {
            input
                .candidates
                .iter()
                .any(|candidate| &candidate.key == key)
        });
    let pending_backstop: Vec<_> = if backstop_reachable {
        input
            .candidates
            .iter()
            .filter(|candidate| {
                required_keys.contains(&candidate.key)
                    && candidate.state != LogicalState::HandoffOnly
                    && !selected
                        .iter()
                        .any(|selected| selected.key == candidate.key)
            })
            .collect()
    } else {
        Vec::new()
    };
    let contingency_steps: usize = pending_backstop
        .iter()
        .map(|candidate| logical_steps_remaining(candidate.state))
        .sum();
    let contingency_wait_s = if input.backstop_blind && !pending_backstop.is_empty() {
        let selected_blind_unfinished = selected.iter().any(|candidate| {
            candidate.blind_possible && candidate.state != LogicalState::HandoffOnly
        });
        let selected_blind_wait = selected
            .iter()
            .filter(|candidate| {
                candidate.blind_possible && candidate.state == LogicalState::HandoffOnly
            })
            .map(|candidate| (input.reaction_s - (input.now - candidate.state_since)).max(0.0))
            .fold(0.0, f64::max);
        let first_wait = if selected_blind_unfinished {
            input.reaction_s
        } else {
            selected_blind_wait
        };
        first_wait + pending_backstop.len().saturating_sub(1) as f64 * input.reaction_s
    } else {
        0.0
    };
    let contingency_s = contingency_steps as f64 * tick_s + contingency_wait_s;
    let uncertainty_s = input.blind_ratio.clamp(0.0, 1.0) * input.reaction_s;
    // A current receipt is authority only while the exact epoch/state acknowledgement remains
    // live. If the advertised serial route would outlast the earliest receipt, that physical
    // target is still a possible emergency candidate later, but it cannot shorten today's
    // stopping distance.
    if backstop_reachable
        && input
            .backstop_receipt_budget_s
            .is_none_or(|budget| budget + 0.001 < ladder_s + contingency_s + uncertainty_s)
    {
        backstop_reachable = false;
    }
    let no_backstop_s = if backstop_reachable {
        0.0
    } else {
        input.reaction_s
    };
    let horizon_s = ladder_s + contingency_s + uncertainty_s + no_backstop_s;
    let remaining_steps = current_remaining_steps + contingency_steps;
    let reserve_tte = input.reserve_tte_s.unwrap_or(f64::INFINITY);
    if reserve_tte > horizon_s && !input.collapse_imminent {
        return CodexAppControlPlan {
            surface_gate: false,
            physical_backstop_reachable: backstop_reachable,
            horizon_s,
            remaining_steps,
            selected_keys: selected
                .iter()
                .map(|candidate| candidate.key.clone())
                .collect(),
            ..CodexAppControlPlan::default()
        };
    }

    let finish_ticks = ((reserve_tte - uncertainty_s - no_backstop_s - contingency_s) / tick_s)
        .floor()
        .max(0.0) as usize;
    let future_steps = finish_ticks.saturating_sub(1).saturating_mul(parallel);
    let mut due_steps = current_remaining_steps.saturating_sub(future_steps);
    if input.collapse_imminent {
        due_steps = due_steps.max(parallel.min(current_remaining_steps));
    }
    let emergency = reserve_tte <= tick_s;
    let mut targets = Vec::new();
    for candidate in &selected {
        if due_steps == 0 || candidate.state == LogicalState::HandoffOnly {
            continue;
        }
        let target = if emergency {
            LogicalState::HandoffOnly
        } else {
            candidate.state.tighten()
        };
        due_steps = due_steps.saturating_sub(if emergency {
            logical_steps_remaining(candidate.state)
        } else {
            1
        });
        targets.push((candidate.key.clone(), target));
    }
    let applied_steps = targets
        .iter()
        .filter_map(|(key, target)| {
            selected
                .iter()
                .find(|candidate| &candidate.key == key)
                .map(|candidate| {
                    logical_steps_remaining(candidate.state)
                        .saturating_sub(logical_steps_remaining(*target))
                })
        })
        .sum();
    CodexAppControlPlan {
        surface_gate: input.blind_ratio >= 0.20,
        physical_backstop_reachable: backstop_reachable,
        horizon_s,
        remaining_steps,
        due_steps_now: applied_steps,
        selected_keys: selected
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect(),
        targets,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppThreadMemory {
    pub key: String,
    pub session_id: String,
    pub app_server_pid: u32,
    pub confirmed_rss_mb: u64,
    pub confirmed_anon_mb: u64,
    pub confirmed_pids: Vec<u32>,
    pub estimated_rss_mb: u64,
    pub estimated_anon_mb: u64,
    pub estimated_pids: Vec<u32>,
    pub active_tools: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppServerMemory {
    pub pid: u32,
    pub identity: String,
    /// Memory inside the app-server process cannot be split exactly by thread.
    pub shared_host_rss_mb: u64,
    pub shared_host_anon_mb: u64,
    /// Descendants that cannot be attributed without guessing remain here.
    pub unattributed_child_rss_mb: u64,
    pub unattributed_child_anon_mb: u64,
    pub unattributed_pids: Vec<u32>,
    /// Unattributed children that appeared in a recorded hook window. Only these may enter the
    /// blind physical last-resort path; internal/old children remain observation-only.
    pub blind_control_pids: Vec<u32>,
    pub blind_candidate_keys: BTreeMap<String, Vec<String>>,
    pub observation_only_pids: Vec<u32>,
    pub total_tree_rss_mb: u64,
    pub total_tree_anon_mb: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppHookRoute {
    pub app_server_pid: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app_server_identity: String,
    pub path: String,
    pub platform: String,
    pub status: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexAppHookTarget {
    pub app_server_pid: u32,
    pub app_server_identity: String,
    pub path: PathBuf,
    /// False means the route is only a supervisor-home fallback. It is display-only and can never
    /// prove that the active App Server actually uses this CODEX_HOME.
    pub environment_resolved: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodexAppSnapshot {
    pub detected: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ownership_capability: String,
    pub app_servers: Vec<CodexAppServerMemory>,
    pub threads: Vec<CodexAppThreadMemory>,
    pub hook_routes: Vec<CodexAppHookRoute>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity_collisions: BTreeMap<String, Vec<u32>>,
    #[serde(default, skip_serializing_if = "CodexAppControlState::is_default")]
    pub control: CodexAppControlState,
    #[serde(default, skip_serializing_if = "CodexAppPressureProfile::is_default")]
    pub pressure: CodexAppPressureProfile,
}

pub fn is_codex_app_server(process: &ProcessInfo) -> bool {
    target_provider(process) == Some("codex")
        && process
            .args
            .iter()
            .any(|argument| argument.trim_matches('"') == APP_SERVER_SURFACE)
}

pub fn app_server_pids(processes: &BTreeMap<u32, ProcessInfo>) -> BTreeSet<u32> {
    processes
        .iter()
        .filter_map(|(pid, process)| is_codex_app_server(process).then_some(*pid))
        .collect()
}

fn descends_from(mut pid: u32, root_pid: u32, processes: &BTreeMap<u32, ProcessInfo>) -> bool {
    let mut visited = 0;
    while pid > 1 && visited < 128 {
        if pid == root_pid {
            return true;
        }
        let Some(process) = processes.get(&pid) else {
            return false;
        };
        pid = process.ppid;
        visited += 1;
    }
    false
}

pub fn process_descends_from(
    pid: u32,
    root_pid: u32,
    processes: &BTreeMap<u32, ProcessInfo>,
) -> bool {
    descends_from(pid, root_pid, processes)
}

fn normalized_program(value: &str) -> String {
    value
        .trim_matches(|character| matches!(character, '"' | '\'' | '(' | ')' | '[' | ']'))
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".exe")
        .to_lowercase()
}

fn command_tokens(command: &str) -> BTreeSet<String> {
    const IGNORED: &[&str] = &[
        "bash",
        "sh",
        "zsh",
        "fish",
        "cmd",
        "cmd.exe",
        "powershell",
        "powershell.exe",
        "pwsh",
        "env",
        "sudo",
        "nohup",
        "command",
    ];
    command
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ';' | '|' | '&' | '<' | '>' | '`')
        })
        .filter(|part| !part.is_empty() && !part.starts_with('-') && !part.contains('='))
        .map(normalized_program)
        .filter(|part| {
            !part.is_empty()
                && !IGNORED.contains(&part.as_str())
                && part.chars().any(|character| character.is_alphanumeric())
        })
        .take(32)
        .collect()
}

fn invocation_process_score(invocation: &CodexAppInvocation, process: &ProcessInfo) -> u32 {
    let Some(command) = invocation.command_hint.as_deref() else {
        return 0;
    };
    let tokens = command_tokens(command);
    if tokens.is_empty() {
        return 0;
    }
    let name = normalized_program(&process.name);
    let mut score = u32::from(tokens.contains(&name)) * 4;
    let process_programs: BTreeSet<_> = process
        .args
        .iter()
        .map(|argument| normalized_program(argument))
        .collect();
    score += tokens.intersection(&process_programs).count() as u32;
    let joined = process.args.join(" ").to_lowercase();
    score += tokens
        .iter()
        .filter(|token| {
            token.len() >= 3
                && joined
                    .split(|character: char| !character.is_alphanumeric() && character != '_')
                    .any(|part| part == token.as_str())
        })
        .count() as u32;
    score
}

fn invocation_id(observation: &HookObservation) -> String {
    format!(
        "{}:{}:{}",
        observation.app_server_pid.unwrap_or_default(),
        observation.key(),
        observation
            .tool_use_id
            .as_deref()
            .unwrap_or(observation.id.as_str())
    )
}

fn invocation_can_own(invocation: &CodexAppInvocation, pid: u32, root_pid: u32, now: f64) -> bool {
    invocation.app_server_pid == root_pid
        && invocation.started_at <= now + 1.0
        && !invocation.baseline_pids.contains(&pid)
        && invocation
            .ended_at
            .is_none_or(|ended| now - ended <= FINISHED_TOOL_GRACE_S)
}

impl CodexAppLedger {
    /// Records only Codex App hook traffic. Ordinary CLI observations are a strict no-op.
    pub fn observe(&mut self, observation: &HookObservation) -> bool {
        if observation.provider != "codex"
            || observation.surface != APP_SERVER_SURFACE
            || observation.app_server_pid.is_none()
        {
            return false;
        }
        if observation.session_id.is_empty() || observation.session_id == "nosid" {
            // A fabricated fallback id would merge unrelated App conversations. Machine-level
            // blind protection remains available without inventing a logical lead.
            return false;
        }
        let app_server_pid = observation.app_server_pid.unwrap_or_default();
        let thread_key = logical_key("codex", &observation.session_id, None);
        if let Some(existing) = self.threads.get(&thread_key)
            && existing.app_server_pid != app_server_pid
        {
            let collision = self.identity_collisions.entry(thread_key).or_default();
            collision.insert(existing.app_server_pid);
            collision.insert(app_server_pid);
            // Do not move a logical identity between two live-looking generations. Reconcile
            // will clear dead generations, and a later hook can register the surviving one.
            return false;
        }
        let thread = self
            .threads
            .entry(thread_key.clone())
            .or_insert_with(|| CodexAppThread {
                key: thread_key.clone(),
                session_id: observation.session_id.clone(),
                app_server_pid,
                app_server_identity: String::new(),
                active: true,
                started_at: observation.observed_at,
                last_seen_at: observation.observed_at,
            });
        if observation.observed_at + 0.001 < thread.last_seen_at {
            return false;
        }
        thread.app_server_pid = app_server_pid;
        thread.last_seen_at = observation.observed_at;
        match observation.event.as_str() {
            "SessionStart" | "UserPromptSubmit" | "BeforeAgent" | "SubagentStart" => {
                thread.active = true;
            }
            "SessionEnd" => {
                thread.active = false;
                for invocation in self
                    .invocations
                    .values_mut()
                    .filter(|invocation| invocation.thread_key == thread_key)
                {
                    invocation.ended_at.get_or_insert(observation.observed_at);
                }
            }
            "Stop" => {
                for invocation in self
                    .invocations
                    .values_mut()
                    .filter(|invocation| invocation.thread_key == thread_key)
                {
                    invocation.ended_at.get_or_insert(observation.observed_at);
                }
            }
            "PreToolUse" if !observation.blocked => {
                let id = invocation_id(observation);
                self.invocations.insert(
                    id.clone(),
                    CodexAppInvocation {
                        id,
                        thread_key,
                        logical_key: observation.key(),
                        app_server_pid,
                        turn_id: observation.turn_id.clone(),
                        tool_use_id: observation.tool_use_id.clone(),
                        tool_name: observation.tool_name.clone(),
                        command_hint: observation.command_hint.clone(),
                        cwd: observation.cwd.clone(),
                        thread_marker: observation.thread_marker.clone().filter(|marker| {
                            marker == &observation.session_id
                                || observation.agent_id.as_ref() == Some(marker)
                        }),
                        baseline_pids: observation.app_server_baseline_pids.clone(),
                        started_at: observation.observed_at,
                        ended_at: None,
                    },
                );
            }
            "PostToolUse" | "AfterTool" => {
                let exact = invocation_id(observation);
                if let Some(invocation) = self.invocations.get_mut(&exact) {
                    invocation.ended_at = Some(observation.observed_at);
                } else if let Some((_, invocation)) = self
                    .invocations
                    .iter_mut()
                    .filter(|(_, invocation)| {
                        invocation.logical_key == observation.key()
                            && invocation.ended_at.is_none()
                            && invocation.tool_name == observation.tool_name
                    })
                    .max_by(|left, right| left.1.started_at.total_cmp(&right.1.started_at))
                {
                    invocation.ended_at = Some(observation.observed_at);
                }
            }
            _ => {}
        }
        true
    }

    pub fn is_shared_host(&self, pid: u32) -> bool {
        self.threads
            .values()
            .any(|thread| thread.app_server_pid == pid)
    }

    pub fn owner_for_pid(&self, pid: u32) -> Option<&CodexAppProcessOwner> {
        self.process_owners.values().find(|owner| owner.pid == pid)
    }

    pub fn owned_pids_for_logical(&self, key: &str) -> Vec<u32> {
        self.process_owners
            .values()
            .filter_map(|owner| (owner.logical_key == key).then_some(owner.pid))
            .collect()
    }

    pub fn confirmed_pids_for_logical(&self, key: &str) -> Vec<u32> {
        self.process_owners
            .values()
            .filter_map(|owner| {
                (owner.logical_key == key && owner.evidence.control_safe()).then_some(owner.pid)
            })
            .collect()
    }

    pub fn control_safe_owner_for_pid(&self, pid: u32) -> Option<&CodexAppProcessOwner> {
        self.owner_for_pid(pid)
            .filter(|owner| owner.evidence.control_safe())
    }

    pub fn reconcile(
        &mut self,
        now: f64,
        processes: &BTreeMap<u32, ProcessInfo>,
    ) -> (CodexAppSnapshot, bool) {
        self.reconcile_with_markers(now, processes, process_thread_marker)
    }

    pub fn reconcile_with_markers(
        &mut self,
        now: f64,
        processes: &BTreeMap<u32, ProcessInfo>,
        marker_for_pid: impl Fn(u32) -> Option<String>,
    ) -> (CodexAppSnapshot, bool) {
        let before_threads = self.threads.clone();
        let before_invocations = self.invocations.clone();
        let before_owners = self.process_owners.clone();
        let before_collisions = self.identity_collisions.clone();
        let roots = app_server_pids(processes);

        self.identity_collisions.retain(|_, pids| {
            pids.retain(|pid| roots.contains(pid));
            pids.len() > 1
        });

        let working_threads: BTreeSet<_> = self
            .invocations
            .values()
            .filter(|invocation| invocation.ended_at.is_none())
            .map(|invocation| invocation.thread_key.clone())
            .chain(
                self.process_owners
                    .values()
                    .map(|owner| owner.thread_key.clone()),
            )
            .collect();
        self.threads.retain(|_, thread| {
            let same_server = processes
                .get(&thread.app_server_pid)
                .is_some_and(|process| {
                    thread.app_server_identity.is_empty()
                        || thread.app_server_identity == process_identity(process)
                });
            roots.contains(&thread.app_server_pid)
                && same_server
                && (now - thread.last_seen_at <= THREAD_RETENTION_S
                    || working_threads.contains(&thread.key))
        });
        for thread in self.threads.values_mut() {
            if let Some(process) = processes.get(&thread.app_server_pid) {
                thread.app_server_identity = process_identity(process);
            }
        }
        let owned_invocations: BTreeSet<_> = self
            .process_owners
            .values()
            .map(|owner| owner.invocation_id.clone())
            .collect();
        self.invocations.retain(|_, invocation| {
            let owns_process = owned_invocations.contains(&invocation.id);
            self.threads.contains_key(&invocation.thread_key)
                && roots.contains(&invocation.app_server_pid)
                && invocation.ended_at.map_or_else(
                    || owns_process || now - invocation.started_at <= UNOWNED_TOOL_RETENTION_S,
                    |ended| now - ended <= FINISHED_TOOL_GRACE_S,
                )
        });
        self.process_owners.retain(|identity, owner| {
            roots.contains(&owner.app_server_pid)
                && self.threads.contains_key(&owner.thread_key)
                && processes.get(&owner.pid).is_some_and(|process| {
                    process_identity(process) == *identity
                        && descends_from(owner.pid, owner.app_server_pid, processes)
                })
        });

        let mut owner_by_pid: BTreeMap<u32, CodexAppProcessOwner> = self
            .process_owners
            .values()
            .map(|owner| (owner.pid, owner.clone()))
            .collect();
        let mut candidates: Vec<_> = processes
            .values()
            .filter(|process| {
                !roots.contains(&process.pid)
                    && !owner_by_pid.contains_key(&process.pid)
                    && roots
                        .iter()
                        .any(|root| descends_from(process.pid, *root, processes))
                    && !normalized_program(&process.name).contains("memory-supervisor")
            })
            .map(|process| process.pid)
            .collect();
        candidates.sort_by_key(|pid| {
            let mut depth = 0;
            let mut current = *pid;
            while let Some(process) = processes.get(&current) {
                if roots.contains(&current) || depth >= 128 {
                    break;
                }
                depth += 1;
                current = process.ppid;
            }
            depth
        });

        for pid in candidates {
            let process = &processes[&pid];
            if let Some(parent_owner) = owner_by_pid.get(&process.ppid).cloned() {
                let mut owner = parent_owner;
                owner.pid = pid;
                owner.identity = process_identity(process);
                owner.evidence = if owner.evidence.control_safe() {
                    CodexAppOwnershipEvidence::InheritedConfirmed
                } else {
                    CodexAppOwnershipEvidence::ThreadEstimated
                };
                self.process_owners
                    .insert(owner.identity.clone(), owner.clone());
                owner_by_pid.insert(pid, owner);
                continue;
            }
            let Some(root_pid) = roots
                .iter()
                .copied()
                .find(|root| descends_from(pid, *root, processes))
            else {
                continue;
            };
            let eligible_invocations: Vec<_> = self
                .invocations
                .values()
                .filter(|invocation| invocation_can_own(invocation, pid, root_pid, now))
                .collect();
            let process_marker = marker_for_pid(pid).filter(|marker| !marker.is_empty());
            let marker_matches: Vec<_> = process_marker
                .as_ref()
                .into_iter()
                .flat_map(|marker| {
                    eligible_invocations
                        .iter()
                        .copied()
                        .filter(move |invocation| invocation.thread_marker.as_ref() == Some(marker))
                })
                .collect();
            let mut confirmed = marker_matches.first().and_then(|first| {
                marker_matches
                    .iter()
                    .all(|candidate| {
                        candidate.thread_key == first.thread_key
                            && candidate.logical_key == first.logical_key
                    })
                    .then(|| {
                        marker_matches
                            .iter()
                            .copied()
                            .max_by(|left, right| left.started_at.total_cmp(&right.started_at))
                            .unwrap_or(first)
                    })
            });
            let mut eligible: Vec<_> = eligible_invocations
                .iter()
                .filter_map(|invocation| {
                    let score = invocation_process_score(invocation, process);
                    (score > 0).then_some((*invocation, score))
                })
                .collect();
            eligible.sort_by_key(|(_, score)| *score);
            let estimated = eligible.pop().and_then(|best| {
                eligible
                    .last()
                    .is_none_or(|(_, score)| score != &best.1)
                    .then_some(best.0)
            });
            let marker_contradiction = confirmed.is_some_and(|marker_owner| {
                estimated.is_some_and(|command_owner| {
                    command_owner.thread_key != marker_owner.thread_key
                        || command_owner.logical_key != marker_owner.logical_key
                })
            });
            if marker_contradiction {
                // A marker that contradicts a unique command/window match is not confirmed
                // evidence. Keep the process blind instead of choosing either story.
                confirmed = None;
            }
            let (invocation, evidence) = if let Some(invocation) = confirmed {
                (invocation, CodexAppOwnershipEvidence::ThreadConfirmed)
            } else if !marker_contradiction && let Some(invocation) = estimated {
                (invocation, CodexAppOwnershipEvidence::ThreadEstimated)
            } else {
                // Concurrent or command-ambiguous work remains in the blind pool.
                continue;
            };
            let identity = process_identity(process);
            let owner = CodexAppProcessOwner {
                identity: identity.clone(),
                pid,
                app_server_pid: root_pid,
                thread_key: invocation.thread_key.clone(),
                logical_key: invocation.logical_key.clone(),
                invocation_id: invocation.id.clone(),
                evidence,
                assigned_at: now,
            };
            self.process_owners.insert(identity, owner.clone());
            owner_by_pid.insert(pid, owner);
        }

        let mut snapshot = CodexAppSnapshot {
            detected: !roots.is_empty(),
            control: self.control.clone(),
            identity_collisions: self
                .identity_collisions
                .iter()
                .map(|(key, pids)| (key.clone(), pids.iter().copied().collect()))
                .collect(),
            ..CodexAppSnapshot::default()
        };
        for thread in self.threads.values() {
            let owners: Vec<_> = self
                .process_owners
                .values()
                .filter(|owner| owner.thread_key == thread.key)
                .collect();
            let mut confirmed_pids: Vec<_> = owners
                .iter()
                .filter_map(|owner| owner.evidence.control_safe().then_some(owner.pid))
                .collect();
            confirmed_pids.sort_unstable();
            confirmed_pids.dedup();
            let mut estimated_pids: Vec<_> = owners
                .iter()
                .filter_map(|owner| (!owner.evidence.control_safe()).then_some(owner.pid))
                .collect();
            estimated_pids.sort_unstable();
            estimated_pids.dedup();
            snapshot.threads.push(CodexAppThreadMemory {
                key: thread.key.clone(),
                session_id: thread.session_id.clone(),
                app_server_pid: thread.app_server_pid,
                confirmed_rss_mb: confirmed_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.rss_mb)
                    .sum(),
                confirmed_anon_mb: confirmed_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.anon_mb)
                    .sum(),
                confirmed_pids,
                estimated_rss_mb: estimated_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.rss_mb)
                    .sum(),
                estimated_anon_mb: estimated_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.anon_mb)
                    .sum(),
                estimated_pids,
                active_tools: self
                    .invocations
                    .values()
                    .filter(|invocation| {
                        invocation.thread_key == thread.key && invocation.ended_at.is_none()
                    })
                    .count(),
            });
        }
        snapshot
            .threads
            .sort_by(|left, right| left.key.cmp(&right.key));

        for root_pid in roots {
            let root = &processes[&root_pid];
            let descendants: Vec<_> = processes
                .values()
                .filter(|process| {
                    process.pid != root_pid && descends_from(process.pid, root_pid, processes)
                })
                .collect();
            let mut unattributed_pids: Vec<_> = descendants
                .iter()
                .filter_map(|process| {
                    self.owner_for_pid(process.pid)
                        .is_none()
                        .then_some(process.pid)
                })
                .collect();
            unattributed_pids.sort_unstable();
            let blind_candidates: BTreeMap<u32, Vec<String>> = unattributed_pids
                .iter()
                .copied()
                .filter_map(|pid| {
                    let process = &processes[&pid];
                    let window: Vec<_> = self
                        .invocations
                        .values()
                        .filter(|invocation| invocation_can_own(invocation, pid, root_pid, now))
                        .collect();
                    let possible: Vec<_> = window
                        .iter()
                        .copied()
                        .filter(|invocation| invocation_process_score(invocation, process) > 0)
                        .collect();
                    let marker = marker_for_pid(pid);
                    let marker_matches: Vec<_> = marker
                        .as_ref()
                        .into_iter()
                        .flat_map(|marker| {
                            window.iter().copied().filter(move |invocation| {
                                invocation.thread_marker.as_deref() == Some(marker.as_str())
                            })
                        })
                        .collect();
                    let contradiction = marker_matches.first().is_some_and(|marker_owner| {
                        possible.len() == 1
                            && (possible[0].thread_key != marker_owner.thread_key
                                || possible[0].logical_key != marker_owner.logical_key)
                    });
                    let ambiguous = possible.len() > 1 || marker_matches.len() > 1 || contradiction;
                    ambiguous.then(|| {
                        let mut keys: Vec<_> = possible
                            .iter()
                            .chain(marker_matches.iter())
                            .map(|invocation| invocation.logical_key.clone())
                            .collect();
                        keys.sort();
                        keys.dedup();
                        (pid, keys)
                    })
                })
                .collect();
            let blind_control: BTreeSet<u32> = blind_candidates.keys().copied().collect();
            snapshot.app_servers.push(CodexAppServerMemory {
                pid: root_pid,
                identity: process_identity(root),
                shared_host_rss_mb: root.rss_mb,
                shared_host_anon_mb: root.anon_mb,
                unattributed_child_rss_mb: unattributed_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.rss_mb)
                    .sum(),
                unattributed_child_anon_mb: unattributed_pids
                    .iter()
                    .filter_map(|pid| processes.get(pid))
                    .map(|process| process.anon_mb)
                    .sum(),
                total_tree_rss_mb: root.rss_mb
                    + descendants
                        .iter()
                        .map(|process| process.rss_mb)
                        .sum::<u64>(),
                total_tree_anon_mb: root.anon_mb
                    + descendants
                        .iter()
                        .map(|process| process.anon_mb)
                        .sum::<u64>(),
                unattributed_pids,
                blind_control_pids: blind_control.iter().copied().collect(),
                blind_candidate_keys: blind_candidates
                    .into_iter()
                    .map(|(pid, keys)| (pid.to_string(), keys))
                    .collect(),
                observation_only_pids: descendants
                    .iter()
                    .filter_map(|process| {
                        (self.owner_for_pid(process.pid).is_none()
                            && !blind_control.contains(&process.pid))
                        .then_some(process.pid)
                    })
                    .collect(),
            });
        }
        snapshot.app_servers.sort_by_key(|server| server.pid);
        let changed = self.threads != before_threads
            || self.invocations != before_invocations
            || self.process_owners != before_owners
            || self.identity_collisions != before_collisions;
        (snapshot, changed)
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn proc_process_environment(pid: u32) -> BTreeMap<String, String> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .unwrap_or_default()
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let text = String::from_utf8_lossy(entry);
            let (key, value) = text.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(windows)]
fn windows_process_environment(pid: u32) -> BTreeMap<String, String> {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;

    type Handle = *mut c_void;
    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: *mut c_void,
        peb_base_address: *mut c_void,
        reserved2: [*mut c_void; 2],
        unique_process_id: usize,
        reserved3: *mut c_void,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn ReadProcessMemory(
            process: Handle,
            address: *const c_void,
            buffer: *mut c_void,
            size: usize,
            read: *mut usize,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process: Handle,
            class: u32,
            info: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    const PROCESS_VM_READ: u32 = 0x0010;
    // These offsets are the native-width PEB and RTL_USER_PROCESS_PARAMETERS pointer fields.
    // Any Windows revision/access mismatch fails closed to the blind path.
    const PEB_PROCESS_PARAMETERS: usize = if cfg!(target_pointer_width = "64") {
        0x20
    } else {
        0x10
    };
    const PARAMETERS_ENVIRONMENT: usize = if cfg!(target_pointer_width = "64") {
        0x80
    } else {
        0x48
    };

    // SAFETY: all handles and remote pointers are validated by the Windows APIs; every read is
    // bounded, the handle is closed on all paths below, and no remote memory is written.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return BTreeMap::new();
        }
        let mut basic: ProcessBasicInformation = zeroed();
        let status = NtQueryInformationProcess(
            handle,
            0,
            (&mut basic as *mut ProcessBasicInformation).cast(),
            size_of::<ProcessBasicInformation>() as u32,
            null_mut(),
        );
        if status != 0 || basic.peb_base_address.is_null() {
            CloseHandle(handle);
            return BTreeMap::new();
        }
        let read_pointer = |address: usize| -> Option<usize> {
            let mut pointer = 0usize;
            let mut read = 0usize;
            (ReadProcessMemory(
                handle,
                address as *const c_void,
                (&mut pointer as *mut usize).cast(),
                size_of::<usize>(),
                &mut read,
            ) != 0
                && read == size_of::<usize>()
                && pointer != 0)
                .then_some(pointer)
        };
        let parameters = read_pointer(basic.peb_base_address as usize + PEB_PROCESS_PARAMETERS);
        let environment =
            parameters.and_then(|parameters| read_pointer(parameters + PARAMETERS_ENVIRONMENT));
        let Some(mut address) = environment else {
            CloseHandle(handle);
            return BTreeMap::new();
        };
        let mut units = Vec::<u16>::new();
        let mut ended = false;
        for _ in 0..256 {
            let mut chunk = [0u16; 1024];
            let mut read = 0usize;
            if ReadProcessMemory(
                handle,
                address as *const c_void,
                chunk.as_mut_ptr().cast(),
                size_of::<[u16; 1024]>(),
                &mut read,
            ) == 0
                || read < 2
            {
                break;
            }
            for unit in chunk.into_iter().take(read / 2) {
                let double_null = unit == 0 && units.last() == Some(&0);
                units.push(unit);
                if double_null {
                    ended = true;
                    break;
                }
            }
            if ended {
                break;
            }
            address = address.saturating_add(read);
        }
        CloseHandle(handle);
        if !ended {
            return BTreeMap::new();
        }
        units
            .split(|unit| *unit == 0)
            .filter_map(|entry| String::from_utf16(entry).ok())
            .filter_map(|entry| {
                let (key, value) = entry.split_once('=')?;
                (!key.is_empty()).then(|| (key.to_owned(), value.to_owned()))
            })
            .collect()
    }
}

#[cfg(target_os = "macos")]
fn macos_process_environment(pid: u32) -> BTreeMap<String, String> {
    let mut mib = [1_i32, 49_i32, pid as i32]; // CTL_KERN, KERN_PROCARGS2, pid
    let mut size = 0usize;
    // SAFETY: the first sysctl call requests only the required buffer size.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size == 0
        || size > 4 * 1024 * 1024
    {
        return BTreeMap::new();
    }
    let mut bytes = vec![0u8; size];
    // SAFETY: `bytes` owns `size` writable bytes and sysctl updates the bounded length.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return BTreeMap::new();
    }
    bytes.truncate(size);
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            (!key.is_empty()).then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn process_environment(pid: u32) -> BTreeMap<String, String> {
    #[cfg(windows)]
    {
        windows_process_environment(pid)
    }
    #[cfg(target_os = "macos")]
    {
        macos_process_environment(pid)
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    proc_process_environment(pid)
}

fn environment_value<'a>(environment: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    environment.get(key).map(String::as_str).or_else(|| {
        environment.iter().find_map(|(candidate, value)| {
            candidate
                .eq_ignore_ascii_case(key)
                .then_some(value.as_str())
        })
    })
}

/// Best-effort cross-platform marker reader. Failure is a normal signal to use estimated/blind
/// App policy; it never degrades the daemon or ordinary CLI control.
pub fn process_thread_marker(pid: u32) -> Option<String> {
    environment_value(&process_environment(pid), "CODEX_THREAD_ID")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn marker_reader_capability() -> &'static str {
    if cfg!(windows) {
        "windows-peb+baseline+blind"
    } else if cfg!(target_os = "macos") {
        "macos-procargs+baseline+blind"
    } else {
        "procfs+baseline+blind"
    }
}

fn same_home(platform: &str, left: &Path, right: &Path) -> bool {
    if platform == "windows" {
        let normalize = |value: &Path| {
            value
                .to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_lowercase()
        };
        normalize(left) == normalize(right)
    } else {
        left == right
    }
}

/// Returns one hook target per active app-server surface. CLI roots never qualify. A fallback
/// target is explicit and non-authoritative so a guessed path cannot unlock blind physical control.
pub fn hook_targets(
    platform: &str,
    processes: &BTreeMap<u32, ProcessInfo>,
) -> Vec<CodexAppHookTarget> {
    let supervisor_home = if platform == "windows" {
        env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"))
    } else {
        env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))
    }
    .map(PathBuf::from);
    let fallback = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| supervisor_home.as_ref().map(|home| home.join(".codex")));
    hook_targets_with(
        platform,
        processes,
        process_environment,
        fallback,
        supervisor_home,
    )
}

fn hook_targets_with(
    platform: &str,
    processes: &BTreeMap<u32, ProcessInfo>,
    environment: impl Fn(u32) -> BTreeMap<String, String>,
    fallback: Option<PathBuf>,
    supervisor_home: Option<PathBuf>,
) -> Vec<CodexAppHookTarget> {
    let roots = app_server_pids(processes);
    if roots.is_empty() {
        return Vec::new();
    }
    let mut targets = Vec::new();
    for pid in roots {
        let process_environment = environment(pid);
        let process_home = if platform == "windows" {
            environment_value(&process_environment, "USERPROFILE")
                .or_else(|| environment_value(&process_environment, "HOME"))
        } else {
            environment_value(&process_environment, "HOME")
                .or_else(|| environment_value(&process_environment, "USERPROFILE"))
        }
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
        let same_owner = supervisor_home.as_ref().is_some_and(|supervisor_home| {
            process_home
                .as_ref()
                .is_some_and(|process_home| same_home(platform, process_home, supervisor_home))
        });
        let resolved_home = same_owner
            .then(|| {
                environment_value(&process_environment, "CODEX_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| process_home.as_ref().map(|home| home.join(".codex")))
            })
            .flatten();
        let (home, environment_resolved) = if let Some(home) = resolved_home {
            (home, true)
        } else if !matches!(platform, "linux" | "wsl") {
            let Some(home) = fallback.clone() else {
                continue;
            };
            (home, false)
        } else {
            continue;
        };
        if home.as_os_str().is_empty() {
            continue;
        }
        targets.push(CodexAppHookTarget {
            app_server_pid: pid,
            app_server_identity: processes
                .get(&pid)
                .map(process_identity)
                .unwrap_or_default(),
            path: home.join("hooks.json"),
            environment_resolved,
        });
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn process(pid: u32, ppid: u32, name: &str, args: &[&str], rss_mb: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            name: name.to_owned(),
            rss_mb,
            anon_mb: rss_mb,
            args: args.iter().map(|value| (*value).to_owned()).collect(),
            start_token: format!("start-{pid}"),
            terminal: String::new(),
            terminal_identity: String::new(),
        }
    }

    fn observation(session: &str, tool_id: &str, command: &str, pid: u32) -> HookObservation {
        let mut observation = HookObservation::from_payload(
            format!("event-{tool_id}"),
            10.0,
            "codex",
            "PreToolUse",
            &json!({
                "session_id": session,
                "turn_id": format!("turn-{session}"),
                "tool_use_id": tool_id,
                "tool_name": "exec_command",
                "tool_input": {"command": command}
            }),
            Some(pid),
            false,
        );
        observation.mark_codex_app(pid);
        observation.thread_marker = Some(session.to_owned());
        observation
    }

    #[test]
    fn ordinary_cli_observation_is_a_strict_noop() {
        let mut ledger = CodexAppLedger::default();
        let observation = HookObservation::from_payload(
            "cli".to_owned(),
            1.0,
            "codex",
            "SessionStart",
            &json!({"session_id":"cli-session"}),
            Some(9),
            false,
        );
        assert!(!ledger.observe(&observation));
        assert_eq!(ledger, CodexAppLedger::default());
        assert_eq!(
            serde_json::to_value(&ledger).unwrap(),
            json!({"threads":{},"invocations":{},"process_owners":{}})
        );
    }

    #[test]
    fn two_threads_share_one_host_but_own_separate_tool_trees() {
        let mut ledger = CodexAppLedger::default();
        assert!(ledger.observe(&observation("one", "tool-one", "cargo test", 100)));
        assert!(ledger.observe(&observation("two", "tool-two", "npm test", 100)));
        let processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (110, process(110, 100, "cargo", &["cargo", "test"], 80)),
            (111, process(111, 110, "rustc", &["rustc"], 40)),
            (120, process(120, 100, "npm", &["npm", "test"], 60)),
            (121, process(121, 120, "node", &["node"], 30)),
        ]);
        let markers = BTreeMap::from([
            (110, "one".to_owned()),
            (111, "one".to_owned()),
            (120, "two".to_owned()),
            (121, "two".to_owned()),
        ]);
        let (snapshot, changed) =
            ledger.reconcile_with_markers(11.0, &processes, |pid| markers.get(&pid).cloned());
        assert!(changed);
        assert_eq!(snapshot.threads.len(), 2);
        assert_eq!(snapshot.threads[0].confirmed_rss_mb, 120);
        assert_eq!(snapshot.threads[0].confirmed_pids, vec![110, 111]);
        assert_eq!(snapshot.threads[1].confirmed_rss_mb, 90);
        assert_eq!(snapshot.threads[1].confirmed_pids, vec![120, 121]);
        assert_eq!(snapshot.app_servers[0].shared_host_rss_mb, 200);
        assert_eq!(snapshot.app_servers[0].total_tree_rss_mb, 410);
        assert!(snapshot.app_servers[0].unattributed_pids.is_empty());
    }

    #[test]
    fn ambiguous_process_stays_unattributed_and_shared_host_is_never_owned() {
        let mut ledger = CodexAppLedger::default();
        ledger.observe(&observation("one", "a", "python task.py", 100));
        ledger.observe(&observation("two", "b", "python other.py", 100));
        let processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (
                110,
                process(110, 100, "python", &["python", "worker.py"], 80),
            ),
        ]);
        let (snapshot, _) = ledger.reconcile(11.0, &processes);
        assert!(ledger.owner_for_pid(100).is_none());
        assert!(ledger.owner_for_pid(110).is_none());
        assert_eq!(snapshot.app_servers[0].unattributed_pids, vec![110]);
        assert_eq!(snapshot.app_servers[0].unattributed_child_rss_mb, 80);
        assert_eq!(snapshot.app_servers[0].blind_control_pids, vec![110]);
        assert_eq!(snapshot.app_servers[0].blind_candidate_keys["110"].len(), 2);
    }

    #[test]
    fn process_present_at_pretool_boundary_is_never_claimed_by_that_tool() {
        let mut ledger = CodexAppLedger::default();
        let mut hook = observation("one", "tool", "cargo test", 100);
        hook.app_server_baseline_pids = vec![110];
        ledger.observe(&hook);
        let processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (110, process(110, 100, "cargo", &["cargo", "test"], 80)),
        ]);
        let (snapshot, _) = ledger.reconcile(11.0, &processes);
        assert!(ledger.owner_for_pid(110).is_none());
        assert_eq!(snapshot.app_servers[0].unattributed_pids, vec![110]);
    }

    #[test]
    fn exact_long_running_tool_keeps_its_thread_beyond_idle_roster_retention() {
        let mut ledger = CodexAppLedger::default();
        ledger.observe(&observation("one", "tool", "cargo test", 100));
        let processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (110, process(110, 100, "cargo", &["cargo", "test"], 80)),
        ]);
        ledger.reconcile(11.0, &processes);
        let (snapshot, _) = ledger.reconcile(7_211.0, &processes);
        assert_eq!(snapshot.threads.len(), 1);
        assert_eq!(snapshot.threads[0].estimated_pids, vec![110]);
    }

    #[test]
    fn app_server_restart_drops_the_previous_process_identity_and_thread_ownership() {
        let mut ledger = CodexAppLedger::default();
        ledger.observe(&observation("one", "tool", "cargo test", 100));
        let mut processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (110, process(110, 100, "cargo", &["cargo", "test"], 80)),
        ]);
        ledger.reconcile(11.0, &processes);
        assert!(ledger.owner_for_pid(110).is_some());

        processes.get_mut(&100).unwrap().start_token = "restarted-server".to_owned();
        let (snapshot, _) = ledger.reconcile(12.0, &processes);
        assert!(snapshot.detected);
        assert!(snapshot.threads.is_empty());
        assert!(ledger.threads.is_empty());
        assert!(ledger.invocations.is_empty());
        assert!(ledger.process_owners.is_empty());
        assert_eq!(snapshot.app_servers[0].unattributed_pids, vec![110]);
    }

    #[test]
    fn app_server_detection_requires_the_exact_subcommand() {
        assert!(!is_codex_app_server(&process(
            1,
            0,
            "codex",
            &["codex", "exec", "task"],
            1
        )));
        assert!(is_codex_app_server(&process(
            2,
            0,
            "codex",
            &["codex", "-c", "feature=true", "app-server"],
            1
        )));
    }

    #[test]
    fn wsl_app_server_uses_its_codex_home_for_the_native_hook_route() {
        let processes =
            BTreeMap::from([(100, process(100, 1, "codex", &["codex", "app-server"], 200))]);
        let targets = hook_targets_with(
            "wsl",
            &processes,
            |_| {
                BTreeMap::from([
                    (
                        "CODEX_HOME".to_owned(),
                        "/mnt/c/Users/owner/.codex".to_owned(),
                    ),
                    ("HOME".to_owned(), "/home/local".to_owned()),
                ])
            },
            Some(PathBuf::from("/home/local/.codex")),
            Some(PathBuf::from("/home/local")),
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].app_server_pid, 100);
        assert!(targets[0].environment_resolved);
        assert_eq!(
            targets[0].path,
            PathBuf::from("/mnt/c/Users/owner/.codex/hooks.json")
        );

        let default_targets = hook_targets_with(
            "wsl",
            &processes,
            |_| BTreeMap::from([("HOME".to_owned(), "/home/local".to_owned())]),
            Some(PathBuf::from("/home/local/.codex")),
            Some(PathBuf::from("/home/local")),
        );
        assert_eq!(default_targets.len(), 1);
        assert!(default_targets[0].environment_resolved);
        assert_eq!(
            default_targets[0].path,
            PathBuf::from("/home/local/.codex/hooks.json")
        );
    }

    #[test]
    fn windows_and_macos_use_the_active_app_servers_environment_not_the_supervisor_fallback() {
        let processes =
            BTreeMap::from([(100, process(100, 1, "codex", &["codex", "app-server"], 200))]);
        let windows = hook_targets_with(
            "windows",
            &processes,
            |_| {
                BTreeMap::from([
                    ("USERPROFILE".to_owned(), r"C:\Users\Owner".to_owned()),
                    ("CODEX_HOME".to_owned(), r"D:\Codex\AppProfile".to_owned()),
                ])
            },
            Some(PathBuf::from(r"C:\Users\Owner\.codex")),
            Some(PathBuf::from(r"c:\users\owner")),
        );
        assert_eq!(windows.len(), 1);
        assert!(windows[0].environment_resolved);
        assert_eq!(
            windows[0].path,
            PathBuf::from(r"D:\Codex\AppProfile").join("hooks.json")
        );

        let macos = hook_targets_with(
            "macos",
            &processes,
            |_| {
                BTreeMap::from([
                    ("HOME".to_owned(), "/Users/owner".to_owned()),
                    (
                        "CODEX_HOME".to_owned(),
                        "/Volumes/Fast/codex-app".to_owned(),
                    ),
                ])
            },
            Some(PathBuf::from("/Users/owner/.codex")),
            Some(PathBuf::from("/Users/owner")),
        );
        assert_eq!(macos.len(), 1);
        assert!(macos[0].environment_resolved);
        assert_eq!(
            macos[0].path,
            PathBuf::from("/Volumes/Fast/codex-app/hooks.json")
        );
    }

    #[test]
    fn unresolved_non_linux_home_is_visible_but_never_authoritative() {
        let processes =
            BTreeMap::from([(100, process(100, 1, "codex", &["codex", "app-server"], 200))]);
        let targets = hook_targets_with(
            "windows",
            &processes,
            |_| BTreeMap::new(),
            Some(PathBuf::from(r"C:\Users\owner\.codex")),
            Some(PathBuf::from(r"C:\Users\owner")),
        );
        assert_eq!(targets.len(), 1);
        assert!(!targets[0].environment_resolved);
        assert_eq!(
            targets[0].path,
            PathBuf::from(r"C:\Users\owner\.codex").join("hooks.json")
        );
    }

    #[test]
    fn temporary_or_foreign_wsl_supervisor_cannot_rewrite_the_owners_app_hooks() {
        let processes =
            BTreeMap::from([(100, process(100, 1, "codex", &["codex", "app-server"], 200))]);
        let targets = hook_targets_with(
            "wsl",
            &processes,
            |_| {
                BTreeMap::from([
                    (
                        "CODEX_HOME".to_owned(),
                        "/mnt/c/Users/owner/.codex".to_owned(),
                    ),
                    ("HOME".to_owned(), "/home/owner".to_owned()),
                ])
            },
            Some(PathBuf::from("/tmp/install-test/.codex")),
            Some(PathBuf::from("/tmp/install-test")),
        );
        assert!(targets.is_empty());
    }

    fn candidate(key: &str, confirmed: bool, blind: bool) -> CodexAppLogicalCandidate {
        CodexAppLogicalCandidate {
            key: key.to_owned(),
            role: "subagent".to_owned(),
            state: LogicalState::Active,
            state_since: 0.0,
            growth_mb_s: if confirmed { 80.0 } else { 0.0 },
            confirmed,
            blind_possible: blind,
            heavy_or_in_flight: true,
            newest_at: 10.0,
        }
    }

    fn plan_input(blind_ratio: f64, tte: f64) -> CodexAppPlanInput {
        CodexAppPlanInput {
            now: 100.0,
            tick_s: 1.0,
            reaction_s: 5.0,
            reserve_tte_s: Some(tte),
            collapse_imminent: false,
            causal: true,
            app_growth_mb_s: 80.0,
            blind_ratio,
            has_physical_backstop: true,
            backstop_required_keys: Vec::new(),
            backstop_blind: false,
            backstop_receipt_budget_s: Some(120.0),
            surface_gate_active: false,
            last_blind_target_at: 0.0,
            candidates: Vec::new(),
        }
    }

    #[test]
    fn exact_app_work_runs_later_and_without_a_surface_wide_gate() {
        let mut exact = plan_input(0.0, 3.0);
        exact.candidates = vec![candidate("exact", true, false)];
        exact.backstop_required_keys = vec!["exact".to_owned()];
        let exact = plan_app_control(exact);
        assert_eq!(exact.horizon_s, 3.0);
        assert!(!exact.surface_gate);
        assert_eq!(
            exact.targets,
            vec![("exact".to_owned(), LogicalState::NoExpansion)]
        );

        let mut blind = plan_input(1.0, 9.0);
        blind.candidates = vec![candidate("blind", false, true)];
        blind.backstop_required_keys = vec!["blind".to_owned()];
        blind.backstop_blind = true;
        let blind = plan_app_control(blind);
        assert_eq!(blind.horizon_s, 9.0);
        assert!(blind.surface_gate);
        assert!(blind.targets.is_empty());
    }

    #[test]
    fn app_planner_waits_outside_the_calculated_horizon_and_moves_one_step_normally() {
        let mut input = plan_input(1.0, 30.0);
        input.candidates = vec![candidate("blind", false, true)];
        input.backstop_required_keys = vec!["blind".to_owned()];
        input.backstop_blind = true;
        let early = plan_app_control(input.clone());
        assert!(early.targets.is_empty());
        assert!(!early.surface_gate);

        input.reserve_tte_s = Some(8.0);
        input.surface_gate_active = true;
        let due = plan_app_control(input);
        assert_eq!(due.horizon_s, 8.0);
        assert_eq!(
            due.targets,
            vec![("blind".to_owned(), LogicalState::NoExpansion)]
        );
    }

    #[test]
    fn blind_planner_adds_one_uncertain_target_then_waits_a_reaction() {
        let mut first = candidate("first", false, true);
        first.state = LogicalState::HandoffOnly;
        first.state_since = 98.0;
        let second = candidate("second", false, true);
        let mut input = plan_input(1.0, 1.0);
        input.collapse_imminent = true;
        input.surface_gate_active = true;
        input.last_blind_target_at = 98.0;
        input.candidates = vec![first, second];
        input.backstop_required_keys = vec!["first".to_owned(), "second".to_owned()];
        input.backstop_blind = true;
        let waiting = plan_app_control(input);
        assert!(!waiting.selected_keys.contains(&"second".to_owned()));
    }

    #[test]
    fn blind_backstop_horizon_counts_every_possible_owner_without_restricting_them_early() {
        let mut input = plan_input(1.0, 26.0);
        input.backstop_required_keys =
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()];
        input.backstop_blind = true;
        input.candidates = vec![
            candidate("first", false, true),
            candidate("second", false, true),
            candidate("third", false, true),
        ];

        let outside = plan_app_control(input.clone());
        assert_eq!(outside.horizon_s, 25.0);
        assert_eq!(outside.remaining_steps, 9);
        assert_eq!(outside.selected_keys.len(), 1);
        assert!(!outside.surface_gate);
        assert!(outside.targets.is_empty());

        input.reserve_tte_s = Some(25.0);
        let boundary = plan_app_control(input);
        assert!(boundary.surface_gate);
        assert_eq!(boundary.selected_keys.len(), 1);
        assert!(boundary.targets.is_empty());
    }

    #[test]
    fn blind_horizon_finishes_the_serial_owner_search_before_the_reserve() {
        let mut candidates = vec![
            candidate("first", false, true),
            candidate("second", false, true),
            candidate("third", false, true),
        ];
        let required = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect::<Vec<_>>();
        let mut now = 100.0;
        let mut tte = 25.0;
        let mut surface_gate_active = false;
        let mut last_blind_target_at = 0.0;

        while tte >= 0.0 {
            let plan = plan_app_control(CodexAppPlanInput {
                now,
                tick_s: 1.0,
                reaction_s: 5.0,
                reserve_tte_s: Some(tte),
                collapse_imminent: false,
                causal: true,
                app_growth_mb_s: 80.0,
                blind_ratio: 1.0,
                has_physical_backstop: true,
                backstop_required_keys: required.clone(),
                backstop_blind: true,
                backstop_receipt_budget_s: Some(120.0),
                surface_gate_active,
                last_blind_target_at,
                candidates: candidates.clone(),
            });
            surface_gate_active |= plan.surface_gate;
            for (key, target) in plan.targets {
                let candidate = candidates
                    .iter_mut()
                    .find(|candidate| candidate.key == key)
                    .expect("planned target must remain in the candidate set");
                if candidate.state == LogicalState::Active {
                    last_blind_target_at = now;
                }
                candidate.state = target;
                candidate.state_since = now;
            }
            if candidates.iter().all(|candidate| {
                candidate.state == LogicalState::HandoffOnly && now - candidate.state_since >= 5.0
            }) {
                break;
            }
            now += 1.0;
            tte -= 1.0;
        }

        assert!(surface_gate_active);
        assert!(
            tte >= 0.0,
            "the advertised horizon must finish before reserve"
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.state == LogicalState::HandoffOnly && now - candidate.state_since >= 5.0
        }));
    }

    #[test]
    fn blind_dominance_does_not_select_exact_threads_that_do_not_materially_explain_growth() {
        let mut input = plan_input(0.9, 30.0);
        input.app_growth_mb_s = 100.0;
        input.backstop_required_keys = vec!["blind".to_owned()];
        input.backstop_blind = true;
        input.candidates = (0..10)
            .map(|index| {
                let mut value = candidate(&format!("exact-{index}"), true, false);
                value.growth_mb_s = 1.0;
                value
            })
            .chain(std::iter::once(candidate("blind", false, true)))
            .collect();

        let plan = plan_app_control(input);
        assert_eq!(plan.selected_keys, vec!["blind".to_owned()]);
        assert!(
            plan.selected_keys
                .iter()
                .all(|key| !key.starts_with("exact-"))
        );
    }

    #[test]
    fn expiring_receipt_cannot_shorten_the_advertised_blind_horizon() {
        let mut input = plan_input(1.0, 30.0);
        input.backstop_required_keys =
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()];
        input.backstop_blind = true;
        input.backstop_receipt_budget_s = Some(24.0);
        input.candidates = vec![
            candidate("first", false, true),
            candidate("second", false, true),
            candidate("third", false, true),
        ];

        let plan = plan_app_control(input);

        assert!(!plan.physical_backstop_reachable);
        assert_eq!(plan.horizon_s, 30.0);
        assert_eq!(plan.selected_keys.len(), 1);
    }

    #[test]
    fn same_session_on_two_live_servers_becomes_an_identity_collision() {
        let mut ledger = CodexAppLedger::default();
        assert!(ledger.observe(&observation("same", "one", "cargo test", 100)));
        assert!(!ledger.observe(&observation("same", "two", "cargo test", 200)));
        let key = logical_key("codex", "same", None);
        assert_eq!(ledger.identity_collisions[&key], BTreeSet::from([100, 200]));
        assert_eq!(ledger.threads[&key].app_server_pid, 100);
    }

    #[test]
    fn estimated_command_ownership_never_authorizes_targeted_physical_control() {
        let mut ledger = CodexAppLedger::default();
        ledger.observe(&observation("one", "tool", "cargo test", 100));
        let processes = BTreeMap::from([
            (100, process(100, 1, "codex", &["codex", "app-server"], 200)),
            (110, process(110, 100, "cargo", &["cargo", "test"], 80)),
        ]);
        ledger.reconcile_with_markers(11.0, &processes, |_| None);
        assert_eq!(
            ledger.owner_for_pid(110).unwrap().evidence,
            CodexAppOwnershipEvidence::ThreadEstimated
        );
        assert!(ledger.control_safe_owner_for_pid(110).is_none());
    }
}
