# Codex Desktop App supervision

<p align="center">
  <strong>English</strong> · <a href="codex-app.ko.md">한국어</a>
</p>

Codex Desktop App uses the same protection policy as Codex CLI, but its process structure is
different. This document explains what can be measured exactly, where control can be targeted, and
how the supervisor stays conservative when App Server does not expose enough ownership evidence.

## CLI and Desktop App are not the same process model

A CLI session normally has a lead process and a process tree that can be measured and revalidated
as one local execution unit. The Desktop App normally routes several conversations through one
App Server process.

Each conversation has a logical thread identifier and is managed as an independent lead for hook
decisions. It is not an independent operating-system process:

```text
Codex Desktop App
        │
        ▼
shared App Server process and shared memory
        │
        ├── logical thread A ── agents and tools
        ├── logical thread B ── agents and tools
        └── unassigned App work ── ownership not yet proven
```

Opening the same conversation in another window does not create another logical lead. Opening a
different conversation creates another logical thread, but both may still share the same App
Server memory.

## What the supervisor observes

The local daemon detects the App Server and records its memory once. Hooks then report lifecycle
boundaries for each logical thread, agent, and tool. Child processes are linked to a thread only
when process ancestry, start identity, hook timing, and thread evidence agree.

The supervisor therefore maintains three ownership levels:

| Level | Evidence | Permitted control |
| --- | --- | --- |
| Exact thread child | The child and logical thread are both revalidated | Thread-specific logical limits; final local PID pause if every guard passes |
| App child, thread uncertain | The child belongs to the App Server but not reliably to one thread | Candidate investigation and the smallest reversible blind action only at the final tier |
| Shared App Server | Several threads and shared memory use one host process | Observation and accounting; host pause only as the last protection stage |

Prompt text, conversation content, model responses, and edited file contents are not used to infer
ownership.

## The same performance policy, adapted to shared memory

The App path keeps the CLI policy goals:

- high but stable memory use does not trigger a restriction;
- a fast change with ample stopping distance does not trigger an immediate action;
- action begins only when current headroom, sustained rate, native pressure, and time to the
  recovery reserve show that danger is approaching;
- new expansion closes before in-progress editing, responses, and result delivery;
- restrictions open again gradually after stable recovery.

App Server memory is counted once. The daemon does not divide its entire resident set equally among
open conversations. A thread receives only growth that can be supported by its hook and child
activity; the remaining shared amount stays in an unassigned pool. This prevents an idle thread
from being blamed merely because another thread or the shared host grew.

## Control ladder

The supervisor reuses the CLI policy order while changing the actuator to fit App Server:

1. **Observe.** Keep all work open while the trajectory remains recoverable.
2. **Delay new expansion.** Near the calculated boundary, hold only new agents, heavy tools, builds,
   and tests. Existing output and result delivery continue.
3. **Narrow one logical target at a time.** When one thread is supported as the cause, reduce that
   thread's future work in small steps and remeasure after every step.
4. **Select one child candidate.** Prefer a currently growing, heavy, recent child with the strongest
   ownership evidence. Unselected threads, agents, and running work remain unchanged.
5. **Pause one verified child if necessary.** This is a reversible final backstop after logical
   controls are insufficient and the danger remains imminent.
6. **Use blind App control only when exact ownership is unavailable.** Investigate the candidate
   pool, act on at most one qualifying App child, remeasure, and stop immediately if the trajectory
   improves.
7. **Pause the shared App Server only as the last protection stage.** This affects every conversation
   hosted by that process, so it requires sustained App-attributed growth, imminent danger, failed
   lower tiers, durable action recording, and a usable notification route.

A single ordinary conversation does not skip this ladder. Session count is not a reason to brake,
and one light thread under a stable trajectory remains unrestricted.

## Blind control and its limit

App Server does not always expose a reliable operating-system PID for every logical agent or tool.
When ownership is incomplete, the supervisor does not pretend that a known thread owns all shared
memory. It narrows candidates using process ancestry, creation time, recent hook activity, memory
growth, workload class, prior actions, and App Server generation.

Blind control can stop additional growth from a selected App child, but it cannot guarantee which
logical conversation will feel that pause. For that reason it is later and more conservative than
exact thread control. If no candidate satisfies the action guards, the supervisor keeps the action
at admission control instead of signaling a guessed PID.

## Lead awareness, notifications, and recovery

The terminal and enabled OS or remote routes receive a notice when a material protection action
starts and when it fully recovers. The affected logical lead receives the same reason and current
recovery state at its next hook boundary.

Pressure-paused children resume one at a time after headroom stabilizes. A lead or shared App Server
paused for sustained self-growth receives one guarded probation resume. If the same growth returns,
the supervisor pauses it again and waits for the owner rather than repeating automatic resumes.

If a hook is disabled, untrusted, or routed through a stale path, the daemon can still observe
system and process memory but loses part of its logical control surface. It reports degraded
protection and tells the user to review Codex App **Settings → Hooks**. It does not silently replace
missing thread evidence with broad process control.

## Multiple windows, App Server generations, and federation

All windows connected to one App Server are one local App surface. Memory is counted once and each
logical thread is deduplicated by its identifier. If the App Server restarts, the new process start
identity creates a new generation so stale ownership from the old process cannot authorize a
signal.

Two App Server processes concurrently claiming the same logical thread are treated as an abnormal
collision. The supervisor does not merge their physical ownership or perform exact thread control
until the ambiguity disappears.

Codex CLI and Codex Desktop App running in the same kernel are observed by the same local daemon.
CLI process trees and the shared App Server are separate local surfaces. If the computer also runs
WSL2, containers, or a dynamic guest, federation shares the aggregate new-work decision across
kernels; it never exports App thread control or PID authority to another environment.

See [architecture](architecture.md), [Codex setup](usage-codex.md), and the
[test matrix](../testing/test-matrix.md).
