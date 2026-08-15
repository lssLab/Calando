# Notification setup — finish in the terminal without opening a file

<p align="center">
  <strong>English</strong> · <a href="notifications.ko.md">한국어</a> · <a href="notifications.zh-CN.md">简体中文</a> · <a href="notifications.ja.md">日本語</a>
</p>

After Memory Supervisor is installed, the same `memory-supervisor notifications ...` commands work
in Linux, WSL, macOS, and Windows PowerShell. You do not need to find a settings file or type
variable names and quoting by hand. Route and credential changes take effect on the next
notification event; the daemon and Claude Code or Codex CLI do not need a restart.

Do not put a Discord webhook URL or Discord or Telegram bot token on the command line. Paste it at
the `(hidden)` prompt after starting the command. The value is not echoed and does not enter shell
history. Memory Supervisor saves it atomically in a per-OS private file, with mode `600` on Unix.

## Check the current state first

Copy this one line into any terminal:

```bash
memory-supervisor notifications show
```

The output shows enabled routes, the selected Discord method, and the Telegram chat, but never
prints the original webhook or token.

## Enable or disable optional routes

Notifications always cover **real protective actions only**. GREEN/YELLOW/ORANGE/RED transitions
and unconfirmed leak observations stay in `memory-status` and the incident ledger. The commands
below choose where the same action notification is delivered; they do not change its detail level.

`hook` is the main agent's awareness and recovery contract. `terminal` delivers the exact recovery
command when a paused lead cannot run its own hook. Both are mandatory and cannot be disabled by a
command, config file, or environment variable. Only `os,discord,telegram` are optional.

Enable every optional route:

```bash
memory-supervisor notifications routes all
```

Add only native OS notifications to the mandatory hook and terminal routes:

```bash
memory-supervisor notifications routes os
```

Use OS notifications and Discord:

```bash
memory-supervisor notifications routes os,discord
```

Disable every optional route while keeping hook and terminal delivery:

```bash
memory-supervisor notifications routes none
```

The valid optional names are `os,discord,telegram`. Supplying `hook` or `terminal` is rejected
because those routes are mandatory. A Discord or Telegram setup command automatically adds its
route to the current selection. Selecting `all` without corresponding credentials simply skips the
unconfigured remote route.

Terminal notices are not printed on every color change. For a real action such as lead pause,
resume, or probation, the supervisor revalidates the target PID's exact TTY or Windows console and
writes one plain-text notice. It does not inject input or change terminal mode. A full-screen TUI
may redraw a line or two and restores its view on the next redraw. If the exact terminal cannot be
verified and written, the supervisor does not leave a lead paused. A denied AI CLI tool call also
returns its reason directly to the lead.

The OS route uses `notify-send` on Linux, the Windows host notification path from WSL, `osascript`
on macOS, and NotifyIcon on Windows.

## Discord A — connect a webhook (recommended)

This is the simplest option because it does not require a bot.

1. Open the target server text channel in Discord desktop or web.
2. Choose **Edit Channel → Integrations → Webhooks → New Webhook**.
3. Confirm the name and target channel, then choose **Copy Webhook URL**.
4. Run:

```bash
memory-supervisor notifications discord-webhook
```

5. At `Discord webhook URL (hidden):`, paste the URL and press Enter. No visible characters is
   expected.
6. Test the connection:

```bash
memory-supervisor notifications test
```

Setup is complete when the command prints `discord: delivered` and the channel receives the test
message. The setup command enables Discord and replaces any previous Discord delivery method.

A webhook URL is a secret that can write to its channel. If it leaks, delete that webhook in
Discord, create a new one, and run the setup command again.

## Discord B — send to a channel through an existing bot

Use this only when you already operate a Discord bot.

1. Get its token from the Discord Developer Portal, invite it to the server, and grant
   **Send Messages** in the target channel.
2. Enable **User Settings → Advanced → Developer Mode**.
3. Right-click the target channel and choose **Copy Channel ID**.
4. Replace the number below with that channel ID:

```bash
memory-supervisor notifications discord-channel 123456789012345678
```

5. Paste the token at `Discord bot token (hidden):`, press Enter, and test:

```bash
memory-supervisor notifications test
```

Do not add a `Bot ` prefix to the token. Memory Supervisor adds it to the API request.

## Discord C — send a direct message through an existing bot

You must share a server with the bot and allow DMs from that server.

1. Enable Discord developer mode, right-click your profile, and choose **Copy User ID**.
2. Replace the number below with your user ID:

```bash
memory-supervisor notifications discord-dm 123456789012345678
```

3. Paste the bot token at the hidden prompt and test:

```bash
memory-supervisor notifications test
```

On the first send, the bot creates a DM channel and caches only that channel ID locally.

Remove Discord credentials and disable its route with one line:

```bash
memory-supervisor notifications disable-discord
```

## Telegram — connect a bot and discover its chat

Memory Supervisor does not create a public webhook server that accepts Telegram commands. It only
sends notifications through the Bot API `sendMessage` method.

1. Open `@BotFather`, create a bot with `/newbot`, and copy its token.
2. For personal alerts, open the new bot's conversation. For group alerts, add it to the group.
3. Run:

```bash
memory-supervisor notifications telegram
```

4. Paste the token at `Telegram bot token (hidden):` and press Enter. The command first checks
   pending updates. If none exists, it prints `waiting 120 seconds`; while it waits, send a fresh
   `/start` or message to that exact bot, or a fresh message in the target group. When exactly one
   chat appears, the command saves its ID and enables Telegram.
5. Test the connection:

```bash
memory-supervisor notifications test
```

Setup is complete when the command prints `telegram: delivered` and Telegram receives the test.

If multiple chats are visible in the bot's updates, the command lists their IDs and labels without
saving anything. Choose one and rerun with its ID; group IDs are usually negative:

```bash
memory-supervisor notifications telegram -1001234567890
```

Paste the same token again. If no chat appears within 120 seconds, rerun the command and send a new
message to the exact bot paired with that token after the waiting message appears. Do not assume an
old `/start` can be read again.

Discovery errors are reported separately:

| Error | Meaning | Action |
| --- | --- | --- |
| `HTTP 401` | The BotFather token is invalid or revoked | Copy the current token from `@BotFather` and rerun |
| `HTTP 409` | This bot already has a webhook or another `getUpdates` consumer | Use a dedicated Memory Supervisor bot; existing integration is not deleted automatically |
| `connection failed or timed out` | Telegram API network connection failed | Check internet, firewall, and proxy, then rerun |
| `No Telegram update arrived within 120 seconds` | No fresh update arrived from the exact bot or group | Send a new `/start` or message while the command waits |

On failure, the token and chat ID are not saved. Memory Supervisor never calls `deleteWebhook`
automatically because that could break an existing bot integration.

Remove Telegram credentials and disable its route with:

```bash
memory-supervisor notifications disable-telegram
```

## Verify connections and read test results

Show the current configuration:

```bash
memory-supervisor notifications show
```

Send a test through enabled OS routes and configured remote routes:

```bash
memory-supervisor notifications test
```

| Result | Meaning | Next action |
| --- | --- | --- |
| `delivered` | The route received the test | Done |
| `disabled` | The route is not selected | Add it with `routes ...` if wanted |
| `not configured` | The route is enabled but credentials are incomplete | Run the Discord or Telegram setup command above |
| `unavailable` | No OS notification transport is available in this GUI/session | Use a desktop session or a remote route |
| `failed` | API, permission, or network error | Check token, ID, permissions, and network, then configure and test again |

`hook` and `terminal` require a real AI CLI hook or the exact target of a real protective action,
so the test command does not synthesize messages for them. `memory-status --connections` reports
daemon, hook, and selected-route wiring. `memory-status` records each real event's
`delivered|failed|skipped|unavailable` result.

Normal use never requires opening the backing file:

| Environment | Private internal location |
| --- | --- |
| Linux, WSL, macOS | `~/.config/memory-supervisor/notifications.conf` |
| Windows | `$HOME\.config\memory-supervisor\notifications.conf` |

Any explicitly set `MEMORY_SUPERVISOR_NOTIFICATION_*` environment variable overrides the saved
value. `show` and setup commands warn about those override names; unset them first if a saved change
does not take effect.

## When notifications are sent

- One `pressure-episode / active` when any `HOLD|DRAIN`, live logical restriction, managed stopped
  PID, or lead probation first becomes active
- One final `recovered` after all those conditions clear, or `ended-with-loss` when a stopped
  worker disappeared before confirmed resume
- Exact-terminal PID pause/resume safety notices
- A previously fresh federation peer going stale, and its later recovery
- A rate-limited protection-unavailable warning while hooks fail open without a live daemon
- Action-required failures such as degraded sensor/runtime/notification protection or failed
  probation

Raw utilization transitions and leak suspects that have not caused an action remain only in the
incident ledger. Ordinary `SessionStart/End`, `SubagentStart/Stop`, a steady `ACTIVE` state, and an
unchanged `HOLD/DRAIN` tick do not create another user notification. Lifecycle inventory never
advances the user-visible logical-control epoch by itself. Interior spawn denials, worker-start
delays, logical cushioning, per-PID pause/resume events, and normal probation stages are also
`importance=detail`. A denied hook still returns its `systemMessage` directly to that lead; it does
not clone the same fact into another Discord, Telegram, or OS message.

The dividing line is intent, not the event name: if the supervisor turns evidence into an explicit
proactive awareness instruction for a lead, that instruction is a user-visible action and is
delivered once. A sensor sample or an unchanged boundary that asks nobody to do anything stays in
the ledger and does not consume model context.

A lead incident message includes the PID, the direct process or machine-pressure evidence, the
separate estimated `agent|external|mixed|unknown` system attribution, and whether to wait for
automatic recovery or use a manual command. Exact terminal and remote routes still work when a paused lead
cannot run its hook. Terminal, OS, and remote delivery are attempted immediately; model and lead
awareness arrives at the next hook boundary. Every pause, probation, success/failure, manual resume,
and external resume message states that timing difference. Repeats are suppressed by event type,
status, source, and incident/session epoch. A real recovery is a new transition and is delivered
once; a boundary merely remaining stable is not.

Hooks, `memory-status`, exact terminal, OS, Discord, and Telegram all render that structured event
through the same user boundary. Runtime records written by an older release are normalized there as
well, so obsolete debug text such as `Some(...)` is not replayed after an update.

Remote channel history remains visible while the user is away, unlike a popup. The authoritative
incident record is still the local notification ledger in `runtime.json` and `state.json`; Discord
and Telegram are best-effort copies whose failure never blocks detection or protection.
