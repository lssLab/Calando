# 安全政策

<p align="center">
  <a href="SECURITY.md">English</a> · <a href="SECURITY.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="SECURITY.ja.md">日本語</a>
</p>

Memory Supervisor读取操作系统内存和进程元数据，写入私有本地状态，并且只能暂停或恢复经过验证的Claude Code和Codex 在其本地控制边界内进行处理。 它不读取提示、响应、源文件、浏览器数据或 IDE 内容。 有关完整的产品边界，请参阅[安全和数据/控制边界](../docs/guides/security.zh-CN.md)。

请通过此存储库的**安全→报告漏洞**表单报告可疑的漏洞。 请勿在公共问题中包含凭据、通知令牌、私有源代码或未编辑的本地路径。

对于报告，请包括受影响的版本、操作系统、最小复制以及编辑的相关 `memory-status --json` 输出。 对于非敏感错误，请使用普通的公共问题。
