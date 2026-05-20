# secret-recovery

このディレクトリは、秘密情報復旧機能に関する文書を配置する。

## 配下の項目

- [implementation-guidelines.md](implementation-guidelines.md): 実装単位、役割分担、実装方針を定義する。
- [secret-recovery-spec.md](secret-recovery-spec.md): 秘密情報復旧機能の仕様を定義する。
- [bitwarden-secrets-manager-design.md](bitwarden-secrets-manager-design.md): Bitwarden Secrets Manager 取得経路の設計を定義する。
- [yubikey-secret-storage-design.md](yubikey-secret-storage-design.md): YubiKey 保存方式の設計を定義する。
- [tasks.md](tasks.md): 固定実装単位ごとの進捗と完了条件を追跡する進捗入口。
- [review-artifacts/README.md](review-artifacts/README.md): レビュー関連文書の配置先を案内する。

## 進捗運用

- `次のタスク` の解釈規則は `docs/docs-governance.md` を正本として参照する。
- secret-recovery の固定実装単位および作業項目進捗の正本は [tasks.md](tasks.md) とし、進捗変化が発生した都度更新する。
- [tasks.md](tasks.md) は固定実装単位の進捗追跡を扱い、機能の実装可否は各作業項目の `実装状態` を別途確認する。
- Phase/Issue レベルの完了判定は [implementation-guidelines.md](implementation-guidelines.md) の進捗状態判定規則を正本とし、`tasks.md` の作業項目 `状態: 完了` だけで判定しない。
