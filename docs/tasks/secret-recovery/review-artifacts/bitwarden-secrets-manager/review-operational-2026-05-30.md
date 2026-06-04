# Bitwarden Secrets Manager 運用整合レビュー記録（2026-05-30）

- レビュー役割: `運用整合レビュー担当`
- 対象作業項目: `Bitwarden Secrets Manager`（GitHub Issue #30 / PR #33）
- 対象ブランチ: `refactor/secrets-structure-issue-30-main`
- 現行 HEAD: `a1a36cc`（作業ツリー clean）
- 実際の対象差分: `5ff5e54..a1a36cc`
- 参照: `docs/task-governance/workflow.md`、`docs/task-governance/implementation-review-judgement.md`、`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`、`docs/tasks/tasks.md`、`docs/tasks/secret-recovery/tasks.md`、`docs/tasks/secret-recovery/review-artifacts/bitwarden-secrets-manager/confirmation.md`、`docs/tasks/secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md`

## 判定

判定: 要修正
判定要約: 記録された `実装/レビュー対象終端`（`77dc03c` / `5ff5e54..77dc03c`）が、HEAD `a1a36cc` に実在する fresh review 未対象の実コード commit（`3e82eac`・`a38d4d7`・`6aceaf0`）を覆っておらず、fresh review gate が検証対象終端を誤認しうる監査可能性の懸念がある。
根拠:

- **強制可能（合格）と判定した点**:
  - 役割分離・ゲート条件・完了判定ロジックは構造として正しく gate されている。`docs/tasks/secret-recovery/review-artifacts/bitwarden-secrets-manager/review.md` の集約判定は `集約後レビュー判定: 未確定`、`差戻し事項: なし`、`commit / push は未実施` であり、fresh review 未実施を `合格` や commit gate 充足として偽装していない。
  - root ledger / area ledger の状態は `進行中（Hypatia 後 fresh review 待ち）`、固定実装単位トラッカーの `レビュー` は `未実施（Hypatia 後 fresh review 必須）` であり、完了判定ロジックが前倒しで満たされていない。
  - 作業定義文書 `完了の判定条件（監査再現）` は、必須レビュー担当集合（実装差分 7 役割 + 文書整合差分があるため参照整合）の個別判定と `集約後レビュー判定: 合格` を完了条件に課しており、現状はそれを満たしていないことが監査可能に記録されている。
  - 対象差分の再特定自体は可能（branch + base `5ff5e54` + `git log` の HEAD `a1a36cc` から `5ff5e54..a1a36cc` を復元できる）。差分識別子文言が終端 `77dc03c` に留まっていること自体は、補助記録の遅れであり単独の不合格根拠にはしない。

- **要修正と判定した具体的懸念（監査可能性）**:
  - `docs/task-governance/workflow.md` は `実装/テスト差分の保存コミット終端` を「実コード・テストコード・実行検証の対象範囲を固定する commit hash。レビュー担当が実装/テスト差分を確認する場合、この終端を検証対象終端として扱う」と定義する。これは補助記録（review artifact / ledger の文言 exact 同期）とは区別される load-bearing なフィールドである。
  - work-item・root ledger・area ledger・confirmation の 4 文書すべてで、この `実装/レビュー対象終端` は `77dc03c` / `diff range 5ff5e54..77dc03c` に固定されている（`docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md:5,6,10`、`docs/tasks/tasks.md:115`、`docs/tasks/secret-recovery/tasks.md:75,140`、confirmation.md:9）。
  - しかし HEAD `a1a36cc` には `77dc03c` より後に 3 件の実コード commit が存在し、`git merge-base --is-ancestor 6aceaf0 77dc03c` は偽（`5ff5e54..77dc03c` の外）である:
    - `3e82eac fix(secrets): port別internal stub datastoreへ分離`（`internal_stub.rs` +173/-、`selected_device.rs`、`secrets_cli.rs` を大幅改変、旧 `tests/secrets_internal_stub/cli_stub_state.rs` 削除）
    - `a38d4d7 fix(secrets): internal stub contractを単一定義へ寄せる`（`secrets_internal_test_stub_contract.rs`、`lib.rs`、`bw.rs`、`yubikey.rs` を改変）
    - `6aceaf0 fix(secrets): internal stub observationをstdoutへ移す`（同 stub/contract/test の executable 挙動変更）
  - これらは executable behavior / test を変える実コード差分であり、文書 only の補助記録更新ではない。よって「補助記録の hash・file-set・current-cycle 文言の exact 同期不足」という単独理由ではなく、`実装/レビュー対象終端` という検証対象終端そのものが実コード状態より古い、という具体的な監査ギャップである。
  - confirmation には対応する確認節（`internal backend stub 独立化確認（2026-05-30）`、`PR #33 Codex review remediation 確認（2026-05-30）`、`PR #33 stdout observation remediation 確認（2026-05-30）`）が存在し、`cargo test --features secrets-internal-test-stub --test secrets_cli`（25 passed）・`cargo xtask check`・`git diff --check` 等の実検証は記録されている。この点で必要な確認結果の実体は現行差分を追跡している。ただしこれらの節は `2026-05-30-port-stub-independent-datastore-worktree` 等の worktree ラベルのみで識別され、commit 終端へ固定されていない。docs 内に `6aceaf0` / `a1a36cc` を参照する記録は存在しない（`git grep` 結果空）。
  - 結果として、次工程の fresh review が canonical な `実装/レビュー対象終端 77dc03c` を検証対象終端として起動された場合、`3e82eac`・`a38d4d7`・`6aceaf0` の実コード変更を検証対象外として取りこぼす構造的リスクがある。これは fresh review gate の監査可能性に対する具体的懸念であり、`スコープ外` や `運用徹底` を理由に格下げできない。

## 解消条件（remediation target）

- canonical な `現行サイクル差分識別子` / `実装/レビュー対象終端` を、実コード commit を覆う実際の終端（現行 HEAD `a1a36cc`、実コード終端は少なくとも `6aceaf0`）まで更新し、`3e82eac`・`a38d4d7`・`6aceaf0` を current-cycle の実装/テスト差分として fresh review 検証対象終端に含められるようにする。文言の自己 hash 固定は不要だが、検証対象終端が実コード状態より古いまま据え置かれない状態にすること。
- confirmation の 3 件の post-`77dc03c` 確認節を、worktree ラベルだけでなく現行 commit 終端から再特定できる形にひも付ける（`git log` 終端で足りる）。
- 上記は実装担当へ差し戻して是正する。運用整合レビュー担当は判定返却のみを行い、文書編集・実装は行わない。

## 補足（fail にしなかった点の明示）

- 差分識別子文言が `77dc03c` に留まっていること「だけ」を不合格根拠にはしていない。不合格ではなく `要修正` とした理由は、(1) 対象差分が再特定可能であり、(2) 必要な実検証の実体は confirmation に追跡されている一方で、(3) 検証対象終端フィールドが実コード commit を覆っておらず fresh review gate の監査可能性に具体懸念が残るためである。
- review/集約状態は内部的に整合（`未確定` を `合格` と併記していない）であり、役割分離・完了判定ロジックの強制可能性自体には別途の懸念を認めない。
