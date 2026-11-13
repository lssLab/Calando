# Operations, notifications, and recovery

<p align="center">
  <strong>English</strong> · <a href="operations.ko.md">한국어</a>
</p>

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
message. Do not put the token on the command line. See [notification setup](notifications.md) for
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
see [security](security.md).

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
`~/.agents/skills`, and `~/.codex/skills`. See the [Claude Code guide](usage-claude.md) and
[Codex guide](usage-codex.md).

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
[security and data/control boundaries](security.md) for the complete list.

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

Rust unit, integration, and installer tests cover policy, process safety, Claude Code and Codex
wiring, federation, recovery, and release bundles. GitHub Actions checks builds and platform
contracts on Linux x86-64, Windows x86-64, Apple Silicon macOS, and macOS x86-64 under Rosetta. A real
near-exhaustion boundary is covered by bounded physical-machine verification plus deterministic
simulation. See [Test coverage](../testing/test-matrix.md).
