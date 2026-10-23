<#
.SYNOPSIS
    Automated uninstaller for tuif.
.DESCRIPTION
    Removes tuif.exe from $env:LOCALAPPDATA\Programs\tuif, cleans User PATH,
    and removes `tf` (Invoke-Tuif) from PowerShell $PROFILE.
.EXAMPLE
    .\uninstall.ps1
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\tuif",
    [switch]$RemoveConfig
)

$ErrorActionPreference = "Stop"

Write-Host "=== tuif Uninstaller ===" -ForegroundColor Cyan

# 1. Remove installation directory
if (Test-Path $InstallDir) {
    Remove-Item -Path $InstallDir -Recurse -Force
    Write-Host "[1/3] Removed installation folder: $InstallDir" -ForegroundColor Green
} else {
    Write-Host "[1/3] Installation folder not found: $InstallDir" -ForegroundColor Gray
}

# 2. Clean User PATH
Write-Host "[2/3] Cleaning User PATH..." -ForegroundColor Green
$UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
if ($UserPath) {
    $PathEntries = ($UserPath -split ";") | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and $_ -ne $InstallDir
    }
    $NewUserPath = ($PathEntries -join ";")
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, [EnvironmentVariableTarget]::User)
    Write-Host "      Removed $InstallDir from User PATH." -ForegroundColor Gray
}

# 3. Clean $PROFILE
Write-Host "[3/3] Cleaning PowerShell `$PROFILE..." -ForegroundColor Green
if (Test-Path $PROFILE) {
    $Content = Get-Content -Path $PROFILE -Raw
    if ($Content -match "# --- tuif \(Invoke-Tuif / tf\) Integration ---[\s\S]*?# -------------------------------------------") {
        $NewContent = $Content -replace "# --- tuif \(Invoke-Tuif / tf\) Integration ---[\s\S]*?# -------------------------------------------`r?`n?", ""
        Set-Content -Path $PROFILE -Value $NewContent.Trim() -Encoding UTF8
        Write-Host "      Removed `tf` function from $PROFILE." -ForegroundColor Gray
    }
}

if ($RemoveConfig) {
    $ConfigDir = Join-Path $env:APPDATA "tuif"
    if (Test-Path $ConfigDir) {
        Remove-Item -Path $ConfigDir -Recurse -Force
        Write-Host "      Removed config directory: $ConfigDir" -ForegroundColor Yellow
    }
}

Write-Host ""
Write-Host "=== tuif uninstallation complete ===" -ForegroundColor Cyan
