# tuif 設定ファイル拡張 開発計画書

本ドキュメントは、`tuif`（`cuifiler`）においてキーアサイン、画面配色、動作オプション、レイアウト等を `config.toml` から柔軟に設定可能にするための開発計画書です。

---

## 1. 目的とゴール

- **画面色・テーマのカスタマイズ**:
  - ハードコードされていたUI色（Cyan, Yellow, DarkGray等）を構造化し、`[theme]` プリセットおよび `[colors]` による個別カラー指定（カラー名 / HEXコード）に対応する。
- **動作挙動（Behavior）の設定化**:
  - ディレクトリ優先表示、削除時確認、スクロールマージンなど、ファイラーの使い勝手を左右する挙動を `[general]` で制御可能にする。
- **画面レイアウトのカスタマイズ**:
  - ツリー幅比率、マルチカラムの最小列幅、日時の表示フォーマットなどを `[layout]` で調整可能にする。
- **キーマップ定義の親和性向上**:
  - 従来の `[input_keymap_custom]` に加え、わかりやすいセクション名 `[keymap]` のエイリアス対応や複数キー定義の柔軟化を行う。
- **後方互換性と堅牢性の維持**:
  - 既存の `config.toml`（`ui_theme = "dark"` や `[[external_apps]]`）との互換性を100%維持し、不正な値が含まれていてもデフォルト値に安全にフォールバックしてクラッシュしない設計とする。

---

## 2. 設定ファイル（`config.toml`）の最終仕様案

```toml
# ==========================================
# tuif (cuifiler) 設定ファイル仕様
# ==========================================

# 1. 基本動作・操作挙動
[general]
confirm_delete = true        # 削除・ごみ箱移動時に確認ダイアログを表示するか (default: true)
delete_to_trash = true       # 'd' 押下時にごみ箱へ送るか (false の場合は完全削除) (default: true)
dirs_first = true            # 一覧でフォルダを常にファイルより上に表示するか (default: true)
scroll_offset = 2            # カーソル移動時に画面端に達する前のスクロール余白行数 (default: 2)
natural_sort = true          # 自然順ソート (file1, file2, file10) を使用するか (default: true)

# 2. 画面レイアウト・表示
[layout]
tree_width_percent = 24      # Drives / Tree ペインの幅の割合 (10〜50, default: 24)
min_col_width = 24           # マルチカラム一覧表示時の1列あたりの最小文字幅 (default: 24)
time_format = "%Y/%m/%d %H:%M" # 更新日時の表示フォーマット (default: "%Y/%m/%d %H:%M")

# 3. テーマ・配色
[theme]
preset = "default"           # "default", "nord", "monokai", "catppuccin", "solarized-dark", "light"

# 各UI要素の個別カラーオーバーライド (カラー名 または "#rrggbb" HEXコード)
[colors]
cursor_fg = "black"
cursor_bg = "cyan"
border = "cyan"
border_inactive = "dark_gray"
folder = "yellow"
file = "white"
symlink = "light_magenta"
executable = "light_green"
selected_mark = "light_blue"
status_bar_fg = "white"
status_bar_bg = "dark_gray"
menu_bar_active_fg = "black"
menu_bar_active_bg = "cyan"

# 4. キーアサイン (既存の [input_keymap_custom] と相互互換)
[keymap]
open = "Enter"
open_default = "o, Shift+Enter"
go_parent = "Backspace, -"
exit = "q"
exit_and_cd = "Ctrl+q"
fuzzy_find_file = "Ctrl+p, Ctrl+f"
fuzzy_live_grep = "Ctrl+g, Shift+f"
fuzzy_jump_dir = "Alt+c, f"

# 5. 外部ツール連携 (既存仕様維持)
[[external_apps]]
name = "Open in Neovim"
program = "nvim.exe"
args = ["{path}"]
key_chord = "Ctrl+Alt+V"
is_gui = false
```

---

## 3. フェーズ別 開発ステップ

### フェーズ 1: テーマ・カラー設定基盤の実装 (優先度: 高) [完了]
ハードコードされている Ratatui `Color` を一元管理する `ThemeColors` モジュールを作成し、設定ファイルから読み込めるようにします。

1. **型定義とパーサーの実装** (完了):
   - `src/theme/mod.rs`（または `src/shared/schema/ui_theme.rs`）の拡張。
   - 文字列（`"cyan"`, `"black"`, `"#1e1e2e"` など）から Ratatui `Color` への安全なパース関数の実装。
   - プリセット定義（`Default`, `Nord`, `Monokai`, `Catppuccin`, `Solarized-Dark`, `Light`）。
2. **`config.toml` ローダーの拡張** (完了):
   - `src/config/config_toml.rs` にて `[theme]` および `[colors]` のパース処理を追加。
3. **TUI 描画処理への統合** (完了):
   - `src/tui/mod.rs` 内で直接参照されていた `Color::Cyan`, `Color::Yellow` 等を、`app.theme` のスタイル経由で描画するように置換。
4. **テスト・検証** (完了):
   - 不正なカラー文字列が入った際のフォールバックテスト。
   - プリセット読み込み・個別上書きの単体テスト。全 760 テスト通過確認済み。

---

### フェーズ 2: 動作・レイアウト設定の実装 (優先度: 中) [完了]
操作性や表示を細かくチューニングできる `[general]` と `[layout]` セクションを導入します。

1. **データ構造の定義** (完了):
   - `GeneralConfig` (`confirm_delete`, `delete_to_trash`, `dirs_first`, `natural_sort`, `scroll_offset`) の追加。
   - `LayoutConfig` (`tree_width_percent`, `min_col_width`, `time_format`) の追加。
2. **`config_toml.rs` での読み込みと AppState への保持** (完了):
   - 各フィールドをフォールバック付きでパースし、`AppState` に格納。
3. **ロジックへの反映** (完了):
   - ソート処理（`src/fs/query.rs`）で `dirs_first` や `natural_sort` を反映。
   - 削除処理（`src/app.rs`）で `confirm_delete` および `delete_to_trash` を反映。
   - レイアウト計算およびマウスヒットテスト（`src/tui/mod.rs`, `src/app.rs`）で `tree_width_percent` や `min_col_width` を反映。
   - 日時フォーマット処理（`format_timestamp`）で `time_format` を反映。
4. **テスト・検証** (完了):
   - ソート順序の切り替えテスト、レイアウト計算の挙動テスト。全 763 テスト通過確認済み。

---

### フェーズ 3: キーマップ設定の強化 (優先度: 中) [完了]
直感的で柔軟なキーカスタマイズを可能にします。

1. **セクション名エイリアス** (完了):
   - `[keymap]` と従来の `[input_keymap_custom]` の両方を認識（`[keymap]` を優先）。
2. **設定キーの表記揺れ対応** (完了):
   - `"Ctrl+P"`, `"ctrl+p"`, `"Ctrl-P"`, `"c-p"`, `"Control-P"` 等の修飾キー記法寛容化。
   - `Enter` (`ret`, `return`), `Backspace` (`bs`), `Space` (`spc`), `PageUp` (`pgup`), `PageDown` (`pgdn`), `Minus` (`-`, `hyphen`) 等のキー名エイリアス対応。
   - `exit` (`quit`), `exit-and-cd`, `move_up` (`up`), `zip_create` (`zip`) 等のアクションIDエイリアスおよびケバブケース・大文字小文字対応。
3. **テスト・検証** (完了):
   - 既存のキーマップテスト群および新規のエイリアス・ハイフン区切りテストを追加。全 766 テスト通過確認済み。

---

## 4. ドキュメント更新と総合検証 (優先度: 高) [完了]
ユーザー向けマニュアルの整備と最終品質確認。

1. **`docs/user_manual.md` の更新** (完了):
   - 設定ファイル（`config.toml`）の完全なリファレンスと設定例の掲載（第9章〜第12章）。
   - プリセットテーマ一覧、サポートカラー一覧の記載。
   - 動作オプション・レイアウト設定リファレンスの記載。
   - キーマップ設定・エイリアス仕様の記載。
   - コピー＆ペースト可能なフルスペック設定テンプレート（第12章）の追加。
2. **総合テスト** (完了):
   - `cargo test`（全766テスト通過、0 failed）。
   - `cargo clippy --lib --bins -- -D warnings`（警告ゼロ）。
   - リリースビルド（`cargo build --release`）の生成完了。

---

## 5. 影響範囲・ファイル一覧

| モジュール / ファイル | 変更内容 |
|---|---|
| `src/config/config_toml.rs` | `[general]`, `[layout]`, `[theme]`, `[colors]`, `[keymap]` のパーサー拡張 |
| `src/shared/schema/ui_theme.rs` | テーマ・カラー構造体、カラー文字列パース、プリセット定義 |
| `src/app.rs` | `AppState` に `ThemeConfig`, `GeneralConfig`, `LayoutConfig` を保持、各処理への反映 |
| `src/tui/mod.rs` | 各ウィジェット描画（ファイル一覧、ツリー、メニューバー、ステータスバー等）での動的カラー参照 |
| `src/fs/query.rs` | ディレクトリ優先・自然順ソート等のオプション反映 |
| `tests/tui_workflow_tests.rs` | カスタムカラー・カスタム設定下での動作検証テスト追加 |
| `docs/user_manual.md` | 設定項目の詳細ドキュメント追記 |
