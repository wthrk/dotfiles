# Hexagonal Implementation Rules

この文書は、特定の機能名や外部製品名に依存しない Hexagonal Architecture 実装規約の正本である。構造判断、責務分離、公開面、レビュー体制はこの文書を基準に行う。

## 目的

この文書の目的は、層ごとの責務、許可成果物、禁止成果物、依存方向、公開範囲、レビュー条件を固定し、構造の揺れや責務混在を防ぐことである。

## 哲学

### ドメインは技術を知らない

ビジネスロジックがインフラ（DB・外部API・SDK）の実装詳細に直接依存すると、インフラの変化がドメインの変化を強制する。ドメインが技術を知らないことで守られるのは「ビジネスルールがインフラ変更から独立して存続できる」という保証である。層境界はこの独立性を構造として固定するための手段であり、慣習ではない。

### ポートは意図の宣言である

`port` 層は「ドメインが何を必要とするか」を表明する契約の場所である。技術選定の詳細はポートに含まれない。ポートが技術詳細（SDK型・パーサー・プロンプト文言）を含んだ時点で、ドメインはその技術に依存し始める。ポートを純粋な意図の宣言にとどめることで、アダプターを差し替えてもドメインは変更を必要としない。

### アダプターは翻訳者である

`adapter` 層の役割は、外部技術とポートの契約の間を翻訳することである。ドメインはアダプターを知らず、アダプターはドメインのポートだけを知る。アダプターが `pub` や `pub(crate)` でポート以外の型・関数を公開した時点で、その公開面は「翻訳者」の役割を超えた結合点になる。呼び出し側がその公開面に依存し始めると、将来のアダプター差し替えはその依存を壊す。

### 公開面最小化は構造的制約である

`adapter` が公開してよいのは port trait を実装する型とそのメソッドのみである。これは「依存されると困るものを依存されないようにする」ための構造的制約であり、コンパイルが通ることとは無関係である。意図していない結合がコンパイルを通過して蓄積し、後の変更コストとして顕在化する。「動けばいい」という判断はこの制約を無効化し、構造的制約が存在する理由そのものを破壊する。

### 「動けばいい」がなぜ不十分か

今の動作は現在の環境・現在の依存・現在の要件に対する正しさを示すに過ぎない。層境界の違反は、違反した時点では機能を損なわないことが多い。問題は、次の変更・次のアダプター差し替え・次の要件変更の際に顕在化する。違反が累積するほど、次の変更コストは増大し、変更の安全性は低下する。「コンパイルが通った」「テストが通った」はレビューの合格根拠にならず、層制約に対する適合の証明にもならない。

## 適用範囲

この文書は、Hexagonal Architecture を採用する repository-authored code、補助文書、設計レビュー、実装レビュー、確認手順に適用する。個別機能の詳細な配置規則は、必要に応じて別文書でこの正本を参照して定義する。

## 層モデル

採用する層は `entrypoint`、`application`、`domain`、`port`、`adapter`、`support`、`tests` である。

- `entrypoint`: 外部入力境界を扱う。
- `application`: use case の順序を扱う。
- `domain`: 不変条件と wire format を扱う。
- `port`: 外部依存 contract を扱う。
- `adapter`: 外部 I/O 接続を扱う。
- `support`: 業務語彙を持たない共通技術部品を扱う。
- `tests`: 層ごとの契約確認を扱う。

## 層ごとの責務と成果物

`entrypoint` は利用者入力、CLI/API 境界、use case 起動を担う。許可する成果物は command 定義、引数値変換、呼び出し開始 DTO、終了 code 変換である。domain rule、本格的な順序制御、具体的な device 制御は置かない。

`application` は use case orchestration、停止条件、手順順序を担う。許可する成果物は use case 型、service 組立、分岐制御、要約生成である。具体 I/O、parser 実装、端末文言、外部 SDK 型は置かない。

`domain` は不変条件、状態遷移、wire format、値制約を担う。許可する成果物は enum/newtype、wire model、validation、domain error である。端末 I/O、プロセス制御、具体 adapter 呼び出しは置かない。

`port` は外部依存 contract の最小定義を担う。許可する成果物は trait、request/response 境界、capability 契約である。parser、DTO、prompt、利用者向け文言は置かない。

`adapter` は port 実装、外部 API 変換、環境差異吸収を担う。許可する成果物は SDK bridge、terminal bridge、filesystem bridge、JSON decode bridge である。use case の順序制御や domain policy の決定は置かない。

`support` は業務語彙を持たない共通技術部品を担う。許可する成果物は memory protection、clock、retry primitive、byte utility である。機能固有 vocabulary、command 名、role 名は置かない。

`tests` は層契約確認と回帰検知を担う。許可する成果物は unit test、integration test、test double、fixture である。本番公開 API やレビュー代替の設計判断は置かない。

## 層ごとの禁止事項

`port` に parser、DTO、prompt、具体的な利用者向け文言を置いてはならない。`adapter` に use case の順序制御を置いてはならない。`application` に concrete I/O を置いてはならない。`support` に業務語彙、command 名、feature 固有 state を置いてはならない。`domain` は外部 SDK 型、端末状態、プロセス状態へ依存してはならない。

## 標準モジュール構成とファイル構成

標準構成は次の分割を基準にする。

- `entrypoint/`: command 定義、外部入力値の境界変換
- `application/`: use case、flow、summary
- `domain/`: value、policy、wire-format、error
- `port/`: 外部依存 contract
- `adapter/`: device、terminal、filesystem、network、serialization
- `support/`: zeroization、protection、shared primitive
- `tests/`: layer-specific test

単一ファイルが terminal I/O、use case、wire format、外部 SDK 変換、test harness を同時に持ち始めた場合は、機能追加より先に責務ごとに sibling module へ分割する。

## 標準シンボル構成

標準シンボルは `Command`、`UseCase`、`Policy`、`Value`、`Port`、`Adapter`、`Summary`、`Report`、`Error` の役割を分離して命名する。閉じた集合は raw string で表現せず enum または newtype を使う。`Summary` と `Report` は application の出力専用にとどめ、port や adapter の公開 contract に流出させない。

## 公開範囲と再公開規則

`entrypoint` command は crate 内または binary 境界で公開し、shared library への再公開はしない。`application` use case は crate 内公開を基本とし、必要最小限の module 公開にとどめる。adapter crate からの再公開はしない。

`domain` value/policy は crate 公開または package 公開を許可するが、adapter convenience API への混在公開はしない。`port` contract は application と adapter の共有境界に限定し、end-user 向け API として再公開しない。

`adapter` concrete type は module 内または package 内に閉じる。domain/application から再公開してはならない。`support` utility は module 内または package 内に限定し、feature vocabulary を伴う公開をしない。`tests` helper は test target 内でのみ使い、本番 module へ再公開しない。

再公開は、domain value と port contract のように構造上の境界を明確化する場合だけ許可する。adapter convenience export によって依存方向を曖昧にしてはならない。

## 依存方向規則

依存方向は常に外側から内側へ向かう。`entrypoint` は `application` と `domain` に依存できるが、`adapter` の具体型へ直接依存してはならない。`application` は `domain`、`port`、`support` の機能中立な保護型に依存できるが、`entrypoint`、`adapter`、`support` の機能固有 API へ依存してはならない。

`domain` は言語標準 library 以外に依存しない。`port` は `domain` のみへ依存する。`adapter` は `port`、`domain`、`support` に依存できるが、`application` の use case 順序制御を持ってはならない。`support` は言語標準 library と外部技術 crate に依存できるが、他層の業務語彙へ依存してはならない。`tests` は対象層と test helper に依存する。

`application` が `support` へ依存できるのは、秘密保護、zeroization、ownership guard のような機能中立な保護型を一時保持し、その寿命管理責務を果たす場合に限る。`application` は `support` の機能固有 API や業務語彙を導入してはならない。`adapter` は `application` の flow decision を持たず、`port` は `adapter` 詳細や end-user 文言を持たない。

## ドキュメントコメント規則

各層で次を必須とする。

- この文書の comment / doc comment 規則は [AGENTS.md](../../AGENTS.md) の Code Style コメント規則を継承し、その適用範囲を狭めてはならない。
- 非自明な module、command entrypoint、use case、adapter、support utility には file-level comment または module doc comment を付ける。
- repository-authored explanatory comment は日本語で書き、周辺文脈が英語で固定されている場合だけ英語を許可する。
- comment は恒久的な設計意図、不変条件、制約、自明でない運用上の文脈を記し、低価値 comment、個人メモ、曖昧な TODO/FIXME を禁止する。
- comment が必要な場合はライフサイクル境界、外部契約、シグナル安全性要件、ワイヤ形式規則、セキュリティ特性、利用者操作制約のいずれかを具体名で記す。
- `application` の public command flow と非自明な private helper は、主要契約を先頭文で述べ、その後に必要入力、停止条件、外部 interaction boundary を記す。
- `domain` の value、policy、wire-format 型は、何を表すかを先頭文で記し、その後に不変条件、version rule、error 条件を書く。
- `port` の trait は、要求する capability と caller/implementor の責任分界を明記する。
- `adapter` の module comment は、どの port をどの外部 API へ接続するか、どの制約を内部で吸収するかを記す。
- `support` の comment は、セキュリティ特性、ライフサイクル境界、シグナル安全性要件、所有権規則のいずれかを具体名で記す。
- 関数、型、module の doc comment は主要契約を先頭文で述べ、条件、分岐、失敗時契約、caller responsibility は別文または別段落で続ける。
- 複数段落の doc comment は、先頭段落で通常系の主契約を示し、後続段落で非 TTY 動作、タイムアウト、所有権移譲、ゼロ化、ロック、出力安全性、再試行規則のような制約を記す。

## ディレクトリと層の対応規則

ディレクトリ名が層を決定する。ファイルの所属層は、そのファイルが置かれているディレクトリ名から導く。

| ディレクトリパターン | 所属層 |
|---|---|
| `<module>/adapters/` または `<module>/adapters.rs` | adapter |
| `<module>/application/` または `<module>/application.rs` | application |
| `<module>/domain/` または `<module>/domain.rs` | domain |
| `<module>/ports/` または `<module>/ports.rs` | port |
| `<module>/support/` または `<module>/support.rs` | support |
| `<module>/entrypoint/` または `<module>/entrypoint.rs` | entrypoint |
| `tests/` または `*_tests.rs`、`test_*.rs` | tests |

ディレクトリ名と層が一致しないファイルは配置違反とみなす。配置違反の解消は、ファイルを正しいディレクトリへ移動することで行う。個別機能の違反一覧は各機能の作業定義文書（`docs/tasks/<area>/work-items/`）を正本として管理する。

## レビュー観点

ディレクトリ別のチェック観点は [review-checklist.md](review-checklist.md) を参照する。層ごとの責務・禁止事項・依存方向・公開範囲はこの文書の各セクションを正本とし、review-checklist.md はその正本を引用してチェック項目を導く。

## エージェント運用とレビューの参照先

secret-recovery 固有の役割分担、段階運用、差戻し経路は [implementation-guidelines.md](../secret-recovery/implementation-guidelines.md) を単一正本として参照する。この文書に secret-recovery 固有の進捗運用を定義しない。

hexagonal review で確認するのは、層責務、依存方向、公開面、禁止成果物、comment 規則、正本参照整合である。secret-recovery のレビュー担当、進捗更新、記録契約の扱いは [implementation-guidelines.md](../secret-recovery/implementation-guidelines.md) とその参照文書に従う。
