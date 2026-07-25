# タスク運用ワークフロー

この文書は、リポジトリ全体で共有する最小限のタスク運用フローを定義する。

## 1. 作業単位

作業単位は、ユーザーが指定した GitHub issue、PR、または明示タスクである。repo 内の active ledger は使用しない。

オーケストレーターは、依頼文と指定された外部 issue / PR / タスク内容から単一の作業単位を確定し、必要な役割へ委譲する。GitHub issue / PR が指定されている場合は、その外部ページを作業単位の正本として扱う。明示タスクの場合は、ユーザー指示を正本として扱う。

## 2. 役割

- `オーケストレーター`: 対象作業と担当を確定し、進行を制御する。
- `実装担当`: 差分を作成し、必要な確認を行う。
- `レビュー担当`: 役割別観点で判定を返す。
- `進捗判定担当`: レビュー結果に基づいて進捗更新要否を判断する。
- `完了判定担当`: 完了条件と残課題を判定する。

タスク実行依頼（進行/継続/完了系指示を含む）は、必ずオーケストレーションから開始する。オーケストレーターは、作業単位の確定、委譲パラメーター抽出、役割起動、起動/利用失敗の最小記録だけを扱う。

オーケストレーション進行中、現在の実行者はオーケストレーター役割に拘束され、以下をしてはならない。

- ファイルを直接編集する。
- 実装作業を直接実行する。
- レビュー判定、進捗判定、完了判定を直接実施する。
- 実装十分性、規約適合、完了可否を自分で判断する。
- 役割起動の成功/失敗処理が確定する前に、実装判定目的で対象コード、仕様、テスト、レビュー成果物を読む。
- テスト、ビルド、検証コマンドを実行する。
- 利用者依頼がすでにタスク実行コマンドである場合に、委譲可否の追加許可を利用者へ求める。

利用者依頼がタスク実行コマンドである場合、オーケストレーターは追加許可を待たず、必要役割の fresh subagent を起動しなければならない。委譲の実行は義務であり、「明示された委譲要求がない」という理由で遅延または回避してはならない。

エージェントの役割は受け取った委譲内容によって決まり、実行機構によっては決まらない。委譲済みの実装担当・レビュー担当・判定担当は、その委譲自体によって担当役割が確定している。子エージェントは同じ delegated task を新たな top-level task-execution request として再解釈してはならない。

## 3. 最小状態

- `S0: 開始待ち`
- `S1: 実装中`
- `S2: レビュー中`
- `S3: レビュー集約完了`
- `S4: コミット着手可`

`S4` はコミット関連作業を開始できる状態を表し、作業単位の完了判定そのものではない。

## 4. 基本フロー

1. `S0 -> S1`
   - オーケストレーターが作業単位と担当を確定する。
   - 指定 GitHub issue / PR / 明示タスクを読み、委譲に必要な範囲で着手前要件、対象パス、完了条件、レビュー要求を抽出する。
   - [implementation-execution.md の不変 implementation handoff と Design artifact](implementation-execution.md#不変-implementation-handoff-と-design-artifact) が外部 issue / PR、明示タスク、または委譲 handoff に存在し、その identity、承認、baseline/design identity、取得時点、必須項目が委譲入力に記録されていることを確認する。handoff が未固定、存在しない、必須項目が欠ける、または利用者もしくは委譲元オーケストレーターによる承認が記録されていない場合は `S0` に留まり、repository-authored 成果物の編集、review、commit、push、重い全体検証を開始させてはならない。S1 の意図済み編集は baseline/design identity を失効させない。
   - 実装担当だけを委譲する。レビュー担当、進捗判定担当、完了判定担当は、未解消 finding のない
     S1 自己照合と固定比較対象が揃うまで起動しない。S1 の途中差分を読む reviewer を起動して
     実装と並走させることは、レビュー対象の固定性を失わせるため禁止する。
   - 実装担当は、編集を始める前に [implementation-execution.md の実装開始条件](implementation-execution.md#実装開始条件) を満たす。S2 の事後レビューは、この着手前設計照合の代替にならない。
2. `S1 -> S2`
   - 実装担当が変更差分を作成し、不変 handoff の coverage table を全行自己照合して必要な確認を記録する。[implementation-execution.md の全経路閉鎖不変条件](implementation-execution.md#全経路閉鎖不変条件) に従い、統合済み coverage と counterexample が全影響経路を閉じる証跡を確認する。coverage 未充足、未否定 counterexample、未解消 finding、または未完の success/error/failure/cleanup/caller/state mutation/test/direct observation/document evidence が一つでもあれば S2 を開始してはならない。文書主成果物では文書 flow、役割、参照経路、必要証跡、理由と正本根拠を伴う明示除外も必須である。変更を「一括」等と呼ぶこと、局所 pass、または test の一部通過は代替にならない。
   - 最終検証を必要とする場合は、[implementation-execution.md の検証選択](implementation-execution.md#検証選択) に従い、固定した execution/cache identity の一 command のみを根拠にする。
   - レビュー前に [比較 identity](implementation-execution.md#比較-identity) の同一形式で S1 完了 review-candidate identity（作業ブランチ、比較範囲、全値、確認結果、取得時点）を固定する。S2、S3、S4 では baseline/design identity ではなくこの review-candidate の全値を比較し、source、index、worktree、比較範囲または取得方法が変化した場合は全根拠を破棄して `S1` に戻る。
   - 上記の S1 完了報告を受けたオーケストレーターだけが、S2 開始時に必須 reviewer を起動する。
     reviewer は、固定した差分識別子・比較範囲・S1 の確認結果を同じ入力として受け取る。利用可能な
     並列枠の範囲で互いに独立な role を同時に起動する。slot 不足時だけ、完了した role の slot へ
     残る独立 role を直ちに起動する。独立 review を番号順や到着順に直列化してはならない。
3. `S2 -> S3`
   - `S2` の開始から `S3` の集約確定まで、固定した review-candidate と worktree をレビュー対象として凍結する。個別 reviewer の finding、途中 verdict、チャット上の所見を受けても、実装、format、生成、source 変更、commit を開始してはならない。source、index、worktree の変更を検出した場合は S1 に戻る。
   - 各 reviewer は、handoff・コード・文書に引用された repository 正本、URL、API symbol、仕様節、source location の原文を [docs-governance.md の参照資料の直接照合](../docs-governance.md#参照資料の直接照合) に従って直接読む。要約、リンク存在確認、過去の報告は verdict の根拠にしない。全経路閉鎖不変条件に反する穴を検出した reviewer は、局所 pass や部分合格を出さず、`要修正` または `不合格` として受入れを拒否する。
   - 必須レビュー役割の**全員**の判定を収集してから、`合格` / `要修正` / `不合格` を一度だけ集約確定する。個別 reviewer が返答したことは review cycle の終了条件ではない。
   - 必須担当は [implementation-review-judgement.md](implementation-review-judgement.md) に従う。
   - 必須レビュー担当は担当ごとに fresh subagent として起動する。複数担当を単一 subagent に一括委譲してはならない。
   - security review または test review の指摘が test-only の dummy / fixture secret の stdout、sentinel、または state 観測を問題にする場合、通常の finding を確定する前に `test-secret-observation false-positive verifier` を fresh subagent として起動する。起点が security reviewer か test reviewer か（両者が同じ指摘をした場合は両者）を記録し、verifier の結果をその起点 reviewer へ返す。verifier が compile-time test-only 選択、production build/runtime 非混入、fixture/spec の dummy 値限定、production 到達経路なしをすべて確認した場合は `誤検知` とし、該当 finding を通常判定から除外する。不採用理由と4条件の根拠を起点 reviewer へ返し、誤検知を `要修正` / `不合格` として集約してはならない。いずれかの条件を満たさない場合は `実漏えい` とし、起点 reviewer の finding を維持したまま、起点 reviewer の判定を `要修正` または `不合格` へ写像し、verifier の根拠とともに集約根拠へ記録する。
4. `S3 -> S4`
   - コミット着手条件を満たす場合のみ進む。

`要修正` または `不合格` がある場合は、集約済みの未解消 finding 全件を lossless に一つの remediation task へ渡してから `S1` に戻る。remediation は、既存 handoff への追記でも finding 要約でもなく、全必須項目と新旧 finding を再統合し承認した新しい不変 handoff から開始する。remediation 完了後は新しい比較対象を固定し、必須 reviewer 全員を fresh subagent で再起動して `S2` から全 review を一巡する。個別 finding ごとの即時修正、review 中の差分更新、部分的な再レビューをしてはならない。

## 5. 記録

主記録は、Git 差分、検証結果、レビュー担当の判定、PR 上の review thread 対応である。repo 内に完了済み作業の台帳、confirmation、review artifact、current-cycle 記録を作成しない。

必要に応じて実装担当またはレビュー担当の応答に、次を最小記録として残す。

- 対象差分識別子。
- 実行した確認と結果。
- 未実施確認と理由。
- 必須レビュー担当の判定。
- PR review comment への採用/不採用返信、修正 commit、resolve 状態。

補助記録の exact 同期、file-count、対象パス列挙、current-cycle 文言一致、自己 hash 固定は review gate / commit gate にしない。

## 6. コミット着手ゲート

コミット関連作業の開始条件は次のとおり。

- 対象差分が特定できる。
- 必要レビュー役割の結果が揃い、集約後レビュー判定が `合格`。
- 必要な実検証が確認できる。
- [implementation-execution.md の全経路閉鎖不変条件](implementation-execution.md#全経路閉鎖不変条件) を満たす coverage、counterexample、自己照合、review 集約の証跡が対象差分に対応付けられている。名称、局所 pass、test の一部通過は証跡の代替にならない。
- チャットや口頭報告のみを根拠にしていない。
- 領域固有文書または指定 issue / PR がより厳格な条件を定める場合は、その条件を満たしている。

文書是正では、無関係な粗粒度進捗更新、重複台帳同期、confirmation/review artifact の exact 整合を要求しない。

コミット関連作業は、上記条件と `集約後レビュー判定: 合格` の記録がそろうまで開始してはならない。S4 到達後もオーケストレーターは commit / PR コマンドを自己実行せず、実行主体を委譲内容で明示した fresh subagent に委譲する。委譲を受けた actor は、委譲された commit / PR 操作だけを実行し、レビュー判定または完了判定を兼務しない。

## 7. 役割分離

- 役割分離は維持する。
- subagent は異なる作業単位、異なる役割、同一作業単位の異なるサイクル間で再利用してはならない。
- オーケストレーターは自己実行を行ってはならない。
- fresh subagent として委譲を受けた子エージェントは、委譲された担当役割を直接実行する。
- オーケストレーターは、役割の完了・中断・差戻し・review cycle の終了ごとに、不要になった subagent を終了して整理しなければならない。自ら起動した `cargo` 等のプロセスは、対象 PID と親子関係を確認したうえで終了し、他の役割・利用者・独立した作業のプロセスを終了してはならない。次の委譲を始める前に live agent と自ら起動したプロセスを点検し、不要な実行枠を回収することを必須とする。ただし、同一の固定比較対象を読んでいる進行中の独立 `S2` review は、この整理の対象にせず中断してはならない。
- 必須役割を起動できない場合は、その事実と扱いを利用者へ報告し、可能な最小記録を残す。
- 実装差分や高リスク変更で要求される複数レビュー役割は省略しない。
- `cargo check` の合格、テストの通過、ビルド成功はレビュー合格の代替にならない。
- 番号付き項目の順序は、前項の出力・判定・権限が次項の前提になる依存関係を示す場合だけ強制する。独立した review、検証、調査を番号だけを根拠に直列化してはならない。

## 8. ブランチ・コミット・プルリクエスト運用

- ブランチ、コミット、PR の作成・更新は、コミット着手ゲートを満たした後に限り、委譲された実行主体が行う。
- 作業前に現在ブランチを確認し、依頼内容に適切でない場合は適切な作業ブランチまたは専用 worktree を作る。
- 新規作業ブランチは upstream の最新 `main` から分岐する。
- `main` へ直接 push してはならない。変更は機能ブランチと PR 経由で取り込む。
- コミットは Conventional Commits 形式 `<type>(<scope>): <description>` を使う。
- PR には恒久的な変更内容、必要性、実施検証を記述し、チャット履歴や作業実況は書かない。
- PR review thread には採用/不採用の理由を返信し、対応済み thread を resolve する。
- AI review で新規指摘が出た場合は、修正または不採用理由を示し、最新 head で no-issue になるまで繰り返す。

### 検証プロセスの競合防止

検証起動前に自分の cargo/nix/test process を PID・PPID・command 付きで列挙し、stale は子から親の順に終了する。artifact/target lock 消失を確認後、target directory、toolchain、profile、package、features、command を固定して単一実行する。同じ検証の並列起動、lock 待ち・未完了の合格扱いを禁止する。終了後は子プロセスと lock を再確認し、残存時は未完了として記録する。
