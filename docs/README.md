# Documentation

<p align="center">
  <strong>English</strong> · <a href="README.ko.md">한국어</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.ja.md">日本語</a>
</p>

You do not need to read every document. Start with the one of these three that matches what you
want to do.

| Start here | Read it when you need to |
| --- | --- |
| [Installation, connection, and supported environments](guides/setup.md) | Install for the first time, connect a running Claude Code or Codex session, or check hook trust and Windows, WSL2, macOS, or Linux conditions |
| [How Memory Supervisor works](guides/how-it-works.md) | Understand gradual braking, CLI versus Codex App, blind control, or federation |
| [Operations, notifications, and recovery](guides/operations.md) | Use status and control commands, configure notifications, or handle a paused process and recovery |

To read the original detailed README continuously from beginning to end, use the
[detailed guide](detailed-guide.md).

<details>
<summary><strong>Show every specialist reference</strong></summary>

### Architecture and platforms

- [Architecture](guides/architecture.md) — terminals, agents, hooks, and the supervisor process
- [Codex Desktop App](guides/codex-app.md) — per-conversation observation and control inside a shared App Server
- [Federation](guides/federation-topology.md) — coordinating multiple kernels and terminals on one machine
- [Platforms](guides/platforms.md) — Windows, WSL2, Linux, macOS, VMs, and containers
- [Resource boundaries](guides/resource-boundaries.md) — automatic thresholds, optional caps, and recovery boundaries

### Connections and operations

- [Claude Code](guides/usage-claude.md) — Claude Code hooks and connection verification
- [Codex](guides/usage-codex.md) — Codex CLI and Desktop App hooks and trust
- [Notifications](guides/notifications.md) — terminal, operating system, Discord, and Telegram
- [Windows executable trust](guides/windows-signing.md) — unsigned Windows builds and Smart App Control

### Security, performance, and verification

- [Security](guides/security.md) — observed data, control scope, and data never handled
- [Performance](guides/performance.md) — resident memory and hook/status latency
- [Test coverage](testing/test-matrix.md) — behaviors and platforms covered by public tests
- [Adaptive stopping distance](testing/stopping-distance.md) — braking calculations and controlled measurements

</details>

Every public document is provided in English `.md`, Korean `.ko.md`, Simplified Chinese
`.zh-CN.md`, and Japanese `.ja.md`.
