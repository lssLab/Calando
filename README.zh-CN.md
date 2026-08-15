<p align="center">
  <img src="assets/memory-supervisor-logo.png" width="59" alt="Calando — Claude Code &amp; Codex Memory Supervisor logo">
</p>

<h1 align="center">Calando</h1>

<p align="center">
  <strong>Claude Code &amp; Codex Memory Supervisor</strong>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <em>在Claude Code和Codex处理长时间运行的大规模工作负载时控制内存使用，帮助防止终端或应用程序冻结和意外会话退出。</em>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/releases/latest"><img src="https://img.shields.io/github/v/release/lssLab/Calando?display_name=tag&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust 1.88 or newer"></a>
  <a href="https://code.claude.com/docs/en/overview"><img src="https://img.shields.io/badge/Claude_Code-2.1.217%2B-D97757?style=flat-square&amp;logo=anthropic&amp;logoColor=white" alt="Claude Code 2.1.217 or newer"></a>
  <a href="https://learn.chatgpt.com/docs/codex/cli"><img src="https://img.shields.io/badge/Codex-CLI%200.145.0%2B%20%C2%B7%20Desktop-10A37F?style=flat-square&amp;logo=openai&amp;logoColor=white" alt="Codex CLI 0.145.0 or newer and Codex Desktop App"></a>
</p>

<p align="center">
  <a href="https://github.com/lssLab/Calando/actions/workflows/test.yml"><img src="https://github.com/lssLab/Calando/actions/workflows/test.yml/badge.svg?branch=main" alt="Test"></a>
  <a href="docs/guides/setup.zh-CN.md"><img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20WSL2%20%C2%B7%20macOS%20%C2%B7%20Windows-4C566A?style=flat-square" alt="Linux, WSL2, macOS, and Windows"></a>
  <a href="docs/guides/performance.zh-CN.md"><img src="https://img.shields.io/badge/daemon-%3C%2010%20MiB-0EA5E9?style=flat-square" alt="Supervisor planning value below 10 MiB"></a>
  <a href="docs/guides/security.zh-CN.md"><img src="https://img.shields.io/badge/telemetry-none-10B981?style=flat-square" alt="No usage telemetry"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-2563EB?style=flat-square" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#installation"><strong>安装</strong></a>·
  <a href="#how-it-works-in-30-seconds">工作原理</a> ·
  <a href="#common-commands">命令</a>·
  <a href="#documentation">文档</a> ·
  <a href="docs/detailed-guide.zh-CN.md">详细指南</a>
</p>

<a id="why-calando"></a>
## 解决什么问题

长时间使用Claude Code、Codex CLI或Codex Desktop App处理大型任务时，子代理、构建、测试和浏览器工具可能会同时运行。可用内存若快速减少，CLI终端可能停止响应或会话意外结束；在Desktop App中，共用一个App Server的多个对话也可能同时受到影响。无论哪种情况，都可能中断尚未返回的结果和当前工作流程。

Calando不会只因内存占用较高就限制工作。无论CLI还是Desktop App，只有在实际风险逼近时，它才会从新工作开始逐步放缓，并尽可能保留正在进行的工作和结果交付，避免会话突然中断。

保护强度不会一次升到最高。风险越近，保护措施越会逐级启用；状态恢复后，再按相反顺序逐级解除。

1. **自动判断** — 像平常一样启动`claude`或`codex`，或在Codex Desktop App中开始对话即可。Calando会自动区分CLI会话与App对话，根据内存容量、当前余量、下降速度和下一项工作需要的缓冲空间，自动设定保护标准。用户无需配置内存预算，也不用持续查看状态。
2. **不受限制地运行** — 即使内存占用较高，只要剩余空间和下降速度保持稳定，就不会限制代理和工具。
3. **保持性能并继续观察** — 只要剩余空间充足，就不会仅因下降速度快而立即采取限制。所有工作仍可继续，Calando只判断下降是否持续，以及实际风险是否正在逼近。
4. **先暂缓创建新的子代理、工作流和任务** — 当内存余量持续减少、风险逐渐接近，或已经没有足够空间启动下一项工作时，此阶段不会影响正在进行的工作，只会暂缓创建新的子代理、工作流和任务。该阶段本身不会阻止构建或测试启动，也不会暂停正在运行的程序，从而为当前工作完成和内存恢复留出缓冲时间。
5. **逐步收窄工作范围** — 风险进一步逼近时，首先全面阻止创建新的子代理、工作流和任务。只有在有可靠依据确认AI工作导致内存余量下降，或超过用户自行设置的上限时，才会把现有代理接下来可执行的工作按`全部工作 → 不再创建新的子代理、工作流或任务 → 不再启动构建、测试等高内存工作 → 仅允许交接、协调、状态查看、停止、恢复和少量读取`的顺序逐步收窄。

   Calando不会同时限制所有子代理。时间充足时，只把一个子代理从下一次工具调用起收窄一级；时间不足时，只对到达恢复线前必须限制的最小一组目标采取措施，然后重新测量内存。未被选中的代理和正在进行的工作保持不变。优先收窄工具范围的子代理按以下顺序选择：(1) 已再次确认其关联进程异常增长；(2) 当前或上一次工具用于创建代理、工作流、任务，或执行构建、测试等重型工作；(3) 已处于较窄阶段；(4) 关联进程更快到达恢复线；(5) 启动时间更晚。

   只有当所有子代理都已进入最窄阶段、风险仍然存在时，才会限制主代理。但如果主代理已被再次确认是主要原因，而且先限制子代理会来不及，则会先将主代理收窄一级。如果原因只来自外部程序，现有AI工作会继续运行；仅暂缓创建新的子代理、工作流和任务，以及在操作系统内存压力严重时启动大型工作。
6. **最后才暂停一个运行进程** — 如果采取以上措施后风险仍在，并且确认Claude Code或Codex的某个运行进程持续增长，Calando才会暂停这一个进程，而不是结束它。措施会立即显示在终端中，主代理也会在下一项工作开始前收到相同信息。
7. **按相反顺序恢复** — 内存状态稳定后，从仅允许结果交付的阶段开始逐级恢复工作范围，暂停的进程也会一次恢复一个。

目标不是让程序少用内存，而是在保护Claude Code、Codex CLI终端会话和Codex Desktop App对话的同时，尽可能长时间维持较高性能。

<a id="installation"></a>
## 安装

打开适合您环境的**终端**，然后粘贴匹配的一行命令。 无需准备 Git、Python、Rust 或单独的安装程序。 安装范围仅限于您当前的用户，因此不需要 `sudo` 或管理员 shell。

<a id="linux-wsl2-macos-terminal"></a>
### Linux · WSL2 · macOS 终端

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

<a id="windows-powershell-terminal"></a>
### Windows PowerShell 终端

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

命令完成后，后台服务会立即启动，并自动连接检测到的Claude Code和Codex Hook。正在运行的AI程序和任务不会被关闭。

> [!IMPORTANT]
> Windows可执行文件目前正在接受[SignPath Foundation](https://signpath.org/)的认证审核，
> 因此在审核完成前，Windows 11需要关闭Smart App Control后才能使用。
>
> - **Windows 11：** 安装前将Smart App Control设为**关闭**，并在使用Windows原生版本期间保持关闭。
> - **Windows 10：** 没有Smart App Control，可直接安装，无需额外设置。
> - **使用WSL引擎的Codex App：** 使用WSL终端命令安装，无需修改Windows安全设置。
>
> 关于Windows 11何时可以重新开启该功能，以及哪些环境会阻止安装，请参阅
> [安装、连接与支持环境](docs/guides/setup.zh-CN.md#windows-powershell-terminal)。

<a id="connect-the-programs-you-use"></a>
### 连接您使用的程序

| 程序 | 安装后做什么 |
| --- | --- |
| **Claude Code** | Hook会自动连接。如果已经在工作，直接继续即可。 |
| **Codex CLI** | 在准备使用的CLI中打开`/hooks`，确认Memory Supervisor的7项Hook全部为**已信任・已开启**，然后直接继续工作。只有安装前已经单独打开的其他CLI，需要在当前工作结束后重启一次。 |
| **Codex Desktop App** | 在**设置 → Hooks**中信任并开启全部7项。返回现有对话，发送原本准备发送的下一条请求即可，无需新建对话或重启App。如果还看不到这些项目，最多等待60秒后重新打开设置。 |

<a id="verify-installation"></a>
### 验证安装

```bash
memory-status --connections
```

- `Core daemon CONNECTED`：后台管理程序运行正常。
- `Claude Code CONNECTED`：支持的版本和用户钩子已连接。
- `Codex CONNECTED`：所有七个 CLI 挂钩均已安装、启用且受信任。
- `Codex App ACTIVE`：应用程序挂钩已准备就绪，并且来自现有或新任务的实际调用已到达。
- `NOT DETECTED` 对于您不使用的程序来说是正常的。

如果某行不同，则仅根据其报告的内容采取行动。 每个异常和确切的实时安装行为均位于[安装、连接和支持的环境](docs/guides/setup.zh-CN.md)中。

<a id="uninstall"></a>
### 卸载

要删除 Calando，请在安装它的每个环境中运行一次：

```bash
memory-supervisor uninstall
```

它会删除后台服务、可执行文件以及 Calando 拥有的挂钩和技能连接，同时保留状态和用户设置。

<a id="how-it-works-in-30-seconds"></a>
## 30 秒内如何运作

Calando不会挡在Claude Code或Codex前面代替它们执行命令。每个操作系统环境中会运行一个小型监控程序，查看可用内存、下降速度、内存压力信号和AI工作带来的增长。Hook会在新工作开始前获取最新判断。

```text
┌──────────────────────┐    memory / PID    ┌──────────────────────┐
│ OS environment       │ ─────────────────► │ Calando              │
└──────────────────────┘                    │ forecast / brake     │
                                            └──────────┬───────────┘
                                                       │ decision
┌──────────────────────┐      pre-run hook  ┌──────────▼───────────┐
│ Claude Code / Codex  │ ─────────────────► │ allow / hold         │
│ CLI / App thread     │ ◄── reason/state ─ │ explain / recover    │
└──────────────────────┘                    └──────────────────────┘
```

1. **自动判断** — 根据内存容量、剩余空间、短期和长期下降速度，以及下一项工作的预计增长，自动计算保护标准和制动距离。
2. **性能优先** — 占用高但稳定时不限制；即使下降较快，只要空间充足，也会先观察，直到实际危险真正接近。
3. **先从新工作留出缓冲** — 只有危险接近时，才先暂缓创建新的子代理、工作流和任务；必要时，从选定代理的下一次工具调用开始逐级收窄范围。
4. **可逆的最后手段** — 只有在所有缓冲措施之后风险仍在，并且准确确认某个Claude Code或Codex进程持续增长时，才会暂停这一个进程，而不会结束它。
5. **按相反顺序恢复** — 空间稳定后逐级恢复工作范围，暂停的工作也会一次恢复一个。

<a id="cli-versus-codex-desktop-app"></a>
### CLI 与 Codex Desktop App

| Claude Code和Codex CLI | Codex Desktop App |
| --- | --- |
| 终端会话和子进程是分开的，因此因果进程和控制目标可以相对精确地连接起来。 | 对话被区分为App Server内的**逻辑线程**，但内存是共享的。 它们不像独立的 CLI 进程那样进行测量。 |
| Hook识别主代理、子代理和工具；最后的物理措施也只会暂停一个经过重新核对的本地PID。 | Hook按对话控制新工作，并把最近的工具、子代理、活动时间和App Server增长结合起来判断。无法确认归属时，Calando不会假定共享内存属于某一个线程，而是先针对共同风险缓冲新工作。只有全部渐进措施之后仍确认持续增长，才可能暂停App Server；这是极少发生的最后一步。 |

每个 Windows、WSL2、macOS、Linux、VM 或隔离容器环境中运行一个 Supervisor。 当共享相同物理内存的环境通过联合连接时，它们共同决定何时可以开始新工作以及何时可以解除限制，而每个主管仅控制自己的进程。

完整的阶段策略、架构和联合拓扑都保留在 [Calando 工作原理](docs/guides/how-it-works.zh-CN.md) 中。

<a id="common-commands"></a>
## 常用命令

| 目的 | 命令 |
| --- | --- |
| 当前内存和保护状态 | `memory-status` |
| 每个互联环境 | `memory-status --all` |
| Claude Code 和 Codex 挂钩连接 | `memory-status --connections` |
| 更新程序并重新连接集成 | `memory-supervisor update` |
| 在此环境下关闭或打开保护 | `memory-supervisor off` / `memory-supervisor on` |
| 显示通知路线 | `memory-supervisor notifications show` |

请参阅[操作、通知和恢复](docs/guides/operations.zh-CN.md)，了解暂停工作处理、自动恢复、手动恢复、可选内存硬上限配置以及 Discord 和 Telegram 通知设置。

<a id="supported-environments-and-safety-boundary"></a>
## 支持的环境和安全边界

| 项目 | 支持或边界 |
| --- | --- |
| **操作系统** | 64 位 Intel/AMD 上的 Linux 和 WSL2、Apple Silicon 和 Intel 上的 macOS、64 位 Intel/AMD 上的 Windows 10 或 11 |
| **人工智能程序** | Claude Code 2.1.217 或更高版本、Codex CLI 0.145.0 或更高版本、Codex Desktop App |
| **常驻内存** | 各平台实测最大5.13 MiB；每个已安装监控程序的设计值低于10 MiB |
| **网络** | 正常监控不会发送网络流量或使用情况遥测数据。 网络访问仅适用于安装和更新，或用户明确启用的 Discord 和 Telegram 通知。 |
| **从未阅读** | 提示、对话、模型响应、项目文件内容、进程内存内容或 Claude/ChatGPT 凭据 |
| **从不控制** | 其他程序，例如浏览器和 IDE、另一个操作系统环境中的 PID，或内存、交换和 VM 设置 |
| **自动物理措施** | 最多只会可逆地暂停一个经过准确复核的Claude Code或Codex工作进程；不会自动结束或强制终止进程。 |

请参阅[安全性](docs/guides/security.zh-CN.md)了解完整的数据和流程边界，[性能](docs/guides/performance.zh-CN.md)了解测量结果，以及[安装、连接和支持的环境](docs/guides/setup.zh-CN.md)了解特定于平台的条件。

<a id="documentation"></a>
## 文档

| 话题 | 文档 |
| --- | --- |
| 安装、运行中连接、Hook信任、Windows、WSL2、macOS和Linux | [安装、连接与支持环境](docs/guides/setup.zh-CN.md) |
| 渐进制动、CLI与Codex App架构、盲控和联动 | [工作原理与架构](docs/guides/how-it-works.zh-CN.md) |
| 终端、操作系统、Discord与Telegram通知、命令、暂停和恢复 | [运行、通知与恢复](docs/guides/operations.zh-CN.md) |
| 在一份文档中连续阅读原始详细 README | [详细指南](docs/detailed-guide.zh-CN.md) |
| 查找安全性、性能、测试和所有专家参考 | [文档索引](docs/README.zh-CN.md) |

<a id="verification"></a>
## 验证

该项目运行自动化 Rust 测试、安装/更新/卸载 E2E、挂钩合约检查、存储库隐私边界检查以及 Linux、Windows 和 macOS 平台验证。 请参阅[测试覆盖范围](docs/testing/test-matrix.zh-CN.md)和[自适应停车距离](docs/testing/stopping-distance.zh-CN.md)了解公开验证范围。

请参阅[安全策略](.github/SECURITY.zh-CN.md)来报告漏洞，并参阅[贡献指南](.github/CONTRIBUTING.zh-CN.md)来处理该项目。

<a id="license"></a>
## 许可证

[MIT](LICENSE)
