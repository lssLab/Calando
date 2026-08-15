<p align="center">
  <img src="../assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <strong>English</strong> · <a href="detailed-guide.ko.md">한국어</a> · <a href="detailed-guide.zh-CN.md">简体中文</a> · <a href="detailed-guide.ja.md">日本語</a>
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
  <a href="guides/setup.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="guides/performance.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="guides/security.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

## What problem does Memory Supervisor solve?

During long-running work in Claude Code, Codex CLI, or Codex Desktop App,
subagents, builds, tests, and browser tools can pile up. If available memory
drops quickly, a CLI terminal can stop responding or exit; in the Desktop App,
multiple conversations sharing the App Server can be affected together. In
either case, pending results and work in progress can be interrupted.

Memory Supervisor does not limit work simply because memory usage is high. In
both the CLI and Desktop App, it delays new work in stages only when real risk
gets close, while keeping in-flight work and result delivery running whenever
possible. This helps prevent a session from ending abruptly.

Protection does not jump from unrestricted work to a full stop. It moves up
one stage at a time as real risk gets closer, then unwinds in reverse after
recovery.

1. **Automatic setup** — Launch `claude` or `codex` as usual, or start a
   conversation in Codex Desktop App. Memory Supervisor distinguishes CLI
   sessions from App conversations automatically, reads total capacity,
   available memory, decline speed, and the buffer needed for upcoming work,
   then sets the protection level. There is no budget to configure or status to
   keep checking.
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
   workflows, and tasks is blocked first. Only when reliable evidence attributes
   the loss wholly or partly to AI work, or an optional user-set cap is exceeded,
   does future work for existing agents narrow through `all work → no new
   subagents, workflows, or tasks → no
   new memory-heavy starts such as builds and tests → handoff, coordination,
   status, stop, recovery, and small reads only`.

   Subagents are not restricted all at once. With enough time, one subagent moves
   down one rung on its next tool call; with less time, the supervisor applies
   the minimum batch needed to finish the ladder before the reserve, then
   remeasures.
   Unselected agents and running work stay unchanged. Subagents are selected for
   restriction in this order: (1) verified abnormal growth in the linked process,
   (2) a current or recent tool for agent, workflow, or task creation or heavy
   work, (3) a tighter current state, (4) a shorter time for the linked process
   to reach the reserve, and (5) a newer start.

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
terminal sessions and Codex Desktop App conversations while sustaining as much
useful performance as possible.

## How does it work?

Memory Supervisor does not sit between you and either CLI. You continue to
launch `claude` and `codex` normally while a small background program watches
their memory use.

1. One background monitor runs in each **operating-system environment**. A
   Windows, macOS, or Linux base system is one environment, and each WSL
   distribution, virtual machine, or isolated container is another. If you use
   Windows and WSL together, one monitor runs in Windows and another in WSL.
   Each monitor measures available memory, native pressure signals, short- and
   long-window decline speed, expected near-term growth, and visible Claude
   Code and Codex programs. Multiple terminals in one environment share the
   same monitor and new-work decision.
2. It distinguishes terminals through the process table and hooks, not window
   titles. Each top-level Claude Code or Codex process is an independent lead;
   descendants are grouped as workers and tools. Hook session and agent IDs are
   recorded with the PID and process start identity so a different session or a
   reused PID is not controlled by mistake.
3. Instead of a fixed utilization percentage, it calculates stopping distance
   from current room and consumption speed. Slow decline brakes closer to the
   boundary; fast decline leaves the longer distance needed to stop within the
   same reaction time.
4. A Claude Code or Codex hook checks before a new subagent, workflow, task, or
   memory-heavy command starts. `ALLOW` permits it, `OBSERVE` permits it while
   watching, `HOLD` delays only creation of new subagents, workflows, and tasks,
   and `DRAIN` blocks those creation requests.
5. Each Windows, Linux, and macOS base OS runs its own Supervisor, as does every
   WSL distribution, VM, or process-isolated container layered on top. Environments
   sharing physical memory use federation to exchange only new-work decisions
   that are at most 10 seconds old, applying the strictest one. Each Supervisor
   still controls only its own hooks and PID space, so it cannot pause a program
   in another environment.
6. Even under `DRAIN`, pressure caused only by Chrome or an IDE never pauses
   existing AI work. For AI-attributed pressure or an operator-set memory cap,
   future work narrows through `ACTIVE → NO_EXPANSION → LIGHT_WORK_ONLY →
   HANDOFF_ONLY`. It does not narrow every session at once. Subagents are ranked
   by verified sustained growth in the linked process, current or recent
   expansion/build/test work, whether restriction has already started, earlier
   arrival at the risk boundary, and newer start time. Each tick applies only
   the minimum set of ladder steps required by the remaining stopping distance;
   unselected sessions and in-flight work remain unchanged. A lead moves first
   only when there is evidence that starting with subagents would be too late.
7. If danger still remains, the supervisor rechecks the PID (the operating
   system's process number) and start identity, then pauses at most one local
   program. A lead can pause only when its exact terminal is writable; a failed
   notice immediately rolls the pause back.

After sustained improvement, agent capabilities reopen in the opposite order
and paused programs resume one at a time. The exact calculation and physical
measurements live in the
[adaptive stopping distance](testing/stopping-distance.md).

The `GREEN` through `RED` color is a quick status display. New work is actually
controlled by `ALLOW`, `OBSERVE`, `HOLD`, or `DRAIN`; color alone never pauses
a program.

### What changes in Codex Desktop App?

In a CLI, each session has its own lead process and child-process tree, so the
supervisor can usually tell which session is growing. Codex Desktop App does not
merge all conversations into one session. Instead, the App Server keeps each
conversation as a **logical thread** with its own `session_id`. Here, a logical
thread is not an operating-system thread. It is the conversation identity used
by the App Server and the supervisor. The supervisor treats each logical thread
as an independent lead and uses `agent_id` to attach subagents to that lead.
Agent lists, next-hook work scope, actions, and recovery notices can therefore
be managed per conversation.

A logical App thread is not physically equivalent to a CLI session. A CLI
session has its own lead PID, descendant process tree, and terminal. An App
logical thread has no independent lead PID, complete child-process tree,
terminal, or dedicated memory total. All logical threads share one App Server
PID and its internal memory. The operating system therefore shows one App
Server total, cannot measure memory per conversation, and cannot pause just one
conversation. In short, conversations are logically separate, while process and
memory are physically shared.

The supervisor counts shared App Server memory once. A separately launched tool
process belongs to a specific logical thread only when the hook, the process
list captured before the task, the parent-child chain, and the PID start
identity all agree. Memory used inside the App Server, or a child launched while
tools from several conversations overlap, may not have a provable logical-thread
owner. This is **blind control**. It does not mean that the supervisor sees
nothing: it still knows system headroom and decline speed, App and child-process
growth, active conversations, and current tool types. Only the owner of some
growth is unknown.

Within that limit, the App controller preserves the CLI policy in this order:

1. **Keep performance first.** High but stable use, or App growth that fails to
   explain the system's loss of headroom, does not trigger App-attributed
   conversation limits. The controller compares time left before risk with the
   time needed to brake, then waits until the latest safe point. When a larger
   share of growth has no provable conversation owner, the controller adds only
   the time needed to try candidates one by one and measure the result;
   uncertainty alone does not cause early throttling.
2. **Cushion new high-memory starts first.** If sustained App growth is causing
   the risk and enters the calculated stopping distance, but its conversation
   owner is unclear, only future high-memory App starts such as builds and tests
   wait across the App. Running work, results, messages, status, and recovery
   remain available.
3. **Narrow only the smallest set that explains the risk.** When growth has an
   exact owner, the controller selects only the minimum conversations needed to
   explain it and normally moves each one's future-work scope down by one stage.
   When ownership is unclear, current heavy tools, subagent role, and recent
   activity rank the candidates. It narrows the first blind candidate, then
   remeasures, stopping when growth slows and moving to another only if danger
   continues. If too little time remains, it still batches only the minimum set
   needed before the risk boundary. Estimated evidence can rank candidates; it
   never grants authority to pause a conversation-specific process.
4. **Use the smallest physical brake available.** After every smaller logical
   action has failed, the order is one exactly owned and still-growing child
   process, then one still-growing child known to belong to the App but not to a
   particular conversation, and finally the shared App Server. The server can
   pause only after every active conversation and subagent has acknowledged the
   final stage, where only light work such as result delivery, status, and
   recovery remains. Server growth itself must remain the dominant cause, and
   no smaller option can be left. An independent recovery guard resumes that
   exact server after a bounded delay. The pause does not free memory
   immediately; it stops further growth so other work can finish and the system
   can recover.

Recovery reopens one stage at a time in reverse. Each affected conversation
receives the reason and current scope at its next hook; a blind child or shared
server brake is reported to every active conversation it can affect. If the App
hook route disconnects, the supervisor does not pretend that new
per-conversation limits or physical brakes have been enforced. It stops counting
them as the next available protection, reports the degraded state, and continues
system-wide decisions about starting new work and App process monitoring.

Running the CLI and the App at the same time does not make either one disappear
from supervision. Within one PID space—such as one OS, WSL distribution, or
VM—a single supervisor watches both CLI process trees and the Codex App Server.
It does not merge them into one session.

- Each CLI session remains an **independent lead with its own terminal, lead PID,
  and descendant process tree**.
- The App Server is not treated as one terminal. It is **one physical process
  host shared by several conversations**. Each `session_id` below it is a
  separate logical lead, while the server PID and its internal memory are
  counted only once.

Both surfaces share the same local memory assessment and new-work admission
decision, but their control targets stay separate. An App-attributed blind
cushion applies only to App hook calls and does not also block ordinary CLI
requests. Memory from either surface contributes to machine risk, but growth on
one surface does not automatically make the other a brake target. Each target
still needs its own growth and attribution evidence.

Federation joins **supervisor instances** that compete for the same physical
memory; it does not merge App conversations or terminals. Windows, each WSL
distribution, a dynamic-memory VM, and a process-isolated container use only
new-work decisions no more than 10 seconds old and apply the strictest fresh
one. They do not merge another instance's conversation list or PIDs into local
control, and each supervisor controls only the CLI and App processes in its own
PID space. For example, `DRAIN` caused by an App in WSL can make a Windows CLI
delay a new subagent or large task through federation, but the WSL supervisor
cannot pause that Windows CLI and the Windows supervisor cannot pause the WSL
App Server.

## How are terminals and agents controlled?

### 1. Claude Code and Codex CLI

In the CLI path, Claude Code and Codex remain attached directly to the terminal.
The background monitor watches related programs in the same local process
space. Control is split into two layers:

- Before-start check: allow or delay work that has not started yet.
- Program pause: if danger continues, pause only one verified PID through the
  operating system.

```text
A. User work path

                ┌──────────────────────┐
                │ Exact user terminal  │
                │ Commands and results │
                └──────────┬───────────┘
                           │ direct attachment
                           ▼
                ┌──────────────────────┐
                │ Claude / Codex lead  │
                │ Main agent           │
                └──────────┬───────────┘
                           │ before supported actions
                           ▼
                ┌──────────────────────┐
                │ Before-tool hook     │
                │ Reads latest decision│
                │ Returns reason       │
                └──────────┬───────────┘
                           │ decision
           ┌───────────────┴───────────────┐
           ▼                               ▼
┌──────────────────────┐        ┌──────────────────────┐
│ ALLOW / OBSERVE      │        │ HOLD / DRAIN         │
│ Requested work runs  │        │ Targeted work waits  │
│ No start is delayed  │        │ In-flight work stays │
└──────────────────────┘        └──────────────────────┘

B. Background protection path

┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ Measure/brake/recover│                │ Latest hook decision │
└──────────────────────┘                └──────────┬───────────┘                └──────────────────────┘
                                                   │ when protection acts
                               ┌───────────────────┴───────────────────┐
                               ▼                                       ▼
                    ┌──────────────────────┐                ┌──────────────────────┐
                    │ Notice + lead context│                │ One verified PID     │
                    │ Exact terminal: now  │                │ Final stage only     │
                    │ Lead: next hook once │                │ Pause + auto-resume  │
                    └──────────────────────┘                └──────────────────────┘

Windows, Linux, and macOS hosts with independent environments layered on top

                         ┌────────────────────────────────────┐
                         │ Shared federation decision         │
                         │ Shares new-work decisions only     │
                         │ Valid for 10 seconds               │
                         │ Strictest fresh decision wins      │
                         └─────────────────┬──────────────────┘
                                           ↕
                         only boundaries competing for shared RAM connect

       ┌────────────────────────────┐  ┌────────────────────────────┐  ┌────────────────────────────┐
       │ WSL distro / VM / container│  │ VM / container             │  │ VM / container             │
       │ each: local Supervisor     │  │ local Supervisor           │  │ local Supervisor           │
       └──────────────▲─────────────┘  └──────────────▲─────────────┘  └──────────────▲─────────────┘
                      │ runs on                       │ runs on                       │ runs on
       ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐
       │ Windows base OS            │  │ Linux base OS              │  │ macOS base OS              │
       │ host Supervisor            │  │ host Supervisor            │  │ host Supervisor            │
       └────────────────────────────┘  └────────────────────────────┘  └────────────────────────────┘

                  Each Supervisor controls only its own state, hooks, and PID space
                              No RAM pooling · no cross-environment PID control
```

CLI lead awareness follows a fixed order:

1. The supervisor first records the cause, target, active restriction, and
   recovery path in its incident ledger.
2. A hook that delays work returns the reason in that same call.
3. A physical process action is shown in the exact terminal immediately.
   Worker incidents without a separate terminal are delivered once at the
   lead's next real hook.
4. If selected, OS, Discord, and Telegram receive one protection-start notice
   and one fully-recovered notice.

For example, suppose a Claude lead on Windows is packaging edits while Codex in
WSL is about to start a subagent and a large test, and their shared physical
memory headroom begins falling quickly. When the WSL supervisor records
`DRAIN`, federation carries that decision to Windows. Both hooks delay only the
new subagent and test; edits, results, and messages stay open. If an external
VM is the cause, no AI PID is paused. New work reopens after sustained
recovery. Only verified growth from the same AI worker can lead to graded
logical restriction and, last, an exact local PID pause.

See [architecture and runtime topology](guides/architecture.md) for the complete
state flow, multi-terminal layout, and failure boundaries.

### 2. Codex Desktop App

In Codex Desktop App, each conversation is one logical thread identified by its
`session_id`. Different session IDs are independent leads; opening the same
conversation in more than one window still counts as one logical thread and one
lead. This lets the supervisor manage hook-level work scope and notices per
conversation. It does not create a separate PID or memory pool for each thread:
they all share one App Server. The diagram shows how the logical conversation
and agent ledger is combined with physical process and memory observations,
while keeping exact ownership separate from blind candidates.

```text
                                        ┌──────────────────────┐
                                        │ Codex Desktop App    │
                                        │ Logical App threads  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Shared App Server    │
                                        │ One PID + shared RAM │
                                        └──────────┬───────────┘
                                                   │ hooks + process view
                           ┌───────────────────────┴───────────────────────┐
                           ▼                                               ▼
                ┌──────────────────────┐                        ┌──────────────────────┐
                │ Conversation ledger  │                        │ Process + memory map │
                │ session ID = lead    │                        │ exact / blind pool   │
                │ agent ID = subagent  │                        │ Shared RAM once      │
                └──────────┬───────────┘                        └──────────┬───────────┘
                           └───────────────────────┬───────────────────────┘
                                                   ▼
┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ App-specific planner │                │ Hook-confirmed stage │
│ Sustained App growth │                │ Cause + braking room │                │ Recovery + notice    │
└──────────────────────┘                └──────────┬───────────┘                └──────────┬───────────┘
                                                   │                                       │
                                                   ▼                                       ▼
                                        ┌──────────────────────┐                ┌──────────────────────┐
                                        │ App staged cushion   │                │ Affected lead context│
                                        │ New heavy starts wait│                │ Scope + recovery     │
                                        │ Chosen sessions only │                └──────────────────────┘
                                        └──────────┬───────────┘
                                                   │ if danger persists
                                                   ▼
                                        ┌──────────────────────┐
                                        │ One subprocess PID   │
                                        │ Exact owner first    │
                                        │ Blind: one-by-one    │
                                        └──────────┬───────────┘
                                                   │ absolute last stage
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Final server brake   │
                                        │ All App work pauses  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Independent recovery │
                                        │ Timed auto-resume    │
                                        └──────────────────────┘
```

Suppose conversation A starts a build while conversation B is preparing an
answer. If the build process is exactly linked to A's hook, the supervisor
narrows A's new work first and leaves B alone. If danger persists, that build
process—not the shared server—is the first possible physical brake.

If the process cannot be proven to belong to A or B, the supervisor does not
arbitrarily blame B. It first holds only new high-memory starts across the App,
then narrows the future work of one conversation that is running heavy work or
best matches the observed growth. After a short window to measure the action's
effect, it checks whether memory decline slowed and adds another candidate only
when there was no useful change. This serial investigation time is included in
the App stopping distance from the start, so candidate checks can finish before
the risk boundary without reducing performance unnecessarily early.

The mechanism is different from the CLI, but the policy outcome is the same:
reduce new work before in-flight results, prefer subagents over leads, control
only the smallest set that explains the risk, and apply physical brakes and
recovery one target at a time. An exactly owned child comes first. A blind child
can pause only after every related conversation and subagent has actually
received its final logical stage. Pausing the shared App Server—and therefore
every conversation—is the last resort after all smaller options are exhausted.
Federation boundaries between a base OS, WSL, VMs, and containers remain the
same as in the CLI design, as does the rule that each supervisor controls only
its own PID space. See [Codex Desktop App](guides/codex-app.md) for the full
safety conditions.

## Installation

Open the **terminal** for your environment and paste the matching one-line
command below. There is no Git, Python, Rust, or separate installer to prepare.
A normal installation is scoped to your user account and does not require
`sudo` or an administrator shell.

### 1. Install Memory Supervisor

#### Linux, WSL2, or macOS terminal

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

When the command finishes, the background service is running and the detected
Claude Code and Codex hooks are connected automatically. It does not close a
running AI program or interrupt work in progress.

#### Windows PowerShell terminal

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

When the command finishes, the background service is running and the detected
Claude Code and Codex hooks are connected automatically. It does not close a
running AI program or interrupt work in progress.

> [!IMPORTANT]
> The Windows executable is currently under review by [SignPath Foundation](https://signpath.org/),
> so Windows 11 requires **Windows Security → App & browser control → Smart App Control** to remain
> `Off` while installing and using the native build until the review is complete.

| Windows state | Can the native build be installed? |
| --- | --- |
| 64-bit Windows 10 | Yes. Smart App Control is not available, so no SAC setting is required. If SmartScreen appears, verify that the download came from this repository's release. |
| Windows 11 24H2 build 26100.8117 or later, 25H2 build 26200.8117 or later, or a newer Windows 11 build where the re-enable control is visible | Yes, after setting Smart App Control to `Off`. You can turn it back on from the same Settings page after you stop using the unsigned build. |
| An older Windows 11 build, or a current build that has not yet received the gradually rolled-out control | Yes after turning it off, but turning it back on may require a Windows reset or reinstall. Check this before disabling it. |
| Smart App Control is already `Off` | Install without changing it. If the separate SmartScreen download-reputation prompt appears, verify the publisher and file source. |
| Windows 11 in S mode or an organization App Control policy that blocks the executable | Not supported by the native Windows path. This installer cannot bypass S mode or an administrator policy. |

Run `winver` from `Win + R` to check the Windows version and build. Before turning Smart App
Control off, also confirm whether its Settings page offers a way to turn it back on; rollout is
per device. See Microsoft's
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)
and [initial rollout notes](https://support.microsoft.com/en-au/help/5079391) for the source criteria.

Install for the environment where the **App Server and its tools actually run**,
not the operating system where the Codex App window is displayed.

- When Codex App on Windows uses a WSL engine, the Supervisor installed in that
  WSL environment protects its WSL App Server, per-task logical threads, and
  WSL-side tools. This path does not run the native Windows Supervisor, so it
  does not require turning off Smart App Control. The Windows App UI process and
  any separate Windows-native Claude Code or Codex CLI remain outside that WSL
  installation's measurement and control boundary.
- When the App Server or CLI runs directly on Windows, macOS, or Linux, install
  in that operating system. A native Windows installation uses the Smart App
  Control requirements above.
- When the App Server or CLI runs in another WSL distribution, virtual machine,
  or isolated container, install in each such environment. Windows and WSL find
  their federation path automatically; macOS or Linux hosts, dynamic-memory
  VMs, and containers connect a shared folder on the same machine. Once
  connected, environments competing for the same physical memory share
  new-work decisions. Fixed-memory VMs and other computers or cloud servers
  protect themselves independently. See
  [platform and multi-environment behavior](guides/platforms.md) for the boundary.

### 2. Set up Claude Code

The installer connects the Memory Supervisor user hook automatically. There
is nothing to configure, approve, or enable in Claude Code.

**If Claude Code was already running during installation:** keep working.
Claude Code automatically reloads user-settings changes, so a restart is not
normally required.

**To verify:** confirm `Claude Code CONNECTED` in step 5's
`memory-status --connections` output. Open the read-only `/hooks` screen and
inspect `User Settings` only if you want to see the hook details. Restart
Claude Code after the current work only in the exceptional case where this
optional view does not show the entry.

### 3. Set up Codex CLI

1. Open `/hooks` in the Codex CLI you will use.
2. Confirm that all seven Memory Supervisor hooks are **trusted and on**.
3. Trust entries marked for review and turn on any disabled entry.
4. Close `/hooks` and continue working.

**If Codex CLI was already running during installation:** continue in the CLI
you just checked; it does not need a restart. For any other Codex CLI that was
already open before installation, finish its current work and restart only
that CLI once.

### 4. Set up Codex Desktop App

1. Open Codex App and go to **Settings → Hooks**. If the Memory Supervisor
   entries are not there yet, wait up to 60 seconds and reopen Settings.
2. Trust and turn on all seven Memory Supervisor hooks. **Trust all** does not
   turn on a previously disabled switch, so check both states.
3. Return to an existing task and send the next request you intended to send.
   Create a new task only if there is no existing task to continue.

**If Codex App was already running during installation:** leave the App and its
existing tasks open and follow steps 1–3. You do not need to restart the App or
create a new task.

### 5. Verify the installation

```bash
memory-status --connections
```

Check the lines for the programs you use:

- `Core daemon CONNECTED`: the background service is healthy.
- `Claude Code CONNECTED`: the supported version and user hook are connected.
- `Codex CONNECTED`: all seven CLI hooks are installed, enabled, and trusted.
- `Codex App ACTIVE`: all seven App hooks are ready and a real call arrived
  from an existing or new task.
- `NOT DETECTED` is normal for a program you do not use or have not installed.

If a line is not healthy, act only on what it reports:

- `disabled` or `not trusted`: use `/hooks` in Codex CLI or **Settings →
  Hooks** in Codex App to trust and enable the named entry.
- `missing`, `stale`, `DEGRADED`, or `NOT RUNNING`: run
  `memory-supervisor update`, then repeat this check.
- `NEEDS ATTENTION`: satisfy the reported program-version or hook requirement,
  then run `memory-supervisor update`.
- `Core daemon OFF`: run `memory-supervisor on`.
- If all seven App hooks look correct but the App still does not become
  `ACTIVE` after a request, restart the App once, send the next request in an
  existing task, and check again.
- If a new installation cannot find the `memory-status` command, reopen only
  the terminal and run it again. Claude Code, Codex CLI, and Codex App do not
  need to restart for this PATH refresh.

Codex hook trust is not administrator access. It is your approval of the exact
local command that Codex will run. Check an administrator policy only if an
organization policy or Windows security policy blocks installation. See the
[Claude Code hooks guide](https://code.claude.com/docs/en/hooks) and
[Codex hooks guide](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)
for the underlying trust rules.

These commands install the latest public release. A Rust build tool is not
required; the verified executable included in that release is used automatically.

### 6. Uninstall

To remove Calando, run this once in each environment where it is installed:

```bash
memory-supervisor uninstall
```

It removes the background service, executable, and Calando-owned hook and skill connections while
preserving state and user settings.

## Supported environments

Protection behaves the same in every supported environment. The supervisor
watches available memory and its rate of decline, narrows new work in stages,
pauses one verified Claude Code or Codex process only if danger remains, and
resumes it after stable recovery. Only the operating-system mechanism used to
read memory and pause a process differs.

| Environment | Test coverage |
| --- | --- |
| Linux and WSL2 on 64-bit Intel/AMD | Physical WSL2 and automated Linux checks |
| macOS Apple Silicon | Automated Apple Silicon checks |
| Windows 10 or 11 on 64-bit Intel/AMD | Physical Windows 11 E2E, automated Windows Server 2022 checks, and Windows 10 runtime/API compatibility review |
| Intel-based macOS | Automated compatibility under Rosetta |

Connected products are Claude Code 2.1.217 or newer, Codex CLI 0.145.0 or newer
with `hooks stable true`, and Codex Desktop App. The same protection policy
applies to the CLI and App.

### Measured resident memory

These operating-system totals were measured in 20 samples at 0.2-second
intervals after warm-up.

| Test environment | Minimum | Mean | Maximum | OS metric |
| --- | ---: | ---: | ---: | --- |
| WSL2 Linux, physical service | 4.88 MiB | 4.88 MiB | 4.88 MiB | RSS |
| Ubuntu on 64-bit Intel/AMD, automated test | 3.50 MiB | 3.52 MiB | 3.54 MiB | RSS |
| Windows on 64-bit Intel/AMD, automated test | 4.15 MiB | 4.20 MiB | 4.25 MiB | Working set |
| macOS Apple Silicon, automated test | 3.38 MiB | 4.35 MiB | 5.13 MiB | RSS |

For capacity planning, use **10 MiB per installed monitor**, not the smallest
sample. See [performance measurements](guides/performance.md) for the detailed
conditions and raw data.

When one physical computer has several execution environments—Windows, WSL
distributions, virtual machines, or isolated containers—install it in every
environment that runs Claude Code or Codex. Multiple terminals in one
environment share one monitor. After installation and federation-path setup in
each environment, the whole computer automatically shares the latest new-work
decision regardless of how many kernels it runs. Each monitor still measures
and controls only its own environment, so it never operates on another
environment's PID. The installer connects the same local shared folder for
Windows and WSL; a VM or container uses a host-shared local folder as its
federation path. Network folders are not used to connect a different physical
computer or cloud server. See
[platform and multi-environment behavior](guides/platforms.md) for setup details.

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
[notification setup](guides/notifications.md) for route selection and removal,
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
`memory-status --connections`. See the [Claude Code guide](guides/usage-claude.md)
and [Codex guide](guides/usage-codex.md) for the detailed differences.

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
[security and data/control boundaries](guides/security.md) for the complete list
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
plus deterministic simulation. See [Test coverage](testing/test-matrix.md).

## Documentation

| Guide | Use it for |
| --- | --- |
| [All documentation](README.md) | Find installation, usage, security-boundary, and public test documents |
| [Architecture](guides/architecture.md) | Background monitoring, before-start checks, state files, and program control |
| [Codex Desktop App](guides/codex-app.md) | Logical conversations, blind control, and recovery inside the shared App Server |
| [Adaptive stopping distance](testing/stopping-distance.md) | Calculation, measured boundaries, gradual braking, and recovery |
| [Platforms and multi-environment behavior](guides/platforms.md) | How operating systems and virtual environments share new-work decisions |
| [Security and data/control boundaries](guides/security.md) | Information read, stored, and shared, plus automatic and manual control limits |
| [Test coverage](testing/test-matrix.md) | Product behavior and platforms covered by public tests |
| [Claude Code](guides/usage-claude.md) / [Codex](guides/usage-codex.md) | CLI and Desktop App integration and session behavior |
| [Notifications](guides/notifications.md) | Terminal, OS, Discord, and Telegram delivery |
| [Performance](guides/performance.md) | Background memory use and before-start check time |
| [Security policy](../.github/SECURITY.md) | Private vulnerability-reporting route |
| [Contributing](../.github/CONTRIBUTING.md) | Change principles and pre-submission checks |

## License

[MIT](../LICENSE)
