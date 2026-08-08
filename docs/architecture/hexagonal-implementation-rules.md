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

この制約は公開シンボルに限らない。private な関数・ロジックであっても、その内容が翻訳（外部技術の型をポートの型に変換すること）以外の責務——use case の順序制御、domain policy の決定、ビジネスロジックの判断——を含む場合、それはアダプターに属さない。public/private の区別はこの制約に影響しない。

この原則はすべての層に共通する。port 層であれば「ドメインが何を必要とするかの宣言のみ」という責務制約は private な型・関数にも適用される。support 層であれば「共通技術部品または secret 保護の backend 境界実装」という責務制約は private な実装にも適用される。visibility はシンボルの見え方を制御するが、そのコードが属すべき層の責務を変えない。

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
- `support`: 共通技術部品と、secret 保護境界の backend 実装を扱う。
- `tests`: 層ごとの契約確認を扱う。

## 層ごとの責務と成果物

`entrypoint` は利用者入力、CLI/API 境界、use case 起動を担う。許可する成果物は command 定義、引数値変換、呼び出し開始 DTO、終了 code 変換である。domain rule、本格的な順序制御、具体的な device 制御は置かない。

`application` は use case の順序制御、停止条件、分岐だけを担う。許可する成果物は use case ごとの `run_*` 関数と、その関数内での port 呼び出し順序・停止条件・分岐制御である。use case 独自型、要約型、意味や方針の決定、service 組立、具体 I/O、parser 実装、端末文言、外部 SDK 型は置かない。

`domain` はビジネスロジックを担う中核層である。ビジネス上の意味、判断、規則、失敗条件を最初に domain の責務として検討し、そのうち外部 I/O や use case 順序に属さないものを domain object / domain policy として表現する。許可する成果物は enum/newtype、wire model、validation、domain error、不変条件、状態遷移、値制約、オブジェクト間の関連・変換・整合判定である。端末 I/O、プロセス制御、具体 adapter 呼び出しは置かない。

`port` は外部依存 contract only の層であり、「何を必要とするか」の最小定義だけを担う。許可する成果物は trait、request/response 境界、capability 契約である。parser、DTO、prompt、利用者向け文言、意味づけ済みの実装詳細は置かない。

`adapter` は port 実装、外部 API 変換、環境差異吸収という技術的な実行と翻訳だけを担う。許可する成果物は SDK bridge、terminal bridge、filesystem bridge、JSON decode bridge である。use case の順序制御、domain policy の決定、summary の意味づけは置かない。

adapter 層で domain object のビジネスロジックを直接実行してはならない。具体的には、manifest/blob の整合判定、nonce/AAD の業務規則決定、`SecretName::additional_data` の業務意味適用、鍵生成可否などの業務判断は application/domain 側の責務であり、adapter はそれらを呼び出すための外部 I/O 翻訳に限定する。

アダプター層に置いてよいファイルは「特定の port trait を実装するファイル」のみである。以下のファイルはシンボルの公開範囲に関わらずアダプター層に属してはならない。

- 再エクスポート集約ファイル（`use` 宣言のみで構成されるファイル、または `pub(super) use` で他 adapter の関数・型を束ねるだけのファイル）はアダプター層に置いてはならない。
- JSON パーサー・デコーダー・ファイル読み取り関数群等、port trait を実装しない純粋なユーティリティファイルはアダプター層に置いてはならない。これらは `support/` 層（業務語彙を持たない場合）または他の適切な層に属する。

`support` は共通技術部品と、secret 保護境界の backend 実装を担う。許可する成果物は memory protection、clock、retry primitive、byte utility、protected secret 操作である。一般の support utility には command 名や role 名を置かない。secret 保護境界では、外部 SDK、暗号処理、device API が secret の借用または所有 plaintext buffer の move を要求する場合に限り、専用の backend 操作として product/service-specific な request 組み立てと呼び出し境界を持てる。

`tests` は層契約確認と回帰検知を担う。許可する成果物は unit test、integration test、test double、fixture である。本番公開 API やレビュー代替の設計判断は置かない。

production 層の `src/` ファイルに置かれた通常の inline unit test（`#[cfg(test)] mod tests { #[test] fn ... }`）は許可する。これはその module 自身の private 関数を検証する標準的な Rust であり、削除を要求してはならない。inline unit test を一律禁止すると、本番関数をテストのためだけに `pub` 化する圧力が生じ、公開面最小化に反する。`#[test]` の存在だけを理由に配置違反としてはならない。禁止対象は、実依存を肩代わりする test double の**定義**が production 層に置かれることであり、判定は形式（`#[cfg(test)]` か `#[cfg(feature)]` か）ではなく責務で行う。

### internal backend stub の配置

test double / fixture の本体は原則として `tests/` 配下に置く。ただし、CLI integration test が同一 `dotfiles` binary と同一 production command path を通り、外部 backend だけを compile-time で差し替える必要がある場合、adapter 配下に internal backend stub を置ける。

この adapter 配下 stub は次の条件をすべて満たす場合に限り、production source tree への test double 混入とは扱わない。

- production build に含まれない。
- internal test 専用 feature に限定される。
- runtime の real/stub 分岐を作らない。
- production command path を変更しない。
- port trait 契約で usecase を駆動する。
- domain/business logic を test stub 側へ移さない。
- integration test は adapter stub module を import せず、feature 有効でビルドされた `dotfiles` binary を実行する。
- test 側は「初期 datastore 定義」と「CLI 実行後の最終 datastore 観測」だけを扱い、backend state schema・状態遷移 helper・write event helper・bincode schema・backend 内部保存形式を保持してはならない。
- 最終 datastore 観測は `secrets-internal-test-stub` feature 専用の stdout sentinel observation を基本とする。この observation は test-only の明示観測面であり、fixture/spec で与えたダミー secret 値を含めてよい。integration test が「secret として保存した値が最終 datastore に意図通り保存された」ことを検証するためであり、production build/runtime には compile されず、本物 secret の出力経路ではない。
- hidden temp file、output path file、共有 state file に secret 値を残してはならない。
- backend state/schema/helper は adapter 側 internal backend stub の責務とし、`tests/` 側へ複製してはならない。
- BWS port stub と YubiKey port stub は独立させ、共通の巨大 StubState や共有 state file で結合してはならない。port 間の結合は application/domain の通常経路でのみ発生させる。
- module/comment で internal test 専用 feature、production build 非混入、stdout observation 境界、compile-time selection を明記する。

この許可は external backend 翻訳の test 専用 adapter stub に限る。adapter の不要な `pub(super)` helper、runtime の real/stub 分岐、domain/business logic の stub への移動、production command path の差し替え、integration test fixture builder / assertion helper の adapter 側混入、`tests/` 側での backend state/schema/helper 保持は、この条件を満たさないため引き続き禁止する。

application 層の use case orchestration test は、internal test stub feature から切り離す。`application` production code や app 層 inline test に `secrets-internal-test-stub` feature gate / bridge を置いてはならない。app 層 inline/unit test は `tests/` 配下の module、support、fixture、file を `#[path]`、`include!`、または test support module 経由で参照してはならない。`rust/dotfiles-secrets/src/application/app_test_support.rs` のような app 層共有 test support file を作ってはならない。private usecase を同一 module context で検査する場合は、各 `run_*.rs` の `#[cfg(test)] mod tests` 内で、port trait から生成した `mockall` mock を直接組み立てる。event recorder、巨大な状態管理 harness、port trait と別に動くテスト専用実装を作ってはならない。port trait の mock は trait 側の test-only `mockall::automock` などから生成し、既存 trait method を `mock!` macro へ手で書き写して二重管理してはならない。

secret 値の test-only 観測は `support/protection::ProtectedSecret` の `#[cfg(test)]` 最小関数へ閉じる。domain value や application code に secret 生値取り出し API を増やしてはならない。この許可は `String` 変換公開、production 経路での取り出し、汎用 plaintext consumer API、または外部 SDK/API 呼び出しを protection 境界外へ移す根拠にしてはならない。

secret-recovery では domain object / port 境界が repository 所有 secret を値として運ぶ必要がある場合、`ProtectedSecret` をその carrier として直接保持・受け渡ししてよい。これは secret 生値取り出し、backend 抽出、downcast、汎用 writer、汎用 plaintext consumer API を許可しない。外部 SDK、PIV、sealed blob、stdout などで平文 borrow または owned plaintext buffer が必要な処理は、用途別の `support/protection` 操作内で完了させる。

## 責任分離の判断原則

層の所属は、処理が低水準か高水準かではなく、その処理がどの責任を持つかで判断する。暗号、binary codec、SDK 呼び出し、JSON、端末 I/O のように技術的に見える処理であっても、業務上の意味、保存可能条件、オブジェクト間の整合、変換規則、状態遷移を決めているなら、その決定部分は domain または application の責務である。逆に、実行手段や外部 API への型変換だけであれば adapter / support の責務になりうる。

実装・レビューでは、移動や分割の前に各処理を次のいずれかへ分類しなければならない。

- `domain rule`: 外部実装を差し替えても変わらない業務上の意味、不変条件、整合判定、失敗条件、対象同一性、値制約。
- `application orchestration`: domain rule と port capability を適用する順序、停止条件、分岐、外部確認 plan の進行。
- `port contract`: application/domain が外部境界へ要求する capability と最小境界型。
- `adapter translation`: port 境界型と外部 SDK / process / filesystem / serialization API の相互変換。
- `support technical primitive`: product 非依存の技術 primitive、または secret 保護境界内で平文借用・外部処理呼び出し・暗号化/復号・sealed blob 操作・zeroize を閉じる専用 backend 操作。

分類できない処理は、どの層へ移しても合格にしてはならない。処理を移動する前に、その処理が既存規定上どの境界の責務かを判定し、規定済みの境界に置くこと。`adapter` から `support` へ移す、ファイルを細分化する、private helper を消す、といった機械的分離は責務分離の十分条件ではない。レビューは「どの層へ移したか」ではなく、「その処理がなぜその規定済み境界の責務なのか」を根拠として要求する。

`domain` に置くべきものは、まずビジネスロジックである。オブジェクト単体の値制約だけに限定してはならない。オブジェクト同士の関連づけ、保存 layout の対応、変換の正当性、整合判定、状態遷移、業務上の失敗条件、別の実装へ差し替えても変わらない規則は domain の内側に属する。これらを `application` の if 文や `adapter` の helper に散らすと、業務規則が手順や I/O 翻訳へ漏れ、次の差し替え時に同じ規則を再実装することになる。

`application` に置くべきものは、domain 規則と port capability をどの順序で適用するかという use case の進行だけである。application は「manifest を検証してから保存する」「PIN が必要なら入力を要求してから復号する」のような順序・停止条件を持てるが、manifest の正しさ、blob の正当な構造、AAD の構築規則、secret 値の妥当性、上書き可否の意味そのものを独自に定義してはならない。application に同じ object 操作が反復している場合は、重複を helper 化する前に domain object として表現できないかを判断する。

`port` は責務を実行しない。port は application/domain が外界へ要求する capability と境界型だけを宣言する層であり、manifest 検証、blob decode、nonce/AAD 構築、暗号方式の選択、上書き判定、device 選択方針のような処理を隠す fat port にしてはならない。port のメソッド名が「何を必要とするか」ではなく「どう実現するか」や「複数責務をまとめて済ませること」を表し始めた場合は、capability 分割または domain object の再設計が必要である。

storage backend が暗号化された永続化機構を内包する場合、port は暗号方式や sealed blob 形式ではなくデータストアの capability を公開する。application/domain から見える契約は「対象 secret を保存する」「保存済み secret を取得する」「必要な datastore 状態を確認する」といった外部依存要求に限定し、暗号化・復号・sealed blob encode/decode・repository 所有 buffer の zeroize は backend 内部機能として隠蔽する。これは crypto/blob 処理を port へ露出しないための責務分離であり、setup 済みか、何が不足しているか、どの secret を必須とするか、固定 key/name/role の意味、一意解決、0件/複数件の failure 化、外部確認 plan の進行を backend/support へ移す許可ではない。

`adapter` は翻訳だけを行う。adapter は port で受け取った domain object / 境界型を外部 SDK、process I/O、filesystem、network、serialization API へ渡す形に変換し、外部 API から戻った値を port の返却型へ戻す。adapter 内で domain object の操作、業務判断、オブジェクト間の対応づけ、AAD/nonce/manifest/blob の意味づけ、保存可否判断、上書き判定を直接実行してはならない。adapter に許される private helper も同じ制約を受け、helper であれば business logic を持てるという例外はない。

`support` の一般 utility はプロダクト非依存の技術 primitive を基本にする。memory protection、zeroization、AEAD/OAEP などの暗号 primitive、process-generic な byte / I/O 補助は support に置ける。特定機能専用の codec や storage format は、単に binary/crypto を扱うという理由だけで support に置かない。例外として、storage backend が暗号化・復号・sealed blob を内部機能として隠蔽する場合、その backend 実装依存の暗号処理、sealed blob encode/decode、protection/zeroize/core dump 保護は support/protection の専用 backend 操作として置ける。技術 primitive と機能固有 storage mechanism を分け、後者が backend 内部機能を越えて業務判断を持つなら既存規定上の責務境界に合わせて配置する。

secret-recovery の `support/protection` は、この一般 utility とは別に secret 保護の backend 境界実装を持てる。ここでは product/service-specific な名前が現れること自体を層違反にしない。判定基準は、その操作が secret の借用、所有 plaintext buffer の作成、外部 SDK/暗号/device API 呼び出し、repository 所有 buffer の zeroize を同じ protection 境界内で完了させるための専用 backend 操作かどうかである。application/domain/ports へ SDK 型や平文 buffer API を漏らすこと、汎用の plaintext consumer API を作ること、use case 手順や domain policy を support に移すことは引き続き禁止する。

`support` は逃げ場ではない。処理が `support` 配下にあること、product/service-specific な SDK 呼び出しを protection 境界内で完了していること、または private helper になっていることは、domain/usecase logic 混入の免除理由にならない。薄い port を保つために adapter/support へ業務判断を押し込むこと、adapter を薄くするために support へ逃がすことは禁止する。ただし、backend 実装依存の技術補助、SDK 呼び出しの安全な補助、暗号化/復号/sealed blob/protection/zeroize/core dump 保護などの storage backend 内部機能、業務判断を含まない変換は support に置ける。固定 key / name / role に基づく意味づけ、一意解決の業務規則、0件/複数件の domain failure 化、外部確認の実行 plan、取得対象の過不足判定、業務上の停止条件は `support technical primitive` ではない。これらは既存規定上の該当境界に置き、support は必要な技術補助と平文保護境界だけを担う。

`support` は「外部 I/O を一切持ってはいけない」層ではない。process/terminal I/O のうち、TTY を開く、echo/raw mode を制御する、標準入出力を byte stream として読む、signal/interrupt と blocking read の安全性を扱う、といった process-generic な低レベル実装支援は support に置ける。これは外部境界の use case 方針や device 選択を support が決めることを許すものではなく、adapter など外部境界実装が利用する技術 primitive を隔離するための配置である。

domain/application は support の process/terminal I/O helper を直接呼んではならない。外部 interaction を必要とする use case は port capability を通じて adapter を呼び、adapter が必要な低レベル I/O helper として support を使う。support に業務判断、domain object 操作、prompt 文言、device 選択方針、use case 手順を入れてはならない。逆に、process-generic helper を support へ分離できる場面で adapter に端末制御や blocking read の補助実装を直詰めし、port 翻訳以外の技術補助で adapter を fat にしてはならない。

## secret-recovery の層別判断（具体化）

以下は本 repository の `dotfiles secrets` で繰り返し誤配置が起きた論点を、allowed / forbidden / 典型誤配置 / 判定質問 / 具体例で固定する。

### application

- allowed:
  use case の順序制御、停止条件、分岐、port 呼び出しの順番
- forbidden:
  command dispatch、input modality の分岐詳細（Prompt/Stdin/StdinJson 等）、report DTO の JSON 変換、protected buffer 化、crypto helper 呼び出し詳細、device selection 実装、use case 独自型定義
- 典型的な誤配置:
  `application` が `--stdin`/`--stdin-json` の実装詳細を enum で保持する、`println!` で report を整形する、`YubiKey::open_*` 相当を直接呼ぶ
- 判定質問:
  「このコードは手順の宣言だけか。具体 I/O 形式・デバイス選択・シリアライズ仕様を知っていないか」
- この repo の具体例:
  `run_with_args` での subcommand 分岐は entrypoint、`run_enroll_primary_with` の手順制御は application、JSON report 生成は adapter

### ports

- allowed:
  capability 契約（例: `read_secret_from_prompt`、`read_secret_from_stdin`、`write_secret`、`write_*_report`）
- forbidden:
  input modality の手段表現そのもの（`Prompt/Stdin/StdinJson` enum）、report DTO の具体型、prompt 文言、stdin JSON parser
- 典型的な誤配置:
  `read_secret(name, source_enum)` のように手段を port 契約へ露出する、`EnrollmentJson` DTO を ports に置く
- 判定質問:
  「この契約は“何をしたいか”だけを表しているか。“どう入力するか/どう表示するか”を含んでいないか」
- この repo の具体例:
  `SecretInputPort` は modality enum を持たず、`read_secret_from_prompt` と `read_secret_from_stdin` を別 capability として宣言する

### adapters

- allowed:
  device selection（serial 指定・対話選択）、stdin/stdout/terminal I/O、report DTO 変換、外部 API 変換
- forbidden:
  application 型への直接依存、use case の順序決定、業務判断の中心化
- 典型的な誤配置:
  adapter が `domain::...Summary` を import して report を生成せず presentation DTO を持ち込む、adapter 内で command flow の可否を決める
- 判定質問:
  「このコードは port 契約と外部技術の翻訳だけか。手順そのものを決めていないか」
- この repo の具体例:
  `adapters/io/report.rs` は `ReportPort` の出力契約を実装し、summary の表示形式・シリアライズ形式だけを adapter 側で決める。summary 自体の意味は domain 型として保持する

### support

- allowed:
  protected buffer 化、zeroization、暗号プリミティブ helper（AEAD/OAEP）、secret 保護境界内で完了する外部処理向け backend 操作、storage backend 内部の暗号化・復号・sealed blob 操作
- forbidden:
  stdin-json、enroll/verify など command 手順の語彙、feature-specific な prompt 文言や device 選択方針、固定 secret key/name/role に基づく一意解決や 0件/複数件の業務判断、外部確認 plan。secret 保護境界の専用 backend 操作では YubiKey や Bitwarden などの外部処理名を持てるが、平文 buffer を返す public API や汎用 consumer API は持てない
- 典型的な誤配置:
  `support/aead.rs` が「YubiKey secret」など機能固有語彙を返す、support が specific command の prompt 文言や選択方針を持つ、`support/protection/bws.rs` が固定 BWS secret name の一意解決や `verify-yubikey --check bws` の成功条件を決める、storage backend の sealed blob helper が setup 済み判定や必須 secret の過不足判定を決める
- 判定質問:
  「この部品は共通技術 primitive か、secret 保護境界を閉じる backend 操作か。storage backend 内部機能であれば暗号化・復号・sealed blob・protection・zeroize・core dump 保護に限定されているか。I/O を扱う場合、それは process-generic な実装支援か、feature-specific な interaction 方針か。対象同一性・一意性・0件/複数件・外部確認 plan を決めていないか」
- この repo の具体例:
  `support/aead.rs` は `protected payload` のような汎用語彙に限定し、device 名を含めない。`support/process_io.rs` のような process-generic terminal/stdin/stdout helper は、domain/application から直接使わせず adapter から利用する支援境界として置ける。BWS SDK が access token の owned plaintext を要求する呼び出し境界は `support/protection` に置ける。storage backend が sealed blob を内部保存形式として使う場合、その暗号化・復号・sealed blob 操作は `support/protection` に置けるが、`gpg-secret-key-backup` / `password-store-remote` を固定取得対象として一意解決する規則、0件/複数件の扱い、`verify-yubikey --check bws` の外部確認 plan は既存規定上の該当境界に置く。

## 層ごとの禁止事項

`port` に parser、DTO、prompt、具体的な利用者向け文言を置いてはならない。`adapter` に use case の順序制御を置いてはならない。`application` に concrete I/O を置いてはならない。`support` に業務語彙、command 名、feature 固有 state、固定 secret key/name/role に基づく対象同一性・一意性・0件/複数件の業務判断、外部確認 plan を置いてはならない。`support` に process/terminal I/O があること自体を禁止根拠にしてはならず、禁止対象は feature-specific な interaction 方針、業務判断、domain object 操作、application からの直接利用である。`domain` は外部 SDK 型、端末状態、プロセス状態へ依存してはならない。

## 標準モジュール構成とファイル構成

標準構成は次の分割を基準にする。

- `entrypoint/`: command 定義、外部入力値の境界変換
- `application/`: use case、flow
- `domain/`: value、policy、summary、wire-format、error
- `port/`: 外部依存 contract
- `adapter/`: device、terminal、filesystem、network、serialization
- `support/`: zeroization、protection、shared primitive
- `tests/`: layer-specific test

単一ファイルが terminal I/O、use case、wire format、外部 SDK 変換、test harness を同時に持ち始めた場合は、機能追加より先に責務ごとに sibling module へ分割する。

### ファイル分割の判断基準

ファイルを分割してよい理由は「独立した責務の境界が存在するから」のみである。以下の理由はファイル分割の正当な根拠にならない。

- 「長くなったから」——行数はファイル分割の根拠にならない。長いファイルが単一の責務を持つなら分割してはならない。
- 「再利用したいから」——再利用の都合は実装都合であり、責務の境界ではない。再利用したいコードがあるなら、その再利用の正当な置き場所（共有可能な層）を責務から判断せよ。
- 「まとめたいから」——集約の都合も実装都合である。集約ファイル・再エクスポートファイルは責務の境界を表さない。

再エクスポートのみで構成されるファイル（`use` 宣言・`pub(super) use` のみで実体を持たないファイル）は責務の境界を表さないため、どの層にも置いてはならない。ファイルに実装の実体がなければ、そのファイルが存在する意味を一文で述べられない。述べられないなら削除または統合せよ。

`application/` または `adapters/` に private helper が存在すること自体は許可されるが、helper を「設計逃がし（本来は port 境界や層境界で解くべき責務を局所関数へ退避する行為）」として使ってはならない。helper の責務が層責務を超えた時点で設計誤りである。`adapters/` の helper が port 契約の不足を肩代わりし始めた場合は、まず helper を増やすのではなく port 契約の粒度（capability 分割、request/response 境界）を再設計すること。

file-level の分割（`xxx.rs` を `xxx/` 配下へ分ける等）は責務分離の十分条件ではない。分割後も helper ごとに「この helper の責務は何か」「その責務はこの層に属するか」を確定できなければ不合格である。

## application 層のサブモジュール分割方針

`application` 層のロジックはユースケースの責務ごとにサブモジュールへ分割する。`entrypoint` が command 選択を担い、`application` は選択済み use case の orchestration に専念する。

`application` 層では use case orchestration だけを保持し、command dispatch は `entrypoint` 層へ置く。ユースケースが必要とする補助処理（manifest 読み書き、blob 変換、暗号呼び出し順序など）のうち技術実装に属するものは `adapter` / `support` 側へ移し、application 側は順序制御と分岐だけを保持する。

`application.rs` が `application/` 直下の `run_*.rs` にある同等関数へ単純委譲するだけの façade になる構造は許可しない。公開面と実装が二重化し、責務境界ではなく導線だけが増えるためである。`application` の公開面を残すならそこで責務を持つこと。責務を持たない場合は module 宣言に縮退させること。

`application` の分割・配線では、次を強制する。

- 乱数生成（key material、nonce、challenge、seed 等を含む）は `application` から直接呼び出してはならない。必ず `port` 契約を経由して取得すること。
- use case は独自の型定義を持ってはならない。use case で扱う型は `domain` 層で定義された型のみを使用すること。
- 各 use case は 1 つの `run_*` 関数として表現し、1 use case = 1 function を崩してはならない。
- `rust/dotfiles-secrets/src/application/` 配下では sibling file ごとに `run_*` 関数を 1 つだけ持たせること。複数 use case を 1 ファイルへ混在させてはならない。
- `application/use_case.rs` を新設してはならない。use case 実装は `application/` 直下の sibling `run_*.rs` に限定し、疑似レイヤーを増やしてはならない。
- `application/use_case/` ディレクトリを新設してはならない。`application` 直下の sibling file だけで構成し、`application` 配下に疑似レイヤーを増やすことを禁止する。
- `mod.rs` を新設してはならない。`#[path = \"...\"]` による分割・配線も禁止する。標準の module 宣言だけで配線すること。
- ある use case から別の use case を呼び出してはならない。共通順序を作りたくなった場合でも use case-to-use case call で接続してはならない。
- use case 層での logic commonization は禁止する。重複を許容し、共通化が必要なら domain / port / adapter / support の責務として再設計すること。

ファイル分割の根拠はここでも「独立した責務の境界が存在するから」のみである。「長くなったから」「まとめたいから」はサブモジュール分割の正当な理由にならない（[ファイル分割の判断基準](#ファイル分割の判断基準) を参照）。

## summary / reporting 型の配置

use case 独自型は定義してはならない。`EnrollSummary`、`VerifySummary` のように use case が入出力で扱う outcome 型も、`application` ではなく `domain` 層の型として定義する。application はそれらの domain 型を順序制御に利用するだけであり、独自の outcome / summary / result 型を所有してはならない。

一方で、JSON key 名、status の文字列表現、pretty-print の有無、writer 選択などの **presentation 仕様** は domain に置かない。これは adapter（出力境界翻訳）の責務である。domain が保持してよいのは outcome の意味・方針・summary semantics だけであり、表示形式・シリアライズ形式は持たない。

判断基準は以下のとおりである。

- その型は「ユースケース結果の意味」か「外部出力フォーマット」か。前者は domain、後者は adapter。
- その型が変わる理由は業務手順（チェック項目の追加/削除）か、表示仕様（JSON key・status 表記・出力整形）か。前者は domain、後者は adapter。

`application` 層に outcome/reporting 型を定義してはならない。`domain` は不変条件・値制約・wire 仕様に加えて、use case が扱う純粋な result / summary value を保持できる。`port` には報告出力 capability（例: `write_report`）のみを置き、具体 DTO は公開 contract に流出させない。

`application` 層で許可される use case 固有の構造体は、`run_*` 関数の実行時依存を束ねる runtime/dependency bundle に限る。bundle は domain 入出力・summary・policy ではなく、`run_*` 関数へ渡す既存 port trait 実装への参照を named fields で保持するだけの型でなければならない。`Ports` という名前を使ってはならない。`Port` / `Ports` は port trait 契約そのものにだけ使う。bundle 名は実行環境の依存束であることを表す `*Runtime` / `*Environment` / `*Gateway` 等から選び、port trait そのものと誤認させてはならない。

runtime/dependency bundle は `too_many_arguments` を隠すための無意味な詰め替えであってはならない。bundle 化しても、port trait が過分割で use case が多数の微細 capability を直接調停している状態は解消扱いにしない。レビューでは、引数数だけでなく「その依存集合が cohesive capability としてまとまっているか」「既存 port 境界を細分化しすぎて application が外部機能の配線表になっていないか」を判定する。同期的な port 群を具体型 generic の羅列として保持するだけなら、trait object 参照や use case が実際に要求する runtime/environment 境界へ寄せられないかを確認する。async trait など Rust の object-safety 制約で generic が必要な場合でも、その generic は必要な capability に限定し、全依存を型パラメータへ展開してはならない。

## port 契約の分割方針

1 つの巨大 trait に無関係な外部機能を混在させてはならない。`port` は「大まかな外部機能」単位で trait を切り、1 trait = 1 外部機能を基本原則とする。

- 例: DB 接続系 capability は 1 trait に集約する。
- 例: ファイルアクセス系 capability は 1 trait に集約する。
- 例: CUI でも `入力`・`出力`・`報告` のように変更理由が異なる capability は分け、同じ変更理由を共有する機能だけを 1 trait にまとめる。

機能内で細分化が必要な場合でも、最終的な use case 境界は外部機能単位 trait（supertrait を含む）で表現し、adapter 実装と module 分割も同じ外部機能単位へ揃えること。

port 分割は「細かいほどよい」ではない。1 method ごとの trait、入力・確認・出力の偶発的な組み合わせ、同じ外部機能を使うたびに増える command 専用 trait、application の都合だけで作った wrapper trait は過分割として扱う。port は変更理由と外部機能の境界で切り、use case 側で常に同じ複数 trait を同時に要求しているなら、cohesive capability として統合するか、上位 capability 境界で要求できないかを先に検討する。逆に、統合によって use case 手順・domain rule・adapter translation を port に隠す場合は fat port として不合格にする。

## 標準シンボル構成

標準シンボルは `Command`、`UseCase`、`Policy`、`Value`、`Port`、`Adapter`、`Summary`、`Report`、`Error` の役割を分離して命名する。閉じた集合は raw string で表現せず enum または newtype を使う。`Summary` は domain の value、`Report` は adapter の presentation 変換として扱い、port の公開 contract に具体 DTO を流出させない（詳細は [summary / reporting 型の配置](#summary--reporting-型の配置) を参照）。

## 公開範囲と再公開規則

`entrypoint` command は crate 内または binary 境界で公開し、shared library への再公開はしない。`application` use case は crate 内公開を基本とし、必要最小限の module 公開にとどめる。adapter crate からの再公開はしない。

`domain` value/policy は crate 公開または package 公開を許可するが、adapter convenience API への混在公開はしない。`port` contract は application と adapter の共有境界に限定し、end-user 向け API として再公開しない。

`adapter` concrete type は module 内または package 内に閉じる。domain/application から再公開してはならない。`support` utility は module 内または package 内に限定し、feature vocabulary を伴う公開をしない。`tests` helper は test target 内でのみ使い、本番 module へ再公開しない。

再公開は、domain value と port contract のように構造上の境界を明確化する場合だけ許可する。adapter convenience export によって依存方向を曖昧にしてはならない。

## 依存方向規則

依存方向は常に外側から内側へ向かう。`entrypoint` は `application` と `domain` に依存できるが、`adapter` の具体型へ直接依存してはならない。`application`（`application/use_case` を含む）は `domain` と `port` にのみ依存できる。`entrypoint`、`adapter`、`support` への依存は禁止する。

`domain` は外部 SDK 型・端末状態・プロセス状態へ依存してはならない。`domain` の外部 crate 利用可否は、対象仕様と実装責務に照らして判定し、`言語標準 library 以外に依存しない` という単独規則を機械適用してはならない。`port` は `domain` のみへ依存する。`adapter` は `port`、`domain`、`support` に依存できるが、`application` の use case 順序制御を持ってはならない。`support` は言語標準 library と外部技術 crate に依存できるが、他層の業務語彙へ依存してはならない。`tests` は対象層と test helper に依存する。

`application` で許容する外部クレート依存は `anyhow`（エラー文脈付与）に限定する。`zeroize::Zeroizing` は `support/protection` モジュール（およびその配下）以外で使用してはならない。`application` を含む他層へ `zeroize` を導入してはならない。`adapter` は `application` の flow decision を持たず、`port` は `adapter` 詳細や end-user 文言を持たない。

## ドキュメントコメント規則

各層で次を必須とする。この comment / doc comment 規則はリポジトリ共通のコメント規約の正本であり、層をまたいで適用する。

- **ヘッダコメント必須対象**: production code の非自明要素/境界要素（use-case entrypoint、core workflow の非自明な internal type/function、`port` trait の責任分界、`adapter` の翻訳境界、`support` の安全性境界）には、対象要素の直上に役割と責務境界を示すヘッダコメント（言語標準 doc comment を含む）を必須とする。
- **テストケースを含めて必須**: `#[test]` 関数、`#[cfg(test)] mod tests`、`tests/` 配下、`*_tests.rs`、`test_*.rs` を含む repository-authored Rust source/test source の各ファイルは、file-level comment または doc comment によるヘッダコメントを必須とする。

- 非自明な module、script、command entrypoint、use case、adapter、support utility、検証フロー定義ファイルには file-level comment または言語標準 doc comment を付け、役割を説明する。
- repository-authored explanatory comment は日本語で書く。周辺文脈が英語で固定されている場合、上流引用、外部形式要件の場合のみ英語を許可する。
- comment は恒久的な設計意図、不変条件、制約、自明でない運用上の文脈を記し、単なるコード言い換え、個人メモ、曖昧な TODO/FIXME を禁止する。
- comment が必要な場合はライフサイクル境界、外部契約、シグナル安全性要件、ワイヤ形式規則、セキュリティ特性、利用者操作制約のいずれかを具体名で記す。
- 関数、型、module の doc comment は主要契約を先頭文で述べ、条件、分岐、失敗時契約、caller responsibility は別文または別段落で続ける。
- 挙動変更時は近傍コメントを同パッチで更新し、誤解を生む旧コメントを残さない。
- `application` の public command flow と非自明な private helper は、主要契約を先頭文で述べ、その後に必要入力、停止条件、外部 interaction boundary を記す。
- `application` の use-case entrypoint（公開開始関数・主要 orchestration 関数）には doc comment を必須とし、先頭文の主要契約に続けて「なぜその順序制御が必要か」または「どの責務境界を保護するためか」を明記する。
- `application/use_case` を含む core workflow の非自明な internal type/function（private helper を含む）は、役割がコードから自明でない場合に doc comment を必須とし、`what` の言い換えではなく `why`・停止条件・caller responsibility のうち必要な項目を記す。
- `domain` の value、policy、wire-format 型は、何を表すかを先頭文で記し、その後に不変条件、version rule、error 条件を書く。
- `port` の trait は、要求する capability と caller/implementor の責任分界を明記する。
- `adapter` の module comment は、どの port をどの外部 API へ接続するか、どの制約を内部で吸収するかを記す。
- `support` の comment は、セキュリティ特性、ライフサイクル境界、シグナル安全性要件、所有権規則のいずれかを具体名で記す。
- 複数段落の doc comment は、先頭段落で通常系の主契約を示し、後続段落で非 TTY 動作、タイムアウト、所有権移譲、ゼロ化、ロック、出力安全性、再試行規則のような制約を記す。
- 上記必須対象で doc comment が欠落している場合、コメント整備は任意改善ではなく規約違反として扱う。レビュー担当は欠落を `要修正` 以上の差戻し条件として判定しなければならない。

## 言語別コードスタイル

この節はリポジトリ共通の言語別コードスタイル規約の正本である。

Rust:

- ワークスペースの edition は Rust 2024。
- 公開 CLI ロジックは `rust/dotfiles-cli`、保守コマンドは `rust/xtask`、共通補助は `rust/dotfiles-core` に置く。
- 責務を混在させない。dispatcher ファイルに端末 I/O、信号方針、暗号補助、ワイヤ形式、テストを過密に載せない。
- `anyhow` はリポジトリの Result 別名を通して使い、panic ではなく文脈付きで伝播する。
- 単純な変換・抽出は反復子と `collect` を優先する。
- `match collection.len()` ではなくスライスパターン、`is_empty`、ドメイン状態で分岐する。
- 不要な `mut` を導入しない。必要性を `git diff` で確認する。
- 閉じた集合を生文字列で渡さず列挙型/新規型で表現する。
- リポジトリ由来 Rust に `unsafe` を導入しない。
- テストを含め `unwrap` と `expect` を使わない。
- 警告を残さない。
- `clippy::too_many_arguments` の `allow` / `expect` / `cfg_attr` による抑止を repository-authored source/test source に置いてはならない。`Cargo.toml` の workspace lint は `too_many_arguments = "deny"` を維持し、関数側で回避しない。引数過多は、小さい関数への分割、cohesive capability への port 統合、または `run_*` 関数に閉じた runtime/dependency bundle で解消する。bundle は `Ports` と命名せず、挙動・責務配置を変えない単なる lint 逃げであれば不合格とする。

Nix:

- 利用者設定は Home Manager、ホスト/システム設定は nix-darwin に置く。
- 明示的な破壊的移行依頼がない限り公開 flake API を維持する。
- 再利用モジュールに実ユーザー名、実ホスト名、マシン固有パスを埋め込まない。
- Nix 整形は `cargo xtask check static` が使う flake formatter に従う。

Shell/zsh:

- `scripts/bootstrap.sh` は導入クリティカルとして扱い、可搬に保ち `bash -n` で構文検証できる状態を維持する。
- zsh 挙動は `tests/zsh` の前提（TAB、fzf-tab、autosuggestions、syntax highlighting、PATH 除外）と整合させる。
- アプリ管理の shell 注入や Docker 認証など、利用者ローカル可変状態はリポジトリ外に置く。

Lua/Neovim:

- 設定の主領域は `config/nvim/lua/omy/` とし、`configs` / `mappings` / `autocmds` の現行構造を固定の前提として維持する。
- 現行の構成・アーキテクチャ（現行のコード構造そのもの）は固定の前提とし、現行構造を別構造へ作り替える大幅な再編・ゼロベース再編を最適化目標にしてはならない。新規追加・是正は現行構造の内側に収める範囲で行い、既存コードを優先的に流用する。

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

ディレクトリ名と層が一致しないファイルは配置違反とみなす。配置違反の解消は、ファイルを正しいディレクトリへ移動することで行う。個別機能の違反一覧は、対象の GitHub issue、PR、明示タスク、または領域仕様文書で管理する。

## エージェント運用とレビューの参照先

secret-recovery 固有の役割境界、実装単位、完了条件は [implementation-guidelines.md](../secret-recovery/implementation-guidelines.md) を単一正本として参照する。この文書に secret-recovery 固有の運用を定義しない。

hexagonal review で確認するのは、層責務、依存方向、公開面、禁止成果物、comment 規則、正本参照整合である。secret-recovery のレビュー担当と記録義務の扱いは [implementation-guidelines.md](../secret-recovery/implementation-guidelines.md) とその参照文書に従う。
