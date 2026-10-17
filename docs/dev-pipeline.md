# 開発パイプライン: Creator-Evaluator構成 (Antigravity 版)

このプロジェクトは、要件抽出からコミット・リリースまでを Creator(生成) / Evaluator(検証) のペアで構成されたパイプラインで進める。各ロールの定義は `.agents/skills/dev-pipeline/references/agents/` に格納されており、`.agents/skills/dev-pipeline/SKILL.md` を通じて参照・実行する。

## ステージ構成

| # | ステージ | Creator | Evaluator | 人間レビュー |
|---|---|---|---|---|
| 1 | 要件抽出 | requirements-extractor | requirements-validator | **必須**(原文解釈の確認) |
| 2 | 既存調査 | codebase-researcher | research-verifier | 解釈が割れた場合のみ |
| 3 | 仕様書作成 | spec-writer | spec-reviewer | **必須・最重要**(矛盾/未決定の解消、ベースライン承認) |
| 4 | アーキ設計 | architecture-mapper | architecture-guardian | 大きな構造変更のみ必須 |
| 5 | 単体テスト/コード生成 | unit-test-writer, code-generator | code-reviewer | マージ前の最終確認 |
| 6 | ユースケース/テストケース | testcase-writer | coverage-auditor | カバレッジレポート確認(推奨) |
| 7 | 自動テスト生成・実行 | test-automation-engineer, test-debugger | `cargo test` 実行結果 | FLAKY/連続失敗時のみ |
| 8 | テスト結果記録 | test-result-recorder | - | リリース判定時に**必須** |
| 9 | 成果物管理 | commit-pr-manager | pr-reviewer | PRマージは常に**必須** |
| - | 全体統制 | dev-pipeline-orchestrator | - | 承認待ちキューの管理 |

## 共有状態(Source of Truth)

横断的な状態は以下のファイルに永続化する。

- `intake/` : 未処理の一次資料
- `specs/_drafts/` : 要件抽出直後のドラフト(未確定)
- `specs/_registry.md` : 確定した仕様のSpec ID台帳(階層構造)
- `specs/_issues.md` : 重複・矛盾・未決定(TBD)の管理台帳
- `docs/architecture/traceability.md` : 仕様ID ⇔ アーキ構成要素 ⇔ コード ⇔ テストケースの対応表
- `tests/cases/_index.md` : ユースケース/テストケース一覧とSpec IDカバレッジ
- `tests/results/ledger.md` : テスト結果(PASS/FAIL/NOT_RUN)と証跡へのリンク
