# 测试覆盖率

<p align="center">
  <a href="test-matrix.md">English</a> · <a href="test-matrix.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="test-matrix.ja.md">日本語</a>
</p>

公共套件涵盖了从策略决策到安装、hook布线、恢复和跨环境协调的产品路径。

| 区域 | 已验证什么 |
| --- | --- |
| 政策与制动 | `ALLOW`、`HOLD` 和 `DRAIN` 涵盖产能和递减率； 缓冲令； 归因; 每个时间间隔选择的目标 |
| 过程安全 | PID 和启动身份重新验证、暂停所有权、一次一个候选者、自动和手动恢复 |
| Claude Code | hook 安装或更新后合并、支持的事件、故障打开行为和连接诊断 |
| Codex CLI | 所有七个hooks的路径和事件、信任和启用诊断以及现有会话的连接 |
| Codex Desktop App | 共享App Server发现、逻辑线程分离、精确/推断/blind candidates、多个窗口和服务器生成更改 |
| 安装和通电 | Unix 和 Windows 安装、更新、卸载、`on`/`off`，以及保留现有用户设置 |
| Federation | 内核本地控制，admission共享相同的物理内存，以及拒绝陈旧或无效的对等点 |
| 通知和安全 | 精确终端验证、重复数据删除、可选路由和私有状态文件权限 |
| 发布捆绑包 | 仅供公共使用的源档案、校验和以及所需的平台二进制文件 |
| 存储库安全 | 公共文件白名单，无个人路径或凭据，匹配英语、韩语、简体中文和日语文档以及有效的内部链接 |

GitHub Actions 检查 Linux x86-64、Windows x86-64、Apple Silicon macOS 和 Rosetta 下的 macOS x86-64 上的 Rust 构建、测试和平台合约。 操作系统信号和真实的接近耗尽边界并不总是能够在托管运行器上安全地再现，因此确定性测试与有界物理机验证相结合。

计算和控制测量结果参见[自适应停止距离](stopping-distance.zh-CN.md)。
