# 自适应停止距离

<p align="center">
  <a href="stopping-distance.md">English</a> · <a href="stopping-distance.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="stopping-distance.ja.md">日本語</a>
</p>

Memory Supervisor 并非旨在保持较低的内存使用量。 现有工作仍然不受限制，而headroom以稳定的速度下降。 它从新的工作开始，仅当测量的轨迹接近真实风险时才应用渐进的保护。

<a id="calculation"></a>
## 计算

```text
reaction time       = max(3 seconds, 5 × sensor interval)
recovery floor      = 0.5% of detected capacity, bounded to 256–1024 MiB
corroborated rate   = the largest sustained physical, commit, or tracked-work rate
                      (the same growth is counted only once)
recovery reserve    = recovery floor + corroborated rate × reaction time
                      (capped at 25% of detected capacity)
new-work floor      = recovery reserve + one minimum work block
                      (capped at 30% of detected capacity)
```

这些百分比只能防止噪音测量导致储备金膨胀。 实时决策结合了可用内存、其持续变化率、本机操作系统压力以及达到恢复储备的估计时间。

- `ALLOW / OBSERVE`：现有和拟议的工作仍然不受限制。
- `HOLD`：当大约剩余两个反应窗口或没有空间容纳一个新的最小工作块时，仅等待新的扩展。
- `DRAIN`：当确认的运动在一个反应​​窗口内达到储备时，只有属于归因代理的未来工作会减少最小必要量。
- 外部或不明原因的压力永远不会授权人工智能进程任意暂停。
- 本地进程暂停是最后的保护措施，仅在早期阶段无法停止增长并且确切目标已得到验证后才使用。

因此，仅高利用率不会导致制动。 缓慢、稳定的工作负载仍在继续； 快速下落会根据测量的速度获得更大的停止距离。

<a id="controlled-physical-machine-verification"></a>
## 受控物理机验证

| 物品 | 环境 |
| --- | --- |
| 主持人 | Windows 11 Pro、15.73 GiB RAM、Intel i5-1135G7、8 个逻辑 CPU |
| 客人 | WSL2 Ubuntu，x86-64 |
| 被测内核检测到的容量 | 7,941 兆字节 |
| 交换 | 16GiB |
| 人工智能工具 | Claude Code 2.1.217 和 Codex CLI 0.145.0 |
| Supervisor | Rust实测构建，传感器间隔1秒，用户内存上限关闭 |

AI 进程树外部的有界分配器以约 64 MiB/s 的速度接触实际内存，在可用容量低于 1 GiB 时减慢至约 32 MiB/s，在 350 MiB 处停止，保持 20 秒，然后释放全部内存。 因为外部程序产生了压力，所以正确的行为是停止新工作，而不责怪Claude Code或Codex。

| 观点 | 验证结果 |
| --- | --- |
| 开始 | 5,910 MiB 可用； 现有工作不受限制 |
| 第一刹车 | `HOLD` 1,143 MiB 可用，577.6 MiB 预留，8.8 秒预留 |
| 下一个刹车 | `DRAIN` 530 MiB 可用，409.6 MiB 预留，3.9 秒预留 |
| `DRAIN`期间 | 新的subagent开始推迟； 允许进行中的编辑 |
| 最低点 | 大约 350 MiB 可用； 没有终端冻结或强制终止 |
| 归因 | 外部压力； 无代理限制且无 PID 暂停 |
| 恢复 | 发布后有 5,902 MiB 可用； 稳定窗口后重新开放新工作 |

<a id="scale-verification"></a>
## 规模验证

确定性 Rust 测试保留了从 512 MiB 到 10 TiB 容量以及从 1 MiB/s 到 128 GiB/s 持续下降的相同时间关系。 预留十二秒不能进入`DRAIN`，七秒进入`HOLD`，四秒进入`DRAIN`。 多智能体测试还验证每个控制间隔仅从剩余阶段中选择所需的最小目标，并在轨迹改善后立即停止下一个限制。

<a id="scope-limits"></a>
## 范围限制

- 物理近边界测试涵盖了Windows+WSL2环境下的外部压力和自动恢复。
- 大型代理队列和极端内存大小通过确定性模拟进行验证。
- 暂停进程会停止额外的增长，但不会立即返回它已使用的内存。
- 在公共 Windows 二进制文件具有可信代码签名之前，Windows 安全设置可能会阻止执行。 请参阅[Windows 可执行文件信任](../guides/windows-signing.zh-CN.md)。
