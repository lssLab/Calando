# Contributing

<p align="center">
  <strong>English</strong> · <a href="CONTRIBUTING.ko.md">한국어</a>
</p>

Contributions should preserve the product's central rule: keep useful work running until measured
risk requires the smallest reversible restriction, and never use uncertain ownership as authority
to pause a process.

Before submitting a change, run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
bash tests/run.sh
```

Windows-specific changes should also run `powershell -File .\tests\run.ps1`. Documentation changes
under `docs/` need matching English `.md` and Korean `.ko.md` files, working relative links, and no
personal paths, credentials, or internal documents unrelated to public use.

Open a focused issue or pull request that explains the user-visible behavior and the verification
performed. Security-sensitive reports belong in the private vulnerability-reporting form described
in [SECURITY.md](SECURITY.md).
