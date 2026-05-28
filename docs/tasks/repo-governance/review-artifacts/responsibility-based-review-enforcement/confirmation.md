# responsibility-based-review-enforcement 確認記録

この文書は、`docs/tasks/repo-governance/tasks.md` の作業項目 `責務基準レビュー強制への是正` に対する確認証跡である。

## サイクル情報

- 区分: `current-cycle confirmation`
- 確認状態: `完了`
- 対象差分識別子: `working-tree-responsibility-review-enforcement`
- 差分区分: `文書整合（review-enforcement 文書）`
- 境界注記: `documentation-remediation。コード差分なし。rust/ source は変更していない。`

## 現行確認対象（変更要約）

- `docs/architecture/review-checklist.md`:
  - `チェックの進め方` に `責務基準の判定原則（形式より責務）` を追加。各シンボル・各ファイル・各 `#[cfg(test)]`/`#[cfg(feature = "...")]` ブロックに問1（責務）・問2（その責務はこの層か）を立て、責務不一致なら形式が正しくても `不合格` とする強制を明記。port trait 実装・feature gate・cfg(test) ラップを免除理由から明示的に除外。
  - `adapters/` `レビュー時の問い` に test double 判定の問いを追加。`確認手順` に先行ステップ0（test double 混入検出）を追加し、手順3/4 を「port trait 実装でも test double なら不合格」へ強化。
  - `tests/` セクションに `レビュー時の問い`・`許可される in, 禁止される out`・`確認手順` を新設。double 定義の production 層配置を禁止しつつ、inline unit test（`#[test]`）を明示的に許可。
- `.agents/skills/test-review/SKILL.md`（`.claude/skills/test-review/SKILL.md` は同一実体・symlink）:
  - 自己矛盾していた `#[cfg(test)] 残存禁止` 規則を削除し、責務基準の判定・double 定義 production 層混入禁止（cfg(test)/cfg(feature)/port-trait 実装は免除理由でない）・inline unit test 許可の3規則へ置換。判定基準正本は review-checklist.md を参照し複製しない。
- `docs/task-governance/implementation-review-judgement.md`:
  - テストレビュー担当 職責の `#[cfg(test)]`-permits 抜け穴を責務基準の判定へ書き換え。inline unit test 許可を明記。
- 新規作業定義文書 `docs/tasks/repo-governance/work-items/responsibility-based-review-enforcement.md` を追加し、責務基準強制の完了条件を定義。area README / area tasks.md に導線を接続。

## 現行確認対象（構造是正 — アーキテクチャ全体整合レビュー役割の導入）

部分ごとの個別ルール判定だけで、誰もアーキテクチャ全体の一貫性を判定しないという構造的欠落（各部分がルールを通過しても全体として設計が破綻する）を埋めるため、全体設計整合を独立に判定する役割を導入し必須レビュー担当へ組み込んだ。

- 新規スキルファイル `.agents/skills/architectural-consistency-review/SKILL.md`（`.claude/skills/` は同一実体・symlink）:
  - 役割名 `アーキテクチャ整合レビュー担当`。frontmatter（name + description）・役割・受け取るパラメーター・Governing Sources・Required Reading Order・Rules を既存レビュアースキル（structural-review 等）の構造に揃えて作成。
  - 責務をモジュール（コードベース）**全体**の設計整合判定とし、個別ルールの逐一照合（公開面・依存方向・test double・命名・配置・完了条件・コメント）に退化させない旨を明記。全体への問い（一貫した1つの設計か／責務が層をまたいで一貫して分配されているか／層関係が全体として意味をなすか／有能なアーキテクトが一貫した設計と呼ぶか部品の山と呼ぶか）で判定し、各部分が個別ルールにすべて合格していても全体非整合なら `判定: 不合格` とする。
  - 受け取るパラメーターは差分ではなくモジュール全体のコードパス。レビュー独立性（他担当の個別判定・過去記録・実装担当報告を代替にしない）と再レビュースコープを規定。判定フォーマットは `docs/task-governance/implementation-review-judgement.md` へ委譲し複製しない。
- `docs/task-governance/implementation-review-judgement.md`:
  - `必須レビュー担当` の実装差分集合へ `アーキテクチャ整合レビュー担当` を追加（6担当 → 7担当）。
  - `各レビュー担当の職責` に `アーキテクチャ整合レビュー担当` サブセクションを新設し、全体整合判定の責務と他担当（部分判定）との構造的差異・集約ロールが全体整合を判定しない欠落を埋める位置づけを明記。
- `.agents/skills/orchestration/SKILL.md` / `.agents/skills/dotfiles-task-governance/SKILL.md`:
  - 実装差分必須レビュアー列挙を「6担当」→「7担当」に更新し `アーキテクチャ整合レビュー担当` を追加。委譲パラメーターに同担当を追加し、差分ではなくモジュール全体のコードパスを渡す旨を規定。
- `.agents/skills/implementation-review-judgement/SKILL.md`:
  - `担当スキルファイル一覧` に `アーキテクチャ整合レビュー担当` のスキルパスを追加。併せて欠落していた `テストレビュー担当`・`ドキュメントレビュー担当` のスキルパスも補完し一覧を必須レビュー担当と整合させた。
- `docs/tasks/repo-governance/work-items/responsibility-based-review-enforcement.md`:
  - 作業目的・構造完了条件・規約違反の解消対象・同一変更セット文書カテゴリ・境界条件・レビュー合格条件を、全体整合レビュー役割の存在・スキルファイル・必須レビュー担当集合への一貫した組込みを要求する形へ拡張。

## 確認手順と結果

- 確認手順: `git diff --check`（対象: review-checklist.md, test-review/SKILL.md, implementation-review-judgement.md, orchestration/SKILL.md, dotfiles-task-governance/SKILL.md, implementation-review-judgement/SKILL.md, repo-governance 配下）
- 確認結果: `exit code 0（空白エラーなし）`
- 新規スキルファイル `.agents/skills/architectural-consistency-review/SKILL.md` の作成確認: `git status --porcelain` で untracked として存在を確認。
- 正本複製確認: enforcement 文書および新規スキルは `hexagonal-implementation-rules.md` の哲学を正本として参照し、判定フォーマットは `implementation-review-judgement.md` へ委譲し、矛盾規則を再記述していない。
- 必須レビュー担当集合の整合確認: `アーキテクチャ整合レビュー担当` が implementation-review-judgement.md（必須レビュー担当 + 職責）、orchestration/SKILL.md、dotfiles-task-governance/SKILL.md（列挙 + 委譲パラメーター）、implementation-review-judgement/SKILL.md（担当スキルファイル一覧）の全列挙箇所に一貫して追加されていることを確認。

## 現行確認対象（root active ledger への作業項目登録 — 運用整合レビュー指摘是正）

運用整合レビュー担当の `要修正`（本作業項目が area ledger と area README/証跡 README には接続されていたが、root active ledger `docs/tasks/tasks.md` には未登録で、ガバナンスフロー上 active work item として選定・追跡・完了前進できない欠陥）を是正した。`docs/tasks/README.md` と `docs/tasks/repo-governance/README.md` に従い、root `docs/tasks/tasks.md` の `現在の作業項目` のみが active 選定正本であり area `tasks.md` は補助台帳/履歴で選定源ではない。

- `docs/tasks/tasks.md` の `作業項目一覧` に `責務基準レビュー強制への是正` エントリを新設。フィールドは area ledger エントリと整合させ、`状態: レビュー中`・`主成果物: 文書差分`・`対象文書パス`（8件）・`作業定義文書`・`確認記録`・`レビュー記録`（委譲により作成予定）・`領域台帳/履歴` を記載。
- `docs/tasks/tasks.md` の `現在の作業項目` を `Bitwarden Secrets Manager` から `責務基準レビュー強制への是正` へ変更。判断根拠: 本作業項目は area ledger 上 `状態: レビュー中`（`進行中`＝S2、未完了）の進行中サイクルであり、`workflow.md` 4節の active item 選定対象に該当する。運用整合レビュー担当の解消条件「現行サイクルで `現在の作業項目` として選定可能であること」を満たすため active item に設定した。作業定義文書・統治源に別の選定指定はない。

## root active ledger 登録の確認手順と結果

- 確認手順: 追加した全リンクのファイルパスとアンカーの解決を検証。
  - ファイルパス（`docs/tasks/` 相対）: `repo-governance/work-items/responsibility-based-review-enforcement.md`, `repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md`, `repo-governance/review-artifacts/responsibility-based-review-enforcement/`, `repo-governance/tasks.md` — 全て存在を確認（OK）。
  - 対象文書パス（repo root 相対）8件 — 全て存在を確認（OK）。
  - アンカー `#責務基準レビュー強制への是正` → work-item H1 `# 責務基準レビュー強制への是正`（解決）。
  - アンカー `#repo-global-ガバナンス文書整合タスク` → area ledger H1 `# repo-global ガバナンス文書整合タスク`（解決。既存 `ガバナンス文書整合` root エントリの `領域台帳/履歴` と同一規約）。
- 確認結果: 追加リンク・アンカーは全て解決。reference-integrity を維持。
- 境界注記: 本是正は markdown/ガバナンス文書のみの変更で `rust/` source は変更していない。prior remediation 編集（責務基準判定・`アーキテクチャ整合レビュー担当` 役割導線）は revert していない。

## 状態注記

- inline unit test 非禁止確認: 3 文書すべてで「`#[test]` 関数や `#[cfg(test)]` ブロックの存在のみを理由に不合格にしない／禁止対象は double の定義に限る」を明記済み。
- 履歴専用注記: 旧 production adapter 内 test stub は、当時の責務基準では `adapter` 層配置違反として検出されるべき対象だった。現行 tree では同 path は削除済みであり、本記録は責務基準レビュー強制の履歴証跡としてのみ扱う。
- 全体整合の検出対象: 維持者が `application/` を「ぐちゃぐちゃ」と評した状態（各部分が個別ルールを通過しても全体として設計が破綻）は、本是正で導入した `アーキテクチャ整合レビュー担当` が全体整合判定で捉えるべき対象。`application/` 等の実コード再設計は secret-recovery 領域の実装作業項目の責務であり、本作業はその全体非整合を独立判定する役割と必須化を文書・スキルへ与えることに限定する。
- 役割定義の方向性確認: 新規役割は個別チェック項目の追加ではなく全体整合判定を責務とする旨を、スキルファイルと implementation-review-judgement.md の職責の双方に明記済み（「チェックリスト項目を1つずつ照合する形に退化させてはならない」「部分の合格の総和では捉えられない全体の非整合を捉える」）。
- 既存先行是正の非revert確認: prior step の責務基準判定編集（review-checklist.md の `責務基準の判定原則`・`adapters/` ステップ0・`tests/` セクション、test-review/SKILL.md の責務基準3規則、implementation-review-judgement.md テストレビュー担当職責）はいずれも保持しており revert していない。
- スコープ整合注記: exact tracked-file set の列挙を gate に使わず、変更要約と確認手順を正本とする。

## 現行確認対象（日本語スキルのコミットゲート閉集合列挙是正 — 運用整合レビュー指摘是正）

運用整合レビュー担当の `要修正`（`.agents/skills/orchestration/SKILL_ja.md` のコミット着手ゲート規則が、必須レビュー担当を閉じた4担当集合（構造・仕様適合・セキュリティ・運用整合）として列挙し、英語版 `SKILL.md` および他2箇所のコミットゲート行が持つ「正本の必須レビュー担当セクションへ委譲する」節を欠いていたため、日本語スキルを用いるオーケストレーターがテスト・ドキュメント・アーキテクチャ整合レビュー担当をスキップしてコミット可能となり、本是正の目的を無効化する欠陥）を是正した。閉じた列挙が正本の7担当集合と矛盾するのを避けるため、英語版と同一の手法（委譲節の付与）で正本 `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションへ拘束要件を解決させた。

- `.agents/skills/orchestration/SKILL_ja.md`（コミットゲート行）:
  - before: `必須レビュー役割（構造レビュー担当・仕様適合レビュー担当・セキュリティレビュー担当・運用整合レビュー担当）の全員から合格判定を得なければならない。`（閉じた4担当、委譲節なし）
  - after: 列挙を正本の7担当（構造・運用整合・セキュリティ・仕様適合・テスト・ドキュメント・アーキテクチャ整合レビュー担当）へ更新し、英語版と同文の委譲節「文書是正を含む場合は参照整合レビュー担当を追加する。変更種別による必須担当の詳細は `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションに従う。」を付与。
- `.agents/skills/dotfiles-task-governance/SKILL_ja.md`（コミットゲート行 = S3→S4 遷移行）:
  - 同一欠陥（閉じた4担当 `構造・仕様適合・セキュリティ・運用整合`、委譲節なし。英語版 `dotfiles-task-governance/SKILL.md` には委譲節あり）を同手法で是正。列挙を7担当へ更新し同文の委譲節を付与。
- 非対象確認: `.agents/skills/implementation-review-judgement/SKILL_ja.md`・`.agents/skills/task-completion-judgement/SKILL_ja.md` は統括文書記述として正本を参照するのみで閉じたコミットゲート列挙を持たないため、本欠陥に該当せず編集しない。

### 日本語スキル是正の確認手順と結果

- 確認手順: 全 `.agents/skills/*/SKILL_ja.md` を走査し、閉じた4担当列挙パターン（`構造レビュー担当・仕様適合レビュー担当・セキュリティレビュー担当・運用整合レビュー担当` / `構造・仕様適合・セキュリティ・運用整合`）の残存を検査。
- 確認結果: 該当パターンの残存なし（マッチ0件）。是正した2ファイルにそれぞれ委譲節「…「必須レビュー担当」セクションに従う」が1回ずつ出現することを確認。
- 参照整合確認: 委譲節が指す `docs/task-governance/implementation-review-judgement.md` の存在と、見出し `## 必須レビュー担当`（アンカー解決対象）の存在を確認（解決）。追加した参照・アンカーはすべて解決し reference-integrity を維持。
- 境界注記: 本是正は markdown/ガバナンス文書（日本語スキルファイル）のみの変更で `rust/` source は変更していない。prior remediation 編集（責務基準判定・`アーキテクチャ整合レビュー担当` 役割導線・root active ledger 登録）は revert していない。コミットは行っていない。

## 現行確認対象（正本コミットゲートの閉集合列挙是正 — 運用整合レビュー指摘是正）

運用整合レビュー担当の `要修正`（先行是正が leaf スキルファイルのみを修正し正本 `docs/task-governance/workflow.md` を修正していなかったため、全スキルが委譲先とする正本のコミットゲート行が依然として閉じた4担当（構造・仕様適合・セキュリティ・運用整合）を「`implementation-review-judgement.md` に定められた全必須レビュー役割」として列挙し委譲節を欠いており、正本に従うオーケストレーターがテスト・ドキュメント・アーキテクチャ整合レビュー担当をスキップしてコミット可能となる root 抜け穴。併せて英語版 leaf スキル `orchestration/SKILL.md`・`dotfiles-task-governance/SKILL.md` のコミットゲート行が委譲節は持つものの inline 列挙が閉じた4担当のまま日本語版と非対称で stale）を是正した。日本語版で受容された手法（正本7担当の列挙＋委譲節）と同一手法で全箇所を正本 `## 必須レビュー担当` セクションへ拘束させ整合させた。

- `docs/task-governance/workflow.md`（7節 コミットゲート行 = 116行付近、PRIMARY/正本）:
  - before: `…全必須レビュー役割（構造レビュー・仕様適合レビュー・セキュリティレビュー・運用整合レビュー）の全員合格を集約済みでなければならない。`（閉じた4担当、委譲節なし）
  - after: 列挙を正本の7担当（構造レビュー担当・運用整合レビュー担当・セキュリティレビュー担当・仕様適合レビュー担当・テストレビュー担当・ドキュメントレビュー担当・アーキテクチャ整合レビュー担当）へ更新し、委譲節「文書是正を含む場合は参照整合レビュー担当を追加する。変更種別による必須担当の詳細は `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションに従う。」を付与。
- `.agents/skills/orchestration/SKILL.md`（コミットゲート行 = 51行付近。`.claude/skills/` は同一 inode のハードリンクで同時反映、SECONDARY）:
  - before の inline 列挙 `structural review, specification-conformance review, security review, and operational-consistency review`（閉じた4担当）を正本7担当 `structural review, operational-consistency review, security review, specification-conformance review, test review, documentation review, and architectural-consistency review` へ更新。既存の委譲節は削除せず保持。
- `.agents/skills/dotfiles-task-governance/SKILL.md`（コミットゲート行 = S3→S4 遷移行 = 75行付近。`.claude/skills/` は同一 inode のハードリンクで同時反映、SECONDARY）:
  - before の inline 列挙 `(structural, specification-conformance, security, operational)`（閉じた4担当）を正本7担当 `(structural, operational-consistency, security, specification-conformance, test, documentation, architectural-consistency)` へ更新。既存の委譲節は削除せず保持。

### 正本コミットゲート是正の最終スイープと確認手順・結果

- 最終スイープ: `docs/task-governance/` および `.agents/skills/`（英語版・`*_ja.md` 含む）を走査し、コミットゲート/必須レビュー文脈で test・documentation・architectural-consistency 担当を欠く閉集合列挙パターン（`構造レビュー・仕様適合レビュー・セキュリティレビュー・運用整合レビュー` / `structural review, specification-conformance review, security review, and operational-consistency review` / `(structural, specification-conformance, security, operational)` / `構造・仕様適合・セキュリティ・運用整合` / `構造レビュー担当・仕様適合レビュー担当・セキュリティレビュー担当・運用整合レビュー担当`）の残存を検査。
- スイープ結果: ライブ規則中の残存マッチ0件。`progress-judgement.md`・`task-completion-judgement.md`・`README.md`・`implementation-review-judgement/SKILL.md`・`task-completion-judgement/SKILL.md` は「必須レビュー役割」を閉集合列挙せず正本を参照するのみのため本欠陥に該当せず編集不要と確認。日本語版 leaf スキル（`orchestration/SKILL_ja.md`・`dotfiles-task-governance/SKILL_ja.md`）は先行サイクルで既に7担当＋委譲節へ是正済みで本サイクルの編集対象外と確認。
- 参照整合確認: 委譲節が指す `docs/task-governance/implementation-review-judgement.md` の存在と、見出し `## 必須レビュー担当`（19行、アンカー解決対象）の存在を確認（解決）。3ファイルに付与/保持した参照・アンカーはすべて解決し reference-integrity を維持。
- 空白確認: `git diff --check`（対象: workflow.md, orchestration/SKILL.md, dotfiles-task-governance/SKILL.md）= `exit code 0（空白エラーなし）`。
- 境界注記: 本是正は markdown/ガバナンス文書のみの変更で `rust/` source は変更していない。prior remediation 編集（責務基準判定・`アーキテクチャ整合レビュー担当` 役割導線・root active ledger 登録・日本語スキルのコミットゲート是正）は revert していない。本確認記録に記載された before-state の旧4担当列挙文字列は証跡であり改変していない。コミットは行っていない。

## 現行確認対象（セッション入口 governing document `AGENTS*.md` の閉集合列挙是正 — 運用整合レビュー指摘是正）

運用整合レビュー担当の `要修正`（先行サイクルの最終スイープが `docs/task-governance/` + `.agents/skills/` にスコープされ、リポジトリのセッション入口 governing document である root `AGENTS.md`／`AGENTS_ja.md` を見落としていた欠陥。`AGENTS.md` は毎セッション開始時に最初に読まれるため、その役割→スキル対応表のレビュー担当行が必須レビュー担当を閉じた4担当集合（構造・仕様適合・セキュリティ・運用整合）として列挙していたことは、正本の7担当集合と矛盾するライブ規則であり、テスト・ドキュメント・アーキテクチャ整合レビュー担当を欠落させていた）を是正した。閉じた列挙が正本と矛盾するのを避けるため、コンパクトな表セル内で stale なリストをハードコードしない委譲方式（正本 `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションへ拘束要件を解決させる）を採用した。`AGENTS.md` と `AGENTS_ja.md` は意味的に同期させた。

- `AGENTS.md`（役割→スキル対応表、14行）:
  - before: `| Review (structural, spec-conformance, security, operational) | /implementation-review-judgement |`（閉じた4担当）
  - after: `| Review (required reviewer set per docs/task-governance/implementation-review-judgement.md の「必須レビュー担当」) | /implementation-review-judgement |`（閉じた列挙を撤廃し正本へ委譲）
- `AGENTS_ja.md`（役割→スキル対応表、14行）:
  - before: `| レビュー担当（構造・仕様適合・セキュリティ・運用整合） | /implementation-review-judgement |`（閉じた4担当）
  - after: `| レビュー担当（必須レビュー担当集合は docs/task-governance/implementation-review-judgement.md の「必須レビュー担当」に従う） | /implementation-review-judgement |`（閉じた列挙を撤廃し正本へ委譲）

### `AGENTS*.md` 是正の最終スイープと確認手順・結果

- 最終スイープ（スコープ拡大）: スイープ対象を `docs/task-governance/` + `.agents/skills/` から **リポジトリ全体（root `AGENTS*.md` を明示的に含む・`docs/`・`.agents/` ほか）** へ拡大し、test・documentation・architectural-consistency 担当を欠く閉集合列挙パターン（`構造・仕様適合・セキュリティ・運用整合` / `構造レビュー担当・仕様適合レビュー担当・セキュリティレビュー担当・運用整合レビュー担当` / `structural, spec-conformance, security, operational` / `(structural, specification-conformance, security, operational)` / `structural review, specification-conformance review, security review, and operational-consistency review`）の残存を、`.worktree`／worktree 重複ツリーと本確認記録の before-state 証跡文字列を除外して検査。
- スイープ結果: ライブ規則中の残存マッチ0件（是正した `AGENTS.md:14`・`AGENTS_ja.md:14` のみが該当し、両者を是正済み）。`.agents/skills/architectural-consistency-review/SKILL.md` の「構造・仕様適合・テスト・セキュリティ・運用整合・ドキュメント」列挙は、アーキテクチャ整合レビュー担当が自身と対比する他6担当の説明であり閉じた必須レビュー担当列挙ではないため本欠陥に該当せず編集不要と確認。`orchestration`・`dotfiles-task-governance`・`workflow.md`・`secret-recovery/implementation-guidelines.md` の列挙は既に7担当列挙＋委譲節（または secret-recovery 領域固有の `最低でも` 最小集合規則）であり本欠陥に該当しないと確認。
- 参照整合確認: 両セルの委譲先 `docs/task-governance/implementation-review-judgement.md` の存在と、見出し `## 必須レビュー担当`（19行、アンカー解決対象）の存在を確認（解決）。両 `AGENTS*.md` に付与した参照はすべて解決し reference-integrity を維持。
- 同期確認: `AGENTS.md` と `AGENTS_ja.md` の是正後セルは意味的に同一（いずれも閉じた列挙を撤廃し正本の「必須レビュー担当」へ委譲）であり、`AGENTS.md` の `## Translation Synchronization` / `AGENTS_ja.md` の `## 翻訳同期` が要求する意味的一致を満たすことを確認。
- 境界注記: 本是正は markdown/ガバナンス文書（root `AGENTS*.md`）のみの変更で `rust/` source は変更していない。prior remediation 編集（責務基準判定・`アーキテクチャ整合レビュー担当` 役割導線・root active ledger 登録・日本語スキルのコミットゲート是正・正本コミットゲート是正）は revert していない。本確認記録に記載された before-state の旧4担当列挙文字列は証跡であり改変していない。コミットは行っていない。最終スイープのスコープは root `AGENTS*.md` を明示的に含む形へ拡大した。

## 現行確認対象（secret-recovery 領域正本ガイドラインの閉集合列挙是正 — 運用整合レビュー指摘是正）

運用整合レビュー担当の `要修正`（先行サイクルの最終スイープが `docs/secret-recovery/implementation-guidelines.md` の列挙を「secret-recovery 領域固有の `最低でも` 最小集合規則であり本欠陥に該当しない」と分類して見落としていた欠陥。同文書は先頭（3行）で secret-recovery 領域のレビューサイクル・役割分担の正本であると自己宣言しており、その `planning / implementation / review の役割分担` の `レビュー` 行（48行）が、起動すべき必須レビュー担当集合を `最低でも 構造レビュー担当・運用整合レビュー担当・セキュリティレビュー担当・仕様適合レビュー担当`（+条件付き参照整合）という閉じた最小集合として列挙し、正本の7担当集合から `テストレビュー担当`・`ドキュメントレビュー担当`・`アーキテクチャ整合レビュー担当` を欠落させていた。同文書は active な secret-recovery YubiKey 実装差分作業項目を直接統治するため、secret-recovery オーケストレーターがこの3担当をスキップ可能となる、他所で既に是正した同一欠陥クラスのライブ規則）を是正した。閉じた列挙が正本と矛盾するのを避けるため、他所と同一の手法（正本 `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」セクションへの委譲＋7担当の明示列挙）を適用した。secret-recovery 固有の厳格規則（文書構成変更・タスク運用変更・スキル変更・`AGENTS.md` 変更時の `参照整合レビュー担当` 起動義務）は弱めず保持した。

- `docs/secret-recovery/implementation-guidelines.md`（`planning / implementation / review の役割分担` の `レビュー` 行、48行）:
  - before: `- \`レビュー\` は複数レビュー担当で行う。secret-recovery では最低でも \`構造レビュー担当\`、\`運用整合レビュー担当\`、\`セキュリティレビュー担当\`、\`仕様適合レビュー担当\` を起動し、文書構成変更、タスク運用変更、スキル変更、\`AGENTS.md\` 変更がある場合は \`参照整合レビュー担当\` も起動する。`（閉じた4担当最小集合、テスト・ドキュメント・アーキテクチャ整合を欠落）
  - after: `- \`レビュー\` は複数レビュー担当で行う。起動すべき必須レビュー担当集合は \`docs/task-governance/implementation-review-judgement.md\` の「必須レビュー担当」セクションに従う（実装差分（executable behavior を含む変更）では \`構造レビュー担当\`、\`運用整合レビュー担当\`、\`セキュリティレビュー担当\`、\`仕様適合レビュー担当\`、\`テストレビュー担当\`、\`ドキュメントレビュー担当\`、\`アーキテクチャ整合レビュー担当\`）。加えて secret-recovery では、文書構成変更、タスク運用変更、スキル変更、\`AGENTS.md\` 変更がある場合は \`参照整合レビュー担当\` も必ず起動する。`（正本の7担当へ委譲＋明示列挙、secret-recovery 固有の参照整合起動義務を保持）

### secret-recovery ガイドライン是正の最終スイープと確認手順・結果

- 最終スイープ（スコープ拡大）: スイープ対象を、`docs/secret-recovery/` を明示的に含むリポジトリ全体（root `AGENTS*.md`・`docs/`・`.agents/`）へ拡大し、test・documentation・architectural-consistency 担当を欠く閉集合列挙パターン（`構造レビュー担当・運用整合レビュー担当・セキュリティレビュー担当・仕様適合レビュー担当` / `構造・仕様適合・セキュリティ・運用整合` / `structural review, specification-conformance review, security review, and operational-consistency review` / `(structural, specification-conformance, security, operational)`）が、コミットゲート・ロスター・必須レビュー担当起動の文脈にライブ規則として残存しないかを、(a) 本確認記録の before-state 証跡文字列、(b) `review-artifacts/` 配下の過去レビュー成果物・履歴記録、(c) `.worktree`／worktree 重複ツリー を除外して検査。
- スイープ結果: ライブ規則中の残存マッチ0件（是正した `docs/secret-recovery/implementation-guidelines.md:48` のみが該当し是正済み）。各マッチの分類:
  - `docs/task-governance/workflow.md:116`・`.agents/skills/orchestration/SKILL.md:51,58`・`.agents/skills/orchestration/SKILL_ja.md:45`・`.agents/skills/dotfiles-task-governance/SKILL.md:61,75`・`.agents/skills/dotfiles-task-governance/SKILL_ja.md:69`・`AGENTS.md:14`・`AGENTS_ja.md:14`: 先行サイクルで既に7担当列挙＋委譲節（または正本委譲）へ是正済みのライブ規則であり本欠陥に該当しない（編集不要）。
  - `.agents/skills/architectural-consistency-review/SKILL.md:12`: アーキテクチャ整合レビュー担当が自身と対比する他6担当（構造・仕様適合・テスト・セキュリティ・運用整合・ドキュメント）の説明であり、閉じた必須レビュー担当起動列挙ではない（contrast-list、編集不要）。
  - `docs/tasks/repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md`（本記録）: before-state 証跡文字列（改変不可、編集不要）。
  - `docs/tasks/secret-recovery/review-artifacts/yubikey/*`（review-operational/-test/-structural/-spec ほか）: 過去サイクルの review-artifact 履歴記録でありライブ規則ではない（編集不要）。
  - `.serena/memories/orchestrator/session-failures-2026-05-25.md:15,23,30`: 過去インシデント（2026-05-24/25 のレビュースキップ失敗）の分析・教訓を記録した memory/履歴成果物であり、オーケストレーターが起動ロスターとして参照するガバナンス正本ではない。当該インシデントでスキップされた4担当を記録したものでライブ必須レビュー担当列挙規則ではないため、本指示のスイープスコープ（root + docs/ + .agents/）外かつ history/analysis として編集不要。
- 参照整合確認: 是正行が指す `docs/task-governance/implementation-review-judgement.md` の存在と、見出し `## 必須レビュー担当`（19行、アンカー解決対象）の存在を確認（解決）。是正行に付与した参照（bare path-in-backticks 方式、正本コミットゲート是正と同一スタイル）は解決し reference-integrity を維持。
- 境界注記: 本是正は markdown/ガバナンス文書（`docs/secret-recovery/implementation-guidelines.md`）のみの変更で `rust/` source は変更していない。prior remediation 編集（責務基準判定・`アーキテクチャ整合レビュー担当` 役割導線・root active ledger 登録・日本語スキルのコミットゲート是正・正本コミットゲート是正・`AGENTS*.md` 是正）は revert していない。本確認記録に記載された before-state の旧4担当列挙文字列および `review-artifacts/`・`.serena/memories/` 配下の履歴記録は改変していない。コミットは行っていない。最終スイープのスコープは `docs/secret-recovery/` を明示的に含むリポジトリ全体へ拡大した。

## 集約レビュー判定の記録と台帳配線（2026-05-25）

- 集約記録の作成: 文書是正・文書主成果物の必須2担当（`運用整合レビュー担当`・`参照整合レビュー担当`）がいずれも個別記録に `判定: 合格`・`判定要約: 所見なし` を永続化済みであることを実体読取りで確認（[review-operational-2026-05-25.md](review-operational-2026-05-25.md)・[review-reference-2026-05-25.md](review-reference-2026-05-25.md)）。`docs/task-governance/implementation-review-judgement.md` の集約規則・判定表示規則に従い、集約記録 [review.md](review.md) を新規作成し `集約後レビュー判定: 合格`・`集約判定要約: 所見なし`・`集約根拠:` を canonical 形式で記録した。
- 台帳配線: root active ledger `docs/tasks/tasks.md` および area ledger `docs/tasks/repo-governance/tasks.md` の `責務基準レビュー強制への是正` エントリの `レビュー記録` を「（レビュー役割の委譲により作成予定）」プレースホルダから、いま実在する集約記録 `review.md`（および個別判定 `review-operational-2026-05-25.md`・`review-reference-2026-05-25.md`）への解決リンクへ差し替えた（完了済み `ガバナンス文書整合` のレビュー記録リンク形式に整合）。
- 状態前進: 両台帳の `状態` を `レビュー中`（S2）から `レビュー集約完了`（S3）へ前進させた。根拠は `docs/task-governance/workflow.md` の最小状態 `S3: レビュー集約完了` と基本フロー `3. S2 -> S3`（必須レビュー役割の判定を集約し `合格` を確定する段階）であり、aggregate `合格` がこの遷移に対応する。`完了`（S4 以降）は完了判定担当の責務のため設定していない。
- リンク解決確認: 追記/差替えした全リンクの解決を確認した。root ledger からの相対リンク（`repo-governance/review-artifacts/responsibility-based-review-enforcement/{review,review-operational-2026-05-25,review-reference-2026-05-25}.md`）、area ledger からの相対リンク（`review-artifacts/responsibility-based-review-enforcement/{...}.md`）、`review.md` 内の上位参照（`../../tasks.md`・`../../../tasks.md`・`../../work-items/responsibility-based-review-enforcement.md`）および同ディレクトリ参照（`confirmation.md`・両個別判定記録）はいずれも実在ターゲットへ解決する。
- 境界注記: 本作業は markdown/ガバナンス文書（集約 `review.md` の新規作成・両台帳 `tasks.md` の配線/状態更新・本確認記録への追記）のみで `rust/` source は変更していない。2件の個別レビュー判定記録の verdict（いずれも `合格`）および prior remediation 編集は改変・revert していない。`状態` を `完了` にしていない。コミットは行っていない。

## 完了遷移の記録（2026-05-25）

- ゲート再検証: 実装担当（最終台帳更新・コミット）として、文書是正・文書主成果物の必須2担当の個別判定（[review-operational-2026-05-25.md](review-operational-2026-05-25.md) = `判定: 合格`・[review-reference-2026-05-25.md](review-reference-2026-05-25.md) = `判定: 合格`）、集約記録 [review.md](review.md) の `集約後レビュー判定: 合格`、および完了判定担当が返した `完了可` を実体読取りで確認した。`docs/task-governance/task-completion-judgement.md` の「文書作業の扱い」（文書主成果物は文書差分とレビュー合格で完了判定できる）および「判定失効条件」（対象差分識別子 `working-tree-responsibility-review-enforcement`・集約後レビュー判定・必須レビュー役割の判定記録・確認記録のいずれも存在）を充足する。
- 状態前進（S3 → 完了）: `docs/task-governance/workflow.md` 4節（`S3 -> S4`）および完了済み `ガバナンス文書整合` 項目の precedent（完了コミット `d8d987d` が当該項目の `状態` を `完了` に設定した形式）に従い、root active ledger `docs/tasks/tasks.md` と area ledger `docs/tasks/repo-governance/tasks.md` の両方で本作業項目の `状態` を `レビュー集約完了`（S3）から `完了` へ前進させた。area ledger 本項目エントリは `実装状態` フィールドを持たない簡易形式（`ガバナンス文書整合` のみが `実装状態` を持つ）であり、本項目はその自エントリ構造に合わせ `状態` のみを更新した。
- `現在の作業項目` の前進: `docs/task-governance/workflow.md` 4節「active work item が完了したら、次回実行前に root active ledger の `現在の作業項目` を次の未完了項目へ進める」に従い、root `docs/tasks/tasks.md` の `現在の作業項目` を `責務基準レビュー強制への是正` から次の未完了項目 `Bitwarden Secrets Manager` へ前進させた。根拠: 本確認記録（上記「root active ledger への作業項目登録」節）に記録のとおり、本作業項目は `Bitwarden Secrets Manager` を一時的に `現在の作業項目` から置換して着手したものであり、本項目の完了に伴い、置換前の未完了 active 候補である `Bitwarden Secrets Manager`（状態 `未開始`）へ戻す。
- 境界注記: 本完了遷移は markdown/ガバナンス文書（両 `tasks.md` の `状態`/`現在の作業項目` 更新・本確認記録への追記）のみで `rust/` source は変更していない。prior remediation 編集および2件の個別レビュー判定記録・集約 `review.md` の verdict は改変・revert していない。
