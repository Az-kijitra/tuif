<#
.SYNOPSIS
    Automated uninstaller for tuif.
.DESCRIPTION
    Removes tuif.exe from $env:LOCALAPPDATA\Programs\tuif, cleans the User PATH,
    and removes `tf` (Invoke-Tuif) from PowerShell $PROFILE.
    The configuration folder (%APPDATA%\tuif) is kept unless -RemoveConfig is given.
.EXAMPLE
    & "$env:LOCALAPPDATA\Programs\tuif\uninstall.ps1"
.EXAMPLE
    irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/uninstall.ps1 | iex
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\tuif",
    [switch]$RemoveConfig
)

# Run inside a script block so `irm | iex` leaves nothing behind in the caller's session.
& {
    $ErrorActionPreference = "Stop"

    Write-Host "=== tuif Uninstaller ===" -ForegroundColor Cyan

    if (Get-Process -Name tuif -ErrorAction SilentlyContinue) {
        Write-Warning "tuif is running. Close all tuif windows first, then run the uninstaller again."
        return
    }

    # 1. Remove only the files the installer created, then the folder if it is empty.
    #    (Guards against deleting an unrelated folder passed via -InstallDir.)
    if (Test-Path $InstallDir) {
        foreach ($name in @("tuif.exe", "uninstall.ps1")) {
            $p = Join-Path $InstallDir $name
            if (Test-Path $p) { Remove-Item -Path $p -Force }
        }
        if (-not (Get-ChildItem -Path $InstallDir -Force)) {
            Remove-Item -Path $InstallDir -Force
            Write-Host "[1/3] Removed installation folder: $InstallDir" -ForegroundColor Green
        } else {
            Write-Host "[1/3] Removed tuif files; kept $InstallDir because it contains other files." -ForegroundColor Green
        }
    } else {
        Write-Host "[1/3] Installation folder not found: $InstallDir" -ForegroundColor Gray
    }

    # 2. User PATH: edit the raw registry value so other entries stay unexpanded (REG_EXPAND_SZ).
    Write-Host "[2/3] Cleaning User PATH..." -ForegroundColor Green
    $EnvKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    try {
        $RawPath = [string]$EnvKey.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $Entries = @($RawPath -split ";" | Where-Object { $_.Trim() })
        $Kept = @($Entries | Where-Object {
            [Environment]::ExpandEnvironmentVariables($_).TrimEnd("\") -ine $InstallDir.TrimEnd("\")
        })
        if ($Kept.Count -ne $Entries.Count) {
            $EnvKey.SetValue("Path", ($Kept -join ";"), [Microsoft.Win32.RegistryValueKind]::ExpandString)
            [Environment]::SetEnvironmentVariable("TUIF_INSTALL_TMP", "1", "User")
            [Environment]::SetEnvironmentVariable("TUIF_INSTALL_TMP", $null, "User")
            Write-Host "      Removed $InstallDir from User PATH." -ForegroundColor Gray
        } else {
            Write-Host "      $InstallDir was not in User PATH." -ForegroundColor Gray
        }
    } finally {
        $EnvKey.Close()
    }

    # 3. $PROFILE
    Write-Host "[3/3] Cleaning PowerShell `$PROFILE..." -ForegroundColor Green
    $Pattern = "\r?\n?# --- tuif \(Invoke-Tuif / tf\) Integration ---[\s\S]*?# -------------------------------------------\r?\n?"
    if (Test-Path $PROFILE) {
        $Content = Get-Content -Path $PROFILE -Raw
        if ($Content -and $Content -match $Pattern) {
            Set-Content -Path $PROFILE -Value ($Content -replace $Pattern, "").TrimEnd() -Encoding UTF8
            Write-Host "      Removed the tf function from $PROFILE." -ForegroundColor Gray
        } else {
            Write-Host "      No tuif block found in $PROFILE." -ForegroundColor Gray
        }
    }
    Remove-Item -Path Alias:\tf -Force -ErrorAction SilentlyContinue
    Remove-Item -Path Function:\Invoke-Tuif -Force -ErrorAction SilentlyContinue

    $ConfigDir = Join-Path $env:APPDATA "tuif"
    if ($RemoveConfig) {
        if (Test-Path $ConfigDir) {
            Remove-Item -Path $ConfigDir -Recurse -Force
            Write-Host "      Removed config directory: $ConfigDir" -ForegroundColor Yellow
        }
    } elseif (Test-Path $ConfigDir) {
        Write-Host "      Kept settings in $ConfigDir (use -RemoveConfig to delete them)." -ForegroundColor DarkGray
    }

    Write-Host ""
    Write-Host "=== tuif uninstallation complete ===" -ForegroundColor Cyan
}
