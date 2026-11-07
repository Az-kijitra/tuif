<#
.SYNOPSIS
    Automated installer for tuif (Windows TUI File Manager).
.DESCRIPTION
    Installs tuif.exe to $env:LOCALAPPDATA\Programs\tuif, configures the User PATH,
    registers the `tf` (cd on exit) function in PowerShell $PROFILE, and initializes configuration.
    No administrator rights are required.
.EXAMPLE
    # Remote one-liner execution:
    irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/install.ps1 | iex

    # Local execution from an extracted release ZIP or the repository:
    .\install.ps1
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\tuif"
)

# Everything runs inside a script block so that `irm | iex` does not leave variables or
# a changed $ErrorActionPreference behind in the caller's session.
& {
    $ErrorActionPreference = "Stop"
    $Repo = "Az-kijitra/tuif"
    $ReleaseBase = "https://github.com/$Repo/releases/latest/download"
    $RawBase = "https://raw.githubusercontent.com/$Repo/master"

    Write-Host "=== tuif Installer ===" -ForegroundColor Cyan

    # Older Windows PowerShell 5.1 defaults to TLS 1.0. Enable TLS 1.2 (and 1.3 when the runtime knows it).
    try {
        $proto = [Net.SecurityProtocolType]::Tls12
        if ([Enum]::GetNames([Net.SecurityProtocolType]) -contains "Tls13") {
            $proto = $proto -bor [Net.SecurityProtocolType]::Tls13
        }
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor $proto
    } catch { }

    function Get-Remote([string]$Url, [string]$OutFile) {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    }

    # 1. Target directory
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    $TargetExe = Join-Path $InstallDir "tuif.exe"
    $TargetUninstall = Join-Path $InstallDir "uninstall.ps1"

    # 2. Binary: copy a local one (release ZIP / repo build) or download the latest release
    $LocalExe = $null
    if ($PSScriptRoot) {
        foreach ($candidate in @("tuif.exe", "target\release\tuif.exe")) {
            $p = Join-Path $PSScriptRoot $candidate
            if (Test-Path $p) { $LocalExe = $p; break }
        }
    }

    if (Get-Process -Name tuif -ErrorAction SilentlyContinue) {
        Write-Warning "tuif is running. Close all tuif windows first, then run the installer again."
        return
    }

    if ($LocalExe) {
        Write-Host "[1/4] Copying local binary from $LocalExe..." -ForegroundColor Green
        Copy-Item -Path $LocalExe -Destination $TargetExe -Force
    } else {
        Write-Host "[1/4] Downloading latest tuif.exe from GitHub Releases..." -ForegroundColor Green
        $TmpExe = Join-Path ([IO.Path]::GetTempPath()) ("tuif-" + [guid]::NewGuid() + ".exe")
        try {
            Get-Remote "$ReleaseBase/tuif.exe" $TmpExe

            # Verify the SHA256 checksum published next to the binary.
            $TmpSha = "$TmpExe.sha256"
            $expected = $null
            try {
                Get-Remote "$ReleaseBase/tuif.exe.sha256" $TmpSha
                $m = [regex]::Match((Get-Content -Path $TmpSha -Raw), "[0-9a-fA-F]{64}")
                if ($m.Success) { $expected = $m.Value.ToLowerInvariant() }
            } catch { } finally {
                Remove-Item -Path $TmpSha -Force -ErrorAction SilentlyContinue
            }
            $actual = (Get-FileHash -Path $TmpExe -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($expected) {
                if ($actual -ne $expected) {
                    throw "SHA256 mismatch for tuif.exe (expected $expected, got $actual). Aborting."
                }
                Write-Host "      SHA256 verified: $actual" -ForegroundColor Gray
            } else {
                Write-Warning "This release has no tuif.exe.sha256; skipping checksum verification (SHA256: $actual)."
            }
            Move-Item -Path $TmpExe -Destination $TargetExe -Force
        } catch {
            Remove-Item -Path $TmpExe -Force -ErrorAction SilentlyContinue
            Write-Error "Failed to install tuif.exe: $_"
            return
        }
    }
    Write-Host "      Installed: $TargetExe" -ForegroundColor Gray

    # uninstall.ps1 goes next to the binary so `& "$env:LOCALAPPDATA\Programs\tuif\uninstall.ps1"` works
    $LocalUninstall = if ($PSScriptRoot) { Join-Path $PSScriptRoot "uninstall.ps1" } else { $null }
    if ($LocalUninstall -and (Test-Path $LocalUninstall)) {
        Copy-Item -Path $LocalUninstall -Destination $TargetUninstall -Force
    } else {
        try {
            Get-Remote "$ReleaseBase/uninstall.ps1" $TargetUninstall
        } catch {
            try {
                Get-Remote "$RawBase/uninstall.ps1" $TargetUninstall
            } catch {
                Write-Warning "Could not download uninstall.ps1: $_"
            }
        }
    }

    # 3. User PATH. Read and write the raw registry value so entries such as
    #    %USERPROFILE%\bin stay unexpanded (REG_EXPAND_SZ).
    Write-Host "[2/4] Configuring User PATH..." -ForegroundColor Green
    $EnvKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    try {
        $RawPath = [string]$EnvKey.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $Entries = @($RawPath -split ";" | Where-Object { $_.Trim() })
        $AlreadyThere = $Entries | Where-Object {
            [Environment]::ExpandEnvironmentVariables($_).TrimEnd("\") -ieq $InstallDir.TrimEnd("\")
        }
        if (-not $AlreadyThere) {
            $NewPath = (@($Entries) + $InstallDir) -join ";"
            $EnvKey.SetValue("Path", $NewPath, [Microsoft.Win32.RegistryValueKind]::ExpandString)
            # Broadcast WM_SETTINGCHANGE so new terminals pick up the change.
            [Environment]::SetEnvironmentVariable("TUIF_INSTALL_TMP", "1", "User")
            [Environment]::SetEnvironmentVariable("TUIF_INSTALL_TMP", $null, "User")
            Write-Host "      Added $InstallDir to User PATH." -ForegroundColor Gray
        } else {
            Write-Host "      $InstallDir is already in User PATH." -ForegroundColor Gray
        }
    } finally {
        $EnvKey.Close()
    }
    if (($env:Path -split ";") -notcontains $InstallDir) {
        $env:Path = "$env:Path;$InstallDir"
    }

    # 4. Config file (the template lives inside tuif.exe; an existing file is never touched)
    Write-Host "[3/4] Initializing configuration (%APPDATA%\tuif)..." -ForegroundColor Green
    $HelpText = (& $TargetExe --help) -join "`n"
    if ($HelpText -match "--init-config") {
        & $TargetExe --init-config | ForEach-Object { Write-Host "      $_" -ForegroundColor Gray }
    } else {
        Write-Host "      Skipped (this tuif version creates config.toml on 'tuif --config')." -ForegroundColor Gray
    }

    # 5. PowerShell $PROFILE: `tf` = launch tuif and cd to its last folder on exit
    Write-Host "[4/4] Setting up PowerShell `$PROFILE (tf function)..." -ForegroundColor Green
    $ProfileBlock = @'

# --- tuif (Invoke-Tuif / tf) Integration ---
function Invoke-Tuif {
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        tuif.exe --choosedir $tmp @args
        if (Test-Path $tmp) {
            $cwd = Get-Content -Path $tmp -Raw -Encoding UTF8
            if (-not [string]::IsNullOrWhiteSpace($cwd) -and (Test-Path -LiteralPath $cwd.Trim())) {
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
        Write-Host "      Added Invoke-Tuif and the tf alias to $PROFILE." -ForegroundColor Gray
    } else {
        Write-Host "      tf is already configured in $PROFILE." -ForegroundColor Gray
    }

    # Make `tf` usable in this session too (global scope, since we run inside a script block).
    $SessionBlock = $ProfileBlock -replace "function Invoke-Tuif", "function global:Invoke-Tuif" `
        -replace "Set-Alias -Name tf", "Set-Alias -Scope Global -Name tf"
    Invoke-Expression $SessionBlock

    Write-Host ""
    Write-Host "=== Installation complete! ===" -ForegroundColor Cyan
    Write-Host "Available commands:" -ForegroundColor White
    Write-Host "  tf                 Launch tuif and cd to its last folder on exit" -ForegroundColor Yellow
    Write-Host "  tuif               Launch tuif standalone" -ForegroundColor Yellow
    Write-Host "  tuif --config      Open config.toml in your text editor" -ForegroundColor Yellow
    Write-Host "  tuif --config-dir  Open %APPDATA%\tuif in Explorer" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Uninstall: & `"$TargetUninstall`"" -ForegroundColor DarkGray
}
