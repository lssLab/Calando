use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::config::Config;
use crate::containment::RunawayVerdict;

const DEFAULT_PSI: [(&str, f64); 3] = [
    ("MEMORY_SUPERVISOR_PSI_GREEN", 5.0),
    ("MEMORY_SUPERVISOR_PSI_YELLOW", 15.0),
    ("MEMORY_SUPERVISOR_PSI_ORANGE", 40.0),
];

// ratio, absolute floor, absolute ceiling, small-machine maximum fraction.
const ADAPTIVE_SPECS: [(&str, [f64; 4]); 5] = [
    ("MEMORY_SUPERVISOR_MEM_GREEN", [0.30, 2048.0, 8192.0, 0.45]),
    ("MEMORY_SUPERVISOR_MEM_YELLOW", [0.18, 1024.0, 6144.0, 0.28]),
    ("MEMORY_SUPERVISOR_MEM_ORANGE", [0.10, 512.0, 4096.0, 0.15]),
    (
        "MEMORY_SUPERVISOR_LEAK_RSS_MB",
        [0.375, 2048.0, 12288.0, 0.65],
    ),
    (
        "MEMORY_SUPERVISOR_LEAK_STOP_MB",
        [0.625, 4096.0, 24576.0, 0.80],
    ),
];

#[derive(Clone, Copy)]
struct ProfileFactors {
    memory: f64,
    leak: f64,
    slope: f64,
    psi: f64,
}

fn profile_factors(profile: &str) -> ProfileFactors {
    match profile {
        "protect" => ProfileFactors {
            memory: 1.15,
            leak: 0.85,
            slope: 0.80,
            psi: 0.80,
        },
        "performance" => ProfileFactors {
            memory: 0.85,
            leak: 1.15,
            slope: 1.25,
            psi: 1.20,
        },
        _ => ProfileFactors {
            memory: 1.0,
            leak: 1.0,
            slope: 1.0,
            psi: 1.0,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedPolicy {
    pub mode: String,
    pub profile: String,
    pub capacity_mb: u64,
    pub overrides: Vec<String>,
    #[serde(flatten)]
    pub values: BTreeMap<String, f64>,
}

impl ResolvedPolicy {
    pub fn value(&self, name: &str) -> f64 {
        self.values[name]
    }
}

fn hybrid_threshold(capacity_mb: u64, spec: [f64; 4]) -> f64 {
    let capacity = capacity_mb as f64;
    (capacity * spec[0])
        .max(spec[1])
        .min(spec[2])
        .min((capacity * spec[3]).max(1.0))
}

fn configured(config: &Config, name: &str) -> bool {
    config.setting(name).is_some_and(|value| match value {
        serde_json::Value::String(value) => !value.is_empty(),
        _ => true,
    })
}

fn base_adaptive(capacity_mb: u64, key: &str, spec: [f64; 4], factors: ProfileFactors) -> f64 {
    let capacity = capacity_mb as f64;
    let factor = if key.starts_with("MEMORY_SUPERVISOR_MEM_") {
        factors.memory
    } else {
        factors.leak
    };
    let mut value = (hybrid_threshold(capacity_mb, spec) * factor).min(spec[2]);
    value = match key {
        "MEMORY_SUPERVISOR_LEAK_RSS_MB" => value.min(capacity * 0.70),
        "MEMORY_SUPERVISOR_LEAK_STOP_MB" => value.min(capacity * 0.85),
        _ => value.min(capacity * 0.95),
    };
    value
}

pub fn resolve_policy(config: &mut Config, capacity_mb: u64) -> ResolvedPolicy {
    let capacity_mb = capacity_mb.max(1);
    let profile = config.validated_choice(
        "MEMORY_SUPERVISOR_POLICY_PROFILE",
        "balanced",
        &["protect", "balanced", "performance"],
    );
    let factors = profile_factors(&profile);
    let mut values = BTreeMap::new();
    let mut overrides = BTreeSet::new();

    for (key, spec) in ADAPTIVE_SPECS {
        let adaptive = base_adaptive(capacity_mb, key, spec, factors);
        let value = if configured(config, key) {
            let value = config.validated_number(key, adaptive, Some(1.0), None);
            if !config.has_validation_error(key) {
                overrides.insert(key.to_owned());
            }
            value
        } else {
            config.clear_validation_error(key);
            adaptive
        };
        values.insert(key.to_owned(), value);
    }

    let mut slope = ((capacity_mb as f64 * 0.0061).clamp(32.0, 256.0) * factors.slope).min(256.0);
    let slope_key = "MEMORY_SUPERVISOR_LEAK_SLOPE_MBS";
    if configured(config, slope_key) {
        slope = config.validated_number(slope_key, slope, Some(0.1), None);
        if !config.has_validation_error(slope_key) {
            overrides.insert(slope_key.to_owned());
        }
    } else {
        config.clear_validation_error(slope_key);
    }
    values.insert(slope_key.to_owned(), slope);

    for (key, default) in DEFAULT_PSI {
        let adaptive = default * factors.psi;
        values.insert(
            key.to_owned(),
            config.validated_number(key, adaptive, Some(0.0), None),
        );
        if configured(config, key) && !config.has_validation_error(key) {
            overrides.insert(key.to_owned());
        }
    }

    let memory_ordered = values["MEMORY_SUPERVISOR_MEM_GREEN"]
        > values["MEMORY_SUPERVISOR_MEM_YELLOW"]
        && values["MEMORY_SUPERVISOR_MEM_YELLOW"] > values["MEMORY_SUPERVISOR_MEM_ORANGE"]
        && values["MEMORY_SUPERVISOR_MEM_ORANGE"] > 0.0;
    if !memory_ordered {
        config.record_validation_error(
            "memory_threshold_order",
            "memory_threshold_order: must satisfy GREEN > YELLOW > ORANGE > 0; using adaptive profile values",
        );
        for (key, spec) in ADAPTIVE_SPECS.into_iter().take(3) {
            values.insert(
                key.to_owned(),
                base_adaptive(capacity_mb, key, spec, factors),
            );
            overrides.remove(key);
        }
    } else {
        config.clear_validation_error("memory_threshold_order");
    }

    let leak_ordered = values["MEMORY_SUPERVISOR_LEAK_STOP_MB"]
        > values["MEMORY_SUPERVISOR_LEAK_RSS_MB"]
        && values["MEMORY_SUPERVISOR_LEAK_RSS_MB"] > 0.0;
    if !leak_ordered {
        config.record_validation_error(
            "leak_threshold_order",
            "leak_threshold_order: must satisfy STOP > WARN > 0; using adaptive profile values",
        );
        for (key, spec) in ADAPTIVE_SPECS.into_iter().skip(3) {
            values.insert(
                key.to_owned(),
                base_adaptive(capacity_mb, key, spec, factors),
            );
            overrides.remove(key);
        }
    } else {
        config.clear_validation_error("leak_threshold_order");
    }

    let psi_ordered = values["MEMORY_SUPERVISOR_PSI_GREEN"]
        < values["MEMORY_SUPERVISOR_PSI_YELLOW"]
        && values["MEMORY_SUPERVISOR_PSI_YELLOW"] < values["MEMORY_SUPERVISOR_PSI_ORANGE"];
    if !psi_ordered {
        config.record_validation_error(
            "psi_threshold_order",
            "psi_threshold_order: must satisfy GREEN < YELLOW < ORANGE; using profile defaults",
        );
        for (key, default) in DEFAULT_PSI {
            values.insert(key.to_owned(), default * factors.psi);
            overrides.remove(key);
        }
    } else {
        config.clear_validation_error("psi_threshold_order");
    }

    ResolvedPolicy {
        mode: "adaptive-evidence".to_owned(),
        profile,
        capacity_mb,
        overrides: overrides.into_iter().collect(),
        values,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "UPPERCASE")]
pub enum Level {
    Green,
    Yellow,
    Orange,
    Red,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Observe,
    Hold,
    Drain,
}

impl Action {
    pub fn level(self) -> Level {
        match self {
            Self::Allow => Level::Green,
            Self::Observe => Level::Yellow,
            Self::Hold => Level::Orange,
            Self::Drain => Level::Red,
        }
    }
}

pub fn level_from(mem_mb: u64, psi_some: f64, policy: &ResolvedPolicy) -> Level {
    let memory = if mem_mb as f64 > policy.value("MEMORY_SUPERVISOR_MEM_GREEN") {
        Level::Green
    } else if mem_mb as f64 > policy.value("MEMORY_SUPERVISOR_MEM_YELLOW") {
        Level::Yellow
    } else if mem_mb as f64 > policy.value("MEMORY_SUPERVISOR_MEM_ORANGE") {
        Level::Orange
    } else {
        Level::Red
    };
    let pressure = if psi_some < policy.value("MEMORY_SUPERVISOR_PSI_GREEN") {
        Level::Green
    } else if psi_some < policy.value("MEMORY_SUPERVISOR_PSI_YELLOW") {
        Level::Yellow
    } else if psi_some < policy.value("MEMORY_SUPERVISOR_PSI_ORANGE") {
        Level::Orange
    } else {
        Level::Red
    };
    memory.max(pressure)
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct MemorySnapshot {
    pub available_mb: u64,
    pub capacity_mb: u64,
    pub capacity_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NativePressure {
    pub some_avg10: f64,
    pub full_avg10: f64,
    pub state: String,
    pub commit_remaining_mb: Option<u64>,
    pub reclaim_total: f64,
    pub swap_total: f64,
    pub oom_total: u64,
    pub confidence: String,
}

impl Default for NativePressure {
    fn default() -> Self {
        Self {
            some_avg10: 0.0,
            full_avg10: 0.0,
            state: "normal".to_owned(),
            commit_remaining_mb: None,
            reclaim_total: 0.0,
            swap_total: 0.0,
            oom_total: 0,
            confidence: "high".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HistorySample {
    pub ts: f64,
    pub available: Option<f64>,
    pub tracked: Option<f64>,
    pub worker: Option<f64>,
    pub reclaim: Option<f64>,
    pub swap: Option<f64>,
    pub oom: Option<f64>,
    pub commit_remaining: Option<f64>,
}

impl HistorySample {
    fn value(&self, key: HistoryKey) -> Option<f64> {
        match key {
            HistoryKey::Available => self.available,
            HistoryKey::Tracked => self.tracked,
            HistoryKey::Worker => self.worker,
            HistoryKey::Reclaim => self.reclaim,
            HistoryKey::Swap => self.swap,
            HistoryKey::Oom => self.oom,
            HistoryKey::CommitRemaining => self.commit_remaining,
        }
        .filter(|value| value.is_finite())
    }
}

#[derive(Clone, Copy)]
enum HistoryKey {
    Available,
    Tracked,
    Worker,
    Reclaim,
    Swap,
    Oom,
    CommitRemaining,
}

fn window_slope(history: &[HistorySample], key: HistoryKey, window_s: f64) -> f64 {
    let usable: Vec<_> = history
        .iter()
        .filter_map(|sample| sample.value(key).map(|value| (sample.ts, value)))
        .collect();
    let Some(&(latest_ts, latest_value)) = usable.last() else {
        return 0.0;
    };
    if usable.len() < 2 {
        return 0.0;
    }
    let cutoff = latest_ts - window_s;
    let (first_ts, first_value) = usable
        .iter()
        .copied()
        .find(|(timestamp, _)| *timestamp >= cutoff)
        .unwrap_or(usable[0]);
    let elapsed = latest_ts - first_ts;
    if elapsed > 0.0 {
        (latest_value - first_value) / elapsed
    } else {
        0.0
    }
}

fn sustained_directional_rate(
    recent: &[(f64, f64)],
    minimum_rate: f64,
    minimum_span_s: f64,
    increasing: bool,
) -> Option<f64> {
    let (first, last) = recent.first().zip(recent.last())?;
    let span = last.0 - first.0;
    if recent.len() < 3 || span < minimum_span_s || span <= 0.0 {
        return None;
    }
    let directed_distance = if increasing {
        last.1 - first.1
    } else {
        first.1 - last.1
    };
    let net_rate = directed_distance / span;
    if net_rate < minimum_rate {
        return None;
    }

    let mut supporting_intervals = 0_usize;
    let mut favorable_distance = 0.0_f64;
    let mut adverse_distance = 0.0_f64;
    for pair in recent.windows(2) {
        let elapsed = pair[1].0 - pair[0].0;
        if elapsed <= 0.0 {
            return None;
        }
        let distance = if increasing {
            pair[1].1 - pair[0].1
        } else {
            pair[0].1 - pair[1].1
        };
        if distance / elapsed >= minimum_rate {
            supporting_intervals += 1;
        }
        if distance >= 0.0 {
            favorable_distance += distance;
        } else {
            adverse_distance -= distance;
        }
    }
    let intervals = recent.len() - 1;
    // Kernel reclaim and swap can create a one-sample rebound while the durable
    // trajectory still falls. Require broad interval agreement and a 2:1 net
    // distance majority instead of demanding that every sample be monotonic.
    (supporting_intervals * 5 >= intervals * 3 && favorable_distance >= adverse_distance * 2.0)
        .then_some(net_rate)
}

fn sustained_exhaustion_risk(
    history: &[HistorySample],
    key: HistoryKey,
    remaining: f64,
    window_s: f64,
    deadline_s: f64,
    minimum_span_s: f64,
) -> bool {
    let usable: Vec<_> = history
        .iter()
        .filter_map(|sample| sample.value(key).map(|value| (sample.ts, value)))
        .collect();
    let Some(&(latest_ts, _)) = usable.last() else {
        return false;
    };
    if usable.len() < 3 {
        return false;
    }
    let cutoff = latest_ts - window_s;
    let recent: Vec<_> = usable
        .iter()
        .copied()
        .filter(|(timestamp, _)| *timestamp >= cutoff)
        .collect();
    sustained_directional_rate(&recent, 1.0, minimum_span_s, false)
        .is_some_and(|fall_rate| remaining / fall_rate <= deadline_s)
}

fn sustained_growth_rate(
    history: &[HistorySample],
    key: HistoryKey,
    minimum_rate: f64,
    window_s: f64,
) -> bool {
    let usable: Vec<_> = history
        .iter()
        .filter_map(|sample| sample.value(key).map(|value| (sample.ts, value)))
        .collect();
    let Some(&(latest_ts, _)) = usable.last() else {
        return false;
    };
    let cutoff = latest_ts - window_s;
    let recent: Vec<_> = usable
        .iter()
        .copied()
        .filter(|(timestamp, _)| *timestamp >= cutoff)
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    recent.len() == 3
        && recent.windows(2).all(|pair| {
            let elapsed = pair[1].0 - pair[0].0;
            elapsed > 0.0 && (pair[1].1 - pair[0].1) / elapsed >= minimum_rate
        })
}

fn sustained_directional_change(
    history: &[HistorySample],
    key: HistoryKey,
    minimum_rate: f64,
    window_s: f64,
    minimum_span_s: f64,
    increasing: bool,
) -> bool {
    let usable: Vec<_> = history
        .iter()
        .filter_map(|sample| sample.value(key).map(|value| (sample.ts, value)))
        .collect();
    let Some(&(latest_ts, _)) = usable.last() else {
        return false;
    };
    let cutoff = latest_ts - window_s;
    let recent: Vec<_> = usable
        .into_iter()
        .filter(|(timestamp, _)| *timestamp >= cutoff)
        .collect();
    sustained_directional_rate(&recent, minimum_rate, minimum_span_s, increasing).is_some()
}

fn rounded(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Assessment {
    pub mem_available_mb: u64,
    pub memory_capacity_mb: u64,
    pub distress: String,
    pub attribution: String,
    pub action: Action,
    pub admission_level: Level,
    pub collapse_imminent: bool,
    pub headroom_slope_short_mb_s: f64,
    pub headroom_slope_long_mb_s: f64,
    pub headroom_fall_mb_s: f64,
    pub physical_headroom_fall_mb_s: f64,
    pub commit_headroom_fall_mb_s: f64,
    pub tracked_growth_mb_s: f64,
    pub worker_growth_mb_s: f64,
    pub external_estimate_mb_s: f64,
    pub time_to_exhaustion_s: Option<f64>,
    pub time_to_recovery_reserve_s: Option<f64>,
    pub physical_time_to_exhaustion_s: Option<f64>,
    pub commit_time_to_exhaustion_s: Option<f64>,
    pub automatic_reserve_mb: f64,
    pub recoverability_floor_mb: f64,
    pub new_fanout_floor_mb: f64,
    pub expected_burst_mb: f64,
    pub reaction_s: f64,
    pub trajectory_confirmed: bool,
    pub native_confidence: String,
    pub native_state: String,
    pub commit_remaining_mb: Option<u64>,
    pub reclaim_rate_s: f64,
    pub swap_rate_s: f64,
    pub oom_rate_s: f64,
    pub adaptive_action: Option<Action>,
    pub cli_hard_cap_mb: Option<f64>,
    pub cli_hard_cap_driving: bool,
    pub cli_memory_used_mb: Option<f64>,
    pub cli_hard_cap_remaining_mb: Option<f64>,
    pub cli_hard_cap_margin_mb: Option<f64>,
    pub cli_hard_cap_status: String,
}

pub fn assess_pressure(
    platform: &str,
    snapshot: &MemorySnapshot,
    pressure: &NativePressure,
    history: &[HistorySample],
    tick_s: f64,
    leak_window_s: f64,
    runtime_degraded: bool,
) -> Assessment {
    let reaction_s = 3.0_f64.max(tick_s * 5.0);
    let short_s = 5.0_f64.max(reaction_s * 2.0);
    let long_s = 30.0_f64.max(leak_window_s * 2.0);
    let headroom_short = window_slope(history, HistoryKey::Available, short_s);
    let headroom_long = window_slope(history, HistoryKey::Available, long_s);
    let tracked_growth = window_slope(history, HistoryKey::Tracked, short_s);
    let worker_growth = window_slope(history, HistoryKey::Worker, short_s);
    let reclaim_rate = window_slope(history, HistoryKey::Reclaim, short_s).max(0.0);
    let swap_rate = window_slope(history, HistoryKey::Swap, short_s).max(0.0);
    let oom_rate = window_slope(history, HistoryKey::Oom, short_s).max(0.0);
    let physical_fall_rate = (-headroom_short).max(0.0);
    let commit_fall_rate = (-window_slope(history, HistoryKey::CommitRemaining, short_s)).max(0.0);
    let physical_tte =
        (physical_fall_rate >= 1.0).then(|| snapshot.available_mb as f64 / physical_fall_rate);
    let commit_tte = pressure.commit_remaining_mb.and_then(|remaining| {
        (commit_fall_rate >= 1.0).then(|| remaining as f64 / commit_fall_rate)
    });
    let tte = match (physical_tte, commit_tte) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let fall_rate = physical_fall_rate.max(commit_fall_rate);

    let mut distress = if pressure.confidence == "low" || pressure.state == "unknown" {
        "unknown"
    } else {
        "normal"
    };
    let sustained_swap = sustained_growth_rate(history, HistoryKey::Swap, 1.0, short_s);
    match platform {
        "linux" | "wsl" => {
            let reclaim_threshold = (snapshot.capacity_mb as f64 * 0.005).max(32.0);
            let sustained_reclaim =
                sustained_growth_rate(history, HistoryKey::Reclaim, reclaim_threshold, short_s);
            if oom_rate > 0.0 || pressure.full_avg10 >= 10.0 {
                distress = "critical";
            } else if pressure.full_avg10 >= 1.0 || sustained_swap || sustained_reclaim {
                distress = "elevated";
            }
        }
        "darwin" => {
            let critical_swap = (snapshot.capacity_mb as f64 * 0.005).max(32.0);
            let sustained_critical_swap =
                sustained_growth_rate(history, HistoryKey::Swap, critical_swap, short_s);
            let sustained_reclaim =
                sustained_growth_rate(history, HistoryKey::Reclaim, 4.0, short_s);
            if pressure.state == "critical" || sustained_critical_swap {
                distress = "critical";
            } else if pressure.state == "warning" || sustained_swap || sustained_reclaim {
                distress = "elevated";
            }
        }
        "windows" => {
            if let Some(remaining) = pressure.commit_remaining_mb {
                if remaining as f64 <= (snapshot.capacity_mb as f64 * 0.01).max(256.0) {
                    distress = "critical";
                } else if remaining as f64 <= (snapshot.capacity_mb as f64 * 0.05).max(512.0) {
                    distress = "elevated";
                }
            }
        }
        _ => {}
    }

    let physical_trajectory = sustained_directional_change(
        history,
        HistoryKey::Available,
        1.0,
        short_s,
        reaction_s,
        false,
    );
    let commit_trajectory = pressure.commit_remaining_mb.is_some()
        && sustained_directional_change(
            history,
            HistoryKey::CommitRemaining,
            1.0,
            short_s,
            reaction_s,
            false,
        );
    let trajectory_confirmed = physical_trajectory || commit_trajectory;
    let tracked_trajectory =
        sustained_directional_change(history, HistoryKey::Tracked, 1.0, short_s, reaction_s, true)
            || sustained_directional_change(
                history,
                HistoryKey::Worker,
                1.0,
                short_s,
                reaction_s,
                true,
            );
    let confirmed_physical_fall = if physical_trajectory {
        physical_fall_rate
    } else {
        0.0
    };
    let confirmed_commit_fall = if commit_trajectory {
        commit_fall_rate
    } else {
        0.0
    };
    let confirmed_fall_rate = confirmed_physical_fall.max(confirmed_commit_fall);
    let recoverability_floor = (snapshot.capacity_mb as f64 * 0.005).clamp(256.0, 1024.0);
    let expected_burst = if trajectory_confirmed && tracked_trajectory {
        tracked_growth.max(worker_growth).max(0.0) * reaction_s
    } else {
        0.0
    };
    // The machine-headroom fall already contains growth from tracked CLI trees.
    // Adding both rates would reserve the same allocation twice and throttle useful
    // work well before the recoverability boundary.  Keep the larger corroborated
    // rate: it covers one complete reaction interval without double counting.
    let confirmed_tracked_rate = if trajectory_confirmed && tracked_trajectory {
        tracked_growth.max(worker_growth).max(0.0)
    } else {
        0.0
    };
    let reserve_rate = confirmed_fall_rate.max(confirmed_tracked_rate);
    let reserve =
        (recoverability_floor + reserve_rate * reaction_s).min(snapshot.capacity_mb as f64 * 0.25);
    // Existing useful work may run down to the recoverability reserve. A new
    // agent needs one additional minimum work/reaction block, so admission has
    // a separate floor and cannot reopen merely because a dangerously small
    // amount of memory became stable.
    let new_fanout_floor = (reserve + recoverability_floor).min(snapshot.capacity_mb as f64 * 0.30);
    let agent_rate = tracked_growth.max(0.0);
    let external_rate = (fall_rate - agent_rate).max(0.0);
    let attribution = if fall_rate < 1.0 {
        // Growth in a tracked tree is not proof that it consumed system headroom.
        // Attribute only when the machine-level headroom signal corroborates it.
        "unknown"
    } else if agent_rate >= (fall_rate * 0.4).max(4.0)
        && external_rate >= (fall_rate * 0.2).max(2.0)
    {
        "mixed"
    } else if agent_rate >= (fall_rate * 0.5).max(4.0) {
        "agent"
    } else if external_rate >= (fall_rate * 0.5).max(2.0) {
        "external"
    } else {
        "unknown"
    };
    let physical_reserve_distance = (snapshot.available_mb as f64 - reserve).max(0.0);
    let commit_reserve_distance = pressure
        .commit_remaining_mb
        .map(|remaining| (remaining as f64 - reserve).max(0.0));
    let physical_reserve_tte =
        (physical_fall_rate >= 1.0).then(|| physical_reserve_distance / physical_fall_rate);
    let commit_reserve_tte = commit_reserve_distance
        .and_then(|remaining| (commit_fall_rate >= 1.0).then(|| remaining / commit_fall_rate));
    let reserve_tte = match (physical_reserve_tte, commit_reserve_tte) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let rapid_projection = tte.is_some_and(|time| time <= reaction_s * 12.0);
    let sustained_to_reserve = |deadline_s: f64| {
        reserve_tte.is_some_and(|time| time <= deadline_s)
            && (sustained_exhaustion_risk(
                history,
                HistoryKey::Available,
                physical_reserve_distance,
                short_s,
                deadline_s,
                reaction_s,
            ) || commit_reserve_distance.is_some_and(|remaining| {
                sustained_exhaustion_risk(
                    history,
                    HistoryKey::CommitRemaining,
                    remaining,
                    short_s,
                    deadline_s,
                    reaction_s,
                )
            }))
    };
    let reserve_within_one_reaction = sustained_to_reserve(reaction_s);
    let reserve_within_two_reactions = sustained_to_reserve(reaction_s * 2.0);
    let below_reserve = snapshot.available_mb as f64 <= reserve
        || pressure
            .commit_remaining_mb
            .is_some_and(|remaining| remaining as f64 <= reserve);
    // Native pressure is corroborating evidence, not a substitute for stopping distance.
    // In particular, a short Linux/WSL PSI-full spike can coexist with abundant, stable
    // headroom. Treating that signal alone as an imminent collapse bypassed the gradual
    // braking policy and could drain a light, single-session App workload. A new OOM event is
    // already a boundary failure, while Windows commit headroom is itself a capacity signal.
    let native_boundary_failure = oom_rate > 0.0
        || (platform == "windows"
            && pressure
                .commit_remaining_mb
                .is_some_and(|remaining| remaining as f64 <= reserve));
    let collapse_imminent = native_boundary_failure || reserve_within_one_reaction;
    let near_reserve = snapshot.available_mb as f64 <= reserve * 1.25
        || pressure
            .commit_remaining_mb
            .is_some_and(|remaining| remaining as f64 <= reserve * 1.25);
    let elevated_reserve = snapshot.available_mb as f64 <= reserve * 2.0
        || pressure
            .commit_remaining_mb
            .is_some_and(|remaining| remaining as f64 <= reserve * 2.0);
    let elevated_near_exhaustion =
        distress == "elevated" && (elevated_reserve || reserve_within_two_reactions);
    let expansion_unsafe = (snapshot.available_mb as f64) < new_fanout_floor
        || pressure
            .commit_remaining_mb
            .is_some_and(|remaining| (remaining as f64) < new_fanout_floor);
    let action = if runtime_degraded {
        Action::Hold
    } else if collapse_imminent {
        Action::Drain
    } else if reserve_within_two_reactions
        || below_reserve
        || elevated_near_exhaustion
        || expansion_unsafe
    {
        Action::Hold
    } else if distress != "normal" || near_reserve || rapid_projection {
        Action::Observe
    } else {
        Action::Allow
    };

    Assessment {
        mem_available_mb: snapshot.available_mb,
        memory_capacity_mb: snapshot.capacity_mb,
        distress: distress.to_owned(),
        attribution: attribution.to_owned(),
        action,
        admission_level: action.level(),
        collapse_imminent,
        headroom_slope_short_mb_s: rounded(headroom_short, 2),
        headroom_slope_long_mb_s: rounded(headroom_long, 2),
        headroom_fall_mb_s: rounded(fall_rate, 2),
        physical_headroom_fall_mb_s: rounded(physical_fall_rate, 2),
        commit_headroom_fall_mb_s: rounded(commit_fall_rate, 2),
        tracked_growth_mb_s: rounded(tracked_growth, 2),
        worker_growth_mb_s: rounded(worker_growth, 2),
        external_estimate_mb_s: rounded(external_rate, 2),
        time_to_exhaustion_s: tte.map(|value| rounded(value, 1)),
        time_to_recovery_reserve_s: reserve_tte.map(|value| rounded(value, 1)),
        physical_time_to_exhaustion_s: physical_tte.map(|value| rounded(value, 1)),
        commit_time_to_exhaustion_s: commit_tte.map(|value| rounded(value, 1)),
        automatic_reserve_mb: rounded(reserve, 1),
        recoverability_floor_mb: rounded(recoverability_floor, 1),
        new_fanout_floor_mb: rounded(new_fanout_floor, 1),
        expected_burst_mb: rounded(expected_burst, 1),
        reaction_s,
        trajectory_confirmed,
        native_confidence: pressure.confidence.clone(),
        native_state: pressure.state.clone(),
        commit_remaining_mb: pressure.commit_remaining_mb,
        reclaim_rate_s: rounded(reclaim_rate, 2),
        swap_rate_s: rounded(swap_rate, 2),
        oom_rate_s: rounded(oom_rate, 3),
        adaptive_action: None,
        cli_hard_cap_mb: None,
        cli_hard_cap_driving: false,
        cli_memory_used_mb: None,
        cli_hard_cap_remaining_mb: None,
        cli_hard_cap_margin_mb: None,
        cli_hard_cap_status: String::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub rss_mb: u64,
    pub anon_mb: u64,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub start_token: String,
    #[serde(default)]
    pub terminal: String,
    #[serde(default)]
    pub terminal_identity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackedProcess {
    pub pid: u32,
    pub name: String,
    pub rss_mb: u64,
    pub anon_mb: u64,
    pub via: String,
    pub role: String,
    pub root_pid: u32,
    pub tree_rss_mb: u64,
    pub tree_anon_mb: u64,
    pub identity: String,
    pub identity_reliable: bool,
    pub start_token: String,
    pub terminal: String,
    pub terminal_identity: String,
    #[serde(default)]
    pub slope_mb_s: f64,
    #[serde(default)]
    pub monotonicity: f64,
    #[serde(default)]
    pub strong_leak: bool,
    #[serde(default)]
    pub recent_slope_mb_s: f64,
    #[serde(default)]
    pub growth_delta_mb: f64,
    #[serde(default)]
    pub observation_span_s: f64,
    #[serde(default)]
    pub runaway_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runaway: Option<RunawayVerdict>,
}

fn normalized_executable(value: &str) -> &str {
    let value = value.trim_matches('"');
    let base = value.rsplit(['/', '\\']).next().unwrap_or(value);
    base.strip_suffix(".exe").unwrap_or(base)
}

fn executable_provider(value: &str) -> Option<&'static str> {
    match normalized_executable(value) {
        "claude" => Some("claude"),
        "codex" | "codex-aarch64-apple-darwin" | "codex-x86_64-apple-darwin" => Some("codex"),
        _ => None,
    }
}

fn is_provider_executable(value: &str) -> bool {
    executable_provider(value).is_some()
}

pub fn target_provider(process: &ProcessInfo) -> Option<&'static str> {
    let name = process.name.to_lowercase();
    executable_provider(&name)
        .or_else(|| {
            process
                .args
                .first()
                .and_then(|argument| executable_provider(&argument.to_lowercase()))
        })
        .or_else(|| {
            process.args.iter().skip(1).take(3).find_map(|argument| {
                ((argument.contains('/') || argument.contains('\\'))
                    && is_provider_executable(&argument.to_lowercase()))
                .then(|| executable_provider(&argument.to_lowercase()))
                .flatten()
            })
        })
}

pub fn is_target(process: &ProcessInfo) -> bool {
    target_provider(process).is_some()
}

pub fn process_identity(process: &ProcessInfo) -> String {
    format!(
        "{}:{}",
        process.pid,
        if process.start_token.is_empty() {
            &process.name
        } else {
            &process.start_token
        }
    )
}

pub fn tracked_processes(processes: &BTreeMap<u32, ProcessInfo>) -> Vec<TrackedProcess> {
    let targets: BTreeSet<u32> = processes
        .iter()
        .filter_map(|(pid, process)| is_target(process).then_some(*pid))
        .collect();
    let mut tracked: BTreeMap<u32, (&str, u32, &str)> = BTreeMap::new();
    for pid in &targets {
        let mut target_ancestors = Vec::new();
        let mut current = processes[pid].ppid;
        while current > 1 && processes.contains_key(&current) && target_ancestors.len() < 64 {
            if targets.contains(&current) {
                target_ancestors.push(current);
            }
            current = processes[&current].ppid;
        }
        let root_pid = target_ancestors.last().copied().unwrap_or(*pid);
        tracked.insert(
            *pid,
            if target_ancestors.is_empty() {
                ("root", root_pid, "lead")
            } else {
                ("child", root_pid, "worker")
            },
        );
    }
    for pid in processes.keys().copied() {
        if tracked.contains_key(&pid) {
            continue;
        }
        let mut chain = Vec::new();
        let mut current = pid;
        while current > 1
            && processes.contains_key(&current)
            && !tracked.contains_key(&current)
            && chain.len() < 64
        {
            chain.push(current);
            current = processes[&current].ppid;
        }
        if let Some((_, root_pid, _)) = tracked.get(&current).copied() {
            for child in chain {
                tracked.insert(child, ("child", root_pid, "support"));
            }
        }
    }

    let mut tree_rss = BTreeMap::<u32, u64>::new();
    let mut tree_anon = BTreeMap::<u32, u64>::new();
    for (pid, (_, root_pid, _)) in &tracked {
        *tree_rss.entry(*root_pid).or_default() += processes[pid].rss_mb;
        *tree_anon.entry(*root_pid).or_default() += processes[pid].anon_mb;
    }
    tracked
        .into_iter()
        .filter_map(|(pid, (via, root_pid, role))| {
            let process = &processes[&pid];
            if via == "child" && process.anon_mb < 32 {
                return None;
            }
            Some(TrackedProcess {
                pid,
                name: process.name.clone(),
                rss_mb: process.rss_mb,
                anon_mb: process.anon_mb,
                via: via.to_owned(),
                role: role.to_owned(),
                root_pid,
                tree_rss_mb: tree_rss[&root_pid],
                tree_anon_mb: tree_anon[&root_pid],
                identity: process_identity(process),
                identity_reliable: !process.start_token.is_empty(),
                start_token: process.start_token.clone(),
                terminal: process.terminal.clone(),
                terminal_identity: process.terminal_identity.clone(),
                slope_mb_s: 0.0,
                monotonicity: 0.0,
                strong_leak: false,
                recent_slope_mb_s: 0.0,
                growth_delta_mb: 0.0,
                observation_span_s: 0.0,
                runaway_verified: false,
                runaway: None,
            })
        })
        .collect()
}

pub fn apply_cli_hard_cap(
    assessment: &mut Assessment,
    tracked: &[TrackedProcess],
    hard_cap_mb: Option<f64>,
    sensor_ok: bool,
) {
    let adaptive_action = assessment.action;
    assessment.adaptive_action = Some(adaptive_action);
    assessment.cli_hard_cap_mb = hard_cap_mb.map(|value| rounded(value, 1));
    assessment.cli_hard_cap_driving = false;
    assessment.cli_memory_used_mb = None;
    assessment.cli_hard_cap_remaining_mb = None;
    assessment.cli_hard_cap_margin_mb = None;
    let Some(hard_cap_mb) = hard_cap_mb else {
        assessment.cli_hard_cap_status = "disabled".to_owned();
        return;
    };
    if !sensor_ok {
        assessment.cli_hard_cap_status = "unavailable".to_owned();
        return;
    }
    let used_mb: f64 = tracked
        .iter()
        .filter(|process| process.via == "root")
        .map(|process| process.tree_rss_mb as f64)
        .sum();
    let margin_mb = (assessment.expected_burst_mb.max(32.0)).min(hard_cap_mb * 0.25);
    let (proposed, status) = if used_mb >= hard_cap_mb {
        (Action::Drain, "exceeded")
    } else if used_mb + margin_mb >= hard_cap_mb {
        (Action::Hold, "near")
    } else {
        (adaptive_action, "within")
    };
    if proposed > adaptive_action {
        assessment.action = proposed;
        assessment.admission_level = proposed.level();
        assessment.cli_hard_cap_driving = true;
    }
    assessment.cli_memory_used_mb = Some(rounded(used_mb, 1));
    assessment.cli_hard_cap_remaining_mb = Some(rounded(hard_cap_mb - used_mb, 1));
    assessment.cli_hard_cap_margin_mb = Some(rounded(margin_mb, 1));
    assessment.cli_hard_cap_status = status.to_owned();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn config_from_json(label: &str, source: &str) -> (Config, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "memory-supervisor-policy-{label}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, source).unwrap();
        (Config::load(&path), path)
    }

    fn sample(ts: f64, available: f64, tracked: f64) -> HistorySample {
        HistorySample {
            ts,
            available: Some(available),
            tracked: Some(tracked),
            worker: Some(0.0),
            reclaim: Some(0.0),
            swap: Some(0.0),
            oom: Some(0.0),
            commit_remaining: None,
        }
    }

    #[test]
    fn adaptive_thresholds_match_python_reference_points() {
        let small = resolve_policy(&mut Config::default(), 2048);
        let reference = resolve_policy(&mut Config::default(), 8192);
        let large = resolve_policy(&mut Config::default(), 102_400);
        assert_eq!(small.value("MEMORY_SUPERVISOR_MEM_GREEN").round(), 922.0);
        assert_eq!(
            reference.value("MEMORY_SUPERVISOR_MEM_GREEN").round(),
            2458.0
        );
        assert_eq!(large.value("MEMORY_SUPERVISOR_MEM_GREEN"), 8192.0);
        assert_eq!(large.value("MEMORY_SUPERVISOR_LEAK_STOP_MB"), 24_576.0);
    }

    #[test]
    fn stable_high_use_is_allowed_and_an_unconfirmed_rapid_projection_only_observes() {
        let stable = [
            sample(0.0, 1024.0, 99_000.0),
            sample(10.0, 1024.0, 99_000.0),
            sample(20.0, 1024.0, 99_000.0),
        ];
        let safe = assess_pressure(
            "linux",
            &MemorySnapshot {
                available_mb: 1024,
                capacity_mb: 102_400,
                capacity_source: "test".to_owned(),
            },
            &NativePressure::default(),
            &stable,
            1.0,
            30.0,
            false,
        );
        assert_eq!(safe.action, Action::Allow);
        assert_eq!(safe.new_fanout_floor_mb, 1024.0);
        assert!(safe.time_to_exhaustion_s.is_none());

        let falling = [
            sample(0.0, 40_000.0, 1000.0),
            sample(10.0, 20_000.0, 1000.0),
        ];
        let danger = assess_pressure(
            "linux",
            &MemorySnapshot {
                available_mb: 20_000,
                capacity_mb: 131_072,
                capacity_source: "test".to_owned(),
            },
            &NativePressure::default(),
            &falling,
            1.0,
            30.0,
            false,
        );
        assert_eq!(danger.action, Action::Observe);
        assert!(!danger.trajectory_confirmed);
        assert!(
            danger
                .time_to_exhaustion_s
                .is_some_and(|value| value <= 10.0)
        );
    }

    #[test]
    fn kernel_noise_does_not_erase_a_durable_collapse_but_stable_low_memory_only_holds_fanout() {
        let noisy_decline = [
            sample(0.0, 480.0, 1000.0),
            sample(1.0, 464.0, 1000.0),
            sample(2.0, 448.0, 1000.0),
            sample(3.0, 451.0, 1000.0),
            sample(4.0, 430.0, 1000.0),
            sample(5.0, 414.0, 1000.0),
            sample(6.0, 382.0, 1000.0),
        ];
        let falling = assess_pressure(
            "wsl",
            &MemorySnapshot {
                available_mb: 382,
                capacity_mb: 7941,
                capacity_source: "wsl-test".to_owned(),
            },
            &NativePressure::default(),
            &noisy_decline,
            1.0,
            30.0,
            false,
        );
        assert!(falling.trajectory_confirmed);
        assert_eq!(falling.action, Action::Drain);

        let stable = [
            sample(0.0, 334.0, 1000.0),
            sample(5.0, 334.0, 1000.0),
            sample(10.0, 334.0, 1000.0),
        ];
        let low_but_stable = assess_pressure(
            "wsl",
            &MemorySnapshot {
                available_mb: 334,
                capacity_mb: 7941,
                capacity_source: "wsl-test".to_owned(),
            },
            &NativePressure::default(),
            &stable,
            1.0,
            30.0,
            false,
        );
        assert!(!low_but_stable.collapse_imminent);
        assert_eq!(low_but_stable.new_fanout_floor_mb, 512.0);
        assert_eq!(low_but_stable.action, Action::Hold);
    }

    #[test]
    fn stopping_distance_is_time_consistent_across_small_and_huge_machines() {
        let cases = [
            (512_u64, 1.0_f64),
            (1024, 4.0),
            (1024, 32.0),
            (2048_u64, 8.0_f64),
            (2048, 64.0),
            (8192, 16.0),
            (8192, 128.0),
            (102_400, 64.0),
            (102_400, 2048.0),
            (1_048_576, 512.0),
            (1_048_576, 16_384.0),
            (10_485_760, 4096.0),
            (10_485_760, 131_072.0),
        ];
        for (capacity, rate) in cases {
            let floor = (capacity as f64 * 0.005).clamp(256.0, 1024.0);
            let reserve = (floor + rate * 5.0).min(capacity as f64 * 0.25);
            let assess_at = |seconds_to_reserve: f64| {
                let latest = reserve + rate * seconds_to_reserve;
                let history = (0..=5)
                    .map(|second| {
                        sample(second as f64, latest + rate * (5 - second) as f64, 1000.0)
                    })
                    .collect::<Vec<_>>();
                assess_pressure(
                    "linux",
                    &MemorySnapshot {
                        available_mb: latest.round() as u64,
                        capacity_mb: capacity,
                        capacity_source: "scale-test".to_owned(),
                    },
                    &NativePressure::default(),
                    &history,
                    1.0,
                    30.0,
                    false,
                )
            };

            let before_braking = assess_at(12.0);
            assert_ne!(
                before_braking.action,
                Action::Drain,
                "capacity={capacity} rate={rate}"
            );
            assert_eq!(assess_at(7.0).action, Action::Hold);
            assert_eq!(assess_at(4.0).action, Action::Drain);

            let new_fanout_floor = (floor * 2.0).min(capacity as f64 * 0.30);
            let flat = [
                sample(0.0, new_fanout_floor, 1000.0),
                sample(5.0, new_fanout_floor, 1000.0),
                sample(10.0, new_fanout_floor, 1000.0),
            ];
            let stable = |available_mb: u64| {
                assess_pressure(
                    "linux",
                    &MemorySnapshot {
                        available_mb,
                        capacity_mb: capacity,
                        capacity_source: "scale-test".to_owned(),
                    },
                    &NativePressure::default(),
                    &flat,
                    1.0,
                    30.0,
                    false,
                )
            };
            assert_eq!(
                stable(new_fanout_floor.floor() as u64 - 1).action,
                Action::Hold
            );
            assert!(matches!(
                stable(new_fanout_floor.ceil() as u64).action,
                Action::Allow | Action::Observe
            ));
        }
    }

    #[test]
    fn tracked_growth_without_machine_headroom_loss_is_not_attributed_to_agents() {
        let history = [
            sample(0.0, 12_000.0, 500.0),
            sample(5.0, 12_000.0, 1000.0),
            sample(10.0, 12_000.0, 1500.0),
        ];
        let assessment = assess_pressure(
            "linux",
            &MemorySnapshot {
                available_mb: 12_000,
                capacity_mb: 32_768,
                capacity_source: "test".to_owned(),
            },
            &NativePressure::default(),
            &history,
            1.0,
            30.0,
            false,
        );
        assert_eq!(assessment.attribution, "unknown");
        assert!(!assessment.collapse_imminent);
    }

    #[test]
    fn ample_normal_headroom_never_holds_on_slope_alone_but_the_real_reserve_boundary_does() {
        let snapshot = |available| MemorySnapshot {
            available_mb: available,
            capacity_mb: 7941,
            capacity_source: "wsl-test".to_owned(),
        };
        let ample_decline = [
            sample(0.0, 4712.0, 1000.0),
            sample(1.0, 4612.0, 1020.0),
            sample(2.0, 4512.0, 1040.0),
            sample(3.0, 4412.0, 1060.0),
            sample(4.0, 4312.0, 1080.0),
            sample(5.0, 4212.0, 1100.0),
        ];
        let ample = assess_pressure(
            "wsl",
            &snapshot(4212),
            &NativePressure::default(),
            &ample_decline,
            1.0,
            30.0,
            false,
        );
        assert_eq!(ample.distress, "normal");
        assert_eq!(ample.action, Action::Observe);
        assert!(ample.trajectory_confirmed);
        assert_eq!(ample.automatic_reserve_mb, 756.0);
        assert_eq!(ample.expected_burst_mb, 100.0);
        assert!(
            ample
                .time_to_recovery_reserve_s
                .is_some_and(|tte| tte > 10.0)
        );

        let near_decline = [
            sample(0.0, 2200.0, 1000.0),
            sample(1.0, 2100.0, 1000.0),
            sample(2.0, 2000.0, 1000.0),
            sample(3.0, 1900.0, 1000.0),
            sample(4.0, 1800.0, 1000.0),
            sample(5.0, 1700.0, 1000.0),
        ];
        let near = assess_pressure(
            "wsl",
            &snapshot(1700),
            &NativePressure::default(),
            &near_decline,
            1.0,
            30.0,
            false,
        );
        assert_eq!(near.action, Action::Hold);
        assert!(near.trajectory_confirmed);
        assert!(
            near.time_to_recovery_reserve_s
                .is_some_and(|tte| (5.0..=10.0).contains(&tte))
        );

        let final_decline = [
            sample(0.0, 1700.0, 1000.0),
            sample(1.0, 1600.0, 1000.0),
            sample(2.0, 1500.0, 1000.0),
            sample(3.0, 1400.0, 1000.0),
            sample(4.0, 1300.0, 1000.0),
            sample(5.0, 1200.0, 1000.0),
        ];
        let final_boundary = assess_pressure(
            "wsl",
            &snapshot(1200),
            &NativePressure::default(),
            &final_decline,
            1.0,
            30.0,
            false,
        );
        assert_eq!(final_boundary.action, Action::Drain);
        assert!(final_boundary.trajectory_confirmed);
        assert!(
            final_boundary
                .time_to_recovery_reserve_s
                .is_some_and(|tte| tte <= 5.0)
        );
    }

    #[test]
    fn target_trees_and_opt_in_cap_match_python_contract() {
        let processes = BTreeMap::from([
            (
                10,
                ProcessInfo {
                    pid: 10,
                    ppid: 1,
                    name: "codex".to_owned(),
                    rss_mb: 900,
                    anon_mb: 800,
                    args: vec!["codex".to_owned()],
                    start_token: "a".to_owned(),
                    terminal: String::new(),
                    terminal_identity: String::new(),
                },
            ),
            (
                11,
                ProcessInfo {
                    pid: 11,
                    ppid: 10,
                    name: "node".to_owned(),
                    rss_mb: 75,
                    anon_mb: 70,
                    args: vec!["node".to_owned()],
                    start_token: "b".to_owned(),
                    terminal: String::new(),
                    terminal_identity: String::new(),
                },
            ),
        ]);
        let tracked = tracked_processes(&processes);
        assert_eq!(tracked.len(), 2);
        assert_eq!(tracked[0].tree_rss_mb, 975);
        let mut assessment = assess_pressure(
            "linux",
            &MemorySnapshot {
                available_mb: 10_000,
                capacity_mb: 16_384,
                capacity_source: "test".to_owned(),
            },
            &NativePressure::default(),
            &[],
            1.0,
            30.0,
            false,
        );
        apply_cli_hard_cap(&mut assessment, &tracked, Some(1000.0), true);
        assert_eq!(assessment.cli_memory_used_mb, Some(975.0));
        assert_eq!(assessment.cli_hard_cap_status, "near");
        assert_eq!(assessment.action, Action::Hold);
    }

    #[test]
    fn pressure_and_hard_cap_matrix_matches_reference_edge_cases() {
        let snapshot = |available, capacity| MemorySnapshot {
            available_mb: available,
            capacity_mb: capacity,
            capacity_source: "test".to_owned(),
        };
        let assess = |platform: &str,
                      snapshot: MemorySnapshot,
                      pressure: NativePressure,
                      history: &[HistorySample]| {
            assess_pressure(platform, &snapshot, &pressure, history, 1.0, 30.0, false)
        };

        let first_fall = [sample(0.0, 9000.0, 500.0), sample(1.0, 8500.0, 500.0)];
        assert_eq!(
            assess(
                "linux",
                snapshot(8500, 32_768),
                NativePressure::default(),
                &first_fall,
            )
            .action,
            Action::Observe
        );
        let flat = [
            sample(0.0, 9000.0, 500.0),
            sample(1.0, 8500.0, 500.0),
            sample(2.0, 8500.0, 500.0),
        ];
        assert_eq!(
            assess(
                "linux",
                snapshot(8500, 32_768),
                NativePressure::default(),
                &flat,
            )
            .action,
            Action::Observe
        );
        let sustained = [
            sample(0.0, 9000.0, 500.0),
            sample(1.0, 8500.0, 500.0),
            sample(2.0, 8000.0, 500.0),
        ];
        assert_eq!(
            assess(
                "linux",
                snapshot(8000, 32_768),
                NativePressure::default(),
                &sustained,
            )
            .action,
            Action::Observe
        );

        let spike = [
            sample(0.0, 10_000.0, 500.0),
            sample(10.0, 2000.0, 500.0),
            sample(20.0, 10_000.0, 500.0),
        ];
        let stable = [sample(0.0, 900.0, 500.0), sample(20.0, 900.0, 500.0)];
        assert_eq!(
            assess(
                "linux",
                snapshot(10_000, 32_768),
                NativePressure::default(),
                &spike,
            )
            .action,
            Action::Allow
        );
        assert_eq!(
            assess(
                "linux",
                snapshot(900, 65_536),
                NativePressure::default(),
                &stable,
            )
            .action,
            Action::Allow
        );

        let mut one_swap_spike = [
            sample(0.0, 8000.0, 500.0),
            sample(1.0, 8000.0, 500.0),
            sample(2.0, 8000.0, 500.0),
        ];
        one_swap_spike[1].swap = Some(2.0);
        one_swap_spike[2].swap = Some(2.0);
        let transient = assess(
            "linux",
            snapshot(8000, 8192),
            NativePressure::default(),
            &one_swap_spike,
        );
        assert_eq!(
            (transient.distress.as_str(), transient.action),
            ("normal", Action::Allow)
        );
        let transient_macos = assess(
            "darwin",
            snapshot(8000, 8192),
            NativePressure::default(),
            &one_swap_spike,
        );
        assert_eq!(
            (transient_macos.distress.as_str(), transient_macos.action),
            ("normal", Action::Allow)
        );

        let mut sustained_swap = [
            sample(0.0, 8000.0, 500.0),
            sample(1.0, 8000.0, 500.0),
            sample(2.0, 8000.0, 500.0),
        ];
        sustained_swap[1].swap = Some(2.0);
        sustained_swap[2].swap = Some(4.0);
        let pressure = assess(
            "linux",
            snapshot(8000, 8192),
            NativePressure::default(),
            &sustained_swap,
        );
        assert_eq!(
            (pressure.distress.as_str(), pressure.action),
            ("elevated", Action::Observe)
        );
        let macos_pressure = assess(
            "darwin",
            snapshot(8000, 8192),
            NativePressure::default(),
            &sustained_swap,
        );
        assert_eq!(
            (macos_pressure.distress.as_str(), macos_pressure.action),
            ("elevated", Action::Observe)
        );

        let mixed_build_pressure = [
            sample(0.0, 5100.0, 1000.0),
            sample(1.0, 5068.0, 1012.0),
            sample(2.0, 5036.0, 1024.0),
        ];
        let build_pressure = NativePressure {
            full_avg10: 1.0,
            ..NativePressure::default()
        };
        let build = assess(
            "wsl",
            snapshot(5036, 8192),
            build_pressure,
            &mixed_build_pressure,
        );
        assert_eq!(build.distress, "elevated");
        assert_eq!(build.time_to_exhaustion_s, Some(157.4));
        assert_eq!(build.action, Action::Observe);

        let normal_build = assess(
            "wsl",
            snapshot(5036, 8192),
            NativePressure::default(),
            &mixed_build_pressure,
        );
        assert_eq!(normal_build.distress, "normal");
        assert_eq!(normal_build.action, Action::Allow);

        let base = [sample(0.0, 4000.0, 500.0), sample(10.0, 4000.0, 500.0)];
        let near = assess(
            "linux",
            snapshot(500, 8192),
            NativePressure {
                full_avg10: 1.0,
                ..NativePressure::default()
            },
            &base,
        );
        assert_eq!(
            (near.distress.as_str(), near.action),
            ("elevated", Action::Hold)
        );

        let pressures = [
            (
                "linux",
                NativePressure {
                    full_avg10: 10.0,
                    ..NativePressure::default()
                },
                Action::Observe,
            ),
            (
                "darwin",
                NativePressure {
                    state: "critical".to_owned(),
                    ..NativePressure::default()
                },
                Action::Observe,
            ),
            (
                "windows",
                NativePressure {
                    commit_remaining_mb: Some(100),
                    ..NativePressure::default()
                },
                Action::Drain,
            ),
        ];
        for (platform, pressure, expected_action) in pressures {
            let result = assess(platform, snapshot(4000, 8192), pressure, &base);
            assert_eq!(
                (result.distress.as_str(), result.action),
                ("critical", expected_action)
            );
        }

        let commit_history = [
            HistorySample {
                commit_remaining: Some(8000.0),
                ..sample(0.0, 8000.0, 100.0)
            },
            HistorySample {
                commit_remaining: Some(2000.0),
                ..sample(10.0, 8000.0, 100.0)
            },
        ];
        let mut result = assess(
            "windows",
            snapshot(8000, 16_384),
            NativePressure {
                commit_remaining_mb: Some(2000),
                ..NativePressure::default()
            },
            &commit_history,
        );
        assert_eq!(result.action, Action::Observe);
        assert!(!result.trajectory_confirmed);
        assert_eq!(result.commit_time_to_exhaustion_s, Some(3.3));

        apply_cli_hard_cap(&mut result, &[], None, true);
        assert_eq!(result.cli_hard_cap_status, "disabled");
        let process_map = BTreeMap::from([(
            10,
            ProcessInfo {
                pid: 10,
                ppid: 1,
                name: "codex".to_owned(),
                rss_mb: 1200,
                anon_mb: 1200,
                args: vec!["codex".to_owned()],
                start_token: "root".to_owned(),
                terminal: String::new(),
                terminal_identity: String::new(),
            },
        )]);
        let tracked = tracked_processes(&process_map);
        let mut safe = assess(
            "linux",
            snapshot(10_000, 16_384),
            NativePressure::default(),
            &[],
        );
        apply_cli_hard_cap(&mut safe, &tracked, Some(1000.0), true);
        assert_eq!(
            (safe.action, safe.cli_hard_cap_status.as_str()),
            (Action::Drain, "exceeded")
        );
        let mut unavailable = safe.clone();
        apply_cli_hard_cap(&mut unavailable, &tracked, Some(1000.0), false);
        assert_eq!(unavailable.cli_hard_cap_status, "unavailable");
        assert!(unavailable.cli_memory_used_mb.is_none());
    }

    #[test]
    fn critical_wsl_pressure_with_ample_slow_headroom_only_observes() {
        let mut first = sample(0.0, 8860.0, 384.0);
        let mut second = sample(1.0, 8854.0, 384.0);
        first.swap = Some(0.0);
        second.swap = Some(6.01);
        let assessment = assess_pressure(
            "wsl",
            &MemorySnapshot {
                available_mb: 8854,
                capacity_mb: 9945,
                capacity_source: "regression-fixture".to_owned(),
            },
            &NativePressure {
                full_avg10: 10.22,
                ..NativePressure::default()
            },
            &[first, second],
            1.0,
            30.0,
            false,
        );

        assert_eq!(assessment.distress, "critical");
        assert_eq!(assessment.action, Action::Observe);
        assert!(!assessment.trajectory_confirmed);
        assert!(!assessment.collapse_imminent);
        assert!(
            assessment
                .time_to_recovery_reserve_s
                .is_some_and(|tte| tte > 1_000.0)
        );
    }

    #[test]
    fn target_detection_is_exact_and_filters_tiny_support_processes() {
        let process = |pid, ppid, name: &str, anon_mb, args: &[&str]| ProcessInfo {
            pid,
            ppid,
            name: name.to_owned(),
            rss_mb: anon_mb,
            anon_mb,
            args: args.iter().map(|value| (*value).to_owned()).collect(),
            start_token: format!("start-{pid}"),
            terminal: String::new(),
            terminal_identity: String::new(),
        };
        assert!(is_target(&process(1, 0, "codex.exe", 100, &[])));
        assert!(is_target(&process(
            4,
            0,
            "codex-aarch64-apple-darwin",
            100,
            &[]
        )));
        assert!(is_target(&process(
            5,
            0,
            "codex-x86_64-apple-darwin",
            100,
            &[]
        )));
        assert!(is_target(&process(
            2,
            0,
            "node",
            100,
            &["node", "/usr/local/bin/claude"]
        )));
        assert!(!is_target(&process(3, 0, "codex-helper", 100, &[])));

        let processes = BTreeMap::from([
            (10, process(10, 1, "codex", 100, &["codex"])),
            (11, process(11, 10, "node", 31, &["node"])),
            (12, process(12, 10, "node", 32, &["node"])),
        ]);
        let tracked = tracked_processes(&processes);
        assert!(
            tracked
                .iter()
                .any(|item| item.pid == 10 && item.role == "lead")
        );
        assert!(!tracked.iter().any(|item| item.pid == 11));
        assert!(
            tracked
                .iter()
                .any(|item| item.pid == 12 && item.role == "support")
        );
    }

    #[test]
    fn profiles_and_invalid_overrides_match_reference_policy() {
        let mut policies = BTreeMap::new();
        for profile in ["protect", "balanced", "performance"] {
            let (mut config, path) = config_from_json(
                profile,
                &format!(r#"{{"MEMORY_SUPERVISOR_POLICY_PROFILE":"{profile}"}}"#),
            );
            policies.insert(profile, resolve_policy(&mut config, 8192));
            fs::remove_file(path).unwrap();
        }
        assert!(
            policies["protect"].value("MEMORY_SUPERVISOR_MEM_GREEN")
                > policies["balanced"].value("MEMORY_SUPERVISOR_MEM_GREEN")
        );
        assert!(
            policies["balanced"].value("MEMORY_SUPERVISOR_MEM_GREEN")
                > policies["performance"].value("MEMORY_SUPERVISOR_MEM_GREEN")
        );
        assert!(
            policies["protect"].value("MEMORY_SUPERVISOR_LEAK_RSS_MB")
                < policies["balanced"].value("MEMORY_SUPERVISOR_LEAK_RSS_MB")
        );
        assert!(
            policies["balanced"].value("MEMORY_SUPERVISOR_LEAK_RSS_MB")
                < policies["performance"].value("MEMORY_SUPERVISOR_LEAK_RSS_MB")
        );

        let (mut invalid_order, order_path) = config_from_json(
            "invalid-order",
            r#"{
                "MEMORY_SUPERVISOR_MEM_GREEN": 100,
                "MEMORY_SUPERVISOR_MEM_YELLOW": 200,
                "MEMORY_SUPERVISOR_MEM_ORANGE": 50
            }"#,
        );
        let fallback = resolve_policy(&mut invalid_order, 8192);
        for key in [
            "MEMORY_SUPERVISOR_MEM_GREEN",
            "MEMORY_SUPERVISOR_MEM_YELLOW",
            "MEMORY_SUPERVISOR_MEM_ORANGE",
        ] {
            assert!(
                !fallback
                    .overrides
                    .iter()
                    .any(|override_key| override_key == key)
            );
        }
        assert!(
            invalid_order
                .configuration_error()
                .is_some_and(|error| error.contains("memory_threshold_order"))
        );
        fs::remove_file(order_path).unwrap();

        let (mut not_finite, finite_path) = config_from_json(
            "not-finite",
            r#"{"MEMORY_SUPERVISOR_LEAK_SLOPE_MBS":"nan"}"#,
        );
        let fallback = resolve_policy(&mut not_finite, 8192);
        assert!(
            fallback
                .value("MEMORY_SUPERVISOR_LEAK_SLOPE_MBS")
                .is_finite()
        );
        assert!(
            !fallback
                .overrides
                .iter()
                .any(|key| key == "MEMORY_SUPERVISOR_LEAK_SLOPE_MBS")
        );
        assert!(
            not_finite
                .configuration_error()
                .is_some_and(|error| error.contains("MEMORY_SUPERVISOR_LEAK_SLOPE_MBS"))
        );
        fs::remove_file(finite_path).unwrap();
    }
}
