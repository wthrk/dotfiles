# 秘密情報復旧基盤の実装ガイドライン

この文書は、秘密情報復旧基盤の実装、確認、レビュー、報告の正本である。planning request では、planning procedure 自体もこの文書を正本として扱う。`dotfiles secrets` の構造、成果物配置、秘密値境界、恒久レビュー規則はこの文書を基準に判断する。

## 1. 目的と対象範囲

目的は、秘密情報復旧基盤の実装を一貫した層構造、秘密値の所有境界、レビュー運用に固定し、secret-recovery work 全体で同じ判断基準を維持することである。対象は `dotfiles secrets` の command flow、関連する domain model、wire format、port、adapter、support utility、review artifact、設計文書、進捗報告である。

## 2. 参照正本

一般構造の判断は [docs/architecture/hexagonal-implementation-rules.md](/Users/ya/works/dotfiles/docs/architecture/hexagonal-implementation-rules.md) を正本とする。機能仕様と保存仕様の判断は [docs/secret-recovery/yubikey-secret-storage-design.md](/Users/ya/works/dotfiles/docs/secret-recovery/yubikey-secret-storage-design.md) を参照する。進捗管理の正本は issue `#11` が open の間は [docs/secret-recovery/tasks.md](/Users/ya/works/dotfiles/docs/secret-recovery/tasks.md) と issue `#11` だけとする。

## 3. planning request 実行手順

planning request を受けた agent は、この節を planning procedure の入口として扱う。最初にこの節を読み、ここで参照する後続節へ進む。plan、planning summary、planning procedure explanation、planning-related recommendation は、この節の Step 1 から Step 6 を順に満たすまで出してはならない。
generic な planning mode、assistant 既定の質問順、または他文書の一般論は、この節の Step 管理、phase 遷移、差戻し先、review 順序を置き換えてはならない。planning request 中の planning-related response は、毎回 current Step または current Phase、次段階、失敗時の戻り先をこの文書の名称で示せる状態に保つ。これを示せない応答は無効とし、直前の成立済み Step へ戻ってこの節からやり直す。

| Step | 開始条件 | 実行内容 | 必要出力 | 次段階 | 失敗時の戻り先 |
| --- | --- | --- | --- | --- | --- |
| 1. 入口確認 | planning request を受領 | current request のためにこの文書を再読し、planning procedure の正本がこの文書であることを確認する。進捗管理入口が `tasks.md` と issue `#11` であることも確認する。 | planning gate 確認 | Step 2 | この手順 |
| 2. 構造診断 | Step 1 完了 | repo、設計文書、progress artifact を探索し、要求に関係する現状構造、既知違反、関連 issue / PR / artifact を集める。探索で確定できる事実はここで確定する。 | concern map、既知違反一覧 | Step 3 | Step 1 |
| 3. 要求整理 | Step 2 完了 | 探索で確定できなかった高影響の未確定事項だけを切り分け、request-specific に確認する。探索で解決できる点を質問してはならない。 | 要求整理済み状態 | Step 4 | Step 2 |
| 4. Phase A 草案 | Step 3 完了 | `## 11` と `## 12` に従い、Architecture Governance のプラン草案を作成する。プランは責務境界、依存方向、公開面、差戻し先が判断できる内容に限定する。 | Phase A 提出物 | Step 5 | Step 3 |
| 5. Phase A review | Step 4 完了 | Plan Review Agent が `plan-section-checklist.md` を基準に review し、`APPROVED` または差戻しを確定する。`scope-conflict` と `content-conflict` はこの phase で扱う。 | Phase A `APPROVED` または差戻し | Step 6 | Step 4 または Step 3 |
| 6. Phase B 草案と review | Phase A `APPROVED` | `## 11` の移行手順と `## 13` の Phase B に従い、実装プランを作成し review する。実装順序、確認方法、差戻し先、報告更新を判断できる状態まで確定する。 | Phase B `APPROVED` または差戻し | `## 11` Step `実装反映` と `## 13` Phase C 以降 | Step 4 または Step 3 |

質問戦略は次のとおり固定する。探索で確定できる repo / doc / artifact 事実は先に探索で確定し、利用者には聞かない。質問するのは、探索で解決できず、かつ答えにより Phase A または Phase B の結論が変わる高影響事項だけとする。対象範囲、成功条件、承認単位、互換性境界、除外対象、報告単位のような high-impact ambiguity を残したまま Phase A 草案へ進んではならない。

finalization rule は次のとおり固定する。Phase A は Plan Review Agent の `APPROVED` が出るまで完了しない。Phase B は Implementation Plan Review Agent の `APPROVED` が出るまで完了しない。planning-related output を終えるときは、現在地が Step 1 から Step 6 のどこか、未解決事項が何か、差戻し先がどこかを文書上の phase 名で説明できなければならない。
Step 1 から Step 6 の間で planning-related response を返すたびに、少なくとも current Step、未解決事項の有無、次段階、失敗時の戻り先を文書上の用語で維持する。current Step を言えないまま探索、質問、草案提示、要約、手順説明へ進んではならない。

## 4. `dotfiles secrets` の目標構造

`dotfiles secrets` は `entrypoint`、`application`、`domain`、`port`、`adapter`、`support`、`tests` の分離を維持する。command parsing は `entrypoint`、use-case orchestration は `application`、domain invariants and wire format は `domain`、port contracts は `port`、device selection と prompt and stdin handling と stdout policy と JSON parsing/decoding は `adapter`、secret protection utilities は `support`、summaries and reporting DTOs は `application`、test doubles and PTY/integration tests は `tests` に属する。

## 5. `dotfiles secrets` の層責務割当

| 層 | 所有責務 | 必須モジュール | 禁止 concern |
| --- | --- | --- | --- |
| `entrypoint` | command parsing、CLI 引数境界、終了 code 変換 | `command`, `args`, `dispatch` | device selection、prompt、JSON decode、wire format |
| `application` | use-case orchestration、停止条件、summary/report DTO | `use_case`, `flow`, `summary`, `report` | concrete I/O、device handle 操作、stdin 読み取り |
| `domain` | domain invariants and wire format、secret 名制約、保存仕様 | `value`, `policy`, `wire_format`, `error` | prompt、stdout policy、SDK 型、JSON parser |
| `port` | port contracts、application が要求する capability | `device_port`, `terminal_port`, `storage_port`, `report_port` | DTO in ports、prompt 文言、parser |
| `adapter` | device selection、prompt and stdin handling、stdout policy、JSON parsing/decoding、外部 API bridge | `device`, `terminal`, `stdout`, `json`, `storage` | use case の順序制御、domain policy 決定 |
| `support` | secret protection utilities、zeroization、memory protection、byte utility | `protect`, `zeroize`, `buffer`, `guard` | business vocabulary in support layer、command 名、secret 名 |
| `tests` | test doubles and PTY/integration tests、層契約検証 | `fakes`, `pty`, `integration`, `fixtures` | production export、review 証跡の代替 |

## 6. 成果物配置規則

| 成果物 | 所有層 | 許可表現 | 禁止表現 | 生存期間規則 |
| --- | --- | --- | --- | --- |
| command parsing | `entrypoint` | enum/newtype への入力変換 | adapter 内 parser、domain 内 clap 依存 | 入口で完結させる |
| use-case orchestration | `application` | use case、flow、停止条件 | adapter callback による順序制御 | command 実行中のみ |
| domain invariants and wire format | `domain` | value object、wire model、validation | terminal 文言、SDK 型、I/O policy | version 契約が続く限り維持 |
| port contracts | `port` | trait と capability 契約 | DTO in ports、prompt、JSON 文言 | adapter 差し替え可能性が続く限り維持 |
| device selection | `adapter` | serial 列挙、選択結果変換 | application 手順への侵入 | 実行ごと |
| prompt and stdin handling | `adapter` | hidden prompt、TTY 判定、stdin read | domain validation の混在 | 入力境界で破棄 |
| stdout policy | `adapter` | terminal/pipe 判定、redaction policy | application 層からの直接 print | 出力完了まで |
| JSON parsing/decoding | `adapter` | serde decode、I/O 境界変換 | domain 内 parser、port DTO | decode 後ただちに移譲 |
| secret protection utilities | `support` | protected buffer、zeroize、guard | business vocabulary、report logic | secret 破棄まで |
| summaries and reporting DTOs | `application` | summary、report value | port contract への流出 | command 完了まで |
| test doubles and PTY/integration tests | `tests` | fake port、PTY harness、integration flow | production module 再公開 | test 実行中のみ |

## 7. 秘密値の所有境界

| 成果物 | 所有層 | 許可表現 | 禁止表現 | 生存期間規則 |
| --- | --- | --- | --- | --- |
| plain secret | `support` の機能中立な保護型として生成し `application` が一時所有 | protected buffer | `String`、debug 表示、永続 log | 直後に zeroize |
| PIN | `adapter` 入力境界から `support` 保護型へ移送 | protected input value | CLI 引数、平文 summary | verify 完了まで |
| decoded input field | `adapter` | decode 済み protected field | domain に unprotected bytes を渡すこと | use case 移譲まで |
| wrapped key | `domain`/`adapter` 境界 | wire-format field | plaintext key と同一型での混在 | unwrap 完了まで |
| encrypted blob | `domain` | wire-format blob | plaintext 扱い、debug dump | 保存仕様が有効な限り |
| summary/report value | `application` | redacted summary | secret 含有 DTO | report 出力まで |
| device handle | `adapter` | device/session handle | `application` や `domain` での所有 | I/O 完了まで |

plain secret、PIN、decoded input field は `support` の保護境界を通さずに `application` や `domain` に渡してはならない。`application` が扱ってよいのは、`docs/architecture/hexagonal-implementation-rules.md` で許可された `support` の機能中立な保護型だけであり、feature 固有 utility や device/session API を保持してはならない。summary/report value は secret 本文を含まず、device handle は adapter 境界の外へ漏らしてはならない。

## 8. 公開面規則

| モジュール領域 | 許可公開項目 | 必須非公開項目 | 再公開規則 |
| --- | --- | --- | --- |
| `entrypoint` | command enum、dispatch entry | parser helper、prompt text | binary 境界以外へ再公開しない |
| `application` | use case 起動 API、summary/report 型 | branch helper、adapter concrete type | top-level module で必要最小限のみ再公開 |
| `domain` | value、policy、wire-format 型 | parse helper の内部 detail | 不変条件を保つ型だけ再公開可 |
| `port` | trait、capability request/response | DTO、prompt text、serde helper | adapter 実装が必要な contract に限定 |
| `adapter` | composition root 用 constructor | SDK 型、device handle、terminal raw state | domain/application へ再公開しない |
| `support` | protected primitive の最小 API | feature vocabulary、summary/report helper | feature 名を含む再公開禁止 |
| `tests` | test module 内 helper | production path | 本番 code へ再公開しない |

## 9. ドキュメントコメント要件

この節の comment / doc comment 要件は [AGENTS.md](/Users/ya/works/dotfiles/AGENTS.md) と [docs/architecture/hexagonal-implementation-rules.md](/Users/ya/works/dotfiles/docs/architecture/hexagonal-implementation-rules.md) の規則を継承し、それらの要求を狭めてはならない。各非自明 module は file-level comment または module doc comment で役割を説明する。repository-authored explanatory comment は日本語で書き、durable project intent、invariant、constraint、non-obvious operational context だけを残す。低価値 comment、個人メモ、曖昧な TODO/FIXME は入れてはならない。`dotfiles secrets` の public command flow、非自明な private helper、wire-format 型、port trait、adapter module は、先頭文で主要契約を述べ、その後に必要入力、失敗時の停止条件、secret 所有境界、interaction boundary を別文または別段落で記述する。複数段落の doc comment は第 1 段落で通常系契約、第 2 段落以降で non-TTY behavior、timeout、ownership transfer、zeroization、locking、output safety、retry rule のような制約を記述する。`support` comment は security property、ownership transfer、zeroization、locking、output safety のいずれかを具体的に書く。

## 10. 除去すべき既知違反パターン

| 違反パターン | 検出条件 | 必要な是正 |
| --- | --- | --- |
| DTO in ports | `port` module が serde struct や end-user 出力 DTO を持つ | contract を trait/request-response の最小形へ戻し DTO を adapter/application へ移す |
| parser/prompt/stdout policy/device selection mixed into one adapter surface | 1 adapter module が入力、出力、device 選択、利用者文言を同時に抱える | adapter を terminal/device/stdout/json へ分割する |
| concrete I/O in application layer | `application` が stdin/stdout/device/session を直接扱う | port 呼び出しへ置換し I/O 詳細を adapter へ移す |
| business vocabulary in support layer | `support` module が command 名、secret 名、role 名を持つ | 中立 primitive へ削減し語彙を domain/application へ戻す |
| rename-only or directory-only refactors that preserve the old structure | path だけ変わり責務境界が不変 | 層責務、公開面、依存方向の実体を是正する |

## 11. リファクタ移行手順

| 移行手順 | 必要入力 | 必要出力 | 次段階 | 失敗時の戻り先 |
| --- | --- | --- | --- | --- |
| 構造診断 | 現行 module、既知違反一覧、参照正本 | concern map、違反一覧 | 責務分解 | この手順 |
| 責務分解 | concern map、違反一覧 | 層ごとの移動計画 | 公開面整理 | 構造診断 |
| 公開面整理 | 移動計画、公開 API 一覧 | visibility plan、re-export plan | 秘密値境界修正 | 責務分解 |
| 秘密値境界修正 | visibility plan、secret lifecycle | protected ownership plan | 実装反映 | 公開面整理 |
| 実装反映 | 承認済み plan、対象 code/doc | 更新済み module/doc | 確認 | 秘密値境界修正 |
| 確認 | 差分、artifact、参照正本 | 確認結果、差戻しまたは承認 | レビュー | 実装反映 |
| レビュー対応 | 確認結果、review findings | 解決済み差分、追跡表更新 | 未解決ゼロ確認 | 確認 |

## 12. secret-recovery 作業のエージェント実行モデル

| 役割 | 兼務禁止 | 必要証跡 | 承認出力 |
| --- | --- | --- | --- |
| Main Orchestrator | Implementation、Verification、Follow-up Issue、Review A/B/C/D | 段階遷移記録 | 完了宣言 |
| Plan Drafting Agent | Plan Review Agent | 計画案 | Phase A 提出 |
| Plan Review Agent | Plan Drafting Agent、Implementation Plan Drafting Agent、Implementation Plan Review Agent、Implementation Agent、Verification Agent、Follow-up Issue Agent、Review A/B/C/D | `plan-section-checklist.md` | `APPROVED` または差戻し |
| Implementation Plan Drafting Agent | Implementation Plan Review Agent | 実装計画案 | Phase B 提出 |
| Implementation Plan Review Agent | Plan Drafting Agent、Plan Review Agent、Implementation Plan Drafting Agent、Implementation Agent、Verification Agent、Follow-up Issue Agent、Review A/B/C/D | 実装計画レビュー記録 | `APPROVED` または差戻し |
| Implementation Agent | Verification、Follow-up Issue、Review A/B/C/D | 更新済み文書、artifact 初版 | Phase C 出力 |
| Verification Agent | Follow-up Issue、Review A/B/C/D | checklist、review matrix、cross-link 証跡 | Phase D/F 承認 |
| Follow-up Issue Agent | Review A/B/C/D | issue 起票証跡、報告草案 | issue 情報提出 |
| Review A | Review B/C/D | review record | `APPROVED` または差戻し |
| Review B | Review A/C/D | review record | `APPROVED` または差戻し |
| Review C | Review A/B/D | review record | `APPROVED` または差戻し |
| Review D | Review A/B/C | `finding-traceability.md` 初版 | `APPROVED`、差戻し、follow-up issue 要求 |

Main Orchestrator は実装、コマンド実行、ファイル確認、差分確認、検証、レビュー、証跡作成を行わない。Plan Review Agent と Implementation Plan Review Agent は、互いの drafting role を含む前段 planning role 以外の実装、確認、起票、review role を兼務してはならない。Main Orchestrator の最小確認責務は、`review-matrix.md` の承認状態確認、`finding-traceability.md` の `unresolved` と `same-class recurrence` のゼロ確認、`tasks.md` と issue `#11` の更新完了確認である。

## 13. レビュー・確認・承認ワークフロー

この節で定義する Phase A から Phase F-2 までを、secret-recovery work における `Architecture Governance` ワークフローと呼ぶ。

| レビュー段階 | 開始条件 | 担当者 | 完了条件 | 失敗時の戻り先 |
| --- | --- | --- | --- | --- |
| Phase A: アーキテクチャ規約プラン | 要求整理済み | Plan Drafting Agent / Plan Review Agent | `APPROVED` | Phase A |
| Phase B: 実装プラン | Phase A `APPROVED` | Implementation Plan Drafting Agent / Implementation Plan Review Agent | `APPROVED` | Phase B |
| Phase C: 規約文書更新 | Phase B `APPROVED` | Implementation Agent | 更新済み 6 文書と review artifact 一式 | Phase C |
| Phase D: 確認 | Phase C 出力揃い | Verification Agent | ファイル存在、見出し一致、表列一致、リンク整合、証跡存在が `APPROVED` | Phase C |
| Phase E: レビュー | Phase D `APPROVED` | Review A/B/C/D | A/B/C/D 全員 `APPROVED` | Review A/B/C failure は Phase C、Review D の planning weakness は Phase B、architecture weakness は Phase A、implementation issue は Phase C、follow-up issue required は Phase F-1 |
| Phase F-1: 後続 issue 起票 | Review D が follow-up issue を要求 | Follow-up Issue Agent と Verification Agent | issue 番号と紐付け確認が `APPROVED` | Phase F-1 |
| Phase F-2: 未解決指摘ゼロ確認 | Phase E 完了後または F-1 完了後 | Verification Agent | `unresolved` 0、`same-class recurrence` 0、状態列妥当 | implementation issue は Phase C、implementation-plan weakness は Phase B、Architecture Governance weakness は Phase A |

Review A は責務境界、依存方向、公開面を確認する。Review B は見出し、表、doc comment 規約、用語整合を確認する。Review C は移行手順、検証手順、進捗報告、承認フローを確認する。Review D は過去 PR 指摘の全件再監査、同種再発確認、指摘単位の状態分類を行う。

conflict type は `scope-conflict`、`content-conflict`、`planning-conflict`、`execution-conflict`、`audit-source-conflict` に固定する。Verification Agent は conflict を `open` で記録し、rules-document work 中は Plan Review Agent が `scope-conflict` と `content-conflict` を判定し、implementation work 中は Implementation Plan Review Agent が `planning-conflict` と `execution-conflict` を判定し、Review D の監査元不整合は Verification Agent が `audit-source-conflict` として記録する。該当 phase の担当者が修正し、Verification Agent が再確認し、解消後に `review-matrix.md` へ `closed` を記録する。

Review D の監査対象 PR 集合は、Review D 開始時点で Verification Agent が `review-matrix.md` に記録した issue `#11` progress comment の `comment ID`、`timestamp`、`comment body hash` により凍結する。凍結コメントには current PR number、同一 work item の predecessor PR numbers、各 predecessor の one-line relation を必須とする。凍結後の進捗コメントが編集された場合、Verification Agent は `audit-source-conflict` を記録し、現在の Review D 実行を無効化して Phase D に戻す。監査 source は PR review threads、PR review comments、top-level PR conversation comments に固定し、finding 出力形式は `resolved`、`unresolved`、`same-class recurrence`、`follow-up issue required` に固定する。

## 14. 報告規則

この節の節目更新は、[docs/secret-recovery/tasks.md](/Users/ya/works/dotfiles/docs/secret-recovery/tasks.md) の `Architecture Governance milestones` と issue `#11` progress comment を同じ用語で同期する。

| 節目 | tasks.md 更新内容 | issue #11 更新内容 | 必要承認者 |
| --- | --- | --- | --- |
| アーキテクチャ規約プラン草案 | 対象節目を追加し状態を更新 | current PR、predecessor PR、関係一行説明付き進捗コメント | Plan Review Agent |
| プランレビュー承認 | `APPROVED` を記録 | 承認コメントまたは要約 | Plan Review Agent |
| 実装プラン草案 | 節目状態更新 | 同上 | Implementation Plan Review Agent |
| 実装プランレビュー承認 | `APPROVED` を記録 | 同上 | Implementation Plan Review Agent |
| 規約文書更新 | Phase C 完了を記録 | current PR、predecessor PR、関係一行説明付き進捗コメント | Verification Agent |
| 確認承認 | 確認節目を更新 | 確認結果と不足有無 | Verification Agent |
| レビュー承認 | Review A/B/C/D の承認完了を更新 | 各 review の結論要約 | Review A/B/C/D |
| 後続 issue 起票確認 | issue 番号を記録 | 後続 issue 番号と finding 紐付け | Verification Agent |
| conflict 解消確認 | conflict 状態を `closed` 記録 | conflict 解消報告 | Verification Agent |
| 未解決指摘ゼロ確認 | `Done` 直前にゼロ確認を記録 | `unresolved` 0、`same-class recurrence` 0 を報告 | Verification Agent |
| issue `#11` への報告 | 最終更新日時と報告完了を記録 | 完了報告 | Main Orchestrator |

issue `#11` が open の間は `tasks.md` を secret-recovery 全体の進捗正本として使う。issue `#11` close 後に新たな secret-recovery epic を始める場合は、着手前に後継 issue と後継 tasks 文書を新設または指定する。review、verification、指摘追跡、未解決ゼロ確認、Review D は今後の secret-recovery work 全体に適用する恒久規則であり、`tasks.md` はこれらを緩和または上書きしてはならない。

| 指摘元 | 指摘ID | 分類 | 状態 | 証跡パス | 次の対応 |
| --- | --- | --- | --- | --- | --- |
| Review A/B/C/D、Verification、issue `#11` progress freeze | 固有 ID | `responsibility-boundary` / `docs-contract` / `workflow-gap` / `same-class recurrence` / `follow-up issue required` | `resolved` / `design-accepted` / `follow-up-issued` / `unresolved` | `docs/secret-recovery/review-artifacts/architecture-rules/` 配下 | Phase A/B/C/F の戻り先に従う |
