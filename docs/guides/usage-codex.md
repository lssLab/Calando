# Codex usage

<p align="center">
  <strong>English</strong> · <a href="usage-codex.ko.md">한국어</a>
</p>

## Supported contract

Memory Supervisor supports Codex CLI **0.145.0 or newer** when this command reports the hook feature
as stable and enabled:

```bash
codex --version
codex features list | grep '^hooks'
codex update
```

The installer runs the same check. Unsupported or disabled-hook releases are left without a
Memory Supervisor Codex hook; a previously owned hook is removed on reinstall so a downgrade cannot
silently claim protection. If `codex update` is unavailable for the original installation method,
upgrade Codex with that same package manager or installer, then run `memory-supervisor update`.

Current Codex hooks can observe local function tools through `PreToolUse`. Version 0.145.0 supplies
the optional subagent identity to thread-spawn hook input and omits it on the root thread. The
installed matcher is deliberately broad so input-aware classification can preserve ordinary work:

```text
.*
```

At adaptive admission `GREEN` or `YELLOW`, new expansion is silent and allowed. At `ORANGE` or
`RED`, only an actual expansion call briefly waits for recovery and then returns a valid deny
decision with `ADMISSION_DEFERRED`. A logical child state can separately deny only its classified
future expansion, high-memory start, broad discovery, or production-work class. Raw utilization
color can remain RED while adaptive admission allows stable work; results, messages, status,
stop/cancel, and recovery remain available in every state.

Official contract references:

- [Codex hooks and tool coverage](https://learn.chatgpt.com/docs/hooks)
- [Codex advanced configuration](https://developers.openai.com/codex/config-advanced#hooks)

## Installation and trust

Run the README's one-line installer. If the supervisor already exists and Codex was installed later,
run `memory-supervisor update`. Then use ordinary Codex commands:

```bash
codex
codex exec "your task"
```

The hook is merged into `$CODEX_HOME/hooks.json` when `CODEX_HOME` is set, otherwise
`~/.codex/hooks.json`; unrelated groups are preserved and the prior file is backed up before an
atomic replacement. Codex requires review and trust when a non-managed hook is
new or its definition changes. The user must personally open `/hooks` in an interactive CLI
session, review and trust the exact current command, and enable any disabled entry. The installer
never automates that user decision, and restarting Codex never substitutes for it.

That `/hooks` action belongs to Codex CLI. In Codex Desktop App, the user makes the same manual
decision in **Settings → Hooks**. Applying an enablement or trust change there reloads hook configuration for every task already loaded by the
shared App Server; continue an existing task with its next request. It does not require a new task
or an App restart, although a `SessionStart` that already happened is not replayed. Conversely, a
CLI `/hooks` save refreshes that CLI process but not a separately running Desktop App, and an App
Settings save does not refresh other running CLI processes. If both surfaces were already running,
save approval in App Settings first, then restart only the pre-existing CLI processes after their
work. If approval was written by the other process and the current surface has no actual change to
save, restart that already-running App or CLI once so it reads the shared trust record.

Memory Supervisor treats the seven owned events as one connection contract. `memory-status
--connections` checks each event's definition, enabled state, and exact current trust hash. A
missing, duplicated, disabled, untrusted, or changed entry therefore cannot be reported as Codex
`CONNECTED`, and a Codex App route cannot be reported as `ACTIVE` merely because one other event
produced a recent receipt. Use `memory-supervisor update` for a missing or stale definition; use
`/hooks` to enable or trust an entry. A successful `SessionStart` performs the same audit and tells
both the lead and the user which remaining events are incomplete and what to do next.

Run `memory-status --connections` after every `memory-supervisor update`. Codex stores trust against
the current hook definition, not against the Supervisor release number. A binary-only update behind
the same command therefore needs no re-approval. If the installer changed a command, matcher, or
another hashed field, Codex reports the new definition for review and the user must trust it again
on the affected CLI or App surface. A process restart may reload an approval already saved by a
different process sharing the same `CODEX_HOME`; it can never create that approval.

Each generated Codex command also includes the absolute `hooks.json` source it belongs to. The gate
compares that source with the current process's `CODEX_HOME`. This prevents a user hook from one
environment from acting again when another Codex home rediscovers it as a project hook. When no
other-OS route has been installed, its command field is a valid no-op rather than a cross-shell
error. An intentionally shared Windows/WSL `CODEX_HOME` retains an existing native route for each;
each local supervisor audits and controls only its own route and PID space. Federation still shares
only fresh admission state, never hook ownership or cross-environment PID authority.

User-level hooks apply independently of project trust. Project-local `.codex` hook layers are
ignored in untrusted repositories, but this installer does not depend on a project-local layer.
Codex merges matching hooks from all trusted sources, which is why the source guard above is part
of the installed route rather than a documentation-only warning.

## Hook events

| Event | Purpose |
| --- | --- |
| `SessionStart` | inject startup contract and unseen incidents on resume/clear/compact |
| `UserPromptSubmit` | warn about pressure or an unseen suspended/resumed incident |
| `PreToolUse` | classify local function tools; deny only new fan-out under machine admission or the work class excluded by an exact logical-agent state |
| `SubagentStart` / `SubagentStop` | observe lifecycle; start adds only the same twelve-second RED fallback |
| `PostToolUse` | deliver unseen incident versions without delaying completed work |
| `Stop` | close the current logical lifecycle record without blocking exit |

ORANGE never delays an already-admitted worker at `SubagentStart`. Codex has no post-tool
cooperative sleep; RED pressure is handled by pre-spawn admission and the independent PID backstop.
Installed command hooks intentionally omit the optional `statusMessage`, so routine Pre/Post hook
execution does not leave Memory Supervisor spinner text in the TUI. A visible message is returned
only for a real action or unseen incident. If an existing session still shows persistent
`Running PreToolUse/PostToolUse hook` lines, reapply with `memory-supervisor update`, review `/hooks` if
the hash changed, then close `/hooks` and continue that CLI session. Restart only other
CLI processes that were already open and did not receive that process-local reload.

All command wrappers fail open. A missing daemon, stale state, malformed hook input, or Rust gate
failure produces no deny decision. The OS daemon remains the independent backstop.

Codex hook trust is hash-based. Reinstalling changed hook commands makes them pending review; use
`/hooks` in the affected CLI process to inspect and trust the exact definitions. That save reloads
sessions hosted by the same CLI process; a new session is needed only to replay `SessionStart`. A
supervisor daemon restart
does not restart Codex. If a paused Codex lead resumes, an exact target-terminal notice and the
OS/remote adapters immediately distinguish direct sustained material process-growth evidence from
the separate `agent|mixed|external|unknown` machine attribution estimate and explain what happens
next. The hook repeats the same safe event once at the next prompt or
post-tool boundary, which is not guaranteed to coincide with OS-level resume. If Codex itself is
restarted, use Codex's session resume path. Codex restores the transcript in a new process, while
the installed `SessionStart` hook automatically injects any retained, unseen resource incident and
current supervisor decision once. `runtime.json` preserves the resource incident; it does not
replace Codex's transcript mechanism.

## Verification

Repository validation checks:

```bash
bash tests/run.sh
memory-status --connections
```

`tests/native_codex.rs` additionally uses the official binary to verify detection and round-trip
native suspend/resume on a disposable Codex process. Run that opt-in canary with:

```bash
MEMORY_SUPERVISOR_NATIVE_CODEX_SMOKE=1 \
  cargo +1.88.0 test --test native_codex -- --nocapture
```

The remaining Rust integration tests verify the minimum version and feature report, installed hook
shape, ORANGE `Agent` denial, exact terminal targeting, and malformed/stale fail-open cases. They do
not start App Server or require a model-authenticated agent spawn.

Automated checks pin the minimum supported Codex version and hook contract so a feature-state or
command-shape change is detected before public executables are built.
