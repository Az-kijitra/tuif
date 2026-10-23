# tuif

[![CI](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml/badge.svg)](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`tuif` は、Windows PowerShell 環境で軽快に動作する **Rust + Ratatui** 製のキーボード主体 (Vim-like) CUI/TUI ファイラーです。

ファイル操作・ツリーナビゲーション・ZIP 圧縮展開・訪問履歴・ファジー検索・内蔵テキストビューア・SFTP リモート接続をサポートし、外部 CUI/GUI ツール連携や PowerShell のカレントディレクトリ追従 (`cd on exit`)、Windows エクスプローラーとのクリップボード連携に特化しています。

---

## 主な特徴

- **Vim-like キーバインド**: `h`/`j`/`k`/`l` 移動、`gg`/`G` 先頭・末尾、`y` (コピー/Yank)、`x` (切り取り/Cut)、`p` (貼り付け/Paste)、`d`/`Delete` (ごみ箱削除)、`D`/`Shift+Delete` (完全削除)、`r` (リネーム)、`a`/`A` (新規ファイル/フォルダ作成)、`/` (インクリメンタル検索: 部分一致・正規表現)。
- **Windows エクスプローラー連携**: `CF_HDROP` クリップボード形式にネイティブ対応。エクスプローラーでコピー/切り取りしたファイルを `tuif` で貼り付け、また `tuif` でヤンク/カットしたファイルをエクスプローラーで貼り付け可能。
- **内蔵テキスト / Markdown ビューア**: `Enter` でテキスト・Markdown ファイルをフルスクリーン TUI ビューアで即座に閲覧。ビューア内検索（`/`, `n`, `N`）、Markdown プレビュー (`leaf`) と生テキスト (`bat`) の切替に対応（UTF-8 / UTF-16 / Shift_JIS 自動判別）。
- **ファジー検索・外部ツール連携**:
  - `Ctrl+P` / `Ctrl+F`: ファイル名ファジー検索 (`fzf` + `rg`)
  - `Ctrl+G` / `Shift+F`: 全文検索 (`fzf` + `rg`)
  - `Alt+C` / `f`: サブフォルダへ一発ジャンプ (`fzf`)
  - `Ctrl+Alt+C`: PowerShell (`pwsh`) を開く
  - `Ctrl+Alt+V`: Neovim (`nvim`) で開く
  - `Ctrl+Alt+G`: Lazygit (`lazygit`) で開く
  - `Ctrl+Alt+B`: bat で開く
- **ZIP 作成・展開**: `z` (圧縮) / `Z` (展開)。CP932 (Shift_JIS) エントリ名に対応。
- **リモート接続 (SFTP)**: `Ctrl+S` で接続マネージャーを開き、リモートサーバーのファイル操作・アップロード・ダウンロードをシームレスに実行。
- **PowerShell `cd on exit` 連携**: `--choosedir <FILE>` オプションにより、`tuif` 終了時のフォルダへ PowerShell のカレントディレクトリをそのまま移動可能。

---

## インストールとビルド

### 必要要件
- Windows 10 / 11
- Rust 1.75 以上 (`cargo`)

### ビルド・インストール
```powershell
# クローンしてリリースビルド
git clone https://github.com/Az-kijitra/tuif.git
cd tuif
cargo build --release

# または PATH 上へインストール
cargo install --path .
```

---

## PowerShell プロファイル連携 (`cd on exit`)

`$PROFILE` に以下のラッパー関数を追加すると、`tf` コマンドで `tuif` を起動し、終了時に移動先のフォルダへ自動で `Set-Location` されます。

```powershell
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
Set-Alias -Name tf -Value Invoke-Tuif
```

---

## 設定ファイル

設定ファイルは `%APPDATA%\tuif\config.toml` に配置します（自動生成されます）。
外観テーマ（デフォルト、Nord、Monokai、Catppuccin、Solarized Dark、Light）、レイアウト、キーマップをカスタマイズできます。

---

## ドキュメント

- [ユーザーマニュアル](docs/user_manual.md)
- [仕様台帳](specs/_registry.md)
- [トレーサビリティ台帳](docs/architecture/traceability.md)

---

## ライセンス

[MIT License](LICENSE)
