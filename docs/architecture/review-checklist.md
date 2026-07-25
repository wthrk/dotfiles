# レビュー観点チェックリスト（構造）

この文書は、ディレクトリパターン別の構造レビュー観点の正本である。層ごとの責務・禁止事項・依存方向・公開範囲の定義は [hexagonal-implementation-rules.md](hexagonal-implementation-rules.md) を正本とし、この文書はそこから導かれたチェック項目を定義する。

## チェックの進め方

1. レビュー対象ファイルのディレクトリ名から所属層を確定する（[ディレクトリと層の対応規則](hexagonal-implementation-rules.md#ディレクトリと層の対応規則)）。
2. 主要な処理を [hexagonal-implementation-rules.md の責任分離の判断原則](hexagonal-implementation-rules.md#責任分離の判断原則) に従い、`domain rule` / `application orchestration` / `port contract` / `adapter forwarding` / `support technical backend` / `presentation interaction` / `composition wiring` のいずれかへ分類する。
3. 所属層に対応するセクションのチェック項目を適用する。
4. ディレクトリ名と層が一致しないファイルは配置違反として記録する。

## 着手前設計照合

このチェックリストは事後レビューだけでなく、実装担当が編集前に予定設計へ適用する。対象差分に必要な reviewer checklist の全観点について、少なくとも次の反例を構成し、予定するコード境界、テスト観測点、仕様または一次資料の根拠によって否定できることを確認する。

| 反例の軸 | 否定に必要な確認 |
|---|---|
| 成功 | 正本が要求する最終状態へ到達せず、途中結果だけを成功として返す caller がないか |
| 失敗 | 外部エラー、無効入力、partial state を成功、空値、既定値、別の意味へ変換して継続する経路がないか |
| cleanup | 成功、通常失敗、cleanup 自体の失敗、unwind の各経路で secret、terminal、session、temporary resource が残らないか |
| 全 caller | 同じ capability を使う setup、enroll、provision、put、clear、verify、restore 等の一部だけが別の前提・別経路・弱い停止条件を持たないか |
| 不可逆境界 | PIN retry、鍵生成、overwrite、delete、remote mutation 等の前に、必要な read-only validation と利用者入力検証が完了しているか |
| 外部状態 | fresh、initialized、partial、legacy、corrupt、missing、duplicate、mismatched、device/reader の不在・追加・swap、同時実行、および外部 resource の0件/1件/複数件を区別し、未定義状態を推測で補完しないか |
| 一次資料根拠 | SDK/API の全体 flow、呼出 API、成功・失敗状態、cleanup の判断を、適用 version の公式一次資料へ対応付けられるか |

各反例には、最上位目的と end-to-end user flow のどの遷移を守るか、予定する責務所有層と境界、対象となる全 caller、反例を直接観測する acceptance test または検証、根拠となる正本の位置を対応付ける。外部 SDK / crate が関係する場合は固定した依存 version、公式の全体 flow、全 error surface、一次資料の URL と API symbol、仕様節、または versioned source location も対応付ける。「テストが通るはず」「既存実装と同じ」「レビューで見つける」は反例の否定にならない。

実装後は同じ反例を実際の file:line、test 名、直接観測へ再対応付ける。S2 の reviewer はこの対応を独立に再確認するが、事後レビューを着手前の設計探索の代用にしてはならない。着手前照合の記録は実装担当の応答または引継ぎに残し、repo 内へ current-cycle artifact として保存しない。

この照合は [implementation-execution.md の全経路閉鎖不変条件](../task-governance/implementation-execution.md#全経路閉鎖不変条件) の受入れ証跡である。全 flow / 境界の coverage と counterexample のどちらかが欠ける場合、局所的に正しい変更、変更を「一括」と呼ぶこと、または test の一部通過をもって設計照合の合格としてはならない。

### 責務基準の判定原則（形式より責務）

このチェックリストの全項目は、形式（ファイル名パターン・命名・公開面の有無・port trait を実装しているか・`#[cfg(test)]` か `#[cfg(feature)]` で gate されているか）ではなく、コードの**責務**が層の責務に一致するかで判定する。形式が正しくても責務が層に属さなければ `判定: 不合格` とする。これは [hexagonal-implementation-rules.md の哲学](hexagonal-implementation-rules.md#哲学)（「visibility はシンボルの見え方を制御するが、そのコードが属すべき層の責務を変えない」）から導かれる強制原則である。

`adapter` から `support` へ移した、ファイルを分けた、private helper を消した、という事実だけで合格にしてはならない。レビューは「その処理がなぜその規定済み境界の責務なのか」を分類結果とコード根拠で確認する。分類できない処理、または分類結果と配置層が一致しない処理は、配置名や公開範囲が整っていても `判定: 不合格` とする。

変数名、型名、コメント、module 名、ファイル配置だけを変えて、挙動・責務配置・依存方向・公開面が変わっていない修正を合格にしてはならない。レビューは「どの処理がどの層へ移ったか」ではなく、「どの違反責務が消えたか」「同じ判断が別名で残っていないか」を差分で確認する。名前だけの変更、`allow` / `expect` 追加、dependency bundle への機械的な詰め替えは、指摘された設計違反の解消根拠にならない。

repository-authored Rust source/test source に `clippy::too_many_arguments` の `allow` / `expect` / `cfg_attr` が残っていれば不合格とする。`Cargo.toml` の workspace lint `too_many_arguments = "deny"` は維持し、関数側の lint suppression で回避しない。解消方法は、小さい関数への分割、cohesive capability への port 境界見直し、または `run_*` 関数に閉じた runtime/dependency bundle であり、bundle 名に `Ports` を使ってはならない。bundle が同期 port の concrete generic を羅列するだけなら、trait object 参照または use case が実際に要求する runtime/environment 境界へ寄せられないかを確認し、async trait など object-safety 制約のない依存まで型パラメータ化していれば不合格とする。


## adapters/ 配下

### なぜこの制約が存在するか

`adapter` は port trait と、その capability の concrete receiver（technical backend は support、interaction 表現は presentation）の接続だけを担う。adapter が forwarding 以外の型・関数・変換・分岐を持った時点で、実装責務が別の層へ分散し、receiver 差し替え時の変更境界が壊れる。`pub(crate)` はクレート内に見えるという意味であり、「外部公開ではない」という意味ではない。`adapters/` 配下の全関数・型・定数について、port trait implementation の receiver とその forwarding method 以外は `pub` も `pub(crate)` も `pub(super)` も禁止である。「adapter 内部で使いやすいから」「SDK 翻訳だから」は公開または実装の正当化にならない。

- **依存方向**: `port`、`domain`、`support`、および interaction 表現を所有する `presentation` receiver にのみ依存していること。`presentation` 依存は receiver operation への単一 forwarding に限定し、`application` の use case 型・flow 関数を import していないこと。
- **責務**: support-owned technical receiver または presentation-owned interaction receiver への port trait forwarding に限定されていること。SDK bridge、外部 API 型変換、terminal raw-byte/device/filesystem/codec 実装は support、prompt/report/schema は presentation が所有する。use case の順序制御・domain policy の決定を含まないこと。
- **公開面（絶対規則）**: `pub`・`pub(crate)`・`pub(super)` で外部に公開できるのは、port trait を実装する型（struct/enum）とそのメソッド実装のみ。stdin 読み取り関数・プロンプト関数・JSON デコード関数・terminal I/O 関数・定数は port trait 実装の一部でない限り private にとどめること。

### レビュー時の問い

- 各 trait method は責務を所有する receiver operation への単一の forwarding expression だけか。local 変数、`match` / `if`、`map` / `collect`、parse/serialize、`format!`、error context、SDK/process/device/filesystem/codec 呼び出しを adapter 自身が持っていれば不合格とする。
- forwarding-only adapter を「実質的な翻訳がない」「単なる委譲」として不合格にしていないか。ここでの単一委譲は required architecture であり、委譲先 support backend の技術実装と domain/application の業務責務を別に判定する。
- この adapter は domain object のビジネスロジック（manifest/blob 整合判定、nonce/AAD 規則、`SecretName::additional_data` の意味適用、鍵生成可否判断）を直接実行していないか。実行している場合は既存規定上の該当境界に置くべきではないか。
- adapter の処理を「低水準だから adapter」と判定していないか。SDK/PIV 呼び出しや codec 呼び出しであっても、domain object の関連づけ、保存可能条件、AAD/nonce/manifest/blob の業務意味、上書き可否、値制約を決めているなら adapter から除去すること。
- adapter の private helper が domain object を分解して業務規則を再構築していないか。helper が private であることは business logic 混入の免除理由にならない。
- adapter に helper、state、inherent impl、adapter-local stub schema/fixture/observation がないか。固定 secret 名、project 名、保存モデル、0件/複数件の failure 化、利用者確認、上書き可否、外部確認 plan、command 固有手順を adapter に置いている場合、private でも不合格にすること。
- `pub(crate)` または `pub(super)` で公開されているシンボルについて：「これを公開しなければならない理由はポートの契約を果たすためか」と問え。「内部で使いやすいから」「呼び出し元が必要としているから」は公開の正当化にならない。後者はアーキテクチャ違反を呼び出し元が要求している状態であり、呼び出し元側の違反を意味する。
- このコードが存在する理由を一文で言えるか。「port 契約Yを責務所有 receiver Xへ委譲するため」と言えるか。それ以外の理由が混在しているなら、その部分は adapter に属さない。
- このファイルを削除して別の技術に差し替えたとき、application/ や domain/ のコードを変更する必要が生じるか。生じるなら、adapter がその依存を外に漏らしている。
- このファイル内の private な関数・ロジックを含む全コードについて：port forwarding だけを行っているか。use case の順序制御・domain policy の決定・SDK 型変換・ビジネスロジックの判断が private コードとして潜り込んでいないか。
- process-generic な terminal/stdin/stdout helper、SDK/process/device/filesystem/codec 実装を adapter private へ直詰めしていないか。TTY 制御、blocking read、echo/raw mode、interrupt-aware read などの低レベル実装支援は support が所有する。
- このファイルが独立して存在することで「特定の外部技術とポートの間の翻訳」として何が独立・差し替え可能になるか。答えられないなら分割の意義がない。
- このファイルは複数の外部技術を1ファイルに混在させていないか。異なる外部技術の翻訳が混在しているなら分割されていなければならない。逆に単一の外部技術の翻訳が不必要に複数ファイルに断片化されていないか。
- この分割は「特定の外部技術とポートの間に独立した責務の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- 再エクスポート集約ファイルではないか。trait implementation が support backend への forwarding だけであることは required であり、その理由だけで不合格にしてはならない。

## application/ 配下

### レビュー時の問い

- このコードはユースケースの「順序」と「分岐」を知っているだけか。具体的なデバイス、I/O、パーサーの実装や意味づけを知っていないか。
- use-case entrypoint と非自明な internal type/function（private helper 含む）に、主要契約と責務境界を示す doc comment があるか。`what` の言い換えだけで `why` が欠落していないか。
- `boundary.some_method()` の呼び出しを見たとき：「この呼び出しはポートの契約を通じているか、それともアダプターの具体型を直接知っているか」を確認せよ。
- `application/` 配下にアダプター実装が紛れ込んでいないか。ファイル名ではなく責務で判断せよ。
- このコードが「何をするか」ではなく「何の順序で、どこで分岐して呼ぶか」だけを知っているか。具体的な入出力の形・エラーの詳細・デバイスの挙動・summary の意味づけを知っていたら application に属さない。
- application に object 操作が反復している場合、それは helper 化すべき重複か、それとも domain object として表現すべき業務規則かを判定しているか。オブジェクト同士の関連・変換・整合判定・値制約は、順序制御ではなく domain 責務である。
- application が manifest/blob/nonce/AAD/上書き可否/値検証の意味を直接定義していないか。application は domain 規則をどの順序で適用するかだけを持つ。
- このuse caseを別のインターフェース（CLI→Web API）に移植したとき、このファイルの変更は最小限で済むか。済まないなら、インターフェース固有の知識が混入している。
- このファイル内の private な関数・ロジックを含む全コードについて：concrete I/O（`println!`・stdin 読み取り・端末文言）、アダプターの具体型参照、外部 SDK 型が含まれていないか。private であることはこれらの禁止事項の免除理由にならない。
- 乱数生成（key material、nonce、challenge、seed 等）を `application` が直接呼んでいないか。必ず port 経由になっているか。
- private helper が増え続けている場合、port capability が粗く application が技術詳細の調停を肩代わりしていないか。helper 追加で対応せず、port 契約の再分割要求を返せるか。
- runtime/dependency bundle がある場合、それは `run_*` 関数に閉じた実行時依存束か。`Ports` と命名されていないか。bundle 化によって引数数だけを隠し、同じ微細 port 群を application が調停し続けていないか。常に同時に使われる複数 port は cohesive capability として見直す余地がないか。同期 port まで全部 concrete generic として詰めていないか。async trait など Rust の object-safety 制約で generic が必要な依存だけに型パラメータが限定されているか。
- このファイルが独立した use case として意味を持つか。「このファイルが担う use case とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- use case を細分化した断片になっていないか。逆に複数の use case を1ファイルに詰め込んでいないか。1ファイル1 use case の対応が崩れているなら分割を見直せ。
- 各 sibling file が `run_*` 関数を 1 つだけ持ち、1 use case = 1 function を守っているか。`application/use_case.rs` を再導入して use case 実装の置き場にしていないか。ここでの `run_*` 単一関数制約は production use case entrypoint（`run_*.rs` 本体）に適用し、`#[cfg(test)] mod tests` および `*_tests.rs` は対象外とする。
- `application/use_case/` ディレクトリ、directory `mod.rs`、`#[path = \"...\"]` を使った配線を導入していないか。
- ある use case が別の use case を呼び出していないか。use case-to-use case call を共通化手段として使っていないか。
- use case 層で logic commonization をしていないか。共通 helper を増やしている場合、その責務は本当に application 層か。重複を許容してでも規定済みの責務境界に置くべきものではないか。
- `application` が use case 順序制御に専念しているか。command dispatch や技術 helper（保護メモリ変換・wire/crypto/persistence 実装詳細）を保持していないか。
- application の feature root が、責務を持たない façade を作っていないか。物理配置は [Feature Boundary Design](feature-boundary-design.md#物理アーキテクチャ) に従い、flat `application/` の sibling 配置を前提にしないこと。
- この分割は「独立した use case の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- doc comment 欠落を「実装を読めば分かる」で見逃していないか。core workflow の責務境界説明がない場合はレビュー不合格にすべきか。

- **依存方向**: `domain` と `port` にのみ依存していること。`support` を含む他層へ依存していないこと。`adapter` の具体型を import していないこと。
- **型規則**: use case 独自の struct / enum / type alias / summary 型を定義していないこと。use case が扱う型は `domain` 層で定義された型のみに限定されていること。
- **関数構成**: feature 内 application の use case entrypoint は単一の orchestration 責務を持たせる。物理的な sibling 数、`run_*` 命名、flat `application/`、又は疑似レイヤーの禁止を責務判定の代替にしてはならない。feature 間接続は public port contract だけを通し、`#[cfg(test)] mod tests` と test target の配置は [Feature Boundary Design](feature-boundary-design.md#静的-boundary-linter-仕様) の exact exception に従う。
- **責務**: use case の順序制御・分岐・停止条件に限定されていること。意味・方針・summary semantics の決定、`println!`、stdin 読み取り、concrete device handle 操作を含まないこと。
- **依存取得**: 乱数生成は port 経由のみとし、`application` が直接 external crate や標準 API から乱数を取得していないこと。
- **外部 crate 制約**: `application` で使う外部 crate は `anyhow` のみに限定されていること。`zeroize::Zeroizing` は `support/protection` モジュール（およびその配下）にのみ存在し、他層で使用されていないこと。
- **共通化禁止**: use case-to-use case call と、use case 層での logic commonization を禁止する。共通 helper の抽出を完了条件にしてはならず、重複は許容される。
- **配置**: adapter 実装ファイルを `application/` 配下に置かないこと。
- **ドキュメントコメント（必須）**: use-case entrypoint と非自明な internal type/function に doc comment があり、主要契約の先頭文と `why`/責務境界（順序制御理由・停止条件・caller responsibility のいずれか）を示していること。欠落時は `判定: 不合格`。

## domain/ 配下

### レビュー時の問い

- このコードは「ビジネスルール」だけを知っているか。技術選定（どのSDK、どのプロトコル、どのシリアライズ形式）を知っていないか。
- domain を単なる data shape / validation 置き場として扱っていないか。ビジネスロジック、業務判断、業務上の失敗条件を domain の第一責務として表現しているか。
- domain がオブジェクト単体の validation だけに矮小化されていないか。オブジェクト同士の関連づけ、保存 layout の対応、変換の正当性、状態遷移、業務上の失敗条件も domain の責務として表現されているか。
- application や adapter に散らばった object 操作を、既存規定上の domain concept として名前付けできないか。名前付けできるなら、その規則は規定済みの domain 境界に置くべきではないか。
- ここに定義された型について：「この型はインフラを差し替えたとき変更を必要とするか」と問え。変更が必要なら domain に置くべきではない。
- このコードはインフラが存在しない環境（ファイルもネットワークも外部SDKもない）でも意味を持つか。意味を持たないなら domain に属さない。
- この型・関数の名前はビジネスの語彙か、技術の語彙か。技術の語彙が現れていたら domain に属さない可能性が高い。
- このファイル内の private な関数・ロジックを含む全コードについて：外部 SDK 型・端末状態・プロセス状態への依存が含まれていないか。private であることはこれらの禁止事項の免除理由にならない。
- この型・関数が独立したドメイン概念として意味を持つか。「このファイルが表すドメイン概念とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 技術的な便宜（「他の場所でも使いたいから」「長くなったから」）のために分割されていないか。ドメイン概念の境界ではなく実装都合による分割はドメインの設計意図を曇らせる。
- この分割は「独立したドメイン概念の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 外部 SDK 型・端末状態・プロセス状態へ依存しないこと。`domain` の外部 crate 利用可否は対象仕様と実装責務に従って判定し、`言語標準ライブラリ以外に依存しない` という単独規則を機械適用しないこと。
- **責務**: value/newtype・不変条件・状態遷移・wire format・domain error に限定されていること。
- **禁止成果物**: port contract（trait）・presentation DTO・`std::io::Write` 等の I/O 型を含まないこと。

## ports/ 配下

### レビュー時の問い

- この trait またはこの型は「ドメインが何を必要とするかの宣言」だけになっているか。「どのように実装するか」の詳細や意味づけ済みの実装選択を含んでいないか。
- ここに定義された struct/enum について：「これは技術の詳細（SDK型・パーサー・プロンプト文言・raw bytes）か、それともドメインの意図の表明か」と問え。技術詳細であれば adapter に属する。
- port が肥大化しているとき：「これはドメインの要求か、それともアダプターの実装都合か」を問え。
- port が複数の責務をまとめて隠す fat port になっていないか。device I/O、key operation、storage codec、crypto、manifest 検証、上書き判定を 1 trait / 1 method に押し込んでいないか。
- port が細分化されすぎていないか。1 method だけの trait が連続して増え、同じ use case が常に複数 trait をセットで要求している場合、それは外部機能単位の cohesive capability ではなく application 都合の分割ではないか。過分割を dependency bundle に詰め替えただけで解消扱いにしていないか。
- storage backend が暗号化された永続化を内包する場合、port は暗号化・復号・sealed blob 形式ではなく datastore capability を公開しているか。暗号処理を port に露出していないことを理由に不合格にしてはならないが、port method が setup 判定、必須 secret の過不足判定、一意解決、0件/複数件 failure、外部確認 plan までまとめて隠すなら fat port として不合格にすること。
- port の method が「何を要求するか」ではなく「手順を丸ごと済ませること」を表していないか。手順や規則は application/domain、外部 API 翻訳は adapter に分解すること。
- この trait を読んだとき「システムが外界に何を要求しているか」が分かるか。「どうやって実現するか」の痕跡が残っていたら ports に属さない。
- この trait の各メソッドはドメインの意図を表しているか。「ファイルを読む」「HTTPリクエストを送る」という技術操作ではなく「秘密情報を取得する」「認証を検証する」というドメイン操作になっているか。
- このファイル内の private な型・関数・ロジックを含む全コードについて：「ドメインが何を必要とするかの宣言のみ」という責務を満たしているか。parser・DTO・prompt・利用者向け文言が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この trait が独立した capability として意味を持つか。「このファイルが宣言する capability とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 1つの外部依存の契約を不必要に複数の trait に分割していないか。逆に無関係な複数の capability を1ファイルに混在させていないか。capability の境界と trait の境界が一致しているか確認せよ。
- trait 境界は「大まかな外部機能」単位になっているか。DB 接続、ファイルアクセス、CUI など変更理由が異なる外部機能を 1 trait に混在させていないか。
- device 取得・secret 入力・secret 出力・report 出力のように変更理由が異なる capability を 1 trait に押し込んでいないか。use case が必要とする capability だけを境界として要求できる構造になっているか。
- `Summary` / `Report` の具体 DTO が ports に定義されていないか。ports に置けるのは report 出力 capability の宣言までであり、use case が扱う outcome の意味は domain、固定 output schema と serialization は presentation に置くこと。
- この分割は「独立した capability の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: `domain` にのみ依存していること。ただし secret-recovery の secret carrier として `ProtectedSecret` を port 境界で受け渡すことは許可し、平文取り出し・backend 抽出・汎用 writer・汎用 plaintext consumer API を port へ持ち込むことは引き続き禁止する。
- **責務**: capability contract を表す trait・request/response の最小境界型に限定されていること。
- **trait 粒度**: 1 trait = 1 外部機能を基本とし、外部機能単位 trait と adapter module 分割が対応していること。
- **禁止成果物**: DTO・parser・prompt・利用者向け文言を含まないこと。

## support/ 配下

### レビュー時の問い

- このコードは共通技術 primitive か、secret 保護境界を閉じる backend 操作か。どちらでもない機能固有の名前（特定のコマンド名・ロール名）が現れていたら support に置くべきではない。
- support を adapter/domain/application 違反の逃げ場として使っていないか。adapter は forwarding-only なので、SDK/process/device/filesystem/codec の実行を support backend へ置くこと自体は required である。一方、薄い port を保つために domain/application の業務判断を support へ押し込んでいないか。移動前の処理を `domain rule` / `application orchestration` / `port contract` / `adapter forwarding` / `support technical backend` に分類し、`support technical backend` 以外なら support 配置を不合格にしているか。
- storage backend 内部の暗号化・復号・sealed blob 操作が support にある場合、それ自体を不合格理由にしていないか。不合格にする場合は、その処理が setup 済み判定、必須 secret の決定、固定 key/name/role の意味づけ、一意解決、0件/複数件 failure 化、取得対象の過不足判定、外部確認 plan などの業務判断を含むことを具体的に示すこと。
- support に product-specific storage mechanism を置いていないか。暗号や binary codec を使っていても、enroll/verify/secret storage role の語彙や保存 format の業務意味を持つなら support ではない。secret 保護境界の dedicated backend 操作として YubiKey/Bitwarden などの外部処理名が現れること自体は不合格理由にしない。
- support backend が port/domain 境界型を technical I/O の引数・返値として受け渡すことだけを理由に不合格にしていないか。adapter が forwarding-only のため、この受け渡しは許可される。判定すべきなのは、support がその値を根拠に対象同一性、必須性、一意性、成功・停止条件、use case 順序を決めていないこと、および SDK 型を domain/application へ漏らしていないことである。
- support 内の BWS 処理が、固定 secret key/name の意味づけ、secret ID の一意解決の業務規則、0件/複数件の domain failure 化、取得対象の過不足判定、`verify-yubikey --check bws` の外部検証 plan を決めていないか。これらは既存規定上の該当境界に属するものとして検出し、support にあれば不合格にすること。
- terminal I/O・prompt 周辺のコードがここにある場合は、その責務が process-generic な補助か、feature-specific な interaction 方針かを区別せよ。TTY open、echo/raw mode 制御、blocking read、interrupt-aware read、byte stream としての stdin/stdout 処理のような前者は support に置ける。specific command の prompt 文言、device 選択判断、use case 手順のような後者は support に属さない。「共通部品だから support」という判断だけで正当化してはならない。
- このコードの名前から command 手順や domain policy を推測できるか。推測できるなら support に属さない。secret 保護境界の専用 backend 操作から外部 SDK/device 名を推測できることだけを不合格理由にしない。
- このコードを別のまったく異なるプロダクトにそのままコピーして使えるか。使えない場合、それは secret 保護境界の dedicated backend 操作として必要な product/service-specific 処理か、単なる配置違反かを判定する。
- このファイル内の private な関数・ロジックを含む全コードについて：共通技術部品または secret 保護境界の dedicated backend 操作という責務を満たしているか。feature-specific な prompt 文言・device 選択判断・use case 手順が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この部品が汎用的な技術部品として独立した意味を持つか。「このファイルが提供する技術部品とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 特定機能に依存した「共通っぽいもの」になっていないか。support にあるが特定の機能でしか使われない部品は、その機能への依存が混入していないか確認せよ。
- この分割は「独立した技術部品の責務境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 言語標準ライブラリと外部技術 crate にのみ依存していること。他層の業務語彙へ依存しないこと。
- **責務**: 共通技術部品（保護メモリ・暗号プリミティブ・byte utility・process-generic な標準入出力補助）または secret 保護境界の dedicated backend 操作に限定されていること。
- **許可成果物**: backend 実装依存の技術補助、SDK 呼び出しの安全な補助、storage backend 内部の暗号化・復号・sealed blob 操作、protection / zeroize / core dump 保護などの技術境界、業務判断を含まない変換を含めてよい。
- **禁止成果物**: feature-specific な terminal I/O 方針、prompt 文言、device 選択判断、command 名、role 名、固定 secret key/name/role に基づく対象同一性・一意性・0件/複数件の業務判断、外部確認 plan、汎用 plaintext consumer API、平文 buffer を返す public API を含まないこと。
- **レビュー禁止**: `support` に process/terminal I/O、concrete backend、port/domain 境界型の技術的受け渡しが存在することだけを理由に `判定: 不合格` としてはならない。不合格にする場合は、業務判断、feature-specific な interaction 方針、対象同一性・一意性・成功/停止条件の決定、domain/application からの直接利用、または support ではなく別層に属する責務を具体的に示すこと。

## secret-recovery 判定クイックガイド（application / ports / adapters / support）

各層レビューで次を追加確認する。

- application:
  command dispatch、input modality（Prompt/Stdin/StdinJson 等）、report DTO 変換、protected buffer 化、crypto helper 呼び出し詳細、device selection 実装を含めていないか。
- ports:
  modality enum や stdin-json の手段表現を契約へ露出していないか。`read_secret_from_prompt` のように capability 名で表現されているか。
- adapters:
  `application::...` 型へ直接依存せず、port trait の各 method を責務所有 receiver へ単一 forwarding しているか。device/SDK/process/filesystem/codec 実装を support、prompt/report formatting を presentation が担い、adapter 自身がどちらも担っていないか。
- support:
  protected buffer / zeroization / crypto helper / process-generic な標準入出力補助、または secret 保護境界の dedicated backend 操作に限定されているか。device 選択判断、use case 手順、固定 BWS secret の一意解決、0件/複数件の扱い、外部確認 plan、汎用 plaintext consumer API を持っていないか。
- BWS / YubiKey / GPG backup 保存モデル:
  [Bitwarden Secrets Manager 復旧設計](../secret-recovery/bitwarden-personal-vault-design.md) の保存先表と [secret-recovery spec](../secret-recovery/secret-recovery-spec.md) の `Secret の置き場所` に照らし、どこに・どういう名前で・何を保存するかが実装と一致しているか。BWS project `dotfiles-secret-recovery` の secret `gpg-secret-key-backup` / `password-store-remote`、YubiKey storage `bitwarden-client-secret`、Bitwarden Password Manager vault の役割を混同していないか。organization / machine account / service account / UI 画面名を実装前提または保存モデル前提として持ち込んでいないか。UI 操作案内は runbook の手動操作に限定し、実装・設計の正本は保存先/名前/値/読書き責務で照合すること。

上記は形式チェックではなく責務判定である。`pub` 範囲や feature gate の有無は免除理由にならない。

## entrypoint/ 配下

- **依存方向**: `application`、`domain`、`port` に依存できること。debug scope に限り presentation-owned の抽象 control/token contract を参照できるが、`composition`、presentation concrete、`support`、`adapter` の具体型を import しないこと。
- **責務**: command 定義・debug を含む引数値変換・entrypoint-owned invocation DTO・use case 起動・終了 code 変換に限定されていること。唯一の入力境界 `entrypoint::start(invocation)` が command 値と port contract reference だけを受け、選択した application input / `run_*` を一回起動することを確認する。debug は token begin → application 一回 → result を token finish の境界だけを扱い、diagnostic decorator/formatting/phase 判断、domain rule、順序制御、device 制御を含まないこと。
- **配線契約**: composition root が concrete receiver から entrypoint-owned invocation DTO を構築して渡すこと。canonical root route は `crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` であり、root bootstrap だけが DTO を構築して feature entrypoint を直接呼ぶこと。entrypoint が `SecretsRuntime` 等の composition concrete を受ける又はその field/composition module を直接参照する、consumer/wrapper が bootstrap 又は feature entrypoint start を import/call/re-export する場合は不合格とする。`pub(crate)` はこの root-only 制約の根拠にならない。

## presentation/ 配下

- **依存方向**: `domain` と `port`、および composition から注入される support の command-neutral な関数型・保護済み carrier に限ること。`application`、`entrypoint`、`composition`、`adapter`、support concrete receiver を参照しないこと。
- **責務**: command 固有の prompt/確認文言、TTY/pipe 選択、input schema decode、output schema/formatting、既存 port operation に表示観測だけを加える decorator、表示 scope の finalization に限定すること。
- **decorator 契約**: inner operation を一回だけ呼び、引数・返値・error chain・operation count を変えないこと。device mutation、use case 順序、retry/fallback、SDK error や raw status の分類・復元を行わないこと。
- **diagnostic 安全性**: PIN/token/secret、長さ/hash/derived bytes、raw APDU/status、任意 error text を carrier と schema に含めず、固定 phase と success/opaque failure だけを stderr sink へ渡すこと。
- **禁止成果物**: use case 呼び出し、command dispatch、concrete backend 所有、support global hook の直接操作、port capability の再定義を含まないこと。

## composition/ 配下

- **依存方向**: concrete `support` / `presentation` / `adapter` を外側から生成し、entrypoint-owned invocation boundary へ注入すること。`crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` を唯一の root entry edge とし、root bootstrap だけが entrypoint start を直接呼ぶこと。他層が composition を import していないこと。
- **責務**: concrete receiver の生成、既存 port trait 実装の組合せ、command-neutral 関数・sink・clock の注入に限定すること。root bootstrap は DTO 構築と entrypoint `start` の起動だけを所有する。feature-local composition factory は存在しない。
- **レビュー時の問い**: composition が `run_*` の順序、domain policy、input parse、prompt/formatting、SDK/device/process 呼出、error 分類、retry/fallback を持っていないか。配線の都合で entrypoint が composition concrete を受け取る、または presentation が support concrete receiver を import する逆向き依存を作っていないか。

## tests/ 配下・`*_tests.rs`・`test_*.rs`

### レビュー時の問い

- production 層（`entrypoint/`・`composition/`・`presentation/`・`adapters/`・`application/`・`domain/`・`ports/`・`support/`）の各ファイルに、実依存を肩代わりする責務を持つ型（Fake/Stub/Mock）が定義されていないか。その型は、テストのためだけに存在し本番経路では使われないか。そうであれば配置違反である。
- support 配下の `#[cfg(feature = "secrets-internal-test-stub")]` backend stub と、それへ forwarding する adapter trait implementation について、[internal backend stub の配置](hexagonal-implementation-rules.md#internal-backend-stub-の配置) の条件をすべて満たしているか。満たす場合は production source tree への test double 混入として扱ってはならない。
- test 側が backend state schema・状態遷移 helper・write event helper・bincode schema・backend 内部保存形式を保持していないか。保持している場合は不合格とする。
- BWS port stub と YubiKey port stub が独立しているか。共通巨大 state や共有 state file で結合している場合は不合格とする。
- integration test の検証観点が「初期 datastore 定義の投入」と「CLI 実行後の最終 datastore 観測」に限定されているか。stub 内部遷移観測を assertion している場合は不合格とする。
- production 層に置かれた `#[cfg(test)]`/`#[cfg(feature = "...")]` ブロックの中身は、(a) その module 自身の private 関数を検証する `#[test]` 関数か、(b) 実依存を肩代わりする double の**定義**か。(b) であれば配置違反である。(a) は許可される（後述）。
- ある double の責務は「テスト時に実依存を substitute すること」か。そうであれば、それが port trait を実装していても・feature gate されていても、production 層ではなく `tests/` 層または専用 test-support crate に属する。

### 許可される in, 禁止される out（責務で区別する）

- **許可**: production 層の `src/` ファイル内に置かれた通常の inline unit test（`#[cfg(test)] mod tests { #[test] fn ... }`）。これはその module 自身の private 関数を検証する標準的かつ idiomatic な Rust であり、削除を要求してはならない。inline unit test を一律禁止すると、本番関数をテストのためだけに `pub` 化する圧力が生じ、公開面最小化の哲学に反する。`#[test]` 関数の存在のみを理由に `判定: 不合格` としてはならない。
- **許可**: support 配下の internal backend stub は、[hexagonal-implementation-rules.md の internal backend stub の配置](hexagonal-implementation-rules.md#internal-backend-stub-の配置) を満たす場合に限り production source tree への test double 混入として扱わない。adapter 側は同じ backend への forwarding trait implementation だけであることを確認する。このチェックリストでは、stub が正本節の許可対象に該当するか、stub 周辺のテスト支援責務と backend 実装責務が混在していないか、test 側が初期/最終 datastore 観測のみを扱っているか、BWS/YubiKey port stub が独立しているかを確認する。
- **許可**: application 層の use case orchestration test を internal test stub feature から切り離し、各 `run_*.rs` の `#[cfg(test)] mod tests` 内で port trait 契約を駆動すること。port double は port trait 側から生成した test-only `mockall` mock を各テストで直接組み立てる。app 層 inline/unit test から `tests/` 配下の module、support、fixture、file を `#[path]`、`include!`、または test support module 経由で参照することは不許可である。`rust/dotfiles-secrets/src/application/app_test_support.rs` のような app 層共有 test support file、event recorder、巨大な状態管理 harness、port trait と別に動くテスト専用実装、既存 trait method を `mock!` macro へ手で書き写す二重管理は不許可である。
- **許可**: `ProtectedSecret` の secret 生値アクセスを `#[cfg(test)]` または `#[test]` に閉じた最小関数として用意し、unit test が値を観測すること。これは test-only の最小観測口であり、domain/application への取り出し API 追加、production 経路での plaintext 取り出し、`String` 変換公開、汎用 plaintext consumer API は引き続き不許可である。
- **不許可**: production 層に置かれた型の責務が「テスト専用の実依存肩代わり」であり、かつ当該層の責務と一致しない場合。`#[cfg(test)]` / `#[cfg(feature = "...")]` / port trait 実装の有無だけで機械的に判定してはならない。責務の不一致を根拠として不合格にする。

判定の分かれ目は「形式（`#[cfg(test)]` か `#[cfg(feature)]` か）」ではなく「責務（その module 自身の検証か、実依存の肩代わり定義か）」である。
