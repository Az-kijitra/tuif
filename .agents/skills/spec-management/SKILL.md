---
name: spec-management
description: Spec IDの採番規則、仕様の階層化方法、ステータス遷移、重複・矛盾・未決定の起票ルール。spec-writer, spec-reviewer, architecture-mapperが仕様を扱う作業の前に参照すること。
---

# 仕様管理ルール

## Spec IDの形式
`SPEC-<領域番号4桁>-<連番4桁>` 例: `SPEC-0100-0001`
領域番号は `specs/_registry.md` 内の領域一覧を参照して割り当てる。新しい領域を追加する場合はspec-writerが `specs/_registry.md` に領域番号を追記する。

## 階層化
仕様は「領域 > 機能 > 詳細仕様」の3階層を基本とする。1ファイルが長大になる場合は詳細仕様ごとにファイルを分割し、親Spec IDを明記する。ディレクトリ構成は `specs/<領域>/SPEC-xxxx-xxxx-<title>.md`。

## ステータス遷移
DRAFT → IN_REVIEW → APPROVED / DEPRECATED

- DRAFT: spec-writer作成直後
- IN_REVIEW: spec-reviewerの検証完了、または人間レビュー待ち(PENDING_HUMAN_REVIEWと併記)
- APPROVED: 人間が承認しベースライン化。以降の変更は新しいDRAFT仕様として差分管理し、旧版はAPPROVEDのまま残すか、置き換え後にDEPRECATEDにする。
- DEPRECATED: 廃止。理由と置き換え先Spec IDを記載する。

**APPROVEDへの遷移は必ず人間が行う。Agentは自動でAPPROVEDに変更しない。** architecture-mapper以降の工程は、APPROVED済みのSpecのみを正式入力として扱う。

## 重複・矛盾・未決定の起票(specs/_issues.md)
- 種別は「重複」「矛盾」「未決定」の3つに分類する。
- 関連するSpec IDを必ず記載する。
- ステータスは OPEN → PENDING_HUMAN_REVIEW → RESOLVED。RESOLVEDは判断者名を記録した上で人間が行う。Agentは検出・起票のみを行い、自分で解決しない。
