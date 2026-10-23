---
name: dev-pipeline
description: Creator-Evaluator構成の9ステージ開発パイプライン手順とSubagent定義集。仕様策定・アーキ設計・TDD実装・テスト検証・コミット管理の各ステージを進行させる際、またはサブエージェント(invoke_subagent / define_subagent)を活用する際に参照すること。
---

# Creator-Evaluator 開発パイプライン Skill (Antigravity 版)

このスキルは、`C:\Users\toshi\git\project` の `.claude/agents/` で運用されていた19種の Creator / Evaluator サブエージェントによる開発パイプラインを、Antigravity 環境で実行するための手順書です。

## 1. ステージ構成とロール一覧

各ロールの詳細な指示書（システムプロンプト）は `./references/agents/<agent-name>.md` に格納されています。

| # | ステージ | Creator ロール | Evaluator ロール | 人間レビュー要否 |
|---|---|---|---|---|
| 1 | 要件抽出 | [requirements-extractor](./references/agents/requirements-extractor.md) | [requirements-validator](./references/agents/requirements-validator.md) | **必須** (原文解釈の確認) |
| 2 | 既存調査 | [codebase-researcher](./references/agents/codebase-researcher.md) | [research-verifier](./references/agents/research-verifier.md) | 解釈が割れた場合のみ |
| 3 | 仕様書作成 | [spec-writer](./references/agents/spec-writer.md) | [spec-reviewer](./references/agents/spec-reviewer.md) | **必須・最重要** (ベースライン承認) |
| 4 | アーキ設計 | [architecture-mapper](./references/agents/architecture-mapper.md) | [architecture-guardian](./references/agents/architecture-guardian.md) | 大きな構造変更のみ必須 |
| 5 | 単体テスト/コード生成 | [unit-test-writer](./references/agents/unit-test-writer.md), [code-generator](./references/agents/code-generator.md) | [code-reviewer](./references/agents/code-reviewer.md) | マージ前の最終確認 |
| 6 | ユースケース/テストケース | [testcase-writer](./references/agents/testcase-writer.md) | [coverage-auditor](./references/agents/coverage-auditor.md) | カバレッジレポート確認(推奨) |
| 7 | 自動テスト生成・実行 | [test-automation-engineer](./references/agents/test-automation-engineer.md), [test-debugger](./references/agents/test-debugger.md) | `cargo test` 実行結果 | FLAKY/連続失敗時のみ |
| 8 | テスト結果記録 | [test-result-recorder](./references/agents/test-result-recorder.md) | - | リリース判定時に**必須** |
| 9 | 成果物管理 | [commit-pr-manager](./references/agents/commit-pr-manager.md) | [pr-reviewer](./references/agents/pr-reviewer.md) | コミット/マージ判断 |

## 2. Antigravity での実行方法

Antigravity では以下の2つの方法でこのパイプラインを運用します。

### 方法A: `define_subagent` / `invoke_subagent` によるサブエージェント分離実行
独立したコンテキストで厳密な Evaluator レビュー（`spec-reviewer`, `architecture-guardian`, `code-reviewer` 等）や並列作業を行いたい場合：
1. 対象ロールの `./references/agents/<role>.md` を読み込む。
2. 読み取り専用の Evaluator ロールは組み込みの `research` サブエージェントを `invoke_subagent` (Model: `inherit`) で呼び出すか、`define_subagent` で定義して呼び出す。
3. ファイル編集・コマンド実行を伴う Creator ロールは組み込みの `self` サブエージェントを `invoke_subagent` で呼び出すか、`define_subagent` (`enable_write_tools: true`) で定義して呼び出す。

### 方法B: メインエージェントによるステージ順守実行
小規模な変更や対話的な進行では、メインエージェント自身が該当ロールの指示書 (`./references/agents/<role>.md`) と関連スキル (`spec-management`, `coding-standards`, `pr-conventions`) を読み込み、Creator ステップと Evaluator 検証ステップを順番に実施する。

## 3. 共有状態 (Source of Truth)
- `specs/_registry.md` : 確定した仕様の Spec ID 台帳
- `specs/_issues.md` : 重複・矛盾・未決定 (TBD) の管理台帳
- `docs/architecture/traceability.md` : 仕様ID ⇔ 実装モジュール ⇔ テストスイートの対応表
- `tests/` : 結合テストコード (`*.rs`)
- `cargo test` / CI : テスト実行と合否判定 (`.github/workflows/ci.yml`)
