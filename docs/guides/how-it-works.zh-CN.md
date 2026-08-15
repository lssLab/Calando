# Memory Supervisor 的工作原理

<p align="center">
  <a href="how-it-works.md">English</a> · <a href="how-it-works.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="how-it-works.ja.md">日本語</a>
</p>

<a id="how-does-it-work"></a>
## 它是如何运作的？

Memory Supervisor 不位于您和任一 CLI 之间。 您继续正常启动`claude`和`codex`，同时一个小后台程序监视它们的内存使用情况。

1. 每个**操作系统环境** 中运行一个后台监视器。 Windows、macOS 或 Linux 基础系统是一种环境，每个 WSL 发行版、虚拟机或隔离容器是另一种环境。 如果同时使用 Windows 和 WSL，则一台显示器在 Windows 中运行，另一台显示器在 WSL 中运行。 每个监视器都会测量可用内存、本机压力信号、短窗口和长窗口下降速度、预期近期增长以及可见的 Claude Code 和 Codex 程序。 一个环境中的多个终端共享相同的监视器和新工作决策。
2. 它通过进程表和hooks来区分终端，而不是窗口标题。 每个顶层Claude Code或Codex进程都是一个独立的lead； 后代被分组为 workers 和工具。 Hook 会话和代理 ID 与 PID 和进程启动标识一起记录，因此不同的会话或重用的 PID 不会被错误控制。
3. 它不是固定的利用率百分比，而是计算距当前房间的停车距离和消耗速度。 缓慢下降制动接近边界； 快速下降使得在相同的反应时间内停止所需的距离更长。
4. Claude Code 或 Codex hook 在新的 subagent、工作流、任务或占用大量内存的命令启动之前进行检查。 `ALLOW` 允许，`OBSERVE` 在观看时允许，`HOLD` 仅延迟创建新的 subagents、工作流和任务，`DRAIN` 阻止这些创建请求。
5. 每个 Windows、Linux 和 macOS 基础操作系统都运行自己的 Supervisor，每个 WSL 发行版、VM 或分层的进程隔离容器也是如此。 共享物理内存的环境使用federation仅交换最多 10 秒的新工作决策，应用最严格的决策。 每个Supervisor仍然只控制自己的hooks和PID空间，因此它无法暂停另一个环境中的程序。
6. 即使在`DRAIN`下，仅由 Chrome 或 IDE 造成的压力也不会暂停现有的 AI 工作。 对于人工智能造成的压力或操作员设置的内存上限，未来的工作范围将缩小到`ACTIVE → NO_EXPANSION → LIGHT_WORK_ONLY → HANDOFF_ONLY`。 它不会同时缩小每个会话的范围。 Subagents 按已验证的关联流程的持续增长、当前或最近的扩展/构建/测试工作、限制是否已经开始、较早到达风险边界以及较新的开始时间进行排名。 每个刻度仅适用剩余停止距离所需的最小梯级； 未选择的会话和飞行中的工作保持不变。 仅当有证据表明从 subagents 开始为时已晚时，lead 首先移动。
7. 如果仍然存在危险，supervisor会重新检查PID（操作系统的进程号）和启动标识，然后最多暂停一个本地程序。 lead 仅当其确切终端可写时才能暂停； 失败的通知会立即将暂停回滚。

经过持续改进后，代理功能以相反的顺序重新打开，暂停的程序一次恢复一个。 精确的计算和物理测量位于[自适应停止距离](../testing/stopping-distance.zh-CN.md)中。

`GREEN` 到 `RED` 颜色是快速状态显示。 新作品实际上由`ALLOW`、`OBSERVE`、`HOLD`或`DRAIN`控制； 仅颜色永远不会暂停程序。

<a id="what-changes-in-codex-desktop-app"></a>
### Codex Desktop App有什么变化？

在 CLI 中，每个会话都有自己的 lead 进程和 child 进程树，因此 supervisor 通常可以判断哪个会话正在增长。 Codex Desktop App 不会将所有对话合并到一个会话中。 相反，App Server 将每个对话保留为具有自己的 `session_id` 的**逻辑线程**。 这里，逻辑线程不是操作系统线程。 App Server和supervisor使用的会话标识。 supervisor将每个逻辑线程视为独立的lead，并使用`agent_id`将subagents附加到lead。 因此，可以根据对话管理代理列表、下一个hook工作范围、操作和恢复通知。

逻辑应用程序线程在物理上并不等同于 CLI 会话。 CLI 会话有自己的 lead PID、后代进程树和终端。 App逻辑线程没有独立的leadPID、完整的child进程树、终端或专用内存总量。 所有逻辑线程共享一个App Server PID 及其内存。 因此，操作系统总共显示一个App Server，无法测量每个会话的内存，并且无法仅暂停一个会话。 简而言之，对话在逻辑上是独立的，而进程和内存在物理上是共享的。

supervisor 计算一次共享的 App Server 内存。 仅当hook、任务之前捕获的进程列表、父child链和PID启动标识都一致时，单独启动的工具进程才属于特定逻辑线程。 App Server 内部使用的内存，或在多个对话重叠的工具中启动的 child 可能没有可证明的逻辑线程所有者。 这是**blind control**。 这并不意味着supervisor什么也看不见：它仍然知道系统headroom和下降速度、应用程序和child进程增长、活跃对话和当前工具类型。 只有一些生长物的主人未知。

在该限制内，应用程序控制器按以下顺序保留 CLI 策略：

1. **保持性能第一。**高但稳定的使用，或无法解释系统损失headroom的应用程序增长，不会触发应用程序归因的会话限制。 控制器将风险发生前的剩余时间与制动所需的时间进行比较，然后等待直到最近的安全点。 当较大份额的增长没有可证明的对话所有者时，控制器仅添加逐一尝试候选者并衡量结果所需的时间； 不确定性本身并不会导致早期限制。
2. **缓冲新的高内存应用程序首先启动。**如果持续的应用程序增长导致风险并进入计算的停止距离，但其对话所有者不清楚，则只有未来的高内存应用程序启动（例如构建和测试）在应用程序中等待。 正在运行的工作、结果、消息、状态和恢复仍然可用。
3. **仅缩小解释风险的最小集合。**当增长有明确的所有者时，控制者仅选择解释它所需的最少对话，并且通常将每个人的未来工作范围下移一个阶段。 当所有权不明确时，当前的重型工具、subagent角色和最近的活动对候选者进行排名。 它缩小了第一个blind candidate，然后重新测量，当增长放缓时停止，只有在危险持续存在时才转向另一个。 如果剩余时间太少，它仍然只批处理风险边界之前所需的最小集合。 估计的证据可以对候选人进行排名； 它从不授予暂停特定于对话的进程的权限。
4. **使用可用的最小物理制动。**在每个较小的逻辑操作失败后，订单是一个完全拥有且仍在增长的child进程，然后是一个仍在增长的已知属于应用程序但不属于特定对话的child，最后是共享的App Server。 服务器只能在每次活动对话后暂停，并且subagent已确认最后阶段，其中仅保留结果交付、状态和恢复等轻量工作。 服务器增长本身仍然是主要原因，不能留下任何较小的选择。 独立的恢复防护在有限的延迟后恢复该服务器。 暂停不会立即释放内存； 它会停止进一步增长，以便其他工作可以完成并且系统可以恢复。

恢复时会按相反顺序逐级重新开放。每个受影响的对话都会在下一次hook中收到原因和当前范围；如果措施针对blind child或共享App Server，则会通知所有可能受到影响的活动对话。如果App的hook路径断开，supervisor不会假装新的对话级限制或物理制动已经生效，而是把这些措施从可用保护手段中排除，报告保护能力下降，并继续进行系统级的新工作准入判断和App进程监测。

同时运行 CLI 和应用程序不会使任何一个脱离监管。 在一个 PID 空间（例如一个操作系统、WSL 发行版或 VM）内，单个 supervisor 监视 CLI 进程树和 Codex App 服务器。 它不会将它们合并到一个会话中。

- 每个 CLI 会话仍然是一个**独立的 lead，具有自己的终端、lead PID 和后代进程树**。
- App Server 不被视为一个终端。 它是**由多个会话共享的一个物理进程主机**。 它下面的每个`session_id`都是一个单独的逻辑lead，而服务器PID及其内部内存只计算一次。

两个表面共享相同的本地内存评估和新工作admission决策，但它们的控制目标保持独立。 应用程序属性 blind cushion 仅适用于应用程序 hook 调用，并且不会阻止普通 CLI 请求。 任一表面的记忆都会增加机器风险，但一个表面上的增长不会自动使另一个表面成为制动目标。 每个目标仍然需要自己的成长和归因证据。

Federation 加入竞争相同物理内存的 **supervisor 实例**； 它不会合并应用程序对话或终端。 Windows、每个 WSL 发行版、动态内存虚拟机和进程隔离容器仅使用不超过 10 秒的新工作决策，并应用最严格的新决策。 它们不会将另一个实例的会话列表或 PID 合并到本地控制中，并且每个 supervisor 仅控制其自己的 PID 空间中的 CLI 和 App 进程。 例如，WSL 中的应用程序引起的 `DRAIN` 可以使 Windows CLI 通过 federation 延迟新的 subagent 或大型任务，但 WSL supervisor 无法暂停 Windows CLI，而 Windows supervisor 无法暂停 WSL App Server。

<a id="how-are-terminals-and-agents-controlled"></a>
## 终端和代理如何控制？

<a id="1-claude-code-and-codex-cli"></a>
### 1. Claude Code 和 Codex CLI

在 CLI 路径中，Claude Code 和 Codex 保持直接连接到终端。 后台监视器监视同一本地进程空间中的相关程序。 控制分为两层：

- 开始前检查：允许或推迟尚未开始的工作。
- 程序暂停：如果危险持续存在，则通过操作系统仅暂停一个经过验证的 PID。

```text
A. User work path

                ┌──────────────────────┐
                │ Exact user terminal  │
                │ Commands and results │
                └──────────┬───────────┘
                           │ direct attachment
                           ▼
                ┌──────────────────────┐
                │ Claude / Codex lead  │
                │ Main agent           │
                └──────────┬───────────┘
                           │ before supported actions
                           ▼
                ┌──────────────────────┐
                │ Before-tool hook     │
                │ Reads latest decision│
                │ Returns reason       │
                └──────────┬───────────┘
                           │ decision
           ┌───────────────┴───────────────┐
           ▼                               ▼
┌──────────────────────┐        ┌──────────────────────┐
│ ALLOW / OBSERVE      │        │ HOLD / DRAIN         │
│ Requested work runs  │        │ Targeted work waits  │
│ No start is delayed  │        │ In-flight work stays │
└──────────────────────┘        └──────────────────────┘

B. Background protection path

┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ Measure/brake/recover│                │ Latest hook decision │
└──────────────────────┘                └──────────┬───────────┘                └──────────────────────┘
                                                   │ when protection acts
                               ┌───────────────────┴───────────────────┐
                               ▼                                       ▼
                    ┌──────────────────────┐                ┌──────────────────────┐
                    │ Notice + lead context│                │ One verified PID     │
                    │ Exact terminal: now  │                │ Final stage only     │
                    │ Lead: next hook once │                │ Pause + auto-resume  │
                    └──────────────────────┘                └──────────────────────┘

Windows, Linux, and macOS hosts with independent environments layered on top

                         ┌────────────────────────────────────┐
                         │ Shared federation decision         │
                         │ Shares new-work decisions only     │
                         │ Valid for 10 seconds               │
                         │ Strictest fresh decision wins      │
                         └─────────────────┬──────────────────┘
                                           ↕
                         only boundaries competing for shared RAM connect

       ┌────────────────────────────┐  ┌────────────────────────────┐  ┌────────────────────────────┐
       │ WSL distro / VM / container│  │ VM / container             │  │ VM / container             │
       │ each: local Supervisor     │  │ local Supervisor           │  │ local Supervisor           │
       └──────────────▲─────────────┘  └──────────────▲─────────────┘  └──────────────▲─────────────┘
                      │ runs on                       │ runs on                       │ runs on
       ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐  ┌──────────────┴─────────────┐
       │ Windows base OS            │  │ Linux base OS              │  │ macOS base OS              │
       │ host Supervisor            │  │ host Supervisor            │  │ host Supervisor            │
       └────────────────────────────┘  └────────────────────────────┘  └────────────────────────────┘

                  Each Supervisor controls only its own state, hooks, and PID space
                              No RAM pooling · no cross-environment PID control
```

CLI lead 认知遵循固定顺序：

1. supervisor首先在其事件账本中记录原因、目标、主动限制和恢复路径。
2. 延迟工作的hook会在同一调用中返回原因。
3. 物理过程操作立即显示在确切的终端中。 没有单独终端的Worker事件将在lead的下一个真实的hook处传送一次。
4. 如果选择，操作系统、Discord 和 Telegram 会收到一份保护启动通知和一份完全恢复通知。

例如，Windows上的Claude lead正在打包修改，而WSL中的Codex准备启动subagent和大型测试，此时二者共用的物理内存余量开始快速下降。WSL supervisor记录`DRAIN`后，federation会把该判断传给Windows。两边的hook只会暂缓新的subagent和测试；编辑、结果和消息仍可继续。如果原因是外部虚拟机，则不会暂停任何AI PID。状态持续恢复后，新工作会重新开放。只有确认同一AI worker持续增长，才会进一步收窄逻辑范围，并最终考虑精确暂停本地PID。

有关完整的状态流、多终端布局和故障边界，请参阅[架构和运行时拓扑](architecture.zh-CN.md)。

<a id="2-codex-desktop-app"></a>
### 2. Codex Desktop App

在Codex Desktop App中，每个对话都是由`session_id`标识的逻辑线程。不同的session ID会被视为独立的lead；同一个对话即使在多个窗口中打开，也仍只算一个逻辑线程和一个lead。这样，supervisor就能按对话管理hook层面的工作范围和通知。但它不会为每个线程创建独立的PID或内存池，因为所有线程仍共享同一个App Server。下图展示了逻辑对话与代理记录如何结合物理进程和内存观测，同时把所有权明确的目标与blind candidates分开。

```text
                                        ┌──────────────────────┐
                                        │ Codex Desktop App    │
                                        │ Logical App threads  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Shared App Server    │
                                        │ One PID + shared RAM │
                                        └──────────┬───────────┘
                                                   │ hooks + process view
                           ┌───────────────────────┴───────────────────────┐
                           ▼                                               ▼
                ┌──────────────────────┐                        ┌──────────────────────┐
                │ Conversation ledger  │                        │ Process + memory map │
                │ session ID = lead    │                        │ exact / blind pool   │
                │ agent ID = subagent  │                        │ Shared RAM once      │
                └──────────┬───────────┘                        └──────────┬───────────┘
                           └───────────────────────┬───────────────────────┘
                                                   ▼
┌──────────────────────┐                ┌──────────────────────┐                ┌──────────────────────┐
│ OS memory + processes│─── measure ───►│ Local Supervisor     │──── write ────►│ State + incidents    │
│ Headroom + decline   │                │ App-specific planner │                │ Hook-confirmed stage │
│ Sustained App growth │                │ Cause + braking room │                │ Recovery + notice    │
└──────────────────────┘                └──────────┬───────────┘                └──────────┬───────────┘
                                                   │                                       │
                                                   ▼                                       ▼
                                        ┌──────────────────────┐                ┌──────────────────────┐
                                        │ App staged cushion   │                │ Affected lead context│
                                        │ New heavy starts wait│                │ Scope + recovery     │
                                        │ Chosen sessions only │                └──────────────────────┘
                                        └──────────┬───────────┘
                                                   │ if danger persists
                                                   ▼
                                        ┌──────────────────────┐
                                        │ One subprocess PID   │
                                        │ Exact owner first    │
                                        │ Blind: one-by-one    │
                                        └──────────┬───────────┘
                                                   │ absolute last stage
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Final server brake   │
                                        │ All App work pauses  │
                                        └──────────┬───────────┘
                                                   ▼
                                        ┌──────────────────────┐
                                        │ Independent recovery │
                                        │ Timed auto-resume    │
                                        └──────────────────────┘
```

假设对话 A 开始构建，而对话 B 正在准备答案。 如果构建过程与 A 的 hook 完全相关，则 supervisor 首先缩小 A 的新工作范围，而不会影响 B。 如果危险持续存在，构建过程（而不是共享服务器）是第一个可能的物理刹车。

如果无法证明该进程属于 A 或 B，supervisor 不会随意责怪 B。它首先只保留整个应用程序中新的高内存启动，然后缩小正在运行繁重工作或最符合观察到的增长的一个对话的未来工作范围。 在测量操作效果的短暂窗口后，它会检查记忆衰退是否减慢，并仅在没有有用的变化时才添加另一个候选者。 此系列调查时间包含在应用程序从一开始的停止距离中，因此候选检查可以在风险边界之前完成，而不会过早不必要地降低性能。

实现机制虽然不同于CLI，但策略结果相同：先减少新工作，再考虑正在返回的结果；优先限制subagents而不是lead；只控制足以解释风险的最小目标集合；物理制动与恢复也始终一次只处理一个目标。首先选择所有权明确的child。只有相关对话和subagent都已实际进入最终逻辑阶段后，才允许暂停blind child。暂停共享App Server会同时影响所有对话，因此只有在所有更小范围的措施都用尽后才作为最后手段。基础OS、WSL、VM和容器之间的Federation边界仍与CLI设计一致，每个supervisor也仍然只控制自己的PID空间。完整的安全条件请参阅[Codex Desktop App](codex-app.zh-CN.md)。
