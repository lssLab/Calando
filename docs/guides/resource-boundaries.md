# Session discovery, capacity detection, and memory boundaries

<p align="center">
  <strong>English</strong> · <a href="resource-boundaries.ko.md">한국어</a>
</p>

This guide explains what one installation can see, how the supervisor discovers terminal sessions
without a wrapper, how it learns the usable memory of an OS or guest, and what each configurable
boundary actually changes.

## The short model

A physical computer is not always one control boundary. It can contain several independently
observable PID and memory domains:

```text
physical computer
├─ Windows host                -> Windows supervisor
├─ WSL distribution           -> Linux/WSL supervisor
├─ Linux or Windows VM        -> supervisor inside that guest
└─ Apple Silicon Mac host
   └─ macOS or Linux VM       -> supervisor inside that guest

fresh state snapshots (≤10 s) -> shared admission decision
local process table            -> only the owning instance may pause/resume that PID
```

Install once in every host, WSL distribution, VM guest, or PID-isolated container where Claude
Code or Codex runs. A host installation cannot signal a guest PID. Federation shares backpressure;
it does not create a cross-kernel process controller or add the RAM totals together.

## What each terminal surface belongs to

| Where the CLI was started | Actual observation boundary | Capacity source | Where to install and configure |
| --- | --- | --- | --- |
| PowerShell, Command Prompt, or native Windows Terminal tab | Windows host | `GlobalMemoryStatusEx` physical total/available; `GetPerformanceInfo` commit headroom | Once in Windows |
| A WSL terminal tab | That WSL distribution's visible Linux PID/memory domain | `/proc/meminfo`, narrowed by every enclosing cgroup limit; ultimately bounded by WSL VM memory | Once inside every WSL distribution that runs a supported CLI; also install on Windows for host protection |
| Bare Linux, SSH session, or tmux pane | That Linux kernel/PID namespace and user permissions | `/proc/meminfo`, narrowed by every enclosing cgroup v1/v2 limit | Once for each protected OS user/environment |
| Terminal or iTerm on an Apple Silicon Mac | macOS `arm64` host | `sysctl hw.memsize`; free, inactive, and purgeable pages from `vm_stat` | Once on macOS |
| macOS VM on an Apple Silicon Mac | The guest macOS `arm64` VM | Guest `hw.memsize` and `vm_stat`; bounded by the VM allocation | Once inside the guest; host installation remains separate |
| Linux or Windows VM on any host | The guest OS | The same native Linux or Windows sources above, bounded by the hypervisor allocation | Once inside the guest; host installation remains separate |
| PID-isolated container | That container's visible process and cgroup domain | Physical memory narrowed by all enclosing cgroup limits | Once inside the isolated container, or intentionally share the host PID namespace |
| Intel-based Mac | macOS `x86_64` host | The same macOS sources | Once on that Mac |

An Apple Silicon macOS VM remains `arm64`. Running x86_64 under Rosetta is compatibility coverage
and is kept distinct from physical Intel Mac verification.

WSL2 distributions can share the same underlying utility VM while retaining separate process
namespaces. Install in each distribution that runs a CLI because one distribution cannot reliably
inventory or signal another distribution's PIDs. Federation takes the worst fresh decision; it
does not sum duplicate views of the shared WSL memory pool.

## How sessions are discovered without a wrapper

The user still starts `claude` or `codex` normally. The daemon does not enumerate terminal windows
or require `claude-governed`/`codex-governed` launch commands.

1. The native daemon scans the complete process inventory visible to its OS account. The normal
   control loop is one second; Windows refreshes the more expensive CIM inventory at most once every
   three seconds while reading cheap global memory counters every tick.
2. A process is a supported-CLI root when its executable or first command arguments resolve to
   `claude`, `codex`, or the official architecture-specific Codex binary.
3. Parent links group nested supported-CLI roots as workers. Other descendants become support
   processes. An ancestry walk is bounded to 64 levels so a malformed process graph cannot loop
   forever.
4. Every descendant contributes to its root tree's RSS estimate. Small descendants below 32 MiB of
   anonymous memory are omitted only as individual pause candidates; they are not removed from the
   tree total.
5. PID plus process start identity prevents PID reuse from targeting a different process. Linux
   uses `/proc/<pid>/stat` start ticks, macOS uses `ps` start time, and Windows uses CIM
   `CreationDate`.
6. The daemon records lead/worker/support role, memory growth, root-tree total, and a verified
   terminal identity when the platform exposes one. OS permissions, Linux `hidepid`, containers,
   and VM boundaries are respected rather than bypassed.

AI CLI hooks are a second path, not the process detector. They ask the latest local/federated
state before new fan-out and inject an incident into the main agent at the next real hook boundary.
If a hook is missing, the daemon can still observe its local process table, but it cannot prevent
that AI CLI's allocation before the new process starts. Check both paths with:

```bash
memory-status --connections
memory-status --all
```

## How usable capacity is learned

| Platform | Capacity | Available/headroom | Additional distress evidence |
| --- | --- | --- | --- |
| Linux and WSL | `MemTotal`, reduced to the smallest finite cgroup ancestor limit | Minimum of `MemAvailable` and each finite `limit - current` cgroup remainder | PSI `some/full`, reclaim, swap, and OOM counters |
| macOS | `sysctl -n hw.memsize` | `vm_stat` free + inactive + purgeable pages | Kernel pressure level when exposed, pageout/compression and swap trends |
| Windows | `GlobalMemoryStatusEx.totalPhys` | `GlobalMemoryStatusEx.availPhys` | Commit limit minus committed pages from `GetPerformanceInfo` |
| Any VM guest | The relevant row above as reported inside the guest | The guest-visible value, therefore already bounded by fixed or dynamic VM allocation | Guest-native pressure signals |

The resolved capacity and adaptive policy are recalculated on every tick. A VM dynamic-memory
change or a cgroup change is therefore picked up without a fixed machine-size profile. If a primary
sensor fails, status reports degraded protection and admission is held; an 8 GiB fallback label is
diagnostic, not a claim that the machine really has 8 GiB.

The supervisor only **reads** enclosing cgroup limits that a container runtime, systemd unit,
scheduler, or administrator already created. It does not create a cgroup, move a CLI into one, or
require a wrapper command. That is why a normal `claude` or `codex` launch is still discovered, while
a byte-exact cgroup allocation remains an optional external boundary rather than this product's
default actuator.

The supervisor does **not** allocate RAM to a process. It calculates a stopping distance rather
than reserving a fixed percentage:

```text
minimum breathing room = 0.5% of detected capacity, clamped to 256–1024 MiB
corroborated burn rate = max(sustained physical/commit headroom fall,
                             sustained tracked-CLI growth)
automatic reserve     = min(minimum breathing room
                            + corroborated burn rate × one reaction interval,
                            25% of detected capacity)
new-fan-out floor     = min(automatic reserve + one minimum breathing/work block,
                            30% of detected capacity)
```

Physical headroom already contains tracked CLI allocations, so the two rates are deliberately
combined with `max`, not added and counted twice. A trajectory needs at least three samples, one
reaction interval of span, at least 60% supporting intervals, and at least twice as much movement
in the dangerous direction as rebound. One reclaim spike therefore cannot erase a real descent,
and one burst cannot create one.

This is the same geometry as braking a vehicle: faster depletion creates more distance in MiB but
does not move the intervention earlier in time; slower depletion can use more of the machine before
the same reaction window is reached. `HOLD` closes only new fan-out when the reserve is within two
reaction intervals or there is no room for one new minimum block. `DRAIN` begins graded existing-
agent cushioning only within one reaction interval, and only with agent/mixed attribution or an
explicit hard cap. The number of logical steps applied each one-second tick is
`ceil(remaining steps / control ticks left)`, so eight workers and hundreds of sessions finish the
same minimum ladder at the boundary without a fixed agent-count cap.

Stable high usage can remain open. Raw GREEN/YELLOW/ORANGE/RED utilization is diagnostic and does
not by itself close admission or authorize a PID pause. The measured small-to-huge-machine and
near-suffocation evidence is in
[Adaptive stopping distance](../testing/stopping-distance.md).

## The five different boundaries

| Boundary | Default | How to change it | Direct scope | Important side effects |
| --- | --- | --- | --- | --- |
| Physical or VM allocation | OS/hypervisor default | Physical RAM cannot be changed in software; change WSL, Hyper-V, Parallels, VMware, UTM, or cloud VM memory in that platform | The host or guest OS itself | Raising guest memory gives it more possible headroom but reduces the host's worst-case reserve. Lowering it makes the guest's adaptive thresholds and reserve recalculate downward. Usually requires guest shutdown/restart. |
| Auto-detected capacity | Native sensor | Do nothing normally. `MEMORY_SUPERVISOR_CAPACITY_MB` is an advanced calibration override only when the native value is demonstrably wrong | One installed instance | It changes policy calculations but does not change the real OS/VM limit. Setting it too high is unsafe; too low is unnecessarily conservative. |
| Adaptive pressure policy | `balanced`; no manual budget | Optional `protect`, `balanced`, or `performance` profile, or advanced threshold overrides | One installed instance | Federation can propagate that instance's stricter admission decision to peers. `performance` never bypasses actual collapse, degraded protection, or an explicit cap. |
| Aggregate supported-CLI memory budget (hard cap) | **OFF** | `memory-supervisor budget set <GiB>` or `budget off` in that environment | All Claude Code and Codex root trees visible to that OS/PID domain; not Chrome or the entire machine | Near the cap, new fan-out is held. Above it, at most one verified growing worker/support process can pause per reaction interval. Cap proximity stays local: a `near/exceeded` state no longer closes admission on federated peers (only measured pressure federates), and it cannot pause their PIDs. |
| Federation admission | Enabled when instances share a directory | Configure a shared `MEMORY_SUPERVISOR_FEDERATION_DIR`; WSL distribution names are automatic, while other cloned guests need unique `MEMORY_SUPERVISOR_INSTANCE` values | New fan-out only across fresh peers | Uses the worst valid snapshot from the last ten seconds. It never pools hard caps, adds RAM totals, migrates jobs, or changes a remote configuration. |

## Changing a supported-CLI memory budget

Run these commands **inside each environment** whose process trees should change:

```bash
memory-supervisor budget
memory-supervisor budget set 12
memory-supervisor budget off
```

`12` is only a GiB syntax example, not a recommended size (`memory-supervisor hard-cap set <MB>`
remains the MB-precision alias). The bare `budget` report shows this environment's theoretical
maximum and the currently possible total after peer environments' explicit budgets, using the
shared federation snapshots; only explicit budgets count as claims, never an environment's default
allocation. `set` validates against that currently possible total — an oversized request is refused
with the exact per-environment reductions that would make it fit, and a request at 90% or more of
the currently possible total — or one that pushes the machine-wide explicit-budget total to 90% or
more of the physical estimate — asks for confirmation
(`--yes` for scripts). `set` preserves unrelated configuration and reloads that local service;
`off` returns the environment to adaptive-only mode.

Examples:

| Desired result | Action |
| --- | --- |
| One shared budget for native Windows Claude Code and Codex | Run `budget set <GiB>` once in PowerShell |
| A different budget for WSL sessions | Run a separate value inside that WSL distribution |
| Same policy in host and guest VM | Run the same command once on the host and once inside the guest |
| Different budgets for two VMs | Run a different value inside each VM |
| Default smart behavior everywhere | Run `budget off` in every environment that previously had an override |

The cap counts each complete supported-CLI root tree once. It is sampled, can overshoot between ticks,
and suspension does not return already resident memory immediately. Use a native cgroup, container,
or VM limit for a byte-exact allocation ceiling.

## Changing WSL or VM allocation

For WSL2, the host-side `%UserProfile%\.wslconfig` sets the maximum shared WSL VM memory. Example:

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

This is a maximum, not preallocation. It applies only after the WSL VM fully stops. Never run
`wsl --shutdown` while CLI sessions are active because it terminates them; use an idle boundary.
See Microsoft's [WSL configuration](https://learn.microsoft.com/windows/wsl/wsl-config) and
[`wsl --shutdown`](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown) documentation.

For Hyper-V, Parallels, VMware, UTM, and cloud VMs, change fixed/dynamic memory in that hypervisor or
cloud control plane, normally while the guest is stopped. The supervisor needs no matching number:
after boot it reads what the guest kernel actually exposes and recalculates. A host and guest still
need separate installations and, for shared admission, a shared federation folder.

## Advanced policy changes

Normal users should leave these unset. Advanced settings live at
`~/.config/memory-supervisor/config.json` on Unix and
`$HOME\.config\memory-supervisor\config.json` on Windows:

```json
{
  "MEMORY_SUPERVISOR_POLICY_PROFILE": "performance"
}
```

After a manual edit, run `memory-supervisor update` and inspect `memory-status`. `protect` acts earlier,
`performance` acts later, and `balanced` is the default. Fine-grained
`MEMORY_SUPERVISOR_MEM_*`, `MEMORY_SUPERVISOR_PSI_*`, and process-observation overrides are available
for measured compatibility problems, but their ordering is validated and invalid groups fall back
to adaptive values. A slope or raw threshold remains an observation; the shared actuator
invariants still control pause authority.


## Verification boundary

The repository runs one shared test suite on Linux, Windows, and macOS through GitHub Actions. It covers
native sensors, process identity, policy decisions, hook behavior, installation lifecycle, and
release artifacts. A controlled Windows and WSL2 workload also verifies stopping distance near the
recovery boundary. See the [test matrix](../testing/test-matrix.md) and
[stopping-distance validation](../testing/stopping-distance.md).

Hosted runners and deterministic simulations verify repeatable product contracts. They do not
claim to reproduce every physical host, guest, container, or long-running workload combination.

## What is deliberately not possible

- One Windows command cannot set a WSL, macOS VM, or Linux VM hard cap.
- A WSL instance cannot pause a Windows PID, and a guest cannot pause a host PID.
- Federation cannot combine 16 GiB host RAM and 10 GiB WSL capacity into a fictional 26 GiB pool.
- A supervisor does not see a powered-off guest or a CLI outside its visible PID/permission domain.
- A macOS VM on Apple Silicon is not an Intel Mac test. Rosetta is compatibility coverage only.
- Changing `MEMORY_SUPERVISOR_CAPACITY_MB` does not allocate or reclaim physical memory.

See [platform deployment and federation](platforms.md) for installation paths and
[performance](performance.md) for the measured per-instance footprint.
