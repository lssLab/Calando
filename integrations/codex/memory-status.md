---
description: Show Memory Supervisor status across this host, WSL, VMs, and containers
---

Run `memory-status --all` (fall back to `memory-status` if it reports no federation instances).
Separate raw utilization from adaptive admission/action, then report headroom, tracked processes,
leak evidence, probation, `PAUSED_BY_SUPERVISOR`, and delivery results. For ORANGE/RED admission,
say that existing work continues while only new fan-out is held. Lead with the human action block,
including cause, automatic/manual recovery, and the exact command; never signal a process without
the user's explicit request.
