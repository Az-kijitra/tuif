# `tuif` ユーザーマニュアル (0.1.0)

`tuif` は、Windows PowerShell 環境で動作するキーボード主体 (Vim-like) かつ Windows エクスプローラー互換のマウス操作・SFTP リモート接続（WinSCP ライク機能）を備えた CUI/TUI ファイラーです。

---

## 目次

1. [前提・連携ソフトウェアのインストール方法](#1-前提連携ソフトウェアのインストール方法)
2. [設定フォルダーと構成ファイル](#2-設定フォルダーと構成ファイル)
   - [2.1 %APPDATA% の確認・アクセス方法](#21-appdata-の確認アクセス方法)
3. [画面構成と基本操作](#3-画面構成と基本操作)
   - [3.1 既定のキー割り当て (Vim-like)](#31-既定のキー割り当て-vim-like)
   - [3.2 マウス操作 (Windows エクスプローラー互換)](#32-マウス操作-windows-エクスプローラー互換)
4. [内蔵ビューア (テキスト・ソースコード・Markdown)](#4-内蔵ビューア-テキストソースコードmarkdown)
   - [4.1 内蔵ビューア内のキー操作](#41-内蔵ビューア内のキー操作)
   - [4.2 ビューア内検索の使い方](#42-ビューア内検索の使い方)
5. [外部 CUI / GUI ツール連携](#5-外部-cui--gui-ツール連携)
   - [5.1 登録済みツール一覧](#51-登録済みツール一覧)
   - [5.2 `fzf` + `ripgrep` による高速ファジー検索・全文検索](#52-fzf--ripgrep-による高速ファジー検索全文検索)
   - [5.3 外部アプリのカスタマイズ (`config.toml`)](#53-外部アプリのカスタマイズ-configtoml)
6. [【オプション】PowerShell カレントディレクトリ追従の設定方法と使い方 (`cd on exit`)](#6-オプションpowershell-カレントディレクトリ追従の設定方法と使い方-cd-on-exit)
7. [SFTP リモート接続・ファイル管理 (WinSCP ライク機能)](#7-sftp-リモート接続ファイル管理-winscp-ライク機能)
   - [7.1 接続プロファイル管理 (`Ctrl+S`)](#71-接続プロファイル管理-ctrls)
   - [7.2 ホスト鍵の確認 (TOFU: Trust on First Use)](#72-ホスト鍵の確認-tofu-trust-on-first-use)
   - [7.3 Drives / Tree ペインでのリモートツリー表示](#73-drives--tree-ペインでのリモートツリー表示)
   - [7.4 ローカル ⇔ リモート間のファイル転送・遠隔操作](#74-ローカル--リモート間のファイル転送遠隔操作)
   - [7.5 ファイル転送の進捗表示と中断 (プログレスバー & キャンセル)](#75-ファイル転送の進捗表示と中断-プログレスバー--キャンセル)
   - [7.6 同名ファイルの衝突確認ダイアログ (衝突解決)](#76-同名ファイルの衝突確認ダイアログ-衝突解決)
8. [トラブルシューティング / FAQ](#8-トラブルシューティング--faq)
9. [テーマ・配色のカスタマイズ (`config.toml`)](#9-テーマ配色のカスタマイズ-configtoml)
10. [動作オプション・レイアウト設定 (`config.toml`)](#10-動作オプションレイアウト設定-configtoml)
11. [キーマップのカスタマイズ (`[keymap]`)](#11-キーマップのカスタマイズ-keymap)
12. [設定ファイル (`config.toml`) の完全サンプル](#12-設定ファイル-configtoml-の完全サンプル)

---

## 1. 前提・連携ソフトウェアのインストール方法

`tuif` は単体でもフル機能で動作しますが、外部ツール連携や内蔵ビューアのシンタックスハイライト・Markdown プレビュー、`fzf` + `ripgrep` による高速検索機能を最大限活用するため、以下のツールのインストールを推奨します。

PowerShell 上で以下のコマンドを実行してインストールできます。

| ツール | 用途 | インストールコマンド (PowerShell) |
|---|---|---|
| **PowerShell 7 (`pwsh`)** | サブシェル起動 (`Ctrl+Alt+C`) | `winget install Microsoft.PowerShell` |
| **Neovim (`nvim`)** | エディタ起動 (`Ctrl+Alt+V`) | `winget install Neovim.Neovim` |
| **Lazygit (`lazygit`)** | Git クライアント起動 (`Ctrl+Alt+G`) | `winget install JesseDuffield.lazygit` |
| **bat (`bat`)** | 内蔵ビューア・`fzf` プレビューのシンタックスハイライト / ページャ表示 (`Ctrl+Alt+B`) | `winget install sharkdp.bat` |
| **fzf (`fzf`)** | ファイル名ファジー検索 (`Ctrl+P`) / 全文検索 (`Ctrl+G`) / フォルダジャンプ (`f`) | `winget install junegunn.fzf` |
| **ripgrep (`rg`)** | 再帰ファイル列挙およびファイル内容の超高速全文検索 (`Ctrl+P` / `Ctrl+G`) | `winget install BurntSushi.ripgrep.MSVC` |
| **VSCode (`Code.exe`)** | GUI エディタ起動 (`Ctrl+Alt+O`) | `winget install Microsoft.VisualStudioCode` |
| **leaf (`leaf`)** | 内蔵ビューアの Markdown プレビュー表示 | `irm https://leaf.rivolink.mg/install.ps1 \| iex` |

> **補足**: インストール後、`PATH` 環境変数を反映させるために PowerShell を再起動してください（`leaf` は `%LOCALAPPDATA%\Programs\leaf\leaf.exe` にインストールされた場合、`PATH` 再読込前でも `tuif` が自動検出します）。

---

## 2. 設定フォルダーと構成ファイル

設定ファイルは `%APPDATA%\tuif`（通常は `C:\Users\<ユーザー名>\AppData\Roaming\tuif`）に保存されます。

| ファイル | 内容 |
|---|---|
| `config.toml` | キー割り当て (`[keymap]` / `[input_keymap_custom]`)、テーマ・配色 (`[theme]` / `[colors]`)、動作・レイアウト (`[general]` / `[layout]`)、外部アプリ定義 (`[[external_apps]]`) |
| `navigation.toml` | 訪問履歴 (`history`) とジャンプリスト (`jump_list`) |
| `ui-state.toml` | ツリー・隠しファイル・サイズ列・更新日時列の表示状態、ソート条件 |
| `remote_profiles.toml` | SFTP 接続プロファイル一覧 (接続先ホスト、ポート、ユーザー名、初期ディレクトリ等) |
| `known_hosts.toml` | SFTP 接続先の SSH ホスト鍵 (SHA256 フィンガープリント) キャッシュ |

> **セキュリティについて**: SFTP のログインパスワードを保存する場合、平文の設定ファイルには一切書き込まれず、Windows OS 標準の **Windows 資格情報マネージャー** (`Windows Credential Manager`) 内に暗号化保管されます。

### 2.1 %APPDATA% の確認・アクセス方法

`%APPDATA%` は Windows における各ユーザーのアプリケーション設定保存先フォルダ（`AppData\Roaming`）を指す環境変数です。以下のいずれかの方法で簡単に確認および開くことができます。

1. **PowerShell でパスを確認する**:
   ```powershell
   $env:APPDATA
   # 出力例: C:\Users\ユーザー名\AppData\Roaming
   ```
2. **PowerShell から tuif 設定フォルダをエクスプローラーで直接開く**:
   ```powershell
   explorer "$env:APPDATA\tuif"
   ```
   ※ 初回起動前などでフォルダがまだ作成されていない場合は、以下のコマンドで作成して開くことができます。
   ```powershell
   New-Item -ItemType Directory -Path "$env:APPDATA\tuif" -Force; explorer "$env:APPDATA\tuif"
   ```
3. **「ファイル名を指定して実行」で開く**:
   - キーボードの `Win + R` キーを押して実行ダイアログを開きます。
   - 名前に `%APPDATA%\tuif`（または親フォルダを開く場合は `%APPDATA%`）と入力して `Enter` を押すと、直接エクスプローラーでフォルダが開きます。
4. **コマンドプロンプト (cmd.exe) で確認する**:
   ```cmd
   echo %APPDATA%
   ```

---

## 3. 画面構成と基本操作

画面は上から **メニューバー**、**パスバー (および検索バー)**、**メイン領域 (Drives / Tree ペイン＋マルチカラム Files ペイン)**、**ステータスバー** の4段で構成されます。

- **マルチカラム表示**: ファイル一覧はターミナルの縦幅に合わせて1列あたりの行数が自動調整され、収まらない項目は右側の列へ折り返して表示されます。
- **親フォルダ移動時のカーソル保持**: 下位フォルダから上位フォルダへ移動した際、直前まで入っていた下位フォルダの位置にカーソルが保持されます。

### 3.1 既定のキー割り当て (Vim-like)

| 操作 | 既定のキー | アクションID (`config.toml`) |
|---|---|---|
| 上へ移動 | `k` または `↓` | `move_up` |
| 下へ移動 | `j` または `↑` | `move_down` |
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
| Windows の既定のアプリで開く (ダブルクリック相当) | `o` または `Shift+Enter` | `open_default` |
| 親フォルダーへ | `Backspace` または `-` | `go_parent` |
| パス欄へ直接入力 | `Ctrl+L` | `focus_path` |
| 検索フィルタ (現在のフォルダ内) | `/` (入力中に `Ctrl+R` で正規表現切替) | `search` |
| 再帰ファイル名ファジー検索 (`fzf` + `rg`) | `Ctrl+P` または `Ctrl+F` | `fuzzy_find_file` |
| ファイル内容の全文検索 (`fzf` + `rg`) | `Ctrl+G` または `F` (`Shift+F`) | `fuzzy_live_grep` |
| サブフォルダへ一発ジャンプ (`fzf`) | `Alt+C` または `f` | `fuzzy_jump_dir` |
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
| SFTP 接続マネージャー | `Ctrl+S` | `connection_manager` |
| メニューバーを開く | `F10` または `Alt+F` / `Alt+E` / `Alt+V` / `Alt+G` | (固定) |
| 終了 (ディレクトリ移動せず終了) | `q` | `exit` |
| カレントディレクトリを変更して終了 (`cd on exit`) | `Ctrl+Q` | `exit_and_cd` |

### 3.2 マウス操作 (Windows エクスプローラー互換)

| 場所 | マウス操作 | 動作 |
|---|---|---|
| **ファイル一覧 (`Files`)** | 左クリック | クリックした項目にカーソル移動（他の複数選択は解除） |
| **ファイル一覧 (`Files`)** | `Ctrl` + 左クリック | クリックした項目の選択状態をトグル（複数選択） |
| **ファイル一覧 (`Files`)** | `Shift` + 左クリック | 直前のカーソル位置からクリック位置までを範囲選択 |
| **ファイル一覧 (`Files`)** | ダブルクリック | フォルダへ移動 / ファイルを内蔵ビューアで開く |
| **ファイル一覧 (`Files`)** | 右クリック | 対象項目にフォーカスを合わせ、**アクションメニュー**（開く・既定のアプリで開く・コピー・切り取り・貼り付け・リネーム・削除・ZIP圧縮/展開・外部CUI/GUIアプリ起動）を表示 |
| **ファイル一覧 / ツリー / ビューア** | マウスホイール上下 | カーソル移動 / スクロール |
| **ドライブ・ツリー (`Drives / Tree`)** | `▸` / `▾` を左クリック | フォルダの展開・折畳を切り替え |
| **ドライブ・ツリー (`Drives / Tree`)** | フォルダ名を左クリック | そのフォルダを開く（ダブルクリックで展開・折畳も切替） |
| **ドライブ・ツリー (`Drives / Tree`)** | 右クリック | 対象ツリーフォルダにフォーカスを合わせ、**アクションメニュー**を表示（Files でディレクトリを右クリックしたときと同じ挙動） |
| **リモートツリー (`☁ [SFTP] ...`)** | 左クリック / 右クリック | ローカルツリーと全く同じ操作でリモートフォルダの開閉・展開・アクションメニュー起動が可能 |
| **メニューバー / パスバー** | 左クリック | 各メニュー (`File` / `Edit` / `View` / `Go`) の展開、またはパス直接入力欄を開く |
| **ポップアップメニュー内** | マウスホバー / 左クリック | 項目のハイライト移動 / クリックした機能を実行（枠外クリックで閉じる） |

### 3.3 Windows エクスプローラーとのクリップボード連携 (コピー＆ペースト)

`tuif` は Windows 標準のファイル転送クリップボード形式（`CF_HDROP` および `Preferred DropEffect`）にネイティブ対応しています。Windows エクスプローラーやデスクトップとの間で、シームレスにファイルのコピー・切り取り・貼り付けを行えます。

#### 1. エクスプローラー → `tuif` への貼り付け
1. Windows エクスプローラーやデスクトップで任意のファイルまたはフォルダを選択し、`Ctrl+C`（コピー）または `Ctrl+X`（切り取り）を押します。
2. `tuif` 画面で貼り付け先のフォルダに移動し、`p`（または `Ctrl+V`）を押します。
3. コピーまたは移動（切り取り時）がバックグラウンドで安全に実行されます。
   - **競合処理**: 貼り付け先に同名ファイルが存在する場合、tuif 標準の衝突解決ダイアログ（上書き / スキップ / 全て上書き / キャンセル）が表示されます。
   - **進捗表示**: 転送中は画面フッターに進捗状況およびプログレスバーが表示されます（`Ctrl+C` または `Esc` で中断可能）。
   - **切り取り完了後の保護**: 移動が全件正常に完了すると、二重移動を防ぐため OS クリップボードは自動的にクリアされます。

#### 2. `tuif` → エクスプローラーへの貼り付け
1. `tuif` 上でファイルまたはフォルダにカーソルを合わせるか、`Space` キーで複数選択します。
2. `y`（または `Ctrl+C`）でコピー（Yank）、または `x`（または `Ctrl+X`）で切り取り（Cut）を実行します。
   - 内部クリップボードの更新と同時に、Windows OS クリップボードへも自動的に出力されます。
3. Windows エクスプローラーまたはデスクトップの任意のフォルダで `Ctrl+V` を押すと、tuif で選択したファイル群が貼り付けられます。

> [!NOTE]
> - SFTP リモートセッション上のファイルはローカルパスを持たないため、OS クリップボードではなく `tuif` 内部クリップボードで管理されます（エクスプローラー側での不正パスエラーを防止）。
> - エクスプローラーでコピーしたローカルファイルを `tuif` 上の SFTP ディレクトリで `p` 貼り付けすると、自動的に SFTP アップロードとして処理されます。

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
| **ファイル内検索** (プロンプト表示) | `/` または `Ctrl+F` |
| **次のマッチへ移動** (順方向) | `n` |
| **前のマッチへ移動** (逆方向) | `N` |
| 検索ハイライト解除 / ビューア終了 | `Esc` (検索時は解除、非検索時は終了) または `q` |
| Markdown プレビュー ⇔ Raw 表示切替 (`.md` ファイルのみ) | `p` または `Tab` または `m` |

### 4.2 ビューア内検索の使い方

1. ビューア表示中に `/`（または `Ctrl+F`）を押すと、画面最下部に `/検索ワード█` の入力プロンプトが現れます。
2. キーワードを入力して `Enter` を押すと検索が実行され、最初のマッチ行へスクロールジャンプします（`Esc` でキャンセル）。
3. タイトルバーに `[Match 1/5 for "keyword"]` のように現在位置と全マッチ件数が表示されます。
4. 行番号欄にはマッチ行インジケータ（通常マッチ `●`、現在フォーカス中のマッチ `▶`）が表示されます。
5. `n` で次のマッチ行、`N` で前のマッチ行へ素早くジャンプできます。
6. `Esc` を1回押すと検索状態がクリアされ、再度 `Esc` または `q` を押すとビューアを閉じます。

---

## 5. 外部 CUI / GUI ツール連携

右クリックのアクションメニューやショートカットキーから外部ツールをワンタッチで起動できます。

### 5.1 登録済みツール一覧

- **`[CUI]` ツール**: 起動時に TUI を一時サスペンドしてターミナルを引き渡し、ツール終了後に元の `tuif` 画面へ自動復帰します。
- **`[GUI]` ツール**: `tuif` の画面を維持したままバックグラウンド（非ブロッキング）で別ウィンドウとして即座に起動します。

| 種類 | ツール名 | 既定ショートカット | 起動コマンドと引数 |
|---|---|---|---|
| `[CUI]` | **Open PowerShell Here** | `Ctrl+Alt+C` | `pwsh.exe -NoLogo -WorkingDirectory {dir}` |
| `[CUI]` | **Edit with Neovim** | `Ctrl+Alt+V` | `nvim.exe {path}` |
| `[CUI]` | **Git UI (lazygit)** | `Ctrl+Alt+G` | `lazygit.exe -p {dir}` |
| `[CUI]` | **View with bat** | `Ctrl+Alt+B` | `bat.exe --paging=always --color=always {path}` |
| `[GUI]` | **Open VSCode** | `Ctrl+Alt+O` | `Code.exe {path}` |
| `[GUI]` | **Open Explorer** | `Ctrl+Alt+E` | `explorer.exe {dir}` (フォルダ選択時) / `explorer.exe /select,{path}` (ファイル選択時) |

#### Windows の既定のアプリで開く (`o` / `Shift+Enter`)

右クリックのアクションメニュー（**`Open with Default App`**）、メニューバーの **`File (Alt+F)` → `Open with Default App`**、またはショートカットキー `o` / `Shift+Enter` で、選択中の項目を **Windows に関連付けられた既定のアプリ** で開きます（エクスプローラーでのダブルクリックと同じ動作）。

- **ファイル**: 拡張子に関連付けられたアプリ（例: `.pdf` → PDF ビューア、`.xlsx` → Excel、`.png` → フォト）で開きます。関連付けが無い場合は「プログラムから開く」ダイアログが表示されます。
- **フォルダ**: エクスプローラーでそのフォルダを開きます（ツリーペインで選択中のフォルダも対象）。
- **リモート (SFTP) ファイル**: 一時フォルダ (`%TEMP%\tuif-remote\<プロファイルID>\`) へダウンロードしてから既定のアプリで開きます（編集内容はサーバーへ自動アップロードされません）。リモートフォルダは対象外です。
- `tuif` の画面は維持されたまま、アプリは別ウィンドウで非ブロッキング起動します。

### 5.2 `fzf` + `ripgrep` による高速ファジー検索・全文検索

現在開いているローカルフォルダ（またはツリーで選択中のローカルフォルダ）を起点に、`fzf`、`ripgrep` (`rg`)、`bat` を組み合わせた3種類のインタラクティブ検索を実行できます（メニューバーの **`Go (Alt+G)`** からも実行可能）。

| 機能 | ショートカット | 動作内容 | `Enter` 確定時の挙動 |
|---|---|---|---|
| **① 再帰的ファイル名ファジー検索** (`Fuzzy Find File`) | `Ctrl+P` または `Ctrl+F` | `rg --files` で配下の全ファイルを高速列挙し、右側ペインに `bat` によるシンタックスハイライト付きプレビューを表示しながら `fzf` でファジー絞り込みます。 | 選択したファイルが存在するフォルダへ移動し、**そのファイルにカーソルを合わせます**。 |
| **② ファイル内容の全文検索** (`Fuzzy Live Grep`) | `Ctrl+G` または `F` (`Shift+F`) | `rg --line-number` で配下の全ファイルの中身を検索し、右側ペインで `bat --highlight-line` により該当行をハイライト表示しながら絞り込みます。 | 選択した該当行のファイルが存在するフォルダへ移動し、**そのファイルにカーソルを合わせます**（ステータスバーに該当行番号も表示）。 |
| **③ サブフォルダへの一発ジャンプ** (`Fuzzy Jump Folder`) | `Alt+C` または `f` | 配下のサブディレクトリを再帰列挙し、`fzf` で目的のサブフォルダを絞り込みます。 | 選択したサブフォルダへ直接移動します。 |

- **隠しファイル設定との連動**: `tuif` 側で隠しファイル表示（`.` / `Ctrl+H`）を ON にしている場合は、`--hidden` 相当で隠しファイル・隠しフォルダも検索対象に含まれます（`.git` は常に除外）。

### 5.3 外部アプリのカスタマイズ (`config.toml`)

`%APPDATA%\tuif\config.toml` に `[[external_apps]]` セクションを追加・編集することで、任意のツールを追加・変更できます。

```toml
# 例: GUI テキストエディタ (notepad++) を追加
[[external_apps]]
name = "Notepad++"
program = "notepad++.exe"
args = ["{path}"]
key_chord = "Ctrl+Alt+N"
is_gui = true

# 例: Python スクリプト実行
[[external_apps]]
name = "Run Python"
program = "python.exe"
args = ["{path}"]
key_chord = "Ctrl+Alt+P"
is_gui = false
```

- **置換プレースホルダー**:
  - `{path}`: 選択中項目のフルパス（フォルダ選択時はフォルダパス）
  - `{dir}`: 現在表示中のフォルダパス
- **`is_gui`**:
  - `true`: バックグラウンドで非ブロッキング起動（`tuif` の画面を保持）
  - `false`: TUI をサスペンドしてフォアグラウンド実行（終了後に自動復帰）

---

## 6. 【オプション】PowerShell カレントディレクトリ追従の設定方法と使い方 (`cd on exit`)

本機能は **必須の設定ではありません**。通常、`tuif`（または `tuif.exe`）を直接起動して使用する分には設定不要です。

Windows OS の仕組み上、実行中の子プロセス（`.exe`）が親プロセス（呼び出し元の PowerShell）のカレントディレクトリを直接書き換えることはできません。  
そのため、「**`tuif` でフォルダーを巡回したあと、`Ctrl+Q` で終了した時だけ PowerShell 側のカレントディレクトリもそのフォルダーへ移動させたい**」という場合のみ、以下のシェル関数を設定します。

### 6.1 設定方法 (PowerShell `$PROFILE`)

PowerShell のプロファイルファイル（`$PROFILE`）に以下の関数とエイリアスを追記します。

```powershell
# tuif: cd on exit ラッパー関数
function Invoke-Tuif {
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        # tuif を終了時パス受取用の一時ファイル指定付きで呼び出し
        tuif --choosedir $tmp @args

        # Ctrl+Q で終了した場合のみ一時ファイルにパスが書き込まれる
        if (Test-Path $tmp) {
            $cwd = Get-Content -LiteralPath $tmp -Raw -Encoding UTF8
            if (-not [string]::IsNullOrWhiteSpace($cwd) -and (Test-Path $cwd.Trim())) {
                Set-Location -LiteralPath $cwd.Trim()
            }
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
}

# 短縮エイリアス 'tf' を登録
Set-Alias -Name tf -Value Invoke-Tuif
```

> **設定の反映**:
> 追記後、PowerShell を再起動するか、`. $PROFILE` を実行して設定を再読み込みしてください。

---

### 6.2 使い方

設定後は、PowerShell 上で **`tf`** コマンドを使って起動します。

```powershell
# カレントディレクトリで開く
tf

# 指定したフォルダーを開く
tf C:\Users\username\Projects
```

#### 終了キーに応じた挙動の違い:
| 終了操作 | キー | 動作 |
|---|---|---|
| **通常の終了** | **`q`**（またはメニューバーの `File` → `Exit`） | **起動元のフォルダーのまま**終了します（PowerShell のカレントディレクトリは変わりません）。 |
| **フォルダー移動して終了** | **`Ctrl+Q`**（またはメニューバーの `File` → `Exit & Cd`） | **`tuif` で現在開いているフォルダーへ PowerShell のカレントディレクトリを移動して終了**します。 |

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
- **Profile Name**: 一覧やツリーに表示する任意の表示名 (例: `Ubuntu VM`)
- **Host / IP**: 接続先サーバーのホスト名または IP アドレス
- **Port**: SSH/SFTP ポート番号 (既定: `22`)
- **Username**: ログインユーザー名
- **Password**: ログインパスワード (手入力。保存しない場合は空欄で接続時にプロンプト入力)
- **Save Password**: Windows 資格情報マネージャー (`Windows Credential Manager`) にパスワードを暗号化保存するかどうかの切り替え (`Space` キーでトグル)
- **Initial Dir**: 接続時に開く初期ディレクトリ (空欄時はサーバーのホームディレクトリ)

---

### 7.2 ホスト鍵の確認 (TOFU: Trust on First Use)

初めて接続するサーバーの場合、ホスト鍵の SHA256 フィンガープリント確認ダイアログが表示されます。
`y` または `Enter` を押すと、`%APPDATA%\tuif\known_hosts.toml` に安全に登録され、次回以降の接続時に自動検証されます（未知の鍵変更時は警告ブロックされます）。

---

### 7.3 Drives / Tree ペインでのリモートツリー表示

接続が完了すると、左側の **`Drives / Tree`** ペインに `☁ [SFTP] <プロファイル名>` のリモートツリーノードが追加されます。
- `l` キーまたはマウスクリックで展開・折畳が可能で、サーバー上のディレクトリ階層をローカルドライブと同様にシームレスに行き来できます。
- パスバーには `sftp://<user>@<host>/<path>` 形式でリモートパスが表示されます。

---

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

---

### 7.5 ファイル転送の進捗表示と中断 (プログレスバー & キャンセル)

ファイルやディレクトリのアップロード・ダウンロード・リモート間複製を実行すると、転送プログレスモーダルが画面中央にポップアップ表示されます。

```text
┌ Transferring files... ───────────────────────────────────────────┐
│ File: large_data.iso                                             │
│ [█████████████████████████████████████             ] 35 MB / 100 MB (35%) │
│                                                                  │
│ Total: 2 / 5 files (150 MB / 450 MB)                             │
│ [██████████████████████                           ] 33%          │
│  Press [Esc] to cancel                                           │
└──────────────────────────────────────────────────────────────────┘
```

- **2 段階プログレスバー**:
  - **上部バー (Current File)**: 現在転送している個々のファイルの転送バイト数・合計サイズ・パーセントを表示します。
  - **下部バー (Overall)**: 事前スキャンに基づき、バッチ全体の完了ファイル数・累積転送バイト数・全体進捗率を表示します。
- **非同期・ノンプリーエンプティブ実行**: ファイル転送はバックグラウンドスレッドで実行されるため、UIの描画やレスポンスがフリーズすることなく滑らかに進捗が更新されます。
- **`Esc` キーによる中断 (Cancel)**:
  - 転送中に `Esc` キーを押すと即座に転送中止フラグがセットされ、`Cancelling transfer... Please wait` と表示された後、安全に転送が中断されます。
  - 中断時には、途中まで書き込まれたローカルまたはリモートの不完全な一時ファイルが自動的にクリーンアップ（削除）されます。

---

### 7.6 同名ファイルの衝突確認ダイアログ (衝突解決)

ファイル転送（ローカル ⇔ リモート、ローカル同士、リモート間）の実行時、転送先に同名のファイルが既に存在する場合、確認ダイアログがポップアップ表示されます。

```text
┌ Confirm File Replace ─────────────────────────────────────────────┐
│ Destination file already exists:                                   │
│   important_report.pdf                                            │
│                                                                   │
│  [ Overwrite(o) ]  Skip(s)   OverwriteAll(a)   SkipAll(k)   Cancel(Esc) │
│ Left/Right/Tab: Select | Enter: Confirm | o/s/a/k/Esc: Quick key  │
└───────────────────────────────────────────────────────────────────┘
```

#### 選択可能なアクション
- **`Overwrite (o)`**: 対象のファイルを上書きします。
- **`Skip (s)`**: 対象のファイルの転送をスキップし、次のファイルへ進みます。
- **`Overwrite All (a)`**: 以降に重複する同名ファイルも含め、すべて確認なしで上書きします。
- **`Skip All (k)`**: 以降に重複する同名ファイルも含め、すべて確認なしでスキップします。
- **`Cancel (Esc)`**: 転送操作全体を中止します。

#### 操作キー
- `←` / `→` または `Tab` / `Shift+Tab`: ボタンのフォーカス移動
- `Enter`: 選択中のアクションを実行
- `o` / `s` / `a` / `k` / `Esc` (または `c`): ショートカットキーによる即時決定

---

## 8. トラブルシューティング / FAQ

### Q. SFTP 接続時に `Authentication failed` になる
- ユーザー名およびパスワードが正しいか確認してください。
- 接続先サーバーの SSH 設定 (`/etc/ssh/sshd_config`) で、パスワード認証 (`PasswordAuthentication yes`) またはキーボードインタラクティブ認証が許可されているか確認してください。
- 接続先で SFTP サブシステム (`Subsystem sftp /usr/lib/openssh/sftp-server`) が有効になっているか確認してください。

### Q. 保存したパスワードを削除・リセットしたい
- `Ctrl+S` で接続マネージャーを開き、対象プロファイルで `e` を押してパスワード欄を空にして保存するか、Windows のスタートメニューから「**資格情報マネージャー**」を開き、`Windows 資格情報` の一覧から `tuif-sftp:<プロファイルID>` を削除してください。

### Q. サーバーの再インストール等で `HOST KEY MISMATCH` 警告が出る
- サーバーの鍵が更新された正当な理由がある場合は、`%APPDATA%\tuif\known_hosts.toml` をテキストエディタで開き、該当ホストのエントリを削除してから再接続してください（新しいホスト鍵の登録ダイアログが表示されます）。

### Q. 日本語を含むファイル名が文字化けする
- `tuif` は UTF-8 を標準として送受信を行います。Linux サーバー側のロケールが UTF-8（例: `ja_JP.UTF-8` や `C.UTF-8`）に設定されていることを推奨します。

---

## 9. テーマ・配色のカスタマイズ (`config.toml`)

`%APPDATA%\tuif\config.toml` を作成・編集することで、画面全体のカラーテーマプリセットや個別UI要素の配色を自由にカスタマイズできます。

### テーマプリセットの指定 (`[theme]`)

`preset` に組み込みのテーマ名を指定します。

```toml
[theme]
preset = "nord"
```

利用可能なプリセット:
- `"default"`: 標準のシアン・イエローベース
- `"nord"`: 落ち着いたブルー・アークティックトーン
- `"monokai"`: ハイコントラストなプログラミング向けテーマ
- `"catppuccin"`: パステルトーンのモダンテーマ (Mocha)
- `"solarized-dark"`: 目に優しいダークテーマ
- `"light"`: 明るい背景環境向けのライトテーマ

### 個別カラーのオーバーライド (`[colors]`)

プリセットをベースにしながら、特定要素の色のみを上書き変更できます。
指定可能な値は、標準カラー名（`"black"`, `"red"`, `"green"`, `"yellow"`, `"blue"`, `"magenta"`, `"cyan"`, `"gray"`, `"dark_gray"`, `"light_red"`, `"light_green"`, `"light_yellow"`, `"light_blue"`, `"light_magenta"`, `"light_cyan"`, `"white"`）、256色インデックス番号（`"0"`〜`"255"`）、または `#RRGGBB` / `#RGB` のHEXコードです。

```toml
[theme]
preset = "nord"

[colors]
bg = "#2E3440"                # 画面全体の背景色 ("none" または "reset" でターミナル背景透過)
border = "#88C0D0"            # アクティブペイン枠・モーダル枠
border_inactive = "#4C566A"   # 非アクティブペイン枠
cursor_fg = "#2E3440"         # カーソル行文字色
cursor_bg = "#88C0D0"         # カーソル行背景色
folder = "#81A1C1"            # ディレクトリ表示色
file = "#ECEFF4"              # 通常ファイル表示色
executable = "#A3BE8C"        # 実行可能ファイル (.exe, .bat, .cmd, .ps1)
selected = "#EBCB8B"          # 複数選択マーク・ハイライト色
status_bar_fg = "#E5E9F0"     # ステータスバー文字色
status_bar_bg = "#3B4252"     # ステータスバー背景色
menu_bar_active_fg = "#2E3440"# メニューバーアクティブ項目文字色
menu_bar_active_bg = "#88C0D0"# メニューバーアクティブ項目背景色
tree_current = "#88C0D0"      # ツリーペインでのカレントフォルダ表示色
tree_cursor = "#EBCB8B"       # ツリーペインのカーソル色
```

---

## 10. 動作オプション・レイアウト設定 (`config.toml`)

`config.toml` に `[general]` および `[layout]` セクションを追加することで、操作挙動や表示レイアウトを細かくカスタマイズできます。

```toml
# 基本動作・操作挙動
[general]
confirm_delete = true        # 削除・ごみ箱移動時に確認ダイアログを表示するか (default: true)
delete_to_trash = true       # 削除時にごみ箱へ送るか (false の場合は直接完全削除) (default: true)
dirs_first = false           # ソート時にフォルダを常にファイルより上に優先表示するか (default: false)
natural_sort = true          # 自然順ソート (file1, file2, file10) を使用するか (default: true)
scroll_offset = 2            # ツリーペイン等のスクロール余白行数 (default: 2)

# 画面レイアウト・表示
[layout]
tree_width_percent = 24      # Drives / Tree ペインの横幅の割合 (10〜50, default: 24)
min_col_width = 24           # マルチカラム一覧表示時の1列あたりの最小文字幅 (10〜80, default: 24)
time_format = "%Y/%m/%d %H:%M" # 更新日時の表示フォーマット (default: "%Y/%m/%d %H:%M")
```

### 設定項目の詳細

- **`confirm_delete`**:
  `false` に設定すると、ファイルやフォルダを削除（`d` / `Delete`）する際に確認ダイアログを出さずに即座に実行します。
- **`delete_to_trash`**:
  `false` に設定すると、ごみ箱を経由せず直ちに完全削除します。
- **`dirs_first`**:
  `true` に設定すると、名前順やサイズ順、日付順ソート時にもディレクトリが常にファイルより上部にまとめて表示されます。
- **`natural_sort`**:
  `true` の場合、ファイル名に含まれる数字を数値として解釈し、`file2.txt` を `file10.txt` より前に並べます。
- **`tree_width_percent`**:
  ツリーペイン（Drives / Tree）の表示幅を全体の 10% 〜 50% の範囲で調整できます。
- **`min_col_width`**:
  ファイル一覧をマルチカラム表示する際、1カラムに確保する最小の幅文字数を設定します。幅を広げることで長いファイル名を省略せずに表示しやすくなります。
- **`time_format`**:
  日時の表示形式を自由に変更できます（例: `"%Y-%m-%d %H:%M"`, `"%m/%d %H:%M:%S"` など）。トークンとして `%Y`（西暦4桁）、`%m`（月2桁）、`%d`（日2桁）、`%H`（時2桁）、`%M`（分2桁）、`%S`（秒2桁）が利用できます。

---

## 11. キーマップのカスタマイズ (`[keymap]`)

`config.toml` の `[keymap]` セクション（または従来の `[input_keymap_custom]`）を編集することで、各アクションを実行するキー割り当てを自由にカスタマイズできます。

> **互換性**: 従来の `[input_keymap_custom]` セクションもそのまま有効です。両方が記述されている場合は `[keymap]` の設定が優先されます。

### 設定例

```toml
[keymap]
# 単一キーまたはカンマ区切りで複数のショートカットキーを割り当て可能
open = "Enter"
open_default = "o, Shift+Enter"
go_parent = "Backspace, -"
exit = "q"
exit_and_cd = "Ctrl+q"
fuzzy_find_file = "Ctrl+P, Ctrl+F"
fuzzy_live_grep = "Ctrl+G, Shift+F"
fuzzy_jump_dir = "Alt+C, f"

# 直感的なエイリアス名やハイフン区切り記法も使用可能
quit = "q"                      # exit のエイリアス
exit-and-cd = "Ctrl-Q"          # ハイフン区切りや大文字小文字の揺れも許容
up = "k, Up"                    # move_up のエイリアス
down = "j, Down"                # move_down のエイリアス
zip = "z, Ctrl+Alt+Z"           # zip_create のエイリアス
unzip = "Z, Ctrl+Alt+X"         # zip_extract のエイリアス
```

### 柔軟なキー記法・エイリアス対応

- **修飾キー記法**:
  - `+` または `-` 区切りの両方に対応（例: `Ctrl+P`, `Ctrl-P`, `c-p`, `Control-P`, `Alt-C`, `a-c`, `Shift+Delete`, `s-delete` など）。
  - 大文字・小文字は問いません（`ctrl+p`, `CTRL+P`, `Ctrl+P` いずれも可）。
- **キー名エイリアス**:
  - `Enter` / `Return` / `ret`
  - `Backspace` / `bs`
  - `Space` / `spc`
  - `PageUp` / `pgup` / `PageDown` / `pgdn`
  - `Minus` / `-` / `hyphen`
  - `,` や `+` も単体キーとして指定可能
- **アクションIDの柔軟性**:
  - スネークケース（`exit_and_cd`）、ケバブケース（`exit-and-cd`）、大文字小文字混在に対応。
  - 主要な短縮エイリアスに対応（`quit` → `exit`, `up` → `move_up`, `down` → `move_down`, `left` → `move_left`, `right` → `move_right`, `zip` → `zip_create`, `unzip` → `zip_extract` など）。
  - 利用可能なアクションIDの完全なリストは [3.1 既定のキー割り当て (Vim-like)](#31-既定のキー割り当て-vim-like) を参照してください。

---

## 12. 設定ファイル (`config.toml`) の完全サンプル

`%APPDATA%\tuif\config.toml` のフルスペック設定テンプレートです。必要に応じてコピーし、お好みの項目を編集してご利用ください。

```toml
# ========================================================
# tuif 設定ファイル (`config.toml`)
# 保存先: %APPDATA%\tuif\config.toml
# ========================================================

# --------------------------------------------------------
# 1. 基本動作・操作挙動 ([general])
# --------------------------------------------------------
[general]
confirm_delete = true        # 削除・ごみ箱移動時に確認ダイアログを表示するか (default: true)
delete_to_trash = true       # 削除時にごみ箱へ送るか (false の場合は直接完全削除) (default: true)
dirs_first = false           # ソート時にフォルダを常にファイルより上に優先表示するか (default: false)
natural_sort = true          # 自然順ソート (file1, file2, file10) を使用するか (default: true)
scroll_offset = 2            # ツリーペイン等のスクロール余白行数 (default: 2)

# --------------------------------------------------------
# 2. 画面レイアウト・表示 ([layout])
# --------------------------------------------------------
[layout]
tree_width_percent = 24      # Drives / Tree ペインの横幅の割合 (10〜50, default: 24)
min_col_width = 24           # マルチカラム一覧表示時の1列あたりの最小文字幅 (10〜80, default: 24)
time_format = "%Y/%m/%d %H:%M" # 更新日時の表示フォーマット (default: "%Y/%m/%d %H:%M")

# --------------------------------------------------------
# 3. カラーテーマ ([theme] / [colors])
# --------------------------------------------------------
[theme]
# 利用可能プリセット: "default", "nord", "monokai", "catppuccin", "solarized-dark", "light"
preset = "nord"

# 個別カラー上書き (指定しない項目はプリセットの色が適用されます)
# 色指定形式: 標準カラー名 ("cyan", "yellow" 等) / 256色番号 ("0"〜"255") / HEXコード ("#88C0D0")
[colors]
bg = "#2E3440"                # 画面全体の背景色 ("none" または "reset" でターミナル背景透過)
border = "#88C0D0"            # アクティブペイン枠・モーダル枠
border_inactive = "#4C566A"   # 非アクティブペイン枠
cursor_fg = "#2E3440"         # カーソル行文字色
cursor_bg = "#88C0D0"         # カーソル行背景色
folder = "#81A1C1"            # ディレクトリ表示色
file = "#ECEFF4"              # 通常ファイル表示色
symlink = "#B48EAD"           # シンボリックリンク・ジャンクション色
executable = "#A3BE8C"        # 実行可能ファイル (.exe, .bat, .cmd, .ps1 等)
selected = "#EBCB8B"          # 複数選択マーク・ハイライト色
status_bar_fg = "#E5E9F0"     # ステータスバー文字色
status_bar_bg = "#3B4252"     # ステータスバー背景色
menu_bar_active_fg = "#2E3440"# メニューバーアクティブ項目文字色
menu_bar_active_bg = "#88C0D0"# メニューバーアクティブ項目背景色
tree_current = "#88C0D0"      # ツリーペインでのカレントフォルダ表示色
tree_cursor = "#EBCB8B"       # ツリーペインのカーソル色

# --------------------------------------------------------
# 4. キーマップ ([keymap])
# --------------------------------------------------------
[keymap]
open = "Enter"
open_default = "o, Shift+Enter"
go_parent = "Backspace, -"
exit = "q"
exit_and_cd = "Ctrl+q"
fuzzy_find_file = "Ctrl+P, Ctrl+F"
fuzzy_live_grep = "Ctrl+G, Shift+F"
fuzzy_jump_dir = "Alt+C, f"

# --------------------------------------------------------
# 5. 外部ツール連携 ([[external_apps]])
# --------------------------------------------------------
[[external_apps]]
name = "Neovim"
program = "nvim.exe"
args = ["{path}"]
key_chord = "Ctrl+Alt+V"
is_gui = false

[[external_apps]]
name = "VS Code"
program = "code"
args = ["{path}"]
key_chord = "Ctrl+Alt+E"
is_gui = true

[[external_apps]]
name = "Lazygit"
program = "lazygit.exe"
args = []
key_chord = "Ctrl+Alt+G"
is_gui = false
```
