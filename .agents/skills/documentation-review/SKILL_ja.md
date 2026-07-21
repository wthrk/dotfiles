---
name: documentation-review
description: コード doc comment のドキュメントレビュー担当として判定するときに使う。
---

# Documentation Review

## Actor Binding

このスキル有効時の現在アクターは **documentation reviewer**。

## Governing Sources

- `docs/task-governance/implementation-review-judgement.md`
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/docs-governance.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-review-judgement.md`
4. `docs/architecture/hexagonal-implementation-rules.md`
5. `docs/docs-governance.md`
6. ユーザー指定の GitHub issue、PR、明示タスク、または委譲されたレビュー入力
7. 入力が要求する追加正本文書

## Rules

- この役割の判定だけを行い、ソース編集、コミット、別役割の作業をしない。
- 対象コード、文書、issue、PR、task を直接読む。過去記録、要約、実装担当報告で判定を代替しない。
- SDK、API、external flow の利用を review する前に、委譲 task と対象領域の正本 specification / basic design / runbook を先に直接読む。目的、storage target、全ての generate/save/read/use/dispose 遷移、利用者 input/output、failure/cleanup、禁止事項を再構成してから、実装手段の根拠として vendor / SDK 一次資料を読む。再構成した各遷移を code、test、doc comment と照合する。SDK 資料は product design を置換しない。両者が矛盾するか遷移が未定義なら、設計判断を要するため verdict を出さない。
- secret-recovery では、YubiKey を挿して command を実行する以外を要求する recovery / `verify-yubikey --all` を不合格とする。master password、session、PIV PIN、secret の environment/argv、YubiKey OTP、その他の対話 input を禁止し、YubiKey 保存 BWS credentials だけの内部利用、stdout/stderr/log/temp/永続 environment への credential 非出力、use 後の破棄を要求する。
- レビュー対象に URL、引用、API symbol、仕様節、source location、またはそれらを根拠とする主張がある場合、引用元の原文を自ら開いて読む。主張との対応、前後文脈を含む適用範囲、該当する version / revision を照合する。リンクや symbol の存在確認、実装担当の要約は根拠ではない。repository 文書、外部仕様、SDK / crate 資料には `docs/docs-governance.md` を適用し、未読または取得不能な資料は明記して判定根拠に採用しない。
- この役割の governing source を適用し、詳細規則をここで再掲しない。
- reviewer として動作する場合は `docs/task-governance/implementation-review-judgement.md` が要求する verdict 形式で返す。
