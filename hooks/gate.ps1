param([Parameter(Position=0, ValueFromRemainingArguments=$true)][string[]]$GateArguments)

if (-not $GateArguments -or $GateArguments.Count -eq 0) { exit 0 }

# Stable fail-open wrapper. This path also keeps hooks cached by an active session working.
$Binary = $env:MEMORY_SUPERVISOR_BINARY
if (-not $Binary) {
    $Pointer = Join-Path $HOME ".memory-supervisor\binary"
    if (Test-Path -LiteralPath $Pointer) {
        $Binary = ([IO.File]::ReadAllText($Pointer)).Trim()
    }
}
if (-not $Binary) {
    $Binary = Join-Path $HOME ".local\lib\memory-supervisor\memory-supervisor.exe"
}
if (-not (Test-Path -LiteralPath $Binary)) { exit 0 }
$output = & $Binary gate @GateArguments 2>$null
if ($LASTEXITCODE -eq 0 -and $null -ne $output) {
    [Console]::Out.Write(($output -join [Environment]::NewLine))
}
exit 0
