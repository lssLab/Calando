[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("on", "off")]
    [string]$Action
)

$ErrorActionPreference = "Stop"
$PointerDir = Join-Path $HOME ".memory-supervisor"
$Marker = Join-Path $PointerDir "power-off"
$Binary = if ($env:MEMORY_SUPERVISOR_BINARY) {
    $env:MEMORY_SUPERVISOR_BINARY
} elseif (Test-Path -LiteralPath (Join-Path $PointerDir "binary") -PathType Leaf) {
    ([IO.File]::ReadAllText((Join-Path $PointerDir "binary"))).Trim()
} else { $null }
if (-not $Binary -or -not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
    throw "Installed Memory Supervisor binary is missing; run memory-supervisor update"
}
$StateDir = if ($env:MEMORY_SUPERVISOR_DIR) {
    $env:MEMORY_SUPERVISOR_DIR
} elseif (Test-Path -LiteralPath (Join-Path $PointerDir "state-dir") -PathType Leaf) {
    ([IO.File]::ReadAllText((Join-Path $PointerDir "state-dir"))).Trim()
} else { Join-Path $HOME ".cache\memory-supervisor" }
$TaskName = if ($env:MEMORY_SUPERVISOR_TASK_NAME) {
    $env:MEMORY_SUPERVISOR_TASK_NAME
} else { "MemorySupervisor" }
if ($TaskName -notmatch '^[A-Za-z0-9_.-]{1,100}$') {
    throw "MEMORY_SUPERVISOR_TASK_NAME must contain only letters, numbers, dot, underscore, or hyphen"
}
$Task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if (-not $Task -or @($Task.Actions).Count -ne 1 -or
    -not [string]::Equals(
        [IO.Path]::GetFullPath([string]$Task.Actions[0].Execute),
        [IO.Path]::GetFullPath($Binary),
        [StringComparison]::OrdinalIgnoreCase
    ) -or [string]$Task.Actions[0].Arguments -ne "daemon --foreground --detach-console") {
    throw "Owned Memory Supervisor scheduled task is missing; run memory-supervisor update"
}
$Utf8 = [Text.UTF8Encoding]::new($false)

function Write-OffMarker {
    New-Item -ItemType Directory -Force -Path $PointerDir | Out-Null
    $Temporary = $Marker + ".tmp." + $PID
    [IO.File]::WriteAllText($Temporary, "off`n", $Utf8)
    Move-Item -Force -LiteralPath $Temporary -Destination $Marker
}

function Remove-StateFile {
    param([Parameter(Mandatory = $true)][string]$Path)
    # File.Delete is deliberately idempotent when the file is already absent. PowerShell's
    # Remove-Item can still surface a terminating PSArgumentException for a missing LiteralPath
    # under ErrorActionPreference=Stop on some Windows hosts.
    [IO.File]::Delete($Path)
}

function Stop-OwnedTask {
    Disable-ScheduledTask -TaskName $TaskName | Out-Null
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    for ($Attempt = 0; $Attempt -lt 50; $Attempt++) {
        $Current = Get-ScheduledTask -TaskName $TaskName
        if ([string]$Current.State -ne "Running") { return }
        Start-Sleep -Milliseconds 100
    }
    throw "Owned supervisor did not stop"
}

function Start-OwnedTask {
    Enable-ScheduledTask -TaskName $TaskName | Out-Null
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Start-ScheduledTask -TaskName $TaskName
}

function Test-FreshState {
    for ($Attempt = 0; $Attempt -lt 100; $Attempt++) {
        & $Binary status --json *> $null
        if ($LASTEXITCODE -eq 0) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

if ($Action -eq "off") {
    Write-OffMarker
    try {
        Stop-OwnedTask
    } catch {
        $OffFailure = $_.Exception.Message
        Remove-StateFile $Marker
        $Restored = $false
        try {
            Start-OwnedTask
            $Restored = Test-FreshState
        } catch { }
        if ($Restored) {
            throw "Memory Supervisor could not be turned off; it remains ON with fresh protection: $OffFailure"
        }
        Write-OffMarker
        try { Stop-OwnedTask } catch { }
        Remove-StateFile (Join-Path $StateDir "admission-green.lease")
        throw "Memory Supervisor could not restore fresh ON protection; it remains OFF: $OffFailure"
    }
    Remove-StateFile (Join-Path $StateDir "admission-green.lease")
    Write-Host "Memory Supervisor is OFF. The scheduled task stays disabled across restarts; installed CLI hooks pass through."
    Write-Host "Run 'memory-supervisor on' once to restore protection."
    exit 0
}

Remove-StateFile $Marker
Remove-StateFile (Join-Path $StateDir "state.json")
Remove-StateFile (Join-Path $StateDir "admission-green.lease")
try {
    Start-OwnedTask
    if (-not (Test-FreshState)) { throw "Supervisor did not publish a fresh state" }
} catch {
    Write-OffMarker
    try { Stop-OwnedTask } catch { }
    throw "Memory Supervisor did not return to a fresh running state; it remains off: $($_.Exception.Message)"
}
Write-Host "Memory Supervisor is ON. Protection is running and stays enabled across restarts."
Write-Host "Installed Claude Code and Codex connections remain in place."
