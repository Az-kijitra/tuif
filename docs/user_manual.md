# `cuifiler` ユーザーマニュアル (0.1.0)

`cuifiler` は、Windows PowerShell 環境で動作するキーボード主体 (Vim-like) の CUI/TUI ファイラーです。

## 1. 設定フォルダー

設定ファイルは `%APPDATA%\cuifiler` に保存されます。

| ファイル | 内容 |
|---|---|
| `config.toml` | キー割り当て (`[input_keymap_custom]`)、テーマ (`ui_theme`) |
| `navigation.toml` | 訪問履歴とジャンプリスト |
| `ui-state.toml` | ツリー・隠しファイル・サイズ列・更新日時列の表示状態、ソート条件 |

## 2. 画面と基本操作

画面は上から **メニューバー**、**パスバー (および検索バー)**、**メイン領域 (ドライブツリー＋ファイル一覧)**、**ステータスバー** の4段で構成されます。

### 2.1 既定のキー割り当て (Vim-like)

| 操作 | 既定のキー | アクションID (`config.toml`) |
|---|---|---|
| 上へ移動 | `k` または `↑` | `move_up` |
| 下へ移動 | `j` または `↓` | `move_down` |
| 左へ移動 (一覧:10件前 / ツリー:折畳・親へ) | `h` または `←` | `move_left` |
| 右へ移動 (一覧:10件次 / ツリー:展開・子へ) | `l` または `→` | `move_right` |
| 先頭へ移動 | `gg` または `Home` | `move_top` |
| 末尾へ移動 | `G` (`Shift+G`) または `End` | `move_bottom` |
| 前のページ | `Ctrl+U` または `PageUp` | `page_up` |
| 次のページ | `Ctrl+D` または `PageDown` | `page_down` |
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

## 3. 外部 CUI ツール連携

既定では以下の4つの CUI ツールが登録されています。起動時は TUI を一時サスペンドしてターミナルを引き渡し、ツール終了後に元のファイラー画面へ復帰します。

1. **PowerShellを開く (`Ctrl+Alt+C`)**: `pwsh.exe -NoLogo -WorkingDirectory {dir}`
2. **Neovimで開く (`Ctrl+Alt+V`)**: `nvim.exe {path}`
3. **Lazygitを開く (`Ctrl+Alt+G`)**: `lazygit.exe -p {dir}`
4. **batで表示 (`Ctrl+Alt+B`)**: `bat.exe --paging=always {path}`
