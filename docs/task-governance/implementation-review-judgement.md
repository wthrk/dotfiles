# 実装レビュー判定

この文書は、レビュー開始条件、必須レビュー担当、集約条件、完了判定への接続を定義する。

## レビューサイクル

レビューサイクルは次の3段階に分離する。

1. **レビュー開始**: 差分識別子、必要な確認結果、必須担当の割当を確認する。
2. **集約**: 必須担当の全判定を収集し、集約後レビュー判定を確定する。
3. **完了判定への接続**: 集約判定 `合格` を前提に、コミット着手条件と完了判定担当への引き渡しを確認する。

前段が確定しない限り次段へ進んではならない。

## 必須レビュー担当

- 実装差分（executable behavior を含む変更）:
  - `構造レビュー担当`
  - `運用整合レビュー担当`
  - `セキュリティレビュー担当`
  - `仕様適合レビュー担当`
  - `テストレビュー担当`
  - `ドキュメントレビュー担当`
  - `アーキテクチャ整合レビュー担当`
- 文書是正・文書主成果物:
  - `運用整合レビュー担当`
  - `参照整合レビュー担当`
- 高リスク変更を含む場合は `セキュリティレビュー担当` を追加必須にする。
- `AGENTS*`、`.agents/skills/`、`docs/task-governance/`、`docs/architecture/` を変更する場合は、文書主成果物でも `構造レビュー担当` と `アーキテクチャ整合レビュー担当` を追加必須にする。

レビュー指摘に test-only の dummy / fixture secret の stdout、sentinel、または state 観測が含まれる場合に限り、通常の security/test finding として確定する前に、`test-secret-observation false-positive verifier` という fresh role をレビューサイクル内で起動する。この専任確認はコミット着手ゲート直前の追加ゲートではなく、指摘の分類を確定するためのレビュー内判定である。起点 reviewer（security reviewer または test reviewer）を記録し、verifier 結果を起点 reviewer へ返す。

## 各レビュー担当の職責

### 外部 SDK / crate の利用根拠（全レビュー担当共通）

対象差分が外部 SDK / crate を利用、変更、またはその戻り値・失敗時挙動を扱う場合、判定に関与する各レビュー担当は [../docs-governance.md の外部 SDK / crate の利用根拠](../docs-governance.md#外部-sdk--crate-の利用根拠) と [参照資料の直接照合](../docs-governance.md#参照資料の直接照合) を直接適用する。実装担当の引用、過去のレビュー、実機観測、エラー本文の推測で代替してはならない。各担当は URL、引用、API symbol、仕様節、source location を見つけたら、対応する原文を自ら開き、主張との対応、引用範囲、版・revision、対象差分への適用範囲を照合する。URL・symbol の存在確認や実装担当の要約では足りない。担当観点に関わる API 利用、認証、データモデル、利用フロー、成功・失敗時状態遷移、全エラー値の意味・分類・retry / fallback / default / 空値化 / 握りつぶしの根拠を判定する。原文を読めない、または適用範囲を確定できない参照は根拠に採用せず、その事実を判定に明記する。根拠がない意味付けまたは遷移があれば、少なくとも `要修正` とする。

### 構造レビュー担当

[../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) と [../architecture/review-checklist.md](../architecture/review-checklist.md) に従い、層別責務、依存方向、公開範囲、処理単位の責務配置を判定する。機械的分離を合格根拠にせず、処理が正本アーキテクチャ文書で規定された境界に置かれているかを確認する。SDK / crate 利用が対象なら、公式一次資料で確認した利用フローとエラー遷移が adapter / support の翻訳に留まり、未根拠の意味付けを層へ持ち込んでいないかを確認する。

### 仕様適合レビュー担当

ユーザー指定の GitHub issue / PR / 明示タスク、および領域固有仕様が定める完了条件、制約、是正対象を現行差分に対して直接照合する。サマリーや実装担当の報告で代替してはならない。SDK / crate 利用が対象なら、公式一次資料に照らし、仕様が要求する利用フローと失敗時遷移を満たすか確認する。

仕様適合レビュー担当は、委譲入力から完了条件を所有する正本を特定し、その正本を読んで判定する。領域固有仕様、architecture/test 配置規則、またはユーザー指定 issue / PR が完了条件を定める場合は、それらを担当職責の入力正本として扱う。

### セキュリティレビュー担当

[security-obligations.md](security-obligations.md) に定義された制約を適用し、秘密情報漏えい、不正アクセス経路、権限境界逸脱、危険な失敗時挙動を確認する。SDK / crate 利用が対象なら、公式一次資料にないエラー分類、握りつぶし、fallback、default、状態変更を安全と扱っていないか確認する。

レビュー指摘が test-only の dummy / fixture secret の観測を問題にしている場合、security reviewer または test reviewer はそれを通常の finding として確定せず、`test-secret-observation false-positive verifier` を fresh role で起動する。verifier は compile-time test-only 選択、production build/runtime 非混入、fixture/spec の dummy 値限定、production 到達経路なしを対象コードから直接確認する。4 条件をすべて満たす場合は `誤検知` として該当 finding を通常判定から除外し、起点 reviewer へ不採用理由と根拠を返す。当該指摘を `要修正` / `不合格` の根拠にしてはならない。いずれかを満たさない、または本番値・本番出力経路が関係する場合は `実漏えい` として起点 reviewer の finding を維持し、起点 reviewer の判定を `要修正` または `不合格` へ写像する。写像理由、verifier の4条件ごとの根拠、維持した finding を集約根拠へ記録する。

### test-secret-observation false-positive verifier

この role は上記の test-only 観測の分類だけを担当し、コード修正、全体レビュー判定、完了判定を行わない。判定対象は指摘された観測経路とその直接の build/runtime 条件に限定する。返答には `判定: 誤検知` または `判定: 実漏えい` を先頭に置き、4 条件ごとの根拠、起点 reviewer、起点 reviewer へ返す採用/不採用理由を記載する。`誤検知` の場合は該当 finding の通常判定からの除外を明記する。`実漏えい` の場合は起点 reviewer の finding を維持し、起点 reviewer へ戻す判定写像（`要修正` または `不合格`）と、集約根拠へ記録すべき根拠を明記する。

### 運用整合レビュー担当

実行手順、役割分離、gate 条件、必要な確認結果、完了判定ロジックが実運用で強制可能かつ監査可能かを確認する。補助記録の exact 同期不足だけを不合格根拠にしてはならない。

運用整合レビュー担当は、対象文書が [workflow.md](workflow.md) の実行手順、役割分離、gate 条件、commit / PR 運用、または完了判定への接続を変更する場合、その変更後の運用経路を対象文書として直接確認する。`workflow.md` はオーケストレーター初動の代替として読むのではなく、この職責の判定対象になった場合に読む。

### テストレビュー担当

テストコードが仕様を実際に検証しているかを確認する。完了条件は、テストで検証すべき項目と構造確認・文書確認で満たす項目に分類し、前者に対してテスト網羅を要求する。test double / fixture の配置判定は形式ではなく責務で行う。

test double / fixture の配置、inline unit test、internal backend stub、test-only secret observation の判定は [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) と [../architecture/review-checklist.md](../architecture/review-checklist.md) の test 関連規則へ照合する。

SDK / crate 利用が対象なら、公式一次資料が定義する成功・失敗遷移と各エラー値の扱いを、テストが実際に検証しているか、または文書・構造確認に分類すべきかを確認する。

YubiKey の `setup` / `put` / `clear` / enroll / rotate、`status`、またはそれらを呼ぶ provisioning script が対象に含まれる場合、テストレビュー担当は [secret-recovery spec の command contract](../secret-recovery/secret-recovery-spec.md#到達仕様のコマンド一覧) を直接照合する。管理操作（`setup` / `put` / `clear` / enroll / rotate）は、controlling TTY から hidden prompt で設定済み PIV PIN を 1 回だけ取得し、PIN-protected management key 認証へ限定して使うこと、TTY を取得できない場合は secret input・device mutation の前に fail-closed することを回帰テストで確認する。PIN は stdin payload、argv、environment、stdout、stderr、log、一時ファイルへ渡してはならない。復旧 read/decrypt path（`status` / `verify-yubikey` / `restore-gpg` / `restore-pass`）は PIN prompt、PIN verification、management-key authentication を発生させず、PIN を要求した場合は回帰テストの有無にかかわらず `要修正` とし、仕様適合レビュー担当へ必須指摘として返す。

### ドキュメントレビュー担当

コード内ドキュメントコメントが実装と整合しているかを確認する。SDK / crate 利用が対象なら、コメントおよび恒久文書が公式一次資料で裏付けられない利用フロー・エラー意味付けを断定していないかを確認する。判定対象と必須範囲は [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) と [../docs-governance.md](../docs-governance.md) に従う。

### アーキテクチャ整合レビュー担当

モジュールまたはコードベースを全体として読み、設計が [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) の哲学と整合しているかを判定する。個別ルールの合格総和で代替してはならない。

アーキテクチャ整合レビュー担当は、責務境界がモジュール全体として一貫しているか、局所的な配置正しさが全体設計の歪みを隠していないか、複数ファイルに分散した判断が同じ設計責務として読めるかを確認する。SDK / crate 利用が対象なら、公式一次資料に基づく外部失敗の翻訳・伝播方針がモジュール横断で一貫し、未根拠の recovery を隠していないかを確認する。構造レビューの checklist 判定は入力にできるが、それだけでこの職責の判定を完了してはならない。

### 参照整合レビュー担当

文書是正で、リンク、参照先、ファイルパス、定義の一貫性と解決可能性を確認する。対象がスキルファイルの場合は frontmatter、Required Reading Order、正本複製禁止への適合を確認する。

## レビュー開始条件

- 対象差分識別子がある。
- 比較範囲またはレビュー対象が固定されている。
- 必要な確認結果があり、対象差分に対して何を確認したか追跡できる。
- 必須レビュー担当が割り当て済みである。

文書是正・文書主成果物では、exact tracked-file set、file-count、補助記録の完全同期を開始条件にしない。

## 判定表示規則

- 各レビュー担当の返答は、先頭に `判定: <合格|要修正|不合格>` を 1 行で記録する。
- `判定` に使ってよいラベルは `合格`、`要修正`、`不合格` のみ。
- `判定要約: ...` を続け、`合格` の場合は `所見なし`、それ以外は主要論点を要約する。
- `根拠:` 見出しと箇条書きで判定根拠を記録する。
- 集約判定も `集約後レビュー判定: <合格|要修正|不合格>`、`集約判定要約: ...`、`集約根拠:` の順に揃える。

## レビュー独立性規則

レビュー担当は、過去のレビュー記録、確認記録、実装担当の報告を判定の代替にしてはならない。対象コード、対象文書、指定 issue / PR / タスクを直接読んで独立に判定する。

レビュー委譲時に、オーケストレーターは差し戻し用詳細 handoff を reviewer へ渡してはならない。レビュー委譲は、この文書とスキル定義が定める最小パラメーターに限定する。

## 差し戻し時の受け渡し規則

- 差し戻しで再実装を委譲する際、オーケストレーターは未解消 finding 内容そのものを新しい実装担当へ handoff できる。
- handoff には、未解消 finding ごとに `reviewer role`、`verdict`、`file:line references`、`required fix`、`finding 本文` を含める。
- 受け渡しは lossless を原則とし、意味が失われる再要約・省略・言い換えをしてはならない。

## 集約規則

- 1件でも `不合格` があれば未完了。
- `要修正` があれば修正後に再レビュー。
- 必須担当が全員 `合格` の場合のみ `集約後レビュー判定: 合格`。
- 具体的懸念、残留リスク、未解消疑義、要追跡事項、運用依存の注意事項を記録したレビューは finding ありと扱う。
- finding ありの記録が残る場合、当該レビュー担当判定は少なくとも `要修正` とし、集約後レビュー判定を `合格` にしてはならない。

## コミット連動規則

- コミット関連作業は `集約後レビュー判定: 合格` の記録後に限り開始できる。
- 個別メッセージやチャット上の宣言のみで完了扱いにしてはならない。
