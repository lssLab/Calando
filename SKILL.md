---
name: memory-supervisor
description: Operate and troubleshoot the cross-platform AI CLI memory supervisor for Claude Code and Codex. Use when the user asks about memory pressure, leaks, OOM, paused or frozen agent processes, supervisor installation or tuning, or when context contains "[memory-supervisor]" or "MEMORY SUPERVISOR NOTICE".
---

# Claude Code & Codex Memory Supervisor

## 이 스킬로 사람이 시킬 일은 네 가지다

이 시스템은 **상황에 따라 자동으로 처리**된다(감지·admission 반려·동결·통지 전부
데몬+훅이 사람 개입 없이 수행). 그래서 에이전트에게 명시적으로 시킬 일은 네 가지뿐:

1. **상태 확인** — "메모리 상태 어때" / `/memory-status` → `memory-status --all` 실행·요약.
2. **동결 프로세스 처분** — 알림을 받은 뒤 사용자가 명시적으로 요청할 때만
   `memory-supervisor resume|terminate|kill <pid>`.
3. **예외 진단·튜닝** — 자동 판단 근거(TTE, distress, attribution, reserve, 선택형 local CLI
   cap, delivery ledger)를 먼저 확인하고, 사용자가 명시적으로 요청한 경우에만 설정을 바꾼다.
4. **전체 전원 전환** — 사용자가 명시적으로 요청한 경우에만 `memory-supervisor on|off`를 실행한다.

나머지(설치·튜닝·플랫폼 이슈)는 이 스킬의 참조 문서로 라우팅한다. 자동으로 되는 일을
"해달라"고 시킬 필요는 없다 — 그것이 이 시스템의 설계 목적이다.

## Inspect first

Run the portable status command before proposing changes:

```bash
memory-status
memory-status --json
```

For installation or AI-CLI-wiring questions, run `memory-status --connections`; it reports the
daemon plus each detected CLI's skill and hook without changing configuration. If a supported CLI
was installed or updated later, or the report says `NEEDS ATTENTION`, the single repair path is
`memory-supervisor update`. It updates the source when possible, reloads the service, and reconnects
every detected supported CLI. Run it only after explicit user authorization because it fetches and
activates executable code. Afterward, always run `memory-status --connections`. A user must personally
trust a new or changed Codex hook in CLI `/hooks` or App **Settings → Hooks**; restarting cannot grant
trust. An unchanged hook definition needs no re-trust after a binary-only update. Claude Code has no
per-hook hash approval, but an interactive session holds every settings-file hook, including the
user hook in `~/.claude/settings.json`, until the user accepts workspace trust for the current folder
or one of its parents. Claude's `/hooks` screen is read-only and cannot grant that trust. `Claude Code
CONNECTED` proves only the supported version, skill, and user-hook wiring; it cannot prove workspace
trust for a particular folder. Organization policy can still disable or restrict hooks.

Codex trust and enablement are separate persisted states. `Trust all` writes current trust hashes but
does not turn an entry with `enabled=false` back on. When App health reports `disabled`, tell the user
to use **Settings → Hooks** and switch on the named entry; never direct a Desktop App user to CLI
`/hooks`. If the diagnostic reports split path state, explain that one shared Windows/WSL hook file can
have different on/off records for the path seen by each runtime, and the live App Server route wins.

If the command is unavailable, resolve `MEMORY_SUPERVISOR_DIR`, then `~/.memory-supervisor/state-dir`, then the default
`~/.cache/memory-supervisor/state.json`. Treat a missing, invalid, or older-than-10-second state as
supervisor unavailable and fail open.

## Turn protection on or off

```bash
memory-supervisor off
memory-supervisor on
```

`off` is a persistent owner choice, not an incident response. It disables the current environment's
service and automatic startup while leaving Claude Code and Codex hooks installed in silent
pass-through mode. `memory-status --connections` reports `OFF`, and update/reboot preserves the
choice. `on` restores the same installation and waits for fresh state. Do not substitute raw
`systemctl`, `launchctl`, or Scheduled Task commands for this normal control path.

The command refuses to turn off while a supervisor-owned PID is paused or process control is
pending; resolve that exact PID first. The power state is local to one OS/PID-control environment,
so Windows, each WSL distribution, VM, and isolated container are toggled separately. Never run
either command without explicit user authorization.

## Respond to a notice

When `[memory-supervisor]` or `MEMORY SUPERVISOR NOTICE` appears:

1. Read the first action block from `memory-status`; do not infer a crash from an unresponsive CLI.
2. When `admission_level` is ORANGE/RED, do not create new agents/workflows. Existing work may
   continue and should drain naturally.
3. Report utilization separately from adaptive `action`, `distress`, `attribution`, TTE, reserve,
   opt-in `cli_hard_cap_status`, incident/PID/recovery policy, and notification deliveries.
4. Resume expansion when adaptive admission returns GREEN/YELLOW; raw utilization may remain RED
   for a stable high-memory workload.
5. For a lead event, quote the phase-specific user message: sustained material process-growth evidence and
   system attribution are different claims. Explain the next automatic/manual action and that the
   model receives the incident only at its next hook boundary even though terminal/OS/remote
   notification can arrive first.

## Handle a suspended process

Present the evidence and keep the owner in control:

```bash
memory-supervisor resume <pid>       # reversible recovery
memory-supervisor resume             # only when exactly one managed PID is paused
memory-supervisor terminate <pid>    # graceful termination where supported
memory-supervisor kill <pid>         # last resort
```

First respect `recovery_policy`: pressure-paused workers recover one at a time automatically, and
a lead paused after sustained material-growth evidence gets one automatic `LEAD_PROBATION` attempt. Do not race those paths with a manual
signal. After `PROBATION_FAILED`, or for a leak-paused child awaiting owner judgment, use
`events.log`, process role, memory, slope, TTE, and workload to explain the choice. Do not kill or
manually resume a process without explicit user authorization.
Raw `SIGCONT`/`NtResumeProcess` and an authorized manual resume bypass automatic lead probation;
the supervisor emits a warning to save work, avoid fan-out, and inspect `memory-status` because the
original cause may still be present.
A `memory-supervisor` process action waits for the daemon. Distinguish `completed`, `rejected`, `unconfirmed`, and
`signal completed but runtime finalization is unconfirmed`; never blindly retry the last case.

A supervisor resume continues the same OS process and in-memory agent session. A supervisor daemon
restart is different: it reloads `runtime.json` and keeps the agent's existing stop state. If the
agent CLI itself was terminated, use that AI CLI's transcript/session resume mechanism. The AI CLI
restores the conversation in a new process; its installed `SessionStart` hook automatically injects
any retained, unseen resource incident and current supervisor decision once. The supervisor does
not resurrect the old PID or transcript itself.

## Tune safely

Persist tuning in `~/.config/memory-supervisor/config.json`; environment variables with the same
names override it. Do not put bootstrap paths (`MEMORY_SUPERVISOR_DIR`, `MEMORY_SUPERVISOR_FEDERATION_DIR`,
`MEMORY_SUPERVISOR_FORCE_PLATFORM`) in that JSON.

Normal operation requires no user budget, reserve, `none|stop` choice, profile, or hard cap; the
hard cap is off unless `MEMORY_SUPERVISOR_CLI_HARD_CAP_MB` is explicitly present. Inspect
`memory_capacity_mb`, short/long slopes, native distress/confidence, TTE,
`automatic_reserve_mb`, and `expected_burst_mb` before proposing any override. Profiles are only
advanced compatibility presets for users who explicitly request one:

Use `memory-supervisor budget` to inspect the cross-environment picture (this environment's
capacity, the physical machine estimate, peer environments' explicit budgets, and the theoretical versus currently
possible totals) and `memory-supervisor budget set <GiB>` for an explicitly requested cap; the
number is GiB, `MB`/`GB` suffixes work, and `memory-supervisor hard-cap set <MB>` is the
MB-precision alias for the same setting. `set` validates against the currently possible total,
refuses an oversized request with the exact per-environment reductions, asks for confirmation at 90% or
more of the currently possible total or when machine-wide explicit budgets would reach 90% of the
physical estimate (`--yes` for non-interactive runs), applies one aggregate ceiling to every Claude Code and
Codex tree in the current control environment, and reloads the local service; `budget off` removes it.
Windows, each WSL distribution, VM guest, and isolated container require their own one-line command
and may use the same or different values. Do not invent a cross-environment pooled quota or a per-CLI cap;
neither exists — only the shared-physical-total accounting above spans environments, and it is
read-only.

| Profile | Choose when | Effect |
| --- | --- | --- |
| `protect` | small RAM, many agents, OOM is costly | earlier available/PSI/leak action |
| `balanced` | normal use | default adaptive hybrid |
| `performance` | ample RAM, throughput matters | later adaptive action; never bypasses an explicit cap |

Persist an explicitly requested choice as `"MEMORY_SUPERVISOR_POLICY_PROFILE": "balanced"` (or
`protect`/`performance`). After a
restart, run `memory-status` again and report the resolved values. The advanced settings are:

- `MEMORY_SUPERVISOR_CAPACITY_MB` only when automatic OS/cgroup capacity needs explicit calibration.
- `MEMORY_SUPERVISOR_CLI_HARD_CAP_MB` is an opt-in aggregate ceiling in MB for all supported CLI trees owned by
  this installed OS/kernel. It defaults off; use `memory-supervisor budget off` to disable it. Configure Windows,
  WSL, macOS, Linux, each VM, and isolated containers separately because PID enforcement cannot
  cross a kernel boundary. Near the cap, admission holds; above it, only one exact growing PID can
  pause per reaction interval. Suspension does not reclaim resident memory and sampling can
  overshoot, so use native cgroup/container/VM limits for byte-exact quotas.
- `MEMORY_SUPERVISOR_MEM_GREEN/YELLOW/ORANGE` for optional available-memory threshold overrides in MB.
- `MEMORY_SUPERVISOR_PSI_GREEN/YELLOW/ORANGE` for Linux PSI thresholds.
- `MEMORY_SUPERVISOR_LEAK_RSS_MB` and `MEMORY_SUPERVISOR_LEAK_SLOPE_MBS` for observations only;
  neither grants pause authority by itself.
- `MEMORY_SUPERVISOR_LEAK_STOP_MB` and `MEMORY_SUPERVISOR_LEAK_ACTION=stop|none` as advanced circuit-breaker overrides;
  direct containment still requires the absolute stop size, full-window and recent sustained growth,
  a bounded projected time to the automatic reserve (or imminent collapse), reliable PID identity,
  and every actuator guard. Automatic evidence-driven protection
  defaults on across all platforms.
- `MEMORY_SUPERVISOR_HYSTERESIS_S`, `MEMORY_SUPERVISOR_PRETOOL_HOLD_S`, and `MEMORY_SUPERVISOR_YIELD_S` for timing. The pre-tool window is also the RED-only `SubagentStart` fallback; yield applies only once per agent/mixed Claude RED action incident.
- `MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S` for the cached CIM inventory interval (default 3 seconds).
- AI CLI `hook` context and exact `terminal` action notices are mandatory and cannot be disabled
  by a command, file, or environment override. Use
  `memory-supervisor notifications routes all|none|os,discord,telegram` to select only the optional
  delivery routes without editing the private backing file. Use
  `memory-supervisor notifications discord-webhook|discord-channel|discord-dm|telegram` for hidden
  credential prompts, `show` to inspect masked settings, and `test` to verify configured transports.
  Changes are reread for the next event without a daemon or CLI-session restart.

The budget command saves and applies its own setting. After another explicitly authorized
`config.json` change, use `memory-supervisor update` to apply it on any OS; notification commands are
the other exception and apply dynamically. Treat
`sensor_ok=false`, `configuration_error`, `runtime_error`, `pending_control`, or
`protection_degraded=true` as degraded protection even if a pressure level is present. Never lower
thresholds while a large, known-good build is running.

Running `memory-supervisor update` reloads only the supervisor daemon and preserves stopped identities
through `runtime.json`, but it also updates source and changes AI CLI hooks/trust. Prefer a safe
boundary after active agent sessions end. If the user explicitly authorizes live deployment, explain that sessions usually
continue and fresh daemon state normally returns within seconds, while a brief fail-open protection
gap, AI CLI hook reload boundary, or Codex `/hooks` trust step can outlast that daemon restart.

## Route platform and CLI work

- Read `docs/guides/platforms.md` for Windows, macOS, Linux, and WSL installation or service issues.
- Read `docs/guides/usage-claude.md` for Claude Code hook behavior.
- Read `docs/guides/usage-codex.md` for the Codex 0.145.0+ stable-hook requirement and trust review.
- Read `docs/guides/codex-app.md` for shared App Server ownership and blind-control boundaries.

Keep the operational answer concise and use the public product guides for supporting detail.
