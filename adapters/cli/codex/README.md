# Codex adapter

Prefer the top-level one-line installer, then use `memory-supervisor update` for upgrades or a Codex
installation added later. Both paths merge the adapter atomically and preserve unrelated hooks.
`hooks.json.template` is for manual inspection and custom deployments. Replace
`__MEMORY_SUPERVISOR_ROOT__` with an absolute forward-slash path and `__CODEX_HOOKS__` with the
absolute `hooks.json` path for that Codex home. The source path lets the gate ignore this user hook
if a different Codex home rediscovers it as a project hook.

Codex hook JSON is Claude-compatible for SessionStart, UserPromptSubmit, SubagentStart,
SubagentStop, Stop, PreToolUse, and PostToolUse. SubagentStop is logged without adding model context.
Codex has no PostToolBatch, so PostToolUse calls the same transition notice path.

The adapter requires Codex 0.145.0 or newer with `hooks stable true`. Native `PreToolUse` maps
`spawn_agent` to `Agent`, so ordinary `codex`, `codex exec`, and IDE-hosted sessions use the same
pre-allocation gate. Unsupported releases are left unmodified. See `docs/guides/usage-codex.md`.
