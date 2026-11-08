<#
.SYNOPSIS
    Packages tuif into a minimal, self-contained end-user ZIP release archive.
.DESCRIPTION
    Builds the release binary (optional), stages minimal runtime files
    (tuif.exe, install.ps1, uninstall.ps1, LICENSE, README.txt),
    compresses into dist/tuif-v<version>-windows-x86_64.zip,
    and generates a SHA256 checksum.
.EXAMPLE
    .\package.ps1
    .\package.ps1 -NoBuild
#>

[CmdletBinding()]
param(
    [string]$Version,
    [string]$OutputDir = (Join-Path $PSScriptRoot "dist"),
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

Write-Host "=== tuif Release Packaging ===" -ForegroundColor Cyan

# 1. Determine version from Cargo.toml if not specified
if (-not $Version) {
    $CargoTomlPath = Join-Path $PSScriptRoot "Cargo.toml"
    if (Test-Path $CargoTomlPath) {
        $CargoContent = Get-Content -Path $CargoTomlPath -Raw
        if ($CargoContent -match '(?m)^version\s*=\s*"([^"]+)"') {
            $Version = $matches[1]
        }
    }
}

if (-not $Version) {
    throw "Could not read the version from Cargo.toml. Pass -Version explicitly."
}

Write-Host "Target Version: v$Version" -ForegroundColor Yellow

# 2. Build release binary if not skipped
$ExePath = Join-Path $PSScriptRoot "target\release\tuif.exe"
if (-not $NoBuild) {
    Write-Host "[1/4] Building release binary (cargo build --release)..." -ForegroundColor Green
    Push-Location $PSScriptRoot
    try {
        & cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo build failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $ExePath)) {
    throw "Release binary not found at $ExePath. Run without -NoBuild to compile first."
}

# 3. Setup output and staging directories
Write-Host "[2/4] Staging release files..." -ForegroundColor Green
$ArchiveBase = "tuif-v$Version-windows-x86_64"
$StagingDir = Join-Path $OutputDir $ArchiveBase
$ZipPath = Join-Path $OutputDir "$ArchiveBase.zip"
$ShaPath = "$ZipPath.sha256"

if (Test-Path $StagingDir) {
    Remove-Item -Path $StagingDir -Recurse -Force
}
if (Test-Path $ZipPath) {
    Remove-Item -Path $ZipPath -Force
}
if (Test-Path $ShaPath) {
    Remove-Item -Path $ShaPath -Force
}

New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null

# 4. Copy required files
Copy-Item -Path $ExePath -Destination (Join-Path $StagingDir "tuif.exe") -Force
Copy-Item -Path (Join-Path $PSScriptRoot "install.ps1") -Destination (Join-Path $StagingDir "install.ps1") -Force
Copy-Item -Path (Join-Path $PSScriptRoot "uninstall.ps1") -Destination (Join-Path $StagingDir "uninstall.ps1") -Force

$LicensePath = Join-Path $PSScriptRoot "LICENSE"
if (Test-Path $LicensePath) {
    Copy-Item -Path $LicensePath -Destination (Join-Path $StagingDir "LICENSE") -Force
}

# 5. Create concise end-user README.txt
$ReadmeContent = @"
================================================================================
tuif - Keyboard-driven CUI/TUI File Manager for Windows
Version: v$Version
Website: https://github.com/Az-kijitra/tuif
================================================================================

[インストール方法 / Installation]
1. このZIPファイルを任意のフォルダに解凍します。
2. PowerShell を開き、解凍先フォルダで以下を実行します:
     .\install.ps1
   (または install.ps1 を右クリックして「PowerShell で実行」)

   ※ インストーラーが行うこと:
     - %LOCALAPPDATA%\Programs\tuif に tuif.exe を配置
     - ユーザー環境変数 PATH に追加
     - PowerShell `$PROFILE に ``tf`` エイリアスを登録 (終了時にフォルダ追従)
     - %APPDATA%\tuif\config.toml を生成

[使い方 / Basic Usage]
  tf                 tuif を起動（終了時に最後に開いていたディレクトリに移動）
  tuif               tuif を単体起動（移動なし）
  tuif --config      設定ファイル (config.toml) をメモ帳などのエディタで開く
  tuif --config-dir  設定フォルダ (%APPDATA%\tuif) をエクスプローラーで開く

[主なキー操作 / Keybindings]
  h / j / k / l      カーソル移動 (← ↓ ↑ →)
  Enter              フォルダを開く / ファイルを内蔵ビューアで表示
  o                  Windows の既定のアプリで開く
  Backspace          親フォルダへ
  Space              選択マーク切り替え
  /                  絞り込み検索
  y / x / p          コピー / 切り取り / 貼り付け (エクスプローラーと相互にコピペ可能)
  d                  削除 (ごみ箱へ移動)
  ;  または F9       アクションメニュー
  F10                メニューバー
  q                  終了 (Ctrl+Q: 終了してそのフォルダへ cd)

[アンインストール / Uninstallation]
  PowerShell で以下を実行します:
     .\uninstall.ps1

[ドキュメント / Documentation]
  操作マニュアルや設定方法の詳細は GitHub リポジトリをご覧ください:
  https://github.com/Az-kijitra/tuif/blob/master/docs/user_manual.md
================================================================================
"@
Set-Content -Path (Join-Path $StagingDir "README.txt") -Value $ReadmeContent -Encoding UTF8

# 6. Compress staging folder
Write-Host "[3/4] Creating ZIP archive ($ArchiveBase.zip)..." -ForegroundColor Green
Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath -Force

# 7. Generate SHA256 checksum
Write-Host "[4/4] Calculating SHA256 checksum..." -ForegroundColor Green
$Hash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
Set-Content -Path $ShaPath -Value "$Hash  $ArchiveBase.zip" -Encoding Ascii

# Standalone binary + checksum for the `irm | iex` installer (it verifies tuif.exe.sha256)
$DistExe = Join-Path $OutputDir "tuif.exe"
Copy-Item -Path $ExePath -Destination $DistExe -Force
$ExeHash = (Get-FileHash -Path $DistExe -Algorithm SHA256).Hash.ToLower()
Set-Content -Path "$DistExe.sha256" -Value "$ExeHash  tuif.exe" -Encoding Ascii

# Clean up staging directory
Remove-Item -Path $StagingDir -Recurse -Force

# Summary
$ZipItem = Get-Item $ZipPath
$SizeKb = [Math]::Round($ZipItem.Length / 1KB, 1)
$SizeMb = [Math]::Round($ZipItem.Length / 1MB, 2)

Write-Host ""
Write-Host "=== Packaging Complete! ===" -ForegroundColor Cyan
Write-Host "Archive:  $ZipPath" -ForegroundColor White
Write-Host "Size:     $SizeMb MB ($SizeKb KB)" -ForegroundColor White
Write-Host "SHA256:   $Hash" -ForegroundColor White
Write-Host "Checksum: $ShaPath" -ForegroundColor White
