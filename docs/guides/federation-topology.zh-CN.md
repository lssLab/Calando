# Federation 在一台机器上跨环境

<p align="center">
  <a href="federation-topology.md">English</a> · <a href="federation-topology.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="federation-topology.ja.md">日本語</a>
</p>

Memory Supervisor 在每个操作系统内核中运行一次，可以查看和控制自己的进程。 当多个内核竞争相同的物理内存时，Federation 让这些管理程序进行协调。

<a id="the-deciding-rule"></a>
## 决定规则

仅当两个条件都成立时，两个环境才会联合：

1. 他们的内存可以在相同的物理 RAM 上增长； 和
2. 他们可以通过主机本地共享目录见面，证明他们位于同一台计算机上。

Federation 分享当前的新作品决定。 它不会池化内存、移动工作或授予一个内核对另一内核进程 ID 的权限。

<a id="topology-matrix"></a>
## 拓扑矩阵

| 环境 | 动态共享主机 RAM？ | Federation 行为 |
| --- | --- | --- |
| 本机 Linux、macOS 或 Windows | 没有第二个内核 | 仅限本地supervisor |
| WSL2 及其 Windows 主机 | 是的 | 通过主机文件系统见面并共享admission |
| 多个 WSL2 发行版 | 是的 | 通过同一主机文件系统见面 |
| 主机和普通容器 | 是的 | 当安装相同的主机本地目录时联合 |
| 具有气球设备的动态内存虚拟机 | 是的 | 当主机本地共享文件夹可用时进行联合 |
| 固定内存虚拟机 | 不; 内存已分区 | 客人保持独立 |
| 云虚拟机或另一台物理计算机 | 没有可管理的共享 RAM | 每台机器保持独立 |

第二台计算机永远不会仅仅因为两台计算机都可以访问同一网络位置而成为federation 对等点。 网络共享并不是共享物理内存的证明。

<a id="detection-and-rendezvous"></a>
## 探测与交会

拓扑适配器分离了三个问题：

```text
OS adapter          -> how this kernel measures memory and controls a verified local PID
AI adapter          -> how Claude Code or Codex exposes sessions, agents, and tools
topology adapter    -> which co-resident kernels share RAM and where they exchange state
```

WSL2 使用已安装的 Windows 主机文件系统。 容器使用显式安装的主机本地目录。 动态VM使用可用的虚拟机管理程序共享文件夹； 否则它仅保留本地。 当支持的内存气球设备存在时，Linux 客户机被分类为动态；当不存在时，Linux 客户机被分类为固定。

`memory-status --all` 显示检测到的环境以及每个环境做出的决策。 只有在新鲜度窗口内刷新的状态才参与。

<a id="what-crosses-the-boundary"></a>
## 什么跨越了界限

联邦状态仅包含协调保护所需的信息：

- 内存容量、headroom、压力、速率；
- 当前admission和恢复状态；
- 诊断所需的环境、进程、终端和逻辑代理标识符；
- hook 连接运行状况和待处理事件状态。

它不包括提示、对话、模型响应、项目文件内容、完整命令行、凭据或通知机密。 每个supervisor只能暂停或恢复它在自己的内核中重新验证的进程。

<a id="host-local-safety-check"></a>
## 主机本地安全检查

集合点目录必须位于物理主机本地。 Linux 拒绝网络文件系统类型，同时允许主机本地挂载，例如 WSL2 的主机文件系统。 macOS 需要本地文件系统标志。 Windows 拒绝 UNC 路径和映射的远程驱动器。

如果拓扑或目录不明确，supervisor 会隔离环境，而不是信任可能的远程对等点。 结果可能会错过协调，但不会错过另一台机器上的过程控制。

<a id="multi-terminal-behavior"></a>
## 多终端行为

一个内核中的多个终端已被同一个本地supervisor观察到。 它们不会创建额外的守护进程或重复的内存总量。 如果 Windows、WSL2、容器或动态来宾在同一台计算机上添加另一个内核，则每个内核都会运行自己的 supervisor 和 federation 仅对齐新工作决策。

最糟糕的新鲜admission状态管理着共享资源。 进程遏制仍然是本地的和选择性的：supervisor从不发出对等环境的 PID 信号。

请参阅[平台部署](platforms.zh-CN.md)进行设置，并参阅[测试矩阵](../testing/test-matrix.zh-CN.md)进行验证覆盖范围。
