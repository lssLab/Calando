# Adaptive stopping distance

<p align="center">
  <strong>English</strong> · <a href="stopping-distance.ko.md">한국어</a>
</p>

Memory Supervisor is not designed to keep memory use low. Existing work remains unrestricted while
headroom falls at a stable rate. It starts with new work and applies gradual protection only when
the measured trajectory approaches real risk.

## Calculation

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

The percentages only prevent a noisy measurement from inflating the reserve. Live decisions combine
available memory, its sustained rate of change, native operating-system pressure, and the estimated
time until the recovery reserve is reached.

- `ALLOW / OBSERVE`: existing and proposed work remain unrestricted.
- `HOLD`: only new expansion waits when roughly two reaction windows remain or there is no room for
  one new minimum work block.
- `DRAIN`: when confirmed motion reaches the reserve within one reaction window, only future work
  belonging to attributed agents is reduced by the minimum necessary amount.
- External or unattributed pressure never authorizes an arbitrary AI-process pause.
- A local process pause is the final safeguard, used only after earlier stages fail to stop growth
  and an exact target has been verified.

High utilization alone therefore does not cause braking. Slow, stable workloads continue; a rapid
fall receives the larger stopping distance demanded by its measured speed.

## Controlled physical-machine verification

| Item | Environment |
| --- | --- |
| Host | Windows 11 Pro, 15.73 GiB RAM, Intel i5-1135G7, 8 logical CPUs |
| Guest | WSL2 Ubuntu, x86-64 |
| Capacity detected by the tested kernel | 7,941 MiB |
| Swap | 16 GiB |
| AI tools | Claude Code 2.1.217 and Codex CLI 0.145.0 |
| Supervisor | Rust `0.2.0-alpha.1`, one-second sensor interval, user memory cap off |

A bounded allocator outside the AI process trees touched real memory at about 64 MiB/s, slowed to
about 32 MiB/s below 1 GiB available, stopped at 350 MiB, held for 20 seconds, and released all of
it. Because an external program created the pressure, the correct behavior was to brake new work
without blaming Claude Code or Codex.

| Point | Verified result |
| --- | --- |
| Start | 5,910 MiB available; existing work unrestricted |
| First brake | `HOLD` at 1,143 MiB available, 577.6 MiB reserve, 8.8 seconds to reserve |
| Next brake | `DRAIN` at 530 MiB available, 409.6 MiB reserve, 3.9 seconds to reserve |
| During `DRAIN` | new subagent start deferred; an in-progress edit allowed |
| Lowest point | about 350 MiB available; no terminal freeze or forced termination |
| Attribution | external pressure; no agent restricted and no PID paused |
| Recovery | 5,902 MiB available after release; new work reopened after the stability window |

## Scale verification

Deterministic Rust tests preserve the same time relationship from 512 MiB to 10 TiB capacities and
from 1 MiB/s to 128 GiB/s sustained decline. Twelve seconds to the reserve cannot enter `DRAIN`,
seven seconds enters `HOLD`, and four seconds enters `DRAIN`. Multi-agent tests also verify that each
control interval selects only the minimum required targets from the remaining stages and stops the
next restriction as soon as the trajectory improves.

## Scope limits

- The physical near-boundary test covers external pressure and automatic recovery on one
  Windows+WSL2 environment.
- Large agent fleets and extreme memory sizes are verified with deterministic simulations.
- Pausing a process stops additional growth but does not immediately return memory it already uses.
- Until public Windows binaries have trusted code signing, Windows security settings may block
  execution. See [Windows executable trust](../guides/windows-signing.md).
