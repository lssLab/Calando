& {
    $ErrorActionPreference = "Stop"
    $ReleaseBaseUrl = if ($env:MEMORY_SUPERVISOR_RELEASE_BASE_URL) {
        $env:MEMORY_SUPERVISOR_RELEASE_BASE_URL.TrimEnd("/")
    } else {
        "https://github.com/lssLab/Calando/releases/latest/download"
    }
    $Source = if ($env:MEMORY_SUPERVISOR_INSTALL_ROOT) {
        $env:MEMORY_SUPERVISOR_INSTALL_ROOT
    } else {
        Join-Path $env:LOCALAPPDATA "MemorySupervisor"
    }
    $Source = [IO.Path]::GetFullPath($Source)
    $SourceMarker = ".memory-supervisor-release-source"

    if ($Source -eq [IO.Path]::GetPathRoot($Source) -or
        $Source.TrimEnd("\") -eq [IO.Path]::GetFullPath($HOME).TrimEnd("\")) {
        throw "Refusing unsafe install root: $Source"
    }

    # A manually cloned development checkout keeps its normal Git update path. The public one-line
    # installer below never asks a user to install Git.
    if (Test-Path -LiteralPath (Join-Path $Source ".git") -PathType Container) {
        $Git = Get-Command git -ErrorAction SilentlyContinue
        if (-not $Git) { throw "This existing development checkout needs Git to update" }
        & $Git.Source -C $Source pull --ff-only
        if ($LASTEXITCODE -ne 0) { throw "Memory Supervisor source update failed" }
        & (Join-Path $Source "install.ps1")
        return
    }

    if (Test-Path -LiteralPath $Source) {
        $Item = Get-Item -Force -LiteralPath $Source
        if ($Item.LinkType) { throw "Refusing to replace symlink install root: $Source" }
        if (-not (Test-Path -LiteralPath (Join-Path $Source $SourceMarker) -PathType Leaf)) {
            throw "Refusing to replace a directory not owned by the release installer: $Source"
        }
    }

    $Temporary = Join-Path ([IO.Path]::GetTempPath()) `
        ("memory-supervisor-bootstrap-" + [guid]::NewGuid().ToString("N"))
    $Archive = Join-Path $Temporary "memory-supervisor-source.zip"
    $Checksum = "$Archive.sha256"
    $Backup = $null
    $SourceReplaced = $false
    $Success = $false
    New-Item -ItemType Directory -Force -Path $Temporary | Out-Null

    try {
        if ($env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE) {
            if (-not (Test-Path -LiteralPath $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE -PathType Leaf)) {
                throw "MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE is not a file"
            }
            if (-not (Test-Path -LiteralPath $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE -PathType Leaf)) {
                throw "MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE is not a file"
            }
            Copy-Item -LiteralPath $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE -Destination $Archive
            Copy-Item -LiteralPath $env:MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE -Destination $Checksum
        } else {
            Invoke-WebRequest -UseBasicParsing `
                -Uri "$ReleaseBaseUrl/memory-supervisor-source.zip" -OutFile $Archive
            Invoke-WebRequest -UseBasicParsing `
                -Uri "$ReleaseBaseUrl/memory-supervisor-source.zip.sha256" -OutFile $Checksum
        }

        $Expected = ((Get-Content -LiteralPath $Checksum -TotalCount 1) -split '\s+')[0].ToLowerInvariant()
        $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Archive).Hash.ToLowerInvariant()
        if (-not $Expected -or $Actual -ne $Expected) {
            throw "Release source checksum verification failed"
        }

        $Extracted = Join-Path $Temporary "extracted"
        Expand-Archive -LiteralPath $Archive -DestinationPath $Extracted -Force
        $Candidates = @(Get-ChildItem -Force -LiteralPath $Extracted -Directory)
        if ($Candidates.Count -ne 1 -or
            -not (Test-Path -LiteralPath (Join-Path $Candidates[0].FullName "install.ps1") -PathType Leaf)) {
            throw "Release source archive has an invalid layout"
        }

        New-Item -ItemType Directory -Force -Path (Split-Path $Source) | Out-Null
        if (Test-Path -LiteralPath $Source) {
            $Backup = "$Source.bootstrap-backup-$([guid]::NewGuid().ToString('N'))"
            Move-Item -LiteralPath $Source -Destination $Backup
        }
        Move-Item -LiteralPath $Candidates[0].FullName -Destination $Source
        $SourceReplaced = $true
        [IO.File]::WriteAllText(
            (Join-Path $Source $SourceMarker),
            $ReleaseBaseUrl + "`n",
            [Text.UTF8Encoding]::new($false)
        )

        & (Join-Path $Source "install.ps1")
        $Success = $true
    }
    catch {
        if ($SourceReplaced) {
            Remove-Item -Recurse -Force -LiteralPath $Source -ErrorAction SilentlyContinue
            if ($Backup -and (Test-Path -LiteralPath $Backup)) {
                Move-Item -LiteralPath $Backup -Destination $Source
            }
        }
        throw
    }
    finally {
        Remove-Item -Recurse -Force -LiteralPath $Temporary -ErrorAction SilentlyContinue
        if ($Success -and $Backup) {
            Remove-Item -Recurse -Force -LiteralPath $Backup -ErrorAction SilentlyContinue
        }
    }
}
