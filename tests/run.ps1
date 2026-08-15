$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).ProviderPath
& cargo fmt --manifest-path (Join-Path $Root "Cargo.toml") --all -- --check
if ($LASTEXITCODE -ne 0) { throw "rustfmt failed" }
& cargo clippy --manifest-path (Join-Path $Root "Cargo.toml") --all-targets --locked -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "Clippy failed" }
& cargo test --manifest-path (Join-Path $Root "Cargo.toml") --all-targets --locked
if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
& cargo build --manifest-path (Join-Path $Root "Cargo.toml") --release --locked
if ($LASTEXITCODE -ne 0) { throw "Rust release build failed" }
$ParseErrors = @()
Get-ChildItem -Recurse -Path $Root -Filter *.ps1 | ForEach-Object {
    [Management.Automation.Language.Parser]::ParseFile($_.FullName, [ref]$null, [ref]$ParseErrors) | Out-Null
}
if ($ParseErrors.Count) { $ParseErrors | Format-List; throw "PowerShell syntax failed" }
& (Join-Path $Root "tests\bootstrap-windows.ps1")
