# README のスクリーンショット撮影ガイド

README に載せる画像（`docs/images/`）の撮り方です。撮り直すときも、この手順に従うと見た目がそろいます。

## 撮る画像

| ファイル | 種類 | 内容 |
|---|---|---|
| `hero.gif` | GIF（10〜15 秒） | README の先頭に置くデモ。`tf` で起動 → 移動 → ビューア → 終了してそのフォルダへ cd |
| `main.png` | PNG | メイン画面（ツリーとファイル一覧、アイコン） |
| `viewer.png` | PNG | 内蔵ビューアで Markdown（またはソースコード）を表示 |
| `fzf.png` | PNG | `Ctrl+P` のファジー検索 |
| `sftp.png` | PNG | SFTP 接続中の画面（撮れない場合は省略。README から外します） |
| `explorer-clipboard.gif` | GIF（10 秒前後） | エクスプローラーと tuif の間でのコピー＆ペースト |
| `themes.png` | PNG（合成） | 6 種類のテーマを並べた画像 |

## 1. 準備

### 1.1 ターミナル

- **Windows Terminal** を使います。
- フォントは Nerd Font（例: `CaskaydiaCove Nerd Font`、`JetBrainsMono Nerd Font`）、サイズ 12〜14。
- ウィンドウは **横 120 桁 × 縦 32 行前後**。設定 → 起動 → 起動サイズで指定できます。撮影中はサイズを変えないでください。
- 背景の透過・アクリル効果はオフにします（GIF の色数が増え、ファイルが大きくなるため）。

### 1.2 設定のバックアップ

テーマの切り替えなどで `config.toml` を書き換えるので、先にバックアップします。

```powershell
Copy-Item "$env:APPDATA\tuif\config.toml" "$env:APPDATA\tuif\config.toml.bak"
```

撮影が終わったら元に戻します。

```powershell
Move-Item "$env:APPDATA\tuif\config.toml.bak" "$env:APPDATA\tuif\config.toml" -Force
```

### 1.3 ダミーフォルダ

個人のファイルやユーザー名が写らないように、撮影用のフォルダを作ります。

```powershell
.\tools\New-DemoFolder.ps1             # C:\tuif-demo に作成
.\tools\New-DemoFolder.ps1 -Force      # 作り直す
```

Rust のプロジェクト、Markdown、日本語のファイル名、写真・音楽などのダミー、ZIP、隠しファイルが入ります。ファイルの更新日時もばらけさせてあります。

### 1.4 写り込みに注意するもの

- **履歴とジャンプリスト**（`Ctrl+Y` / `Ctrl+J`）には普段開いたフォルダが並ぶので、開かないでください。
- **PowerShell のプロンプト**にユーザー名が出る場合は、撮影中だけ短くします。

  ```powershell
  function prompt { "PS $($PWD.Path)> " }
  ```

- **ツリーの `C:\` 直下**（`Users` など）は写っても問題ありませんが、気になる場合はツリーを `C:\tuif-demo` の付近までスクロールしておきます。

## 2. 撮影

PNG は Windows Terminal のウィンドウだけを撮ります（`Alt+PrintScreen`、または Snipping Tool のウィンドウモード）。GIF は [ScreenToGif](https://www.screentogif.com/)（`winget install NickeManarin.ScreenToGif`）で、録画範囲を Windows Terminal に合わせて撮ります。

### hero.gif

```powershell
cd C:\tuif-demo
tf
```

1. `j` / `k` で数回移動し、`projects` → `weather-cli` に `Enter` で入る
2. `README.md` で `Enter`（ビューア）→ 少しスクロール → `q`
3. `Ctrl+Q` で終了する
4. プロンプトが `C:\tuif-demo\projects\weather-cli` になっているところで 1〜2 秒止めて終わる

キー入力は少しゆっくり（1 秒に 2〜3 回）にすると見やすくなります。

### main.png

`C:\tuif-demo` で起動し、`documents` や `photos` を少し選択（`Space`）した状態で撮ります。ツリーとアイコン、サイズ・日時の列が見えるようにします。

### viewer.png

`projects\weather-cli\README.md` を開いた状態。Markdown のプレビューには `leaf` が必要です（[ユーザーマニュアル](user_manual.md) の「前提・連携ソフトウェア」参照）。`leaf` がない場合は `src\main.rs` を開いてシンタックスハイライトの画面を撮ります（`bat` が必要）。

### fzf.png

`C:\tuif-demo` で `Ctrl+P` を押し、`main` などと入力して候補が絞り込まれた状態で撮ります（`fzf` と `rg` が必要）。

### sftp.png

SFTP サーバー（自分の Linux マシン、WSL の `sshd` など）に接続し、リモートのフォルダを開いた状態で撮ります。ホスト名やユーザー名が写るので、撮影用のアカウントやホスト名を使ってください。用意できない場合は撮らなくて構いません。

### explorer-clipboard.gif

Windows Terminal とエクスプローラーを左右に並べ、両方が入る範囲で録画します。

1. エクスプローラーで `C:\tuif-demo\photos\wallpaper.png` を `Ctrl+C`
2. tuif で `inbox` に入り `p` で貼り付け
3. tuif で `documents\notes.txt` を `y`
4. エクスプローラーで `inbox` を開いて `Ctrl+V`

### themes.png

`config.toml` の `theme` を変えながら、同じ画面（`C:\tuif-demo` のメイン画面）を 6 枚撮ります。tuif は起動時に設定を読むので、テーマを変えるたびに起動し直します。

| `theme` の値 | 保存するファイル名 |
|---|---|
| `"default"` | `theme-default.png` |
| `"nord"` | `theme-nord.png` |
| `"monokai"` | `theme-monokai.png` |
| `"catppuccin"` | `theme-catppuccin.png` |
| `"solarized-dark"` | `theme-solarized-dark.png` |
| `"light"` | `theme-light.png` |

撮った 6 枚を 1 枚にまとめます。

```powershell
$names = 'default', 'nord', 'monokai', 'catppuccin', 'solarized-dark', 'light'
.\tools\Join-Screenshots.ps1 -Path ($names | ForEach-Object { "theme-$_.png" }) `
    -Captions $names -OutFile docs\images\themes.png
```

## 3. 仕上げ

- PNG は幅 1200〜1600 px 程度。大きすぎる場合は縮小します。
- GIF は 1 ファイル 5 MB 以下を目安にします。ScreenToGif の保存時に、フレームレート 10〜15 fps、色数の削減（「Quantizer: Octree」など）で小さくできます。
- できた画像を `docs/images/` に置き、README を表示して確認します。

```powershell
Get-ChildItem docs\images | Select-Object Name, @{ n = 'KB'; e = { [int]($_.Length / 1KB) } }
```
