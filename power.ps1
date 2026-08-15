[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("on", "off")]
    [string]$Action
)

$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "packaging\power.ps1") $Action
