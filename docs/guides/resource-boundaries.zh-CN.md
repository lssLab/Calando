# 会话发现、容量检测和内存边界

<p align="center">
  <a href="resource-boundaries.md">English</a> · <a href="resource-boundaries.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="resource-boundaries.ja.md">日本語</a>
</p>

本指南解释了一个安装可以看到什么，supervisor如何在没有包装器的情况下发现终端会话，它如何了解操作系统或来宾的可用内存，以及每个可配置边界实际更改的内容。

<a id="the-short-model"></a>
## 短模型

一台物理计算机并不总是一个控制边界。 它可以包含几个独立可观察的PID和内存域：

```text
physical computer
├─ Windows host                -> Windows supervisor
├─ WSL distribution           -> Linux/WSL supervisor
├─ Linux or Windows VM        -> supervisor inside that guest
└─ Apple Silicon Mac host
   └─ macOS or Linux VM       -> supervisor inside that guest

fresh state snapshots (≤10 s) -> shared admission decision
local process table            -> only the owning instance may pause/resume that PID
```

在运行 Claude Code 或 Codex 的每个主机、WSL 发行版、VM 来宾或 PID 隔离容器中安装一次。 主机安装无法发出来宾 PID 信号。 Federation 分享背压； 它不会创建跨内核进程控制器或将 RAM 总数加在一起。

<a id="what-each-terminal-surface-belongs-to"></a>
## 每个终端表面属于什么

| CLI 启动的位置 | 实际观测边界 | 产能来源 | 在哪里安装和配置 |
| --- | --- | --- | --- |
| PowerShell、命令提示符或本机 Windows 终端选项卡 | Windows主机 | `GlobalMemoryStatusEx` 物理总量/可用； `GetPerformanceInfo`提交headroom | 一旦进入Windows |
| WSL 终端选项卡 | 该 WSL 发行版的可见 Linux PID/内存域 | `/proc/meminfo`，由每个封闭的 cgroup 限制缩小； 最终受 WSL VM 内存限制 | 一旦进入运行支持的 CLI 的每个 WSL 发行版； 也安装在 Windows 上以保护主机 |
| Bare Linux、SSH 会话或 tmux 窗格 | Linux 内核/PID 命名空间和用户权限 | `/proc/meminfo`，通过每个封闭的 cgroup v1/v2 限制缩小 | 每个受保护的操作系统用户/环境一次 |
| Apple Silicon Mac 上的终端或 iTerm | macOS `arm64` 主机 | `sysctl hw.memsize`; 来自 `vm_stat` 的空闲、非活动且可清除的页面 | 一旦进入 macOS |
| Apple Silicon Mac 上的 macOS VM | 来宾 macOS `arm64` VM | 客人`hw.memsize`和`vm_stat`； 受 VM 分配限制 | 一旦进入客人体内； 主机安装保持独立 |
| 任何主机上的 Linux 或 Windows VM | 来宾操作系统 | 与上面相同的本机 Linux 或 Windows 源，受虚拟机管理程序分配的限制 | 一旦进入客人体内； 主机安装保持独立 |
| PID隔离容器 | 该容器的可见进程和 cgroup 域 | 物理内存因所有封闭的 cgroup 限制而变窄 | 一旦进入隔离容器，或故意共享主机 PID 命名空间 |
| 基于 Intel 的 Mac | macOS `x86_64` 主机 | 相同的 macOS 源 | 一旦使用那台 Mac |

Apple Silicon macOS VM 仍为 `arm64`。 在 Rosetta 下运行 x86_64 属于兼容性范围，并且与物理 Intel Mac 验证不同。

WSL2 发行版可以共享相同的底层实用程序 VM，同时保留单独的进程命名空间。 在运行 CLI 的每个发行版中进行安装，因为一个发行版无法可靠地清点或发出另一发行版的 PID。 Federation 采取最糟糕的新决策； 它不会对共享 WSL 内存池的重复视图求和。

<a id="how-sessions-are-discovered-without-a-wrapper"></a>
## 如何在没有包装器的情况下发现会话

用户仍然正常启动`claude`或`codex`。 daemon 不枚举终端窗口或需要 `claude-governed`/`codex-governed` 启动命令。

1. 本机daemon扫描其操作系统帐户可见的完整进程清单。 正常控制循环为一秒； Windows 最多每三秒刷新一次更昂贵的 CIM 库存，同时每次读取便宜的全局内存计数器。
2. 当进程的可执行文件或第一个命令参数解析为 `claude`、`codex` 或官方特定于体系结构的 Codex 二进制文件时，进程就是受支持的 CLI 根。
3. 父链接将嵌套支持的 CLI 根分组为 workers。 其他后代成为支持进程。 祖先行走仅限于 64 个级别，因此格式错误的流程图不能永远循环。
4. 每个后代都对其根树的 RSS 估计做出贡献。 匿名内存低于 32 MiB 的小后代仅作为单独的暂停候选者被忽略； 它们不会从树总数中删除。
5. PID 加上进程启动标识可防止 PID 重用以不同的进程为目标。 Linux 使用 `/proc/<pid>/stat` 开始时间，macOS 使用 `ps` 开始时间，Windows 使用 CIM `CreationDate`。
6. daemon记录lead/worker/支持角色、内存增长、根树总数以及平台公开时经过验证的终端身份。 操作系统权限、Linux `hidepid`、容器和虚拟机边界受到尊重，而不是被绕过。

AI CLI hooks 是第二条路径，而不是进程检测器。 他们在新的扇出之前询问最新的本地/联邦状态，并将事件注入到下一个真实hook边界的主代理中。 如果缺少 hook，daemon 仍然可以观察其本地进程表，但无法阻止 AI CLI 在新进程启动之前进行分配。 使用以下命令检查两条路径：

```bash
memory-status --connections
memory-status --all
```

<a id="how-usable-capacity-is-learned"></a>
## 如何学习可用容量

| 平台 | 容量 | 可用/headroom | 额外的遇险证据 |
| --- | --- | --- | --- |
| Linux 和 WSL | `MemTotal`，减少到最小的有限cgroup祖先限制 | `MemAvailable` 和每个有限 `limit - current` cgroup 余数的最小值 | PSI `some/full`、回收、交换和 OOM 计数器 |
| macOS | `sysctl -n hw.memsize` | `vm_stat` 免费 + 非活动 + 可清除页面 | 暴露时的内核压力水平、页面输出/压缩和交换趋势 |
| 视窗 | `GlobalMemoryStatusEx.totalPhys` | `GlobalMemoryStatusEx.availPhys` | 提交限制减去 `GetPerformanceInfo` 中提交的页面 |
| 任何虚拟机访客 | 上面的相关行在客人内部报告 | 访客可见值，因此已经受到固定或动态 VM 分配的限制 | 来宾本地压力信号 |

已解决的容量和自适应策略会在每次更新时重新计算。 因此，无需固定机器大小配置文件即可拾取 VM 动态内存更改或 cgroup 更改。 如果主传感器发生故障，状态报告保护降级并保留admission； 8 GiB 后备标签是诊断性的，而不是声称该计算机确实具有 8 GiB。

supervisor 仅**读取**包含已创建的容器运行时、systemd 单元、调度程序或管理员的 cgroup 限制。 它不创建 cgroup、将 CLI 移至 cgroup 中，也不需要包装器命令。 这就是为什么仍然会发现正常的 `claude` 或 `codex` 启动，而字节精确的 cgroup 分配仍然是可选的外部边界，而不是该产品的默认执行器。

supervisor **不** 为进程分配 RAM。 它计算停止距离而不是保留固定的百分比：

```text
minimum breathing room = 0.5% of detected capacity, clamped to 256–1024 MiB
corroborated burn rate = max(sustained physical/commit headroom fall,
                             sustained tracked-CLI growth)
automatic reserve     = min(minimum breathing room
                            + corroborated burn rate × one reaction interval,
                            25% of detected capacity)
new-fan-out floor     = min(automatic reserve + one minimum breathing/work block,
                            30% of detected capacity)
```

物理headroom已经包含跟踪的CLI分配，因此这两个速率故意与`max`合并，而不是添加和计算两次。 一条轨迹至少需要三个样本，一个跨度的反应区间，至少60%的支撑区间，以及至少两倍于反弹的危险方向的移动量。 因此，一次回收尖峰无法消除真正的下降，而一次爆发也无法创造真正的下降。

这与制动车辆的几何形状相同：更快的消耗会在 MiB 中产生更大的距离，但不会提前进行干预； 在达到相同的反应窗口之前，较慢的消耗可以使用更多的机器。 当储备在两个反应间隔内或没有空间容纳一个新的最小块时，`HOLD`仅关闭新的扇出。 `DRAIN` 仅在一个反应​​间隔内开始分级现有代理缓冲，并且仅具有代理/混合归因或明确的硬上限。 每个一秒计时所应用的逻辑步骤数为`ceil(remaining steps / control ticks left)`，因此八个workers和数百个会话在边界处完成相同的最小阶梯，而没有固定的代理计数上限。

稳定的高使用率可以保持开放。 原始绿色/黄色/橙色/红色利用率是诊断性的，本身不会关闭admission或授权 PID 暂停。 测量到的从小到大机器和近乎窒息的证据位于[自适应停止距离](../testing/stopping-distance.zh-CN.md)中。

<a id="the-five-different-boundaries"></a>
## 五种不同的界限

| 边界 | 默认 | 如何改变它 | 直接范围 | 重要的副作用 |
| --- | --- | --- | --- | --- |
| 物理或虚拟机分配 | 操作系统/管理程序默认值 | 物理RAM无法通过软件更改； 更改该平台中的 WSL、Hyper-V、Parallels、VMware、UTM 或云 VM 内存 | 主机或来宾操作系统本身 | 提高来宾内存可以提供更多可能性headroom，但会减少主机的最坏情况储备。 降低它会使客人的自适应阈值和预订向下重新计算。 通常需要来宾关闭/重新启动。 |
| 自动检测容量 | 原生传感器 | 正常情况下什么也不做。 仅当本机值明显错误时，`MEMORY_SUPERVISOR_CAPACITY_MB` 才是高级校准覆盖 | 一个已安装的实例 | 它改变了策略计算，但不改变实际的操作系统/虚拟机限制。 设置得太高是不安全的； 太低是不必要的保守。 |
| 适应性压力政策 | `balanced`; 没有手动预算 | 可选的 `protect`、`balanced` 或 `performance` 配置文件，或高级阈值覆盖 | 一个已安装的实例 | Federation 可以将该实例更严格的 admission 决策传播给同级。 `performance` 永远不会绕过实际崩溃、降级保护或显式上限。 |
| 聚合支持的 CLI 内存预算（硬上限） | **关闭** | 该环境中的`memory-supervisor budget set <GiB>`或`budget off` | 该 OS/PID 域可见的所有Claude Code 和 Codex 根树； 不是 Chrome 或整个机器 | 在上限附近，举行新的扇出。 在此之上，每个反应间隔最多可以暂停一个经过验证的增长worker/支持流程。 上限邻近保持在本地：`near/exceeded`状态不再在联合对等点上关闭admission（仅测量压力联合），并且无法暂停其PID。 |
| Federationadmission | 当实例共享目录时启用 | 配置共享`MEMORY_SUPERVISOR_FEDERATION_DIR`； WSL 分发名称是自动的，而其他克隆来宾需要唯一的 `MEMORY_SUPERVISOR_INSTANCE` 值 | 新的扇出仅在新的同行之间进行 | 使用最近十秒中最差的有效快照。 它从不池化硬上限、添加 RAM 总量、迁移作业或更改远程配置。 |

<a id="changing-a-supported-cli-memory-budget"></a>
## 更改支持的 CLI 内存预算

**在进程树应更改的每个环境**中运行以下命令：

```bash
memory-supervisor budget
memory-supervisor budget set 12
memory-supervisor budget off
```

`12` 只是 GiB 语法示例，不是推荐的大小（`memory-supervisor hard-cap set <MB>` 仍然是 MB 精度别名）。 裸露的`budget`报告使用共享的federation快照显示了该环境的理论最大值和当前可能的总计（在对等环境的显式预算之后）； 只有明确的预算才算作声明，而不是环境的默认分配。 `set` 针对当前可能的总数进行验证 — 过大的请求会被拒绝，并根据每个环境进行精确的缩减以使其适合，而当前可能总数的 90% 或更多的请求 — 或者将机器范围的显式预算总数推至物理估计的 90% 或更多的请求 — 要求确认（对于脚本来说是`--yes`）。 `set` 保留不相关的配置并重新加载该本地服务； `off` 将环境返回到仅自适应模式。

示例：

| 期望的结果 | 行动 |
| --- | --- |
| 本机 Windows Claude Code 和 Codex 的一份共享预算 | 在 PowerShell 中运行一次 `budget set <GiB>` |
| WSL 课程的不同预算 | 在该 WSL 发行版中运行单独的值 |
| 主机和来宾虚拟机中的策略相同 | 在主机上和访客内部运行相同的命令一次 |
| 两个虚拟机的预算不同 | 在每个虚拟机内运行不同的值 |
| 无处不在的默认智能行为 | 在之前有覆盖的每个环境中运行 `budget off` |

该上限对每个完整支持的 CLI 根树进行一次计数。 它被采样，可能在滴答之间超调，并且暂停不会立即返回已驻留的内存。 使用本机 cgroup、容器或 VM 限制来实现字节精确分配上限。

<a id="changing-wsl-or-vm-allocation"></a>
## 更改 WSL 或 VM 分配

对于 WSL2，主机端 `%UserProfile%\.wslconfig` 设置最大共享 WSL VM 内存。 例子：

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

这是最大值，而不是预分配。 它仅在 WSL VM 完全停止后适用。 当 CLI 会话处于活动状态时，切勿运行 `wsl --shutdown`，因为它会终止它们； 使用空闲边界。 请参阅 Microsoft 的 [WSL 配置](https://learn.microsoft.com/windows/wsl/wsl-config) 和 [`wsl --shutdown`](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown) 文档。

对于 Hyper-V、Parallels、VMware、UTM 和云 VM，通常在来宾停止时更改该虚拟机管理程序或云控制平面中的固定/动态内存。 supervisor 不需要匹配的数字：启动后它会读取来宾内核实际公开的内容并重新计算。 主机和来宾仍然需要单独安装，并且对于共享 admission，需要共享 federation 文件夹。

<a id="advanced-policy-changes"></a>
## 高级政策变更

普通用户应该保留这些未设置。 高级设置位于 Unix 上的 `~/.config/memory-supervisor/config.json` 和 Windows 上的 `$HOME\.config\memory-supervisor\config.json`：

```json
{
  "MEMORY_SUPERVISOR_POLICY_PROFILE": "performance"
}
```

手动编辑后，运行`memory-supervisor update`并检查`memory-status`。 `protect` 较早起作用，`performance` 较晚起作用，`balanced` 为默认值。 细粒度的`MEMORY_SUPERVISOR_MEM_*`、`MEMORY_SUPERVISOR_PSI_*`和过程观察覆盖可用于测量兼容性问题，但它们的顺序经过验证，无效组回退到自适应值。 斜率或原始阈值仍然是观察值； 共享执行器不变量仍然控制暂停权限。


<a id="verification-boundary"></a>
## 验证边界

该存储库通过 GitHub Actions 在 Linux、Windows 和 macOS 上运行一套共享测试套件。 它涵盖本机传感器、进程身份、策略决策、hook 行为、安装生命周期和发布工件。 受控的 Windows 和 WSL2 工作负载还会验证恢复边界附近的停止距离。 请参阅[测试矩阵](../testing/test-matrix.zh-CN.md)和[停止距离验证](../testing/stopping-distance.zh-CN.md)。

托管运行程序和确定性模拟验证可重复的产品合同。 他们并不声称能够重现每个物理主机、来宾、容器或长时间运行的工作负载组合。

<a id="what-is-deliberately-not-possible"></a>
## 故意不可能的事情

- 一条 Windows 命令无法设置 WSL、macOS VM 或 Linux VM 硬上限。
- WSL 实例无法暂停 Windows PID，并且来宾无法暂停主机 PID。
- Federation 无法将 16 GiB 主机 RAM 和 10 GiB WSL 容量组合到虚构的 26 GiB 池中。
- supervisor 在其可见 PID/权限域之外看不到已关闭的来宾或 CLI。
- Apple Silicon 上的 macOS 虚拟机不是英特尔 Mac 测试。 Rosetta 仅涵盖兼容性。
- 更改`MEMORY_SUPERVISOR_CAPACITY_MB`不会分配或回收物理内存。

请参阅[平台部署和federation](platforms.zh-CN.md)了解安装路径，以及[性能](performance.zh-CN.md)了解测量的每个实例占用空间。
