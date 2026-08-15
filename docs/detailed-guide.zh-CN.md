<p align="center">
  <img src="../assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="detailed-guide.md">English</a> · <a href="detailed-guide.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="detailed-guide.ja.md">日本語</a>
</p>

<p align="center">
  <em>在 Claude Code 和 Codex 处理长时间运行的大规模工作负载时控制内存使用，有助于防止终端或应用程序冻结和意外会话退出。</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 or newer"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 or newer"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 or newer and Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="Test"></a>
  <a href="guides/setup.zh-CN.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="guides/performance.zh-CN.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="guides/security.zh-CN.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

<a id="what-problem-does-memory-supervisor-solve"></a>
## Memory Supervisor解决什么问题？

在Claude Code、Codex CLI或Codex Desktop App、subagents中长时间运行的工作期间，构建、测试和浏览器工具可能会堆积起来。 如果可用内存快速下降，CLI 终端可能会停止响应或退出； 在桌面应用程序中，共享App Server的多个对话可能会同时受到影响。 在任何一种情况下，待处理的结果和正在进行的工作都可能被中断。

Memory Supervisor 不会仅仅因为内存使用率高而限制工作。 在 CLI 和桌面应用程序中，只有当真正的风险接近时，它才会分阶段延迟新工作，同时尽可能保持正在进行的工作和结果交付运行。 这有助于防止会话突然结束。

保护不会从无限制运行直接跳到完全停止。实际风险越近，措施就逐级加强；状态恢复后，再按相反顺序逐级解除。

1. **自动设置** — 像往常一样启动`claude`或`codex`，或在Codex Desktop App中开始对话。 Memory Supervisor 自动区分 CLI 会话和 App 会话，读取总容量、可用内存、下降速度以及即将进行的工作所需的缓冲区，然后设置保护级别。 没有需要配置的预算，也没有需要持续检查的状态。
2. **无限制运行** - 当可用内存及其变化率保持稳定时，仅高内存使用不会触发限制。
3. **以最佳性能观察** - 快速下降本身并不会限制工作，同时还有大量内存。 supervisor 保持一切开放，同时检查下跌是否持续以及真正的风险是否越来越近。
4. **首先延迟新的subagents、工作流程和任务** - 当内存headroom持续丢失使风险更接近或为另一个工作块留下的空间太少时，此阶段仅延迟创建新的subagents、工作流程和任务，而不影响已经正在进行的工作。 它本身不会延迟构建或测试启动或暂停正在运行的程序，从而提供当前工作的完成时间和内存的恢复时间。
5. **逐渐减少工作** - 随着风险越来越近，新的subagents、工作流程和任务的创建首先会被阻止。 只有当可靠证据将损失全部或部分归因于人工智能工作，或者超过可选的用户设置上限时，现有代理的未来工作范围才会缩小到`all work → no new subagents, workflows, or tasks → no new memory-heavy starts such as builds and tests → handoff, coordination, status, stop, recovery, and small reads only`。
 Subagents 不会一次性全部受到限制。 如果有足够的时间，subagent 在下一次工具调用时向下移动一个梯级； 在较少的时间内，supervisor应用在保留之前完成梯子所需的最小批次，然后重新测量。 未选择的代理和正在运行的工作保持不变。 按以下顺序选择Subagents进行限制：(1) 已验证链接流程中的异常增长，(2) 当前或最近用于代理、工作流或任务创建或繁重工作的工具，(3) 更严格的当前状态，(4) 链接流程达到保留的更短时间，以及 (5) 较新的开始。
 只有当所有subagent都已进入最窄阶段、危险仍然存在时，才会收窄lead。但如果lead已被确认是主要原因，而且先限制subagent会来不及，则会先把lead收窄一级。如果原因只来自外部程序，现有AI工作会继续运行；仅暂缓创建新的subagents、工作流和任务，以及在系统内存压力严重时启动重型工作。
6. **暂停一个进程作为最后的手段** - 只有当危险持续存在并且属于Claude Code或Codex的一个进程显示出确认的持续增长时，supervisor才会暂停该进程而不终止它。 终端立即显示操作，并且lead在执行下一个任务之前接收到相同的上下文。
7. **反向恢复** - 内存保持稳定后，工作从结果交付开始一次重新打开一个阶段，暂停的进程一次恢复一个。

目标不是使用更少的内存。 它是为了保护Claude Code和Codex CLI终端会话和Codex Desktop App对话，同时维持尽可能多的有用性能。

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

经过持续改进后，代理功能以相反的顺序重新打开，暂停的程序一次恢复一个。 精确的计算和物理测量位于[自适应停止距离](testing/stopping-distance.zh-CN.md)中。

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

有关完整的状态流、多终端布局和故障边界，请参阅[架构和运行时拓扑](guides/architecture.zh-CN.md)。

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

实现机制虽然不同于CLI，但策略结果相同：先减少新工作，再考虑正在返回的结果；优先限制subagents而不是lead；只控制足以解释风险的最小目标集合；物理制动与恢复也始终一次只处理一个目标。首先选择所有权明确的child。只有相关对话和subagent都已实际进入最终逻辑阶段后，才允许暂停blind child。暂停共享App Server会同时影响所有对话，因此只有在所有更小范围的措施都用尽后才作为最后手段。基础OS、WSL、VM和容器之间的Federation边界仍与CLI设计一致，每个supervisor也仍然只控制自己的PID空间。完整的安全条件请参阅[Codex Desktop App](guides/codex-app.zh-CN.md)。

<a id="installation"></a>
## 安装

打开适合您环境的**终端**，然后粘贴下面匹配的一行命令。 无需准备 Git、Python、Rust 或单独的安装程序。 正常安装的范围仅限于您的用户帐户，不需要 `sudo` 或管理员 shell。

<a id="1-install-memory-supervisor"></a>
### 1.安装Memory Supervisor

<a id="linux-wsl2-or-macos-terminal"></a>
#### Linux、WSL2 或 macOS 终端

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

命令完成后，后台服务正在运行，检测到的Claude Code和Codexhooks会自动连接。 它不会关闭正在运行的 AI 程序或中断正在进行的工作。

<a id="windows-powershell-terminal"></a>
#### Windows PowerShell 终端

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

命令完成后，后台服务正在运行，检测到的Claude Code和Codexhooks会自动连接。 它不会关闭正在运行的 AI 程序或中断正在进行的工作。

> [！重要的]
> Windows 可执行文件目前正在由 [SignPath Foundation](https://signpath.org/) 进行审核，
> 因此 Windows 11 需要 **Windows 安全 → 应用程序和浏览器控制 → Smart App Control** 保留
> `Off`，同时安装和使用本机构建，直到审核完成。

| 窗口状态 | 原生版本可以安装吗？ |
| --- | --- |
| 64 位 Windows 10 | 是的。 Smart App Control 不可用，因此不需要 SAC 设置。 如果出现 SmartScreen，请验证下载是否来自此存储库的版本。 |
| Windows 11 24H2 build 26100.8117 或更高版本、25H2 build 26200.8117 或更高版本，或者重新启用控件可见的较新 Windows 11 版本 | 是的，将Smart App Control设置为`Off`后。 停止使用未签名的版本后，您可以从同一“设置”页面重新打开它。 |
| 较旧的 Windows 11 版本或尚未收到逐步推出的控制权的当前版本 | 关闭后可以，但重新打开可能需要重置或重新安装 Windows。 在禁用它之前检查一下。 |
| Smart App Control 已经是 `Off` | 安装而不更改它。 如果出现单独的 SmartScreen 下载信誉提示，请验证发布者和文件源。 |
| 处于 S 模式的 Windows 11 或阻止可执行文件的组织应用程序控制策略 | 本机 Windows 路径不支持。 此安装程序无法绕过 S 模式或管理员策略。 |

从 `Win + R` 运行 `winver` 以检查 Windows 版本和构建。 在关闭Smart App Control之前，还要确认其设置页面是否提供重新打开它的方法； 推出是按设备进行的。 请参阅 Microsoft 的 [Smart App Control 常见问题解答](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions) 和 [初始推出说明](https://support.microsoft.com/en-au/help/5079391) 了解来源标准。

针对**App Server及其工具实际运行的环境**进行安装，而不是针对显示Codex App窗口的操作系统进行安装。

- 当 Windows 上的 Codex App 使用 WSL 引擎时，安装在该 WSL 环境中的 Supervisor 会保护其 WSL App Server、每任务逻辑线程和 WSL 端工具。 该路径不运行本机WindowsSupervisor，因此不需要关闭Smart App Control。 Windows 应用程序 UI 进程和任何单独的 Windows 本机 Claude Code 或 Codex CLI 均位于 WSL 安装的测量和控制边界之外。
- 当 App Server 或 CLI 直接在 Windows、macOS 或 Linux 上运行时，请安装在该操作系统中。 本机 Windows 安装使用上述 Smart App Control 要求。
- 当 App Server 或 CLI 在另一个 WSL 发行版、虚拟机或隔离容器中运行时，请在每个此类环境中安装。 Windows 和 WSL 自动查找其federation 路径； macOS 或 Linux 主机、动态内存虚拟机和容器连接同一台计算机上的共享文件夹。 连接后，争夺相同物理内存的环境将共享新工作决策。 固定内存虚拟机和其他计算机或云服务器独立保护自己。 请参阅[平台和多环境行为](guides/platforms.zh-CN.md)了解边界。

<a id="2-set-up-claude-code"></a>
### 2. 设置Claude Code

安装程序自动连接Memory Supervisor用户hook。 Claude Code 中无需配置、批准或启用任何内容。

**如果 Claude Code 在安装过程中已在运行：** 继续工作。 Claude Code 自动重新加载用户设置更改，因此通常不需要重新启动。

**验证：**确认步骤 5 的 `memory-status --connections` 输出中的 `Claude Code CONNECTED`。 仅当您想查看 hook 详细信息时，才打开只读 `/hooks` 屏幕并检查 `User Settings`。 仅在该可选视图不显示条目的特殊情况下，在当前工作之后重新启动 Claude Code。

<a id="3-set-up-codex-cli"></a>
### 3. 设置Codex CLI

1. 在您将使用的Codex CLI中打开`/hooks`。
2. 确认所有七个 Memory Supervisor hooks 均**受信任且处于** 状态。
3. 信任标记为要审查的条目并打开任何禁用的条目。
4. 关闭`/hooks`并继续工作。

**如果 Codex CLI 在安装过程中已在运行：** 继续在您刚刚检查的 CLI 中； 它不需要重新启动。 对于安装前已打开的任何其他Codex CLI，完成其当前工作并仅重新启动该 CLI 一次。

<a id="4-set-up-codex-desktop-app"></a>
### 4. 设置Codex Desktop App

1. 打开Codex App并进入**设置→Hooks**。 如果 Memory Supervisor 条目尚未存在，请等待最多 60 秒，然后重新打开“设置”。
2. 信任并打开所有七个Memory Supervisorhooks。 **全部信任** 不会打开之前禁用的开关，因此请检查这两个状态。
3. 返回现有任务并发送您想要发送的下一个请求。 仅当没有要继续的现有任务时才创建新任务。

**如果 Codex App 在安装过程中已在运行：** 让应用程序及其现有任务保持打开状态，然后按照步骤 1-3 进行操作。 您无需重新启动应用程序或创建新任务。

<a id="5-verify-the-installation"></a>
### 5. 验证安装

```bash
memory-status --connections
```

检查您使用的程序的行：

- `Core daemon CONNECTED`：后台服务正常。
- `Claude Code CONNECTED`：已连接支持的版本和用户hook。
- `Codex CONNECTED`：所有七个 CLI hooks 均已安装、启用并受信任。
- `Codex App ACTIVE`：所有七个应用程序hooks 均已准备就绪，并且来自现有任务或新任务的真实呼叫已到达。
- `NOT DETECTED` 对于您不使用或尚未安装的程序来说是正常的。

如果线路不健康，则仅根据其报告的内容采取行动：

- `disabled` 或 `not trusted`：使用 Codex CLI 中的 `/hooks` 或 Codex App 中的 **设置 → Hooks** 来信任并启用指定条目。
- `missing`、`stale`、`DEGRADED` 或`NOT RUNNING`：运行`memory-supervisor update`，然后重复此检查。
- `NEEDS ATTENTION`：满足报告的程序版本或hook要求，然后运行`memory-supervisor update`。
- `Core daemon OFF`：运行`memory-supervisor on`。
- 如果所有七个应用程序hooks看起来都正确，但在请求后应用程序仍然没有变成`ACTIVE`，请重新启动应用程序一次，在现有任务中发送下一个请求，然后再次检查。
- 如果新安装找不到`memory-status`命令，请仅重新打开终端并再次运行。 Claude Code、Codex CLI 和 Codex App 不需要为此 PATH 刷新重新启动。

Codex hook 信任不是管理员访问权限。 这是您对 Codex 将运行的确切本地命令的批准。 仅当组织策略或 Windows 安全策略阻止安装时才检查管理员策略。 有关底层信任规则，请参阅 [Claude Code hooks 指南](https://code.claude.com/docs/en/hooks) 和 [Codex hooks 指南](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)。

这些命令安装最新的公开版本。 不需要 Rust 构建工具； 自动使用该版本中包含的经过验证的可执行文件。

<a id="6-uninstall"></a>
### 6. 卸载

要删除 Calando，请在安装它的每个环境中运行一次：

```bash
memory-supervisor uninstall
```

它删除后台服务、可执行文件以及Calando拥有的hook和技能连接，同时保留状态和用户设置。

<a id="supported-environments"></a>
## 支持的环境

在每个受支持的环境中，保护的行为都是相同的。 supervisor监视可用内存及其下降率，分阶段缩小新工作范围，仅在危险仍然存在时暂停一个经过验证的Claude Code或Codex进程，并在稳定恢复后恢复它。 仅用于读取内存和暂停进程的操作系统机制有所不同。

| 环境 | 测试覆盖率 |
| --- | --- |
| 64 位 Intel/AMD 上的 Linux 和 WSL2 | 物理 WSL2 和自动化 Linux 检查 |
| macOS 苹果芯片 | 自动 Apple Silicon 检查 |
| 64 位 Intel/AMD 上的 Windows 10 或 11 | 物理 Windows 11 E2E、自动化 Windows Server 2022 检查以及 Windows 10 运行时/API 兼容性审查 |
| 基于 Intel 的 macOS | Rosetta 下的自动兼容性 |

连接的产品为 Claude Code 2.1.217 或更高版本、Codex CLI 0.145.0 或更高版本以及 `hooks stable true` 和 Codex Desktop App。 CLI和App应用相同的保护策略。

<a id="measured-resident-memory"></a>
### 测量的常驻内存

这些操作系统总量是在预热后以 0.2 秒的间隔在 20 个样本中测量的。

| 测试环境 | 最低限度 | 意思是 | 最大限度 | 操作系统指标 |
| --- | ---: | ---: | ---: | --- |
| WSL2 Linux，物理服务 | 4.88 字节 | 4.88 字节 | 4.88 字节 | RSS |
| 64 位 Intel/AMD 上的 Ubuntu，自动化测试 | 3.50MB | 3.52米B | 3.54米B | RSS |
| 64 位 Intel/AMD 上的 Windows，自动化测试 | 4.15 米B | 4.20 米B | 4.25 MiB | 工作集 |
| macOS Apple Silicon，自动化测试 | 3.38 米B | 4.35 MiB | 5.13 米布 | RSS |

对于容量规划，请使用**每个已安装的监视器 10 MiB**，而不是最小的样本。 有关详细条件和原始数据，请参阅[性能测量](guides/performance.zh-CN.md)。

当一台物理计算机具有多个执行环境（Windows、WSL 发行版、虚拟机或隔离容器）时，请将其安装在运行 Claude Code 或 Codex 的每个环境中。 一个环境中的多个终端共享一台显示器。 在每个环境中安装并设置federation路径后，无论运行多少个内核，整个计算机都会自动共享最新的新工作决策。 每个监视器仍然只测量和控制自己的环境，因此它永远不会在另一个环境的 PID 上运行。 安装程序为 Windows 和 WSL 连接相同的本地共享文件夹； VM 或容器使用主机共享的本地文件夹作为其 federation 路径。 网络文件夹不用于连接不同的物理计算机或云服务器。 有关设置详细信息，请参阅[平台和多环境行为](guides/platforms.zh-CN.md)。

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

请勿将 Discord Webhook URL 或 Discord 或 Telegram 机器人令牌放在命令行上。 在运行安装命令后出现的隐藏提示中输入它。 更改适用于下一个通知，无需重新启动 supervisor 或 AI 程序。 请参阅[通知设置](guides/notifications.zh-CN.md)了解路由选择和删除、Discord 频道和 DM 设置、Telegram 群组设置以及故障排除。

<a id="skills-and-commands-in-claude-code-and-codex"></a>
## Claude Code和Codex中的技能和命令

安装程序连接三个独立的部分：做出自动决策的**hooks**、教代理理解和解释状态的**技能**以及调用该工作流程的**简短命令**。 Hooks 无需用户调用即可运行； 该技能本身并不强制执行内存策略。

| 使用地点 | 输入什么 | 它的作用 |
| --- | --- | --- |
| Claude Code | 询问“检查内存状态”，使用`/memory-supervisor`，或使用`/memory-status` | 安装的技能或快捷方式会读取完整状态并解释原因、自动恢复以及任何所需的命令。 |
| Codex CLI | 使用`$memory-supervisor check memory status`； 使用`/skills`确认发现。 `/prompts:memory-status` 是兼容性快捷方式。 | 通过Codex的主要技能路径运行相同的状态工作流程。 Hook 信任和支持在`/hooks` 中保持分离。 |
| Codex Desktop App | 使用`$memory-supervisor check memory status`或在任务中自然询问 | 在每个任务中使用相同的用户级别Codex技能。 没有单独的App技能； 在 ** 设置 → Hooks** 中管理 hooks。 |
| 操作系统终端 | 使用`memory-status`或`memory-supervisor ...` | 这些是真实的状态、设置和恢复命令，而不是技能。 `resume`、`terminate` 和 `kill` 仅在明确的用户请求后运行。 |

该技能读取 `memory-status --all` 并解释原因和下一步操作，但未经用户批准，它不会恢复或终止进程。 如果Claude Code或Codex是在Memory Supervisor之后安装的，请运行`memory-supervisor update`并验证与`memory-status --connections`的连接。 有关详细差异，请参阅[Claude Code指南](guides/usage-claude.zh-CN.md)和[Codex指南](guides/usage-codex.zh-CN.md)。

<a id="security"></a>
## 安全

Memory Supervisor 读取操作系统内存和进程信息，以及会话、代理、工具、工作目录和连接状态信息以及由 Claude Code 和 Codex hooks 提供的命令前缀。 它仅使用此信息来决定是否可以开始新的工作并确定确切的控制目标。

自动控制在延迟未来Claude Code或Codex工作时停止，并在最后的保护阶段暂停和恢复一个经过验证的本地工作流程。 它永远不会自动终止程序或控制不相关的程序。 正常监控不向外部发出请求； 只有 GitHub 安装和更新以及运营商启用的 Discord 或 Telegram 通知使用网络。

**这是完整的检查和控制边界； Memory Supervisor 不处理其之外的任何内容。** 它不使用可能存在于 hook 负载中的提示、对话文本、模型响应或文件内容来进行控制决策，并且不保留它们。 它不会直接打开项目文件或进程内存，也不会检查或更改浏览器或 IDE 内部数据、Claude 或 ChatGPT 凭据或操作系统内核、内存、交换和防火墙设置。 请参阅[安全和数据/控制边界](guides/security.zh-CN.md)，了解存储数据、同一机器federation字段和安全措施的完整列表。

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

Rust 单元、集成和安装程序测试涵盖策略、流程安全、Claude Code 和 Codex 接线、federation、恢复和发布捆绑包。 GitHub Actions 检查 Linux x86-64、Windows x86-64、Apple Silicon macOS 和 Rosetta 下的 macOS x86-64 上的构建和平台合同。 有界物理机验证加上确定性模拟涵盖了真正的接近耗尽边界。 请参阅[测试覆盖率](testing/test-matrix.zh-CN.md)。

<a id="documentation"></a>
## 文档

| 指导 | 用它来 |
| --- | --- |
| [所有文档](README.zh-CN.md) | 查找安装、使用、安全边界和公共测试文档 |
| [架构](guides/architecture.zh-CN.md) | 后台监控、启动前检查、状态文件和程序控制 |
| [Codex Desktop App](guides/codex-app.zh-CN.md) | 逻辑对话，blind control，以及共享App Server内部的恢复 |
| [自适应停车距离](testing/stopping-distance.zh-CN.md) | 计算、测量边界、逐渐制动和恢复 |
| [平台和多环境行为](guides/platforms.zh-CN.md) | 操作系统和虚拟环境如何共享新工作决策 |
| [安全和数据/控制边界](guides/security.zh-CN.md) | 信息读取、存储和共享，以及自动和手动控制限制 |
| [测试覆盖率](testing/test-matrix.zh-CN.md) | 公开测试涵盖的产品行为和平台 |
| [Claude Code](guides/usage-claude.zh-CN.md) / [Codex](guides/usage-codex.zh-CN.md) | CLI 和桌面应用程序集成和会话行为 |
| [通知](guides/notifications.zh-CN.md) | 终端、操作系统、Discord 和 Telegram 交付 |
| [性能](guides/performance.zh-CN.md) | 后台内存使用和启动前检查时间 |
| [安全策略](../.github/SECURITY.zh-CN.md) | 私有漏洞报告路径 |
| [贡献](../.github/CONTRIBUTING.zh-CN.md) | 变更原则和提交前检查 |

<a id="license"></a>
## 执照

[麻省理工学院](../LICENSE)
