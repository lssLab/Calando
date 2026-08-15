[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Binary,

    [string]$FailingBinary
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).ProviderPath
$Binary = (Resolve-Path $Binary).ProviderPath
$Sandbox = Join-Path ([IO.Path]::GetTempPath()) ("memory-supervisor-windows-install-" + [guid]::NewGuid().ToString("N"))
$Runtime = Join-Path $Sandbox "runtime"
$TaskName = "MemorySupervisorCI-" + [guid]::NewGuid().ToString("N")
$ForeignTaskName = "MemorySupervisorForeignCI-" + [guid]::NewGuid().ToString("N")
$PreviousTaskName = $env:MEMORY_SUPERVISOR_TASK_NAME
$PreviousRuntime = $env:MEMORY_SUPERVISOR_RUNTIME_DIR
$PreviousBinary = $env:MEMORY_SUPERVISOR_BINARY_SOURCE
$PreviousCodexHome = $env:CODEX_HOME

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Invoke-Install {
    param([string]$Source)
    $env:MEMORY_SUPERVISOR_BINARY_SOURCE = $Source
    & (Join-Path $Root "install.ps1")
    if ($LASTEXITCODE -ne 0) { throw "install.ps1 exited $LASTEXITCODE" }
}

function Invoke-Uninstall {
    & (Join-Path $Root "uninstall.ps1")
    if ($LASTEXITCODE -ne 0) { throw "uninstall.ps1 exited $LASTEXITCODE" }
}

function Wait-FreshStatus {
    param([string]$InstalledBinary)
    for ($Attempt = 0; $Attempt -lt 50; $Attempt++) {
        & $InstalledBinary status --json *> $null
        if ($LASTEXITCODE -eq 0) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "installed daemon never published a fresh status"
}

New-Item -ItemType Directory -Force -Path $Sandbox | Out-Null
try {
    $env:MEMORY_SUPERVISOR_TASK_NAME = $TaskName
    $env:MEMORY_SUPERVISOR_RUNTIME_DIR = $Runtime
    $env:CODEX_HOME = Join-Path $Sandbox "codex-home"
    New-Item -ItemType Directory -Force -Path $env:CODEX_HOME | Out-Null

    if ($FailingBinary) {
        $FailingBinary = (Resolve-Path $FailingBinary).ProviderPath
    } else {
        $FailingBinary = Join-Path $Sandbox "failing-runtime.exe"
        & rustc --edition=2024 (Join-Path $Root "tests\fixtures\failing-runtime.rs") -O -o $FailingBinary
        if ($LASTEXITCODE -ne 0) { throw "could not build activation-failure canary" }
    }
    $FailedAsExpected = $false
    try {
        Invoke-Install $FailingBinary
    } catch {
        $FailedAsExpected = $true
    }
    Assert-True $FailedAsExpected "clean activation-failure canary unexpectedly installed"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $Runtime "memory-supervisor.exe"))) `
        "clean activation failure left the candidate binary"
    Assert-True ($null -eq (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) `
        "clean activation failure left the scheduled task"

    Invoke-Install $Binary

    $InstalledBinary = Join-Path $Runtime "memory-supervisor.exe"
    Assert-True (Test-Path -LiteralPath $InstalledBinary -PathType Leaf) "clean install missed the Rust binary"
    $InstalledTask = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Assert-True ($null -ne $InstalledTask) `
        "clean install missed the scheduled task"
    Assert-True ([string]$InstalledTask.Actions[0].Arguments -eq "daemon --foreground --detach-console") `
        "scheduled task can leave the daemon console window open"
    Wait-FreshStatus $InstalledBinary
    $InitialHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $InstalledBinary).Hash

    & $InstalledBinary off
    if ($LASTEXITCODE -ne 0) { throw "memory-supervisor off exited $LASTEXITCODE" }
    Assert-True (Test-Path -LiteralPath (Join-Path $HOME ".memory-supervisor\power-off") -PathType Leaf) `
        "off did not persist its power marker"
    Assert-True ([string](Get-ScheduledTask -TaskName $TaskName).State -eq "Disabled") `
        "off did not disable the scheduled task"
    $OffStatus = (& $InstalledBinary status --json | ConvertFrom-Json)
    Assert-True ($OffStatus.power -eq "off") "status did not report intentional off state"
    $GateOutput = ('{}' | & $InstalledBinary gate claude SessionStart | Out-String).Trim()
    Assert-True (-not $GateOutput) "an off hook did not pass through silently"

    Invoke-Install $Binary
    Assert-True ([string](Get-ScheduledTask -TaskName $TaskName).State -eq "Disabled") `
        "reinstall turned an intentionally off supervisor back on"
    Assert-True (Test-Path -LiteralPath (Join-Path $HOME ".memory-supervisor\power-off") -PathType Leaf) `
        "reinstall lost the persisted off state"

    & $InstalledBinary on
    if ($LASTEXITCODE -ne 0) { throw "memory-supervisor on exited $LASTEXITCODE" }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $HOME ".memory-supervisor\power-off"))) `
        "on did not clear the persisted off state"
    Wait-FreshStatus $InstalledBinary

    Invoke-Install $Binary
    Assert-True ((Get-FileHash -Algorithm SHA256 -LiteralPath $InstalledBinary).Hash -eq $InitialHash) `
        "reinstall changed the verified binary unexpectedly"
    Assert-True ($null -ne (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) `
        "reinstall lost the owned scheduled task"
    Wait-FreshStatus $InstalledBinary

    $FailedAsExpected = $false
    try {
        Invoke-Install $FailingBinary
    } catch {
        $FailedAsExpected = $true
    }
    Assert-True $FailedAsExpected "activation-failure canary unexpectedly installed"
    Assert-True ((Get-FileHash -Algorithm SHA256 -LiteralPath $InstalledBinary).Hash -eq $InitialHash) `
        "failed activation did not restore the previous Rust binary"
    Assert-True ($null -ne (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) `
        "failed activation did not restore the previous task"
    Wait-FreshStatus $InstalledBinary

    Invoke-Uninstall
    Assert-True (-not (Test-Path -LiteralPath $InstalledBinary)) "uninstall left the installed binary"
    Assert-True ($null -eq (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) `
        "uninstall left the owned scheduled task"

    $env:MEMORY_SUPERVISOR_TASK_NAME = $ForeignTaskName
    $env:MEMORY_SUPERVISOR_RUNTIME_DIR = Join-Path $Sandbox "foreign-runtime"
    $ForeignAction = New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\cmd.exe" -Argument "/d /c exit 0"
    $ForeignTrigger = New-ScheduledTaskTrigger -Once -At ([DateTime]::Now.AddHours(1))
    Register-ScheduledTask -TaskName $ForeignTaskName -Action $ForeignAction -Trigger $ForeignTrigger -Force | Out-Null
    $Refused = $false
    try {
        Invoke-Install $Binary
    } catch {
        $Refused = $_.Exception.Message -match "foreign scheduled task"
    }
    Assert-True $Refused "installer did not refuse a foreign task with the requested name"
    Invoke-Uninstall
    $Foreign = Get-ScheduledTask -TaskName $ForeignTaskName -ErrorAction SilentlyContinue
    Assert-True ($null -ne $Foreign) "uninstaller removed a foreign scheduled task"
    Assert-True ([string]$Foreign.Actions[0].Execute -match '(?i)cmd\.exe$') `
        "foreign scheduled task changed"

    Write-Host "WINDOWS-INSTALL PASS"
} finally {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $ForeignTaskName -Confirm:$false -ErrorAction SilentlyContinue
    if ($null -eq $PreviousTaskName) { Remove-Item Env:MEMORY_SUPERVISOR_TASK_NAME -ErrorAction SilentlyContinue }
    else { $env:MEMORY_SUPERVISOR_TASK_NAME = $PreviousTaskName }
    if ($null -eq $PreviousRuntime) { Remove-Item Env:MEMORY_SUPERVISOR_RUNTIME_DIR -ErrorAction SilentlyContinue }
    else { $env:MEMORY_SUPERVISOR_RUNTIME_DIR = $PreviousRuntime }
    if ($null -eq $PreviousBinary) { Remove-Item Env:MEMORY_SUPERVISOR_BINARY_SOURCE -ErrorAction SilentlyContinue }
    else { $env:MEMORY_SUPERVISOR_BINARY_SOURCE = $PreviousBinary }
    if ($null -eq $PreviousCodexHome) { Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue }
    else { $env:CODEX_HOME = $PreviousCodexHome }
    Remove-Item -Recurse -Force -LiteralPath $Sandbox -ErrorAction SilentlyContinue
}
