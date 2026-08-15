# Installation, connection, and supported environments

<p align="center">
  <strong>English</strong> · <a href="setup.ko.md">한국어</a> · <a href="setup.zh-CN.md">简体中文</a> · <a href="setup.ja.md">日本語</a>
</p>

## Installation

Open the **terminal** for your environment and paste the matching one-line
command below. There is no Git, Python, Rust, or separate installer to prepare.
A normal installation is scoped to your user account and does not require
`sudo` or an administrator shell.

### 1. Install Memory Supervisor

#### Linux, WSL2, or macOS terminal

```bash
curl -fsSL https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.sh | sh
```

When the command finishes, the background service is running and the detected
Claude Code and Codex hooks are connected automatically. It does not close a
running AI program or interrupt work in progress.

#### Windows PowerShell terminal

```powershell
irm https://raw.githubusercontent.com/lssLab/Calando/main/bootstrap.ps1 | iex
```

When the command finishes, the background service is running and the detected
Claude Code and Codex hooks are connected automatically. It does not close a
running AI program or interrupt work in progress.

> [!IMPORTANT]
> The Windows executable is currently under review by [SignPath Foundation](https://signpath.org/),
> so Windows 11 requires **Windows Security → App & browser control → Smart App Control** to remain
> `Off` while installing and using the native build until the review is complete.

| Windows state | Can the native build be installed? |
| --- | --- |
| 64-bit Windows 10 | Yes. Smart App Control is not available, so no SAC setting is required. If SmartScreen appears, verify that the download came from this repository's release. |
| Windows 11 24H2 build 26100.8117 or later, 25H2 build 26200.8117 or later, or a newer Windows 11 build where the re-enable control is visible | Yes, after setting Smart App Control to `Off`. You can turn it back on from the same Settings page after you stop using the unsigned build. |
| An older Windows 11 build, or a current build that has not yet received the gradually rolled-out control | Yes after turning it off, but turning it back on may require a Windows reset or reinstall. Check this before disabling it. |
| Smart App Control is already `Off` | Install without changing it. If the separate SmartScreen download-reputation prompt appears, verify the publisher and file source. |
| Windows 11 in S mode or an organization App Control policy that blocks the executable | Not supported by the native Windows path. This installer cannot bypass S mode or an administrator policy. |

Run `winver` from `Win + R` to check the Windows version and build. Before turning Smart App
Control off, also confirm whether its Settings page offers a way to turn it back on; rollout is
per device. See Microsoft's
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)
and [initial rollout notes](https://support.microsoft.com/en-au/help/5079391) for the source criteria.

Install for the environment where the **App Server and its tools actually run**,
not the operating system where the Codex App window is displayed.

- When Codex App on Windows uses a WSL engine, the Supervisor installed in that
  WSL environment protects its WSL App Server, per-task logical threads, and
  WSL-side tools. This path does not run the native Windows Supervisor, so it
  does not require turning off Smart App Control. The Windows App UI process and
  any separate Windows-native Claude Code or Codex CLI remain outside that WSL
  installation's measurement and control boundary.
- When the App Server or CLI runs directly on Windows, macOS, or Linux, install
  in that operating system. A native Windows installation uses the Smart App
  Control requirements above.
- When the App Server or CLI runs in another WSL distribution, virtual machine,
  or isolated container, install in each such environment. Windows and WSL find
  their federation path automatically; macOS or Linux hosts, dynamic-memory
  VMs, and containers connect a shared folder on the same machine. Once
  connected, environments competing for the same physical memory share
  new-work decisions. Fixed-memory VMs and other computers or cloud servers
  protect themselves independently. See
  [platform and multi-environment behavior](platforms.md) for the boundary.

### 2. Set up Claude Code

The installer connects the Memory Supervisor user hook automatically. There
is nothing to configure, approve, or enable in Claude Code.

**If Claude Code was already running during installation:** keep working.
Claude Code automatically reloads user-settings changes, so a restart is not
normally required.

**To verify:** confirm `Claude Code CONNECTED` in step 5's
`memory-status --connections` output. Open the read-only `/hooks` screen and
inspect `User Settings` only if you want to see the hook details. Restart
Claude Code after the current work only in the exceptional case where this
optional view does not show the entry.

### 3. Set up Codex CLI

1. Open `/hooks` in the Codex CLI you will use.
2. Confirm that all seven Memory Supervisor hooks are **trusted and on**.
3. Trust entries marked for review and turn on any disabled entry.
4. Close `/hooks` and continue working.

**If Codex CLI was already running during installation:** continue in the CLI
you just checked; it does not need a restart. For any other Codex CLI that was
already open before installation, finish its current work and restart only
that CLI once.

### 4. Set up Codex Desktop App

1. Open Codex App and go to **Settings → Hooks**. If the Memory Supervisor
   entries are not there yet, wait up to 60 seconds and reopen Settings.
2. Trust and turn on all seven Memory Supervisor hooks. **Trust all** does not
   turn on a previously disabled switch, so check both states.
3. Return to an existing task and send the next request you intended to send.
   Create a new task only if there is no existing task to continue.

**If Codex App was already running during installation:** leave the App and its
existing tasks open and follow steps 1–3. You do not need to restart the App or
create a new task.

### 5. Verify the installation

```bash
memory-status --connections
```

Check the lines for the programs you use:

- `Core daemon CONNECTED`: the background service is healthy.
- `Claude Code CONNECTED`: the supported version and user hook are connected.
- `Codex CONNECTED`: all seven CLI hooks are installed, enabled, and trusted.
- `Codex App ACTIVE`: all seven App hooks are ready and a real call arrived
  from an existing or new task.
- `NOT DETECTED` is normal for a program you do not use or have not installed.

If a line is not healthy, act only on what it reports:

- `disabled` or `not trusted`: use `/hooks` in Codex CLI or **Settings →
  Hooks** in Codex App to trust and enable the named entry.
- `missing`, `stale`, `DEGRADED`, or `NOT RUNNING`: run
  `memory-supervisor update`, then repeat this check.
- `NEEDS ATTENTION`: satisfy the reported program-version or hook requirement,
  then run `memory-supervisor update`.
- `Core daemon OFF`: run `memory-supervisor on`.
- If all seven App hooks look correct but the App still does not become
  `ACTIVE` after a request, restart the App once, send the next request in an
  existing task, and check again.
- If a new installation cannot find the `memory-status` command, reopen only
  the terminal and run it again. Claude Code, Codex CLI, and Codex App do not
  need to restart for this PATH refresh.

Codex hook trust is not administrator access. It is your approval of the exact
local command that Codex will run. Check an administrator policy only if an
organization policy or Windows security policy blocks installation. See the
[Claude Code hooks guide](https://code.claude.com/docs/en/hooks) and
[Codex hooks guide](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)
for the underlying trust rules.

These commands install the latest public release. A Rust build tool is not
required; the verified executable included in that release is used automatically.

### 6. Uninstall

To remove Calando, run this once in each environment where it is installed:

```bash
memory-supervisor uninstall
```

It removes the background service, executable, and Calando-owned hook and skill connections while
preserving state and user settings.

## Supported environments

Protection behaves the same in every supported environment. The supervisor
watches available memory and its rate of decline, narrows new work in stages,
pauses one verified Claude Code or Codex process only if danger remains, and
resumes it after stable recovery. Only the operating-system mechanism used to
read memory and pause a process differs.

| Environment | Test coverage |
| --- | --- |
| Linux and WSL2 on 64-bit Intel/AMD | Physical WSL2 and automated Linux checks |
| macOS Apple Silicon | Automated Apple Silicon checks |
| Windows 10 or 11 on 64-bit Intel/AMD | Physical Windows 11 E2E, automated Windows Server 2022 checks, and Windows 10 runtime/API compatibility review |
| Intel-based macOS | Automated compatibility under Rosetta |

Connected products are Claude Code 2.1.217 or newer, Codex CLI 0.145.0 or newer
with `hooks stable true`, and Codex Desktop App. The same protection policy
applies to the CLI and App.

### Measured resident memory

These operating-system totals were measured in 20 samples at 0.2-second
intervals after warm-up.

| Test environment | Minimum | Mean | Maximum | OS metric |
| --- | ---: | ---: | ---: | --- |
| WSL2 Linux, physical service | 4.88 MiB | 4.88 MiB | 4.88 MiB | RSS |
| Ubuntu on 64-bit Intel/AMD, automated test | 3.50 MiB | 3.52 MiB | 3.54 MiB | RSS |
| Windows on 64-bit Intel/AMD, automated test | 4.15 MiB | 4.20 MiB | 4.25 MiB | Working set |
| macOS Apple Silicon, automated test | 3.38 MiB | 4.35 MiB | 5.13 MiB | RSS |

For capacity planning, use **10 MiB per installed monitor**, not the smallest
sample. See [performance measurements](performance.md) for the detailed
conditions and raw data.

When one physical computer has several execution environments—Windows, WSL
distributions, virtual machines, or isolated containers—install it in every
environment that runs Claude Code or Codex. Multiple terminals in one
environment share one monitor. After installation and federation-path setup in
each environment, the whole computer automatically shares the latest new-work
decision regardless of how many kernels it runs. Each monitor still measures
and controls only its own environment, so it never operates on another
environment's PID. The installer connects the same local shared folder for
Windows and WSL; a VM or container uses a host-shared local folder as its
federation path. Network folders are not used to connect a different physical
computer or cloud server. See
[platform and multi-environment behavior](platforms.md) for setup details.
