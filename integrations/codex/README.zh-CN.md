# Codex 适配器

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="README.ja.md">日本語</a>
</p>

首选顶级单行安装程序，然后使用 `memory-supervisor update` 进行升级或稍后添加 Codex 安装。 两条路径以原子方式合并适配器并保留不相关的hooks。 `hooks.json.template` 用于手动检查和自定义部署。 将 `__MEMORY_SUPERVISOR_ROOT__` 替换为绝对正斜杠路径，将 `__CODEX_HOOKS__` 替换为 Codex 主目录的绝对 `hooks.json` 路径。 如果不同的 Codex 家庭将其重新发现为项目 hook，则源路径可以让门忽略此用户 hook。

Codex hook JSON 与 SessionStart、UserPromptSubmit、SubagentStart、SubagentStop、Stop、PreToolUse 和 PostToolUse 兼容。 记录 SubagentStop 而不添加模型上下文。 Codex没有PostToolBatch，因此PostToolUse调用相同的转换通知路径。

该适配器需要 Codex 0.145.0 或更高版本以及 `hooks stable true`。 本机 `PreToolUse` 将 `spawn_agent` 映射到 `Agent`，因此普通 `codex`、`codex exec` 和 IDE 托管会话使用相同的预分配门。 不受支持的版本保持不变。 请参阅`../../docs/guides/usage-codex.md`。
