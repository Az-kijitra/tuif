# トレーサビリティ台帳 (`traceability.md`)

仕様ID (Spec ID) ⇔ 実装モジュール ⇔ テストスイートの対応表。

| Spec ID | タイトル | 実装モジュール | 単体テスト / 統合テスト |
|---|---|---|---|
| SPEC-0100-0001 | `tuif` 全体コンセプト・スコープ境界 | `src/main.rs`, `src/app.rs`, `src/config/paths.rs` | `src/config/paths_tests.rs`, `tests/tui_workflow_tests.rs` |
| SPEC-0200-0001 | 技術スタックとレイヤードアーキテクチャ (Rust + Ratatui) | `src/shared/`, `src/config/`, `src/fs/`, `src/fs_ops/`, `src/archive/`, `src/viewer/`, `src/keymap/`, `src/external_apps/`, `src/tui/` | 全 778 テスト (単体 765 件 + 結合 13 件) |
| SPEC-0300-0001 | コアTUI画面・ファイル一覧・ツリー・Vim-likeキーマップ・外部ツール連携仕様 | `src/app.rs`, `src/tui/mod.rs`, `src/keymap/mod.rs`, `src/external_apps/mod.rs` | `src/keymap/mod.rs::tests`, `src/external_apps/mod.rs::tests`, `src/tui/mod.rs::tests`, `tests/tui_workflow_tests.rs`, `tests/os_clipboard_integration_tests.rs` |
