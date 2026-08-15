# Windows executable trust

<p align="center">
  <strong>English</strong> · <a href="windows-signing.ko.md">한국어</a>
</p>

The Windows executable is currently under open-source code-signing review by
[SignPath Foundation](https://signpath.org/), so Smart App Control must remain off on Windows 11
until the review is complete. The current preparation build does not carry an Authenticode
signature. The installer verifies the SHA-256 checksum
published with the release, but integrity verification does not replace the publisher signature
expected by Windows.

## When this applies

- PowerShell, Windows Terminal, and a Windows-native Codex App Server use the native Windows path and
  are subject to Smart App Control.
- If the Codex App window is on Windows while its App Server and tools run inside WSL, install the WSL
  Supervisor. No Windows executable is launched, so this guidance does not apply.
- Organization App Control, Windows 11 S mode, and the separate SmartScreen download-reputation check
  can impose other restrictions. The installer does not bypass them.

## Current installation condition

Smart App Control has no per-application exception. To use the unsigned native Windows build, check
**Windows Security → App & browser control → Smart App Control**.

| Windows state | Result |
| --- | --- |
| 64-bit Windows 10 | Smart App Control is not available, so no SAC setting is required. If SmartScreen appears, verify that the download came from this repository's release. |
| Smart App Control is already `Off` | Proceed with the native installation. If a separate SmartScreen prompt appears, verify that the download came from this repository's release. |
| A current Windows 11 build shows the re-enable control | Set it to `Off` while using the unsigned build; it can be enabled again from the same screen afterward. |
| Windows 11 does not show the re-enable control | Turning it off may require a Windows reset or reinstall to turn it on again, so confirm this first. |
| Windows 11 in S mode or a blocking organization policy | The native Windows path is unsupported. The installer cannot bypass the restriction; use a separately permitted environment such as WSL if appropriate. |

Run `winver` from `Win + R` to inspect the Windows version and build. The re-enable control is being
rolled out on Windows 11 24H2 build 26100.8117 or later and 25H2 build 26200.8117 or later, so verify
that the control is actually visible before turning Smart App Control off. See Microsoft's
[Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)
and [rollout notes](https://support.microsoft.com/en-au/help/5079391) for current criteria.

## Verifying a download

The one-line installer downloads the release source and executable, then verifies their published
SHA-256 values automatically. For a manually downloaded executable, inspect its signature state in
PowerShell:

```powershell
Get-AuthenticodeSignature .\memory-supervisor.exe | Format-List Status, StatusMessage, SignerCertificate
```

`NotSigned` is the expected result for this preparation build. When release artifacts become code
signed, the installation guide and release notes will state that change and this condition will be
updated with them.
