# 贡献

<p align="center">
  <a href="CONTRIBUTING.md">English</a> · <a href="CONTRIBUTING.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="CONTRIBUTING.ja.md">日本語</a>
</p>

贡献应该保留产品的中心规则：保持有用的工作运行，直到测量的风险需要最小的可逆限制，并且永远不要使用不确定的所有权作为暂停流程的权力。

在提交更改之前，运行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
bash tests/run.sh
```

Windows 特定的更改还应运行 `powershell -File .\tests\run.ps1`。 `docs/` 下的文档更改需要匹配英文 `.md`、韩文 `.ko.md`、简体中文 `.zh-CN.md` 和日语 `.ja.md` 文件、工作相对链接，并且没有与公共使用无关的个人路径、凭据或内部文档。

打开一个重点问题或拉取请求，解释用户可见的行为和执行的验证。 安全敏感报告属于 [SECURITY.md](SECURITY.zh-CN.md) 中描述的私有漏洞报告形式。
