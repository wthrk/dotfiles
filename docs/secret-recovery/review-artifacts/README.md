# review-artifacts

このディレクトリは、secret-recovery のレビュー関連文書を配置する。

ここに文書が存在しても、それだけで実装進捗、レビュー準備完了、作業項目完了を意味してはならない。`docs/secret-recovery/tasks.md` の作業項目は、対象コードパスの実コード差分が存在しない限り、確認証跡やレビュー記録があっても暫定記録として扱う。
`実装状態` / `確認` / `レビュー` を前進させる場合は、同一変更セットで前提証跡（対象コード差分識別子と必要な確認・レビュー成果物）を同時更新しなければならない。`コード差分なし` 記録は暫定記録に限り、前進根拠としては利用できない。

## 配下の項目

- [architecture-rules/README.md](architecture-rules/README.md): アーキテクチャ規約レビュー用文書を案内する。
- [yubikey/confirmation.md](yubikey/confirmation.md): YubiKey 作業項目の確認証跡を記録する。
- [yubikey/review.md](yubikey/review.md): YubiKey 作業項目のレビュー判定を記録する。
