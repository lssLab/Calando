# Test coverage

<p align="center">
  <strong>English</strong> · <a href="test-matrix.ko.md">한국어</a>
</p>

The public suite covers the product path from policy decisions through installation, hook wiring,
recovery, and coordination across environments.

| Area | What is verified |
| --- | --- |
| Policy and braking | `ALLOW`, `HOLD`, and `DRAIN` across capacities and decline rates; cushioning order; attribution; targets selected per interval |
| Process safety | PID and start-identity revalidation, pause ownership, one candidate at a time, automatic and manual recovery |
| Claude Code | hook merge, supported events, fail-open behavior, and connection diagnostics after install or update |
| Codex CLI | paths and events for all seven hooks, trust and enablement diagnostics, and connection of existing sessions |
| Codex Desktop App | shared App Server discovery, logical-thread separation, exact/inferred/blind candidates, multiple windows, and server generation changes |
| Install and power | Unix and Windows install, update, uninstall, `on`/`off`, and preservation of existing user settings |
| Federation | kernel-local control, admission sharing for the same physical memory, and rejection of stale or invalid peers |
| Notifications and security | exact-terminal validation, deduplication, optional routes, and private state-file permissions |
| Release bundle | public-only source archives, checksums, and required platform binaries |
| Repository safety | public-file allowlist, no personal paths or credentials, paired Korean and English docs, and valid internal links |

GitHub Actions checks Rust builds, tests, and platform contracts on Linux x86-64,
Windows x86-64, Apple Silicon macOS, and macOS x86-64 under Rosetta. Operating-system signals and a
real near-exhaustion boundary cannot always be reproduced safely on hosted runners, so deterministic
tests are paired with bounded physical-machine verification.

See [Adaptive stopping distance](stopping-distance.md) for the calculation and controlled measured
result.
