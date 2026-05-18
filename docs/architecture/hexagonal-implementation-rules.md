# Hexagonal Implementation Rules

この文書は、特定の機能名や外部製品名に依存しない Hexagonal Architecture 実装規約の正本である。構造判断、責務分離、公開面、レビュー体制はこの文書を基準に行う。

## 1. 目的

この文書の目的は、層ごとの責務、許可成果物、禁止成果物、依存方向、公開範囲、レビュー条件を固定し、構造の揺れや責務混在を防ぐことである。

## 2. 適用範囲

この文書は、Hexagonal Architecture を採用する repository-authored code、補助文書、設計レビュー、実装レビュー、確認手順に適用する。個別機能の詳細な配置規則は、必要に応じて別文書でこの正本を参照して定義する。

## 3. 層モデル

採用する層は `entrypoint`、`application`、`domain`、`port`、`adapter`、`support`、`tests` である。`entrypoint` は外部入力境界、`application` は use case の順序、`domain` は不変条件と wire format、`port` は外部依存 contract、`adapter` は外部 I/O 接続、`support` は業務語彙を持たない共通技術部品、`tests` は層ごとの契約確認を所有する。

## 4. 層ごとの許可成果物

| 層 | 責務 | 許可成果物 | 禁止成果物 |
| --- | --- | --- | --- |
| `entrypoint` | 利用者入力、CLI/API 境界、use case 起動 | command 定義、引数値変換、呼び出し開始 DTO、終了 code 変換 | domain rule、本格的な順序制御、具体的な device 制御 |
| `application` | use case orchestration、停止条件、手順順序 | use case 型、service 組立、分岐制御、要約生成 | 具体 I/O、parser 実装、端末文言、外部 SDK 型 |
| `domain` | 不変条件、状態遷移、wire format、値制約 | enum/newtype、wire model、validation、domain error | 端末 I/O、プロセス制御、具体 adapter 呼び出し |
| `port` | 外部依存 contract の最小定義 | trait、request/response 境界、capability 契約 | parser、DTO、prompt、利用者向け文言 |
| `adapter` | port 実装、外部 API 変換、環境差異吸収 | SDK bridge、terminal bridge、filesystem bridge、JSON decode bridge | use case の順序制御、domain policy の決定 |
| `support` | 業務語彙を持たない共通技術部品 | memory protection、clock、retry primitive、byte utility | 機能固有 vocabulary、command 名、role 名 |
| `tests` | 層契約確認、回帰検知 | unit test、integration test、test double、fixture | 本番公開 API、review 代替の設計判断 |

## 5. 層ごとの禁止成果物

`port` に parser、DTO、prompt、具体的な利用者向け文言を置いてはならない。`adapter` に use case の順序制御を置いてはならない。`application` に concrete I/O を置いてはならない。`support` に業務語彙、command 名、feature 固有 state を置いてはならない。`domain` は外部 SDK 型、端末状態、プロセス状態へ依存してはならない。

## 6. 標準モジュール構成とファイル構成

標準構成は次の分割を基準にする。

- `entrypoint/`: command 定義、外部入力値の境界変換
- `application/`: use case、flow、summary
- `domain/`: value、policy、wire-format、error
- `port/`: 外部依存 contract
- `adapter/`: device、terminal、filesystem、network、serialization
- `support/`: zeroization、protection、shared primitive
- `tests/`: layer-specific test

単一ファイルが terminal I/O、use case、wire format、外部 SDK 変換、test harness を同時に持ち始めた場合は、機能追加より先に責務ごとに sibling module へ分割する。

## 7. 標準シンボル構成

標準シンボルは `Command`、`UseCase`、`Policy`、`Value`、`Port`、`Adapter`、`Summary`、`Report`、`Error` の役割を分離して命名する。閉じた集合は raw string で表現せず enum または newtype を使う。`Summary` と `Report` は application の出力専用にとどめ、port や adapter の公開 contract に流出させない。

## 8. 公開範囲と再公開規則

| 成果物種別 | 許可公開範囲 | 禁止公開範囲 | 補足 |
| --- | --- | --- | --- |
| `entrypoint` command | crate 内または binary 境界 | shared library の再公開 | 外部入力境界に閉じ込める |
| `application` use case | crate 内公開、必要最小限の module 公開 | adapter crate からの再公開 | orchestration detail を固定しない |
| `domain` value/policy | crate 公開または package 公開 | adapter convenience API への混在公開 | 不変条件の正本 |
| `port` contract | application と adapter の共有境界 | end-user 向け API としての再公開 | 最小 contract を維持する |
| `adapter` concrete type | module 内または package 内 | domain/application からの再公開 | 外部実装詳細を閉じる |
| `support` utility | module 内または package 内 | feature vocabulary を伴う公開 | 中立な技術部品に限定する |
| `tests` helper | test target 内 | production module への再公開 | 本番依存を作らない |

再公開は、domain value と port contract のように構造上の境界を明確化する場合だけ許可する。adapter convenience export によって依存方向を曖昧にしてはならない。

## 9. 依存方向規則

| 依存元の層 | 依存可能な層 | 依存禁止の層 |
| --- | --- | --- |
| `entrypoint` | `application`, `domain` | `adapter` の具体型への直接依存 |
| `application` | `domain`, `port`, `support` の機能中立な保護型 | `entrypoint`, `adapter`, `support` の機能固有 API |
| `domain` | なし、または言語標準 library のみ | `application`, `port`, `adapter`, `support` |
| `port` | `domain` | `entrypoint`, `application`, `adapter`, `support` |
| `adapter` | `port`, `domain`, `support` | `application` の use case 順序制御 |
| `support` | 言語標準 library、外部技術 crate | `entrypoint`, `application`, `domain`, `port`, `adapter` の業務語彙 |
| `tests` | 対象層、test helper | 対象外層の責務迂回 |

依存方向は常に外側から内側へ向かう。`application` が `support` へ依存できるのは、秘密保護、zeroization、ownership guard のような機能中立な保護型を一時保持し、その寿命管理責務を果たす場合に限る。`application` は `support` の機能固有 API や業務語彙を導入してはならない。`adapter` は `application` の flow decision を持たず、`port` は `adapter` 詳細や end-user 文言を持たない。

## 10. ドキュメントコメント規則

各層で次を必須とする。

- この文書の comment / doc comment 規則は [AGENTS.md](/Users/ya/works/dotfiles/AGENTS.md) の Code Style コメント規則を継承し、その適用範囲を狭めてはならない。
- 非自明な module、command entrypoint、use case、adapter、support utility には file-level comment または module doc comment を付ける。
- repository-authored explanatory comment は日本語で書き、周辺文脈が英語で固定されている場合だけ英語を許可する。
- comment は durable project intent、invariant、constraint、non-obvious operational context を記し、低価値 comment、個人メモ、曖昧な TODO/FIXME を禁止する。
- comment が必要な場合は lifecycle boundary、external contract、signal-safety requirement、wire-format rule、security property、user interaction constraint のいずれかを具体名で記す。
- `application` の public command flow と非自明な private helper は、主要契約を先頭文で述べ、その後に必要入力、停止条件、外部 interaction boundary を記す。
- `domain` の value、policy、wire-format 型は、何を表すかを先頭文で記し、その後に不変条件、version rule、error 条件を書く。
- `port` の trait は、要求する capability と caller/implementor の責任分界を明記する。
- `adapter` の module comment は、どの port をどの外部 API へ接続するか、どの制約を内部で吸収するかを記す。
- `support` の comment は、security property、lifecycle boundary、signal-safety requirement、ownership rule のいずれかを具体名で記す。
- 関数、型、module の doc comment は主要契約を先頭文で述べ、条件、分岐、失敗時契約、caller responsibility は別文または別段落で続ける。
- 複数段落の doc comment は、第 1 段落で通常系の主契約、第 2 段落以降で non-TTY behavior、timeout、ownership transfer、zeroization、locking、output safety、retry rule のような制約を記す。

## 11. エージェント運用規則

| 役割 | 必要入力 | 必要出力 | コマンド実行可否 | ファイル編集可否 |
| --- | --- | --- | --- | --- |
| Main Orchestrator | 計画、進捗、承認状態 | 役割割当、段階遷移、完了判断 | 不可 | 不可 |
| Planning Agent | 要求、既存文書、関連差分 | 計画案、見出し案、判断根拠 | 可 | 可 |
| Implementation Agent | 承認済み計画、対象文書 | 実装差分、更新済み成果物 | 可 | 可 |
| Verification Agent | 実装差分、対象文書、証跡 | 確認結果、欠落一覧、承認または差戻し | 可 | 可 |
| Review Agent | 凍結対象、差分、証跡 | 指摘、承認、差戻し理由 | 可 | 可 |

実装、確認、コマンド実行、検証、レビューはサブエージェントが行う。メインエージェントはオーケストレーションだけを行い、差分作成や証跡生成を兼務してはならない。

## 12. レビューと承認規則

| レビュー役割 | 必須確認項目 | 承認条件 | 失敗条件 |
| --- | --- | --- | --- |
| Architecture Review | 層責務、依存方向、公開面、禁止成果物 | required heading/table が揃い、依存方向違反がない | 責務混在、禁止成果物の容認、依存表の欠落 |
| Documentation Review | 用語整合、doc comment 規則、見出し順、表列 | 規約文書同士の語彙と契約が一致する | 見出し順の崩れ、語彙の不一致、必須列欠落 |
| Verification Review | リンク、証跡、参照正本、差戻し経路 | cross-link と artifact path が追跡可能 | 参照切れ、証跡欠落、戻り先不明 |
| Final Approval | 全役割承認、未解決ゼロ、進捗更新完了 | required approval がそろい未解決が 0 | 承認不足、未解決残存、進捗記録未更新 |

承認は、required review role の出力がすべてそろうまで成立しない。レビューは「概ね問題なし」で閉じず、未解決指摘、差戻し先、証跡パスを明示して閉じる。
