# `tuif` ユーザーマニュアル (0.1.0)

`tuif` は、Windows PowerShell 環境で動作するキーボード主体 (Vim-like) の CUI/TUI ファイラーです。

## 1. 前提・連携ソフトウェアのインストール方法

`tuif` は単体でも動作しますが、外部 CUI ツール連携および内蔵ビューアのシンタックスハイライト・Markdown プレビュー機能を最大限活用するため、以下のツールのインストールを推奨します。

PowerShell 上で以下のコマンドを実行してインストールできます。

| ツール | 用途 | インストールコマンド (PowerShell) |
|---|---|---|
| **PowerShell 7 (`pwsh`)** | サブシェル起動 (`Ctrl+Alt+C`) | `winget install Microsoft.PowerShell` |
| **Neovim (`nvim`)** | エディタ起動 (`Ctrl+Alt+V`) | `winget install Neovim.Neovim` |
| **Lazygit (`lazygit`)** | Git クライアント起動 (`Ctrl+Alt+G`) | `winget install JesseDuffield.lazygit` |
| **bat (`bat`)** | 内蔵ビューアのシンタックスハイライト / ページャ表示 (`Ctrl+Alt+B`) | `winget install sharkdp.bat` |
| **leaf (`leaf`)** | 内蔵ビューアの Markdown プレビュー表示 | `irm https://leaf.rivolink.mg/install.ps1 \| iex` |

> **補足**: インストール後、`PATH` 環境変数を反映させるために PowerShell を再起動してください（`leaf` は `%LOCALAPPDATA%\Programs\leaf\leaf.exe` にインストールされた場合、`PATH` 再読込前でも `tuif` が自動検出します）。

---

## 2. 設定フォルダー

設定ファイルは `%APPDATA%\cuifiler` に保存されます。

| ファイル | 内容 |
|---|---|
| `config.toml` | キー割り当て (`[input_keymap_custom]`)、テーマ (`ui_theme`) |
| `navigation.toml` | 訪問履歴とジャンプリスト |
| `ui-state.toml` | ツリー・隠しファイル・サイズ列・更新日時列の表示状態、ソート条件 |

---

## 3. 画面構成と基本操作

画面は上から **メニューバー (`tuif  File (Alt+F)  Edit (Alt+E)  View (Alt+V)  Go (Alt+G)`)**、**パスバー (および検索バー)**、**メイン領域 (Drives / Tree ペイン＋マルチカラム Files ペイン)**、**ステータスバー** の4段で構成されます。

- **マルチカラム表示**: ファイル一覧はターミナルの縦幅に合わせて1列あたりの行数が自動調整され、収まらない項目は右側の列へ折り返して表示されます。
- **親フォルダ移動時のカーソル保持**: 下位フォルダから上位フォルダへ移動した際、直前まで入っていた下位フォルダの位置にカーソルが保持されます。

### 3.1 既定のキー割り当て (Vim-like)

| 操作 | 既定のキー | アクションID (`config.toml`) |
|---|---|---|
| 上へ移動 | `k` または `↑` | `move_up` |
| 下へ移動 | `j` または `↓` | `move_down` |
| 左へ移動 (一覧:左の列へ / ツリー:折畳・親フォルダへ) | `h` または `←` | `move_left` |
| 右へ移動 (一覧:右の列へ / ツリー:展開・子フォルダへ) | `l` または `→` | `move_right` |
| 先頭へ移動 | `gg` または `Home` | `move_top` |
| 末尾へ移動 | `G` (`Shift+G`) または `End` | `move_bottom` |
| 前のページ (1列分上へ) | `Ctrl+U` または `PageUp` | `page_up` |
| 次のページ (1列分下へ) | `Ctrl+D` または `PageDown` | `page_down` |
| ツリー⇔一覧フォーカス切替 | `Tab` | (固定) |
| 選択切替 (ツリーでは展開/折畳) | `Space` または `v` | `select_toggle` |
| すべて選択 | `V` (`Shift+V`) または `Ctrl+A` | `select_all` |
| 選択解除 / 検索クリア | `Esc` | `clear_selection` |
| 開く (フォルダ移動 / 内蔵ビューア) | `Enter` | `open` |
| 親フォルダーへ | `Backspace` または `-` | `go_parent` |
| パス欄へ直接入力 | `Ctrl+L` | `focus_path` |
| 検索フィルタ | `/` (入力中に `Ctrl+R` で正規表現切替) | `search` |
| コピー (Yank) | `y` または `Ctrl+C` | `copy` |
| 切り取り (Cut) | `x` または `Ctrl+X` | `cut` |
| 貼り付け (Paste) | `p` または `Ctrl+V` | `paste` |
| ごみ箱へ削除 | `d` または `Delete` | `delete` |
| 完全削除 | `D` (`Shift+D`) または `Shift+Delete` | (`delete` + Shift) |
| 名前の変更 | `r` または `F2` | `rename` |
| 新規ファイル作成 | `a` または `Ctrl+N` | `new_file` |
| 新規フォルダ作成 | `A` (`Shift+A`) または `Ctrl+Shift+N` | `new_folder` |
| ZIPに圧縮 | `z` または `Ctrl+Alt+Z` | `zip_create` |
| ZIPを展開 | `Z` (`Shift+Z`) または `Ctrl+Alt+X` | `zip_extract` |
| 並び替えメニュー | `s` | `sort_menu` |
| 訪問履歴 | `Ctrl+Y` | `history` |
| ジャンプリスト | `Ctrl+J` | `history_jump_list` |
| ジャンプリストに追加 | `m` または `Ctrl+Shift+J` | `jump_add` |
| ツリー表示切替 | `t` または `Ctrl+Shift+B` | `toggle_tree` |
| 隠しファイル表示切替 | `.` または `Ctrl+H` | `toggle_hidden` |
| サイズ列の表示切替 | `Ctrl+Shift+S` | `toggle_size` |
| 更新日時列の表示切替 | `Ctrl+Shift+T` | `toggle_time` |
| アクションメニュー | `;` または `F9` | `action_menu` |
| メニューバーを開く | `F10` または `Alt+F` / `Alt+E` / `Alt+V` / `Alt+G` | (固定) |
| 終了 | `q` または `Ctrl+Q` | `exit` |

### 3.2 マウス操作 (Windows エクスプローラー互換)

| 場所 | マウス操作 | 動作 |
|---|---|---|
| **ファイル一覧 (`Files`)** | 左クリック | クリックした項目にカーソル移動（他の複数選択は解除） |
| **ファイル一覧 (`Files`)** | `Ctrl` + 左クリック | クリックした項目の選択状態をトグル（複数選択） |
| **ファイル一覧 (`Files`)** | `Shift` + 左クリック | 直前のカーソル位置からクリック位置までを範囲選択 |
| **ファイル一覧 (`Files`)** | ダブルクリック | フォルダへ移動 / ファイルを内蔵ビューアで開く |
| **ファイル一覧 (`Files`)** | 右クリック | 対象項目にフォーカスを合わせ、**アクションメニュー**（開く・コピー・切り取り・貼り付け・リネーム・削除・ZIP圧縮/展開・外部CUI/GUIアプリ起動）を表示 |
| **ファイル一覧 / ツリー / ビューア** | マウスホイール上下 | カーソル移動 / スクロール |
| **ドライブ・ツリー (`Drives / Tree`)** | `▸` / `▾` を左クリック | フォルダの展開・折畳を切り替え |
| **ドライブ・ツリー (`Drives / Tree`)** | フォルダ名を左クリック | そのフォルダを開く（ダブルクリックで展開・折畳も切替） |
| **ドライブ・ツリー (`Drives / Tree`)** | 右クリック | 対象ツリーフォルダにフォーカスを合わせ、**アクションメニュー**を表示（Files でディレクトリを右クリックしたときと同じ挙動） |
| **メニューバー / パスバー** | 左クリック | 各メニュー (`File` / `Edit` / `View` / `Go`) の展開、またはパス直接入力欄を開く |
| **ポップアップメニュー内** | マウスホバー / 左クリック | 項目のハイライト移動 / クリックした機能を実行（枠外クリックで閉じる） |

---

## 4. 内蔵ビューア (テキスト・ソースコード・Markdown)

ファイル一覧でテキストファイルや Markdown ファイルにカーソルを合わせて `Enter` を押すと、フルスクリーンの内蔵ビューアが開きます（UTF-8 / UTF-16LE / UTF-16BE / Shift_JIS を自動判別）。

- **シンタックスハイライト (`bat` 連携)**: `bat` がインストールされている場合、言語に応じたシンタックスハイライトおよび行番号付きで表示されます。
- **Markdown プレビュー (`leaf` 連携)**: `.md` / `.markdown` ファイルを開いた場合、既定で `leaf --inline` による整形済み Markdown プレビュー (`Viewer [Preview: leaf]`) が表示されます。ビューア内で `p` または `Tab`（または `m`）を押すと、`bat` による生テキスト表示 (`Viewer [Raw: bat]`) と即座に切り替えられます。

### 4.1 内蔵ビューア内のキー操作

| 操作 | キー |
|---|---|
| 1行スクロール (下 / 上) | `j` / `k` または `↓` / `↑` |
| ページスクロール (下 / 上) | `Ctrl+D` / `Ctrl+U` または `PageDown` / `PageUp` |
| 先頭 / 末尾へ移動 | `g` (または `Home`) / `G` (または `End`) |
| 水平スクロール (左 / 右) | `h` / `l` または `←` / `→` |
| Markdown プレビュー ⇔ Raw 表示切替 (`.md` ファイルのみ) | `p` または `Tab` または `m` |
| ビューアを閉じる | `q` または `Esc` |

---

## 5. 外部 CUI / GUI ツール連携

既定では以下の6つの外部ツールが登録されています（右クリックのアクションメニューやショートカットキーから起動可能）。
- **`[CUI]` ツール**: 起動時に TUI を一時サスペンドしてターミナルを引き渡し、ツール終了後に元の `tuif` 画面へ復帰します。
- **`[GUI]` ツール**: `tuif` の画面を維持したままバックグラウンド（非ブロッキング）で別ウィンドウとして起動します。

1. **`[CUI]` Open PowerShell Here (`Ctrl+Alt+C`)**: `pwsh.exe -NoLogo -WorkingDirectory {dir}`
2. **`[CUI]` Edit with Neovim (`Ctrl+Alt+V`)**: `nvim.exe {path}`
3. **`[CUI]` Git UI (lazygit) (`Ctrl+Alt+G`)**: `lazygit.exe -p {dir}`
4. **`[CUI]` View with bat (`Ctrl+Alt+B`)**: `bat.exe --paging=always --color=always {path}`
5. **`[GUI]` Open VSCode (`Ctrl+Alt+O`)**: `Code.exe {path}`
6. **`[GUI]` Open Explorer (`Ctrl+Alt+E`)**: フォルダ選択時は `explorer.exe {dir}`、ファイル選択時は `explorer.exe /select,{path}`

---

## 6. PowerShell カレントディレクトリ追従 (`cd on exit`)

PowerShell の `$PROFILE` に以下のラッパー関数を追加すると、`tf` コマンドで `tuif` を起動し、`q` で終了した際に最後に開いていたフォルダへ PowerShell のカレントディレクトリが自動で移動します。

```powershell
function Invoke-Tuif {
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        & "C:\Users\toshi\git\tuif\target\release\tuif.exe" --choosedir $tmp @args
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

## 7. SFTP リモート接続・ファイル管理 (WinSCP ライク機能)

`tuif` は、Linux サーバーなどのリモートホストと SFTP 経由で直接接続し、既存の使い慣れたキーバインド・UI のままファイル転送や遠隔ファイル操作が行える機能を備えています。

### 7.1 接続プロファイル管理 (`Ctrl+S`)

`Ctrl+S` を押すか、メニューバーの **`Go (Alt+G)` -> `SFTP Connections...`** を選択すると、**SFTP 接続マネージャー**が開きます。

| キー | 動作 |
|---|---|
| `j` / `k` (または `↓` / `↑`) | プロファイルの選択 |
| `a` または `n` | 新規プロファイルの追加 |
| `e` | 選択中プロファイルの編集 |
| `d` | 選択中プロファイルの削除 (Windows 資格情報マネージャーのパスワードも同時に削除) |
| `Enter` | 選択中プロファイルへ接続 |
| `Esc` または `q` | 接続マネージャーを閉じる |

#### プロファイル設定項目
- **Profile Name**: 一覧やツリーに表示する任意の表示名
- **Host / IP**: 接続先サーバーのホスト名または IP アドレス
- **Port**: SSH/SFTP ポート番号 (既定: 22)
- **Username**: ログインユーザー名
- **Password**: ログインパスワード (手入力。保存しない場合は空欄で接続時にプロンプト入力)
- **Save Password**: Windows 資格情報マネージャー (`Windows Credential Manager`) にパスワードを暗号化保存するかどうかの切り替え (`Space` キーでトグル)
- **Initial Dir**: 接続時に開く初期ディレクトリ (空欄時はサーバーのホームディレクトリ)

> **セキュリティについて**: パスワードを保存する場合、平文の設定ファイルには一切書き込まれず、Windows OS 標準の **Windows 資格情報マネージャー** 内にセキュアに保管されます。

### 7.2 ホスト鍵の確認 (TOFU: Trust on First Use)

初めて接続するサーバーの場合、ホスト鍵の SHA256 フィンガープリント確認ダイアログが表示されます。`y` または `Enter` を押すと、`%APPDATA%\cuifiler\known_hosts.toml` に安全に登録され、次回以降の接続時に自動検証されます（未知の鍵変更時は警告ブロックされます）。

### 7.3 Drives / Tree ペインでのリモートツリー表示

接続が完了すると、左側の **`Drives / Tree`** ペインに `☁ [SFTP] <プロファイル名>` のリモートツリーノードが追加されます。
- `l` キーまたはマウスクリックで展開・折畳が可能で、サーバー上のディレクトリ階層をローカルドライブと同様にシームレスに行き来できます。
- パスバーには `sftp://<user>@<host>/<path>` 形式でリモートパスが表示されます。

### 7.4 ローカル ⇔ リモート間のファイル転送・遠隔操作

ローカル同士のファイル操作と全く同じ操作感で、シームレスなファイル転送や遠隔ファイル管理が行えます。

- **アップロード (Local → Remote)**: ローカル側でファイルやフォルダを選択して `y` (コピー) または `x` (切り取り) を押し、リモートディレクトリに移動して `p` (貼り付け) を押します。
- **ダウンロード (Remote → Local)**: リモート側でファイルやフォルダを選択して `y` (コピー) または `x` (切り取り) を押し、ローカルフォルダへ移動して `p` (貼り付け) を押します。
- **リモート上でのファイル操作**:
  - `a` (`Ctrl+N`): リモート上に新規ファイルを作成
  - `A` (`Ctrl+Shift+N`): リモート上に新規フォルダを作成
  - `r` (`F2`): リモートファイル・フォルダの名前変更
  - `d` (`Delete`): リモートファイル・フォルダの削除
- **リモートファイルの内蔵ビューア表示**: リモート上のテキストファイルや Markdown ファイルで `Enter` を押すと、一時フォルダへ自動ダウンロードされ、シンタックスハイライト付き内蔵ビューア（または `leaf` による Markdown プレビュー）で即座に閲覧できます。

