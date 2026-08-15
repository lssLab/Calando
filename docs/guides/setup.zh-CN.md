# 安装、连接和支持的环境

<p align="center">
  <a href="setup.md">English</a> · <a href="setup.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="setup.ja.md">日本語</a>
</p>

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
- 当 App Server 或 CLI 在另一个 WSL 发行版、虚拟机或隔离容器中运行时，请在每个此类环境中安装。 Windows 和 WSL 自动查找其federation 路径； macOS 或 Linux 主机、动态内存虚拟机和容器连接同一台计算机上的共享文件夹。 连接后，争夺相同物理内存的环境将共享新工作决策。 固定内存虚拟机和其他计算机或云服务器独立保护自己。 请参阅[平台和多环境行为](platforms.zh-CN.md)了解边界。

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

对于容量规划，请使用**每个已安装的监视器 10 MiB**，而不是最小的样本。 有关详细条件和原始数据，请参阅[性能测量](performance.zh-CN.md)。

当一台物理计算机具有多个执行环境（Windows、WSL 发行版、虚拟机或隔离容器）时，请将其安装在运行 Claude Code 或 Codex 的每个环境中。 一个环境中的多个终端共享一台显示器。 在每个环境中安装并设置federation路径后，无论运行多少个内核，整个计算机都会自动共享最新的新工作决策。 每个监视器仍然只测量和控制自己的环境，因此它永远不会在另一个环境的 PID 上运行。 安装程序为 Windows 和 WSL 连接相同的本地共享文件夹； VM 或容器使用主机共享的本地文件夹作为其 federation 路径。 网络文件夹不用于连接不同的物理计算机或云服务器。 有关设置详细信息，请参阅[平台和多环境行为](platforms.zh-CN.md)。
