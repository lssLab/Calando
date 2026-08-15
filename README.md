<p align="center">
  <img src="assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <strong>English</strong> · <a href="README.ko.md">한국어</a>
</p>

<p align="center">
  <em>Keeps memory use under control while Claude Code and Codex handle long-running,
  large-scale workloads, helping prevent terminal or app freezes and unexpected session exits.</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 or newer"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 or newer"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 or newer and Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="Test"></a>
  <a href="docs/guides/setup.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="docs/guides/performance.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="docs/guides/security.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#installation"><strong>Install</strong></a> ·
  <a href="#how-it-works-in-30-seconds">How it works</a> ·
  <a href="#common-commands">Commands</a> ·
  <a href="#documentation">Docs</a> ·
  <a href="README.full.md">Detailed guide</a>
</p>

## Why Calando

During long-running work in Claude Code, Codex CLI, or Codex Desktop App,
subagents, builds, tests, and browser tools can pile up. If available memory
drops quickly, a CLI terminal can stop responding or its session can end; in
the Desktop App, multiple conversations sharing the App Server can be affected
together. In either case, pending results and the flow of work can be interrupted.

Calando does not limit work simply because memory usage is high. In both the CLI
and Desktop App, it delays new work in stages only when real risk
gets close, while keeping in-flight work and result delivery running whenever
possible. This helps prevent a session from ending abruptly.

Protection does not jump from unrestricted work to a full stop. It moves up
one stage at a time as real risk gets closer, then unwinds in reverse after
recovery.

1. **Automatic discovery** — Launch `claude` or `codex` as usual, or start a
   conversation in Codex Desktop App. Calando distinguishes CLI sessions from
   App conversations automatically, determines memory capacity,
   available memory, decline speed, and the buffer needed for upcoming work,
   then sets the protection thresholds automatically. Users do not need to
   configure a budget or keep checking status.
2. **Run without limits** — High memory use alone does not trigger a restriction
   when available memory and its rate of change remain stable.
3. **Observe at full performance** — A fast decline by itself does not limit
   work while plenty of memory remains. The supervisor keeps everything open
   while checking whether the decline persists and real risk is getting closer.
4. **Delay new subagents, workflows, and tasks first** — When sustained loss of
   memory headroom brings risk closer or leaves too little room for another work
   block, this stage delays only creation of new subagents, workflows, and tasks
   without touching work already in progress. It does not itself delay a build
   or test start or pause a running program, giving current work time to finish
   and memory time to recover.
5. **Reduce work gradually** — As risk moves closer, creation of new subagents,
   workflows, and tasks is blocked first. Only when reliable evidence shows
   that AI work is causing the loss, or an optional user-set cap is exceeded,
   does future work for existing agents narrow through `all work → no new
   subagents, workflows, or tasks → no new memory-heavy starts such as builds
   and tests → handoff, coordination, status, stop, recovery, and small reads only`.

   Subagents are not restricted all at once. With enough time, one subagent moves
   down one rung on its next tool call; with less time, the supervisor applies
   only the minimum group of restrictions needed before the process reaches the
   reserve, then remeasures memory.
   Unselected agents and running work stay unchanged. Subagents are selected for
   restriction in this order: (1) verified abnormal growth in the linked process,
   (2) a current or immediately preceding tool for agent, workflow, or task
   creation or heavy work such as a build or test, (3) an already narrower state,
   (4) a shorter time for the linked process to reach the reserve, and (5) a
   newer start.

   The lead narrows only if danger remains after every subagent reaches the
   narrowest state. If the lead is the verified dominant cause and subagent
   restrictions would be too late, it moves down one rung first. When an external
   program is the sole cause, existing AI work stays open; only new subagents,
   workflows, tasks, and—under critical system distress—heavy starts wait.
6. **Pause one process as a last resort** — Only when danger continues and one
   process belonging to Claude Code or Codex shows confirmed sustained growth
   does the supervisor pause that process without terminating it. The terminal
   shows the action immediately, and the lead receives the same context before
   its next task.
7. **Recover in reverse** — After memory remains stable, work reopens one stage
   at a time starting from result delivery, and paused processes resume one at a
   time.

The goal is not to use less memory. It is to protect Claude Code and Codex CLI
terminal sessions and Codex Desktop App conversations while sustaining the
highest possible performance for as long as possible.

## Installation

Open the **terminal** for your environment and paste the matching one-line command. There is no Git,
Python, Rust, or separate installer to prepare. Installation is scoped to your current user, so it
does not require `sudo` or an administrator shell.

### Linux · WSL2 · macOS terminal

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

### Windows PowerShell terminal

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

When the command finishes, the background service is running and detected Claude Code and Codex
hooks are connected automatically. It does not close a running AI program or interrupt work in
progress.

> [!IMPORTANT]
> The Windows executable is currently under review by [SignPath Foundation](https://signpath.org/),
> so Smart App Control must remain **Off** on Windows 11 until the review is complete.
>
> - **Windows 11:** turn Smart App Control **Off** before installation and leave it off while using the native build.
> - **Windows 10:** Smart App Control is not available, so no SAC setting is required.
> - **Codex App with a WSL engine:** use the WSL terminal command; no Windows security setting needs to change.
>
> See
> [installation, connection, and supported environments](docs/guides/setup.md#windows-powershell-terminal)
> for Windows 11 re-enable conditions and environments that remain blocked.

### Connect the programs you use

| Program | What to do after installation |
| --- | --- |
| **Claude Code** | Its hook is connected automatically. If work is already open, keep working. |
| **Codex CLI** | Open `/hooks` in the CLI you will use and confirm that all seven Memory Supervisor entries are **trusted and on**, then continue. Restart only another CLI that was already open before installation, after its current work finishes. |
| **Codex Desktop App** | In **Settings → Hooks**, trust and turn on all seven entries. Return to an existing conversation and send the next request you intended to send; no new conversation or App restart is required. If the entries are not visible yet, wait up to 60 seconds and reopen Settings. |

### Verify installation

```bash
memory-status --connections
```

- `Core daemon CONNECTED`: the background supervisor is healthy.
- `Claude Code CONNECTED`: the supported version and user hook are connected.
- `Codex CONNECTED`: all seven CLI hooks are installed, enabled, and trusted.
- `Codex App ACTIVE`: the App hooks are ready and a real call arrived from an existing or new task.
- `NOT DETECTED` is normal for a program you do not use.

If a line differs, act only on what it reports. Every exception and the exact live-install behavior
are in [installation, connection, and supported environments](docs/guides/setup.md).

## How it works in 30 seconds

Calando does not sit in front of Claude Code or Codex and execute commands on their behalf.
One small supervisor runs beside them in each operating-system environment, watching available
memory, decline speed, pressure signals, and growth from AI work. Hooks ask for the latest decision
immediately before new work begins.

```text
┌──────────────────────┐    memory / PID    ┌──────────────────────┐
│ OS environment       │ ─────────────────► │ Calando              │
└──────────────────────┘                    │ forecast / brake     │
                                            └──────────┬───────────┘
                                                       │ decision
┌──────────────────────┐      pre-run hook  ┌──────────▼───────────┐
│ Claude Code / Codex  │ ─────────────────► │ allow / hold         │
│ CLI / App thread     │ ◄── reason/state ─ │ explain / recover    │
└──────────────────────┘                    └──────────────────────┘
```

1. **Discover automatically** — Memory capacity, available memory, short and long decline rates,
   and the expected increase from the next task automatically determine the protection thresholds
   and stopping distance.
2. **Prefer performance** — Stable high use is left alone. A fast decline with plenty of headroom is
   observed while the supervisor checks whether real danger is getting close.
3. **Create a buffer with new work first** — Only near danger does it delay creation of new
   subagents, workflows, and tasks. If needed, it then narrows the next tools of one selected agent
   at a time.
4. **Use a reversible last resort** — Only when danger continues after every buffer and sustained
   growth is tied to one exact Claude Code or Codex process does it pause, rather than terminate,
   that process.
5. **Recover in reverse** — Work reopens one step at a time, and paused work resumes one process at
   a time after headroom stabilizes.

### CLI versus Codex Desktop App

| Claude Code and Codex CLI | Codex Desktop App |
| --- | --- |
| Terminal sessions and child processes are separate, so the causal process and control target can be connected relatively precisely. | Conversations are distinguished as **logical threads** inside one App Server, but memory is shared. They are not measured like independent CLI processes. |
| Hooks identify leads, subagents, and tools; the last resort pauses only one revalidated local PID. | Hooks control new work per conversation and correlate recent tools, subagents, activity times, and App Server growth. When attribution is uncertain, the supervisor does not pretend shared memory belongs to one thread; it buffers new work against the shared risk. Pausing the App Server remains an exceptionally rare last step after every gradual control and confirmed sustained growth. |

One supervisor runs in each Windows, WSL2, macOS, Linux, VM, or isolated-container environment.
When environments sharing the same physical memory are connected through federation, they decide
together when new work can start and when restrictions can be lifted, while each supervisor controls
only its own processes.

The complete stage policy, both architectures, and federation topology are preserved in
[how Calando works](docs/guides/how-it-works.md).

## Common commands

| Purpose | Command |
| --- | --- |
| Current memory and protection state | `memory-status` |
| Every connected environment | `memory-status --all` |
| Claude Code and Codex hook connections | `memory-status --connections` |
| Update the program and reconnect integrations | `memory-supervisor update` |
| Turn protection off or on in this environment | `memory-supervisor off` / `memory-supervisor on` |
| Show notification routes | `memory-supervisor notifications show` |

See [operations, notifications, and recovery](docs/guides/operations.md) for paused-work handling,
automatic recovery, manual resume, optional memory hard-cap configuration, and Discord and Telegram
notification setup.

## Supported environments and safety boundary

| Item | Support or boundary |
| --- | --- |
| **Operating systems** | Linux and WSL2 on 64-bit Intel/AMD, macOS on Apple Silicon and Intel, Windows 10 or 11 on 64-bit Intel/AMD |
| **AI programs** | Claude Code 2.1.217 or newer, Codex CLI 0.145.0 or newer, Codex Desktop App |
| **Resident memory** | Measured maximum 5.13 MiB across platforms; planning value below 10 MiB per installed supervisor |
| **Network** | Normal supervision sends no network traffic or usage telemetry. Network access occurs only for installation and updates, or for Discord and Telegram notifications explicitly enabled by the user. |
| **Never read** | Prompt, conversation, model response, project-file contents, process-memory contents, or Claude/ChatGPT credentials |
| **Never control** | Other programs such as browsers and IDEs, PIDs in another OS environment, or memory, swap, and VM settings |
| **Automatic physical action** | At most a reversible pause of one revalidated Claude Code or Codex work process. There is no automatic termination or kill. |

See [security](docs/guides/security.md) for the complete data and process boundary,
[performance](docs/guides/performance.md) for measurements, and
[installation, connection, and supported environments](docs/guides/setup.md) for platform-specific
conditions.

## Documentation

| Topic | Document |
| --- | --- |
| Installation, live-session connection, hook trust, Windows, WSL2, macOS, and Linux | [Installation, connection, and supported environments](docs/guides/setup.md) |
| Gradual braking, CLI and Codex App architecture, blind control, and federation | [How Memory Supervisor works](docs/guides/how-it-works.md) |
| Terminal, OS, Discord, and Telegram notifications, commands, pause, and recovery | [Operations, notifications, and recovery](docs/guides/operations.md) |
| Read the original detailed README continuously in one document | [Detailed guide](README.full.md) |
| Find security, performance, testing, and every specialist reference | [Documentation index](docs/README.md) |

## Verification

The project runs automated Rust tests, installation/update/uninstall E2E, hook-contract checks, repository
privacy-boundary checks, and Linux, Windows, and macOS platform validation. See
[test coverage](docs/testing/test-matrix.md) and
[adaptive stopping distance](docs/testing/stopping-distance.md) for the public verification scope.

See the [security policy](SECURITY.md) to report a vulnerability and
[contributing guide](CONTRIBUTING.md) to work on the project.

## License

[MIT](LICENSE)
