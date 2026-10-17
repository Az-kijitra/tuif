# SPEC-0300-0001: コアTUI画面・ファイル一覧・ツリー・Vim-likeキーマップ・外部CUI連携仕様

| 項目 | 内容 |
|---|---|
| Spec ID | SPEC-0300-0001 |
| 領域 | 0300 コア機能 |
| 階層 | 機能・詳細仕様 (親: SPEC-0100-0001) |
| ステータス | APPROVED (2026-10-17 発注者合意事項に基づくベースライン) |
| 作成日 | 2026-10-17 |
| 一次出典 | `PLAN.md` §2〜§6、発注者合意事項 (`ISS-0001`)、先行GUI版仕様 (`SPEC-0300-0009`〜`0021`) |

## 1. 画面構成・レイアウト (Ratatui)

- **[R-301] 4段画面構成**: 上から順に以下の4領域を縦方向に配置する。
  1. **メニューバー (1行)**: 「ファイル (`Alt+F`)」「編集 (`Alt+E`)」「表示 (`Alt+V`)」「移動 (`Alt+G`)」およびヘルプ案内を表示。`F10` または `Alt+F/E/V/G` で開く。
  2. **パスバー / 検索バー (1〜2行)**: 現在のディレクトリ絶対パスを表示 (`Ctrl+L` で直接パス編集)。検索起動中 (`/`) はパスバー直下に1行の検索バーを表示する。
  3. **メインペイン (残り全高)**:
     - 左ペイン: **ドライブ＆ディレクトリツリー** (`ui_show_tree == true` のとき幅25〜30%で表示。`Ctrl+Shift+B` または `t` で表示切替、`Tab` でフォーカス切替)。最上位に論理ドライブ (`C:`, `D:` 等) を並べる。
     - 右ペイン: **ファイル一覧**。ディレクトリを先頭、ファイルをその後にソート (`name` / `size` / `type` / `modified`, `asc` / `desc`) して表示する。サイズ列 (`ui_show_size`) または更新日時列 (`ui_show_time`) が OFF のときは画面幅に応じた多カラムグリッド表示、ON のときは詳細テーブル表示とする。空フォルダや検索結果0件のときは `項目なし` を表示する。
  4. **ステータスバー (1行)**: 項目数・選択件数・現在のソート条件・クリップボード状態・一時通知メッセージを表示する。

## 2. Vim-like キーボードナビゲーションと既定キーマップ

- **[R-302] 既定キーマップ (Vim-like + 標準キー併用)**:
  `config.toml` の `[input_keymap_custom]` で上書き可能とし、既定値は以下の通りとする。

| アクションID | 動作 | 既定キー (`config.toml` 表記) |
|---|---|---|
| `move_up` | 上へ移動 | `K,ArrowUp` |
| `move_down` | 下へ移動 | `J,ArrowDown` |
| `move_left` | 左カラムへ移動 (ツリーフォーカス時は親/折りたたみ) | `H,ArrowLeft` |
| `move_right` | 右カラムへ移動 (ツリーフォーカス時は展開) | `L,ArrowRight` |
| `move_top` | 先頭項目へ移動 | `Home,G_Prefix` (`gg` シーケンスまたは `Home`) |
| `move_bottom` | 末尾項目へ移動 | `Shift+G,End` (`G` または `End`) |
| `page_up` | 前のページ | `Ctrl+U,PageUp` |
| `page_down` | 次のページ | `Ctrl+D,PageDown` |
| `select_toggle` | 選択の切り替え | `Space,V` |
| `select_all` | すべて選択 | `Shift+V,Ctrl+A` |
| `clear_selection` | 選択の解除 / 通知クリア | `Escape` |
| `open` | 開く (フォルダは移動、テキスト/MDは内蔵ビューア) | `Enter` |
| `go_parent` | 親フォルダへ移動 | `Backspace,Minus` (`-` キー併用) |
| `exit` | 終了 | `Q,Ctrl+Q` |
| `focus_path` | パス欄へ移動 (直接入力) | `Ctrl+L` |
| `toggle_hidden` | 隠しファイルの表示切替 | `Dot,Ctrl+H` (`.` または `Ctrl+H`) |
| `toggle_tree` | ツリー表示の切替 | `T,Ctrl+Shift+B` |
| `copy` | コピー (Yank: アプリ内バッファへ登録) | `Y,Ctrl+C` |
| `cut` | 切り取り (Cut: アプリ内バッファへ移動予約) | `X,Ctrl+X` |
| `paste` | 貼り付け (Paste) | `P,Ctrl+V` |
| `delete` | ごみ箱へ削除 (`Shift+Delete` または `Shift+D` で完全削除) | `Delete,D` |
| `rename` | 名前の変更 | `R,F2` |
| `new_file` | 新規ファイル作成 | `A,Ctrl+N` |
| `new_folder` | 新規フォルダ作成 | `Shift+A,Ctrl+Shift+N` |
| `search` | 検索バーを開く | `/` |
| `sort_menu` | 並び替えメニューを開く | `S` |
| `history` | 訪問履歴を開く | `Ctrl+Y` |
| `history_jump_list` | ジャンプリストを開く | `Ctrl+J` |
| `jump_add` | 現在のフォルダをジャンプリストに追加 | `Ctrl+Shift+J,M` |
| `toggle_size` | サイズ列の表示切替 | `Ctrl+Shift+S` |
| `toggle_time` | 更新日時列の表示切替 | `Ctrl+Shift+T` |
| `zip_create` | ZIPに圧縮 | `Z,Ctrl+Alt+Z` |
| `zip_extract` | ZIPを展開 | `Shift+Z,Ctrl+Alt+X` |
| `action_menu` | コンテキストアクションメニューを開く | ` semicolon,F9` (`;` または `F9` / `Apps`) |

## 3. 外部 CUI ツール連携 (`external_apps`)

- **[R-303] 既定の4登録**:
  `config.toml` に `external_apps` が未指定の場合、以下の4つの CUI ツールを既定とする。

| 順 | `name` | `program` | `args` | `args_for_file` | `key` |
|---|---|---|---|---|---|
| 1 | `PowerShellを開く` | `pwsh.exe` | `["-NoLogo", "-WorkingDirectory", "{dir}"]` | (なし) | `Ctrl+Alt+C` |
| 2 | `Neovimで開く` | `nvim.exe` | `["{path}"]` | (なし) | `Ctrl+Alt+V` |
| 3 | `Lazygitを開く` | `lazygit.exe` | `["-p", "{dir}"]` | (なし) | `Ctrl+Alt+G` |
| 4 | `batで表示` | `bat.exe` | `["--paging=always", "{path}"]` | (なし) | `Ctrl+Alt+B` |

- **[R-304] ターミナル引き渡し起動**:
  外部 CUI ツールを起動する際は、Ratatui の Alternate Screen と Raw mode を一時解除し、標準入力・標準出力・標準エラー出力を継承 (`Stdio::inherit()`) して子プロセスをフォアグラウンド実行 (`Command::status()`) する。子プロセス終了後、再び Alternate Screen と Raw mode を有効化して画面を完全再描画 (`terminal.clear()`) し、ディレクトリ内容を最新に再読込する。

## 4. PowerShell カレントディレクトリ連携 (`--choosedir`)

- **[R-305] 終了時パス書き出しオプション**:
  コマンドライン引数 `--choosedir <FILE>` (または `--cwd-file <FILE>`) が指定されて起動された場合、正常終了時にその時点のカレントディレクトリ絶対パスを `<FILE>` に UTF-8 で書き出す。これにより PowerShell 関数ラッパー側で `Set-Location` によるディレクトリ移動を実現する。
