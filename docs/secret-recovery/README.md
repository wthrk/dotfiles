# secret-recovery

このディレクトリは、秘密情報復旧機能の仕様、設計、実装ガイドラインを配置する。

## 配下の項目

- [implementation-guidelines.md](implementation-guidelines.md#計画依頼の固定実装単位): secret-recovery 固有の固定実装単位、役割分担、実装方針を定義する。
- [secret-handling.md](secret-handling.md#secret-handling-policy): secret の保護境界、protection 内操作、外部処理境界、レビュー観点を定義する。
- [secret-recovery-spec.md](secret-recovery-spec.md): 秘密情報復旧機能の仕様を定義する。
- [bitwarden-secrets-manager-design.md](bitwarden-secrets-manager-design.md): Bitwarden Secrets Manager 取得経路の設計を定義する。
- [gnupg-ssh-design.md](gnupg-ssh-design.md): GPG 復元と gpg-agent SSH support 経路の設計を定義する。
- [yubikey-secret-storage-design.md](yubikey-secret-storage-design.md): YubiKey 保存方式の設計を定義する。

## タスク文書

- [../tasks/secret-recovery/README.md](../tasks/secret-recovery/README.md): secret-recovery のタスク台帳、作業定義、粗粒度進捗、証跡を案内する。
