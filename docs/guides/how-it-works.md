# How Memory Supervisor works

<p align="center">
  <strong>English</strong> · <a href="how-it-works.ko.md">한국어</a>
</p>

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
[adaptive stopping distance](../testing/stopping-distance.md).

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

See [architecture and runtime topology](architecture.md) for the complete
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
its own PID space. See [Codex Desktop App](codex-app.md) for the full
safety conditions.
