# Claude Code usage

<p align="center">
  <strong>English</strong> · <a href="usage-claude.ko.md">한국어</a>
</p>

## Supported contract

Memory Supervisor `0.2.0-alpha.3` supports Claude Code **2.1.217 or newer**. This is the pinned
latest-supported baseline for the graded logical-control contract; old releases do not receive a
reduced matcher set or compatibility policy.

```bash
claude --version
claude update
```

The installer checks version and hook wiring as two separate facts. It searches the active `PATH`
and known user installation locations, including native, NVM, fnm, asdf, Volta, and Windows npm
paths, then uses the newest supported Claude Code it can verify. An older executable earlier on a
non-login process's `PATH` therefore cannot hide a current user installation.

If no supported executable can be verified, installation reports the version problem but preserves
any existing Memory Supervisor hook. A failed version probe is not evidence that a valid hook should
be deleted. `memory-status --connections` continues to show hook health separately and does not call
the provider protected until both the version and hook are ready. If `claude update` is unavailable
for the original installation method, upgrade Claude Code with that same package manager or
installer, then run `memory-supervisor update`.

Claude Code has the broadest documented integration of the supported CLIs. `PreToolUse` observes
all tool paths, classifies their actual input, gates only new expansion at machine admission, and
enforces a named logical agent's future-work cushion when one exists. It does not undo a tool that
already started.

Run the platform installer. It atomically merges these events into
`~/.claude/settings.json` without replacing unrelated hooks:

- `SessionStart`: inject the resource contract at startup; resume/clear/compact stay silent only
  when there is no unseen suspension incident.
- `UserPromptSubmit`: inject non-GREEN adaptive admission and any unseen incident even after recovery.
- `PreToolUse`: classify every tool; delay and hand back new expansion under machine pressure, hold
  new classified high-memory starts while machine distress is critical, or deny only the
  future-work class excluded by the target logical state.
- `SubagentStart`: lifecycle observation plus a twelve-second RED-only fallback; ORANGE never delays
  an already-admitted worker.
- `SubagentStop`: close the logical lifecycle record while preserving any supervisor denial that
  may have made the result partial.
- `PostToolUse` and `PostToolBatch`: record progress. A lead boundary delivers unseen incident
  context; a subagent boundary cannot consume the lead's incident cursor. Neither adds a fixed RED
  sleep.
- `Stop` and `SessionEnd`: close lead/session lifecycle state without blocking normal exit.

## Hook activation, workspace trust, and reload

Claude Code does not use Codex's per-hook hash approval. The installer writes the Memory Supervisor
hook to user settings at `~/.claude/settings.json`; that user hook has no separate approve/enable
step. Interactive Claude Code nevertheless holds every settings-file hook, including this user
hook, until the user accepts workspace trust for the current folder or one of its parents. Claude's
`/hooks` screen is a read-only browser and cannot grant that trust.

Workspace trust is one folder-level decision rather than a Memory Supervisor-specific Hook review.
Accept it only for a working folder you trust. After that decision, current Claude Code watches
settings files, so a running session normally picks up a later user-hook change. Restart only if the
entry does not appear after a short wait, or open a new session when the goal is specifically to
exercise the once-per-session `SessionStart` event.

A plain non-interactive `claude -p` run loads the same user settings and hooks, so Memory Supervisor
covers it without an additional setup step. Claude Code skips workspace-trust verification in this
mode. If `--bare` is added, Claude Code deliberately skips all hooks and Memory Supervisor cannot
supervise that invocation.

After installation or `memory-supervisor update`, run `memory-status --connections`. Its Claude
`CONNECTED` result verifies a supported executable, skill, and current user-hook wiring. Use
Claude's read-only `/hooks` view to confirm the entry under `User Settings` when needed. Neither
check proves workspace trust for the current folder. An organization policy such as managed-only
hooks or `disableAllHooks` can still prevent the user hook and requires administrator action.

Installed command hooks intentionally omit the optional `statusMessage`, so normal hook execution
does not keep a Memory Supervisor progress line in the TUI. User-visible text appears only for a
real protective action or unseen incident. If an already-running session still shows old routine
hook progress lines, run `memory-supervisor update` and open a new Claude Code session so the AI CLI
reloads the current hook definition.

Admission uses the worst fresh adaptive action from `MEMORY_SUPERVISOR_FEDERATION_DIR`, so pressure in a host,
WSL, or VM holds new Claude fan-out everywhere while process pause remains local. Raw utilization
color alone does not block fan-out.

If the Claude lead is `PAUSED_BY_SUPERVISOR`, no in-process hook can run during that pause. The
supervisor therefore writes the cause and exact recovery policy to the revalidated target terminal
and independently queues OS, Discord, and Telegram delivery. Automatic probation, success,
failure, manual resume, and raw external resume get the same phase-specific guidance. A hook then
delivers it once to the user and model at the next prompt/tool boundary; that boundary may be later
than the OS-level resume itself. `memory-supervisor resume` continues the same PID and in-memory
session. If Claude was terminated and started with `--resume`, `SessionStart source=resume`
delivers the resource incident while Claude's transcript mechanism restores the conversation
separately.

`StructuredOutput` and the other result/message/status tools remain allowed in `HANDOFF_ONLY`.
Supervisor denials are recorded with their tool, reason, time, and logical epoch, then summarized
to the lead at its next completion/prompt boundary. Provider-specific quota exhaustion that arrives
as an ordinary successful tool-result string has no structured failure signal and must still be
reported by the subagent.

Intentional decisions are JSON with exit code 0. The stable wrapper converts Rust gate, state, or
policy failures into a silent exit 0, so an internal failure cannot accidentally become Claude
Code's exit-code 2 prompt block.

Verify:

```bash
bash tests/run.sh
memory-status --connections
memory-status
printf '{}' | hooks/gate.sh SessionStart
```

Contract references:

- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code permissions and workspace trust](https://code.claude.com/docs/en/permissions)
- [Claude Code installation and updates](https://code.claude.com/docs/en/installation)
- [Claude Code installation troubleshooting](https://code.claude.com/docs/en/troubleshoot-install)

On Windows use:

```powershell
'{}' | powershell -File .\hooks\gate.ps1 SessionStart
```

The personal skill is linked at `~/.claude/skills/memory-supervisor`. A newly created top-level
skills directory may require a new Claude Code session before discovery.

When a process is paused, first follow the action block: pressure-paused workers and the first
probation for a lead paused after sustained material-growth evidence recover automatically. For an authorized manual override, use
`memory-supervisor resume <pid>` (or `memory-supervisor resume` when exactly one PID is paused) rather
than raw `kill -CONT` so the
daemon validates the process start identity, clears its state, persists the RESUMED incident, and
applies the resume cooldown.

## If a hook blocks every prompt

Do not keep editing the active hook from the blocked session. From a separate terminal:

1. Back up `~/.claude/settings.json` and the current supervisor checkout.
2. Run `printf '{}' | hooks/gate.sh UserPromptSubmit`; the safe result is valid JSON or no output,
   always with exit code 0.
3. Run `bash tests/run.sh`.
4. Run `memory-supervisor update` to atomically replace only owned supervisor hook entries and reload the service.
5. Open a new Claude Code session because hook definitions may be snapshotted at session start.

If every prompt remains blocked after this procedure, use `memory-status --connections` and the
gate exit code to separate a hook-wiring failure from supervisor state.
