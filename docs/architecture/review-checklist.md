# レビュー観点チェックリスト（構造）

この文書は、ディレクトリパターン別の構造レビュー観点の正本である。層ごとの責務・禁止事項・依存方向・公開範囲の定義は [hexagonal-implementation-rules.md](hexagonal-implementation-rules.md) を正本とし、この文書はそこから導かれたチェック項目を定義する。

## チェックの進め方

1. レビュー対象ファイルのディレクトリ名から所属層を確定する（[ディレクトリと層の対応規則](hexagonal-implementation-rules.md#ディレクトリと層の対応規則)）。
2. 所属層に対応するセクションのチェック項目を適用する。
3. ディレクトリ名と層が一致しないファイルは配置違反として記録する。

### 責務基準の判定原則（形式より責務）

このチェックリストの全項目は、形式（ファイル名パターン・命名・公開面の有無・port trait を実装しているか・`#[cfg(test)]` か `#[cfg(feature)]` で gate されているか）ではなく、コードの**責務**が層の責務に一致するかで判定する。形式が正しくても責務が層に属さなければ `判定: 不合格` とする。これは [hexagonal-implementation-rules.md の哲学](hexagonal-implementation-rules.md#哲学)（「visibility はシンボルの見え方を制御するが、そのコードが属すべき層の責務を変えない」）から導かれる強制原則である。


## adapters/ 配下

### なぜこの制約が存在するか

`adapter` は外部技術とポートの契約の間を翻訳する唯一の場所である。`adapter` が port trait 実装以外の型・関数を `pub(crate)` 以上で公開した時点で、その公開面は「翻訳者」の役割を超えた結合点になる。呼び出し側がその面に依存し始めると、アダプター差し替え時にその依存が壊れる。`pub(crate)` はクレート内に見えるという意味であり、「外部公開ではない」という意味ではない。`adapters/` 配下の全関数・型・定数について、port trait を実装する型またはそのメソッド実装でなければ `pub` も `pub(crate)` も `pub(super)` も禁止である。「adapter 内部で使いやすいから」という理由は公開の正当化にならない。

- **依存方向**: `port`、`domain`、`support` にのみ依存していること。`application` の use case 型・flow 関数を import していないこと。
- **責務**: port trait の実装、外部 API 変換、SDK bridge に限定されていること。use case の順序制御・domain policy の決定を含まないこと。
- **公開面（絶対規則）**: `pub`・`pub(crate)`・`pub(super)` で外部に公開できるのは、port trait を実装する型（struct/enum）とそのメソッド実装のみ。stdin 読み取り関数・プロンプト関数・JSON デコード関数・terminal I/O 関数・定数は port trait 実装の一部でない限り private にとどめること。

### レビュー時の問い

- このコードは「翻訳」のみをしているか。外部技術の型をポートの型に変換する以外の判断・順序制御・ポリシー決定を持ち込んでいないか。
- この adapter は domain object のビジネスロジック（manifest/blob 整合判定、nonce/AAD 規則、`SecretName::additional_data` の意味適用、鍵生成可否判断）を直接実行していないか。実行している場合は application/domain へ戻すべきではないか。
- adapter の処理を「低水準だから adapter」と判定していないか。SDK/PIV 呼び出しや codec 呼び出しであっても、domain object の関連づけ、保存可能条件、AAD/nonce/manifest/blob の業務意味、上書き可否、値制約を決めているなら adapter から除去すること。
- adapter の private helper が domain object を分解して業務規則を再構築していないか。helper が private であることは business logic 混入の免除理由にならない。
- `pub(crate)` または `pub(super)` で公開されているシンボルについて：「これを公開しなければならない理由はポートの契約を果たすためか」と問え。「内部で使いやすいから」「呼び出し元が必要としているから」は公開の正当化にならない。後者はアーキテクチャ違反を呼び出し元が要求している状態であり、呼び出し元側の違反を意味する。
- このコードが存在する理由を一文で言えるか。「外部技術Xとポート契約Yの間を翻訳するため」と言えるか。それ以外の理由が混在しているなら、その部分は adapter に属さない。
- このファイルを削除して別の技術に差し替えたとき、application/ や domain/ のコードを変更する必要が生じるか。生じるなら、adapter がその依存を外に漏らしている。
- このファイル内の private な関数・ロジックを含む全コードについて：外部技術とポートの契約の間の翻訳のみを行っているか。use case の順序制御・domain policy の決定・ビジネスロジックの判断が private コードとして潜り込んでいないか。
- private helper が増殖している場合、それは port capability が粗すぎる兆候ではないか。helper 群が port 契約不足の穴埋めになっていないか。なっている場合は helper 整理ではなく port 分割を要求すること。
- process-generic な terminal/stdin/stdout helper を adapter private へ直詰めして fat adapter 化していないか。外部技術と port 契約の翻訳ではなく、TTY 制御、blocking read、echo/raw mode、interrupt-aware read などの低レベル実装支援であれば、業務語彙を持たない support として分離できないか確認すること。
- このファイルが独立して存在することで「特定の外部技術とポートの間の翻訳」として何が独立・差し替え可能になるか。答えられないなら分割の意義がない。
- このファイルは複数の外部技術を1ファイルに混在させていないか。異なる外部技術の翻訳が混在しているなら分割されていなければならない。逆に単一の外部技術の翻訳が不必要に複数ファイルに断片化されていないか。
- この分割は「特定の外部技術とポートの間に独立した責務の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- 再エクスポートや委譲のみで実質的な翻訳をしないファイルになっていないか。翻訳の実体を持たないファイルはアダプター層に存在してはならない
。

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
- このファイルが独立した use case として意味を持つか。「このファイルが担う use case とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- use case を細分化した断片になっていないか。逆に複数の use case を1ファイルに詰め込んでいないか。1ファイル1 use case の対応が崩れているなら分割を見直せ。
- 各 sibling file が `run_*` 関数を 1 つだけ持ち、1 use case = 1 function を守っているか。`application/use_case.rs` を再導入して use case 実装の置き場にしていないか。ここでの `run_*` 単一関数制約は production use case entrypoint（`run_*.rs` 本体）に適用し、`#[cfg(test)] mod tests` および `*_tests.rs` は対象外とする。
- `application/use_case/` ディレクトリ、`mod.rs`、`#[path = \"...\"]` を使った配線を導入していないか。
- ある use case が別の use case を呼び出していないか。use case-to-use case call を共通化手段として使っていないか。
- use case 層で logic commonization をしていないか。共通 helper を増やしている場合、その責務は本当に application 層か。重複を許容してでも他層へ押し戻すべきものではないか。
- `application` が use case 順序制御に専念しているか。command dispatch や技術 helper（保護メモリ変換・wire/crypto/persistence 実装詳細）を保持していないか。
- `application.rs` が `application/` 直下の `run_*.rs` にある同等関数へ単純委譲する façade になっていないか。責務を持たない公開面の二重化を作っていないか。
- この分割は「独立した use case の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- doc comment 欠落を「実装を読めば分かる」で見逃していないか。core workflow の責務境界説明がない場合はレビュー不合格にすべきか。

- **依存方向**: `domain` と `port` にのみ依存していること。`support` を含む他層へ依存していないこと。`adapter` の具体型を import していないこと。
- **型規則**: use case 独自の struct / enum / type alias / summary 型を定義していないこと。use case が扱う型は `domain` 層で定義された型のみに限定されていること。
- **関数構成**: `rust/dotfiles-cli/src/secrets/application/` 配下では production use case entrypoint の sibling file ごとに `run_*` 関数を 1 つだけ持たせること。`application/use_case.rs` を再導入せず、`application/use_case/`・`mod.rs`・`#[path = \"...\"]` による疑似レイヤー配線を持ち込まないこと。`#[cfg(test)] mod tests` と `*_tests.rs` はこの単一 `run_*` 制約の対象外。
- **責務**: use case の順序制御・分岐・停止条件に限定されていること。意味・方針・summary semantics の決定、`println!`、stdin 読み取り、concrete device handle 操作を含まないこと。
- **依存取得**: 乱数生成は port 経由のみとし、`application` が直接 external crate や標準 API から乱数を取得していないこと。
- **外部 crate 例外**: `application` で使う外部 crate は `anyhow` と `zeroize` のみに限定されていること。
- **共通化禁止**: use case-to-use case call と、use case 層での logic commonization を禁止する。共通 helper の抽出を完了条件にしてはならず、重複は許容される。
- **配置**: adapter 実装ファイルを `application/` 配下に置かないこと。
- **ドキュメントコメント（必須）**: use-case entrypoint と非自明な internal type/function に doc comment があり、主要契約の先頭文と `why`/責務境界（順序制御理由・停止条件・caller responsibility のいずれか）を示していること。欠落時は `判定: 不合格`。

## domain/ 配下

### レビュー時の問い

- このコードは「ビジネスルール」だけを知っているか。技術選定（どのSDK、どのプロトコル、どのシリアライズ形式）を知っていないか。
- domain を単なる data shape / validation 置き場として扱っていないか。ビジネスロジック、業務判断、業務上の失敗条件を domain の第一責務として表現しているか。
- domain がオブジェクト単体の validation だけに矮小化されていないか。オブジェクト同士の関連づけ、保存 layout の対応、変換の正当性、状態遷移、業務上の失敗条件も domain の責務として表現されているか。
- application や adapter に散らばった object 操作を、domain concept として名前付けできないか。名前付けできるなら、その規則は domain へ戻すべきではないか。
- ここに定義された型について：「この型はインフラを差し替えたとき変更を必要とするか」と問え。変更が必要なら domain に置くべきではない。
- このコードはインフラが存在しない環境（ファイルもネットワークも外部SDKもない）でも意味を持つか。意味を持たないなら domain に属さない。
- この型・関数の名前はビジネスの語彙か、技術の語彙か。技術の語彙が現れていたら domain に属さない可能性が高い。
- このファイル内の private な関数・ロジックを含む全コードについて：外部 SDK 型・端末状態・プロセス状態への依存が含まれていないか。private であることはこれらの禁止事項の免除理由にならない。
- この型・関数が独立したドメイン概念として意味を持つか。「このファイルが表すドメイン概念とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 技術的な便宜（「他の場所でも使いたいから」「長くなったから」）のために分割されていないか。ドメイン概念の境界ではなく実装都合による分割はドメインの設計意図を曇らせる。
- この分割は「独立したドメイン概念の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 外部 SDK 型・端末状態・プロセス状態へ依存しないこと。`domain` の外部 crate 利用可否は作業定義文書の current-cycle 指示に従って判定し、`言語標準ライブラリ以外に依存しない` という単独規則を機械適用しないこと（`YubiKey` current-cycle では `ProtectedSecret`/`anyhow` を理由に不合格としてはならない）。
- **責務**: value/newtype・不変条件・状態遷移・wire format・domain error に限定されていること。
- **禁止成果物**: port contract（trait）・presentation DTO・`std::io::Write` 等の I/O 型を含まないこと。

## ports/ 配下

### レビュー時の問い

- この trait またはこの型は「ドメインが何を必要とするかの宣言」だけになっているか。「どのように実装するか」の詳細や意味づけ済みの実装選択を含んでいないか。
- ここに定義された struct/enum について：「これは技術の詳細（SDK型・パーサー・プロンプト文言・raw bytes）か、それともドメインの意図の表明か」と問え。技術詳細であれば adapter に属する。
- port が肥大化しているとき：「これはドメインの要求か、それともアダプターの実装都合か」を問え。
- port が複数の責務をまとめて隠す fat port になっていないか。device I/O、key operation、storage codec、crypto、manifest 検証、上書き判定を 1 trait / 1 method に押し込んでいないか。
- port の method が「何を要求するか」ではなく「手順を丸ごと済ませること」を表していないか。手順や規則は application/domain、外部 API 翻訳は adapter に分解すること。
- この trait を読んだとき「システムが外界に何を要求しているか」が分かるか。「どうやって実現するか」の痕跡が残っていたら ports に属さない。
- この trait の各メソッドはドメインの意図を表しているか。「ファイルを読む」「HTTPリクエストを送る」という技術操作ではなく「秘密情報を取得する」「認証を検証する」というドメイン操作になっているか。
- このファイル内の private な型・関数・ロジックを含む全コードについて：「ドメインが何を必要とするかの宣言のみ」という責務を満たしているか。parser・DTO・prompt・利用者向け文言が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この trait が独立した capability として意味を持つか。「このファイルが宣言する capability とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 1つの外部依存の契約を不必要に複数の trait に分割していないか。逆に無関係な複数の capability を1ファイルに混在させていないか。capability の境界と trait の境界が一致しているか確認せよ。
- trait 境界は「大まかな外部機能」単位になっているか。DB 接続、ファイルアクセス、CUI など変更理由が異なる外部機能を 1 trait に混在させていないか。
- device 取得・secret 入力・secret 出力・report 出力のように変更理由が異なる capability を 1 trait に押し込んでいないか。use case が必要とする capability だけを境界として要求できる構造になっているか。
- `Summary` / `Report` の具体 DTO が ports に定義されていないか。ports に置けるのは report 出力 capability の宣言までであり、use case が扱う outcome の意味は domain、presentation DTO は adapter 側に置くこと。
- この分割は「独立した capability の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: `domain` にのみ依存していること。`support` の具体型（`ProtectedSecret` 等）へ直接依存していないこと。
- **責務**: capability contract を表す trait・request/response の最小境界型に限定されていること。
- **trait 粒度**: 1 trait = 1 外部機能を基本とし、外部機能単位 trait と adapter module 分割が対応していること。
- **禁止成果物**: DTO・parser・prompt・利用者向け文言を含まないこと。

## support/ 配下

### レビュー時の問い

- このコードに業務語彙が含まれていないか。機能固有の名前（特定のサービス名・コマンド名・ロール名）が現れていたら support に置くべきではない。
- support に product-specific storage mechanism を置いていないか。暗号や binary codec を使っていても、YubiKey/Bitwarden/enroll/verify/secret storage role の語彙や保存 format の業務意味を持つなら support ではない。
- support の関数が特定機能の domain object や adapter SDK 型を受け取っていないか。汎用 primitive と機能固有 wrapper を分離し、support は前者に限定すること。
- terminal I/O・prompt 周辺のコードがここにある場合は、その責務が process-generic な補助か、feature-specific な interaction 方針かを区別せよ。TTY open、echo/raw mode 制御、blocking read、interrupt-aware read、byte stream としての stdin/stdout 処理のような前者は support に置ける。specific command の prompt 文言、device 選択判断、use case 手順のような後者は support に属さない。「共通部品だから support」という判断だけで正当化してはならない。
- このコードの名前からプロダクトの機能・ドメインを推測できるか。推測できるなら support に属さない — それは業務語彙を持っている。
- このコードを別のまったく異なるプロダクトにそのままコピーして使えるか。使えないなら、プロダクト固有の知識が混入している。
- このファイル内の private な関数・ロジックを含む全コードについて：業務語彙を持たない共通技術部品のみという責務を満たしているか。機能固有の名称（特定のサービス名・コマンド名・ロール名）・feature-specific な prompt 文言・device 選択判断・use case 手順が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この部品が汎用的な技術部品として独立した意味を持つか。「このファイルが提供する技術部品とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 特定機能に依存した「共通っぽいもの」になっていないか。support にあるが特定の機能でしか使われない部品は、その機能への依存が混入していないか確認せよ。
- この分割は「独立した技術部品の責務境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 言語標準ライブラリと外部技術 crate にのみ依存していること。他層の業務語彙へ依存しないこと。
- **責務**: 業務語彙を持たない共通技術部品（保護メモリ・暗号プリミティブ・byte utility・process-generic な標準入出力補助）に限定されていること。
- **禁止成果物**: feature-specific な terminal I/O 方針、prompt 文言、device 選択判断、機能固有 vocabulary・command 名・role 名を含まないこと。
- **レビュー禁止**: `support` に process/terminal I/O が存在することだけを理由に `判定: 不合格` としてはならない。不合格にする場合は、業務語彙、feature-specific な interaction 方針、domain object 操作、domain/application からの直接利用、または support ではなく別層に属する責務を具体的に示すこと。

## secret-recovery 判定クイックガイド（application / ports / adapters / support）

各層レビューで次を追加確認する。

- application:
  command dispatch、input modality（Prompt/Stdin/StdinJson 等）、report DTO 変換、protected buffer 化、crypto helper 呼び出し詳細、device selection 実装を含めていないか。
- ports:
  modality enum や stdin-json の手段表現を契約へ露出していないか。`read_secret_from_prompt` のように capability 名で表現されているか。
- adapters:
  `application::...` 型へ直接依存せず、port 契約へ変換しているか。device selection と report 出力は adapter が担い、技術的な実行/翻訳以外の意味づけや use case 順序は持ち込んでいないか。
- support:
  protected buffer / zeroization / crypto helper / process-generic な標準入出力補助に限定され、YubiKey など業務語彙の error 文言、device 選択判断、use case 手順を持っていないか。

上記は形式チェックではなく責務判定である。`pub` 範囲や feature gate の有無は免除理由にならない。

## entrypoint/ 配下

- **依存方向**: `application` と `domain` に依存できること。`adapter` の具体型へ直接依存しないこと。
- **責務**: command 定義・引数値変換・呼び出し開始 DTO・終了 code 変換に限定されていること。domain rule・順序制御・device 制御を含まないこと。

## tests/ 配下・`*_tests.rs`・`test_*.rs`

### レビュー時の問い

- production 層（`adapters/`・`application/`・`domain/`・`ports/`・`support/`）の各ファイルに、実依存を肩代わりする責務を持つ型（Fake/Stub/Mock）が定義されていないか。その型は、テストのためだけに存在し本番経路では使われないか。そうであれば配置違反である。
- `#[cfg(all(test, feature = "..."))]` で `tests/` 配下の test support を module context へ `include!` する箇所について、それが [Rust private module 用 test-only bridge](hexagonal-implementation-rules.md#rust-private-module-用-test-only-bridge) の条件をすべて満たしているか。満たす場合は production source tree への test double 混入として扱ってはならない。
- production 層に置かれた `#[cfg(test)]`/`#[cfg(feature = "...")]` ブロックの中身は、(a) その module 自身の private 関数を検証する `#[test]` 関数か、(b) 実依存を肩代わりする double の**定義**か。(b) であれば配置違反である。(a) は許可される（後述）。
- ある double の責務は「テスト時に実依存を substitute すること」か。そうであれば、それが port trait を実装していても・feature gate されていても、production 層ではなく `tests/` 層または専用 test-support crate に属する。

### 許可される in, 禁止される out（責務で区別する）

- **許可**: production 層の `src/` ファイル内に置かれた通常の inline unit test（`#[cfg(test)] mod tests { #[test] fn ... }`）。これはその module 自身の private 関数を検証する標準的かつ idiomatic な Rust であり、削除を要求してはならない。inline unit test を一律禁止すると、本番関数をテストのためだけに `pub` 化する圧力が生じ、公開面最小化の哲学に反する。`#[test]` 関数の存在のみを理由に `判定: 不合格` としてはならない。
- **許可**: Rust private module / private usecase を検査するため、`#[cfg(all(test, feature = "..."))]` で `tests/` 配下の test support を module context へ `include!` する test-only bridge。許可条件は `hexagonal-implementation-rules.md` の同節を正本とし、production build 非混入、internal test 専用 feature、runtime real/stub 分岐なし、production command path 変更なし、port trait 契約での駆動、domain/business logic 非移動、mock/fake 本体の `tests/` 配置、module/comment による xtask/internal test 経路明記をすべて確認する。
- **不許可**: production 層に置かれた型の責務が「テスト専用の実依存肩代わり」であり、かつ当該層の責務と一致しない場合。`#[cfg(test)]` / `#[cfg(feature = "...")]` / port trait 実装の有無だけで機械的に判定してはならない。責務の不一致を根拠として不合格にする。

判定の分かれ目は「形式（`#[cfg(test)]` か `#[cfg(feature)]` か）」ではなく「責務（その module 自身の検証か、実依存の肩代わり定義か）」である。
