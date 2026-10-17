# テストケース一覧 (`_index.md`)

| Case ID | 対応Spec ID | 区分 (AUTO/MANUAL) | 概要 |
|---|---|---|---|
| TC-0001 | SPEC-0100-0001, SPEC-0200-0001, SPEC-0300-0001 | AUTO | コアモジュール単体テスト群 (ファイル列挙、コピー/移動/削除/リネーム/新規作成、ZIP圧縮/展開、設定読み書き、Vim-likeキーマップ解決、外部CUIツール引数展開、Ratatui `TestBackend` 日本語描画): 726件 |
| TC-0002 | SPEC-0300-0001 | AUTO | TUI E2E ワークフロー統合テスト (`tests/tui_workflow_tests.rs`): `j`/`k`/`gg`/`G` ナビゲーション、`a` 新規作成、`y`/`p` コピー＆ペースト、内蔵ビューア表示、`/` インクリメンタル検索、`Tab` ペイン切替 |
