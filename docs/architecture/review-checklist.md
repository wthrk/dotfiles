# レビュー観点チェックリスト（構造）

この文書は、ディレクトリパターン別の構造レビュー観点の正本である。層ごとの責務・禁止事項・依存方向・公開範囲の定義は [hexagonal-implementation-rules.md](hexagonal-implementation-rules.md) を正本とし、この文書はそこから導かれたチェック項目を定義する。

## チェックの進め方

1. レビュー対象ファイルのディレクトリ名から所属層を確定する（[ディレクトリと層の対応規則](hexagonal-implementation-rules.md#ディレクトリと層の対応規則)）。
2. 所属層に対応するセクションのチェック項目を適用する。
3. ディレクトリ名と層が一致しないファイルは配置違反として記録する。

### 責務基準の判定原則（形式より責務）

このチェックリストの全項目は、形式（ファイル名パターン・命名・公開面の有無・port trait を実装しているか・`#[cfg(test)]` か `#[cfg(feature)]` で gate されているか）ではなく、コードの**責務**が層の責務に一致するかで判定する。形式が正しくても責務が層に属さなければ `判定: 不合格` とする。これは [hexagonal-implementation-rules.md の哲学](hexagonal-implementation-rules.md#哲学)（「visibility はシンボルの見え方を制御するが、そのコードが属すべき層の責務を変えない」）から導かれる強制原則である。

レビュー担当は、各シンボル・各ファイル・各 `#[cfg(test)]`/`#[cfg(feature = "...")]` ブロックについて、次の2問を必ず立て、`根拠:` に回答を明示しなければならない。

- 問1: このコードの責務は何か（一文で述べる）。
- 問2: その責務はこのファイルが属する層の責務か。

問2の答えが「否」であれば、以下のいずれが成立していても `判定: 不合格` とする。形式的正しさは責務不一致の免除理由にならない。

- 正しいディレクトリに置かれている。
- 命名規約に従っている。
- port trait を実装している。
- `#[cfg(test)]` でラップされている。
- `#[cfg(feature = "...")]` で gate されている。
- 通常 build には含まれない。

## adapters/ 配下

### なぜこの制約が存在するか

`adapter` は外部技術とポートの契約の間を翻訳する唯一の場所である。`adapter` が port trait 実装以外の型・関数を `pub(crate)` 以上で公開した時点で、その公開面は「翻訳者」の役割を超えた結合点になる。呼び出し側がその面に依存し始めると、アダプター差し替え時にその依存が壊れる。`pub(crate)` はクレート内に見えるという意味であり、「外部公開ではない」という意味ではない。`adapters/` 配下の全関数・型・定数について、port trait を実装する型またはそのメソッド実装でなければ `pub` も `pub(crate)` も `pub(super)` も禁止である。「adapter 内部で使いやすいから」という理由は公開の正当化にならない。

- **依存方向**: `port`、`domain`、`support` にのみ依存していること。`application` の use case 型・flow 関数を import していないこと。
- **責務**: port trait の実装、外部 API 変換、SDK bridge に限定されていること。use case の順序制御・domain policy の決定を含まないこと。
- **公開面（絶対規則）**: `pub`・`pub(crate)`・`pub(super)` で外部に公開できるのは、port trait を実装する型（struct/enum）とそのメソッド実装のみ。stdin 読み取り関数・プロンプト関数・JSON デコード関数・terminal I/O 関数・定数は port trait 実装の一部でない限り private にとどめること。

### レビュー時の問い

- このコードは「翻訳」のみをしているか。外部技術の型をポートの型に変換する以外の判断・順序制御・ポリシー決定を持ち込んでいないか。
- `pub(crate)` または `pub(super)` で公開されているシンボルについて：「これを公開しなければならない理由はポートの契約を果たすためか」と問え。「内部で使いやすいから」「呼び出し元が必要としているから」は公開の正当化にならない。後者はアーキテクチャ違反を呼び出し元が要求している状態であり、呼び出し元側の違反を意味する。
- このコードが存在する理由を一文で言えるか。「外部技術Xとポート契約Yの間を翻訳するため」と言えるか。それ以外の理由が混在しているなら、その部分は adapter に属さない。
- このファイルを削除して別の技術に差し替えたとき、application/ や domain/ のコードを変更する必要が生じるか。生じるなら、adapter がその依存を外に漏らしている。
- このファイル内の private な関数・ロジックを含む全コードについて：外部技術とポートの契約の間の翻訳のみを行っているか。use case の順序制御・domain policy の決定・ビジネスロジックの判断が private コードとして潜り込んでいないか。
- private helper が増殖している場合、それは port capability が粗すぎる兆候ではないか。helper 群が port 契約不足の穴埋めになっていないか。なっている場合は helper 整理ではなく port 分割を要求すること。
- このファイルが独立して存在することで「特定の外部技術とポートの間の翻訳」として何が独立・差し替え可能になるか。答えられないなら分割の意義がない。
- このファイルは複数の外部技術を1ファイルに混在させていないか。異なる外部技術の翻訳が混在しているなら分割されていなければならない。逆に単一の外部技術の翻訳が不必要に複数ファイルに断片化されていないか。
- この分割は「特定の外部技術とポートの間に独立した責務の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- 再エクスポートや委譲のみで実質的な翻訳をしないファイルになっていないか。翻訳の実体を持たないファイルはアダプター層に存在してはならない。
- このファイル（または `#[cfg(feature = "...")]`/`#[cfg(test)]` で gate されたモジュール）の責務は、実在の外部技術とポート契約の間の翻訳か、それともテスト用に実依存を肩代わりすること（device の模倣・固定応答の返却・in-memory state の保持）か。後者であれば、それは test double（Fake/Stub/Mock）であり、`adapter` 層の責務ではない。port trait（`SecretDevice` 等）を実装していること、`#[cfg(feature = "...")]` で gate されていること、`#[cfg(test)]` でラップされていることは、いずれもこの判定の免除理由にならない。test double の定義は `tests/` 層または専用の test-support crate に属する。

### 確認手順

`adapters/` 配下では、後続の公開面・private 関数チェックを行う前に、まず各ファイル・各 gate ブロックの責務がテスト用の肩代わり（test double）でないことを確認する。順序は次のとおり。

0. **test double 混入検出（先行・必須）**: `adapters/` 配下の各ファイルおよび各 `#[cfg(feature = "...")]`/`#[cfg(test)]` ブロックについて、その責務が「実在の外部技術とポート契約の間の翻訳」か「テスト用に実依存を肩代わりすること」かを判定する。次のいずれかに該当する型・モジュールは test double であり、port trait を実装していても・feature gate されていても・通常 build に含まれなくても、`adapter` 層への配置は配置違反であり即座に `判定: 不合格` とする。
   - 実在のデバイス／外部 API へ接続せず、in-memory state・固定値・乱数で応答を生成する port 実装。
   - 名前または doc comment が stub/fake/mock/test/dummy を表し、テスト時にのみ実依存の代わりに使われる型。
   - integration test contract・test fixture からのみ駆動され、本番経路では決して使われない型。
   - 解消方法は、当該 test double 定義を `tests/` 層または専用 test-support crate へ移動すること（feature gate の有無では解消しない）。

1. `adapters/` 配下の全ファイルを開く（`adapters.rs` 含む）。対象ファイルの列挙は作業定義文書の「対象コードパス」に依存せず、ディレクトリ内の全ファイルを自分で確認すること。
2. 各ファイルで `pub fn`、`pub(crate) fn`、`pub(super) fn`、`pub struct`、`pub(crate) struct`、`pub(super) struct`、`pub type`、`pub const` をすべて列挙する。
3. 列挙した各公開シンボルについて「これは port trait を実装する型か、またはそのメソッド実装か」を判定する。port trait を実装していること自体は配置の十分条件ではない。手順0で test double と判定した型は、port trait を実装していても `adapter` 層に属さない。
4. 1件でも「port trait 実装でない公開シンボル」または「port trait を実装するが責務が翻訳でない型（手順0の test double 等）」が存在した場合、即座に `判定: 不合格` とする。
5. `adapters.rs`（または `adapters/mod.rs`）が `pub(super)` で子モジュールを公開している場合、そのモジュール内の公開シンボルが親モジュールから参照可能になる。この経路も確認する。
6. 各ファイルの `fn`（private 関数）をすべて列挙する。列挙した各 private 関数について「この関数の責務は翻訳（外部技術の型をポートの型に変換すること）のみか」を判定する。use case の順序制御・domain policy の決定・ビジネスロジックの判断を含む private 関数が1件でも存在した場合、即座に `判定: 不合格` とする。private であることはこの制約の免除理由にならない。
7. file-level に分割済みでも、helper 単位の責務判定を省略してはならない。各 helper について「責務は何か」「その責務はこの層か」を記録し、1件でも答えられなければ `判定: 不合格` とする。

## application/ 配下

### レビュー時の問い

- このコードはユースケースの「順序」と「分岐」を知っているだけか。具体的なデバイス、I/O、パーサーの実装や意味づけを知っていないか。
- use-case entrypoint と非自明な internal type/function（private helper 含む）に、主要契約と責務境界を示す doc comment があるか。`what` の言い換えだけで `why` が欠落していないか。
- `boundary.some_method()` の呼び出しを見たとき：「この呼び出しはポートの契約を通じているか、それともアダプターの具体型を直接知っているか」を確認せよ。
- `application/` 配下にアダプター実装が紛れ込んでいないか。ファイル名ではなく責務で判断せよ。
- このコードが「何をするか」ではなく「何の順序で、どこで分岐して呼ぶか」だけを知っているか。具体的な入出力の形・エラーの詳細・デバイスの挙動・summary の意味づけを知っていたら application に属さない。
- このuse caseを別のインターフェース（CLI→Web API）に移植したとき、このファイルの変更は最小限で済むか。済まないなら、インターフェース固有の知識が混入している。
- このファイル内の private な関数・ロジックを含む全コードについて：concrete I/O（`println!`・stdin 読み取り・端末文言）、アダプターの具体型参照、外部 SDK 型が含まれていないか。private であることはこれらの禁止事項の免除理由にならない。
- 乱数生成（key material、nonce、challenge、seed 等）を `application` が直接呼んでいないか。必ず port 経由になっているか。
- private helper が増え続けている場合、port capability が粗く application が技術詳細の調停を肩代わりしていないか。helper 追加で対応せず、port 契約の再分割要求を返せるか。
- このファイルが独立した use case として意味を持つか。「このファイルが担う use case とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- use case を細分化した断片になっていないか。逆に複数の use case を1ファイルに詰め込んでいないか。1ファイル1 use case の対応が崩れているなら分割を見直せ。
- 各 sibling file が `run_*` 関数を 1 つだけ持ち、1 use case = 1 function を守っているか。`application/use_case.rs` を再導入して use case 実装の置き場にしていないか。
- `application/use_case/` ディレクトリ、`mod.rs`、`#[path = \"...\"]` を使った配線を導入していないか。
- ある use case が別の use case を呼び出していないか。use case-to-use case call を共通化手段として使っていないか。
- use case 層で logic commonization をしていないか。共通 helper を増やしている場合、その責務は本当に application 層か。重複を許容してでも他層へ押し戻すべきものではないか。
- `application` が use case 順序制御に専念しているか。command dispatch や技術 helper（保護メモリ変換・wire/crypto/persistence 実装詳細）を保持していないか。
- `application.rs` が `application/` 直下の `run_*.rs` にある同等関数へ単純委譲する façade になっていないか。責務を持たない公開面の二重化を作っていないか。
- この分割は「独立した use case の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。
- doc comment 欠落を「実装を読めば分かる」で見逃していないか。core workflow の責務境界説明がない場合はレビュー不合格にすべきか。

- **依存方向**: `domain` と `port` にのみ依存していること。`support` を含む他層へ依存していないこと。`adapter` の具体型を import していないこと。
- **型規則**: use case 独自の struct / enum / type alias / summary 型を定義していないこと。use case が扱う型は `domain` 層で定義された型のみに限定されていること。
- **関数構成**: `rust/dotfiles-cli/src/secrets/application/` 配下では sibling file ごとに `run_*` 関数を 1 つだけ持たせること。`application/use_case.rs` を再導入せず、`application/use_case/`・`mod.rs`・`#[path = \"...\"]` による疑似レイヤー配線を持ち込まないこと。
- **責務**: use case の順序制御・分岐・停止条件に限定されていること。意味・方針・summary semantics の決定、`println!`、stdin 読み取り、concrete device handle 操作を含まないこと。
- **依存取得**: 乱数生成は port 経由のみとし、`application` が直接 external crate や標準 API から乱数を取得していないこと。
- **外部 crate 例外**: `application` で使う外部 crate は `anyhow` と `zeroize` のみに限定されていること。
- **共通化禁止**: use case-to-use case call と、use case 層での logic commonization を禁止する。共通 helper の抽出を完了条件にしてはならず、重複は許容される。
- **配置**: adapter 実装ファイルを `application/` 配下に置かないこと。
- **ドキュメントコメント（必須）**: use-case entrypoint と非自明な internal type/function に doc comment があり、主要契約の先頭文と `why`/責務境界（順序制御理由・停止条件・caller responsibility のいずれか）を示していること。欠落時は `判定: 不合格`。

## domain/ 配下

### レビュー時の問い

- このコードは「ビジネスルール」だけを知っているか。技術選定（どのSDK、どのプロトコル、どのシリアライズ形式）を知っていないか。
- ここに定義された型について：「この型はインフラを差し替えたとき変更を必要とするか」と問え。変更が必要なら domain に置くべきではない。
- このコードはインフラが存在しない環境（ファイルもネットワークも外部SDKもない）でも意味を持つか。意味を持たないなら domain に属さない。
- この型・関数の名前はビジネスの語彙か、技術の語彙か。技術の語彙が現れていたら domain に属さない可能性が高い。
- このファイル内の private な関数・ロジックを含む全コードについて：外部 SDK 型・端末状態・プロセス状態への依存が含まれていないか。private であることはこれらの禁止事項の免除理由にならない。
- この型・関数が独立したドメイン概念として意味を持つか。「このファイルが表すドメイン概念とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 技術的な便宜（「他の場所でも使いたいから」「長くなったから」）のために分割されていないか。ドメイン概念の境界ではなく実装都合による分割はドメインの設計意図を曇らせる。
- この分割は「独立したドメイン概念の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 言語標準ライブラリ以外に依存しないこと。外部 SDK 型・端末状態・プロセス状態へ依存しないこと。
- **責務**: value/newtype・不変条件・状態遷移・wire format・domain error に限定されていること。
- **禁止成果物**: port contract（trait）・presentation DTO・`std::io::Write` 等の I/O 型を含まないこと。

## ports/ 配下

### レビュー時の問い

- この trait またはこの型は「ドメインが何を必要とするかの宣言」だけになっているか。「どのように実装するか」の詳細や意味づけ済みの実装選択を含んでいないか。
- ここに定義された struct/enum について：「これは技術の詳細（SDK型・パーサー・プロンプト文言・raw bytes）か、それともドメインの意図の表明か」と問え。技術詳細であれば adapter に属する。
- port が肥大化しているとき：「これはドメインの要求か、それともアダプターの実装都合か」を問え。
- この trait を読んだとき「システムが外界に何を要求しているか」が分かるか。「どうやって実現するか」の痕跡が残っていたら ports に属さない。
- この trait の各メソッドはドメインの意図を表しているか。「ファイルを読む」「HTTPリクエストを送る」という技術操作ではなく「秘密情報を取得する」「認証を検証する」というドメイン操作になっているか。
- このファイル内の private な型・関数・ロジックを含む全コードについて：「ドメインが何を必要とするかの宣言のみ」という責務を満たしているか。parser・DTO・prompt・利用者向け文言が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この trait が独立した capability として意味を持つか。「このファイルが宣言する capability とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 1つの外部依存の契約を不必要に複数の trait に分割していないか。逆に無関係な複数の capability を1ファイルに混在させていないか。capability の境界と trait の境界が一致しているか確認せよ。
- device 取得・secret 入力・secret 出力・report 出力のように変更理由が異なる capability を 1 trait に押し込んでいないか。use case が必要とする capability だけを境界として要求できる構造になっているか。
- `Summary` / `Report` の具体 DTO が ports に定義されていないか。ports に置けるのは report 出力 capability の宣言までであり、use case が扱う outcome の意味は domain、presentation DTO は adapter 側に置くこと。
- この分割は「独立した capability の境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: `domain` にのみ依存していること。`support` の具体型（`ProtectedSecret` 等）へ直接依存していないこと。
- **責務**: capability contract を表す trait・request/response の最小境界型に限定されていること。
- **禁止成果物**: DTO・parser・prompt・利用者向け文言を含まないこと。

## support/ 配下

### レビュー時の問い

- このコードに業務語彙が含まれていないか。機能固有の名前（特定のサービス名・コマンド名・ロール名）が現れていたら support に置くべきではない。
- terminal I/O・prompt がここにあったら、それは adapter に属する。「共通部品だから support」という判断は誤りである。
- このコードの名前からプロダクトの機能・ドメインを推測できるか。推測できるなら support に属さない — それは業務語彙を持っている。
- このコードを別のまったく異なるプロダクトにそのままコピーして使えるか。使えないなら、プロダクト固有の知識が混入している。
- このファイル内の private な関数・ロジックを含む全コードについて：業務語彙を持たない共通技術部品のみという責務を満たしているか。機能固有の名称（特定のサービス名・コマンド名・ロール名）・terminal I/O・prompt が private コードとして潜り込んでいないか。private であることはこれらの禁止事項の免除理由にならない。
- この部品が汎用的な技術部品として独立した意味を持つか。「このファイルが提供する技術部品とは何か」を一文で言えるか。答えられないなら分割の意義がない。
- 特定機能に依存した「共通っぽいもの」になっていないか。support にあるが特定の機能でしか使われない部品は、その機能への依存が混入していないか確認せよ。
- この分割は「独立した技術部品の責務境界があるから」という設計上の理由によるか。「長くなったから」「再利用したいから」「まとめたいから」は分割の正当な理由にならない。

- **依存方向**: 言語標準ライブラリと外部技術 crate にのみ依存していること。他層の業務語彙へ依存しないこと。
- **責務**: 業務語彙を持たない共通技術部品（保護メモリ・暗号プリミティブ・byte utility）に限定されていること。
- **禁止成果物**: terminal I/O・prompt・機能固有 vocabulary・command 名・role 名を含まないこと。

## secret-recovery 判定クイックガイド（application / ports / adapters / support）

各層レビューで次を追加確認する。

- application:
  command dispatch、input modality（Prompt/Stdin/StdinJson 等）、report DTO 変換、protected buffer 化、crypto helper 呼び出し詳細、device selection 実装を含めていないか。
- ports:
  modality enum や stdin-json の手段表現を契約へ露出していないか。`read_secret_from_prompt` のように capability 名で表現されているか。
- adapters:
  `application::...` 型へ直接依存せず、port 契約へ変換しているか。device selection と report 出力は adapter が担い、技術的な実行/翻訳以外の意味づけや use case 順序は持ち込んでいないか。
- support:
  protected buffer / zeroization / crypto helper に限定され、YubiKey など業務語彙の error 文言を持っていないか。

上記は形式チェックではなく責務判定である。`pub` 範囲や feature gate の有無は免除理由にならない。

## entrypoint/ 配下

- **依存方向**: `application` と `domain` に依存できること。`adapter` の具体型へ直接依存しないこと。
- **責務**: command 定義・引数値変換・呼び出し開始 DTO・終了 code 変換に限定されていること。domain rule・順序制御・device 制御を含まないこと。

## tests/ 配下・`*_tests.rs`・`test_*.rs`

### レビュー時の問い

- production 層（`adapters/`・`application/`・`domain/`・`ports/`・`support/`）の各ファイルに、実依存を肩代わりする責務を持つ型（Fake/Stub/Mock）が定義されていないか。その型は、テストのためだけに存在し本番経路では使われないか。そうであれば配置違反である。
- production 層に置かれた `#[cfg(test)]`/`#[cfg(feature = "...")]` ブロックの中身は、(a) その module 自身の private 関数を検証する `#[test]` 関数か、(b) 実依存を肩代わりする double の**定義**か。(b) であれば配置違反である。(a) は許可される（後述）。
- ある double の責務は「テスト時に実依存を substitute すること」か。そうであれば、それが port trait を実装していても・feature gate されていても、production 層ではなく `tests/` 層または専用 test-support crate に属する。

### 許可される in, 禁止される out（責務で区別する）

- **許可**: production 層の `src/` ファイル内に置かれた通常の inline unit test（`#[cfg(test)] mod tests { #[test] fn ... }`）。これはその module 自身の private 関数を検証する標準的かつ idiomatic な Rust であり、削除を要求してはならない。inline unit test を一律禁止すると、本番関数をテストのためだけに `pub` 化する圧力が生じ、公開面最小化の哲学に反する。`#[test]` 関数の存在のみを理由に `判定: 不合格` としてはならない。
- **禁止**: production 層に置かれた test double の**定義**（Fake/Stub/Mock 型、すなわち実依存を肩代わりする型）。これは `#[cfg(test)]` でラップされていても・`#[cfg(feature = "...")]` で gate されていても・port trait を実装していても禁止である。

判定の分かれ目は「形式（`#[cfg(test)]` か `#[cfg(feature)]` か）」ではなく「責務（その module 自身の検証か、実依存の肩代わり定義か）」である。

### 確認手順

1. production 層（`adapters/`・`application/`・`domain/`・`ports/`・`support/`）配下の全ファイルを開く。
2. 各ファイルで `impl <PortTrait> for <Type>` を含む型定義、および名前・doc comment が stub/fake/mock/dummy を表す型定義を列挙する。`#[cfg(test)]`/`#[cfg(feature = "...")]` で gate された定義も対象に含める。
3. 列挙した各型について手順問1（責務は何か）を立てる。責務が「実依存をテスト用に肩代わりすること」である型が production 層に1件でも存在した場合、即座に `判定: 不合格` とする。解消方法は当該定義を `tests/` 層または専用 test-support crate へ移動すること。
4. `#[cfg(test)] mod tests` ブロック内に `#[test]` 関数のみがあり double 定義を含まない場合は配置違反としない。double 定義を含む場合のみ手順3に従う。

- **配置**: test double（Fake/Stub/Mock の定義）・fixture は production tree（`adapters/`・`application/`・`domain/`・`ports/`・`support/` 配下等）に置かないこと。`#[cfg(test)]` ラップや `#[cfg(feature = "...")]` gate、port trait 実装はこの禁止の免除理由にならない。production 層の `src/` における通常の inline unit test（`#[test]` 関数）はこの禁止の対象外であり許可される。
- **責務**: unit test・integration test・test double・fixture に限定されていること。本番公開 API やレビュー代替の設計判断を含まないこと。
- **公開**: test helper を本番 module へ再公開しないこと。
