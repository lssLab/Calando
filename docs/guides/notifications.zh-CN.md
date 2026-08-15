# 通知设置 - 在终端中完成而不打开文件

<p align="center">
  <a href="notifications.md">English</a> · <a href="notifications.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="notifications.ja.md">日本語</a>
</p>

安装 Memory Supervisor 后，相同的 `memory-supervisor notifications ...` 命令可在 Linux、WSL、macOS 和 Windows PowerShell 中运行。 您不需要查找设置文件或手动键入变量名称和引用。 路由和凭证更改在下一个通知事件时生效； daemon 和 Claude Code 或 Codex CLI 不需要重新启动。

请勿将 Discord Webhook URL 或 Discord 或 Telegram 机器人令牌放在命令行上。 启动命令后将其粘贴到`(hidden)`提示符下。 该值不会回显，也不会进入 shell 历史记录。 Memory Supervisor 以原子方式将其保存在每个操作系统的私有文件中，在 Unix 上使用模式 `600`。

<a id="check-the-current-state-first"></a>
## 首先检查当前状态

将这一行复制到任何终端中：

```bash
memory-supervisor notifications show
```

输出显示已启用的路由、选定的 Discord 方法和 Telegram 聊天，但从不打印原始 Webhook 或令牌。

<a id="enable-or-disable-optional-routes"></a>
## 启用或禁用可选路由

通知始终**仅涵盖真正的保护措施**。 绿色/黄色/橙色/红色转换和未经确认的泄漏观察结果保留在`memory-status`和事件分类账中。 以下命令选择相同操作通知的发送位置； 他们不会改变其细节级别。

`hook`是主代理人的意识和恢复合同。 当暂停的 lead 无法运行自己的 hook 时，`terminal` 会下发准确的恢复命令。 两者都是强制性的，不能通过命令、配置文件或环境变量禁用。 只有`os,discord,telegram`是可选的。

启用每条可选路线：

```bash
memory-supervisor notifications routes all
```

仅将本机操作系统通知添加到强制 hook 和终端路由：

```bash
memory-supervisor notifications routes os
```

使用操作系统通知和 Discord：

```bash
memory-supervisor notifications routes os,discord
```

禁用所有可选路线，同时保留 hook 和终端交付：

```bash
memory-supervisor notifications routes none
```

有效的可选名称是`os,discord,telegram`。 拒绝提供 `hook` 或 `terminal`，因为这些路由是强制性的。 Discord 或 Telegram 设置命令会自动将其路由添加到当前选择。 在没有相应凭据的情况下选择`all`只会跳过未配置的远程路由。

并非每次颜色更改时都会打印终端通知。 对于诸如 lead 暂停、恢复或试用等实际操作，supervisor 会重新验证目标 PID 的确切 TTY 或 Windows 控制台，并写入一个纯文本通知。 它不会注入输入或更改终端模式。 全屏 TUI 可能会重画一两条线，并在下一次重画时恢复其视图。 如果无法验证和写入确切的终端，supervisor 不会让 lead 暂停。 被拒绝的 AI CLI 工具调用也会直接将其原因返回到lead。

操作系统路由在 Linux 上使用 `notify-send`，来自 WSL 的 Windows 主机通知路径，在 macOS 上使用`osascript`，在 Windows 上使用 NotifyIcon。

<a id="discord-a-connect-a-webhook-recommended"></a>
## Discord A — 连接 webhook（推荐）

这是最简单的选项，因为它不需要机器人。

1. 在 Discord 桌面或 Web 中打开目标服务器文本通道。
2. 选择**编辑渠道 → 集成 → Webhook → 新 Webhook**。
3. 确认名称和目标渠道，然后选择**复制 Webhook URL**。
4. 跑步：

```bash
memory-supervisor notifications discord-webhook
```

5. 在`Discord webhook URL (hidden):`处，粘贴 URL 并按 Enter。 预计没有可见字符。
6. 测试连接：

```bash
memory-supervisor notifications test
```

当命令打印`discord: delivered`并且通道收到测试消息时，设置完成。 设置命令启用 Discord 并取代任何以前的 Discord 交付方法。

Webhook URL 是一个可以写入其通道的秘密。 如果发生泄漏，请在 Discord 中删除该 Webhook，创建一个新 Webhook，然后再次运行安装命令。

<a id="discord-b-send-to-a-channel-through-an-existing-bot"></a>
## Discord B — 通过现有机器人发送到频道

仅当您已经操作 Discord 机器人时才使用此功能。

1. 从 Discord 开发者门户获取其令牌，邀请其访问服务器，并在目标频道中授予**发送消息** 权限。
2. 启用**用户设置→高级→开发者模式**。
3. 右键单击目标通道并选择**复制通道 ID**。
4. 将下面的数字替换为该通道 ID：

```bash
memory-supervisor notifications discord-channel 123456789012345678
```

5. 将令牌粘贴到`Discord bot token (hidden):`处，按Enter键，然后测试：

```bash
memory-supervisor notifications test
```

不要向令牌添加 `Bot ` 前缀。 Memory Supervisor 将其添加到 API 请求中。

<a id="discord-c-send-a-direct-message-through-an-existing-bot"></a>
## Discord C — 通过现有机器人发送直接消息

您必须与机器人共享服务器并允许来自该服务器的 DM。

1. 启用 Discord 开发者模式，右键单击您的个人资料，然后选择**复制用户 ID**。
2. 将下面的数字替换为您的用户 ID：

```bash
memory-supervisor notifications discord-dm 123456789012345678
```

3. 将机器人令牌粘贴到隐藏提示符处并测试：

```bash
memory-supervisor notifications test
```

在第一次发送时，机器人会创建一个 DM 通道并仅在本地缓存该通道 ID。

删除 Discord 凭据并用一行禁用其路由：

```bash
memory-supervisor notifications disable-discord
```

<a id="telegram-connect-a-bot-and-discover-its-chat"></a>
## Telegram — 连接机器人并发现其聊天

Memory Supervisor 不会创建接受 Telegram 命令的公共 Webhook 服务器。 它仅通过 Bot API `sendMessage` 方法发送通知。

1. 打开`@BotFather`，使用`/newbot`创建一个机器人，并复制其令牌。
2. 对于个人警报，请打开新机器人的对话。 对于组警报，将其添加到组中。
3. 跑步：

```bash
memory-supervisor notifications telegram
```

4. 将令牌粘贴到`Telegram bot token (hidden):`，然后按 Enter。 该命令首先检查挂起的更新。 如果不存在，则打印`waiting 120 seconds`； 在等待期间，向该机器人发送新的 `/start` 或消息，或目标组中的新消息。 当恰好出现一个聊天时，该命令会保存其 ID 并启用 Telegram。
5. 测试连接：

```bash
memory-supervisor notifications test
```

当命令打印`telegram: delivered`并且 Telegram 收到测试时，设置完成。

如果机器人的更新中显示多个聊天，该命令会列出它们的 ID 和标签，而不保存任何内容。 选择一个并使用其 ID 重新运行； 组 ID 通常为负数：

```bash
memory-supervisor notifications telegram -1001234567890
```

再次粘贴相同的标记。 如果 120 秒内没有出现任何聊天，请重新运行该命令，并在出现等待消息后向与该令牌配对的确切机器人发送一条新消息。 不要假设可以再次读取旧的`/start`。

发现错误单独报告：

| 错误 | 意义 | 行动 |
| --- | --- | --- |
| `HTTP 401` | BotFather 令牌无效或已撤销 | 从`@BotFather`复制当前令牌并重新运行 |
| `HTTP 409` | 该机器人已经有一个 webhook 或另一个 `getUpdates` 消费者 | 使用专用的Memory Supervisor机器人； 现有集成不会自动删除 |
| `connection failed or timed out` | Telegram API 网络连接失败 | 检查互联网、防火墙和代理，然后重新运行 |
| `No Telegram update arrived within 120 seconds` | 确切的机器人或组没有收到新的更新 | 在命令等待期间发送新的 `/start` 或消息 |

失败时，不会保存令牌和聊天 ID。 Memory Supervisor 永远不会自动调用 `deleteWebhook`，因为这可能会破坏现有的机器人集成。

删除 Telegram 凭据并禁用其路由：

```bash
memory-supervisor notifications disable-telegram
```

<a id="verify-connections-and-read-test-results"></a>
## 验证连接并读取测试结果

显示当前配置：

```bash
memory-supervisor notifications show
```

通过启用的操作系统路由和配置的远程路由发送测试：

```bash
memory-supervisor notifications test
```

| 结果 | 意义 | 下一步行动 |
| --- | --- | --- |
| `delivered` | 该路线已接受测试 | 完毕 |
| `disabled` | 路线未选择 | 如果需要，可以添加 `routes ...` |
| `not configured` | 路由已启用但凭据不完整 | 运行上面的 Discord 或 Telegram 设置命令 |
| `unavailable` | 此 GUI/会话中没有可用的操作系统通知传输 | 使用桌面会话或远程路由 |
| `failed` | API、权限或网络错误 | 检查token、ID、权限、网络，然后重新配置并测试 |

`hook`和`terminal`需要真实的AI CLIhook或真实保护操作的确切目标，因此测试命令不会为它们合成消息。 `memory-status --connections` 报告daemon、hook 和选定路线接线。 `memory-status`记录每个真实事件的`delivered|failed|skipped|unavailable`结果。

正常使用不需要打开备份文件：

| 环境 | 私人内部位置 |
| --- | --- |
| Linux、WSL、macOS | `~/.config/memory-supervisor/notifications.conf` |
| 视窗 | `$HOME\.config\memory-supervisor\notifications.conf` |

任何显式设置的 `MEMORY_SUPERVISOR_NOTIFICATION_*` 环境变量都会覆盖保存的值。 `show` 和设置命令会警告这些覆盖名称； 如果保存的更改未生效，请先取消设置。

<a id="when-notifications-are-sent"></a>
## 发送通知时

- 当任何 `HOLD|DRAIN`、实时逻辑限制、托管停止 PID 或 lead 试用期首次激活时，一个 `pressure-episode / active`
- 所有这些条件清除后，最后一个`recovered`，或者当停止的worker在确认恢复之前消失时`ended-with-loss`
- 精确终端PID暂停/恢复安全注意事项
- 之前新鲜的federation对等点变得陈旧，以及后来的恢复
- 当 hooks 在没有实时 daemon 的情况下打开失败时，出现速率限制保护不可用的警告
- 需要采取措施的故障，例如传感器/运行时/通知保护降级或试用失败

未引起操作的原始利用率转换和泄漏嫌疑仅保留在事件分类帐中。 普通 `SessionStart/End`、`SubagentStart/Stop`、稳定的 `ACTIVE` 状态和未更改的 `HOLD/DRAIN` 勾号不会创建另一个用户通知。 生命周期库存永远不会自行推进用户可见的逻辑控制时代。 内部生成拒绝、worker启动延迟、逻辑缓冲、每个 PID 暂停/恢复事件以及正常试用阶段也是`importance=detail`。 被拒绝的hook仍然将其`systemMessage`直接返回给lead； 它不会将相同的事实克隆到另一个 Discord、Telegram 或操作系统消息中。

分界线是意图，而不是事件名称：如果 supervisor 将证据转化为针对 lead 的明确的主动意识指令，则该指令是用户可见的操作并且被传递一次。 传感器样本或不要求任何人执行任何操作的未更改边界保留在分类帐中，并且不消耗模型上下文。

lead事件消息包括PID、直接进程或机器压力证据、单独估计的`agent|external|mixed|unknown`系统归因以及是否等待自动恢复或使用手动命令。 当暂停的 lead 无法运行其 hook 时，精确的终端和远程路由仍然有效。 立即尝试终端、操作系统和远程交付； 模型和lead意识到达下一个hook边界。 每次暂停、试用、成功/失败、手动恢复和外部恢复消息都会说明时间差异。 事件类型、状态、源和事件/会话时期抑制重复。 真正的恢复是一次新的转变，并且只交付一次； 仅仅保持稳定的边界是不稳定的。

Hooks、`memory-status`、精确终端、操作系统、Discord 和 Telegram 都通过相同的用户边界呈现结构化事件。 由旧版本编写的运行时记录也在那里标准化，因此更新后不会重播过时的调试文本，例如`Some(...)`。

与弹出窗口不同，当用户离开时，远程通道历史记录仍然可见。 权威的事件记录仍然是`runtime.json`和`state.json`中的本地通知账本； Discord 和 Telegram 是尽力而为的副本，其故障永远不会阻止检测或保护。
