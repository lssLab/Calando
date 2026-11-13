# Calando agent reference

Read only the section the current task needs. Owner commands run only on the user's explicit
request in this conversation.

## Inspect first

```bash
calando status          # this environment, CLI connections, and every other environment
calando status --json
```

If a connection shows `NEEDS ATTENTION`, the repair is `calando update` (owner command: it fetches
and activates code). A new or changed Codex hook must be trusted by the user in CLI `/hooks` or App
**Settings → Hooks**; restarting cannot grant trust, and an entry that is off must also be switched
on there. Claude Code has no per-hook approval, but an interactive session runs settings-file hooks
only after the user trusts the workspace folder.

If `calando` is unavailable, read `~/.cache/calando/state.json` (or the path in
`~/.calando/state-dir`). A missing or older-than-10-second state means Calando is unavailable and
everything fails open.

## Turn protection on or off

- `calando on|off` (terminal): the whole environment. `off` first resumes anything Calando paused,
  then disables the service; hooks stay installed and pass through. Windows, each WSL distribution,
  VM, and container are separate environments.
- `/calando on|off` (Codex: `$calando`): only the session where the user types it. The hook answers
  it; it never reaches the model.
- If an administrator ran `calando lock on`, both refuse to turn Calando off.

## Respond to a notice

1. `New heavy work held`: do not create new agents, workflows, or heavy builds; existing work
   continues. Retry when admission returns GREEN or YELLOW. Utilization may stay RED under a stable
   heavy workload; the admission action is what limits new work.
2. `Process paused`: do not recreate or resume that worker. Calando resumes pressure pauses one at
   a time after memory recovers; a lead paused for runaway growth gets one guarded automatic resume.
3. A paused lead that Calando will not resume: tell the user to close that terminal and continue
   with the CLI's own resume (`claude --resume`, `codex resume`), or run `calando off`. When a
   session is resumed, its first hook says that its previous process ended and that subagent
   results from that run may be missing.

## Memory cap (advanced)

Not needed for normal use. `calando cap <GB>` limits all agents in this environment;
`/calando cap <GB>` limits one session's process tree. Both explain themselves and ask for
confirmation (`calando cap <GB> --yes` for a non-interactive caller after the user agreed). Near the
cap new heavy work is held; above it one growing subagent is paused. A pause cannot reclaim memory
already used and use can pass the cap briefly between checks, so use cgroup, container, or VM limits
for byte-exact quotas.

## Notices to the user

Telegram is the only push route: `calando telegram` asks for a bot token, finds the chat, and sends
a test message; `calando telegram off` removes it. The hook context, the terminal line, and the
paused-terminal notice are always on.

## Tune safely

Persist tuning in `~/.config/calando/config.json`; environment variables with the same names
override it. Normal operation needs no tuning. Inspect `memory_capacity_mb`, slopes, distress, TTE,
`automatic_reserve_mb`, and `expected_burst_mb` before proposing any override, never lower
thresholds while a large known-good build is running, and apply a change with `calando update`.
Treat `sensor_ok=false`, `configuration_error`, `runtime_error`, `pending_control`, or
`protection_degraded=true` as degraded protection.

## Route platform and CLI work

- `docs/guides/platforms.md`: Windows, macOS, Linux, and WSL installation or service issues.
- `docs/guides/usage-claude.md`: Claude Code hook behavior.
- `docs/guides/usage-codex.md`: Codex hook requirements and trust review.
- `docs/guides/codex-app.md`: shared App Server ownership and blind-control limits.
