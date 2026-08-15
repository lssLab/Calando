# 操作、通知和恢复

<p align="center">
  <a href="operations.md">English</a> · <a href="operations.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="operations.ja.md">日本語</a>
</p>

<a id="notifications"></a>
## 通知

Memory Supervisor 不会在每次内存读取时发出通知。 每次真正的保护操作开始或完全恢复时，或者当连接或保护状况需要用户注意时，它都会发送一个通知。

| 路线 | 它出现在哪里 | 报告的时间和内容 |
| --- | --- | --- |
| 终端 | 运行受影响的 Claude Code 或 Codex CLI 进程的确切终端 | 当进程暂停或恢复时，或者当 lead 暂停被释放一次以检查恢复时，立即显示原因、PID 和恢复命令。 这条路线永远畅通。 |
| 操作系统 | Linux、WSL、macOS 或 Windows 桌面通知 | 当保护首次起作用或完全恢复时，或者当federation连接或保护需要注意时出现。 当桌面通知可用时，此可选路由有效。 |
| 电报 | 用户选择的机器人私人聊天或群组 | 报告重要操作的启动和恢复以及连接或保护问题。 它包括内存状态、原因、存在目标时的 PID 以及下一步操作，并留下用户可以在离开时查看的历史记录。 |
| 不和谐 | 连接的通道、Webhook 或直接消息 | 发布相同的重要操作、恢复和注意事项。 此可选路线适用于团队频道或个人通知。 |

记录事件后立即尝试终端、操作系统、Telegram 和 Discord 传送。 未改变的条件不会重复发送。 lead 在其下一个 hook 边界接收相同的情况和恢复状态。 终端路由始终保持连通； 使用以下命令配置和测试可选路由：

```bash
memory-supervisor notifications show
memory-supervisor notifications routes os
memory-supervisor notifications discord-webhook
memory-supervisor notifications telegram
memory-supervisor notifications test
```

请勿将 Discord Webhook URL 或 Discord 或 Telegram 机器人令牌放在命令行上。 在运行安装命令后出现的隐藏提示中输入它。 更改适用于下一个通知，无需重新启动 supervisor 或 AI 程序。 请参阅[通知设置](notifications.zh-CN.md)了解路由选择和删除、Discord 频道和 DM 设置、Telegram 群组设置以及故障排除。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## Claude Code和Codex中的技能和命令

安装程序连接三个独立的部分：做出自动决策的**hooks**、教代理理解和解释状态的**技能**以及调用该工作流程的**简短命令**。 Hooks 无需用户调用即可运行； 该技能本身并不强制执行内存策略。

| 使用地点 | 输入什么 | 它的作用 |
| --- | --- | --- |
| Claude Code | 询问“检查内存状态”，使用`/memory-supervisor`，或使用`/memory-status` | 安装的技能或快捷方式会读取完整状态并解释原因、自动恢复以及任何所需的命令。 |
| Codex CLI | 使用`$memory-supervisor check memory status`； 使用`/skills`确认发现。 `/prompts:memory-status` 是兼容性快捷方式。 | 通过Codex的主要技能路径运行相同的状态工作流程。 Hook 信任和支持在`/hooks` 中保持分离。 |
| Codex Desktop App | 使用`$memory-supervisor check memory status`或在任务中自然询问 | 在每个任务中使用相同的用户级别Codex技能。 没有单独的App技能； 在 ** 设置 → Hooks** 中管理 hooks。 |
| 操作系统终端 | 使用`memory-status`或`memory-supervisor ...` | 这些是真实的状态、设置和恢复命令，而不是技能。 `resume`、`terminate` 和 `kill` 仅在明确的用户请求后运行。 |

该技能读取 `memory-status --all` 并解释原因和下一步操作，但未经用户批准，它不会恢复或终止进程。 如果Claude Code或Codex是在Memory Supervisor之后安装的，请运行`memory-supervisor update`并验证与`memory-status --connections`的连接。 有关详细差异，请参阅[Claude Code指南](usage-claude.zh-CN.md)和[Codex指南](usage-codex.zh-CN.md)。

<a id="security"></a>
## 安全

Memory Supervisor 读取操作系统内存和进程信息，以及会话、代理、工具、工作目录和连接状态信息以及由 Claude Code 和 Codex hooks 提供的命令前缀。 它仅使用此信息来决定是否可以开始新的工作并确定确切的控制目标。

自动控制在延迟未来Claude Code或Codex工作时停止，并在最后的保护阶段暂停和恢复一个经过验证的本地工作流程。 它永远不会自动终止程序或控制不相关的程序。 正常监控不向外部发出请求； 只有 GitHub 安装和更新以及运营商启用的 Discord 或 Telegram 通知使用网络。

**这是完整的检查和控制边界； Memory Supervisor 不处理其之外的任何内容。** 它不使用可能存在于 hook 负载中的提示、对话文本、模型响应或文件内容来进行控制决策，并且不保留它们。 它不会直接打开项目文件或进程内存，也不会检查或更改浏览器或 IDE 内部数据、Claude 或 ChatGPT 凭据或操作系统内核、内存、交换和防火墙设置。 请参阅[安全和数据/控制边界](security.zh-CN.md)，了解存储数据、同一机器federation字段和安全措施的完整列表。

<a id="control-and-recovery"></a>
## 控制与恢复

当内存再次稳定时，暂停的工作会自动恢复，一次一项。 如果lead因为自身内存不断增长而暂停，它会自动恢复一次，以便supervisor可以检查结果。 如果返回相同的增长，则lead再次暂停并等待用户决定。 要手动恢复，请首先检查当前状态并使用其中显示的 PID。

```bash
memory-status
memory-supervisor resume [pid]
```

lead 暂停是故意非常罕见的。 它是**最终保护阶段**，仅在新工作延迟且subagent和工具控制尚未消除危险，并且已确认同一lead及其确切终点的持续增长时使用。 大多数事件通过较小的工作范围、worker 暂停或自动恢复来提前完成。

如果 Claude Code 或 Codex 意外终止，CLI 将恢复其对话，并且已安装的 `SessionStart` hook 将保留内存事件和当前决策传递给 lead 一次：

```bash
claude --resume
codex resume
```

当您有意想要关闭或打开保护时，仅使用这两个命令。 `off` 停止并禁用后台服务，同时保持安装的 Claude Code 和 Codex hooks 以静默直通模式连接。 该选择在重新启动和`memory-supervisor update`后仍然有效； 一条`on`命令恢复保护。

```bash
memory-supervisor off
memory-supervisor on
```

`off` 拒绝搁置supervisor 管理的暂停 PID 或正在进行的流程操作； 首先解析列出的 PID。 如果服务在无意中停止`off`，hooks 仍会在十秒后丢弃其过时的决策，并警告**保护不可用**。

```bash
memory-status --connections
memory-supervisor update
```

如果您需要固定限制，您可以选择为该本地环境中的所有 Claude Code 和 Codex 程序设置一个总内存上限：

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

命令按其控制的内容进行分组：

- `memory-status` 命令是只读的：本地原因、federation、服务、hook 和通知连接。
- `on` 和 `off` 控制整个当前安装。 一条命令涵盖每个已连接的Claude Code和Codex会话； 必须在该环境内切换另一个操作系统、WSL 发行版或 VM。
- `resume` 继续由supervisor 暂停的进程。 `terminate` 和 `kill` 是操作员在检查原因后选择的进程退出。
- `budget` 仅对当前环境中的 Claude Code 和 Codex 应用可选上限，而不是对整个计算机或 Chrome。
- `update` 重新应用服务并检测到 CLI 连接。 `notifications` 控制可选的操作系统、Discord 和 Telegram 路由； lead hooks 和确切的终端通知仍然是强制性的。

<a id="common-commands"></a>
## 常用命令

| 命令 | 目的 |
| --- | --- |
| `memory-status` | 当地健康状况、原因和下一步行动 |
| `memory-status --all` | 同一计算机上的 Windows、WSL、虚拟机和容器状态 |
| `memory-status --connections` | 后台服务、AI CLI 和通知连接 |
| `memory-supervisor on` / `off` | 在此环境中持续启用或禁用保护； 已连接 hooks 关闭时通过 |
| `memory-supervisor update` | 更新并重新连接检测到的 CLI |
| `memory-supervisor budget` | 显示此环境中的适应能力和任何可选上限 |
| `memory-supervisor budget set <GiB>` / `budget off` | 设置或删除聚合本地 Claude Code 和 Codex 上限 |
| `memory-supervisor resume [pid]` | 恢复supervisor暂停的进程； 仅当恰好有一个暂停时才忽略 PID |
| `memory-supervisor terminate <pid>` | 优雅地终止一个经过验证的托管进程 |
| `memory-supervisor kill <pid>` | 作为最后手段强制终止一个经过验证的进程 |
| `memory-supervisor notifications show` | 显示隐藏秘密的通知设置 |
| `内存-supervisor通知路由<全部\|没有任何\|路线>` | 选择可选的操作系统、Discord 和 Telegram 路由 |
| `memory-supervisor notifications test` | 测试启用的可选通知路由 |
| `memory-supervisor uninstall` | 删除其服务和 AI CLI 连接，同时保留状态 |

<a id="verification"></a>
## 确认

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust 单元、集成和安装程序测试涵盖策略、流程安全、Claude Code 和 Codex 接线、federation、恢复和发布捆绑包。 GitHub Actions 检查 Linux x86-64、Windows x86-64、Apple Silicon macOS 和 Rosetta 下的 macOS x86-64 上的构建和平台合同。 有界物理机验证加上确定性模拟涵盖了真正的接近耗尽边界。 请参阅[测试覆盖率](../testing/test-matrix.zh-CN.md)。
