---
name: pr-conventions
description: コミットメッセージとプルリクエストの記述フォーマット、レビューチェックリスト。commit-pr-manager, pr-reviewerがコミット/PR作成・レビュー時に参照すること。
---

# コミット・PR規約

## コミットメッセージ形式
```
<種別>: <変更内容の要約>

対応Spec ID: SPEC-xxxx-xxxx, SPEC-yyyy-yyyy
```
種別は feat/fix/refactor/test/docs 等、プロジェクトの慣習に従う。

## PR説明テンプレート
```
## 概要
(変更内容)

## 対応Spec ID
- SPEC-xxxx-xxxx

## テスト結果
(tests/results/ledger.md の該当箇所を要約、またはリンク)

## レビュー観点
(pr-reviewer/人間に特に見てほしい点)
```

## レビューチェックリスト(pr-reviewer用)
1. diffが対応Spec IDの変更範囲と一致しているか(意図しないファイル変更がないか)
2. コーディング規約(coding-standards skill参照)・アーキテクチャ整合性に問題がないか
3. テスト結果が添付され、FAILが残っていないか
