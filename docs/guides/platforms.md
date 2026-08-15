# Platform deployment and federation

<p align="center">
  <strong>English</strong> · <a href="platforms.ko.md">한국어</a>
</p>

## One supervisor per protected user and PID-control environment

The supervisor reads the process inventory visible to its OS user and PID namespace. One
installation covers that user's Claude Code and Codex sessions in the same PID-control environment,
regardless of whether they were opened from Windows Terminal, iTerm, VS Code, tmux, SSH, or another
terminal surface.

The supervisor never signals outside its local PID-control environment. Install once in the host
and once in every WSL distribution, VM, or PID-isolated container that should be protected. WSL 2
distributions can share the managed VM and kernel while retaining separate PID namespaces, so they
still need separate local instances. Each instance publishes a small state snapshot to a shared
federation directory. Hooks use the worst valid snapshot from the last ten seconds for new fan-out
admission, while only the local supervisor may pause a local PID.

| Base OS | Environment above it | Required installation | Federation boundary |
| --- | --- | --- | --- |
| Windows | One or more WSL2 distributions | Windows plus every WSL distribution | Each WSL instance auto-detects the Windows user's `.memory-supervisor/instances` |
| Windows, macOS, or Linux | Dynamic-memory VM | Host plus every guest | Connect only the sides that actually compete for the same physical RAM through a host-local shared folder |
| Windows, macOS, or Linux | Fixed-memory VM | Host plus every guest | Keep each side independent; do not federate across the fixed allocation boundary |
| Linux kernel (native Linux, WSL, or a desktop VM) | One or more PID-isolated containers | Kernel host environment plus every isolated container | Share a host-local volume inside that kernel |
| Any nested combination | Every protected PID namespace | One connection per dynamic shared-memory boundary | Do not stretch one directory across a fixed-VM boundary or a network |

### Codex App follows the App Server environment

The Codex App window and its execution engine do not have to run in the same operating-system
environment. Memory Supervisor follows the `codex ... app-server` process, not the desktop window:

- A Windows Codex App using a WSL engine is protected by the Supervisor installed in that WSL
  distribution. It detects the WSL App Server, resolves that process's active `CODEX_HOME`, and
  manages its logical threads, hook decisions, WSL child tools, and WSL-side physical brakes. This
  does not require the unsigned native Windows Supervisor or a Smart App Control change.
- That WSL instance cannot measure or pause the Windows App UI process or a separate Windows-native
  Claude Code or Codex CLI. Install the Windows Supervisor as well when those Windows processes must
  be covered; the Windows and WSL instances then share admission through federation while retaining
  local PID control.
- A native Windows or macOS App Server uses the Supervisor in that OS. An App Server actually
  running in Linux, another WSL distribution, a VM, or a PID-isolated container uses the Supervisor
  inside that environment. The same rule applies even if the window or client that requested it is
  elsewhere.
- A fixed-memory VM or remote computer protects itself independently. Federate only execution
  environments that dynamically compete for the same physical RAM.

This is a general process-boundary rule rather than a hard-coded Windows/WSL exception. A shared
Windows/WSL `CODEX_HOME` is handled only as a file-layout case: the hook file retains both native
command fields, but each command still reaches only the Supervisor and PIDs in its own environment.

Without a shared path, each instance still protects its own local environment. Only
cross-environment admission and the combined `memory-status --all` view are unavailable.

The federation reader does not whitelist a Windows/WSL pair. Windows, WSL, Linux, and macOS peers
inside one host-local memory boundary use the same snapshot contract, and only the strictest valid
new-work decision from the last ten seconds is applied. Windows/WSL is special only because the
shared path can be discovered automatically. macOS and Linux hosts, dynamic VMs, containers, and
nested environments use the shared folder for their actual boundary. Give cloned guests or
containers with the same hostname unique `MEMORY_SUPERVISOR_INSTANCE` values.

Share the federation directory, not `CODEX_HOME`. Hook files, trust state, and PID authority stay
local to each environment. Only when a Windows App and a WSL runtime genuinely use the same
`CODEX_HOME` does one Codex file retain both Windows and POSIX command fields. That is a hook-file
layout exception, not a limit on federation OS combinations.

## Runtime and startup

A public release installation does not require or install Git, Python, or Rust. It downloads the
source bundle and native binary for the current OS and architecture from the same release and
checks both SHA-256 values. It uses the downloader in the pasted command plus the operating
system's standard archive and SHA-256 support. A manual development checkout can be built locally
with Rust 1.88 or newer.

Windows 10 has no Smart App Control. The Supervisor executable's
[minimum Windows baseline](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html) is
also Windows 10, and the required memory and process facilities are available there. It needs no SAC
setting, although a SmartScreen prompt still requires checking the download source. Windows 11 Smart
App Control can block a new unsigned executable even when its
checksum is correct, and it provides no per-app exception. Until the public Windows artifact is
signed, the native Windows 11 path therefore requires Smart App Control to remain Off while the
executable is installed and running. The Windows installer executes the candidate before cutover and
leaves an existing service untouched if Windows refuses it. Windows 11 24H2 builds starting at
26100.8117 and 25H2 builds starting at 26200.8117 can receive a reversible On/Off control, but rollout
is gradual: check `winver` and confirm that the re-enable control is visible before turning SAC off.
Older builds or devices without that control may require a reset or reinstall to turn it back on.
WSL binaries do not require changing Windows Smart App Control, but they protect only processes
inside WSL. Windows 11 in S mode and organization App Control policies that still block the
executable are not supported native paths. See the [Windows signing runbook](windows-signing.md), Microsoft's
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions),
[rollout notes](https://support.microsoft.com/en-au/help/5079391), and
[code-signing guidance](https://learn.microsoft.com/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control).

| Platform | User-level startup mechanism |
| --- | --- |
| Linux / WSL | `~/.config/systemd/user/memory-supervisor.service`; installer-owned linger when available |
| macOS | `~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist` |
| Windows | `MemorySupervisor` Scheduled Task at user logon |
| Unix without user systemd | PID-supervised fallback; starts immediately, but boot startup is manual |

`memory-supervisor update` updates the checkout when possible, verifies and activates the native
runtime, reloads the local service, and reconnects every detected supported CLI. It never signals an
agent PID during the daemon cutover, and the new daemon reloads paused identities from `runtime.json`.

The safest update time is between active CLI sessions. A live update normally preserves them, but
there can be a brief fail-open protection gap. Always run `memory-status --connections` after an
update. Only an actual Codex hook-definition change requires the user to trust again personally in
that CLI process's `/hooks` or Desktop App **Settings → Hooks**; restarting never substitutes for
trust. An App Settings change refreshes existing tasks loaded by the shared App Server, but not
separate CLI processes. Claude Code has no per-hook hash approval: its user-settings hook is active
without a Codex-like item review, but interactive Claude holds all settings-file hooks until the user
accepts workspace trust for the current folder or one of its parents. A trusted running session
normally reloads later user-settings changes automatically. Those trust and reload boundaries can
last longer than the daemon restart itself.

The supported runtime is the native Rust binary. Restarting the supervisor reloads its persisted
state, while replacing an installed build should be done between active CLI sessions when possible.

### What happens after a machine restart

- Linux and WSL use the enabled user unit. Installer-owned linger lets the unit start with the user
  manager; WSL services begin only after that WSL distribution itself starts.
- macOS loads the `RunAtLoad` and `KeepAlive` LaunchAgent at GUI login.
- Windows starts the Scheduled Task at user logon and retries an unexpected daemon exit up to five
  times at one-minute intervals. The task requests console detachment, which the daemon performs
  only when that console belongs to the daemon process alone. Background starts therefore leave no
  user-facing black window open, and its periodic PowerShell sensors use `CREATE_NO_WINDOW`;
  commands run from an existing terminal keep that shared terminal.
- Claude Code and Codex hook/skill files remain installed. Open a new AI CLI session after login
  and run `memory-status --connections`. Only an actual hook-hash change can require review: use
  `/hooks` in Codex CLI and **Settings → Hooks** in Codex App.
- A reboot is not an update. Do not run `memory-supervisor update` unless source, setup, or a later
  installed CLI needs to be reapplied.

## Federation paths

- Default: `~/.memory-supervisor/instances`
- Override: `MEMORY_SUPERVISOR_FEDERATION_DIR`
- Persisted pointer: `~/.memory-supervisor/federation-dir`
- State pointer: `~/.memory-supervisor/state-dir`
- WSL automatically looks for the Windows user's shared instance directory.
- WSL default instance names include `WSL_DISTRO_NAME`, so Ubuntu and Debian on the same host do not
  overwrite one another even though WSL shares the Windows hostname.
- Stale, malformed, or errored snapshots never participate in admission.
- If non-WSL cloned guests still share an identity, set `MEMORY_SUPERVISOR_INSTANCE` to a unique
  name.

```bash
memory-status --all
```

Federation is global backpressure, not a scheduler. It neither migrates workers nor sends a signal
to a PID owned by another OS.

## Multiple SSH/tmux sessions and VPS deployments

One user-level installation covers that user's Claude Code and Codex sessions across SSH logins,
terminal windows, and `tmux` panes in the same PID-control environment. They share one admission
decision rather than competing through independent supervisors. A multi-user server should install
once per protected OS user; `/proc` restrictions such as `hidepid` can prevent one user from
inspecting another user's processes, and the product does not bypass that boundary.

A constrained VPS is a natural deployment shape because native cgroup ceilings, PSI, swap/reclaim,
and all same-user remote sessions feed the same policy. Enable the installed user service and, when
appropriate, user linger so it remains available without an open SSH shell. Desktop OS notifications
are often unavailable on a headless server, so use the mandatory hook/terminal action messages and
optionally Discord or Telegram. This path is covered by Linux and cgroup contract tests, but has not
yet been claimed as a completed hours-long real-VPS model soak.

## Native capacity and sensors

| Platform | Capacity and available memory | Pressure and processes |
| --- | --- | --- |
| Linux / WSL | `/proc/meminfo` limited by every enclosing cgroup v1/v2 ceiling | OS low-memory signals (PSI, reclaim, swap, and out-of-memory counters), `/proc/<pid>`, PID start ticks, TTY identity |
| macOS | `sysctl hw.memsize`; free/inactive/purgeable pages from `vm_stat` | kernel pressure level when exposed, primary `vm_stat` pageout/compression trends, `ps` start time and TTY |
| Windows | `GlobalMemoryStatusEx` physical memory | `GetPerformanceInfo` commit headroom, cached CIM process inventory, creation identity, console/ConPTY evidence |

Linux checks every cgroup ancestor rather than trusting an unlimited leaf. If the macOS
pressure-level sysctl cannot be read, `vm_stat` counters remain available but native pressure is
reported as unknown/low-confidence, the pressure sensor error is exposed, and admission
conservatively holds. A failed `vm_stat` is also a real sensor failure. macOS uses RSS as the
per-process approximation because anonymous RSS is not exposed in the same form. Windows refreshes
cheap global counters every tick and caches the expensive process inventory for three seconds.

Every platform reports `sensor_ok`, `sensor_errors`, and `last_process_scan_ts`. A failed process
scan may leave the last inventory visible for diagnosis, but that stale inventory cannot cause a new
leak pause or paused-PID reconciliation.

Adaptive admission uses actual headroom, short/long slope, time to exhaustion, native distress,
recent bursts, and an automatic recoverability reserve. It does not reserve a fixed percentage of
RAM. Stable high use can remain open; a rapid decline with ample headroom is observed before it is
held, and Hold is reserved for a near reserve, sustained short TTE, an explicit hard cap, or degraded
protection.

## WSL2 capacity on a 16 GiB Windows host

Microsoft currently documents WSL2's default `memory` ceiling as 50% of Windows RAM. On a 16 GiB
host, deleting an explicit `memory=8GB` line therefore normally leaves the same 8 GiB ceiling rather
than giving heavy Linux CLI sessions more room. `memory=10GB` is an example for several heavy WSL
tasks alongside Windows applications; `memory=12GB` is a larger example to consider only when the
Windows-side workload is light. Neither is a supervisor default or automatic recommendation.

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

`memory` is a maximum, not a 10 GiB preallocation. The supervisor still needs a VM ceiling because
an exact-PID pause stops further execution but does not immediately return resident memory, and it
does not control unrelated Linux or Windows applications. A higher WSL ceiling increases agent
headroom but reduces the host's worst-case reserve for external apps; federation observes both sides
but cannot turn one kernel's PID signal into the other kernel's memory reclamation.

Changes to `.wslconfig` require the WSL VM to stop before they take effect. Run `wsl --shutdown` only
at an idle boundary because it immediately terminates every running WSL distribution and every CLI
session inside it. See Microsoft's [advanced WSL settings](https://learn.microsoft.com/windows/wsl/wsl-config)
and [`wsl --shutdown` command](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown).

## Optional local CLI memory budget

The budget is **off by default**. It is one aggregate ceiling for all Claude Code and Codex trees
visible to this installed control environment, not a per-CLI limit or a pooled Windows+WSL quota.

```bash
memory-supervisor budget
memory-supervisor budget set 6
memory-supervisor budget off
```

`6` is only a GiB syntax example (`memory-supervisor hard-cap set <MB>` is the MB-precision alias).
Run the command separately in Windows, WSL, each VM, or each isolated container. Because those
control environments can share one physical machine, `memory-supervisor budget` first reports the
theoretical maximum for this environment and the currently possible total after peer environments'
explicit budgets;
`budget set` refuses a request that no longer fits (naming how much to reduce where) and asks for
confirmation at 90% or more of the currently possible total, or when the machine-wide
explicit-budget total would reach 90% of the physical estimate. An environment's default allocation,
such as an unconfigured WSL VM ceiling, is never counted as a claim.
Near the ceiling, new fan-out is held first. Above it, at most one verified growing worker/support PID
can be paused per reaction interval; a lead remains the last resort and requires exact recovery
visibility. Suspension stops further execution but does not immediately return resident memory, so
use cgroup/container/VM limits when a byte-exact quota is required.

## Persistent advanced settings

Normal operation needs no configuration file. Advanced overrides live in
`~/.config/memory-supervisor/config.json`; environment variables with the same names take precedence.
The budget commands above are the preferred way to set or clear a cap.

```json
{
  "MEMORY_SUPERVISOR_TICK_S": 1,
  "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S": 3,
  "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB": 32768
}
```

`MEMORY_SUPERVISOR_TICK_S` accepts 0.25 through 5 seconds. The five-second ceiling keeps the next
sample inside the ten-second state-freshness and five-second lease contracts. An out-of-range value
falls back to one second and appears in `configuration_error`.

Path/bootstrap settings such as `MEMORY_SUPERVISOR_DIR`,
`MEMORY_SUPERVISOR_FEDERATION_DIR`, and `MEMORY_SUPERVISOR_FORCE_PLATFORM` do not belong in this
JSON file. After a manual advanced edit, run `memory-supervisor update` and verify with `memory-status`.

## Pause, resume, and restart

- `SIGSTOP` on Unix and native process suspension on Windows preserve the PID and in-memory session.
- `memory-supervisor resume <pid>` revalidates PID plus start identity before continuing it.
- `memory-supervisor resume` is accepted only when exactly one managed PID is paused.
- Control intent is persisted before the signal and reported complete only after daemon acknowledgement.
- Restarting the supervisor reloads its incident ledger and does not automatically resume agents.
- Restarting the agent CLI is different: use that AI CLI's transcript/session resume feature.
- A remote incident must be controlled from the OS named by its `source` field.

AI CLI/model context is delivered at the next real hook boundary, which can be later than the
operating-system resume. Exact terminal, OS, Discord, and Telegram action notices are attempted
independently.


## Turn the whole installation on or off

```bash
memory-supervisor off
memory-supervisor on
```

One `off` command disables the service and automatic startup for the current OS/PID-control
environment and persists that choice at `~/.memory-supervisor/power-off`. Installed Claude Code and
Codex hooks and skills remain connected, but every hook passes through silently. `memory-status`
and `--connections` report intentional `OFF`, and `memory-supervisor update` preserves it. `on`
removes the marker, restores automatic startup, and verifies that a fresh state was published.

`off` refuses while the supervisor owns a paused PID or a process-control action is pending, so it
cannot leave a process stranded without a daemon to resume it. Windows, each WSL distribution, VM
guest, and PID-isolated container have separate services and PID namespaces; run the command once
inside each environment that you want to toggle.

## Low-level service recovery commands

```bash
# Linux / WSL
systemctl --user restart memory-supervisor.service
systemctl --user is-active memory-supervisor.service

# macOS: restart a loaded agent
launchctl kickstart -k gui/$(id -u)/io.github.lsslab.memory-supervisor

# macOS: explicitly unload, then load again
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist

# Windows
schtasks /End /TN MemorySupervisor
schtasks /Run /TN MemorySupervisor
```

Use these commands to repair an unexpected service failure, not as the product power switch. If the
service is unavailable without an `off` marker, hooks fail open rather than disabling the CLI and
`memory-status` reports the stale or missing supervisor and resulting protection gap.
