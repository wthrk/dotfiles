# #13 Bitwarden Secrets Manager クライアント

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `Bitwarden Secrets Manager` 取得経路を、secret-recovery の層分割と外部境界規約に沿って実装する。
- 現行サイクル差分識別子: `PR #33 / branch refactor/secrets-structure-issue-30-main / base 5ff5e54 / 実装/レビュー対象終端 77dc03c / diff range 5ff5e54..77dc03c`
- 現行サイクル確認基準: `5ff5e54..77dc03c`（PR #33 作り直し commit `2ececf1` と、補正 commit `ffe9880`、`7320c55`、`fbc5096`、`fa396f3`、`ae1b917`、`97748c4`、`5e21afb`、`4cd47d4` を対象にする。`97748c4` は BSM 対象コードパス漏れ指摘への対応、`5e21afb` は PR #33 現行 HEAD 証跡更新、`4cd47d4` は削除済み adapter root を現行対象パス扱いしない台帳補正）
- 履歴内訳（PR #33 current-cycle）: `11ff088` は直前 P1 対応 commit、`77dc03c` は fresh review 差し戻し（構造・PTY・追跡更新）対応 commit
- 履歴サイクル差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`（旧 BSM Hypatia サイクル。PR #33 / Issue #30 現行サイクルの合格根拠として扱わない）
- 現行サイクルレビュー scope: BSM 実装レビュー対象は、本作業項目の対象コードパス、BSM へ直接関係する文書差分、必須レビュー結果、必要な実検証で判断する。旧 Hypatia サイクルや BSM scope 外の `.agents/skills/`、`AGENTS.md`、`docs/task-governance/`、repo-governance/YubiKey 証跡などの文書差分を、BSM current-cycle のレビュー合格根拠・commit 着手 gate の充足根拠・不充足根拠にしない。対象パス exact list、confirmation/review artifact、root/area 台帳、current-cycle 文言の完全同期は補助記録であり gate ではない。
- 実装/テスト差分の保存コミット終端: `実装/レビュー対象終端 77dc03c`（PR #33 の現行保存済み commit 終端。fresh review 未実施・集約未確定であり、保存済み commit 終端だけをレビュー合格や commit gate 充足の根拠として扱わない）
- 構造完了条件:
  - SDK 呼び出しは adapter / port 境界へ隔離する。
  - secret の保護境界、protection 内操作、BWS SDK 呼び出し境界は [`docs/secret-recovery/secret-handling.md`](../../../secret-recovery/secret-handling.md) に適合させる。
  - app 層の use case orchestration test は `secrets-internal-test-stub` / internal test stub feature と切り離す。app 層 production code や app 層 inline test に internal stub feature gate / bridge を置かず、`tests/` 配下の app test support も参照しない。`rust/dotfiles-cli/src/secrets/application/app_test_support.rs` のような app 層共有 test support file は禁止する。必要な app 層テストは各 `run_*.rs` の `#[cfg(test)] mod tests` 内で port trait から生成した `mockall` mock を直接組み、event recorder、巨大な状態管理 harness、port trait と別に動くテスト専用実装を作らない。既存 port trait の method を `mock!` macro へ手で書き写して二重管理してはならない。
  - `ProtectedSecret` の secret 生値アクセスは公開 API にしない。ただし `#[cfg(test)]` / `#[test]` に閉じた最小アクセス関数は test-only 観測口として許可する。この許可を `String` 変換公開や production 経路での取り出し許可に拡大しない。
  - storage backend が暗号化・復号・sealed blob を内包する場合、port は datastore capability を公開し、暗号化・復号・sealed blob 操作は backend 内部機能として隠蔽する。support に置けるのはその技術境界に限り、setup 判定、必須 secret 判定、一意解決、0件/複数件 failure、外部確認 plan は support へ移さない。
  - 固定 project / secret name の意味づけ、secret ID の一意解決、0件/複数件の failure 化、取得対象の過不足判定、`verify-yubikey --check bws` の外部検証 plan は、[`docs/architecture/hexagonal-implementation-rules.md`](../../../architecture/hexagonal-implementation-rules.md) と [`docs/secret-recovery/bitwarden-secrets-manager-design.md`](../../../secret-recovery/bitwarden-secrets-manager-design.md) が規定する責務境界に置く。`support` への移動、ファイル分割、private helper 削除だけで解消扱いにしない。
  - `application` は secret recovery の順序制御だけを持つ。
  - `domain` は Bitwarden SDK 型と I/O 型へ依存しない。
- 既存実装の流用方針: `規約に合う部分だけを流用し、境界違反が残る場合は再分割を優先する。`
- 規約違反の解消対象:
  - SDK 依存の境界漏れ
  - application と domain の責務混在
  - entrypoint / adapter / domain の直接結合
- レビュー合格条件: `Bitwarden SDK 依存が境界内へ閉じ、アーキテクチャ規約違反が残っていないこと。`

## 完了の判定条件（監査再現）

- 注記: 本節は完了判定時に満たすべき実質条件を定義する。confirmation/review artifact の整合や current-cycle 文言同期そのものを完了条件にしない。
- 未コミット worktree を含む対象差分を再特定できること。
- BWS 外部確認経路（`verify-yubikey --check bws` 相当の application 経路を含む）、application 層の `secrets-internal-test-stub` bridge 除去、app 層残存検索など、作業項目の実質条件に対応する確認結果があること。
- 実装差分の必須レビュー担当集合（構造、運用整合、セキュリティ、仕様適合、テスト、ドキュメント、アーキテクチャ整合。文書整合差分を含む場合は参照整合を追加）の個別判定および `集約後レビュー判定: 合格` が揃うこと。
- 修正済み PR review comment は、fresh review 全員合格、集約合格、commit gate 充足、commit/push 完了後にだけ返信して resolve/close する。誤検出と判断した comment は説明返信し close しない。これは PR 運用上の実作業であり、補助記録同期で代替しない。
- PR #33 現行サイクルでは、ユーザー依頼の PR AI review 対応として fresh review/集約/commit gate 確定前に一部 PR review comment への返信または resolve を先行実施した。この先行実施は PR 運用記録として追跡し、repository governance 上の fresh review 全員合格、集約合格、commit gate 充足、最終完了扱いの根拠にはしない。
