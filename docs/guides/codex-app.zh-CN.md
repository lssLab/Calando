# Codex Desktop App监控

<p align="center">
  <a href="codex-app.md">English</a> · <a href="codex-app.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="codex-app.ja.md">日本語</a>
</p>

Codex Desktop App与Codex CLI采用相同的保护策略，但其进程结构不同。 本文档解释了可以准确衡量的内容、控制的目标位置，以及当 App Server 没有公开足够的所有权证据时，supervisor 如何保持保守。

<a id="cli-and-desktop-app-are-not-the-same-process-model"></a>
## CLI 和桌面应用程序不是同一进程模型

CLI 会话通常具有一个 lead 进程和一棵进程树，可以将其作为一个本地执行单元进行测量和重新验证。 桌面应用程序通常通过一个 App Server 进程路由多个对话。

每个对话都有一个逻辑线程标识符，并作为独立的 lead 进行管理，用于 hook 决策。 它不是一个独立的操作系统进程：

```text
Codex Desktop App
        │
        ▼
shared App Server process and shared memory
        │
        ├── logical thread A ── agents and tools
        ├── logical thread B ── agents and tools
        └── unassigned App work ── ownership not yet proven
```

在另一个窗口中打开相同的对话不会创建另一个逻辑lead。 打开不同的对话会创建另一个逻辑线程，但两者仍可能共享相同的App Server内存。

<a id="what-the-supervisor-observes"></a>
## supervisor 观察到的情况

本地daemon检测到App Server并记录一次其内存。 Hooks 然后报告每个逻辑线程、代理和工具的生命周期边界。 仅当进程祖先、启动身份、hook计时和线程证据一致时，Child进程才会链接到线程。

因此，supervisor 维持三个所有权级别：

| 等级 | 证据 | 允许控制 |
| --- | --- | --- |
| 精确螺纹child | child 和逻辑线程都被重新验证 | 特定于线程的逻辑限制； 如果每个警卫都通过，则最终本地 PID 暂停 |
| 应用程序child，线程不确定 | child 属于 App Server 但不可靠地属于一个线程 | 候选调查和最小可逆blind action仅在最后一层 |
| 共享App Server | 多个线程和共享内存使用一个主机进程 | 观察和会计； 主机暂停仅作为最后一个保护阶段 |

提示文本、对话内容、模型响应和编辑的文件内容不用于推断所有权。

<a id="the-same-performance-policy-adapted-to-shared-memory"></a>
## 相同的性能策略，适应共享内存

应用程序路径保持 CLI 策略目标：

- 高但稳定的内存使用不会触发限制；
- 具有足够停车距离的快速变化不会触发立即行动；
- 仅当当前headroom、持续速率、原生压力和恢复储备时间表明危险即将来临时才开始行动；
- 新扩展在正在进行的编辑、响应和结果交付之前关闭；
- 状态稳定恢复后，限制会再次逐级解除。

App Server内存计数一次。 daemon 不会在开放对话之间平均划分其整个驻留集。 线程仅接收其 hook 和 child 活动可以支持的增长； 剩余的共享金额保留在未分配的池中。 这可以防止空闲线程仅仅因为另一个线程或共享主机增长而受到指责。

<a id="control-ladder"></a>
## 控制梯形图

supervisor 重用 CLI 策略顺序，同时更改执行器以适应 App Server：

1. **观察。**在轨迹保持可恢复的同时，保持所有工作开放。
2. **延迟新的扩展​​。**在计算的边界附近，仅保留新代理、重型工具、构建和测试。 现有的产出和成果交付继续进行。
3. **一次缩小一个逻辑目标。**当支持一个线程作为原因时，以小步减少该线程的未来工作，并在每一步后重新测量。
4. **选择一个 child 候选者。** 优先选择当前正在增长的、沉重的、最近的 child 且拥有最强有力的所有权证据。 未选择的线程、代理和正在运行的工作保持不变。
5. **如有必要，暂停一个已验证的child。**这是逻辑控制不足且危险迫在眉睫后的可逆最终后盾。
6. **仅在无法获得确切所有权时使用blind App控制。**调查候选池，对最多一个合格的应用程序child采取行动，重新测量，如果轨迹改善则立即停止。
7. **仅将共享App Server暂停作为最后的保护阶段。**这会影响该进程托管的每个对话，因此需要持续的应用程序归因增长、迫在眉睫的危险、失败的较低层、持久的操作记录和可用的通知路由。

一次普通的谈话不会跳过这个阶梯。 会话数不是停止的理由，稳定轨迹下的一个轻线程仍然不受限制。

<a id="blind-control-and-its-limit"></a>
## Blind control及其极限

App Server 并不总是为每个逻辑代理或工具公开可靠的操作系统 PID。 当所有权不完整时，supervisor不会假装已知线程拥有所有共享内存。 它使用进程祖先、创建时间、最近的hook活动、内存增长、工作负载类别、先前的操作和App Server生成来缩小候选范围。

Blind control可以阻止所选应用程序child的额外增长，但它不能保证哪个逻辑对话会感到暂停。 因此，它比精确的线程控制更晚、更保守。 如果没有候选人满足动作守卫，则supervisor将动作保持在admission控制，而不是发出猜测的PID信号。

<a id="lead-awareness-notifications-and-recovery"></a>
## Lead 意识、通知和恢复

当物质保护操作开始以及完全恢复时，终端和启用的操作系统或远程路由会收到通知。 受影响的逻辑lead 在其下一个hook 边界接收相同的原因和当前恢复状态。

因内存压力而暂停的children会在headroom稳定后一次恢复一个。因自身内存持续增长而暂停的lead或共享App Server会在保护状态下获得一次试运行恢复。如果相同的增长再次出现，supervisor会重新暂停并等待用户判断，而不会反复自动恢复。

如果 hook 被禁用、不受信任或通过陈旧路径路由，daemon 仍然可以观察系统和进程内存，但会丢失部分逻辑控制面。 它报告保护降级并告诉用户查看Codex App**设置→Hooks**。 它不会默默地用广泛的过程控制来替换丢失的线程证据。

<a id="multiple-windows-app-server-generations-and-federation"></a>
## 多个窗口、App Server 代和federation

连接到一个App Server的所有窗口都是一个本地应用程序表面。 内存计数一次，每个逻辑线程根据其标识符进行重复数据删除。 如果 App Server 重新启动，新进程启动标识将创建新一代，因此旧进程的陈旧所有权无法授权信号。

两个App Server进程同时声明同一逻辑线程被视为异常冲突。 supervisor 不会合并它们的物理所有权或执行精确的线程控制，直到歧义消失。

运行在同一内核中的Codex CLI和Codex Desktop App被同一个本地daemon观察到。 CLI 进程树和共享 App Server 是单独的本地表面。 如果计算机还运行 WSL2、容器或动态来宾，federation 跨内核共享聚合新工作决策； 它永远不会将App线程控制或PID权限导出到另一个环境。

请参阅[架构](architecture.zh-CN.md)、[Codex设置](usage-codex.zh-CN.md)和[测试矩阵](../testing/test-matrix.zh-CN.md)。
