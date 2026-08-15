[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
$PointerDir = Join-Path $HOME ".memory-supervisor"
$RuntimeDir = if ($env:MEMORY_SUPERVISOR_RUNTIME_DIR) {
    $env:MEMORY_SUPERVISOR_RUNTIME_DIR
} else {
    Join-Path $HOME ".local\lib\memory-supervisor"
}
$OwnedBinary = Join-Path $RuntimeDir "memory-supervisor.exe"
$TaskName = if ($env:MEMORY_SUPERVISOR_TASK_NAME) {
    $env:MEMORY_SUPERVISOR_TASK_NAME
} else {
    "MemorySupervisor"
}
if ($TaskName -notmatch '^[A-Za-z0-9_.-]{1,100}$') {
    throw "MEMORY_SUPERVISOR_TASK_NAME must contain only letters, numbers, dot, underscore, or hyphen"
}
$Binary = $env:MEMORY_SUPERVISOR_BINARY
if (-not $Binary) {
    $Pointer = Join-Path $PointerDir "binary"
    if (Test-Path -LiteralPath $Pointer) { $Binary = ([IO.File]::ReadAllText($Pointer)).Trim() }
}
if (-not $Binary) { $Binary = $OwnedBinary }

function Test-OwnedTask {
    param($Task)
    if (-not $Task -or @($Task.Actions).Count -ne 1) { return $false }
    $Action = $Task.Actions[0]
    $Execute = [string]$Action.Execute
    $Arguments = [string]$Action.Arguments
    return (([IO.Path]::GetFullPath($Execute) -eq [IO.Path]::GetFullPath($OwnedBinary) -and
        $Arguments -match '(?i)(^|\s)daemon(\s|$)') -or
        $Arguments.IndexOf((Join-Path $Root "supervisor.py"), [StringComparison]::OrdinalIgnoreCase) -ge 0)
}

# Parse and remove owned hooks before changing the running task. A malformed
# provider file must leave the working installation intact for manual repair.
if (Test-Path -LiteralPath $Binary -PathType Leaf) {
    $ClaudeSettings = Join-Path $HOME ".claude\settings.json"
    $CodexHome = if ([string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        Join-Path $HOME ".codex"
    } else {
        $env:CODEX_HOME
    }
    $CodexHooks = Join-Path $CodexHome "hooks.json"
    $DefaultCodexHooks = Join-Path $HOME ".codex\hooks.json"
    if (Test-Path -LiteralPath $ClaudeSettings) {
        & $Binary integration hooks --target $ClaudeSettings --provider claude --binary $Binary --remove
        if ($LASTEXITCODE -ne 0) { throw "Claude hook removal failed" }
    }
    if (Test-Path -LiteralPath $CodexHooks) {
        & $Binary integration hooks --target $CodexHooks --provider codex --binary $Binary --remove
        if ($LASTEXITCODE -ne 0) { throw "Codex hook removal failed" }
    }
    if (-not $DefaultCodexHooks.Equals($CodexHooks, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $DefaultCodexHooks)) {
        & $Binary integration hooks --target $DefaultCodexHooks --provider codex --binary $Binary --remove
        if ($LASTEXITCODE -ne 0) { throw "default Codex hook removal failed" }
    }
} else {
    Write-Warning "Installed binary is missing; provider hook files were left unchanged"
}

$Task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if (Test-OwnedTask $Task) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
}
$Legacy = if ($TaskName -eq "MemorySupervisor") {
    Get-ScheduledTask -TaskName "ClaudeMemoryGovernor" -ErrorAction SilentlyContinue
} else { $null }
if ($Legacy -and @($Legacy.Actions).Count -eq 1 -and
    ([string]$Legacy.Actions[0].Arguments).IndexOf((Join-Path $Root "governor.py"), [StringComparison]::OrdinalIgnoreCase) -ge 0) {
    Stop-ScheduledTask -TaskName "ClaudeMemoryGovernor" -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName "ClaudeMemoryGovernor" -Confirm:$false
}

foreach ($Path in @(
    (Join-Path $HOME ".claude\skills\memory-supervisor"),
    (Join-Path $HOME ".agents\skills\memory-supervisor"),
    (Join-Path $HOME ".codex\skills\memory-supervisor"),
    (Join-Path $HOME ".claude\skills\memory-governor"),
    (Join-Path $HOME ".agents\skills\memory-governor"),
    (Join-Path $HOME ".codex\skills\memory-governor")
)) {
    $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($Item -and ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        $Target = @($Item.Target) | Select-Object -First 1
        if ($Target -and ([IO.Path]::GetFullPath($Target) -eq [IO.Path]::GetFullPath($Root))) {
            [IO.Directory]::Delete($Path)
        }
    }
}

function Remove-OwnedLauncher {
    param([string]$Path)
    $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $Item -or $Item.PSIsContainer -or
        ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return }
    if ([IO.File]::ReadAllText($Path).IndexOf(
        "Memory Supervisor managed command", [StringComparison]::OrdinalIgnoreCase
    ) -ge 0) { Remove-Item -Force -LiteralPath $Path }
}
$Bin = Join-Path $HOME ".local\bin"
Remove-OwnedLauncher (Join-Path $Bin "memory-supervisor.cmd")
Remove-OwnedLauncher (Join-Path $Bin "memory-status.cmd")
Remove-OwnedLauncher (Join-Path $Bin "memory-control.cmd")

function Remove-UnchangedCommandFile {
    param([string]$Path, [string]$Source)
    $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $Item -or $Item.PSIsContainer -or
        ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return }
    if ((Test-Path -LiteralPath $Source) -and
        [IO.File]::ReadAllText($Path) -eq [IO.File]::ReadAllText($Source)) {
        Remove-Item -Force -LiteralPath $Path
    }
}
Remove-UnchangedCommandFile (Join-Path $HOME ".claude\commands\memory-status.md") `
    (Join-Path $Root "commands\claude\memory-status.md")
Remove-UnchangedCommandFile (Join-Path $HOME ".codex\prompts\memory-status.md") `
    (Join-Path $Root "commands\codex\memory-status.md")

if ([IO.Path]::GetFullPath($Binary) -eq [IO.Path]::GetFullPath($OwnedBinary)) {
    Remove-Item -Force -LiteralPath $OwnedBinary -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath "$OwnedBinary.previous" -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath $RuntimeDir -ErrorAction SilentlyContinue
}
Remove-Item -Force -LiteralPath (Join-Path $PointerDir "binary") -ErrorAction SilentlyContinue
Remove-Item -Force -LiteralPath (Join-Path $PointerDir "install-root") -ErrorAction SilentlyContinue
Write-Host "removed; state, incidents, power choice, hard-cap settings, and notifications.conf were preserved"
