# Notification setup — finish in the terminal without opening a file

<p align="center">
  <strong>English</strong> · <a href="notifications.ko.md">한국어</a> · <a href="notifications.zh-CN.md">简体中文</a> · <a href="notifications.ja.md">日本語</a>
</p>

Calando has three notice routes. Two are always on; Telegram is the only optional push route.

| Route | Where it appears | Can be turned off |
| --- | --- | --- |
| Hook context | The agent's next hook, so the lead knows what happened and what to do | No |
| Terminal | The one-line message the hook shows, and a notice written to a paused process's terminal | No |
| Telegram | A bot's private chat or a group | Yes |

Check state at any time with
`calando status` in a terminal or `/calando status` (Codex: `$calando status`) in a session.

## Telegram — connect a bot

Calando does not run a webhook server and does not accept Telegram commands. It only sends messages
through the Bot API `sendMessage` method.

1. Open `@BotFather`, create a bot with `/newbot`, and copy its token.
2. For personal notices, open the new bot's conversation. For a group, add the bot to the group.
3. Run:

```bash
calando telegram
```

4. Paste the token at the `(hidden)` prompt and press Enter. The value is not echoed and does not
   enter shell history. If the bot has no recent message, the command prints
   `Waiting 120 seconds...`; send the bot (or the group) a new message while it waits.
5. When exactly one chat is found, Calando saves it and sends a test message. Setup is complete when
   the command prints `Telegram on. A test message was sent.`

If several chats wrote to the bot, the command lists their IDs and saves nothing. Rerun with the one
you want; group IDs are usually negative:

```bash
calando telegram -1001234567890
```

| Error | Meaning | Action |
| --- | --- | --- |
| `HTTP 401` | The token is invalid or revoked | Copy the current token from `@BotFather` and rerun |
| `HTTP 409` | The bot already has a webhook or another `getUpdates` client | Use a dedicated bot for Calando; no existing webhook is changed |
| `No message reached the bot within 120 seconds` | No new message arrived | Rerun and send a new message while it waits |

On failure, nothing is saved. Calando never calls `deleteWebhook`.

Remove the Telegram settings with:

```bash
calando telegram off
```

Changes take effect on the next notice; the daemon and Claude Code or Codex do not need a restart.

The settings are saved atomically in a private file (mode `600` on Unix). Normal use never requires
opening it:

| Environment | Location |
| --- | --- |
| Linux, WSL, macOS | `~/.config/calando/notifications.conf` |
| Windows | `%USERPROFILE%\.config\calando\notifications.conf` |

An explicitly set `MEMORY_SUPERVISOR_TELEGRAM_*` environment variable overrides the saved value.

## Message format

Messages are in English. The first line is the state, the second the environment, then one
`Label: value` per line.

```text
Calando — Memory protection on
WSL (Ubuntu-LAPTOP-1)
Free: 2.1 of 16 GB
Restricted agents: 1
Action: New heavy work is on hold. Running work continues.
```

Lines that can appear: `Free`, `WSL VM: holds X GB, uses Y GB` (Windows only), `Restricted agents`
and `Paused processes` (when not zero), `Duration` (on recovery of a pressure episode), and `Action`. The environment line reads, for example,
`Windows (LAPTOP-1)` or `WSL (Ubuntu-LAPTOP-1)`.

## Scope

A notice about a process goes only to the session that owns that process. Other sessions are
mentioned only as a count and their total agent memory, never by name, path, or PID.

## When notices are sent

Notices cover **real protective actions only**. Utilization color changes and unconfirmed leak
observations stay in `calando status` and the incident ledger.

- One `active` notice when braking, a hold that actually deferred work, a live logical restriction, a
  paused process, or lead probation first becomes active
- One final `recovered` after all those conditions stay clear for the stability window, or
  `ended-with-loss` when a paused worker disappeared before it was resumed
- Process pause and resume notices on the exact terminal
- A federation peer going stale, and its later recovery
- A rate-limited protection-unavailable warning while hooks fail open without a live daemon
- Action-required failures such as degraded sensor or runtime protection, or failed probation
- With automatic updates off, one Telegram message per new version; a Codex hook-changing release
  also sends one message asking you to run `calando update` and trust the hooks

Ordinary session and subagent start or stop, a steady state, and an unchanged hold do not create
another notice. Repeats are suppressed by event type, status, source, and incident epoch. A denied hook
returns its reason directly to that lead and is not copied to Telegram.

Terminal notices are written only for a real action such as a lead pause, resume, or probation. The
supervisor revalidates the target PID's exact TTY or Windows console and writes one plain-text
notice; it does not inject input or change terminal mode. A full-screen TUI may redraw a line or two.
If the exact terminal cannot be verified and written, the supervisor does not leave a lead paused.

Hook and terminal delivery happen at once; the lead's model sees the situation at its next hook
boundary. The authoritative record is the local ledger in `runtime.json` and `state.json`. Telegram
is a best-effort copy whose failure never blocks detection or protection.
