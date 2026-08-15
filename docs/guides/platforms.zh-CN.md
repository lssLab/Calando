# 平台部署及federation

<p align="center">
  <a href="platforms.md">English</a> · <a href="platforms.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="platforms.ja.md">日本語</a>
</p>

<a id="one-supervisor-per-protected-user-and-pid-control-environment"></a>
## 每个受保护用户和 PID 控制环境一个 supervisor

supervisor 读取对其操作系统用户和 PID 命名空间可见的进程清单。 一次安装涵盖同一 PID 控制环境中用户的 Claude Code 和 Codex 会话，无论它们是从 Windows 终端、iTerm、VS Code、tmux、SSH 还是其他终端表面打开。

supervisor 从不在其本地 PID 控制环境之外发出信号。 在主机中安装一次，在每个应受保护的 WSL 发行版、VM 或 PID 隔离容器中安装一次。 WSL 2 发行版可以共享托管 VM 和内核，同时保留单独的 PID 命名空间，因此它们仍然需要单独的本地实例。 每个实例都会将一个小型状态快照发布到共享的federation目录。 Hooks 使用最近十秒中最差的有效快照来进行新的扇出admission，而只有本地supervisor 可以暂停本地PID。

| 基础操作系统 | 上面的环境 | 需要安装 | Federation 边界 |
| --- | --- | --- | --- |
| 视窗 | 一个或多个 WSL2 发行版 | Windows 以及每个 WSL 发行版 | 每个 WSL 实例都会自动检测 Windows 用户的 `.memory-supervisor/instances` |
| Windows、macOS 或 Linux | 动态内存虚拟机 | 主人加上每一位客人 | 仅通过主机本地共享文件夹连接实际竞争相同物理 RAM 的双方 |
| Windows、macOS 或 Linux | 固定内存虚拟机 | 主人加上每一位客人 | 保持各方独立； 不要跨越固定分配边界进行联合 |
| Linux 内核（本机 Linux、WSL 或桌面 VM） | 一个或多个 PID 隔离容器 | 内核主机环境加上每个隔离的容器 | 在该内核内共享主机本地卷 |
| 任何嵌套组合 | 每个受保护的 PID 命名空间 | 每个动态共享内存边界一个连接 | 不要将一个目录延伸到固定虚拟机边界或网络上 |

<a id="codex-app-follows-the-app-server-environment"></a>
### Codex App 遵循 App Server 环境

Codex App窗口及其执行引擎不必在同一操作系统环境中运行。 Memory Supervisor 遵循`codex ... app-server` 进程，而不是桌面窗口：

- 使用 WSL 引擎的 Windows Codex App 受该 WSL 发行版中安装的 Supervisor 的保护。 它检测 WSL App Server，解析该进程的活动 `CODEX_HOME`，并管理其逻辑线程、hook 决策、WSL child 工具和 WSL 端物理制动器。 这不需要未签名的本机 Windows Supervisor 或Smart App Control 更改。
- 该 WSL 实例无法测量或暂停 Windows 应用程序 UI 进程或单独的 Windows 本机Claude Code 或 Codex CLI。 当必须覆盖这些 Windows 进程时，还要安装 Windows Supervisor； 然后，Windows 和 WSL 实例通过 federation 共享 admission，同时保留本地 PID 控制。
- 本机 Windows 或 macOS App Server 在该操作系统中使用 Supervisor。 实际在 Linux 中运行的 App Server、另一个 WSL 发行版、VM 或 PID 隔离的容器在该环境中使用 Supervisor。 即使请求它的窗口或客户端在其他地方，同样的规则也适用。
- 固定内存虚拟机或远程计算机可以独立保护自身。 仅联合动态竞争相同物理 RAM 的执行环境。

这是一般的进程边界规则，而不是硬编码的 Windows/WSL 异常。 共享 Windows/WSL `CODEX_HOME` 仅作为文件布局情况进行处理：hook 文件保留两个本机命令字段，但每个命令仍然仅到达其自己环境中的 Supervisor 和 PID。

如果没有共享路径，每个实例仍然保护自己的本地环境。 仅跨环境admission和组合`memory-status --all`视图不可用。

federation 阅读器未将 Windows/WSL 对列入白名单。 一个主机本地内存边界内的 Windows、WSL、Linux 和 macOS 对等体使用相同的快照合约，并且仅应用过去 10 秒中最严格的有效新工作决策。 Windows/WSL 的特殊之处仅在于可以自动发现共享路径。 macOS 和 Linux 主机、动态 VM、容器和嵌套环境使用共享文件夹作为其实际边界。 为具有相同主机名的克隆来宾或容器提供唯一的`MEMORY_SUPERVISOR_INSTANCE`值。

共享federation目录，而不是`CODEX_HOME`。 Hook 文件、信任状态和 PID 权限保留在每个环境的本地。 仅当 Windows 应用程序和 WSL 运行时真正使用相同的 `CODEX_HOME` 时，一个 Codex 文件才会同时保留 Windows 和 POSIX 命令字段。 这是一个 hook 文件布局例外，而不是对 federation 操作系统组合的限制。

<a id="runtime-and-startup"></a>
## 运行时和启动

公开发布安装不需要或安装 Git、Python 或 Rust。 它从同一版本下载当前操作系统和架构的源包和本机二进制文件，并检查两个 SHA-256 值。 它使用粘贴命令中的下载程序以及操作系统的标准存档和 SHA-256 支持。 可以使用 Rust 1.88 或更高版本在本地构建手动开发检查。

Windows 10 没有 Smart App Control。 Supervisor 可执行文件的[最低 Windows 基准](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html) 也是 Windows 10，并且那里提供了所需的内存和进程设施。 它不需要 SAC 设置，但 SmartScreen 提示仍然需要检查下载源。 即使校验和正确，Windows 11 Smart App Control 也可以阻止新的未签名可执行文件，并且它不提供每个应用程序的异常。 因此，在公共 Windows 工件被签名之前，本机 Windows 11 路径需要在安装和运行可执行文件时使 Smart App Control 保持关闭状态。 Windows 安装程序会在切换之前执行候选服务，如果 Windows 拒绝，则不会影响现有服务。 从 26100.8117 开始的 Windows 11 24H2 版本和从 26200.8117 开始的 25H2 版本可以接收可逆的开/关控制，但推出是渐进的：检查 `winver` 并确认在关闭 SAC 之前重新启用控件可见。 没有该控制的较旧版本或设备可能需要重置或重新安装才能将其重新打开。 WSL 二进制文件不需要更改 Windows Smart App Control，但它们仅保护 WSL 内的进程。 处于 S 模式的 Windows 11 和仍阻止可执行文件的组织应用程序控制策略不受本机路径支持。 请参阅 [Windows 签名运行手册](windows-signing.zh-CN.md)、Microsoft 的 [Smart App Control 常见问题解答](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)、[推出说明](https://support.microsoft.com/en-au/help/5079391) 和 [代码签名指南](https://learn.microsoft.com/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)。

| 平台 | 用户级启动机制 |
| --- | --- |
| Linux/WSL | `~/.config/systemd/user/memory-supervisor.service`; 安装者拥有的 linger（可用时） |
| macOS | `~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist` |
| 视窗 | `MemorySupervisor` 用户登录时的计划任务 |
| 没有用户 systemd 的 Unix | PID 监督后备； 立即启动，但引导启动是手动的 |

`memory-supervisor update` 在可能的情况下更新结账，验证并激活本机运行时，重新加载本地服务，并重新连接每个检测到的支持的 CLI。 在 daemon 切换期间，它从不发出代理 PID 信号，新的 daemon 会从 `runtime.json` 重新加载暂停的身份。

最安全的更新时间是在活动 CLI 会话之间。 实时更新通常会保留它们，但可能会存在短暂的故障开放保护间隙。 更新后始终运行`memory-status --connections`。 只有实际的Codexhook定义更改需要用户再次亲自信任该CLI进程的`/hooks`或桌面应用程序**设置→Hooks**； 重新启动永远不能取代信任。 应用程序设置更改会刷新共享 App Server 加载的现有任务，但不会刷新单独的 CLI 进程。 Claude Code 没有每hook 哈希批准：其用户设置 hook 处于活动状态，无需类似 Codex 的项目审核，但交互式 Claude 保留所有设置文件 hooks，直到用户接受当前文件夹或其父文件夹之一的工作区信任。 受信任的运行会话通常会自动重新加载以后的用户设置更改。 这些信任和重新加载边界的持续时间比daemon重新启动本身还要长。

支持的运行时是本机 Rust 二进制文件。 重新启动 supervisor 会重新加载其持久状态，同时应尽可能在活动 CLI 会话之间替换已安装的版本。

<a id="what-happens-after-a-machine-restart"></a>
### 机器重启后会发生什么

- Linux 和 WSL 使用启用的用户单元。 安装者拥有的linger让设备以用户管理器启动； WSL 服务仅在 WSL 分发本身启动后才开始。
- macOS 在 GUI 登录时加载 `RunAtLoad` 和 `KeepAlive` LaunchAgent。
- Windows 在用户登录时启动计划任务，并以一分钟的间隔重试意外的daemon 退出最多五次。 该任务请求控制台分离，仅当该控制台单独属于 daemon 进程时，daemon 才会执行该操作。 因此，后台启动时不会打开面向用户的黑色窗口，并且其定期 PowerShell 传感器使用`CREATE_NO_WINDOW`； 从现有终端运行的命令保留该共享终端。
- Claude Code 和 Codex hook/技能文件仍保持安装状态。 登录后打开一个新的 AI CLI 会话并运行`memory-status --connections`。 只有实际的hook哈希值更改才需要审核：在Codex CLI中使用`/hooks`，在Codex App中使用**设置→Hooks**。
- 重启不是更新。 不要运行`memory-supervisor update`，除非需要重新应用源、安装程序或以后安装的 CLI。

<a id="federation-paths"></a>
## Federation 路径

- 默认值：`~/.memory-supervisor/instances`
- 覆盖：`MEMORY_SUPERVISOR_FEDERATION_DIR`
- 持久指针：`~/.memory-supervisor/federation-dir`
- 状态指针：`~/.memory-supervisor/state-dir`
- WSL 自动查找 Windows 用户的共享实例目录。
- WSL 默认实例名称包括 `WSL_DISTRO_NAME`，因此即使 WSL 共享 Windows 主机名，同一主机上的 Ubuntu 和 Debian 也不会相互覆盖。
- 陈旧、格式错误或错误的快照永远不会参与admission。
- 如果非 WSL 克隆来宾仍然共享身份，请将 `MEMORY_SUPERVISOR_INSTANCE` 设置为唯一名称。

```bash
memory-status --all
```

Federation是全局背压，不是调度程序。 它既不迁移 workers 也不向另一个操作系统拥有的 PID 发送信号。

<a id="multiple-sshtmux-sessions-and-vps-deployments"></a>
## 多个 SSH/tmux 会话和 VPS 部署

一个用户级安装涵盖同一 PID 控制环境中跨 SSH 登录、终端窗口和 `tmux` 窗格的用户 Claude Code 和 Codex 会话。 他们共享一个admission 决策，而不是通过独立监管者进行竞争。 多用户服务器应为每个受保护的操作系统用户安装一次； `/proc` 限制（例如`hidepid`）可以阻止一个用户检查另一用户的进程，并且产品不会绕过该边界。

受限 VPS 是一种自然的部署形式，因为本机 cgroup 上限、PSI、交换/回收以及所有同一用户远程会话都提供相同的策略。 启用已安装的用户服务，并在适当的情况下启用用户停留，以便在没有打开的 SSH shell 的情况下它仍然可用。 桌面操作系统通知通常在无头服务器上不可用，因此请使用强制性的hook/终端操作消息以及可选的 Discord 或 Telegram。 Linux 和 cgroup 合同测试涵盖了这条路径，但尚未声称是完整的长达数小时的真实 VPS 模型浸泡。

<a id="native-capacity-and-sensors"></a>
## 原生容量和传感器

| 平台 | 容量和可用内存 | 压力和过程 |
| --- | --- | --- |
| Linux/WSL | `/proc/meminfo` 受每个封闭 cgroup v1/v2 上限的限制 | 操作系统低内存信号（PSI、回收、交换和内存不足计数器）、`/proc/<pid>`、PID 启动标记、TTY 标识 |
| macOS | `sysctl hw.memsize`; 来自 `vm_stat` 的空闲/非活动/可清除页面 | 暴露时的内核压力级别、主要 `vm_stat` 页面输出/压缩趋势、`ps` 启动时间和 TTY |
| 视窗 | `GlobalMemoryStatusEx` 物理内存 | `GetPerformanceInfo`提交headroom，缓存的CIM进程清单，创建身份，控制台/ConPTY证据 |

Linux 检查每个 cgroup 祖先，而不是信任无限的叶子。 如果无法读取 macOS 压力级别 sysctl，`vm_stat` 计数器仍然可用，但本机压力被报告为未知/低置信度，压力传感器错误会暴露，并且 admission 保守地保持不变。 失败的`vm_stat`也是真正的传感器故障。 macOS 使用 RSS 作为每个进程的近似值，因为匿名 RSS 不以相同的形式公开。 Windows 每次更新都会刷新廉价的全局计数器，并将昂贵的进程库存缓存三秒钟。

每个平台都会报告 `sensor_ok`、`sensor_errors` 和 `last_process_scan_ts`。 失败的进程扫描可能会留下最后的清单以供诊断，但过时的清单不会导致新的泄漏暂停或暂停的 PID 协调。

自适应admission使用实际headroom、短/长斜率、耗尽时间、本地遇险、最近爆发和自动可恢复储备。 它不保留固定百分比的 RAM。 稳定高使用量可保持开启状态； 在持有之前观察到大量headroom的快速下跌，持有是为接近储备、持续空头 TTE、明确的硬上限或降级保护而保留的。

<a id="wsl2-capacity-on-a-16-gib-windows-host"></a>
## 16 GiB Windows 主机上的 WSL2 容量

Microsoft 目前将 WSL2 的默认 `memory` 上限记录为 Windows RAM 的 50%。 因此，在 16 GiB 主机上，删除显式 `memory=8GB` 行通常会留下相同的 8 GiB 上限，而不是为繁重的 Linux CLI 会话提供更多空间。 `memory=10GB` 是几个繁重的 WSL 任务以及 Windows 应用程序的示例； `memory=12GB` 是一个较大的示例，仅当 Windows 端工作负载较轻时才需要考虑。 supervisor 也不是默认或自动推荐。

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

`memory` 是最大值，而不是 10 GiB 预分配。 supervisor 仍然需要 VM 上限，因为精确 PID 暂停会停止进一步执行，但不会立即返回常驻内存，并且它不会控制不相关的 Linux 或 Windows 应用程序。 较高的 WSL 上限会增加代理headroom，但会减少主机对外部应用程序的最坏情况保留； federation 观察双方，但无法将一个内核的 PID 信号转化为另一内核的内存回收。

对 `.wslconfig` 的更改需要 WSL VM 停止才能生效。 仅在空闲边界运行 `wsl --shutdown`，因为它会立即终止每个正在运行的 WSL 发行版及其内部的每个 CLI 会话。 请参阅 Microsoft 的[高级 WSL 设置](https://learn.microsoft.com/windows/wsl/wsl-config) 和 [`wsl --shutdown` 命令](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown)。

<a id="optional-local-cli-memory-budget"></a>
## 可选的本地 CLI 内存预算

预算**默认关闭**。 它是对此安装的控制环境可见的所有 Claude Code 和 Codex 树的一个聚合上限，而不是每个 CLI 限制或池化 Windows+WSL 配额。

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

`6` 只是 GiB 语法示例（`memory-supervisor hard-cap set <MB>` 是 MB 精度别名）。 在 Windows、WSL、每个 VM 或每个隔离的容器中单独运行该命令。 由于这些控制环境可以共享一台物理机，因此`memory-supervisor budget`首先报告该环境的理论最大值以及对等环境明确预算后当前可能的总数； `budget set` 拒绝不再适合的请求（指定在哪里减少多少），并要求确认当前可能总数的 90% 或更多，或者当机器范围内的显式预算总数达到物理估计的 90% 时。 环境的默认分配（例如未配置的 WSL VM 上限）永远不会被计为声明。 在天花板附近，首先举行新的扇出。 在此之上，每个反应间隔最多可以暂停一个已验证的增长worker/支持PID； lead 仍然是最后的手段，需要精确的恢复可见性。 暂停会停止进一步执行，但不会立即返回常驻内存，因此当需要字节精确配额时，请使用 cgroup/container/VM 限制。

<a id="persistent-advanced-settings"></a>
## 持久的高级设置

正常运行不需要配置文件。 高级覆盖位于`~/.config/memory-supervisor/config.json`； 具有相同名称的环境变量优先。 上述预算命令是设置或清除上限的首选方式。

```json
{
  "MEMORY_SUPERVISOR_TICK_S": 1,
  "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S": 3,
  "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB": 32768
}
```

`MEMORY_SUPERVISOR_TICK_S` 接受 0.25 到 5 秒。 五秒的上限将下一个样本保留在十秒的状态新鲜度和五秒的租赁合同内。 超出范围的值会回退到一秒并出现在 `configuration_error` 中。

路径/引导设置（例如 `MEMORY_SUPERVISOR_DIR`、`MEMORY_SUPERVISOR_FEDERATION_DIR` 和 `MEMORY_SUPERVISOR_FORCE_PLATFORM`）不属于此 JSON 文件。 手动高级编辑后，运行`memory-supervisor update`并使用`memory-status`进行验证。

<a id="pause-resume-and-restart"></a>
## 暂停、恢复和重新启动

- Unix 上的 `SIGSTOP` 和 Windows 上的本机进程暂停保留 PID 和内存中会话。
- `memory-supervisor resume <pid>` 在继续之前重新验证 PID 和启动标识。
- 仅当恰好有一个托管 PID 暂停时，才接受`memory-supervisor resume`。
- 控制意图在信号之前持续存在，并且仅在 daemon 确认后报告完成。
- 重新启动 supervisor 会重新加载其事件分类帐，并且不会自动恢复代理。
- 重新启动代理 CLI 有所不同：使用 AI CLI 的脚本/会话恢复功能。
- 远程事件必须通过其 `source` 字段命名的操作系统进行控制。

AI CLI/模型上下文在下一个实际hook边界处交付，该边界可能晚于操作系统恢复。 准确的终端、操作系统、Discord 和 Telegram 操作通知是独立尝试的。


<a id="turn-the-whole-installation-on-or-off"></a>
## 打开或关闭整个安装

```bash
memory-supervisor off
memory-supervisor on
```

一个 `off` 命令可禁用当前 OS/PID 控制环境的服务和自动启动，并在 `~/.memory-supervisor/power-off` 中保留该选择。 安装的Claude Code和Codexhooks和技能保持连接，但每一个hook都默默地穿过。 `memory-status` 和 `--connections` 报告有意 `OFF`，`memory-supervisor update` 保留它。 `on` 删除标记，恢复自动启动，并验证是否发布了新状态。

当 supervisor 拥有已暂停的 PID 或进程控制操作处于待处理状态时，`off` 会拒绝，因此如果没有 daemon 来恢复进程，它就无法让进程陷入困境。 Windows、每个 WSL 发行版、VM 来宾和 PID 隔离的容器都有单独的服务和 PID 命名空间； 在您想要切换的每个环境中运行一次命令。

<a id="low-level-service-recovery-commands"></a>
## 低级服务恢复命令

```bash
# Linux / WSL
systemctl --user restart memory-supervisor.service
systemctl --user is-active memory-supervisor.service

# macOS: restart a loaded agent
launchctl kickstart -k gui/$(id -u)/io.github.lsslab.memory-supervisor

# macOS: explicitly unload, then load again
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist

# Windows
schtasks /End /TN MemorySupervisor
schtasks /Run /TN MemorySupervisor
```

使用这些命令来修复意外的服务故障，而不是作为产品电源开关。 如果服务在没有 `off` 标记的情况下不可用，hooks 将无法打开，而不是禁用 CLI，并且 `memory-status` 会报告过时或丢失的 supervisor 以及由此产生的保护间隙。
