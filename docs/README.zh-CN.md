# 文档

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="README.ja.md">日本語</a>
</p>

您不需要阅读每份文档。 从这三个中与您想做的事情相匹配的一个开始。

| 从这里开始 | 当你需要的时候阅读它 |
| --- | --- |
| [安装、连接和支持的环境](guides/setup.zh-CN.md) | 首次安装，连接正在运行的 Claude Code 或 Codex 会话，或检查挂钩信任和 Windows、WSL2、macOS 或 Linux 条件 |
| [Memory Supervisor工作原理](guides/how-it-works.zh-CN.md) | 了解渐进制动、CLI 与 Codex App、blind control 或联合 |
| [操作、通知和恢复](guides/operations.zh-CN.md) | 使用状态和控制命令、配置通知或处理暂停的进程和恢复 |

要从头到尾连续阅读原始详细自述文件，请使用[详细指南](detailed-guide.zh-CN.md)。

<details>
<summary><strong>显示每个专家参考资料</strong></summary>

<a id="architecture-and-platforms"></a>
### 架构和平台

- [架构](guides/architecture.zh-CN.md) — 终端、代理、挂钩和主管进程
- [Codex Desktop App](guides/codex-app.zh-CN.md) — 在共享App Server内进行每次对话观察和控制
- [Federation](guides/federation-topology.zh-CN.md) — 协调一台机器上的多个内核和终端
- [平台](guides/platforms.zh-CN.md) — Windows、WSL2、Linux、macOS、虚拟机和容器
- [资源边界](guides/resource-boundaries.zh-CN.md) — 自动阈值、可选上限和恢复边界

<a id="connections-and-operations"></a>
### 连接和操作

- [Claude Code](guides/usage-claude.zh-CN.md) — Claude Code 挂钩和连接验证
- [Codex](guides/usage-codex.zh-CN.md) — Codex CLI 和桌面应用程序挂钩和信任
- [通知](guides/notifications.zh-CN.md) — 终端、操作系统、Discord 和 Telegram
- [Windows 可执行文件信任](guides/windows-signing.zh-CN.md) — 未签名的 Windows 版本和Smart App Control

<a id="security-performance-and-verification"></a>
### 安全性、性能和验证

- [安全性](guides/security.zh-CN.md) — 观察到的数据、控制范围和从未处理过的数据
- [性能](guides/performance.zh-CN.md) — 驻留内存和挂钩/状态延迟
- [测试覆盖范围](testing/test-matrix.zh-CN.md) - 公共测试涵盖的行为和平台
- [自适应停车距离](testing/stopping-distance.zh-CN.md) — 制动计算和受控测量

</details>

每个公共文档均以英语 `.md`、韩语 `.ko.md`、简体中文 `.zh-CN.md` 和日语提供 `.ja.md`。
