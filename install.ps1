<#
.SYNOPSIS
    Automated installer for tuif (Windows TUI File Manager).
.DESCRIPTION
    Installs tuif.exe to $env:LOCALAPPDATA\Programs\tuif, configures the User PATH,
    registers the `tf` (cd on exit) alias in PowerShell $PROFILE, and initializes configuration.
.EXAMPLE
    # Remote one-liner execution:
    irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/install.ps1 | iex

    # Local execution from repository:
    .\install.ps1
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\tuif"
)

$ErrorActionPreference = "Stop"

Write-Host "=== tuif Installer ===" -ForegroundColor Cyan

# 1. Target directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TargetExe = Join-Path $InstallDir "tuif.exe"

# 2. Copy local release binary if present, or download from GitHub Releases
$LocalReleaseExe = ""
if ($PSScriptRoot) {
    $LocalReleaseExe = Join-Path $PSScriptRoot "target\release\tuif.exe"
}

if ($LocalReleaseExe -and (Test-Path $LocalReleaseExe)) {
    Write-Host "[1/4] Copying local binary from $LocalReleaseExe..." -ForegroundColor Green
    Copy-Item -Path $LocalReleaseExe -Destination $TargetExe -Force
} else {
    Write-Host "[1/4] Downloading latest tuif.exe from GitHub Releases..." -ForegroundColor Green
    $DownloadUrl = "https://github.com/Az-kijitra/tuif/releases/latest/download/tuif.exe"
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $TargetExe -UseBasicParsing
    } catch {
        Write-Error "Failed to download tuif.exe from $DownloadUrl. Error: $_"
        return
    }
}

if (-not (Test-Path $TargetExe)) {
    Write-Error "tuif.exe could not be installed to $TargetExe."
    return
}
Write-Host "      Installed: $TargetExe" -ForegroundColor Gray

# 3. Add to User PATH
Write-Host "[2/4] Configuring User PATH..." -ForegroundColor Green
$UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
if (-not $UserPath) { $UserPath = "" }
$PathEntries = ($UserPath -split ";") | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
if ($PathEntries -notcontains $InstallDir) {
    $NewUserPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, [EnvironmentVariableTarget]::User)
    Write-Host "      Added $InstallDir to User PATH." -ForegroundColor Gray
} else {
    Write-Host "      $InstallDir is already in User PATH." -ForegroundColor Gray
}

# Update current session PATH so tuif is immediately usable
if (($env:Path -split ";") -notcontains $InstallDir) {
    $env:Path = "$env:Path;$InstallDir"
}

# 4. Initialize config file
Write-Host "[3/4] Initializing configuration (%APPDATA%\tuif)..." -ForegroundColor Green
try {
    $ConfigDir = Join-Path $env:APPDATA "tuif"
    if (-not (Test-Path $ConfigDir)) {
        New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
    }
    $ConfigFile = Join-Path $ConfigDir "config.toml"
    if (-not (Test-Path $ConfigFile)) {
        $Template = @'
# tuif configuration file (%APPDATA%\tuif\config.toml)

# UI Theme: "default", "nord", "monokai", "catppuccin", "solarized_dark", "light"
theme = "default"

[general]
confirm_delete = true

[layout]
show_tree = true
show_hidden = false
show_size = true
show_time = true

[input_keymap_custom]
# Example:
# open = "Enter"
# copy = "y,Ctrl+C"
'@
        Set-Content -Path $ConfigFile -Value $Template -Encoding UTF8
    }
    Write-Host "      Config file ready at: $ConfigFile" -ForegroundColor Gray
} catch {
    Write-Host "      Note: Could not initialize config: $_" -ForegroundColor Yellow
}

# 5. Configure PowerShell $PROFILE (tf alias with cd on exit)
Write-Host "[4/4] Setting up PowerShell `$PROFILE (`tf` alias)..." -ForegroundColor Green
$ProfileBlock = @'

# --- tuif (Invoke-Tuif / tf) Integration ---
function Invoke-Tuif {
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        tuif.exe --choosedir $tmp @args
        if (Test-Path $tmp) {
            $cwd = Get-Content -Path $tmp -Raw -Encoding UTF8
            if (-not [string]::IsNullOrWhiteSpace($cwd) -and (Test-Path $cwd)) {
                Set-Location -LiteralPath $cwd.Trim()
            }
        }
    } finally {
        Remove-Item -Path $tmp -Force -ErrorAction SilentlyContinue
    }
}
Set-Alias -Name tf -Value Invoke-Tuif -ErrorAction SilentlyContinue
# -------------------------------------------
'@

$ProfileDir = Split-Path -Parent $PROFILE
if (-not (Test-Path $ProfileDir)) {
    New-Item -ItemType Directory -Path $ProfileDir -Force | Out-Null
}
if (-not (Test-Path $PROFILE)) {
    New-Item -ItemType File -Path $PROFILE -Force | Out-Null
}

$ProfileContent = Get-Content -Path $PROFILE -Raw -ErrorAction SilentlyContinue
if ($ProfileContent -notmatch "Invoke-Tuif") {
    Add-Content -Path $PROFILE -Value "`n$ProfileBlock" -Encoding UTF8
    Write-Host "      Added `Invoke-Tuif` and `tf` alias to $PROFILE." -ForegroundColor Gray
} else {
    Write-Host "      `tf` alias already configured in $PROFILE." -ForegroundColor Gray
}

# Apply to current session
Invoke-Expression $ProfileBlock

Write-Host ""
Write-Host "=== Installation complete! ===" -ForegroundColor Cyan
Write-Host "Available commands:" -ForegroundColor White
Write-Host "  tf                 Launch tuif with automatic folder following (cd on exit)" -ForegroundColor Yellow
Write-Host "  tuif               Launch tuif standalone" -ForegroundColor Yellow
Write-Host "  tuif --config      Open config.toml in your text editor" -ForegroundColor Yellow
Write-Host "  tuif --config-dir  Open %APPDATA%\tuif in Explorer" -ForegroundColor Yellow
Write-Host ""
