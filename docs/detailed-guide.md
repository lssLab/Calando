<p align="center">
  <img src="../assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <strong>English</strong> · <a href="detailed-guide.ko.md">한국어</a>
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

## What problem does Calando solve?

During long-running work in Claude Code, Codex CLI, or Codex Desktop App,
subagents, builds, tests, and browser tools can pile up. If available memory
drops quickly, a CLI terminal can stop responding or exit; in the Desktop App,
multiple conversations sharing the App Server can be affected together. In
either case, pending results and work in progress can be interrupted.

Calando does not limit work simply because memory usage is high. In
both the CLI and Desktop App, it delays new work in stages only when real risk
gets close, while keeping in-flight work and result delivery running whenever
possible. This helps prevent a session from ending abruptly.

Protection does not jump from unrestricted work to a full stop. It moves up
one stage at a time as real risk gets closer, then unwinds in reverse after
recovery.

1. **Automatic setup** — Launch `claude` or `codex` as usual, or start a
   conversation in Codex Desktop App. Calando distinguishes CLI
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

Calando does not sit between you and either CLI. You continue to
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
4. If configured, Telegram receives one protection-start notice
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

### 1. Install Calando

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

- When Codex App on Windows uses a WSL engine, Calando installed in that
  WSL environment protects its WSL App Server, per-task logical threads, and
  WSL-side tools. This path does not run native Windows Calando, so it
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

The installer connects the Calando user hook automatically. There
is nothing to configure, approve, or enable in Claude Code.

**If Claude Code was already running during installation:** keep working.
Claude Code automatically reloads user-settings changes, so a restart is not
normally required.

**To verify:** confirm `Claude Code CONNECTED` in step 5's
`calando status` output. Open the read-only `/hooks` screen and
inspect `User Settings` only if you want to see the hook details. Restart
Claude Code after the current work only in the exceptional case where this
optional view does not show the entry.

### 3. Set up Codex CLI

1. Open `/hooks` in the Codex CLI you will use.
2. Confirm that all seven Calando hooks are **trusted and on**.
3. Trust entries marked for review and turn on any disabled entry.
4. Close `/hooks` and continue working.

**If Codex CLI was already running during installation:** continue in the CLI
you just checked; it does not need a restart. For any other Codex CLI that was
already open before installation, finish its current work and restart only
that CLI once.

### 4. Set up Codex Desktop App

1. Open Codex App and go to **Settings → Hooks**. If the Calando
   entries are not there yet, wait up to 60 seconds and reopen Settings.
2. Trust and turn on all seven Calando hooks. **Trust all** does not
   turn on a previously disabled switch, so check both states.
3. Return to an existing task and send the next request you intended to send.
   Create a new task only if there is no existing task to continue.

**If Codex App was already running during installation:** leave the App and its
existing tasks open and follow steps 1–3. You do not need to restart the App or
create a new task.

### 5. Verify the installation

```bash
calando status
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
  `calando update`, then repeat this check.
- `NEEDS ATTENTION`: satisfy the reported program-version or hook requirement,
  then run `calando update`.
- `Core daemon OFF`: run `calando on`.
- If all seven App hooks look correct but the App still does not become
  `ACTIVE` after a request, restart the App once, send the next request in an
  existing task, and check again.
- If a new installation cannot find the `calando` command, reopen only
  the terminal and run it again. Claude Code, Codex CLI, and Codex App do not
  need to restart for this PATH refresh.

Codex hook trust is not administrator access. It is your approval of the exact
local command that Codex will run. Check an administrator policy only if an
organization policy or Windows security policy blocks installation. See the
[Claude Code hooks guide](https://code.claude.com/docs/en/hooks) and
[Codex hooks guide](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)
for the underlying trust rules.

Upgrading from 0.2.x moves the old `memory-supervisor` folders to the `calando` names once (on Unix
the old names are left as links) and changes the hook commands, so Codex CLI and App users trust the
seven Calando hooks once more. After that, updates run automatically; see
[operations](guides/operations.md#update).

These commands install the latest public release. A Rust build tool is not
required; the verified executable included in that release is used automatically.

### 6. Uninstall

To remove Calando, run this once in each environment where it is installed:

```bash
calando uninstall
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

Calando does not notify on every memory reading. It sends one notice each time a real protection
action begins or fully recovers, or when a connection or protection condition needs attention.

| Route | Where it appears | Always on |
| --- | --- | --- |
| Hook context | The agent's next hook, before its next task | Yes |
| Terminal | The one-line message the hook shows, and a notice written to a paused process's terminal | Yes |
| Telegram | A bot's private chat or group chosen by the user | Optional |

Telegram is the only push route. Check state at any time with `calando status` or
`/calando status`. Set up Telegram with:

```bash
calando telegram
calando telegram off
```

`calando telegram` asks for the bot token in a hidden prompt, finds the chat, and sends a test
message. Do not put the token on the command line. See [notification setup](guides/notifications.md) for
the message format and troubleshooting.

## Terminal commands

Terminal commands act on the whole environment. Windows, each WSL distribution, each VM, and each
container are separate environments; run the command inside each one.

```text
calando status [--json]        this environment + CLI connections + every other environment
calando on | off               off first resumes everything Calando paused, then turns off
calando telegram [CHAT_ID]     asks for the bot token (hidden), finds the chat, sends a test
calando telegram off           removes the Telegram settings
calando cap [<GB> | off]       advanced hard cap for all agents here; explains and asks [y/N]
calando update                 update now
calando update auto on | off   automatic updates (default on)
calando uninstall              remove, keeping settings and state
calando lock on | off          administrator only; blocks off and uninstall while on
calando help
```

`memory-supervisor` and `memory-status` still work as aliases during 0.3.x.

### On and off

`calando off` first resumes every process Calando paused, then stops and disables the background
service. Installed hooks stay connected and pass through silently. The choice survives reboots and
updates; `calando on` restores protection. If the service stops without `off`, hooks discard its
stale decision after ten seconds and warn that **protection is unavailable**.

### Cap

`calando cap <GB>` sets an advanced hard cap for all Claude Code and Codex agents in this
environment. It is not recommended for normal use; it is meant for strict research or benchmark
runs. The command prints an explanation and asks `[y/N]`; non-interactive callers pass `--yes`.

- The value is checked against what this environment can use now.
- Near the cap, new heavy work is held. Above it, one growing subagent is paused.
- Pausing cannot reclaim memory already used, so use can briefly pass the cap.
- For byte-exact quotas, use cgroup, container, or VM limits.

`calando cap off` removes it.

### Update

The installed daemon checks the latest GitHub release once a day. It runs the update outside its own
service when nothing is paused and memory is calm, and retries hourly while busy.

- Claude Code: applied automatically, including hook changes. New sessions pick them up.
- Codex: a release that changes hook definitions (a minor version, for example 0.3 → 0.4) is not
  applied automatically while Codex is connected, because Codex skips changed hooks until the user
  trusts them. `calando status` and one Telegram message ask you to run `calando update` and trust
  the hooks in CLI `/hooks` or App **Settings → Hooks**. Patch releases never change hooks.
- With `calando update auto off`, `calando status`, `/calando status`, and one Telegram message per
  version say an update is available. Agent hooks never mention updates.
- The daily check also restores Calando's Claude Code hooks if Claude Code dropped them from
  `settings.json`.

Upgrading from 0.2.x to 0.3.0 changes hook commands, so Codex users must trust the Calando hooks once.

### Lock

`calando lock on` must be run as an administrator. It writes `/etc/calando/lock` (Windows
`%ProgramData%\Calando\lock`). While the file exists, terminal and session `off` and
`calando uninstall` refuse. `calando lock off` removes it. The lock is a policy file, not a sandbox;
see [security](guides/security.md).

## Session commands

These act only on the session that types them. The `UserPromptSubmit` hook answers them, so the prompt
never reaches the model and uses no tokens or context. The target is the session that typed it; no
PID or session list is involved.

```text
/calando on | off | status | help        (Codex CLI and App: $calando ...)
/calando cap <GB> | off                  advanced; apply with /calando cap <GB> yes
```

- **off** — the session is still measured but never held, paused, or notified.
- **cap** — limits that session's own process tree. The first reply explains; `yes` applies it. Near
  90% of the cap its heavy starts are held. Above the cap one growing subagent is paused and resumes
  when the session drops under 90%.
- **status** — what that session's agent sees, plus an update line when relevant.

The command comes from one installed skill named `calando`, copied into `~/.claude/skills`,
`~/.agents/skills`, and `~/.codex/skills`. See the [Claude Code guide](guides/usage-claude.md) and
[Codex guide](guides/usage-codex.md).

## Security

Calando reads operating-system memory and process information, plus session, agent, tool,
working-directory, and connection-state information and a command prefix supplied by Claude Code and
Codex hooks. It uses this information only to decide whether new work may start and to identify an
exact control target.

Automatic control stops at delaying future Claude Code or Codex work and, at the final protection
stage, pausing and resuming one verified local work process. It never terminates a program or
controls unrelated programs. Normal monitoring makes no external request; only installation,
updates, the daily update check, and an enabled Telegram route use the network.

It does not use prompts, conversation text, model responses, or file contents that may be present in a
hook payload for a control decision and does not retain them. It does not open project files or
process memory, or inspect or change browser or IDE data, Claude or ChatGPT credentials, or
operating-system kernel, memory, swap, and firewall settings. See
[security and data/control boundaries](guides/security.md) for the complete list.

## Control and recovery

There is no manual resume command. When memory becomes stable again, paused work resumes
automatically, one item at a time. A lead paused because its own memory kept growing gets one
guarded automatic resume so the supervisor can check the result. If the same growth returns, it
stays paused.

A lead pause is intentionally very rare. It is the **final protection stage**, used only when staged
new-work delays and subagent and tool controls have not removed the danger, and sustained growth from
that same lead has been confirmed.

If a lead stays paused, either:

- close that terminal and continue with the CLI's own resume, or
- run `calando off`, which resumes it in place.

```bash
claude --resume
codex resume
```

After a CLI resume, the session's first hook says its previous process was paused and ended, and that
subagent results from that run may be missing.

In the Codex App, the shared App Server is never left paused after a failed guarded resume; Calando
falls back to per-thread limits.

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
| [Security and data/control boundaries](guides/security.md) | Information read, stored, and shared, plus control limits and isolation |
| [Test coverage](testing/test-matrix.md) | Product behavior and platforms covered by public tests |
| [Claude Code](guides/usage-claude.md) / [Codex](guides/usage-codex.md) | CLI and Desktop App integration and session behavior |
| [Notifications](guides/notifications.md) | Terminal, hook context, and Telegram delivery |
| [Performance](guides/performance.md) | Background memory use and before-start check time |
| [Security policy](../.github/SECURITY.md) | Private vulnerability-reporting route |
| [Contributing](../.github/CONTRIBUTING.md) | Change principles and pre-submission checks |

## License

[MIT](../LICENSE)
