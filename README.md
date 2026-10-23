# tuif

[![CI](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml/badge.svg)](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`tuif` は、Windows PowerShell 環境で軽快に動作する **Rust + Ratatui** 製のキーボード主体 (Vim-like) CUI/TUI ファイラーです。

ファイル操作・ツリーナビゲーション・ZIP 圧縮展開・訪問履歴・ファジー検索・内蔵テキストビューア・SFTP リモート接続をサポートし、外部 CUI/GUI ツール連携や PowerShell のカレントディレクトリ追従 (`cd on exit`)、Windows エクスプローラーとのクリップボード連携に特化しています。

---

## クイックインストール (推奨)

PowerShell を開き、以下の **1行を実行するだけ** でセットアップが完了します。  
`tuif.exe` の配置、ユーザー環境変数 PATH の設定、PowerShell `$PROFILE` への `tf` (`cd on exit`) コマンド登録をすべて全自動で行います。

```powershell
irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/install.ps1 | iex
```

インストール完了後、ターミナルで `tf` または `tuif` と入力すればすぐに起動できます。

<details>
<summary>ソースから手動でビルド・インストールする場合</summary>

### 必要要件
- Windows 10 / 11
- Rust 1.75 以上 (`cargo`)

```powershell
# クローンしてビルド
git clone https://github.com/Az-kijitra/tuif.git
cd tuif
cargo build --release

# PATH 上へインストールする場合
cargo install --path .
```
</details>

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
- **PowerShell `cd on exit` 連携**: `tf` コマンドで起動すると、`tuif` 終了時に移動先フォルダへ PowerShell のカレントディレクトリがそのまま追従移動します。

---

## 設定ファイル（手軽なカスタマイズ）

設定ファイル（`config.toml`）を探す手間をなくすため、以下のコマンドで一発で開けます：

```powershell
# 設定ファイル (config.toml) を既定のテキストエディタで直接開く
tuif --config

# 設定フォルダ (%APPDATA%\tuif) をエクスプローラーで開く
tuif --config-dir
```

外観テーマ（デフォルト、Nord、Monokai、Catppuccin、Solarized Dark、Light）、レイアウト、キーマップを自由にカスタマイズ可能です。

---

## ドキュメント

- [ユーザーマニュアル](docs/user_manual.md)
- [仕様台帳](specs/_registry.md)
- [トレーサビリティ台帳](docs/architecture/traceability.md)

---

## アンインストール

アンインストールしたい場合は、以下を実行するだけで簡単に元通りにクリーンアップできます：

```powershell
irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/uninstall.ps1 | iex
```

---

## ライセンス

[MIT License](LICENSE)
