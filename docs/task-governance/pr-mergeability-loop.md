# PR マージ可能化ループ

この文書は、PR を実際に merge する前の「マージ可能な状態」へ到達させるための、オーケストレーション拡張としての共通運用を定義する。

## 対象

次のいずれかを含む依頼では、この文書を適用する。

- PR URL または PR 番号が指定され、mergeability、checks、review thread 対応、PR review 対応など、PR を merge 可能状態へ近づける操作が依頼されている。
- PR review 対応、AI review、Codex review、Copilot review が指定され、対象 PR を特定できる。
- 対象 PR が特定できる文脈で `@codex review` が指定されている。
- review thread の返信、修正、resolve が指定されている。
- 対象 PR を特定できる文脈で、checks、mergeability、merge 可能状態の確認が指定されている。

## ゴール

ゴールは、PR が merge 可能状態であると確認できることとする。少なくとも次を満たす。

- GitHub が PR を merge 可能と示しており、required status / checks / approvals が満たされている。`mergeStateStatus` の値が `CLEAN` の場合、または required gate の充足を別途確認したうえで `HAS_HOOKS` の場合は merge 可能状態として扱う。`UNSTABLE` は required status / checks / approvals が満たされており非必須 status の未通過だけである場合に限り、GitHub 上で merge 可能な状態として扱う。
- required checks は、required status context では `success` 相当、check run conclusion では `success` / `skipped` / `neutral` 相当として扱われる値である。
- 未解決の review thread がない。
- AI / Codex / Copilot review が明示的に依頼されている場合、または required review / required check として要求されている場合は、最新 head に対する review が no-issue である。
- 採用した指摘への修正、採用しない指摘への理由返信、対応済み thread の resolve が完了している。

実際の merge 実行は、この文書の責務に含めない。

## アクターモデル

top-level の PR マージ可能化依頼では、ループの所有者は main orchestrator である。main orchestrator は `/orchestration` に基づく既存の役割境界を維持したまま、この文書をオーケストレーション拡張として使う。

main orchestrator は、対象 PR の確定、head / checks / review thread / review 結果の inventory、必要な bounded delegation、委譲結果の集約、最新 head に対する再確認、保留条件の報告を調整する。

この文書は、単一の delegated PR actor に PR マージ可能化ループ全体を委譲する根拠ではない。委譲済みの実装担当、レビュー担当、判定担当、commit / PR 操作担当は、親オーケストレーターから割り当てられた bounded task の範囲だけを実行し、ループ全体を引き取ったり、同じ delegated task を再オーケストレーションしたりしてはならない。

main orchestrator は、`workflow.md` の役割分離に従い、実装修正、テスト・検証コマンド、レビュー判定、完了判定、commit / push / PR 操作を自己実行しない。これらが必要な場合は、`workflow.md` の gate と担当境界に従って bounded task として委譲し、その結果を受け取って PR 状態を再確認する。

## 反復手順

main orchestrator は、次を PR が merge 可能状態になるまで繰り返す。

1. PR 番号、base branch、head branch、head OID を確認する。
2. checks と mergeability を確認し、pending / failing / blocked の理由を特定する。
3. review、comment、review thread の inventory を作成し、未対応、対応済み、誤検出、権限不足で resolve 不能のものを分ける。
4. 最新 head に AI / Codex / Copilot review がない、または古い head への review しかない場合は、対象 PR が特定できる範囲で、`@codex review` などのリクエストが必要かを inventory する。リクエストが PR コメント、review request、その他 PR 上の操作を伴う場合、main orchestrator は自己実行せず、`workflow.md` と権限に従って bounded PR operation task として許可された実行主体へ委譲し、結果だけを再確認する。PR が特定できない review 依頼は、この文書ではなく通常の差分レビューとして扱う。
5. 指摘ごとに、判断に必要な担当を特定する。実装十分性、規約適合、レビュー判定、完了可否など、main orchestrator が自己判定できない事項は、該当する bounded review / judgement task として委譲する。
6. 採用する指摘に実装修正が必要な場合は、bounded implementation task として実装担当へ委譲する。実装担当は割り当てられた修正と確認だけを行い、PR マージ可能化ループ全体を所有しない。
7. commit / push / PR 更新が必要な場合は、`workflow.md` のコミット着手ゲートを満たした後に限り、bounded commit / PR operation task として委譲された実行主体へ渡す。main orchestrator は commit / push / PR 操作を自己実行しない。
8. 不採用の指摘への理由返信、対応済み review thread の resolve、PR 上の操作が必要な場合は、`workflow.md` と権限に従って、許可された PR 操作担当へ bounded task として委譲する。権限不足の場合は保留条件として扱う。
9. 委譲結果を集約し、新しい head OID に対して、checks、mergeability、未解決 thread、AI / Codex / Copilot review 結果を再確認する。
10. ゴールを満たすまで手順 2 から 9 を繰り返す。

GitHub Actions の macOS runner / インスタンスを手動で起動して checks を通す運用は、金額上の制約により許可しない。macOS 実行が pending / unavailable のまま mergeability が満たせない場合は、手動起動で回避せず保留条件として報告する。

## 指摘の扱い

PR review comment への採用/不採用返信、対応済み thread の resolve、AI review の反復規則は `workflow.md` の `## 8. ブランチ・コミット・プルリクエスト運用` を正本とする。

この文書では、main orchestrator が指摘ごとに採用、不採用、誤検出、権限不足で対応不能のいずれかの分類を集約する。採用する場合に同じ欠陥クラスが変更セット内に残っていないかの確認は、該当する実装担当またはレビュー担当へ bounded task として委譲する。

## 完了条件

完了報告では、次を確認できる形で示す。

- 確認した PR と最新 head OID。
- checks の結果。
- mergeability の結果。
- 未解決 review thread がないこと。
- AI / Codex / Copilot review が明示的に依頼されている場合、または required review / required check として要求されている場合は、最新 head に対する review が no-issue であること。
- 採用修正、不採用返信、resolve の実施状況。

## 保留条件

次の場合は、merge 可能化を完了扱いにせず、保留理由を具体的に報告する。

- required checks が pending / failing / blocked である、または required status context では `success` 相当、check run conclusion では `success` / `skipped` / `neutral` 相当として扱われる値ではない。
- required review、または明示的に依頼された bot review が未完了である。
- review thread の resolve 権限が不足している。
- merge conflict がある。
- branch protection、required approval、required check などの条件が未充足である。
- 明示的に依頼された、または required review / required check として要求された最新 head に対する AI / Codex / Copilot review を取得できない。
- GitHub Actions の macOS runner / instance を手動起動しないと required checks を進められない。金額上の理由でその運用は許可されないため、手動起動で迂回せず blocked 条件として報告する。

保留報告では、確認した head OID、未充足条件、次に必要な外部操作または待機対象を明示する。
