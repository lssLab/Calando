# Codex 用法

<p align="center">
  <a href="usage-codex.md">English</a> · <a href="usage-codex.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="usage-codex.ja.md">日本語</a>
</p>

<a id="supported-contract"></a>
## 支持合约

当此命令报告 hook 功能稳定且已启用时，Memory Supervisor 支持 Codex CLI **0.145.0 或更高版本**：

```bash
codex --version
codex features list | grep '^hooks'
codex update
```

安装程序运行相同的检查。 不受支持或禁用 - hook 版本没有 Memory Supervisor Codex hook； 以前拥有的hook会在重新安装时被删除，因此降级无法默默地要求保护。 如果 `codex update` 对于原始安装方法不可用，请使用相同的软件包管理器或安装程序升级 Codex，然后运行 ​​`memory-supervisor update`。

目前Codexhooks可以通过`PreToolUse`观察本地功能工具。 版本 0.145.0 为线程生成 hook 输入提供可选的 subagent 标识，并在根线程上忽略它。 安装的匹配器故意广泛，因此输入感知分类可以保留普通工作：

```text
.*
```

在自适应admission`GREEN`或`YELLOW`，新的扩展是静默的并且允许的。 在`ORANGE`或`RED`，只有实际的扩展调用会短暂等待恢复，然后使用`ADMISSION_DEFERRED`返回有效的拒绝决策。 逻辑child状态只能单独拒绝其分类的未来扩展、高内存启动、广泛发现或生产工作类。 原始利用颜色可以保持红色，而自适应admission可以稳定工作； 结果、消息、状态、停止/取消和恢复在每种状态下均保持可用。

官方合同参考：

- [Codex hooks 和工具覆盖范围](https://learn.chatgpt.com/docs/hooks)
- [Codex高级配置](https://developers.openai.com/codex/config-advanced#hooks)

<a id="installation-and-trust"></a>
## 安装和信任

运行自述文件的一行安装程序。 如果supervisor已经存在并且Codex是稍后安装的，则运行`memory-supervisor update`。 然后使用普通的Codex命令：

```bash
codex
codex exec "your task"
```

当`CODEX_HOME`设置时，hook合并到`$CODEX_HOME/hooks.json`，否则`~/.codex/hooks.json`； 不相关的组将被保留，并且在原子替换之前备份先前的文件。 当非托管 hook 是新的或其定义发生变化时，Codex 需要审查和信任。 用户必须亲自在交互式 CLI 会话中打开`/hooks`，查看并信任确切的当前命令，并启用任何禁用的条目。 安装程序永远不会自动执行用户决定，并且重新启动Codex永远不会替代它。

该`/hooks`操作属于Codex CLI。 在Codex Desktop App中，用户在**设置→Hooks**中做出相同的手动决定。 在那里应用启用或信任更改会重新加载共享App Server已加载的每个任务的hook配置； 继续执行现有任务并执行下一个请求。 它不需要新任务或应用程序重新启动，尽管已经发生的`SessionStart`不会重播。 相反，CLI `/hooks` 保存会刷新该 CLI 进程，但不会刷新单独运行的桌面应用程序，并且应用程序设置保存不会刷新其他正在运行的 CLI 进程。 如果两个表面都已在运行，请首先在应用程序设置中保存批准，然后在工作后仅重新启动预先存在的 CLI 进程。 如果批准是由其他进程写入的，并且当前表面没有要保存的实际更改，请重新启动已运行的应用程序或 CLI 一次，以便它读取共享信任记录。

Memory Supervisor将自有的七类事件视为一份完整的连接合约。`memory-status --connections`会检查每类事件的定义、启用状态和当前信任哈希。因此，只要有项目缺失、重复、禁用、未受信任或发生变更，Codex就不能被报告为`CONNECTED`，Codex App路由也不能仅凭另一类事件留下了近期记录就被报告为`ACTIVE`。定义缺失或过期时运行`memory-supervisor update`；需要启用或信任项目时使用`/hooks`。成功触发`SessionStart`时也会执行同样的检查，并向lead和用户说明还有哪些项目未完成以及下一步该怎么做。

每运行一次 `memory-supervisor update` 之后运行 `memory-status --connections`。 Codex 根据当前的 hook 定义存储信任，而不是针对 Supervisor 版本号。 因此，同一命令后面的仅二进制更新不需要重新批准。 如果安装程序更改了命令、匹配器或其他哈希字段，Codex 会报告新定义以供审核，并且用户必须在受影响的 CLI 或应用程序界面上再次信任它。 流程重新启动可能会重新加载共享相同`CODEX_HOME`的不同流程已保存的批准； 它永远无法创造那种认可。

每个生成的 Codex 命令还包含其所属的绝对 `hooks.json` 源。 门将该源与当前进程的`CODEX_HOME`进行比较。 当另一个 Codex 家庭将其重新发现为项目 hook 时，这可以防止来自一个环境的用户 hook 再次执行操作。 当没有安装其他操作系统路由时，其命令字段是有效的无操作而不是跨 shell 错误。 有意共享的 Windows/WSL `CODEX_HOME` 为每个保留现有的本机路由； 每个本地supervisor仅审核和控制自己的路由和PID空间。 Federation 仍然只共享新鲜的 admission 状态，从不共享 hook 所有权或跨环境 PID 权限。

用户级别hooks独立于项目信任而应用。 项目本地 `.codex` hook 层在不受信任的存储库中被忽略，但此安装程序不依赖于项目本地层。 Codex 合并来自所有可信来源的匹配 hooks，这就是为什么上面的源防护是已安装路由的一部分而不是仅文档警告的原因。

<a id="hook-events"></a>
## Hook 事件

| 事件 | 目的 |
| --- | --- |
| `SessionStart` | 在恢复、清除或压缩时注入启动合约和尚未传递的事件 |
| `UserPromptSubmit` | 警告压力或未见的暂停/恢复事件 |
| `PreToolUse` | 对本地功能工具进行分类； 仅拒绝机器admission下的新扇出或被精确逻辑代理状态排除的工作类 |
| `SubagentStart` / `SubagentStop` | 观察生命周期； 开始仅添加相同的十二秒红色后备 |
| `PostToolUse` | 交付未见过的事件版本，而不延迟已完成的工作 |
| `Stop` | 关闭当前逻辑生命周期记录而不阻塞退出 |

ORANGE 绝不会在 `SubagentStart` 延迟已录取的 worker。 Codex没有后工具协同睡眠； 红色压力由预生成admission 和独立的 PID 逆止器处理。 安装的命令 hooks 有意省略可选的 `statusMessage`，因此例程前/后 hook 执行不会在 TUI 中留下 Memory Supervisor 微调器文本。 仅针对真实操作或看不见的事件返回可见消息。 如果现有会话仍显示持久的 `Running PreToolUse/PostToolUse hook` 行，请使用 `memory-supervisor update` 重新申请，如果哈希值已更改，请检查 `/hooks`，然后关闭 `/hooks` 并继续该 CLI 会话。 仅重新启动已打开且未收到该进程本地重新加载的其他 CLI 进程。

所有命令包装器都无法打开。 缺失 daemon、陈旧状态、格式错误的 hook 输入或 Rust 门故障不会产生拒绝决策。 操作系统daemon仍然是独立的后盾。

Codex的hook信任以哈希为依据。重新安装变更后的hook命令会使其重新进入待审核状态；请在受影响的CLI进程中使用`/hooks`检查并信任准确的定义。保存后，由同一CLI进程承载的会话会重新加载；只有需要重新触发`SessionStart`时才需要新会话。重启supervisor daemon不会重启Codex。暂停的Codex lead恢复后，准确的目标终端通知和OS/远程适配器会把同一进程持续实际增长的直接证据，与单独估算的`agent|mixed|external|unknown`整机归因区分开，并说明接下来会发生什么。hook会在下一个提示或工具结束边界再传递一次同一安全事件，但该时点不保证与OS层面的恢复完全一致。如果Codex本身重启，请使用Codex的会话恢复功能。Codex会在新进程中恢复历史记录，已安装的`SessionStart`hook则会自动注入一次尚未传递的资源事件和当前supervisor判断。`runtime.json`只保存资源事件，不会替代Codex自身的历史记录机制。

<a id="verification"></a>
## 确认

存储库验证检查：

```bash
bash tests/run.sh
memory-status --connections
```

`tests/native_codex.rs` 另外使用官方二进制文件来验证一次性 Codex 进程上的检测和往返本机挂起/恢复。 使用以下命令运行选择加入的金丝雀：

```bash
MEMORY_SUPERVISOR_NATIVE_CODEX_SMOKE=1 \
  cargo +1.88.0 test --test native_codex -- --nocapture
```

其余的 Rust 集成测试验证最低版本和功能报告、已安装的 hook 形状、ORANGE `Agent` 拒绝、准确的终端定位以及格式错误/过时的故障打开案例。 它们不会启动 App Server 或需要经过模型验证的代理生成。

自动检查固定受支持的最低 Codex 版本和 hook 合约，以便在构建公共可执行文件之前检测到功能状态或命令形状更改。
