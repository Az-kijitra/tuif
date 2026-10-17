# `cuifiler` プロジェクトルール (Antigravity Workspace Rules)

このリポジトリは、Windows PowerShell 環境で動作する Rust + Ratatui 製のキーボード主体 CUI/TUI ファイラー `cuifiler` です。
先行する GUI ファイラー `ReflexFiles` (`C:\Users\toshi\git\project`) の機能・設計資産をベースに、CUI/TUI に最適化した仕様で構築します。

## 1. 確定している基本方針
- **アプリ名 / バイナリ名**: `cuifiler` (設定フォルダ: `%APPDATA%\cuifiler`)
- **技術スタック**: Rust + Ratatui (`crossterm`) 単一バイナリ構成
- **キーバインド**: **Vim like** (`h`/`j`/`k`/`l` 移動、`gg`/`G` 先頭末尾、`y` コピー、`x`/`d` 切り取り、`p` 貼り付け、`r` リネーム、`a`/`A` 新規作成、`/` 検索など) + `config.toml` の `[input_keymap_custom]` によるカスタマイズ
- **外部 CUI ツール連携 (`external_apps`)**: GUI特化機能を廃し CUI ツールに限定。既定の4登録は **`pwsh`**, **`nvim`**, **`lazygit`**, **`bat`**。TUI の Alternate Screen / Raw mode を一時サスペンドして子プロセスへ標準入出力を引き渡す。
- **内蔵ビューア**: テキスト / Markdown を TUI フルスクリーンオーバーレイで表示。画像はスコープ外。

## 2. カスタム Skills の参照義務
作業内容に応じて `.agents/skills/` 配下の Skill を必ず参照すること：
- **仕様の作成・改訂・レビュー時**: `spec-management` (`.agents/skills/spec-management/SKILL.md`)
- **アーキテクチャ設計・コード生成・レビュー時**: `coding-standards` (`.agents/skills/coding-standards/SKILL.md`)
- **コミット・PR作成・レビュー時**: `pr-conventions` (`.agents/skills/pr-conventions/SKILL.md`)
- **パイプライン進行・サブエージェント呼び出し時**: `dev-pipeline` (`.agents/skills/dev-pipeline/SKILL.md`)

## 3. 仕様管理・人間レビューの原則
- `specs/_registry.md` における仕様ステータスの `APPROVED` への遷移、および `specs/_issues.md` の `RESOLVED` への変更は、人間の承認を得た上で行う。
- 自動でリトライ可能な工程（コード生成 → `cargo test` / `cargo clippy` 失敗 → 修正）は自律的に回してよい。
