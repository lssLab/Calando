---
name: memory-supervisor
description: Check or troubleshoot Memory Supervisor when the user asks about memory pressure, OOM, a paused or frozen agent process, or Memory Supervisor setup and tuning. Routine `[memory-supervisor]` hook notices are self-contained and do not need this skill.
---

# Memory Supervisor

The daemon and hooks act automatically. An agent only reads state and adapts its own work.

## Check state (read-only)

```bash
memory-status                 # add --json for fields, --connections for hook/skill wiring
```

Report per environment (Windows, WSL, macOS, Linux are measured separately): free memory,
admission action, paused PIDs. Utilization can stay RED under a stable heavy workload; the
admission action is what limits new work.

## When a hook notice arrives

- `New heavy work held`: do not start new agents, workflows, or heavy builds; finish current work
  and retry after admission reopens. It is not a crash.
- `Process paused`: do not recreate or resume that worker; tell the user once.
- Anything else: no action needed.

## Owner-only commands

Run these only when the user explicitly asks in this conversation, and never from a sandboxed or
isolated agent, because they act on every session of this user:
`memory-supervisor resume|terminate|kill <pid>`, `on|off`, `update`, `budget ...`,
`notifications ...`.

Details for diagnosis, recovery, tuning, and platforms: `agents/reference.md` in this skill
directory (read only the section you need).
