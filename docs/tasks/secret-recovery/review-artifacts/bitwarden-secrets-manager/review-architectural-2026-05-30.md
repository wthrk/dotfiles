# アーキテクチャ整合レビュー（BSM）2026-05-30

- 対象: `rust/dotfiles-cli/src/secrets/`（モジュール全体通読）
- リポジトリ/ブランチ: `/Users/ya/works/dotfiles` / `refactor/secrets-structure-issue-30-main` / HEAD `a1a36cc`
- 作業項目: `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`（PR #33 / Issue #30）
- 役割スキル: `.agents/skills/architectural-consistency-review/SKILL.md`（Required Reading Order 準拠）

判定: 合格
判定要約: 所見なし。secrets モジュールは entrypoint→application→domain/port/adapter/support の責務分配が一貫した1つの設計を表現しており、BWS 経路の業務判断（固定 key/name の意味づけ、一意解決、0件/複数件 failure、外部確認 plan）は domain/application に置かれ、support/adapter へ寄せられていない。internal backend stub は canonical 条件（同一 production command path、同一 port 契約、compile-time selection、test 側 datastore 観測限定、BWS/YubiKey port stub 独立）を満たす。

根拠:

## 全体一貫性の問い（Step 2）への回答

- モジュール構造は1つの coherent な設計か、ルール通過の部品の山か:
  coherent な設計である。`secrets.rs` の composition root が adapter concrete を `RuntimePorts::production()` 1経路で所有し、`entrypoint/dispatch.rs` が command 選択→use case 呼び出しだけを担い、各 `application/run_*.rs` が port を順序適用し、`domain/` が値・規則・summary を、`ports/` が capability 契約を、`adapters/` が SDK 翻訳を、`support/protection/` が secret 保護境界を担う。各層が役割で噛み合っており façade 二重化や疑似レイヤーは無い。

- 責務が層をまたいで一貫分配されているか:
  BWS の対象同一性・一意解決・0件/複数件 failure は `domain/bws.rs`（`BwsSecretName::key/resolve_id`、`BwsProjectName::resolve_id`、`resolve_single_bws_lookup`）に1箇所で集約。`verify-yubikey --check bws` の外部確認 plan（project 解決→secret 一覧→必須 secret 取得の順序）は `application/run_verify_yubikey_with.rs` が保持し、必須 secret 集合は `domain/verification.rs::required_bws_secrets` が domain plan として固定する。同種責務が複数層へ散っていない。

- 層関係（依存方向・責務境界）が全体として意味をなすか:
  `ports/bw.rs` は `list_bws_projects`/`list_bws_secrets`/`fetch_bws_secret_by_id` の capability だけを宣言し SDK 型・UUID を露出しない。`adapters/bw.rs` は SDK list/get を port 境界型へ翻訳するだけで lookup 判定を再実装しない。`support/protection/bws.rs` は SDK login の所有 token buffer 作成・zeroize・返却 secret の保護値化という保護境界操作に限定。`domain`/`application`/`ports` に `bitwarden`/`uuid` の import が無いことを grep で確認。

- 機械的分離に依存していないか / 薄い port・adapter のため業務判断を support へ寄せていないか:
  寄せていない。`support/protection/bws.rs` の doc/実装とも「lookup rule や 0件/複数件の failure 化を扱わない」「project/secret lookup rule や外部確認 plan は扱わない」と明示し、実際に固定 name の意味づけ・一意解決・check plan を持たない。固定 project/secret name の値は domain に置かれ、support は SDK buffer 境界のみ。

- 有能なアーキテクトが coherent と呼ぶか:
  呼ぶ。各 `run_*.rs` が 1 use case = 1 関数で sibling 配置され、`application.rs` は module 宣言のみの薄い root（façade ロジック無し）。port は capability 単位（input/output/report を別 trait）で切られ、adapter module 分割と対応している。

- 新規 use case/adapter を自然に受け入れる構造か:
  受け入れる。新 backend は `ports/` に capability trait、`adapters/` に翻訳実装、必要なら `support/protection/` に保護境界操作を追加し、composition root の `RuntimePorts` へ1本注入する既存パターンへ素直に乗る。

## internal backend stub（canonical 条件）の全体整合確認

- 同一 production command path: stub は `adapters/bw.rs` 内 `#[cfg(feature = "secrets-internal-test-stub")]` で real SDK 実装と排他に `BwsClientPort` を実装し、composition root・dispatch・use case は無変更。runtime real/stub 分岐は無く compile-time selection のみ。
- 同一 port 契約: `internal_stub.rs` は `BwsClientPort` の 3 method をそのまま実装し、domain/business logic を stub へ移していない（lookup 判定は application/domain のまま）。
- test 側 datastore 観測限定: stub は private `BWS_DATASTORE`（OnceLock）に spec から初期展開し、最終状態を `STUB_OBSERVATION_PREFIX` 付き stdout sentinel として出力。`secrets_internal_test_stub_contract.rs` は env 名と prefix のみを公開し、backend schema/state helper を外へ出さない。
- BWS/YubiKey port stub 独立: BWS は `BWS_DATASTORE`/`BWS_STUB_SPEC_ENV`、YubiKey は `selected_device.rs` の `YUBIKEY_DATASTORE`/`YUBIKEY_STUB_SPEC_ENV` と、state・schema・spec env を共有せず独立。共通巨大 StubState や共有 state file による結合は無い。
- 上記より、`adapter 配下に test 専用 backend stub があること自体` を全体非整合の根拠にしない（SKILL / 判定文書の規定どおり）。same-route 不成立・配置+責務不一致・単一 command path 破綻・runtime 分岐・test 側 backend state 保持・port stub 結合のいずれも全体文脈で確認されない。
