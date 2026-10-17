---
name: coding-standards
description: コード生成・実装レビュー時に適用するコーディング規約・lintルール・命名規則（Rust + Ratatui 版）。code-generator, code-reviewer, architecture-guardianなど実装に関わるAgentが作業前に参照すること。
---

# コーディング規約 (`cuifiler`: Rust + Ratatui)

このプロジェクトの実装コードが従うべき規約。`cargo fmt` / `cargo clippy` の設定と矛盾する場合はツール設定を正とし、このドキュメントを更新すること。

## 1. 命名規則・基本スタイル
- Rust 標準の命名規則（RFC 430）に従う：
  - 型・トレイト・Enum バリアント: `UpperCamelCase`
  - 関数・メソッド・変数・モジュール・ファイル名: `snake_case`
  - 定数・static: `SCREAMING_SNAKE_CASE`
- モジュール分割とファイルサイズ基準：
  - 1ファイルの目安は **400行以下** とし、責務ごとに適切に分割する。
  - 逆に、20行未満の極小ファイルを無意味に乱立させない（凝集度を保つ）。
  - 単体テストは対象モジュールと同階層の `<module>_tests.rs` に分離するか、ファイル末尾の `#[cfg(test)] mod tests` に記述する。

## 2. フォーマット・Lint
- フォーマットは `cargo fmt --check` が差分なしで通過すること。
- Lint は `cargo clippy --all-targets -- -D warnings` が警告・エラーなしで通過すること。

## 3. アーキテクチャ・TUI 設計原則
- **UI描画 (`tui/`) とドメインロジック (`domain/`, `keymap/`, `config/`) の分離**:
  - `tui/widgets/` 内の描画関数 (`Frame` を受け取る関数) は副作用を持たず、`AppState` の読み取り専用参照から描画のみを行うこと。
  - ファイルシステム操作や設定の読み書きを、描画関数内から直接呼び出してはならない。
- **日本語・全角文字幅の扱い**:
  - ターミナル上の表示カラム幅計算・切り詰め (`truncate`) にはバイト長 (`len()`) や文字数 (`chars().count()`) を直接使わず、必ず `unicode-width` (`UnicodeWidthStr` / `UnicodeWidthChar`) を使用すること。
- **ターミナル状態の保護**:
  - Alternate Screen / Raw mode の有効化中は、パニック時や外部CUIアプリ (`pwsh`, `nvim`, `lazygit`, `bat` 等) の起動時に必ずターミナル状態を復帰させるガードを設けること。

## 4. エラーハンドリング・安全性方針
- プロダクションコード（テストコードを除く）での `.unwrap()` / `.expect()` / `panic!()` の使用は原則禁止とする（不変条件が静的に保証される箇所を除く）。`Result<T, E>` で伝播させ、ステータスバー通知またはエラーダイアログとしてユーザーに提示する。
- パスを受け取るファイル操作・外部アプリ起動では、ファイルシステムに触れる前に `shared::abs_path` による絶対パス検証を通すこと。
- `unsafe` ブロックは Windows API 呼び出し（ドライブ列挙や空き容量取得など標準ライブラリで代替できない箇所）に限定し、最小限のモジュールに閉じ込めること。

## 5. レビューチェックリスト (code-reviewer / architecture-guardian 用)
1. `cargo fmt --check` および `cargo clippy --all-targets -- -D warnings` がエラーなく通過しているか
2. プロダクションコードで安易な `unwrap()` / `expect()` を使っていないか
3. TUI 描画層とドメインロジック層の責務分離・依存方向が守られているか
4. 日本語ファイル名の表示幅計算に `unicode-width` が使われているか
5. 外部コマンド起動時にシェルインジェクションの危険がないか（シェルを介さず `std::process::Command` に絶対パスと引数配列を渡しているか）
