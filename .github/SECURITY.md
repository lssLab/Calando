# Security policy

<p align="center">
  <strong>English</strong> · <a href="SECURITY.ko.md">한국어</a> · <a href="SECURITY.zh-CN.md">简体中文</a> · <a href="SECURITY.ja.md">日本語</a>
</p>

Memory Supervisor reads operating-system memory and process metadata, writes private local state,
and can pause or resume only verified Claude Code and Codex processes in its local control boundary.
It does not read prompts, responses, source files, browser data, or IDE contents. See
[Security and data/control boundaries](../docs/guides/security.md) for the complete product boundary.

Please report a suspected vulnerability through this repository's **Security → Report a
vulnerability** form. Do not include credentials, notification tokens, private source code, or
unredacted local paths in a public issue.

For a report, include the affected release, operating system, a minimal reproduction, and redacted
`memory-status --json` output when relevant. Use a normal public issue for non-sensitive bugs.
