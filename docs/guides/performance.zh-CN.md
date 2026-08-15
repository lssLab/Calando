# 性能和常驻内存

<p align="center">
  <a href="performance.md">English</a> · <a href="performance.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="performance.ja.md">日本語</a>
</p>

Memory Supervisor 使用一个同步 Rust 可执行文件。 它不会启动单独的语言运行时或常驻 worker 池来进行后台测量或 hook 决策。

<a id="measured-native-release-builds"></a>
## 测量的本机版本构建

每行包含 20 个样本，在预热后以 0.2 秒的间隔采集。 WSL2 行测量已安装的服务； CI 行测量匹配的平台可执行文件。

| 环境 | 驻留内存最小值/平均值/最大值 | 线程数最小–最大 | 剥离可执行文件 |
| --- | ---: | ---: | ---: |
| WSL2 Linux，物理服务 | 4.88 / **4.88** / 4.88 MiB RSS | 1 | 1.65 兆字节 |
| Ubuntu x86-64，CI | 3.50 / **3.52** / 3.54 MiB RSS | 1 | 1.69 米B |
| Windows x86-64、CI | 4.15 / **4.20** / 4.25 MiB 工作集 | 4–6 | 1.34 米B |
| Apple Silicon macOS、CI | 3.38 / **4.35** / 5.13 MiB RSS | 1–3 | 1.41 米B |

正常的控制循环是单线程的。 额外的 CI 线程是有界读取器，仅在操作系统传感器命令处于活动状态时才存在。 每个测量的最大值均低于每个实例 10 MiB 的规划限额。

<a id="hook-and-status-latency"></a>
## Hook 和状态延迟

| 小路 | 样品 | 结果 |
| --- | ---: | --- |
| WSL2 健康状态hook | 200 | 最小 4.29 毫秒 / 平均 4.92 毫秒 / **5.50 毫秒 p95** / 最大 6.13 毫秒 |
| WSL2 状态 JSON | 50 | 最小 7.37 毫秒 / 平均 8.17 毫秒 / **8.80 毫秒 p95** / 最大 9.65 毫秒 |

每个 WSL2 p95 都低于 15 毫秒。

<a id="why-it-stays-small"></a>
## 为什么它仍然很小

- 一个可执行文件实现daemon、hook门、状态、控制、通知和集成功能。
- 正常的daemon循环是同步的； 没有 Tokio 运行时或驻留 worker 池。
- Linux 和 macOS hooks 使用短期健康状态租约，在其有效时不启动慢速路径。
- Windows 仅缓存昂贵的进程库存三秒钟，同时每秒读取全局内存计数器。
- 操作系统传感器命令和读取器线程仅在有界调用期间存在。

<a id="interpreting-the-measurements"></a>
## 解释测量结果

RSS 和 Windows 工作集是操作系统计数值，而不是字节精确的唯一物理页。 进程计数和本机传感器实现可以改变结果。 对于容量规划，请使用**每个已安装的 supervisor 实例 10 MiB**，而不是最小的测量样本。 Windows、每个 WSL 发行版、每个虚拟机和每个独立的容器都运行自己的实例，因此它们的驻留内存会单独添加。

仅当 daemon 的简短当前决策有效时，才使用健康状态 hook 快速路径。 过期或路径不匹配的决策会退回到 Rust 门，该门会再次验证本地和联邦状态。 这可以防止停止的daemon使旧的健康决策保持活动状态。
