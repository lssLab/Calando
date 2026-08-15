# Operations, notifications, and recovery

<p align="center">
  <strong>English</strong> · <a href="operations.ko.md">한국어</a>
</p>

## Notifications

Memory Supervisor does not notify on every memory reading. It sends one notice each
time a real protection action begins or fully recovers, or when a connection or
protection condition needs the user's attention.

| Route | Where it appears | When and what it reports |
| --- | --- | --- |
| Terminal | The exact terminal running the affected Claude Code or Codex CLI process | Immediately shows the reason, PID, and recovery command when a process is paused or resumed, or when a lead pause is released once to check recovery. This route is always on. |
| OS | Linux, WSL, macOS, or Windows desktop notifications | Appears when protection first acts or fully recovers, or when federation connectivity or protection needs attention. This optional route works when desktop notifications are available. |
| Telegram | A bot's private chat or group selected by the user | Reports the start and recovery of important actions and connection or protection problems. It includes memory state, the reason, a PID when there is a target, and the next action, leaving a history the user can check while away. |
| Discord | A connected channel, webhook, or direct message | Posts the same important actions, recoveries, and attention items. This optional route is intended for a team channel or personal notification. |

Terminal, OS, Telegram, and Discord delivery is attempted immediately after an
event is recorded. An unchanged condition is not sent repeatedly. The lead
receives the same situation and recovery state at its next hook boundary. The
terminal route always remains connected; configure and test optional routes
with these commands:

```bash
memory-supervisor notifications show
memory-supervisor notifications routes os
memory-supervisor notifications discord-webhook
memory-supervisor notifications telegram
memory-supervisor notifications test
```

Do not put a Discord webhook URL or a Discord or Telegram bot token on the
command line. Enter it in the hidden prompt that appears after running the
setup command. Changes apply to the next notification without restarting the
supervisor or an AI program. See
[notification setup](notifications.md) for route selection and removal,
Discord channel and DM setup, Telegram group setup, and troubleshooting.

## Skills and commands in Claude Code and Codex

The installer connects three separate pieces: **hooks** that make automatic
decisions, a **skill** that teaches the agent to understand and explain status,
and **short commands** that invoke that workflow. Hooks run without being
called by the user; the skill does not enforce memory policy itself.

| Where it is used | What to enter | What it does |
| --- | --- | --- |
| Claude Code | Ask “check memory status”, use `/memory-supervisor`, or use `/memory-status` | The installed skill or shortcut reads the full status and explains the cause, automatic recovery, and any required command. |
| Codex CLI | Use `$memory-supervisor check memory status`; use `/skills` to confirm discovery. `/prompts:memory-status` is a compatibility shortcut. | Runs the same status workflow through Codex's primary skill path. Hook trust and enablement remain separate in `/hooks`. |
| Codex Desktop App | Use `$memory-supervisor check memory status` or ask naturally in a task | Uses the same user-level Codex skill in each task. There is no separate App skill; manage hooks in **Settings → Hooks**. |
| Operating-system terminal | Use `memory-status` or `memory-supervisor ...` | These are real status, setup, and recovery commands rather than skills. `resume`, `terminate`, and `kill` run only after an explicit user request. |

The skill reads `memory-status --all` and explains the cause and next action,
but it does not resume or terminate a process without the user's approval. If
Claude Code or Codex is installed after Memory Supervisor, run
`memory-supervisor update` and verify the connection with
`memory-status --connections`. See the [Claude Code guide](usage-claude.md)
and [Codex guide](usage-codex.md) for the detailed differences.

## Security

Memory Supervisor reads operating-system memory and process information, plus
session, agent, tool, working-directory, and connection-state information and a
command prefix supplied by Claude Code and Codex hooks. It uses this information
only to decide whether new work may start and to identify an exact control
target.

Automatic control stops at delaying future Claude Code or Codex work and, at
the final protection stage, pausing and resuming one verified local work
process. It never terminates a program automatically or controls unrelated
programs. Normal monitoring makes no external request; only GitHub installation
and updates and operator-enabled Discord or Telegram notifications use the
network.

**This is the complete inspection and control boundary; Memory Supervisor does
not handle anything outside it.** It does not use prompts, conversation text,
model responses, or file contents that may be present in a hook payload for a
control decision and does not retain them. It does not open project files or
process memory directly, or inspect or change browser or IDE internal data,
Claude or ChatGPT credentials, or operating-system kernel,
memory, swap, and firewall settings. See
[security and data/control boundaries](security.md) for the complete list
of stored data, same-machine federation fields, and safeguards.

## Control and recovery

When memory becomes stable again, paused work resumes automatically, one item
at a time. If a lead was paused because its own memory kept growing, it resumes
once automatically so the supervisor can check the result. If the same growth
returns, the lead pauses again and waits for the user to decide. To resume it
manually, check the current status first and use the PID shown there.

```bash
memory-status
memory-supervisor resume [pid]
```

A lead pause is intentionally very rare. It is the **final protection stage**,
used only when staged new-work delays and subagent and tool controls have not
removed the danger, and sustained growth from that same lead and its exact
terminal have been confirmed. Most incidents finish earlier through a smaller
work scope, a worker pause, or automatic recovery.

If Claude Code or Codex was accidentally terminated, the CLI restores its
conversation and the installed `SessionStart` hook delivers the retained
memory incident and current decision to the lead once:

```bash
claude --resume
codex resume
```

Use only these two commands when you intentionally want protection off or on.
`off` stops and disables the background service while keeping installed Claude
Code and Codex hooks connected in silent pass-through mode. The choice survives
reboots and `memory-supervisor update`; one `on` command restores protection.

```bash
memory-supervisor off
memory-supervisor on
```

`off` refuses to strand a supervisor-managed paused PID or an in-progress
process action; resolve the listed PID first. If the service stops without an
intentional `off`, hooks still discard its stale decision after ten seconds and
warn that **protection is unavailable**.

```bash
memory-status --connections
memory-supervisor update
```

If you need a fixed limit, you can optionally set one total memory ceiling for
all Claude Code and Codex programs in that local environment:

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

Commands are grouped by what they control:

- `memory-status` commands are read-only: local cause, federation, and
  service, hook, and notification connections.
- `on` and `off` control the whole current installation. One command covers
  every connected Claude Code and Codex session; another OS, WSL distribution,
  or VM must be toggled inside that environment.
- `resume` continues a process paused by the supervisor. `terminate` and
  `kill` are operator-selected process exits after reviewing the cause.
- `budget` applies an optional cap only to Claude Code and Codex in the current
  environment, not the whole computer or Chrome.
- `update` reapplies the service and detected CLI connections.
  `notifications` controls optional OS, Discord, and Telegram routes; lead
  hooks and exact-terminal notices remain mandatory.

## Common commands

| Command | Purpose |
| --- | --- |
| `memory-status` | Local health, cause, and next action |
| `memory-status --all` | Windows, WSL, virtual-machine, and container state on the same computer |
| `memory-status --connections` | Background service, AI CLI, and notification connections |
| `memory-supervisor on` / `off` | Persistently enable or disable protection in this environment; connected hooks pass through while off |
| `memory-supervisor update` | Update and reconnect detected CLIs |
| `memory-supervisor budget` | Show adaptive capacity and any optional cap in this environment |
| `memory-supervisor budget set <GiB>` / `budget off` | Set or remove the aggregate local Claude Code and Codex cap |
| `memory-supervisor resume [pid]` | Resume a supervisor-paused process; omit PID only when exactly one is paused |
| `memory-supervisor terminate <pid>` | Gracefully terminate one verified managed process |
| `memory-supervisor kill <pid>` | Force-terminate one verified process as a last resort |
| `memory-supervisor notifications show` | Show notification settings with secrets hidden |
| `memory-supervisor notifications routes <all\|none\|routes>` | Select optional OS, Discord, and Telegram routes |
| `memory-supervisor notifications test` | Test enabled optional notification routes |
| `memory-supervisor uninstall` | Remove its services and AI CLI connections while preserving state |

## Verification

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust unit, integration, and installer tests cover policy, process safety,
Claude Code and Codex wiring, federation, recovery, and release bundles. The
GitHub Actions checks builds and platform contracts on Linux x86-64,
Windows x86-64, Apple Silicon macOS, and macOS x86-64 under Rosetta. A real
near-exhaustion boundary is covered by bounded physical-machine verification
plus deterministic simulation. See [Test coverage](../testing/test-matrix.md).
