$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).ProviderPath
$Temporary = Join-Path ([IO.Path]::GetTempPath()) `
    ("memory-supervisor-bootstrap-test-" + [guid]::NewGuid().ToString("N"))
$InstallRoot = Join-Path $Temporary "installed"
$PreviousInstallRoot = $env:MEMORY_SUPERVISOR_INSTALL_ROOT
$PreviousArchive = $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE
$PreviousChecksum = $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE

function New-ReleaseFixture {
    param(
        [string]$Label,
        [bool]$Fails
    )
    $Fixture = Join-Path $Temporary "fixture-$Label"
    $Source = Join-Path $Fixture "memory-supervisor"
    New-Item -ItemType Directory -Force -Path $Source | Out-Null
    [IO.File]::WriteAllText((Join-Path $Source "version"), "$Label`n")
    $Failure = if ($Fails) { 'throw "fixture failure"' } else { '' }
    $Script = @"
`$ErrorActionPreference = "Stop"
[IO.File]::WriteAllText((Join-Path `$env:MEMORY_SUPERVISOR_INSTALL_ROOT "install-ran"), "$Label``n")
$Failure
"@
    [IO.File]::WriteAllText(
        (Join-Path $Source "install.ps1"),
        $Script,
        [Text.UTF8Encoding]::new($false)
    )
    $Archive = Join-Path $Temporary "$Label.zip"
    Compress-Archive -LiteralPath $Source -DestinationPath $Archive -Force
    $Checksum = "$Archive.sha256"
    $Hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Archive).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($Checksum, "$Hash  memory-supervisor-source.zip`n")
    return @($Archive, $Checksum)
}

function Invoke-FixtureBootstrap {
    param([string[]]$Fixture)
    $env:MEMORY_SUPERVISOR_INSTALL_ROOT = $InstallRoot
    $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE = $Fixture[0]
    $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE = $Fixture[1]
    & (Join-Path $Root "bootstrap.ps1")
}

try {
    New-Item -ItemType Directory -Force -Path $Temporary | Out-Null
    Invoke-FixtureBootstrap (New-ReleaseFixture "first" $false)
    if ((Get-Content -Raw -LiteralPath (Join-Path $InstallRoot "version")).Trim() -ne "first") {
        throw "First release source was not installed"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $InstallRoot ".memory-supervisor-release-source"))) {
        throw "Release ownership marker is missing"
    }

    Invoke-FixtureBootstrap (New-ReleaseFixture "second" $false)
    if ((Get-Content -Raw -LiteralPath (Join-Path $InstallRoot "version")).Trim() -ne "second") {
        throw "Second release source was not installed"
    }

    $Failed = $false
    try {
        Invoke-FixtureBootstrap (New-ReleaseFixture "broken" $true)
    } catch {
        $Failed = $true
    }
    if (-not $Failed) { throw "Broken release fixture unexpectedly succeeded" }
    if ((Get-Content -Raw -LiteralPath (Join-Path $InstallRoot "version")).Trim() -ne "second") {
        throw "Broken release did not restore the previous source"
    }
}
finally {
    $env:MEMORY_SUPERVISOR_INSTALL_ROOT = $PreviousInstallRoot
    $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE = $PreviousArchive
    $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE = $PreviousChecksum
    Remove-Item -Recurse -Force -LiteralPath $Temporary -ErrorAction SilentlyContinue
}
