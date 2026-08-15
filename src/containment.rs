use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogicalState {
    #[default]
    Active,
    NoExpansion,
    LightWorkOnly,
    HandoffOnly,
}

impl LogicalState {
    pub fn tighten(self) -> Self {
        match self {
            Self::Active => Self::NoExpansion,
            Self::NoExpansion => Self::LightWorkOnly,
            Self::LightWorkOnly | Self::HandoffOnly => Self::HandoffOnly,
        }
    }

    pub fn relax(self) -> Self {
        match self {
            Self::Active | Self::NoExpansion => Self::Active,
            Self::LightWorkOnly => Self::NoExpansion,
            Self::HandoffOnly => Self::LightWorkOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    Expansion,
    HighMemoryStart,
    Edit,
    SmallRead,
    Handoff,
    #[default]
    Ordinary,
}

impl ToolClass {
    pub fn allowed_in(self, state: LogicalState) -> bool {
        match state {
            LogicalState::Active => true,
            LogicalState::NoExpansion => self != Self::Expansion,
            LogicalState::LightWorkOnly => !matches!(self, Self::Expansion | Self::HighMemoryStart),
            LogicalState::HandoffOnly => matches!(self, Self::Handoff | Self::SmallRead),
        }
    }
}

fn normalized_tool_name(payload: &Value) -> String {
    let normalized = payload
        .get("tool_name")
        .or_else(|| payload.get("toolCall").and_then(|value| value.get("name")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase()
        .replace(['-', ' '], "_");
    normalized
        .rsplit(['.', ':', '/'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn tool_input(payload: &Value) -> &Value {
    payload
        .get("tool_input")
        .or_else(|| {
            payload
                .get("toolCall")
                .and_then(|value| value.get("arguments"))
        })
        .or_else(|| payload.get("toolCall").and_then(|value| value.get("input")))
        .unwrap_or(&Value::Null)
}

fn input_text<'a>(input: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .unwrap_or_default()
}

/// Restriction strength of the classes `shell_class` can yield, so a compound
/// command adopts its strongest segment.  Handoff is the most permissive shell
/// outcome, a high-memory start the most restricted.
fn shell_class_rank(class: ToolClass) -> u8 {
    match class {
        ToolClass::HighMemoryStart => 2,
        ToolClass::Ordinary => 1,
        _ => 0,
    }
}

/// Classify one shell command segment that contains no operator chaining.
/// Read-only container/cluster queries are checked before the high-memory
/// markers because their launcher names (`docker `, `kubectl `) are marker
/// substrings; the high-memory markers are checked before the handoff prefixes
/// so a status-prefixed segment that embeds a heavy command cannot hide.
fn shell_segment_class(segment: &str) -> ToolClass {
    let segment = segment.trim();
    let query_prefixes = [
        "docker ps",
        "docker images",
        "docker logs",
        "docker inspect",
        "docker version",
        "docker info",
        "docker stats",
        "podman ps",
        "podman images",
        "podman logs",
        "podman inspect",
        "podman version",
        "podman info",
        "kubectl get",
        "kubectl describe",
        "kubectl logs",
        "kubectl version",
        "kubectl config",
    ];
    if query_prefixes
        .iter()
        .any(|prefix| segment.starts_with(prefix))
    {
        return ToolClass::Ordinary;
    }
    let high_memory_markers = [
        "cargo build",
        "cargo test",
        "cargo bench",
        "cargo run",
        "npm install",
        "npm ci",
        "npm test",
        "npm run build",
        "pnpm install",
        "pnpm test",
        "pnpm build",
        "yarn install",
        "yarn test",
        "yarn build",
        "pip install",
        "python -m pytest",
        "pytest ",
        "docker ",
        "podman ",
        "kubectl ",
        "terraform ",
        "cmake --build",
        "make -j",
        "ninja ",
        "playwright",
        "chromium",
        "google-chrome",
        "claude ",
        "codex ",
        "nohup ",
        "start-process",
    ];
    if high_memory_markers
        .iter()
        .any(|marker| segment.contains(marker))
    {
        return ToolClass::HighMemoryStart;
    }
    let handoff_prefixes = [
        "memory-status",
        "memory-supervisor",
        "ps ",
        "ps\n",
        "jobs",
        "pwd",
        "git status",
        "git diff --stat",
        "git log -",
    ];
    if handoff_prefixes
        .iter()
        .any(|prefix| segment == prefix.trim() || segment.starts_with(prefix))
    {
        return ToolClass::Handoff;
    }
    ToolClass::Ordinary
}

/// Classify a shell tool call by its strongest segment so appending or
/// embedding a heavier command can only raise the class, never hide behind a
/// lighter prefix: `git status && cargo build` is a high-memory start, not a
/// handoff.  The command is split on every sequence, pipe, background,
/// subshell, redirection, and substitution boundary before per-segment
/// classification.
///
/// ponytail: substring markers over-approximate — a quoted "cargo build"
/// string reads as a build and fails closed (over-restrict), the safe
/// direction for a memory guard; a per-shell tokenizer is the upgrade path if
/// that false-positive rate ever bites.
fn shell_class(input: &Value) -> ToolClass {
    let command = input_text(input, &["command", "cmd", "script"])
        .trim()
        .to_lowercase();
    // A backgrounded command escapes the foreground control loop, so treat it
    // as a high-memory start.  Match a trailing background `&` but neither `&&`
    // chaining nor `2>&1`-style redirection.
    let trimmed = command.trim_end();
    let background = ["run_in_background", "background", "detach"]
        .into_iter()
        .any(|key| input.get(key).and_then(Value::as_bool) == Some(true))
        || (trimmed.ends_with('&') && !trimmed.ends_with("&&"));
    if background {
        return ToolClass::HighMemoryStart;
    }
    command
        .split(|c: char| {
            matches!(
                c,
                ';' | '\n' | '|' | '&' | '(' | ')' | '`' | '<' | '>' | '$'
            )
        })
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(shell_segment_class)
        .max_by_key(|class| shell_class_rank(*class))
        .unwrap_or(ToolClass::Ordinary)
}

pub fn classify_tool(payload: &Value) -> ToolClass {
    let name = normalized_tool_name(payload);
    if matches!(
        name.as_str(),
        "agent" | "workflow" | "task" | "spawn_agent" | "create_agent"
    ) || name.ends_with("spawn_agent")
    {
        return ToolClass::Expansion;
    }
    if [
        "send_message",
        "wait_agent",
        "list_agents",
        "interrupt_agent",
        "taskoutput",
        "task_output",
        "structuredoutput",
        "structured_output",
        "taskstop",
        "task_stop",
        "stop",
        "cancel",
        "status",
        "get_goal",
        "update_goal",
        "request_user_input",
        "yield_control",
    ]
    .contains(&name.as_str())
    {
        return ToolClass::Handoff;
    }
    if matches!(
        name.as_str(),
        "edit" | "write" | "apply_patch" | "notebookedit" | "notebook_edit"
    ) {
        return ToolClass::Edit;
    }
    if matches!(
        name.as_str(),
        "bash" | "shell" | "exec" | "exec_command" | "run_command"
    ) {
        return shell_class(tool_input(payload));
    }
    if matches!(name.as_str(), "read" | "read_file" | "open_file") {
        return ToolClass::SmallRead;
    }
    if matches!(name.as_str(), "grep" | "glob" | "search" | "find") {
        let input = tool_input(payload);
        let path = input_text(input, &["path", "directory", "root"]);
        let broad = matches!(path, "/" | "~" | ".")
            || input
                .get("max_results")
                .and_then(Value::as_u64)
                .is_some_and(|limit| limit > 5000);
        return if broad {
            ToolClass::HighMemoryStart
        } else {
            ToolClass::SmallRead
        };
    }
    if name.contains("browser")
        || name.contains("playwright")
        || name.contains("container")
        || name.contains("imagegen")
    {
        return ToolClass::HighMemoryStart;
    }
    ToolClass::Ordinary
}

pub fn session_id(payload: &Value) -> String {
    let raw = payload
        .get("session_id")
        .or_else(|| payload.get("conversationId"))
        .and_then(Value::as_str)
        .unwrap_or("nosid");
    sanitize(raw, "nosid")
}

pub fn agent_id(payload: &Value) -> Option<String> {
    payload
        .get("agent_id")
        .and_then(Value::as_str)
        .map(|value| sanitize(value, ""))
        .filter(|value| !value.is_empty())
}

fn sanitize(raw: &str, fallback: &str) -> String {
    let value: String = raw
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(*character, '-' | '_'))
        .take(128)
        .collect();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

pub fn logical_key(provider: &str, session: &str, agent: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        sanitize(provider, "unknown"),
        sanitize(session, "nosid"),
        agent
            .map(|value| sanitize(value, "root"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "root".to_owned())
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookObservation {
    pub id: String,
    pub observed_at: f64,
    pub provider: String,
    pub event: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub surface: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_server_pid: Option<u32>,
    /// Thread marker inherited by the hook command. This corroborates the hook payload but is
    /// never sufficient by itself to authorize physical control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_marker: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_server_baseline_pids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_class: Option<ToolClass>,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// Exact logical-control epoch read by the hook process that produced this observation.
    /// A daemon-side requested state is not treated as enforced until a later hook carries it
    /// back as a receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_control_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_logical_state: Option<LogicalState>,
}

impl HookObservation {
    pub fn from_payload(
        id: String,
        observed_at: f64,
        provider: &str,
        event: &str,
        payload: &Value,
        process_pid: Option<u32>,
        blocked: bool,
    ) -> Self {
        let is_tool = matches!(event, "PreToolUse" | "PostToolUse" | "AfterTool");
        let name = normalized_tool_name(payload);
        let bounded_string = |value: Option<&Value>, limit: usize| {
            value
                .and_then(Value::as_str)
                .map(|value| value.chars().take(limit).collect::<String>())
                .filter(|value| !value.is_empty())
        };
        let tool_input = payload.get("tool_input").and_then(Value::as_object);
        let command_hint = tool_input
            .and_then(|input| input.get("command").or_else(|| input.get("cmd")))
            .and_then(|command| {
                command.as_str().map(str::to_owned).or_else(|| {
                    command.as_array().map(|parts| {
                        parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
            })
            .map(|value| value.chars().take(2048).collect::<String>())
            .filter(|value| !value.is_empty());
        Self {
            id,
            observed_at,
            provider: sanitize(provider, "unknown"),
            event: event.to_owned(),
            session_id: session_id(payload),
            agent_id: agent_id(payload),
            agent_type: payload
                .get("agent_type")
                .and_then(Value::as_str)
                .map(|value| sanitize(value, "unknown")),
            process_pid,
            surface: String::new(),
            app_server_pid: None,
            thread_marker: None,
            app_server_baseline_pids: Vec::new(),
            turn_id: bounded_string(payload.get("turn_id"), 256),
            tool_use_id: bounded_string(payload.get("tool_use_id"), 256),
            cwd: bounded_string(
                payload.get("cwd").or_else(|| {
                    tool_input.and_then(|input| input.get("cwd").or_else(|| input.get("workdir")))
                }),
                2048,
            ),
            command_hint,
            tool_name: (is_tool && !name.is_empty()).then_some(name),
            tool_class: is_tool.then(|| classify_tool(payload)),
            blocked,
            block_reason: None,
            observed_control_epoch: None,
            observed_logical_state: None,
        }
    }

    pub fn key(&self) -> String {
        logical_key(&self.provider, &self.session_id, self.agent_id.as_deref())
    }

    pub fn mark_codex_app(&mut self, app_server_pid: u32) {
        self.surface = "app-server".to_owned();
        self.app_server_pid = Some(app_server_pid);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LogicalAgent {
    pub key: String,
    pub provider: String,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub agent_type: String,
    pub role: String,
    pub process_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub surface: String,
    pub state: LogicalState,
    pub epoch: u64,
    pub reason: String,
    pub evidence_stage: String,
    pub state_since: f64,
    pub started_at: f64,
    pub last_seen_at: f64,
    pub last_progress_at: f64,
    pub idle_since: Option<f64>,
    pub last_heavy_at: Option<f64>,
    pub completed_turns: u64,
    pub next_check: f64,
    pub last_tool_class: Option<ToolClass>,
    pub in_flight_tool_class: Option<ToolClass>,
    pub last_blocked_at: Option<f64>,
    pub last_blocked_tool: Option<String>,
    pub last_blocked_reason: Option<String>,
    pub last_blocked_epoch: Option<u64>,
    /// Receipt from an App hook that actually loaded this agent's persisted control state.
    /// These fields are deliberately separate from `last_seen_at`: the daemon can request a
    /// tighter state without proving that Codex has loaded or enforced it yet.
    pub last_hook_receipt_at: Option<f64>,
    pub last_hook_receipt_epoch: Option<u64>,
    pub last_hook_receipt_state: Option<LogicalState>,
    pub active: bool,
}

impl Default for LogicalAgent {
    fn default() -> Self {
        Self {
            key: String::new(),
            provider: String::new(),
            session_id: String::new(),
            agent_id: None,
            agent_type: String::new(),
            role: "lead".to_owned(),
            process_pid: None,
            surface: String::new(),
            state: LogicalState::Active,
            epoch: 0,
            reason: String::new(),
            evidence_stage: String::new(),
            state_since: 0.0,
            started_at: 0.0,
            last_seen_at: 0.0,
            last_progress_at: 0.0,
            idle_since: None,
            last_heavy_at: None,
            completed_turns: 0,
            next_check: 0.0,
            last_tool_class: None,
            in_flight_tool_class: None,
            last_blocked_at: None,
            last_blocked_tool: None,
            last_blocked_reason: None,
            last_blocked_epoch: None,
            last_hook_receipt_at: None,
            last_hook_receipt_epoch: None,
            last_hook_receipt_state: None,
            active: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunawayConfirmation {
    pub first_complete_at: f64,
    pub last_complete_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunawayInputs {
    pub identity_reliable: bool,
    pub owned_mb: f64,
    pub warning_mb: f64,
    pub delta_mb: f64,
    pub sample_count: usize,
    pub observation_span_s: f64,
    pub observation_s: f64,
    pub monotonicity: f64,
    pub long_slope_mb_s: f64,
    pub recent_slope_mb_s: f64,
    pub usable_headroom_mb: f64,
    pub automatic_reserve_mb: f64,
    pub reaction_s: f64,
    pub native_confidence: String,
    pub attribution: String,
    /// True only when current lifecycle evidence proves that the measured growth continues
    /// outside useful work (for example, a completed turn or stopped child remains growing).
    pub work_mismatch: bool,
    pub headroom_fall_mb_s: f64,
    pub total_positive_growth_mb_s: f64,
    pub same_role_peer_slopes_mb_s: Vec<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunawayGates {
    pub identity: bool,
    pub materiality: bool,
    pub persistence: bool,
    pub machine_corroboration: bool,
    pub work_mismatch: bool,
    pub causal_dominance: bool,
}

impl RunawayGates {
    pub fn complete(&self) -> bool {
        self.identity
            && self.materiality
            && self.persistence
            && self.machine_corroboration
            && self.work_mismatch
            && self.causal_dominance
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunawayVerdict {
    pub stage: String,
    pub gates: RunawayGates,
    pub required_growth_mb: f64,
    pub candidate_tte_s: Option<f64>,
    pub growth_contribution: f64,
    pub headroom_share: f64,
    pub peer_outlier: bool,
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

pub fn evaluate_runaway(input: &RunawayInputs) -> RunawayVerdict {
    let long_slope = input.long_slope_mb_s.max(0.0);
    let candidate_tte =
        (long_slope >= 1.0).then(|| input.usable_headroom_mb.max(0.0) / long_slope.max(1.0));
    let intervention_horizon = (input.observation_s * 4.0).max(input.reaction_s * 12.0);
    let required_growth = 256.0_f64.max(input.automatic_reserve_mb * 0.25);
    let ordinary_material = input.owned_mb >= input.warning_mb && input.delta_mb >= required_growth;
    let urgent_small_exception = input.owned_mb >= input.warning_mb.clamp(128.0, 512.0)
        && input.delta_mb >= (required_growth * 0.5).max(128.0)
        && candidate_tte.is_some_and(|tte| tte <= intervention_horizon * 0.5);
    let materiality = ordinary_material || urgent_small_exception;
    let minimum_span = (input.observation_s * 0.8).max(5.0);
    let persistence = input.sample_count >= 5
        && input.observation_span_s >= minimum_span
        && input.monotonicity >= 0.8
        && input.long_slope_mb_s >= 1.0
        && input.recent_slope_mb_s >= 1.0;
    let machine_corroboration = input.native_confidence != "low"
        && matches!(input.attribution.as_str(), "agent" | "mixed")
        && input.headroom_fall_mb_s >= 1.0;
    let growth_contribution = if input.total_positive_growth_mb_s >= 1.0 {
        long_slope / input.total_positive_growth_mb_s
    } else {
        0.0
    };
    let headroom_share = if input.headroom_fall_mb_s >= 1.0 {
        long_slope / input.headroom_fall_mb_s
    } else {
        0.0
    };
    let mut peers: Vec<_> = input
        .same_role_peer_slopes_mb_s
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    let peer_outlier = if peers.len() >= 3 {
        let peer_median = median(&mut peers);
        let mut deviations: Vec<_> = peers
            .iter()
            .map(|value| (value - peer_median).abs())
            .collect();
        let mad = median(&mut deviations);
        long_slope > peer_median + (3.0 * mad).max(4.0)
    } else {
        false
    };
    let causal_dominance = headroom_share >= 0.25
        && (growth_contribution >= 0.5 || (peers.len() >= 3 && peer_outlier));
    let gates = RunawayGates {
        identity: input.identity_reliable,
        materiality,
        persistence,
        machine_corroboration,
        work_mismatch: input.work_mismatch,
        causal_dominance,
    };
    let observed =
        input.delta_mb > 0.0 && (input.long_slope_mb_s > 0.0 || input.recent_slope_mb_s > 0.0);
    let suspected = gates.identity && gates.materiality && gates.persistence && gates.work_mismatch;
    RunawayVerdict {
        stage: if suspected {
            "RUNAWAY_SUSPECT"
        } else if observed {
            "GROWTH_OBSERVED"
        } else {
            "STABLE"
        }
        .to_owned(),
        gates,
        required_growth_mb: required_growth,
        candidate_tte_s: candidate_tte,
        growth_contribution,
        headroom_share,
        peer_outlier,
    }
}

pub fn newest_first(left: &LogicalAgent, right: &LogicalAgent) -> std::cmp::Ordering {
    left.started_at
        .total_cmp(&right.started_at)
        .then_with(|| left.key.cmp(&right.key))
}

pub fn roster_by_state(agents: &BTreeMap<String, LogicalAgent>) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for agent in agents.values().filter(|agent| agent.active) {
        *result
            .entry(format!("{:?}", agent.state).to_uppercase())
            .or_default() += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strict_input() -> RunawayInputs {
        RunawayInputs {
            identity_reliable: true,
            owned_mb: 4096.0,
            warning_mb: 2048.0,
            delta_mb: 1024.0,
            sample_count: 31,
            observation_span_s: 30.0,
            observation_s: 30.0,
            monotonicity: 0.97,
            long_slope_mb_s: 34.0,
            recent_slope_mb_s: 38.0,
            usable_headroom_mb: 1600.0,
            automatic_reserve_mb: 512.0,
            reaction_s: 5.0,
            native_confidence: "high".to_owned(),
            attribution: "agent".to_owned(),
            work_mismatch: true,
            headroom_fall_mb_s: 40.0,
            total_positive_growth_mb_s: 50.0,
            same_role_peer_slopes_mb_s: vec![3.0, 4.0],
        }
    }

    #[test]
    fn strict_certificate_rejects_each_common_false_positive() {
        let baseline = strict_input();
        assert!(evaluate_runaway(&baseline).gates.complete());
        for mutate in [
            |value: &mut RunawayInputs| value.identity_reliable = false,
            |value: &mut RunawayInputs| value.delta_mb = 20.0,
            |value: &mut RunawayInputs| value.observation_span_s = 4.0,
            |value: &mut RunawayInputs| value.recent_slope_mb_s = -2.0,
            |value: &mut RunawayInputs| value.attribution = "external".to_owned(),
            |value: &mut RunawayInputs| value.native_confidence = "low".to_owned(),
            |value: &mut RunawayInputs| value.work_mismatch = false,
            |value: &mut RunawayInputs| value.total_positive_growth_mb_s = 200.0,
        ] {
            let mut input = baseline.clone();
            mutate(&mut input);
            assert!(!evaluate_runaway(&input).gates.complete());
        }
    }

    #[test]
    fn a_large_stable_process_and_a_short_spike_are_not_runaway() {
        let mut stable = strict_input();
        stable.owned_mb = 64_000.0;
        stable.delta_mb = 0.0;
        stable.long_slope_mb_s = 0.0;
        stable.recent_slope_mb_s = 0.0;
        assert_eq!(evaluate_runaway(&stable).stage, "STABLE");

        let mut spike = strict_input();
        spike.sample_count = 3;
        spike.observation_span_s = 2.0;
        assert!(!evaluate_runaway(&spike).gates.persistence);
    }

    #[test]
    fn hard_cap_or_external_pressure_does_not_manufacture_a_leak() {
        let mut external = strict_input();
        external.attribution = "external".to_owned();
        assert!(!evaluate_runaway(&external).gates.machine_corroboration);
        // Hard-cap authority is intentionally absent from RunawayInputs. It can authorize
        // containment elsewhere, but cannot make this certificate complete.
    }

    #[test]
    fn tool_classifier_preserves_results_and_blocks_only_the_named_ladder_classes() {
        let spawn = json!({"tool_name":"collaboration.spawn_agent","tool_input":{}});
        let edit = json!({"tool_name":"apply_patch","tool_input":{}});
        let build =
            json!({"tool_name":"functions.exec_command","tool_input":{"cmd":"cargo test --all"}});
        let result = json!({"tool_name":"send_message","tool_input":{}});
        let structured = json!({"tool_name":"StructuredOutput","tool_input":{}});
        assert_eq!(classify_tool(&spawn), ToolClass::Expansion);
        assert_eq!(classify_tool(&edit), ToolClass::Edit);
        assert_eq!(classify_tool(&build), ToolClass::HighMemoryStart);
        let query = json!({"tool_name":"bash","tool_input":{"command":"docker ps -a"}});
        assert_eq!(classify_tool(&query), ToolClass::Ordinary);
        let kubectl_query =
            json!({"tool_name":"bash","tool_input":{"command":"kubectl get pods -A"}});
        assert_eq!(classify_tool(&kubectl_query), ToolClass::Ordinary);
        let heavy_container =
            json!({"tool_name":"bash","tool_input":{"command":"docker build -t app ."}});
        assert_eq!(classify_tool(&heavy_container), ToolClass::HighMemoryStart);
        assert_eq!(classify_tool(&result), ToolClass::Handoff);
        assert_eq!(classify_tool(&structured), ToolClass::Handoff);
        assert!(ToolClass::Edit.allowed_in(LogicalState::LightWorkOnly));
        assert!(!ToolClass::Edit.allowed_in(LogicalState::HandoffOnly));
        assert!(ToolClass::Handoff.allowed_in(LogicalState::HandoffOnly));
        assert!(classify_tool(&structured).allowed_in(LogicalState::HandoffOnly));
    }

    #[test]
    fn chained_shell_commands_cannot_hide_a_heavy_segment_behind_a_light_prefix() {
        let bash = |cmd: &str| json!({"tool_name": "bash", "tool_input": {"command": cmd}});
        // Regression (P0-1): a safe prefix must not downgrade a chained, piped,
        // backgrounded, or substituted heavy command.
        for command in [
            "git status && cargo build --release",
            "docker ps && docker build -t app .",
            "pwd && npm test",
            "memory-status || make -j8",
            "echo ok | xargs cargo test",
            "git status; pytest tests/",
            "true && echo $(cargo build)",
            "nohup python worker.py &",
            "python worker.py &",
        ] {
            assert_eq!(
                classify_tool(&bash(command)),
                ToolClass::HighMemoryStart,
                "compound heavy command misclassified: {command}"
            );
        }
        // Lone light commands keep their class; redirection is not backgrounding.
        assert_eq!(classify_tool(&bash("git status")), ToolClass::Handoff);
        assert_eq!(classify_tool(&bash("docker ps -a")), ToolClass::Ordinary);
        assert_eq!(classify_tool(&bash("ls -la && pwd")), ToolClass::Ordinary);
        assert_eq!(classify_tool(&bash("cat log 2>&1")), ToolClass::Ordinary);
        // Strongest-segment invariant: appending a heavy command never lowers the class.
        for base in ["git status", "pwd", "ls", "docker ps", "echo hi"] {
            let plain = shell_class_rank(classify_tool(&bash(base)));
            let chained = shell_class_rank(classify_tool(&bash(&format!("{base} && cargo build"))));
            assert!(
                chained >= plain && chained == shell_class_rank(ToolClass::HighMemoryStart),
                "strongest-segment invariant broken for base: {base}"
            );
        }
    }

    #[test]
    fn hook_observation_keeps_old_json_compatible_and_extracts_app_tool_identity() {
        let old: HookObservation = serde_json::from_value(json!({
            "id":"old",
            "observed_at":1.0,
            "provider":"codex",
            "event":"SessionStart",
            "session_id":"session",
            "blocked":false
        }))
        .unwrap();
        assert!(old.surface.is_empty());
        assert!(old.tool_use_id.is_none());

        let mut current = HookObservation::from_payload(
            "new".to_owned(),
            2.0,
            "codex",
            "PreToolUse",
            &json!({
                "session_id":"thread-one",
                "turn_id":"turn-one",
                "tool_use_id":"call-one",
                "cwd":"/workspace",
                "tool_name":"exec_command",
                "tool_input":{"command":["cargo","test"]}
            }),
            Some(42),
            false,
        );
        current.mark_codex_app(42);
        assert_eq!(current.surface, "app-server");
        assert_eq!(current.turn_id.as_deref(), Some("turn-one"));
        assert_eq!(current.tool_use_id.as_deref(), Some("call-one"));
        assert_eq!(current.command_hint.as_deref(), Some("cargo test"));
        assert_eq!(current.cwd.as_deref(), Some("/workspace"));
    }
}
