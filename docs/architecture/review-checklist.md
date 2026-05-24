# レビュー観点チェックリスト（構造）

この文書は、ディレクトリパターン別の構造レビュー観点の正本である。層ごとの責務・禁止事項・依存方向・公開範囲の定義は [hexagonal-implementation-rules.md](hexagonal-implementation-rules.md) を正本とし、この文書はそこから導かれたチェック項目を定義する。

## チェックの進め方

1. レビュー対象ファイルのディレクトリ名から所属層を確定する（[ディレクトリと層の対応規則](hexagonal-implementation-rules.md#ディレクトリと層の対応規則)）。
2. 所属層に対応するセクションのチェック項目を適用する。
3. ディレクトリ名と層が一致しないファイルは配置違反として記録する。

## adapters/ 配下

- **依存方向**: `port`、`domain`、`support` にのみ依存していること。`application` の use case 型・flow 関数を import していないこと。
- **責務**: port trait の実装、外部 API 変換、SDK bridge に限定されていること。use case の順序制御・domain policy の決定を含まないこと。
- **公開面（絶対規則）**: `pub`・`pub(crate)`・`pub(super)` で外部に公開できるのは、port trait を実装する型（struct/enum）とそのメソッド実装のみ。stdin 読み取り関数・プロンプト関数・JSON デコード関数・terminal I/O 関数・定数は port trait 実装の一部でない限り private にとどめること。

## application/ 配下

- **依存方向**: `domain`、`port`、および機能中立な `support` 保護型にのみ依存していること。`adapter` の具体型を import していないこと。
- **責務**: use case の順序制御・分岐・停止条件に限定されていること。`println!`・stdin 読み取り・concrete device handle 操作を含まないこと。
- **配置**: adapter 実装ファイルを `application/` 配下に置かないこと。

## domain/ 配下

- **依存方向**: 言語標準ライブラリ以外に依存しないこと。外部 SDK 型・端末状態・プロセス状態へ依存しないこと。
- **責務**: value/newtype・不変条件・状態遷移・wire format・domain error に限定されていること。
- **禁止成果物**: port contract（trait）・summary DTO・`std::io::Write` 等の I/O 型を含まないこと。

## ports/ 配下

- **依存方向**: `domain` にのみ依存していること。`support` の具体型（`ProtectedSecret` 等）へ直接依存していないこと。
- **責務**: capability contract を表す trait・request/response の最小境界型に限定されていること。
- **禁止成果物**: DTO・parser・prompt・利用者向け文言を含まないこと。

## support/ 配下

- **依存方向**: 言語標準ライブラリと外部技術 crate にのみ依存していること。他層の業務語彙へ依存しないこと。
- **責務**: 業務語彙を持たない共通技術部品（保護メモリ・暗号プリミティブ・byte utility）に限定されていること。
- **禁止成果物**: terminal I/O・prompt・機能固有 vocabulary・command 名・role 名を含まないこと。

## entrypoint/ 配下

- **依存方向**: `application` と `domain` に依存できること。`adapter` の具体型へ直接依存しないこと。
- **責務**: command 定義・引数値変換・呼び出し開始 DTO・終了 code 変換に限定されていること。domain rule・順序制御・device 制御を含まないこと。

## tests/ 配下・`*_tests.rs`・`test_*.rs`

- **配置**: test double・fixture は production tree（`adapters/`・`application/` 配下等）に置かないこと。
- **責務**: unit test・integration test・test double・fixture に限定されていること。本番公開 API やレビュー代替の設計判断を含まないこと。
- **公開**: test helper を本番 module へ再公開しないこと。
