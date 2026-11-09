<#
.SYNOPSIS
    Creates a folder of realistic-looking dummy files for README screenshots.

.DESCRIPTION
    Builds a small "workspace" (a Rust project, documents, photos, music, an archive,
    Japanese file names, a hidden file) with varied sizes and timestamps, so screenshots
    show icons, columns and the viewer without exposing any personal files.

    Nothing outside -Path is touched. An existing -Path is refused unless -Force is given,
    and -Force only deletes it when it contains the marker file this script writes.

.PARAMETER Path
    Where to create the demo folder. Defaults to C:\tuif-demo so the path bar does not
    show your user name.

.PARAMETER Force
    Recreate -Path if it was created by this script before.

.EXAMPLE
    .\tools\New-DemoFolder.ps1
    .\tools\New-DemoFolder.ps1 -Path D:\demo -Force
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Path = 'C:\tuif-demo',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$marker = '.tuif-demo'

if (Test-Path -LiteralPath $Path) {
    if (-not $Force) {
        throw "$Path already exists. Use -Force to recreate it (only if this script created it)."
    }
    if (-not (Test-Path -LiteralPath (Join-Path $Path $marker))) {
        throw "$Path was not created by this script (no $marker marker); refusing to delete it."
    }
    if ($PSCmdlet.ShouldProcess($Path, 'Remove existing demo folder')) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
$rng = New-Object System.Random 42
$now = Get-Date

function New-DemoFile {
    param(
        [Parameter(Mandatory)][string]$RelativePath,
        [string]$Text,
        [long]$Bytes = 0,
        [double]$DaysAgo = 0
    )
    $full = Join-Path $Path $RelativePath
    $dir = Split-Path -Parent $full
    if (-not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    if ($PSBoundParameters.ContainsKey('Text')) {
        [System.IO.File]::WriteAllText($full, $Text, $utf8)
    }
    else {
        $buffer = New-Object byte[] $Bytes
        $rng.NextBytes($buffer)
        [System.IO.File]::WriteAllBytes($full, $buffer)
    }
    (Get-Item -LiteralPath $full -Force).LastWriteTime = $now.AddDays(-$DaysAgo).AddMinutes(-$rng.Next(0, 600))
}

if (-not $PSCmdlet.ShouldProcess($Path, 'Create demo folder')) { return }
New-Item -ItemType Directory -Path $Path -Force | Out-Null
New-DemoFile -RelativePath $marker -Text "Created by tools/New-DemoFolder.ps1`n"
(Get-Item -LiteralPath (Join-Path $Path $marker) -Force).Attributes = 'Hidden'

# --- A small Rust project -------------------------------------------------------------
New-DemoFile 'projects\weather-cli\Cargo.toml' -DaysAgo 12 -Text @'
[package]
name = "weather-cli"
version = "0.3.1"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
'@

New-DemoFile 'projects\weather-cli\src\main.rs' -DaysAgo 1 -Text @'
use clap::Parser;

/// Show the weather forecast for a city.
#[derive(Parser)]
struct Args {
    /// City name, e.g. "Tokyo"
    city: String,
    /// Number of days to show
    #[arg(short, long, default_value_t = 3)]
    days: u8,
}

fn main() {
    let args = Args::parse();
    let forecast = fetch_forecast(&args.city, args.days);
    for (day, summary) in forecast.iter().enumerate() {
        println!("Day {}: {}", day + 1, summary);
    }
}

fn fetch_forecast(city: &str, days: u8) -> Vec<String> {
    (0..days)
        .map(|d| format!("{city}: sunny, {}°C", 18 + d as i32))
        .collect()
}
'@

New-DemoFile 'projects\weather-cli\src\api.rs' -DaysAgo 3 -Text @'
pub struct Client {
    base_url: String,
}

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into() }
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}
'@

New-DemoFile 'projects\weather-cli\README.md' -DaysAgo 2 -Text @'
# weather-cli

A tiny command-line tool that prints the **weather forecast** for a city.

## Usage

```powershell
weather-cli Tokyo --days 5
```

## Features

- Forecast for up to 7 days
- Celsius and Fahrenheit
- Works offline with cached data

| Option   | Default | Description            |
|----------|---------|------------------------|
| `--days` | `3`     | Number of days to show |
| `--unit` | `c`     | `c` or `f`             |

> Data is provided for demonstration purposes only.
'@

New-DemoFile 'projects\weather-cli\.gitignore' -DaysAgo 30 -Text "/target`n"
New-DemoFile 'projects\homepage\index.html' -DaysAgo 40 -Text "<!doctype html>`n<title>Home</title>`n<h1>Hello</h1>`n"
New-DemoFile 'projects\homepage\style.css' -DaysAgo 40 -Text "body { font-family: sans-serif; }`n"
New-DemoFile 'projects\homepage\app.js' -DaysAgo 38 -Text "document.querySelector('h1').textContent = 'Hello, tuif';`n"
New-DemoFile 'projects\scripts\backup.ps1' -DaysAgo 9 -Text "Copy-Item -Path .\docs -Destination D:\backup -Recurse`n"
New-DemoFile 'projects\scripts\report.py' -DaysAgo 15 -Text "import csv`n`nwith open('sales.csv') as f:`n    print(sum(1 for _ in csv.reader(f)))`n"

# --- Documents (including Japanese names) ---------------------------------------------
New-DemoFile 'documents\議事録_2026-11.md' -DaysAgo 4 -Text @'
# 定例会議 議事録

- 日時: 2026-11-04 10:00
- 参加者: 佐藤、鈴木、高橋

## 決定事項

1. リリースは 11 月第 3 週
2. ドキュメントを日本語と英語で用意する
'@
New-DemoFile 'documents\見積書.pdf' -Bytes 184320 -DaysAgo 20
New-DemoFile 'documents\presentation.pptx' -Bytes 2457600 -DaysAgo 7
New-DemoFile 'documents\budget-2026.xlsx' -Bytes 48210 -DaysAgo 11
New-DemoFile 'documents\sales.csv' -DaysAgo 5 -Text "month,region,amount`n2026-09,East,1200`n2026-10,East,1350`n2026-10,West,980`n"
New-DemoFile 'documents\notes.txt' -DaysAgo 0.2 -Text "TODO`n- Update screenshots`n- Publish the release notes`n"
New-DemoFile 'documents\settings.json' -DaysAgo 6 -Text "{`n  `"theme`": `"dark`",`n  `"fontSize`": 14`n}`n"

# --- Media ------------------------------------------------------------------------------
New-DemoFile 'photos\2026-10-kyoto\IMG_0412.jpg' -Bytes 3145728 -DaysAgo 25
New-DemoFile 'photos\2026-10-kyoto\IMG_0413.jpg' -Bytes 2867200 -DaysAgo 25
New-DemoFile 'photos\2026-10-kyoto\IMG_0420.png' -Bytes 1048576 -DaysAgo 24
New-DemoFile 'photos\wallpaper.png' -Bytes 5242880 -DaysAgo 60
New-DemoFile 'music\track01.mp3' -Bytes 4718592 -DaysAgo 90
New-DemoFile 'music\track02.flac' -Bytes 26214400 -DaysAgo 90
New-DemoFile 'videos\demo.mp4' -Bytes 15728640 -DaysAgo 14

# --- Top-level files --------------------------------------------------------------------
New-DemoFile 'README.md' -DaysAgo 1 -Text "# Demo workspace`n`nDummy files for tuif screenshots.`n"
New-DemoFile 'todo.md' -DaysAgo 0.5 -Text "- [x] Install tuif`n- [ ] Try the fuzzy finder (Ctrl+P)`n"
New-DemoFile 'setup.exe' -Bytes 7340032 -DaysAgo 45
New-DemoFile '.env' -DaysAgo 10 -Text "API_URL=https://example.com`n"
(Get-Item -LiteralPath (Join-Path $Path '.env') -Force).Attributes = 'Hidden'

# An archive made from the homepage project
Compress-Archive -Path (Join-Path $Path 'projects\homepage\*') -DestinationPath (Join-Path $Path 'homepage-backup.zip') -Force
(Get-Item -LiteralPath (Join-Path $Path 'homepage-backup.zip')).LastWriteTime = $now.AddDays(-35)

# An empty folder for the "new folder" / paste demos
New-Item -ItemType Directory -Path (Join-Path $Path 'inbox') -Force | Out-Null
(Get-Item -LiteralPath (Join-Path $Path 'inbox')).LastWriteTime = $now.AddDays(-3)

# Folders take the time of their newest entry (deepest first), instead of "just now".
Get-ChildItem -LiteralPath $Path -Directory -Recurse |
    Sort-Object { $_.FullName.Length } -Descending |
    ForEach-Object {
        $newest = Get-ChildItem -LiteralPath $_.FullName -Force |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($newest) { $_.LastWriteTime = $newest.LastWriteTime }
    }

Write-Host "Created demo folder: $Path" -ForegroundColor Green
Write-Host "Start tuif there with:  tuif `"$Path`"" -ForegroundColor Gray
