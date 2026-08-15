# Claude Code 用法

<p align="center">
  <a href="usage-claude.md">English</a> · <a href="usage-claude.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="usage-claude.ja.md">日本語</a>
</p>

<a id="supported-contract"></a>
## 支持合约

Memory Supervisor `0.2.1` 支持 Claude Code **2.1.217 或更高版本**。 这是分级逻辑控制合约固定的最新支持的基线； 旧版本不会收到减少的匹配器集或兼容性策略。

```bash
claude --version
claude update
```

安装程序将版本和hook接线作为两个单独的事实进行检查。 它搜索活动的`PATH`和已知的用户安装位置，包括本机、NVM、fnm、asdf、Volta和Windows npm路径，然后使用它可以验证的最新支持的Claude Code。 因此，非登录进程的 `PATH` 上较早的可执行文件无法隐藏当前用户安装。

如果无法验证受支持的可执行文件，安装会报告版本问题，但保留任何现有的 Memory Supervisor hook。 失败的版本探测并不能证明应删除有效的hook。 `memory-status --connections` 继续单独显示 hook 运行状况，并且在版本和 hook 准备就绪之前不会调用受保护的提供程序。 如果 `claude update` 对于原始安装方法不可用，请使用相同的软件包管理器或安装程序升级 Claude Code，然后运行 ​​`memory-supervisor update`。

Claude Code 拥有最广泛的受支持 CLI 集成记录。 `PreToolUse` 观察所有刀具路径，对其实际输入进行分类，仅在机器 admission 上门控新扩展，并在存在时强制指定逻辑代理的未来工作缓冲。 它不会撤消已经启动的工具。

运行平台安装程序。 它以原子方式将这些事件合并到`~/.claude/settings.json`，而不替换不相关的hooks：

- `SessionStart`：启动时注入资源合约； resume/clear/compact 仅在没有未见的暂停事件时才保持沉默。
- `UserPromptSubmit`：注入非绿色自适应admission以及恢复后任何未见过的事件。
- `PreToolUse`：对每个工具进行分类； 在机器压力下延迟并交回新的扩展，在机器困难严重时保留新的分类高内存启动，或者仅拒绝目标逻辑状态排除的未来工作类。
- `SubagentStart`：生命周期观察加上 12 秒的仅 RED 回退； ORANGE 绝不会拖延已经录取的worker。
- `SubagentStop`：关闭逻辑生命周期记录，同时保留任何可能导致结果部分的supervisor拒绝。
- `PostToolUse`和`PostToolBatch`：记录进度。 lead 边界提供了看不见的事件上下文； subagent 边界不能消耗 lead 的事件光标。 两者都不添加固定的红色睡眠。
- `Stop`和`SessionEnd`：关闭lead/session生命周期状态，不阻塞正常退出。

<a id="hook-activation-workspace-trust-and-reload"></a>
## Hook 激活、工作区信任和重新加载

Claude Code 不使用 Codex 的 per-hook 哈希批准。 安装程序将 Memory Supervisor hook 写入位于 `~/.claude/settings.json` 的用户设置； 该用户hook没有单独的批准/启用步骤。 尽管如此，交互式 Claude Code 仍保留每个设置文件 hook，包括该用户 hook，直到用户接受当前文件夹或其父文件夹之一的工作区信任。 Claude 的 `/hooks` 屏幕是只读浏览器，无法授予该信任。

工作区信任是一个文件夹级别的决策，而不是特定于 Memory Supervisor 的 Hook 审查。 仅接受您信任的工作文件夹。 做出该决定后，当前的 Claude Code 会监视设置文件，因此正在运行的会话通常会获取稍后的用户 - hook 更改。 仅当短暂等待后条目未出现时才重新启动，或者当目标是专门执行每会话一次`SessionStart` 事件时打开新会话。

普通的非交互式 `claude -p` 运行会加载相同的用户设置和 hooks，因此 Memory Supervisor 无需额外的设置步骤即可覆盖它。 Claude Code 在此模式下跳过工作区信任验证。 如果添加了`--bare`，Claude Code会故意跳过所有hooks，并且Memory Supervisor无法监督该调用。

安装后或`memory-supervisor update`，运行`memory-status --connections`。 其 Claude `CONNECTED` 结果验证了受支持的可执行文件、技能和当前用户 - hook 连接。 需要时，使用 Claude 的只读 `/hooks` 视图确认 `User Settings` 下的条目。 这两项检查都不能证明工作区对当前文件夹的信任。 组织策略（例如仅托管 hooks 或 `disableAllHooks`）仍然可以阻止用户 hook，并且需要管理员操作。

安装的命令hooks故意省略了可选的`statusMessage`，因此正常的hook执行不会在TUI中保留Memory Supervisor进度线。 用户可见的文本仅在真正的保护操作或未发生的事件时出现。 如果已运行的会话仍显示旧例程 hook 进度线，请运行 `memory-supervisor update` 并打开新的 Claude Code 会话，以便 AI CLI 重新加载当前的 hook 定义。

Admission 使用来自 `MEMORY_SUPERVISOR_FEDERATION_DIR` 的最差的新自适应操作，因此主机、WSL 或 VM 中的压力会在各处保持新的 Claude 扇出，而进程暂停仍然是本地的。 单独使用原始颜色不会阻止扇出。

如果Claude lead处于`PAUSED_BY_SUPERVISOR`状态，暂停期间无法运行进程内hook。因此，supervisor会把原因和准确的恢复策略写入重新核对过的目标终端，并分别安排OS、Discord和Telegram通知。自动试运行、成功、失败、手动恢复和外部直接恢复都会收到对应阶段的同一套说明。随后，hook会在下一个提示或工具边界把这些信息向用户和模型各传递一次；这一时点可能晚于OS层面的实际恢复。`memory-supervisor resume`会继续使用同一个PID和内存中的会话。如果Claude已结束并通过`--resume`重新启动，`SessionStart source=resume`会传递资源事件，而会话本身由Claude的历史恢复机制单独还原。

`StructuredOutput` 和其他结果/消息/状态工具在 `HANDOFF_ONLY` 中仍然允许使用。 Supervisor 拒绝记录及其工具、原因、时间和逻辑纪元，然后在其下一个完成/提示边界处汇总到 lead。 作为普通成功工具结果字符串到达​​的特定于提供商的配额耗尽没有结构化失败信号，并且仍必须由 subagent 报告。

有意的决策是带有退出代码 0 的 JSON。稳定的包装器将 Rust 门、状态或策略失败转换为静默退出 0，因此内部故障不会意外地成为 Claude Code 的退出代码 2 提示块。

核实：

```bash
bash tests/run.sh
memory-status --connections
memory-status
printf '{}' | runtime/hooks/gate.sh SessionStart
```

合同参考：

- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code 权限和工作区信任](https://code.claude.com/docs/en/permissions)
- [Claude Code安装和更新](https://code.claude.com/docs/en/installation)
- [Claude Code安装疑难解答](https://code.claude.com/docs/en/troubleshoot-install)

在 Windows 上使用：

```powershell
'{}' | powershell -File .\runtime\hooks\gate.ps1 SessionStart
```

个人技能链接在`~/.claude/skills/memory-supervisor`。 新创建的顶级技能目录在发现之前可能需要新的Claude Code会话。

进程被暂停后，先按照通知中的操作说明处理。因内存压力而暂停的workers，以及确认持续实际增长后被暂停的lead，其第一次试运行恢复由系统自动完成。需要进行已授权的手动恢复时，请使用`memory-supervisor resume <pid>`（如果只有一个PID处于暂停状态，可直接使用`memory-supervisor resume`），不要直接执行`kill -CONT`。这样daemon才能核对进程启动身份、清除状态、保存`RESUMED`事件并应用恢复冷却时间。

<a id="if-a-hook-blocks-every-prompt"></a>
## 如果 hook 阻止每个提示

不要继续从被阻止的会话中编辑活动的 hook。 从单独的终端：

1. 备份`~/.claude/settings.json`和当前的supervisor结账。
2. 运行`printf '{}' | runtime/hooks/gate.sh UserPromptSubmit`； 安全结果是有效的 JSON 或无输出，退出代码始终为 0。
3. 运行`bash tests/run.sh`。
4. 运行 `memory-supervisor update` 以原子方式替换仅拥有的 supervisor hook 条目并重新加载服务。
5. 打开新的 Claude Code 会话，因为 hook 定义可能会在会话开始时创建快照。

如果在此过程后每个提示仍然被阻止，请使用`memory-status --connections`和门退出代码将hook接线故障与supervisor状态分开。
