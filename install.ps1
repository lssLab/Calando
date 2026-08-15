[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
$RuntimeDir = if ($env:MEMORY_SUPERVISOR_RUNTIME_DIR) {
    $env:MEMORY_SUPERVISOR_RUNTIME_DIR
} else {
    Join-Path $HOME ".local\lib\memory-supervisor"
}
$InstalledBinary = Join-Path $RuntimeDir "memory-supervisor.exe"
$PointerDir = Join-Path $HOME ".memory-supervisor"
$PoweredOff = Test-Path -LiteralPath (Join-Path $PointerDir "power-off")
$TaskName = if ($env:MEMORY_SUPERVISOR_TASK_NAME) {
    $env:MEMORY_SUPERVISOR_TASK_NAME
} else {
    "MemorySupervisor"
}
if ($TaskName -notmatch '^[A-Za-z0-9_.-]{1,100}$') {
    throw "MEMORY_SUPERVISOR_TASK_NAME must contain only letters, numbers, dot, underscore, or hyphen"
}
$Temporary = Join-Path ([IO.Path]::GetTempPath()) ("memory-supervisor-install-" + [guid]::NewGuid().ToString("N"))
$Utf8 = [Text.UTF8Encoding]::new($false)
$Cutover = $false
$Success = $false
$Activated = $false
$HadBinary = $false
$HadTask = $false
$HadState = $false
$BinaryReplaced = $false
$PreviousRoot = $Root
$InstallRootPointer = Join-Path $PointerDir "install-root"
if (Test-Path -LiteralPath $InstallRootPointer -PathType Leaf) {
    $PointedRoot = ([IO.File]::ReadAllText($InstallRootPointer)).Trim()
    if ($PointedRoot) { $PreviousRoot = $PointedRoot }
}
$PythonFiles = @(
    "supervisor.py",
    "memory_supervisor_config.py",
    "memory_supervisor_events.py",
    "memory_supervisor_platform.py",
    "notify/notify.py",
    "notify/terminal_notice.py"
)
$PythonRollbackDir = Join-Path $Temporary "python-rollback"
$OldPythonRuntime = $false
New-Item -ItemType Directory -Force -Path $Temporary | Out-Null
$PointerNames = @("state-dir", "federation-dir", "binary", "install-root")
$PreviousPointers = @{}
foreach ($Name in $PointerNames) {
    $Path = Join-Path $PointerDir $Name
    $PreviousPointers[$Name] = (Test-Path -LiteralPath $Path -PathType Leaf)
    if ($PreviousPointers[$Name]) {
        [IO.File]::WriteAllBytes(
            (Join-Path $Temporary ("pointer-" + $Name)),
            [IO.File]::ReadAllBytes($Path)
        )
    }
}

function Restore-PreviousPointers {
    New-Item -ItemType Directory -Force -Path $PointerDir | Out-Null
    foreach ($Name in $PointerNames) {
        $Path = Join-Path $PointerDir $Name
        if ($PreviousPointers[$Name]) {
            [IO.File]::WriteAllBytes(
                $Path,
                [IO.File]::ReadAllBytes((Join-Path $Temporary ("pointer-" + $Name)))
            )
        } else {
            Remove-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        }
    }
}

function Test-PythonRollbackComplete {
    foreach ($Relative in $PythonFiles) {
        if (-not (Test-Path -LiteralPath (Join-Path $PythonRollbackDir $Relative) -PathType Leaf)) {
            return $false
        }
    }
    return $true
}

function Copy-PythonRollback {
    param([string]$SourceRoot)
    Remove-Item -Recurse -Force -LiteralPath $PythonRollbackDir -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $PythonRollbackDir | Out-Null
    foreach ($Relative in $PythonFiles) {
        $Source = Join-Path $SourceRoot $Relative
        if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { return $false }
        $Destination = Join-Path $PythonRollbackDir $Relative
        New-Item -ItemType Directory -Force -Path (Split-Path $Destination) | Out-Null
        Copy-Item -LiteralPath $Source -Destination $Destination
    }
    return $true
}

function Prepare-PythonRollback {
    $Prepared = (Copy-PythonRollback $PreviousRoot)
    if (-not $Prepared -and $PreviousRoot -ne $Root) {
        $Prepared = (Copy-PythonRollback $Root)
    }
    if (-not $Prepared) {
        $Git = Get-Command git -ErrorAction SilentlyContinue
        if ($Git) {
            foreach ($SourceRoot in @($PreviousRoot, $Root) | Select-Object -Unique) {
                if (-not (Test-Path -LiteralPath (Join-Path $SourceRoot ".git"))) { continue }
                foreach ($Revision in @("ORIG_HEAD", "HEAD^")) {
                    & $Git.Source -C $SourceRoot cat-file -e "${Revision}:supervisor.py" 2>$null
                    if ($LASTEXITCODE -ne 0) { continue }
                    $Archive = Join-Path $Temporary "python-rollback.zip"
                    Remove-Item -Force -LiteralPath $Archive -ErrorAction SilentlyContinue
                    & $Git.Source -C $SourceRoot archive --format=zip "--output=$Archive" `
                        $Revision -- $PythonFiles
                    if ($LASTEXITCODE -ne 0) { continue }
                    Remove-Item -Recurse -Force -LiteralPath $PythonRollbackDir -ErrorAction SilentlyContinue
                    Expand-Archive -LiteralPath $Archive -DestinationPath $PythonRollbackDir -Force
                    if (Test-PythonRollbackComplete) { $Prepared = $true; break }
                }
                if ($Prepared) { break }
            }
        }
    }
    if (-not $Prepared -or -not (Test-PythonRollbackComplete)) {
        throw "Cannot preserve the running Python supervisor for rollback; installation was not changed"
    }
    $script:OldPythonRuntime = $true
}

function Restore-PythonRollback {
    if (-not $OldPythonRuntime) { return }
    foreach ($Relative in $PythonFiles) {
        $Destination = Join-Path $PreviousRoot $Relative
        New-Item -ItemType Directory -Force -Path (Split-Path $Destination) | Out-Null
        Copy-Item -Force -LiteralPath (Join-Path $PythonRollbackDir $Relative) -Destination $Destination
    }
    [IO.File]::WriteAllText(
        (Join-Path $PreviousRoot ".memory-supervisor-python-rollback"),
        "restored after failed Rust activation`n",
        $Utf8
    )
}

function Remove-EmergencyPythonRollback {
    $Marker = Join-Path $PreviousRoot ".memory-supervisor-python-rollback"
    if (-not (Test-Path -LiteralPath $Marker -PathType Leaf)) { return }
    foreach ($Relative in $PythonFiles) {
        Remove-Item -Force -LiteralPath (Join-Path $PreviousRoot $Relative) -ErrorAction SilentlyContinue
    }
    Remove-Item -Force -LiteralPath $Marker -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $PreviousRoot "notify") -ErrorAction SilentlyContinue
}

function Get-ReleaseTarget {
    $Architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    switch ($Architecture) {
        "X64" { return "x86_64-pc-windows-msvc" }
        "Arm64" { return "aarch64-pc-windows-msvc" }
        default { throw "No release binary is available for Windows/$Architecture" }
    }
}

function Get-RuntimeCandidate {
    if ($env:MEMORY_SUPERVISOR_BINARY_SOURCE) {
        if (-not (Test-Path -LiteralPath $env:MEMORY_SUPERVISOR_BINARY_SOURCE -PathType Leaf)) {
            throw "MEMORY_SUPERVISOR_BINARY_SOURCE is not a file"
        }
        return $env:MEMORY_SUPERVISOR_BINARY_SOURCE
    }
    $Cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($Cargo -and
        (Test-Path -LiteralPath (Join-Path $Root "Cargo.toml")) -and
        -not (Test-Path -LiteralPath (Join-Path $Root ".memory-supervisor-release-source"))) {
        Write-Host "building the current Memory Supervisor source with Rust"
        & $Cargo.Source build --manifest-path (Join-Path $Root "Cargo.toml") --release --locked
        if ($LASTEXITCODE -ne 0) { throw "Rust release build failed" }
        return (Join-Path $Root "target\release\memory-supervisor.exe")
    }
    $Target = Get-ReleaseTarget
    $Asset = "memory-supervisor-$Target.exe"
    $Base = if ($env:MEMORY_SUPERVISOR_RELEASE_BASE_URL) {
        $env:MEMORY_SUPERVISOR_RELEASE_BASE_URL.TrimEnd("/")
    } else {
        "https://github.com/lssLab/claude-code-codex-memory-supervisor-prerelease/releases/latest/download"
    }
    $Binary = Join-Path $Temporary $Asset
    $Checksum = "$Binary.sha256"
    Write-Host "downloading verified release binary: $Asset"
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset" -OutFile $Binary
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset.sha256" -OutFile $Checksum
    $Expected = (([IO.File]::ReadAllText($Checksum)).Trim() -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Binary).Hash.ToLowerInvariant()
    if (-not $Expected -or $Expected -ne $Actual) { throw "release checksum verification failed" }
    return $Binary
}

function Test-OwnedTask {
    param($Task)
    if (-not $Task) { return $false }
    $Actions = @($Task.Actions)
    if ($Actions.Count -ne 1) { return $false }
    $Action = $Actions[0]
    $Execute = [string]$Action.Execute
    $Arguments = [string]$Action.Arguments
    $NewRuntime = ([IO.Path]::GetFullPath($Execute) -eq [IO.Path]::GetFullPath($InstalledBinary) -and
        $Arguments -match '(?i)(^|\s)daemon(\s|$)')
    $PythonRuntime = ($Arguments.IndexOf(
        (Join-Path $PreviousRoot "supervisor.py"), [StringComparison]::OrdinalIgnoreCase
    ) -ge 0 -or $Arguments.IndexOf(
        (Join-Path $Root "supervisor.py"), [StringComparison]::OrdinalIgnoreCase
    ) -ge 0)
    return $NewRuntime -or $PythonRuntime
}

function Get-OwnedRuntimeProcesses {
    $OldScripts = @(
        (Join-Path $PreviousRoot "supervisor.py"),
        (Join-Path $Root "supervisor.py")
    ) | Select-Object -Unique
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        $Process = $_
        ($Process.ExecutablePath -and [string]::Equals(
            [string]$Process.ExecutablePath, $InstalledBinary, [StringComparison]::OrdinalIgnoreCase
        )) -or ($Process.CommandLine -and @($OldScripts | Where-Object {
            $Process.CommandLine.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0
        }).Count -gt 0)
    })
}

function Wait-OwnedRuntimeStopped {
    for ($Attempt = 0; $Attempt -lt 50; $Attempt++) {
        if (@(Get-OwnedRuntimeProcesses).Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

function Restore-PreviousRuntime {
    Write-Warning "Activation failed; restoring the previous Memory Supervisor runtime"
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if (-not $HadTask) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    }
    if ($BinaryReplaced -and -not (Wait-OwnedRuntimeStopped)) {
        @(Get-OwnedRuntimeProcesses) | ForEach-Object {
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
    if ($BinaryReplaced) {
        if ($HadBinary -and (Test-Path -LiteralPath (Join-Path $Temporary "previous-binary"))) {
            Copy-Item -Force -LiteralPath (Join-Path $Temporary "previous-binary") -Destination $InstalledBinary
        } elseif (-not $HadBinary) {
            Remove-Item -Force -LiteralPath $InstalledBinary -ErrorAction SilentlyContinue
        }
    }
    Restore-PythonRollback
    if ($HadState -and (Test-Path -LiteralPath (Join-Path $Temporary "previous-state.json"))) {
        Copy-Item -Force -LiteralPath (Join-Path $Temporary "previous-state.json") `
            -Destination (Join-Path $StateDir "state.json")
    } else {
        Remove-Item -Force -LiteralPath (Join-Path $StateDir "state.json") -ErrorAction SilentlyContinue
    }
    Restore-PreviousPointers
    if ($HadTask -and (Test-Path -LiteralPath (Join-Path $Temporary "previous-task.xml"))) {
        $Xml = [IO.File]::ReadAllText((Join-Path $Temporary "previous-task.xml"))
        Register-ScheduledTask -TaskName $TaskName -Xml $Xml -Force | Out-Null
        if ($PoweredOff) {
            Disable-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue | Out-Null
        } else {
            Enable-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue | Out-Null
            Start-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
        }
        if (-not (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) {
            throw "Previous scheduled task could not be restored: $TaskName"
        }
    }
}

try {
    Write-Host "[1/7] acquire and verify Rust runtime"
    $Candidate = Get-RuntimeCandidate
    try {
        & $Candidate --version
    } catch {
        $Detail = if ($_.Exception.InnerException -and $_.Exception.InnerException.Message) {
            $_.Exception.InnerException.Message
        } else {
            $_.Exception.Message
        }
        throw "Windows refused to execute the runtime candidate before cutover; the existing installation is unchanged. The current Windows executable is unsigned. On Windows 11, Smart App Control must remain Off while using it. Windows 10 has no Smart App Control, so check SmartScreen or an organization policy instead. You can also use the WSL build without changing Smart App Control. Detail: $Detail"
    }
    if ($LASTEXITCODE -ne 0) { throw "runtime candidate did not execute" }

    Write-Host "[2/7] preserve state and switch the owned daemon"
    $ExistingTask = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($ExistingTask) {
        if (-not (Test-OwnedTask $ExistingTask)) {
            throw "Refusing to replace foreign scheduled task: $TaskName"
        }
        Export-ScheduledTask -TaskName $TaskName | Set-Content -Encoding Unicode -Path (Join-Path $Temporary "previous-task.xml")
        $HadTask = $true
        if (([string]$ExistingTask.Actions[0].Arguments).IndexOf(
            "supervisor.py", [StringComparison]::OrdinalIgnoreCase
        ) -ge 0) {
            Prepare-PythonRollback
        }
    }

    New-Item -ItemType Directory -Force -Path $RuntimeDir | Out-Null
    if (Test-Path -LiteralPath $InstalledBinary -PathType Leaf) {
        Copy-Item -LiteralPath $InstalledBinary -Destination (Join-Path $Temporary "previous-binary")
        $HadBinary = $true
    }
    $Cutover = $true
    if ($ExistingTask) {
        Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
        if (-not (Wait-OwnedRuntimeStopped)) {
            throw "Owned supervisor did not stop; installation aborted without force-kill"
        }
    }

    $NewBinary = Join-Path $RuntimeDir (".memory-supervisor.new." + $PID + ".exe")
    Copy-Item -Force -LiteralPath $Candidate -Destination $NewBinary
    Move-Item -Force -LiteralPath $NewBinary -Destination $InstalledBinary
    $BinaryReplaced = $true

    & $InstalledBinary integration migrate-names
    if ($LASTEXITCODE -ne 0) { throw "install-name migration failed" }
    $StateDir = (& $InstalledBinary integration path state | Select-Object -Last 1).Trim()
    $FederationDir = (& $InstalledBinary integration path federation | Select-Object -Last 1).Trim()
    if (-not $StateDir -or -not $FederationDir) { throw "runtime paths could not be resolved" }
    New-Item -ItemType Directory -Force -Path $PointerDir, $StateDir, $FederationDir | Out-Null
    $StatePath = Join-Path $StateDir "state.json"
    if (Test-Path -LiteralPath $StatePath -PathType Leaf) {
        Copy-Item -LiteralPath $StatePath -Destination (Join-Path $Temporary "previous-state.json")
        $HadState = $true
    }
    Remove-Item -Force -LiteralPath $StatePath -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $StateDir "admission-green.lease") -ErrorAction SilentlyContinue
    [IO.File]::WriteAllText((Join-Path $PointerDir "state-dir"), $StateDir + "`n", $Utf8)
    [IO.File]::WriteAllText((Join-Path $PointerDir "federation-dir"), $FederationDir + "`n", $Utf8)
    [IO.File]::WriteAllText((Join-Path $PointerDir "binary"), $InstalledBinary + "`n", $Utf8)
    [IO.File]::WriteAllText((Join-Path $PointerDir "install-root"), $Root + "`n", $Utf8)

    $Action = New-ScheduledTaskAction -Execute $InstalledBinary `
        -Argument "daemon --foreground --detach-console"
    $Trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
    $Settings = New-ScheduledTaskSettingsSet -RestartCount 5 `
        -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger `
        -Settings $Settings -Description "Claude Code & Codex CLI Memory Supervisor" -Force | Out-Null
    if ($PoweredOff) {
        Disable-ScheduledTask -TaskName $TaskName | Out-Null
        if ([string](Get-ScheduledTask -TaskName $TaskName).State -ne "Disabled") {
            throw "Power state is OFF but the scheduled task remains enabled"
        }
    } else {
        Enable-ScheduledTask -TaskName $TaskName | Out-Null
        Start-ScheduledTask -TaskName $TaskName
    }

    Write-Host "[3/7] verify the new Rust daemon before changing hooks or commands"
    if ($PoweredOff) {
        Write-Host "Power state is OFF; daemon activation remains disabled"
    } else {
        $Published = $false
        $Healthy = $false
        for ($Attempt = 0; $Attempt -lt 200; $Attempt++) {
            if ((Test-Path -LiteralPath $StatePath -PathType Leaf) -and
                (Get-Item -LiteralPath $StatePath).Length -gt 0) {
                $Published = $true
                & $InstalledBinary status --json *> $null
                if ($LASTEXITCODE -eq 0) { $Healthy = $true; break }
            }
            Start-Sleep -Milliseconds 100
        }
        if (-not $Published) { throw "Supervisor did not publish a fresh state" }
        if (-not $Healthy) {
            & $InstalledBinary status --json
            throw "Supervisor state did not become healthy before the activation deadline"
        }
        $Activated = $true
    }
    # Remove the pre-rename task only after the Rust task has published fresh state.
    $LegacyTask = if ($TaskName -eq "MemorySupervisor") {
        Get-ScheduledTask -TaskName "ClaudeMemoryGovernor" -ErrorAction SilentlyContinue
    } else { $null }
    if ($LegacyTask -and @($LegacyTask.Actions).Count -eq 1) {
        $LegacyArguments = [string]$LegacyTask.Actions[0].Arguments
        $OwnedLegacy = @(
            (Join-Path $PreviousRoot "governor.py"),
            (Join-Path $Root "governor.py")
        ) | Select-Object -Unique | Where-Object {
            $LegacyArguments.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0
        }
        if (@($OwnedLegacy).Count -gt 0) {
            Stop-ScheduledTask -TaskName "ClaudeMemoryGovernor" -ErrorAction SilentlyContinue
            Unregister-ScheduledTask -TaskName "ClaudeMemoryGovernor" -Confirm:$false
        }
    }
    Remove-EmergencyPythonRollback

    Write-Host "[4/7] connect supported Claude Code and Codex hooks"
    # Record whether the merge changed a provider's hook wiring so the summary
    # can give the correct reload path. A binary-only update needs no action;
    # changed Codex wiring reloads through CLI /hooks or App Settings. Claude
    # normally reloads the user hook in an already trusted workspace; an
    # untrusted interactive workspace still needs the user's trust decision.
    # The merge prints "updated: <file>" or "unchanged: <file>".
    $script:HookWiringChanged = @()
    function Resolve-ClaudeExecutable {
        # An absent or unsupported Claude is an expected probe result, not an
        # installer failure. Windows PowerShell turns native stderr into an
        # ErrorRecord when ErrorActionPreference is Stop, so isolate the probe
        # and restore the caller's strict error policy immediately afterward.
        $PreviousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $Resolved = @(& $InstalledBinary integration resolve-claude 2>$null)
            $ResolveExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $PreviousErrorActionPreference
        }
        if ($ResolveExitCode -ne 0 -or @($Resolved).Count -eq 0) { return $null }
        return ([string]($Resolved | Select-Object -Last 1)).Trim()
    }
    $script:ClaudeCommandPath = Resolve-ClaudeExecutable
    function Connect-Provider {
        param([string]$Provider, [string]$CommandName, [string]$Target, [string]$Requirement)
        $CommandPath = $null
        if ($Provider -eq "claude") {
            $CommandPath = $script:ClaudeCommandPath
        } else {
            $Command = Get-Command $CommandName -ErrorAction SilentlyContinue
            if ($Command) { $CommandPath = $Command.Source }
        }
        if (-not [string]::IsNullOrWhiteSpace($CommandPath)) {
            & $InstalledBinary integration "check-$Provider" --command $CommandPath
            if ($LASTEXITCODE -eq 0) {
                $Merge = & $InstalledBinary integration hooks --target $Target --provider $Provider --binary $InstalledBinary | Out-String
                if ($LASTEXITCODE -ne 0) { throw "$Provider hook merge failed" }
                Write-Host $Merge.TrimEnd()
                if ($Merge -match 'updated: ') { $script:HookWiringChanged += $Provider }
                & $InstalledBinary integration hooks --target $Target --provider $Provider --binary $InstalledBinary --check
                if ($LASTEXITCODE -ne 0) { throw "$Provider hook verification failed" }
                return
            }
        }
        if ($Provider -ne "claude" -and (Test-Path -LiteralPath $Target)) {
            & $InstalledBinary integration hooks --target $Target --provider $Provider --binary $InstalledBinary --remove
            if ($LASTEXITCODE -ne 0) { throw "$Provider stale hook cleanup failed" }
        }
        if ($Provider -eq "claude") {
            Write-Warning "$Provider integration skipped: $Requirement; any existing Memory Supervisor hook was preserved"
        } else {
            Write-Warning "$Provider integration skipped: $Requirement"
        }
    }
    $ClaudeSettings = Join-Path $HOME ".claude\settings.json"
    $CodexHome = if ([string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        Join-Path $HOME ".codex"
    } else {
        $env:CODEX_HOME
    }
    $CodexHooks = Join-Path $CodexHome "hooks.json"
    $DefaultCodexHooks = Join-Path $HOME ".codex\hooks.json"
    $ClaudeDetected = (Test-Path (Split-Path $ClaudeSettings)) -or
        (-not [string]::IsNullOrWhiteSpace($script:ClaudeCommandPath))
    if ($ClaudeDetected) {
        Connect-Provider "claude" "claude" $ClaudeSettings "Claude Code 2.1.217+ is required"
    }
    if ((Test-Path (Split-Path $CodexHooks)) -or (Get-Command codex -ErrorAction SilentlyContinue)) {
        Connect-Provider "codex" "codex" $CodexHooks "Codex 0.145.0+ with stable hooks enabled is required"
    }
    # Older releases always wrote ~/.codex even when Codex used a different CODEX_HOME. Refresh an
    # existing owned CLI route so it receives source ownership, but never create a second route.
    if (-not $DefaultCodexHooks.Equals($CodexHooks, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $DefaultCodexHooks -PathType Leaf) -and
        ([IO.File]::ReadAllText($DefaultCodexHooks) -match 'memory-supervisor|hooks[/\\]gate')) {
        $LegacyMerge = & $InstalledBinary integration hooks --target $DefaultCodexHooks `
            --provider codex --binary $InstalledBinary | Out-String
        if ($LASTEXITCODE -ne 0) { throw "legacy Codex hook merge failed" }
        Write-Host $LegacyMerge.TrimEnd()
        if ($LegacyMerge -match 'updated: ') { $script:HookWiringChanged += "codex" }
        & $InstalledBinary integration hooks --target $DefaultCodexHooks --provider codex `
            --binary $InstalledBinary --check
        if ($LASTEXITCODE -ne 0) { throw "legacy Codex hook verification failed" }
    }
    if ($script:HookWiringChanged.Count -gt 0) {
        Write-Host "Hook wiring changed for: $($script:HookWiringChanged -join ' '). Follow the provider-specific reload and trust steps below."
        if ($script:HookWiringChanged -contains "claude") {
            Write-Host "Claude Code: USER ACTION REQUIRED for an untrusted workspace. There is no per-hook approval, but interactive Claude holds every settings-file hook until the user accepts workspace trust for the current folder or a parent. Accept only a folder you trust. /hooks is read-only. An already-trusted running session normally reloads the User Settings hook; restart only if it does not appear."
        }
        if ($script:HookWiringChanged -contains "codex") {
            Write-Host "Codex CLI: USER ACTION REQUIRED. Open /hooks in the CLI you are using. Confirm that all seven Memory Supervisor entries are trusted and on; trust only entries marked for review and enable only entries that are off. Then close /hooks and continue the current work. Restarting does not grant trust."
            Write-Host "Codex App: USER ACTION REQUIRED. The user must personally use Settings > Hooks, not /hooks, to trust every new or changed Memory Supervisor entry and enable any disabled entry. Restarting cannot grant trust; applying the change reloads every existing task currently loaded by the shared App Server, so continue an existing task with its next request."
            Write-Host "Shared CODEX_HOME edge case: if another process already saved approval and this running surface has no change left to save, restart only that pre-existing App or CLI once so it reads the shared trust record."
        }
    } else {
        Write-Host "Hook wiring unchanged: a binary-only update does not create new Codex trust. Run memory-status --connections anyway; any existing Codex disabled or untrusted state still requires the user action it reports."
    }

    Write-Host "[5/7] install skills and user commands"
    function Test-SupervisorSkillPath {
        param([string]$Path)
        $Manifest = Join-Path $Path "SKILL.md"
        return ((Test-Path -LiteralPath $Manifest) -and
            ([IO.File]::ReadAllText($Manifest) -match '(?m)^name: (memory-governor|memory-supervisor)$'))
    }
    function Install-SkillJunction {
        param([string]$Path)
        New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null
        $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($Item) {
            if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                $Target = @($Item.Target) | Select-Object -First 1
                if ($Target -and ([IO.Path]::GetFullPath($Target) -eq [IO.Path]::GetFullPath($Root))) { return }
                if (Test-SupervisorSkillPath $Path) { [IO.Directory]::Delete($Path) }
                else { Write-Warning "Preserving existing foreign skill link: $Path"; return }
            } else { Write-Warning "Preserving existing non-link skill: $Path"; return }
        }
        New-Item -ItemType Junction -Path $Path -Target $Root | Out-Null
    }
    foreach ($Path in @(
        (Join-Path $HOME ".claude\skills\memory-governor"),
        (Join-Path $HOME ".agents\skills\memory-governor"),
        (Join-Path $HOME ".codex\skills\memory-governor")
    )) {
        $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($Item -and ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
            (Test-SupervisorSkillPath $Path)) { [IO.Directory]::Delete($Path) }
    }
    Install-SkillJunction (Join-Path $HOME ".claude\skills\memory-supervisor")
    Install-SkillJunction (Join-Path $HOME ".agents\skills\memory-supervisor")
    if (Test-Path (Join-Path $HOME ".codex")) {
        Install-SkillJunction (Join-Path $HOME ".codex\skills\memory-supervisor")
    }

    function Install-CommandFile {
        param([string]$Source, [string]$Path)
        New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null
        $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($Item) {
            if (-not $Item.PSIsContainer -and -not ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
                [IO.File]::ReadAllText($Path) -eq [IO.File]::ReadAllText($Source)) { return }
            Write-Warning "Preserving customized or non-regular command path: $Path"
            return
        }
        Copy-Item -LiteralPath $Source -Destination $Path
    }
    Install-CommandFile (Join-Path $Root "commands\claude\memory-status.md") `
        (Join-Path $HOME ".claude\commands\memory-status.md")
    if (Test-Path (Join-Path $HOME ".codex")) {
        Install-CommandFile (Join-Path $Root "commands\codex\memory-status.md") `
            (Join-Path $HOME ".codex\prompts\memory-status.md")
    }

    $Bin = Join-Path $HOME ".local\bin"
    New-Item -ItemType Directory -Force -Path $Bin | Out-Null
    function Install-OwnedLauncher {
        param([string]$Path, [string]$Subcommand)
        $Marker = "Memory Supervisor managed command"
        $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($Item) {
            if ($Item.PSIsContainer -or ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                Write-Warning "Preserving existing non-regular command path: $Path"; return
            }
            $Existing = [IO.File]::ReadAllText($Path)
            if ($Existing.IndexOf($Marker, [StringComparison]::OrdinalIgnoreCase) -lt 0 -and
                $Existing.IndexOf("memory-status.py", [StringComparison]::OrdinalIgnoreCase) -lt 0 -and
                $Existing.IndexOf("memory-control.py", [StringComparison]::OrdinalIgnoreCase) -lt 0) {
                Write-Warning "Preserving existing foreign command: $Path"; return
            }
        }
        $Suffix = if ($Subcommand) { " $Subcommand" } else { "" }
        $Content = "@echo off`r`n@rem $Marker`r`n`"$InstalledBinary`"$Suffix %*`r`nexit /b %errorlevel%`r`n"
        [IO.File]::WriteAllText($Path, $Content, $Utf8)
    }
    Install-OwnedLauncher (Join-Path $Bin "memory-supervisor.cmd") ""
    Install-OwnedLauncher (Join-Path $Bin "memory-status.cmd") "status"
    foreach ($Legacy in @("memory-control.cmd", "cmg-status.cmd", "cmg-control.cmd", "codex-governed.cmd")) {
        $Path = Join-Path $Bin $Legacy
        $Item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
        if ($Item -and -not $Item.PSIsContainer) {
            $Content = [IO.File]::ReadAllText($Path)
            if ($Content -match '(?i)(cmg-|codex-governed|memory governor)' -or
                $Content.IndexOf("Memory Supervisor managed command", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
                $Content.IndexOf("memory-control.py", [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                Remove-Item -Force -LiteralPath $Path
            }
        }
    }
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $UserPath) { $UserPath = "" }
    if (($UserPath -split ";") -notcontains $Bin) {
        [Environment]::SetEnvironmentVariable("Path", (($UserPath.TrimEnd(";") + ";" + $Bin).TrimStart(";")), "User")
        $env:Path += ";$Bin"
    }

    Write-Host "[6/7] preserve private notification settings"
    $NotifyDir = Join-Path $HOME ".config\memory-supervisor"
    New-Item -ItemType Directory -Force -Path $NotifyDir | Out-Null
    $NotifyConfig = Join-Path $NotifyDir "notifications.conf"
    if (-not (Test-Path -LiteralPath $NotifyConfig)) {
        Copy-Item (Join-Path $Root "notify\notifications.conf.example") $NotifyConfig
    }
    if (-not (Get-Command curl.exe -ErrorAction SilentlyContinue)) {
        Write-Warning "Install curl before enabling Discord or Telegram notifications"
    }

    Write-Host "[7/7] verify connections"
    & $InstalledBinary status --connections
    if ($LASTEXITCODE -ne 0) { throw "Connection smoke check failed" }
    $Success = $true
    if ($PoweredOff) {
        Write-Host "OK. Memory Supervisor remains OFF across updates and restarts; run 'memory-supervisor on' once to restore protection."
    } else {
        Write-Host "OK. Open a new terminal for PATH changes. Run memory-status --connections. If user action is reported, Codex CLI uses /hooks, Codex App uses Settings -> Hooks, and interactive Claude requires workspace trust before settings-file hooks run."
    }
} catch {
    if ($Cutover -and -not $Success -and -not $Activated) {
        Restore-PreviousRuntime
    } elseif ($Activated -and -not $Success) {
        Write-Warning "Rust runtime is active, but post-activation setup was incomplete; rerun install.ps1"
    }
    throw
} finally {
    Remove-Item -Recurse -Force -LiteralPath $Temporary -ErrorAction SilentlyContinue
}
