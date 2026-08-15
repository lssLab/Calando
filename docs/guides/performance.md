# Performance and resident memory

<p align="center">
  <strong>English</strong> · <a href="performance.ko.md">한국어</a> · <a href="performance.zh-CN.md">简体中文</a> · <a href="performance.ja.md">日本語</a>
</p>

Memory Supervisor uses one synchronous Rust executable. It does not start a separate language
runtime or resident worker pool for background measurements or hook decisions.

## Measured native release builds

Each row contains 20 samples taken at 0.2-second intervals after warm-up. The WSL2 row measures an
installed service; CI rows measure the matching platform executable.

| Environment | Resident memory min / mean / max | Threads min–max | Stripped executable |
| --- | ---: | ---: | ---: |
| WSL2 Linux, physical service | 4.88 / **4.88** / 4.88 MiB RSS | 1 | 1.65 MiB |
| Ubuntu x86-64, CI | 3.50 / **3.52** / 3.54 MiB RSS | 1 | 1.69 MiB |
| Windows x86-64, CI | 4.15 / **4.20** / 4.25 MiB working set | 4–6 | 1.34 MiB |
| Apple Silicon macOS, CI | 3.38 / **4.35** / 5.13 MiB RSS | 1–3 | 1.41 MiB |

The normal control loop is single-threaded. Extra CI threads are bounded readers that exist only
while an operating-system sensor command is active. Every measured maximum is below the 10 MiB
planning allowance per instance.

## Hook and status latency

| Path | Samples | Result |
| --- | ---: | --- |
| WSL2 healthy-state hook | 200 | 4.29 ms min / 4.92 ms mean / **5.50 ms p95** / 6.13 ms max |
| WSL2 status JSON | 50 | 7.37 ms min / 8.17 ms mean / **8.80 ms p95** / 9.65 ms max |

Every WSL2 p95 is below 15 ms.

## Why it stays small

- One executable implements daemon, hook gate, status, control, notification, and integration
  functions.
- The normal daemon loop is synchronous; there is no Tokio runtime or resident worker pool.
- Linux and macOS hooks use a short healthy-state lease without starting the slow path while it is
  valid.
- Windows caches only the expensive process inventory for three seconds while reading global
  memory counters every second.
- Operating-system sensor commands and reader threads exist only during a bounded call.

## Interpreting the measurements

RSS and Windows working set are operating-system accounting values, not byte-exact unique physical
pages. Process count and native sensor implementations can move the result. For capacity planning,
use **10 MiB per installed supervisor instance**, not the smallest measured sample. Windows, each WSL
distribution, each VM, and each isolated container run their own instance, so their resident memory
adds separately.

The healthy-state hook fast path is used only while the daemon's short, current decision is valid.
An expired or path-mismatched decision falls back to the Rust gate, which validates local and
federated state again. This prevents a stopped daemon from leaving an old healthy decision active.
