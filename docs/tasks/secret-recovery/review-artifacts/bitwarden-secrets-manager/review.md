# Bitwarden Secrets Manager レビュー記録

この文書は `docs/tasks/secret-recovery/tasks.md` の作業項目 `Bitwarden Secrets Manager` に対する固定実装単位 `レビュー` の記録先である。

## BSM 作業項目履歴からの引き継ぎ（PR #33 現行サイクル対象外）

- レビュー状態: `旧 BSM Hypatia サイクル履歴（fresh review 完了・集約済み）`
- 判定位置づけ: `PR #33 / Issue #30 task-list-outside 現行サイクルとは別の旧 BSM 実装差分サイクル履歴。PR #33 の合格根拠、fresh review 完了、集約済み判定として再利用しない。`
- 対象ブランチ: `feat/bitwarden-secrets-manager`
- 確認開始時点参照: `../../work-items/bitwarden-secrets-manager.md` 記載の `現行サイクル差分識別子`
- 対象差分識別子: `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7`
- 比較範囲: `HEAD` = `dccada7` を基点にした未コミット worktree 差分（未コミット tracked diff と未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む）。保存済み commit 終端そのものを review 対象終端とは扱わない。
- レビュー scope: BSM 実装レビュー対象は、本作業項目の対象コードパス、BSM へ直接関係する文書差分、必須レビュー結果、必要な実検証で判断する。同じ未コミット worktree に残るその他の `.agents/skills/`、`AGENTS.md`、`docs/task-governance/`、repo-governance/YubiKey 証跡などの文書差分は対象外差分として扱い、BSM current-cycle のレビュー合格根拠・commit 着手 gate の充足根拠・不充足根拠にしない。対象パス exact list、confirmation/review artifact、root/area 台帳、current-cycle 文言の完全同期は補助記録であり gate ではない。
- 実装側確認証跡: `./confirmation.md`
- 差戻し解消メモ（2026-05-28 実装担当追記）:
  - required evidence `verify-yubikey --check bws` を `confirmation.md` へ追記済み。
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の責務混在は、`device_serial_adapter.rs` + `process_io_adapter.rs` + `storage_adapter.rs` + `report_adapter.rs` へ分割して解消済み。
  - `BwsClientAdapter` は通常ビルドで Bitwarden Secrets Manager SDK（`bitwarden` crate）経路を持つ実装へ更新し、`bws external check is not available in this build` 固定失敗を除去済み。
- 差戻し解消メモ（2026-05-28 remediation 追記）:
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の `pub(crate) mod` を private `mod` 化し、adapter 公開面を縮小した。
  - 同ファイルの内部境界（`RealDeviceIo`、`YubikeySecretDevice`、`RealDeviceAdapter`、`YubikeyPinVerifier`、`open_by_serial`、`wrap_content_key`、`unwrap_content_key`）へ責務境界 doc comment を追加した。
  - `confirmation.md` 参照は行番号固定を廃止し、`確認手順と結果` 節のコマンド記録参照へ統一した。
- 差戻し解消メモ（2026-05-29 実装担当追記）:
  - ユーザー訂正により、`support/protection` は secret 保護境界の backend 実装として product/service-specific な専用操作を持てることを architecture / secret-handling 文書へ反映した。`support/protection/bws.rs` を product-neutral でないことだけで層違反とする前回 structural / architectural 指摘は、この訂正後の再レビュー対象として扱う。
  - `secrets-internal-test-stub` feature による production module への `include!` 差し替えを除去し、production build は feature 有効時も実 adapter module を compile する形へ戻した。
  - `rust/dotfiles-cli/src/secrets/adapters/piv_io.rs` の `SecretDeviceIo`、`SelectedDeviceDiscoveryIo`、`YubikeySecretDevice` に責務境界と caller responsibility の doc comment を追加した。
- 差戻し解消メモ（2026-05-29 実装担当追加再修正）:
  - `rust/dotfiles-cli/src/secrets/application.rs` の test-only bridge と、`app_test_support` を使う application unit tests を `#[cfg(all(test, feature = "secrets-internal-test-stub"))]` に限定した。
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` の SDK response value handling は、受け取った `String` を直後に `Zeroizing<String>` へ移してから `SecretSession::start()` と `ProtectedSecret` 確保へ進む順序へ修正した。
  - root ledger、area ledger、confirmation の対象コードパスに、当時の current-cycle 実差分を補助記録として追記した。この整合自体は現行 gate として扱わない。
- 差戻し解消メモ（2026-05-29 実装担当再レビュー後追記）:
  - `with_access_token_login_request` の login request buffer は Drop で `request.access_token` を zeroize する guard に保持し、await 後の明示 zeroize へ依存しない形へ修正した。
  - `BwsClientPort` 実装内の SDK 認証・project/secret 一意解決・保護値化 flow と、`piv_io.rs` の `ProtectedSecret` 取り扱い境界へ doc comment を追加した。
  - `application.rs` と app test support の test-only bridge コメントへ、`secrets-internal-test-stub` feature と xtask/internal test 専用経路を明記した（後続差分で app 層から `tests/` 配下への bridge は削除済み）。
  - root ledger、area ledger、confirmation の対象コードパスへ application `run_*.rs` の実差分と `rust/dotfiles-cli/tests/secrets_cli.rs` を反映した。
- 差戻し解消メモ（2026-05-29 実装担当 operational 再修正）:
  - root ledger、area ledger、confirmation の対象コードパスへ `git diff --name-only` に含まれる削除ファイル（`bws_client_real.rs`、`bws_client_stub.rs`、`device_selection.rs`、`selected_device_real.rs`、`selected_device_stub.rs`、`secret_consumer.rs`）を削除差分として追記した。
- 旧集約メモ（2026-05-29 実装担当追記・現行判定対象外）:
  - このメモは後続の実装継続差分より前の履歴であり、現行差分の fresh review 合格根拠として扱わない。
  - 後続の実装継続差分に対する fresh review では structural / security / operational-consistency / documentation / reference-integrity に未合格指摘があった。James 是正後の現行差分では、本記録末尾の `未判定` を正本とする。
- 実装継続メモ（2026-05-29 delegated implementation executor 追記）:
  - `run_verify_yubikey_with` の BWS project/secret lookup failure application tests を追加し、fetch failure test と合わせて failed report 収束を確認できるようにした。
  - 当時の共有 app test support 方針は後続差分で撤回済み。現行実装参照として扱わない。
  - `adapters/bws_client.rs` の BWS helper 境界 doc comment を補足した。
  - focused verification は `confirmation.md` の `実装継続確認（2026-05-29）` に追記済み。この実装継続差分に対する fresh review は未実施。
- fresh review 未合格メモ（2026-05-29 delegated implementation executor 追記）:
  - structural: `secrets.rs` entrypoint の adapter concrete 生成と `RuntimeBoundary` の port 実装集約が不合格。
  - security: YubiKey storage 読み出しと BWS SDK login/list/get 境界前に process-wide core dump 抑止が成立しない経路があり要修正。
  - operational: 再レビュー未完了にもかかわらず root/area 台帳と review artifact が合格/commit前を示しており要修正。
  - documentation: port trait/doc comment は現行 API（`PortFuture` 非依存）に合わせて再確認が必要。
  - reference-integrity: skill の正本詳細再掲、`SKILL.md` / `SKILL_ja.md` 意味同期、過去レビュー記録の固有ツール名残存が不合格。
  - specification / test / architectural-consistency は当該 fresh review では合格扱い。ただし本是正後は再レビューが必要。
- James 是正後 fresh review 前メモ（2026-05-29 補助記録追記）:
  - structural finding 対応として、`rust/dotfiles-cli/src/secrets/adapters/bws_client.rs` の `fetch_bws_secret_by_id` から `secrets().get()` と `secret.value` の受け取りを除去し、adapter 側は BWS secret ID を protection 境界へ渡す構造へ変更された。
  - `rust/dotfiles-cli/src/secrets/support/protection/bws.rs` に `get_protected_secret_value(id)` を置き、BWS SDK get と返却 value の `ProtectedSecret` 化を protection 境界内で完了する構造へ変更された。
  - 固定 secret key/name、一意解決、0件/複数件 failure 化、`verify-yubikey --check bws` 相当の外部確認 plan は support へ移していない。
  - 確認済みコマンドは `confirmation.md` の `James 是正後 fresh review 前確認（2026-05-29）` を参照する。
  - James 是正後 current-cycle の対象差分識別子は `2026-05-29-james-current-cycle-worktree@HEAD-dccada7` である。`HEAD dccada7` を基点にした未コミット worktree 差分を対象にし、未追跡 `rust/dotfiles-cli/src/secrets/runtime.rs` を含む。
  - 同一未コミット worktree には BSM 対象コードパス外の文書差分も残っている。BSM current-cycle scope は本記録冒頭の `レビュー scope` に限定し、対象外差分は BSM review/commit gate から除外する。除外根拠は、当該差分が BSM 作業項目の対象コードパス/証跡/必要 skill 修正に含まれず、別作業項目または過去/並行文書是正の残差として扱われるためである。
  - この差分は fresh review 未実施であり、下記の前回レビュー担当判定や旧集約結果を James 是正後差分の合格根拠として扱わない。
- Hegel/Linnaeus 後 fresh review 前メモ（2026-05-29 補助記録追記）:
  - Hegel 文書補正により、storage backend が暗号化・復号・sealed blob を内包する場合、port は datastore capability を公開し、support/protection は backend 内部の暗号化・復号・sealed blob・protection・zeroize・core dump 保護などの技術境界に限定することを正本へ同期した。
  - setup 判定、必須 secret 判定、固定 key/name/role の意味づけ、一意解決、0件/複数件 failure、取得対象の過不足、外部確認 plan は support に逃がせない基準として正本へ同期した。
  - Linnaeus 実装補正により、application 層の `secrets-internal-test-stub` bridge と feature-gated inline tests は除去済み。`runtime` module 参照は残さず、`entrypoint` composition root 境界へ収めた。
  - `piv::decrypt_data(...)` の `Zeroizing<Vec<u8>>` unwrap 境界は `support/protection/sealed_blob.rs` へ移動済み。`adapters.rs` の module comment、`report_adapter.rs` の external output emit 境界、`StorageAdapter::inspect_secret_storage_setup` の raw 観測値取得境界も Linnaeus 後状態として同期した。
  - 確認済みコマンドは `confirmation.md` の `Hegel/Linnaeus 後 fresh review 前確認（2026-05-29）` を参照する。
  - Linnaeus 後 current-cycle の対象差分識別子は `2026-05-29-linnaeus-current-cycle-worktree@HEAD-dccada7` である。`HEAD dccada7` を基点にした未コミット worktree 差分を対象にし、未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む。
  - この差分は fresh review 未実施であり、下記の前回レビュー担当判定や旧集約結果を Linnaeus 後差分の合格根拠として扱わない。
- Aristotle 後 fresh review 前メモ（2026-05-29 補助記録追記）:
  - `rust/dotfiles-cli/src/secrets/application.rs` から、実装本体を持たない module root への usecase 単位テスト集約を除去した。
  - 元の app usecase test は各 `run_*.rs` の実装ファイルへ戻し、順序制御、停止条件、port 呼び出し確認を `secrets-internal-test-stub` / internal test stub feature から切り離した。
  - app 層から `tests/` 配下の app test support へ向かう `#[cfg(test)]` bridge を削除した。
  - 当時の共有 helper / event expectation 方針は後続差分で撤回済み。現行実装では port trait 由来の `mockall` mock を各 `run_*.rs` の test 内で直接使う。
  - `ProtectedSecret` の test-only 最小アクセス関数は support/protection 側に閉じ、production の `String` 変換や平文取り出し API は追加しない。
  - 確認済みコマンドは `confirmation.md` の `Aristotle 後 fresh review 前確認（2026-05-29）` を参照する。
  - Aristotle 後 current-cycle の対象差分識別子は `2026-05-29-aristotle-current-cycle-worktree@HEAD-dccada7` である。`HEAD dccada7` を基点にした未コミット worktree 差分を対象にし、未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む。
  - この差分は fresh review 未実施であり、下記の前回レビュー担当判定や旧集約結果を Aristotle 後差分の合格根拠として扱わない。文書差分を含むため参照整合レビューを必須レビュー集合に含める。
- Hypatia 後 fresh review 前メモ（2026-05-29 補助記録追記）:
  - app 層から `tests/` 配下の app test support へ向かう `include!` bridge を削除した。現行方針では `rust/dotfiles-cli/src/secrets/application/app_test_support.rs` も禁止対象であり、現存実装として扱わない。
  - `mockall` は port trait 側の test-only `automock` から生成した `Mock*Port` を各 `run_*.rs` の `#[cfg(test)] mod tests` 内で直接組み立てる方針で扱う。`MockAppEventExpectation` や event recorder / shared harness は現行実装として扱わない。
  - `secrets-internal-test-stub` feature gate / bridge は app 層 production code / inline test / app test helper へ置いていない。
  - `ProtectedSecret` の test-only 最小アクセス関数は support/protection 側に閉じ、production の `String` 変換や平文取り出し API は追加しない。
  - 確認済みコマンドは `confirmation.md` の `Hypatia 後 fresh review 前確認（2026-05-29）` を参照する。
  - Hypatia 後 current-cycle の対象差分識別子は `2026-05-29-hypatia-current-cycle-worktree@HEAD-dccada7` である。`HEAD dccada7` を基点にした未コミット worktree 差分を対象にし、未追跡 `rust/dotfiles-cli/src/secrets/entrypoint.rs` を含む。
  - この差分は fresh review 未実施であり、下記の前回レビュー担当判定や旧集約結果を Hypatia 後差分の合格根拠として扱わない。文書差分を含むため参照整合レビューを必須レビュー集合に含める。

## current-cycle 必須レビュー担当（実装差分 7 役割 + 参照整合レビュー、計 8 役割）

- `構造レビュー担当`
- `運用整合レビュー担当`
- `セキュリティレビュー担当`
- `仕様適合レビュー担当`
- `テストレビュー担当`
- `ドキュメントレビュー担当`
- `アーキテクチャ整合レビュー担当`
- `参照整合レビュー担当`（文書整合差分を含むため追加）

## レビュー担当チェック項目

1. 対象作業項目のスコープ外へ越境していないこと。
2. 責務境界と依存方向が [レビュー観点チェックリスト](../../../../architecture/review-checklist.md#レビュー観点チェックリスト構造) の観点に適合していること。
3. 対象 work-item の `レビュー合格条件` を満たすこと。
4. 仕様・設計・作業定義文書の要求挙動、停止条件、成功条件が反映されていること。
5. 文書構造の整合だけでなく、必須の実行手順・役割分離・ゲート条件・必要な確認結果・完了判定ロジックについて、実運用での強制可能性または監査可能性に具体的懸念がないこと。補助記録の exact 同期不足だけを懸念にしない。懸念がある場合は必ず所見化し、強制可能性/監査可能性が不確実なまま `合格` にしないこと（`スコープ外` や `運用徹底` を理由に格下げしない）。
6. 具体的懸念、残留リスク、未解消疑義、要追跡事項、運用依存の注意事項を記録した場合、それは `finding あり` であり、`No findings` / `指摘なし` / `懸念なし` / `合格` と併記しないこと。判定は少なくとも `要修正` とし、解消条件または差戻し事項を同じ記録に書くこと。

## 判定記録フォーマット

- 各レビュー担当の記録は、必ず次の順で書く。
  1. `判定: <合格|要修正|不合格>`
  2. `判定要約: <所見なし|主要論点要約>`
  3. `根拠:`
- `判定` に使ってよいラベルは `合格`、`要修正`、`不合格` のみとする。
- `通しません`、`No findings`、`指摘なし`、`no blockers`、`pass` などの自由文を `判定` の代わりに使ってはならない。
- `合格` の場合、`判定要約` は `所見なし` とする。
- `要修正` または `不合格` の場合、`判定要約` は主要論点を 1 行で要約し、`根拠:` に差戻し条件または不合格理由を箇条書きで記録する。
- 集約判定も同じ構造で書き、`集約後レビュー判定`、`集約判定要約`、`集約根拠` を必ず揃える。

## セキュリティ所見記録（必須）

### 1) 秘密値・認証情報の扱い

- 確認状態: `完了`
- 確認対象（ファイル/経路）: `rust/dotfiles-cli/src/secrets/adapters/bw.rs`（`BwsClientAdapter` token handling。旧 `rust/dotfiles-cli/src/secrets/adapters.rs` は現行 `11ff088` tree では削除済み）
- 所見: `token は `ProtectedSecret` 借用で扱い、BWS SDK login request は `support/protection` 内の BWS 専用操作で作成・zeroize する。SDK が所有 plaintext buffer の move を要求する箇所は、借用境界内の呼び出し直前にだけ buffer を作る。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

### 2) 漏えい経路（ログ/引数/一時ファイル/stdout/stderr）

- 確認状態: `完了`
- 確認対象（出力経路）: `Bitwarden SDK 呼び出し失敗時の user-visible error 経路`
- 所見: `secret 値や raw API 応答本文は返さず、SDK 呼び出し失敗を固定要約へ翻訳して返す。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

### 3) 権限境界・永続化・失敗時挙動

- 確認状態: `完了`
- 確認対象（境界/保存先/失敗経路）: `BWS fetch/check の失敗時挙動と永続化有無`
- 所見: `失敗時はエラー返却へ収束し、トークン永続化や追加権限昇格経路は確認されない。`
- 差戻し要否: `不要`
- 未実施理由（未実施時のみ）: `なし`

## 役割別レビュー記録（レビュー担当記入）

注記: 以下の役割別レビュー記録は James 是正前の fresh review 結果である。James 是正後の現行差分は fresh review 未実施であり、必須レビュー担当集合を再起動して個別判定と集約判定を更新する必要がある。

### 構造レビュー担当

- 判定: `不合格`
- 判定要約: `entrypoint の adapter concrete 生成と RuntimeBoundary の複数 port 実装集約が残っている`
- 根拠:
  - `rust/dotfiles-cli/src/secrets.rs` が adapter concrete を直接生成していた。
  - `RuntimeBoundary` が複数 port trait 実装の委譲集約を持っていた。
  - entrypoint から adapter concrete 生成と port 実装集約を除去し、runtime 配線境界へ整理する必要がある。

### 運用整合レビュー担当

- 判定: `要修正`
- 判定要約: `fresh review 未完了の差分が合格/commit前として記録されていた`
- 根拠:
  - `docs/tasks/tasks.md`、`docs/tasks/secret-recovery/tasks.md`、`confirmation.md`、本記録が fresh review 未完了の差分を合格/commit前として扱っていた。
  - 再レビュー前は合格扱いにせず、対象差分と現在のレビュー状態が監査可能な記録へ戻す必要がある。

### セキュリティレビュー担当

- 判定: `要修正`
- 判定要約: `secret 読み取り前の core dump 抑止が BWS/YubiKey 経路で十分に先行していない`
- 根拠:
  - YubiKey storage 読み出し前に process-wide core dump 抑止を確立する必要がある。
  - BWS SDK login/list/get 境界へ入る前に保護済み session/guard が成立する構造にする必要がある。
  - `protect_secret_value` だけで BWS get 後に保護する構造では不十分。

### 仕様適合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、残る Fail は operational-consistency のみと判定された。
  - `support/protection` 内の BWS 専用操作で SDK login request の作成、login 呼び出し、request buffer zeroize を完了する。
  - SDK へ所有権を移した後の内部保持は SDK 側責任とし、repository 側は移譲前の境界、zeroize、ログ非露出を確認対象にする。
  - 旧サイクルでは operational-consistency 再レビュー合格後に review artifact を合格状態へ更新していたが、後続の実装継続差分により現行 fresh review は未合格へ戻っている。

### テストレビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、test は合格と判定された。
  - 2026-05-29 再レビューで、production module への test double/stub 定義混入は解消済み、必要テストも確認済みと判定された。
  - 追加修正後、`secrets-internal-test-stub --lib secrets::application` で application bridge が internal feature 有効時だけ compile/test されることを確認した。
  - `rust/dotfiles-cli/src/secrets/application.rs` と app test support に、`secrets-internal-test-stub` feature と xtask/internal test 専用経路を明記した（後続差分で app 層から `tests/` 配下への bridge は削除済み）。

### ドキュメントレビュー担当

- 判定: `要修正`
- 判定要約: `port doc comment の旧前提記述を現行 API に合わせて更新する必要がある`
- 根拠:
  - `rust/dotfiles-cli/src/secrets/ports.rs` の doc comment は `PortFuture` 前提ではなく、現行 trait 契約の説明として整備する必要がある。

### アーキテクチャ整合レビュー担当

- 判定: `合格`
- 判定要約: `所見なし`
- 根拠:
  - 2026-05-29 再レビューで、support/protection を secret backend implementation と扱う前提は canonical docs と整合済みと判定された。

### 参照整合レビュー担当

- 判定: `不合格`
- 判定要約: `skill の正本詳細再掲、日英 skill 同期、過去レビュー記録の固有ツール名残存に不整合がある`
- 根拠:
  - `SKILL.md` の正本詳細再掲を正本文書参照に寄せる必要がある。
  - 変更済み `SKILL.md` と `SKILL_ja.md` を意味同期する必要がある。
  - 過去レビュー記録内の固有ツール名残存を中立表現へ置換する必要がある。

### 差戻し履歴トレース（current-cycle 引き継ぎ）

- サイクル 1 解消済み:
  - required evidence 不足（`verify-yubikey --check bws`）: `解消済み`
  - root/area 台帳状態不一致: `解消済み`
- サイクル 2 解消済み:
  - 構造: `application.rs` test-only bridge を internal feature 限定へ修正済み。2026-05-29 再レビューで合格。
- サイクル 3 解消済み:
  - セキュリティ: BWS SDK request buffer の panic/unwind 時 zeroize guard は 2026-05-29 再レビューで合格。
  - ドキュメント: BWS adapter flow と PIV protection 変換境界の doc comment は 2026-05-29 再レビューで合格。
  - テスト: application test-only bridge コメントと test double/stub 配置は 2026-05-29 再レビューで合格。
  - 運用: root ledger / area ledger / confirmation の対象コードパスへ削除ファイルを反映済み。2026-05-29 再レビューで合格。
  - 仕様: 必須担当個別判定と集約後レビュー判定を合格として追跡可能な状態へ更新済み。

### 起動不能役割がある場合の記録参照

- 記録参照: `該当なし`

## 集約判定

- 集約後レビュー判定: `未確定`
- 集約判定要約: `fresh review 未実施のため未確定`
- 集約根拠:
  - 現行差分（`5ff5e54..77dc03c`）に対する必須レビュー担当の fresh review は未実施である。
  - 旧サイクルの個別合格・集約合格は履歴情報であり、現行サイクルの合格根拠として再利用しない。
- 差戻し事項: `なし`
- 後続対応状態: `commit gate 記録更新済み（commit / push は未実施）`
- 懸念/残留リスク/未解消疑義/要追跡事項/運用依存の注意事項が1件でも残る場合は `合格` を記録しない。
- 後続対応メモ: `Hypatia 後差分は fresh review 開始前。集約後レビュー判定は未確定。`

## PR #33 / Issue #30 task-list-outside レビュー追跡（2026-05-30）

- 対象位置づけ: `PR #33 / branch refactor/secrets-structure-issue-30-main / Issue #30 の構造整理差分に対する task-list-outside 追跡。Bitwarden Secrets Manager 作業項目の Hypatia 以前の current-cycle レビュー記録とは別に扱い、既存の合格集約を PR #33 の合格根拠として再利用しない。`
- PR #33 現行対象差分: `base 5ff5e54..実装/レビュー対象終端 77dc03c`（この文書-only 補正後の実際の HEAD は `git log` の HEAD で確認する）
- 現行 HEAD 内訳: `2ececf1 refactor(secrets): port/domain/adapter構造を整理` に、差戻し補正 commit 群（`ffe9880`、`7320c55`、`fbc5096`、`fa396f3`、`ae1b917`、`97748c4`、`5e21afb`、`4cd47d4` を含む）、直前 P1 対応 `11ff088`、fresh review 差し戻し（構造・PTY・追跡更新）対応 `77dc03c` を含む。
- 補正対象: `2ececf1..77dc03c`（`4cd47d4` は過去補正時点として履歴保持、`11ff088` は直前 P1 対応 commit、現行対象終端は `77dc03c`）
- 対象ブランチ: `refactor/secrets-structure-issue-30-main`
- 確認証跡: `confirmation.md` の `PR #33 / Issue #30 task-list-outside 確認（2026-05-30）`
- outside-ledger 分類記録: `docs/task-governance/review-artifacts/outside-ledger-intake.md` の `2026-05-30 PR #33 / Issue #30 secrets structure branch 作り直し記録`
- 最新 AI review 対応記録: `5e21afb docs(secrets): PR33証跡を現行HEADへ補正` と `4cd47d4 docs(secrets): adapters削除対象の台帳表記を補正` は、PR #33 の AI review コメントで指摘された現行 HEAD 証跡不足と削除済み adapter root の対象パス扱いへの対応 commit として扱う。`97748c4 docs(secrets): BSM対象コードパスを補正` は先行する BSM 対象コードパス漏れ指摘への対応 commit として保持する。
- PR comment 対応記録: fresh review/集約/commit gate 確定前に、ユーザー依頼の PR AI review 対応として一部 PR review comment への返信または resolve を先行実施済み。この先行実施は PR 運用記録として扱い、repository governance 上の fresh review 合格、集約合格、commit gate 充足、最終完了扱いの根拠にはしない。

### current-cycle 差戻し handoff

- 構造レビュー担当: `判定: 不合格`
  - required fix: `adapters.rs` の `pub(crate) use` による実装型再公開を解消し、adapter root が公開面集約にならない構造へ変更する。
- ドキュメントレビュー担当: `判定: 要修正`
  - required fix: adapter-local `#[path = "stub/yubikey.rs"]` 構造に doc comment を整合し、`TestStubSecretDevice` の責務境界 comment を追加し、test-review skill の internal backend stub 条件列挙を正本参照へ寄せる。
- 運用整合レビュー担当: `判定: 要修正`
  - required fix: PR #33 / commit `2ececf1` 起点の作り直し記録に加え、diff range `5ff5e54..77dc03c` に含まれる補正 commit 群（`4cd47d4` を含む履歴）と現行終端 `77dc03c` の対象差分、確認結果、レビュー/集約状況、PR #32 close から PR #33 作り直しへの分類・責任境界を repository 内の証跡へ記録する。
- 運用整合レビュー担当（追加 current-cycle finding）: `判定: 要修正`
  - required fix: PR #33 / branch HEAD `77dc03c`、base `5ff5e54`、diff range `5ff5e54..77dc03c` を現行対象として追跡できるようにし、補正 commit `ae1b917`、`97748c4`、`5e21afb`、`4cd47d4` を PR #33 の補正履歴として保持する。`4cd47d4` は過去補正時点、`11ff088` は直前 P1 対応 commit、現行終端は `77dc03c` として repository 内証跡へ記録する。
- 運用整合レビュー担当（最新 current-cycle finding）: `判定: 要修正`
  - required fix: `5ff5e54..77dc03c` / HEAD `77dc03c` を PR #33 現行対象差分として固定し、`5e21afb` と `4cd47d4` を含む確認結果・AI review 対応記録・fresh review/集約未確定状態を更新する。fresh review/集約/commit gate 前に実施した PR comment 返信または resolve は、ユーザー依頼の PR AI review 対応として先行実施した PR 運用記録であり、repository governance 上の fresh review 合格、集約合格、commit gate 充足、最終完了扱いの根拠にしない。PR #32 close から PR #33 作り直しへの責任境界は `77dc03c` 現在地まで追跡可能にする。
- ドキュメントレビュー担当（追加 current-cycle finding）: `判定: 要修正`
  - required fix: review/confirmation/outside-ledger と root/area ledger の現行欄を PR #33 / Issue #30 現行サイクルとして追跡可能にし、旧 Hypatia サイクルを現行状態に見せない。confirmation の旧パス説明は履歴として明確化し、`review-checklist.md` の internal backend stub 許可条件列挙は `hexagonal-implementation-rules.md` 正本参照へ寄せ、`verification.rs` の stale test doc comment / function name を現行の `VerifySummary` 確認内容に合わせる。
- ドキュメントレビュー担当（最新 current-cycle finding）: `判定: 要修正`
  - required fix: `4cd47d4` / `5ff5e54..77dc03c` への証跡更新に加え、YubiKey 完了項目で `rust/dotfiles-cli/src/secrets/adapters.rs` を現行実在ファイルとして読ませない。YubiKey 欄では当時の対象かつ現行 `11ff088` tree では削除済みであることを明記し、BSM 欄の削除対象表記と矛盾させない。

### 補正後レビュー状況

- 実装担当補正: `実施済み`
- 実装担当確認: `confirmation.md` の `PR #33 / Issue #30 task-list-outside 確認（2026-05-30）` に、HEAD `77dc03c` / diff range `5ff5e54..77dc03c` として記録。
- 必須レビュー担当の fresh review: `未実施`
- 集約後レビュー判定: `未確定`
- 集約判定要約: `current-cycle finding 補正後の fresh review が未実施のため、合格/commit gate 充足とは扱わない`
- 集約根拠: `本節は対象差分・確認結果・差戻し状況・commit linkage の追跡記録であり、必須レビュー担当の合格判定、集約判定、commit gate 充足を代替しない。`
