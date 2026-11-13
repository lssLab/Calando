# 通知设置

<p align="center">
  <a href="notifications.md">English</a> · <a href="notifications.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="notifications.ja.md">日本語</a>
</p>

Telegram 是唯一的推送渠道。随时可以用 `calando status` 或会话内的 `/calando status` 查看状态。

<a id="always-on"></a>
## 始终开启的通知

以下三项无法关闭，也不需要设置：

- 代理的 hook 上下文：受影响会话的主代理在下一个 hook 边界收到一次原因和恢复状态。
- hook 在终端显示的一行消息。
- 写入被暂停进程所在终端的通知。暂停的主代理无法运行自己的 hook，所以这条通知会说明发生了什么、接下来会怎样。

终端通知只写入经过重新核对的那个 TTY 或 Windows 控制台，不注入输入，也不改变终端模式。全屏 TUI 可能重画一两行，下次重画时恢复。无法核对并写入该终端时，不会暂停主代理。

<a id="telegram-connect-a-bot-and-discover-its-chat"></a>
## 连接 Telegram

Calando 不运行接收 Telegram 命令的 Webhook 服务器，只通过 Bot API 的 `sendMessage` 发送消息。

1. 打开 `@BotFather`，用 `/newbot` 创建机器人并复制令牌。
2. 个人通知：打开与新机器人的对话。群组通知：把机器人加入群组。
3. 运行：

```bash
calando telegram
```

4. 在 `Telegram bot token from @BotFather (hidden):` 处粘贴令牌并按 Enter。输入不会显示，也不会进入 shell 历史。
5. 如果还没有聊天，命令会提示等待 120 秒。在此期间给机器人发一条新消息（或在目标群组里发一条）。
6. 只找到一个聊天时，命令会保存设置并发送测试消息。看到 `Telegram on. A test message was sent.` 即完成。

如果有多个聊天给这个机器人发过消息，命令会列出它们的 ID 和名称，不保存任何内容。选一个后重新运行，并再次粘贴令牌。群组 ID 通常是负数：

```bash
calando telegram -1001234567890
```

删除 Telegram 设置：

```bash
calando telegram off
```

| 错误 | 含义 | 处理 |
| --- | --- | --- |
| `HTTP 401` | 令牌无效或已撤销 | 从 `@BotFather` 复制当前令牌后重试 |
| `HTTP 409` | 该机器人已有 Webhook 或其他 `getUpdates` 客户端 | 为 Calando 使用专用机器人；现有 Webhook 不会被修改 |
| `No message reached the bot within 120 seconds` | 等待期间机器人没有收到新消息 | 重新运行，在提示等待后给同一个机器人发新消息 |
| `Saved, but the test message failed` | 设置已保存，但测试消息未送达 | 检查机器人和聊天后重试 |

发现聊天失败时不会保存任何内容。Calando 从不自动调用 `deleteWebhook`，以免破坏现有的机器人集成。

设置保存在 `~/.config/calando/notifications.conf`（Windows：`%USERPROFILE%\.config\calando\notifications.conf`），在 Unix 上权限为 `600`。正常使用不需要打开它。显式设置的 `MEMORY_SUPERVISOR_TELEGRAM_*` 环境变量会覆盖保存的值。更改在下一条通知时生效，无需重启服务或 AI 程序。

<a id="message-format"></a>
## 消息格式

消息为英文。第一行是状态，第二行是环境，之后每行一个 `Label: value`：

```text
Calando — Memory protection on
Windows (LAPTOP-1)
Free: 1.5 of 15.7 GB
WSL VM: holds 9.6 GB, uses 3.2 GB
Action: New heavy work is on hold. Running work continues.
```

- 环境示例：`Windows (LAPTOP-1)`、`WSL (Ubuntu-LAPTOP-1)`。
- 可能出现的字段：`Free`、`WSL VM: holds X GB, uses Y GB`（Windows）、`Restricted agents`、`Paused processes`、`Duration`、`Action`。

<a id="when-notifications-are-sent"></a>
## 何时发送

- 保护措施首次生效时一条，所有条件在稳定确认时间内清除后一条恢复消息。
- 进程暂停和恢复。
- 之前正常的 federation 对等环境失联，以及之后的恢复。
- hooks 在没有运行中 daemon 的情况下放行时，有频率限制的**保护不可用**警告。
- 需要处理的故障，例如传感器或运行时降级、受保护的恢复失败。
- 有可用更新时（自动更新关闭，或 Codex 需要重新信任 hooks），每个版本一条 Telegram 消息。

不引起任何措施的原始颜色变化、未确认的泄漏迹象和不变的状态不会生成通知，只留在本地事件记录中。

通知有范围：进程相关的通知只发给拥有该进程的会话。其他会话只以数量和代理内存总量出现，不显示名称、路径或 PID。

权威的事件记录是本地的 `runtime.json` 和 `state.json`。Telegram 是尽力而为的副本，发送失败不会影响检测或保护。
