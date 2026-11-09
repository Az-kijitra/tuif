<div align="center">

# tuif

**PowerShell のための、Windows ネイティブな TUI ファイラー**

*A keyboard-driven TUI file manager built for Windows & PowerShell — in Rust.*

[![CI](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml/badge.svg)](https://github.com/Az-kijitra/tuif/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Az-kijitra/tuif)](https://github.com/Az-kijitra/tuif/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6?logo=windows)](#インストール)
[![PowerShell](https://img.shields.io/badge/shell-PowerShell%205.1%20%7C%207-5391FE?logo=powershell&logoColor=white)](#powershell-との統合)
[![Rust](https://img.shields.io/badge/built%20with-Rust%20%2B%20Ratatui-B7410E?logo=rust)](https://ratatui.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

---

## なぜ tuif？

ターミナル用ファイラーの多くは Unix で生まれ、Windows は「一応動く」扱いです。パスの区切り、ドライブレター、ごみ箱、Shift_JIS のファイル名、エクスプローラーとのコピペといった Windows ならではの部分で引っかかりがちです。

**tuif は最初から Windows と PowerShell のために作っています。**

| | Windows / PowerShell 向けの作り込み |
|---|---|
| 📋 **エクスプローラーとコピペ** | `CF_HDROP` クリップボードに対応しています。エクスプローラーで `Ctrl+C` したファイルを tuif で `p`、tuif で `y` したファイルをエクスプローラーで `Ctrl+V`。 |
| 🗑️ **ごみ箱** | `d` で Windows のごみ箱へ移動します。完全削除は `D` だけ。 |
| 💾 **ドライブ** | `C:\` `D:\` などのドライブがツリーのルートに並びます。 |
| 🇯🇵 **日本語環境** | ビューアは UTF-8 / UTF-16 / Shift_JIS を自動判別します。ZIP は CP932 のエントリ名を文字化けせずに展開します。 |
| 🐚 **PowerShell の cd on exit** | `tf` で起動して `Ctrl+Q` で終了すると、そのとき開いていたフォルダへ PowerShell がそのまま `cd` します（`q` なら移動しません）。設定はインストーラーが `$PROFILE` に書き込みます。 |
| ⚡ **導入はワンライナー** | `irm ... \| iex` の 1 行で PATH・`$PROFILE`・設定ファイルまで準備します。管理者権限は不要です。 |
| 📦 **単体の exe** | 実行時の依存はありません。Rust 製のネイティブバイナリなので起動が速いです。 |

---

## インストール

PowerShell で次の 1 行を実行します。

```powershell
irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/install.ps1 | iex
```

完了したら `tf` または `tuif` で起動できます。

インストーラーは次の処理を行います（管理者権限は不要です）。

- `%LOCALAPPDATA%\Programs\tuif` に `tuif.exe` を配置する
- ユーザー環境変数 `PATH` に追加する
- PowerShell の `$PROFILE` に `tf`（cd on exit）関数を登録する
- `%APPDATA%\tuif\config.toml` を生成する

<details>
<summary>ZIP から手動でインストールする</summary>

[Releases](https://github.com/Az-kijitra/tuif/releases) から `tuif-v*-windows-x86_64.zip` をダウンロードして解凍し、中にある `install.ps1` を実行します。
</details>

<details>
<summary>ソースからビルドする（Rust 1.75 以上）</summary>

```powershell
git clone https://github.com/Az-kijitra/tuif.git
cd tuif
cargo install --path .
```
</details>

<details>
<summary>アンインストール</summary>

```powershell
irm https://raw.githubusercontent.com/Az-kijitra/tuif/master/uninstall.ps1 | iex
```

PATH と `$PROFILE` の登録も削除します。
</details>

### あると便利な外部ツール

tuif は単体で動きます。次のツールを入れると、ファジー検索やシンタックスハイライトなどが使えるようになります。

```powershell
winget install junegunn.fzf            # ファジー検索
winget install BurntSushi.ripgrep.MSVC # 全文検索
winget install sharkdp.bat             # ビューアのシンタックスハイライト
```

Markdown のプレビュー表示には `leaf` を使います（導入方法は [ユーザーマニュアル](docs/user_manual.md) を参照）。

アイコンを表示するには [Nerd Font](https://www.nerdfonts.com/) をターミナルのフォントに設定してください（使わない場合は `config.toml` で `icons = "unicode"` または `"ascii"` に変更します）。

---

## 主な機能

- **Vim 風のキー操作**: `h` `j` `k` `l` で移動、`gg` / `G` で先頭 / 末尾、`/` でインクリメンタル検索（正規表現にも切り替え可能）。
- **マウス操作**: クリック、`Ctrl` / `Shift` + クリックでの複数選択、ダブルクリック、右クリックメニュー、ホイール。
- **ファイル操作**: `y` コピー / `x` 切り取り / `p` 貼り付け / `r` リネーム / `a` 新規ファイル / `A` 新規フォルダ。同名ファイルがあれば衝突解決ダイアログを表示し、進捗バーから中断もできます。
- **内蔵ビューア**: `Enter` でテキスト / ソースコード / Markdown を表示します。ビューア内でも検索できます（`/` `n` `N`）。
- **ZIP**: `z` で圧縮、`Z` で展開。
- **SFTP**: `Ctrl+S` で接続マネージャーを開きます。ホスト鍵は初回接続時に確認して保存し（TOFU）、パスワードは Windows 資格情報マネージャーに保存します。
- **外部ツール連携**: fzf / ripgrep / bat / Neovim / Lazygit / VS Code / pwsh を起動できます。任意のアプリを `config.toml` でキーに割り当てることもできます。
- **履歴とジャンプリスト**: `Ctrl+Y` で訪問履歴、`m` / `Ctrl+J` でブックマーク。
- **カスタマイズ**: テーマ 6 種、色の個別上書き、キーマップの変更、アイコン（Nerd Font / Unicode / ASCII）。

### よく使うキー

| キー | 動作 | キー | 動作 |
|---|---|---|---|
| `h` `j` `k` `l` | 移動 | `Enter` | 開く / ビューア |
| `Backspace` | 親フォルダへ | `o` | 既定のアプリで開く |
| `Space` | 選択 | `/` | 絞り込み検索 |
| `y` / `x` / `p` | コピー / 切り取り / 貼り付け | `Ctrl+P` | ファイル名ファジー検索 |
| `d` / `D` | ごみ箱へ / 完全削除 | `Ctrl+G` | 全文検索 |
| `r` | リネーム | `Ctrl+S` | SFTP 接続 |
| `z` / `Z` | ZIP 圧縮 / 展開 | `;` | アクションメニュー |
| `q` | 終了 | `Ctrl+Q` | 終了してそのフォルダへ cd |

すべてのキーは [ユーザーマニュアル](docs/user_manual.md#31-既定のキー割り当て-vim-like) にあります。

---

## PowerShell との統合

`tf` は、インストーラーが `$PROFILE` に登録する小さな関数です。

```powershell
PS C:\Users\me> tf          # tuif でフォルダを移動して Ctrl+Q で終了すると…
PS D:\work\project>         # PowerShell もそのフォルダに移動している
```

tuif の中からは `Ctrl+Alt+C` で pwsh を開けるので、tuif とシェルを行き来しながら作業できます。

---

## 設定

```powershell
tuif --config       # config.toml を既定のエディタで開く
tuif --config-dir   # %APPDATA%\tuif をエクスプローラーで開く
```

```toml
theme = "catppuccin"

[layout]
icons = "nerd"            # "nerd" | "unicode" | "ascii" | "none"

[keymap]
copy = "y, Ctrl+C"
```

設定項目の一覧は [ユーザーマニュアル](docs/user_manual.md) にあります。

---

## よくある質問

<details>
<summary>「Windows によって PC が保護されました」と表示される</summary>

`tuif.exe` にはコード署名をしていないため、ブラウザでダウンロードした ZIP から実行すると SmartScreen の警告が出ることがあります。リリースに添付した `.sha256` とハッシュが一致することを確認してから、「詳細情報」→「実行」を選んでください。

```powershell
Get-FileHash .\tuif.exe -Algorithm SHA256
```

ワンライナーのインストーラーはダウンロードした `tuif.exe` を `tuif.exe.sha256` と自動で照合し、一致しない場合はインストールを中止します。
</details>

<details>
<summary>ZIP の <code>install.ps1</code> が実行できない</summary>

ブラウザでダウンロードしたファイルには「インターネットから取得した」印が付くため、PowerShell の実行ポリシーで止められることがあります。解凍したフォルダで次を実行してから、もう一度 `.\install.ps1` を実行してください。

```powershell
Get-ChildItem -Recurse | Unblock-File
```
</details>

<details>
<summary>アイコンが「□」や「?」になる</summary>

ターミナルのフォントが Nerd Font ではありません。[Nerd Font](https://www.nerdfonts.com/) を入れてターミナルのフォントに設定するか、`config.toml` の `[layout]` で `icons = "unicode"`（絵文字）または `icons = "ascii"` に変更してください。
</details>

<details>
<summary><code>tf</code> が見つからない</summary>

`tf` はインストールしたウィンドウと、その後に開いた PowerShell で使えます。インストール前から開いていた別のウィンドウでは、`. $PROFILE` を実行するか開き直してください。実行ポリシーで `$PROFILE` の読み込みが止められている場合は、`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` で許可できます。
</details>

---

## ドキュメント

- [ユーザーマニュアル](docs/user_manual.md)
- [変更履歴 (CHANGELOG)](CHANGELOG.md)
- [貢献ガイド (CONTRIBUTING)](CONTRIBUTING.md): ビルド方法、PR の流れ、仕様書と開発パイプライン
- [セキュリティポリシー (SECURITY)](SECURITY.md): 脆弱性の報告方法、認証情報の扱い

## ライセンス

[MIT License](LICENSE)
