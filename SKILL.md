---
name: calando
description: "Calando for this session: on | off | status | help | cap (advanced). Also use to check or troubleshoot Calando when the user asks about memory pressure, OOM, a paused or frozen agent process, or Calando setup. Routine `[calando]` hook notices are self-contained and do not need this skill."
---

# Calando

If the user typed `/calando ...` or `$calando ...` and you are reading this, Calando's hook did not
answer it: tell the user in one sentence to run `calando status` in a terminal, and stop.

The daemon and hooks act automatically. An agent only reads state and adapts its own work.

## Check state (read-only)

```bash
calando status          # add --json for fields
```

Report per environment (Windows, WSL, macOS, Linux are measured separately): free memory,
admission action, paused PIDs. Utilization can stay RED under a stable heavy workload; the
admission action is what limits new work.

## When a hook notice arrives

- `New heavy work held`: do not start new agents, workflows, or heavy builds; finish current work
  and retry after admission reopens. It is not a crash.
- `Process paused`: do not recreate or resume that worker; tell the user once.
- Anything else: no action needed.

## Owner commands

Run these only when the user explicitly asks in this conversation, and never from a sandboxed or
isolated agent, because they act on every session of this user: `calando on|off`, `update`,
`cap ...`, `telegram ...`. `calando cap` asks for confirmation; add `--yes` only after the user
confirmed the explanation it prints.

Details for diagnosis, recovery, tuning, and platforms: `agents/reference.md` in this skill
directory (read only the section you need).
