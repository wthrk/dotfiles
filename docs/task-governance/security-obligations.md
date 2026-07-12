# セキュリティ義務

この文書は、実装・確認・レビュー・進捗更新で共通して守るセキュリティ義務の正本である。

## 基本義務

- 秘密情報、認証情報、鍵素材、セッション情報、API トークン、Docker 認証状態、SSH 秘密鍵、アプリセッションファイルをコミットしてはならない。
- ログ、標準出力、コマンド引数、一時ファイル、レビュー証跡に秘密情報を残してはならない。
- 機密値の検証が必要な場合は、秘匿化済みの値または再現手順のみを記録し、平文を記録してはならない。
- 意図的に宣言化する場合を除き、マシン固有の可変状態を Home Manager モジュールに入れてはならない。
- Homebrew taps は flake inputs で固定されるため、設計変更が必要としない限り可変 tap 運用を導入してはならない。

## test-only 観測の判定

`secrets-internal-test-stub` のように compile-time で選択され、production build/runtime に含まれない internal test stub が、fixture/spec で与えたダミー値を stdout sentinel observation として出力することは、利用者向け production stdout への secret 出力ではない。これは integration test が最終 datastore を観測するためだけの test-only 観測チャネルである。

セキュリティレビューでは、上記の出力を raw stdout の secret 漏えいとして機械的に指摘してはならない。次を区別して判定する。

- production command の通常出力、ログ、エラー、引数、環境変数、一時ファイル、または production build/runtime で到達できる出力経路に secret が含まれる場合は不合格とする。
- feature 専用の test stub observation は、[Hexagonal Implementation Rules の internal backend stub の配置](../architecture/hexagonal-implementation-rules.md#internal-backend-stub-の配置) の全条件、特に compile-time selection、production build 非混入、sentinel で明示された観測面、fixture/spec のダミー値だけを扱うことを確認できる場合に限り、production stdout と区別して合格とする。
- feature gate の存在だけではこの例外を適用しない。runtime の real/stub 分岐、production command path の変更、本物 secret の使用、または production build/runtime から到達可能な出力経路があれば、通常の secret 出力として不合格とする。

## 役割別義務

- `実装担当`: 差分作成時に秘密情報の永続化経路、出力経路、失敗時挙動を確認する。
- `セキュリティレビュー担当`: 秘密情報漏えい、権限境界逸脱、危険な失敗時挙動をレビュー記録で明示判定する。
- `進捗判定担当`: セキュリティ所見が未解消の変更セットを前進反映してはならない。

## 記録義務

- 確認記録とレビュー記録には、セキュリティ観点の確認結果と未実施理由（未実施がある場合）を記載する。
- セキュリティ懸念を検出した場合は、差戻し事項と解消条件を同一記録内に記載する。
