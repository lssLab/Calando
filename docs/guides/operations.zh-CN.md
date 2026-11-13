# 运行、通知与恢复

<p align="center">
  <a href="operations.md">English</a> · <a href="operations.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="operations.ja.md">日本語</a>
</p>

<a id="notifications"></a>
## 通知

Calando 不会为每次内存读数发通知。只有真正的保护措施开始或完全恢复时，或者连接与保护状态需要用户处理时，才发出一条通知。

| 去向 | 显示位置 | 内容 |
| --- | --- | --- |
| 代理的 hook 上下文 | 受影响会话的主代理 | 在下一个 hook 边界传递一次原因、当前范围和恢复状态。始终开启。 |
| 终端 | 运行受影响 Claude Code 或 Codex CLI 进程的那个终端 | hook 显示的一行消息，以及进程暂停或恢复时写入该终端的通知。始终开启。 |
| Telegram | 用户选择的机器人私聊或群组 | 重要措施的开始与恢复、连接或保护问题。可选，是唯一的推送渠道。 |

推送只通过 Telegram。随时可以用 `calando status`（终端）或 `/calando status`（会话内）查看状态。

```bash
calando telegram
calando telegram off
```

令牌在隐藏提示中输入，不要写在命令行上。设置方法、消息格式和故障排查见[通知设置](notifications.zh-CN.md)。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## Claude Code 和 Codex 中的命令

安装程序连接两部分：自动判断的 **hooks**，以及一个名为 `calando` 的**技能**。技能以复制方式安装到 `~/.claude/skills`、`~/.agents/skills` 和 `~/.codex/skills`。旧的 `memory-status` 命令文件会被删除。

会话命令只作用于输入它的那个会话：

```text
/calando on | off | status | help        (Codex CLI 和 App：$calando ...)
/calando cap <GB> | off                  高级；第一次回复先说明，用 `/calando cap <GB> yes` 应用
```

- `UserPromptSubmit` hook 直接回答这些命令，提示不会发给模型，不消耗 token 和上下文。
- 目标是输入命令的会话（取自 hook 负载中的会话 ID），不需要 PID 或会话列表。
- 会话 `off`：该会话仍被测量，但不会被暂缓、暂停或收到通知。
- 会话 `cap`：限制该会话自己的进程树。接近 90% 时暂缓其高内存启动；超过上限时暂停一个正在增长的子代理，会话回落到 90% 以下后恢复。
- `/calando status` 显示该会话的代理看到的内容；有更新时附加一行更新提示。

终端里的 `calando ...` 作用于整个环境，见下文。详细差异见 [Claude Code 指南](usage-claude.zh-CN.md)和 [Codex 指南](usage-codex.zh-CN.md)。

<a id="security"></a>
## 安全

Calando 读取操作系统的内存和进程信息，以及 Claude Code 和 Codex hooks 提供的会话、代理、工具、工作目录、连接状态和命令前缀。这些信息只用于判断能否开始新工作，以及确定准确的控制目标。

自动控制只做两件事：暂缓之后的 Claude Code 或 Codex 工作；在最后阶段暂停并恢复一个经过核对的本地进程。它不会自动结束程序，也不控制无关程序。正常监控不访问外部网络；只有 GitHub 安装与更新、用户开启的 Telegram 会使用网络。

它不把 hook 负载中可能出现的提示、对话、模型回复或文件内容用于控制，也不保留它们；不直接打开项目文件或进程内存；不读取或修改浏览器与 IDE 内部数据、Claude 或 ChatGPT 凭据，以及操作系统内核、内存、交换区和防火墙设置。完整列表见[安全和数据/控制边界](security.zh-CN.md)。

<a id="control-and-recovery"></a>
## 控制与恢复

没有手动恢复命令。

- 因内存压力暂停的进程，在内存恢复后自动恢复，一次一个。
- 因自身失控增长而暂停的主代理，会获得一次受保护的自动恢复。
- 如果主代理仍处于暂停：关闭那个终端，用 CLI 自带的恢复功能继续（`claude --resume`、`codex resume`）；或者运行 `calando off`，它会就地恢复该进程。
- 通过 CLI 恢复后，会话的第一个 hook 会说明上一个进程曾被暂停并已结束，那次运行中子代理的结果可能缺失。
- Codex App：受保护的恢复失败后，共享的 App Server 不会一直停在暂停状态，而是退回到按对话限制。

主代理暂停非常少见。它是**最后的保护阶段**：只有在暂缓新工作、限制子代理和工具之后风险仍在，并且已确认同一个主代理及其终端持续增长时才会使用。

<a id="on-off-cap-update-lock"></a>
### 开关、上限、更新和锁

```bash
calando off
calando on
```

`off` 先恢复 Calando 暂停的所有进程，再停止并禁用后台服务。已安装的 hooks 保持连接，但静默放行。这一选择在重启和更新后保持不变，`on` 恢复保护。如果服务意外停止，hooks 会在十秒后丢弃过期判断并提示**保护不可用**。

```bash
calando cap 6
calando cap off
```

`calando cap` 是高级功能，为本环境中所有代理设置硬上限。它会先打印说明并询问 `[y/N]`，非交互调用可加 `--yes`。请求会对照本环境当前可用的内存校验。接近上限时暂缓新的高内存工作；超过上限时暂停一个正在增长的子代理。暂停无法收回已经占用的内存，使用量可能短暂超过上限；需要精确到字节的配额时，请用 cgroup、容器或 VM 限制。正常使用不建议开启，它适合严格的研究或基准测试。

```bash
calando update
calando update auto off
calando update auto on
```

已安装的服务每天在后台检查一次 GitHub 最新版本，在没有暂停进程且内存平稳时，在自身服务之外执行更新；忙时每小时重试。

- Claude Code：自动应用，包括 hook 变更，新会话会使用新的 hooks。
- Codex：如果新版本改变了 hook 定义（次版本，如 0.3 → 0.4），并且 Codex 已连接，则**不会**自动应用。因为 Codex 会静默跳过变更过的 hook，直到用户在 CLI 的 `/hooks` 或 App 的**设置 → Hooks** 中信任它们。状态输出和一条 Telegram 消息会提示运行 `calando update` 并信任 hooks。补丁版本不会改变 hooks。
- 关闭自动更新后，`calando status`、`/calando status` 以及每个版本一条 Telegram 消息会提示有更新。代理的 hooks 从不提及更新。
- 每日检查也会在 Claude Code 从 `settings.json` 中丢失 Calando 的 hooks 时把它们恢复。
- 从 0.2.x 升级到 0.3.0 会改变 hook 命令，Codex 用户需要在 `/hooks`（CLI）或**设置 → Hooks**（App）中重新信任一次 Calando hooks。

```bash
calando lock on
calando lock off
```

`lock` 只能由管理员运行。开启后，终端和会话中的 `off` 以及 `uninstall` 都会被拒绝。锁文件是 `/etc/calando/lock` 或 `%ProgramData%\Calando\lock`。它是策略文件，不是沙箱，边界见[安全](security.zh-CN.md)。

<a id="common-commands"></a>
## 常用命令

每个环境（Windows、每个 WSL 发行版、VM、容器）分别执行。

| 命令 | 作用 |
| --- | --- |
| `calando status [--json]` | 本环境、CLI 连接以及其他所有环境的状态 |
| `calando on` / `off` | 开启或关闭保护；`off` 先恢复 Calando 暂停的所有进程 |
| `calando telegram [CHAT_ID]` | 询问机器人令牌（隐藏输入），找到聊天并发送测试消息 |
| `calando telegram off` | 删除 Telegram 设置 |
| `calando cap [<GB> \| off]` | 高级：本环境所有代理的硬上限，需要确认 |
| `calando update` | 立即更新 |
| `calando update auto on` / `off` | 自动更新（默认开启） |
| `calando uninstall` | 删除程序，保留设置和状态 |
| `calando lock on` / `off` | 仅管理员；开启期间拒绝 `off` 和 `uninstall` |
| `calando help` | 显示帮助 |

`memory-supervisor` 和 `memory-status` 在 0.3.x 期间仍可作为命令别名使用。

<a id="verification"></a>
## 验证

```bash
bash tests/run.sh
```

```powershell
powershell -File .\tests\run.ps1
```

Rust 单元、集成和安装程序测试覆盖策略、进程安全、Claude Code 与 Codex 接线、federation、恢复和发布包。GitHub Actions 在 Linux x86-64、Windows x86-64、Apple Silicon macOS 和 Rosetta 下的 macOS x86-64 上检查构建与平台约定。真实的接近耗尽边界由有界的实机验证加确定性模拟覆盖。见[测试覆盖范围](../testing/test-matrix.zh-CN.md)。
