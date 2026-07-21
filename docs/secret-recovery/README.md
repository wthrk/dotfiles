# secret-recovery

このディレクトリは、秘密情報復旧機能の仕様、設計、実装ガイドラインを配置する。

## 配下の項目

- [implementation-guidelines.md](implementation-guidelines.md#実装単位): secret-recovery 固有の固定実装単位、役割分担、実装方針を定義する。
- [secret-handling.md](secret-handling.md#secret-handling-policy): secret の保護境界、protection 内操作、外部処理境界、レビュー観点を定義する。
- [secret-recovery-spec.md](secret-recovery-spec.md): 秘密情報復旧機能の仕様を定義する。
- [initial-provisioning-runbook.md](initial-provisioning-runbook.md): 初期プロビジョニングと新規マシン復旧の実行手順を案内する。
- [bitwarden-personal-vault-design.md](bitwarden-personal-vault-design.md): Bitwarden Secrets Manager 取得経路の設計を定義する。
- [gnupg-ssh-design.md](gnupg-ssh-design.md): GPG 復元と gpg-agent SSH support 経路の設計を定義する。
- [yubikey-secret-storage-design.md](yubikey-secret-storage-design.md): YubiKey 保存方式の設計を定義する。
- [external-sdk-evidence.md](external-sdk-evidence.md): YubiKey PIV、Bitwarden Secrets Manager、GPG、Git の公式フロー、SDK API、sample、error handling の一次資料を対応づける。

## 一次資料

- [#11: 新規マシン秘密情報復旧基盤を実装する](https://github.com/wthrk/dotfiles/issues/11) と [正式 supersession comment](https://github.com/wthrk/dotfiles/issues/11#issuecomment-5037432015): 復旧の全体目的を示す issue と、旧本文の Password Manager login / OTP を含む復旧契約を BWS-only 契約へ正式に supersede した記録。
- [#12: YubiKey 秘密情報保存](https://github.com/wthrk/dotfiles/issues/12): YubiKey public-key fingerprint と slot 再生成検出の一次資料。
- [#14: GPG 復元 / gpg-agent SSH 対応](https://github.com/wthrk/dotfiles/issues/14): SPKI fingerprint による recipient 照合の一次資料。
- [#15: password-store 復元](https://github.com/wthrk/dotfiles/issues/15): GPG authentication subkey を SSH identity として private `password-store` を clone し `pass` を利用可能にする目的・完了条件。
- [#17: 新規マシン復旧フロー統合](https://github.com/wthrk/dotfiles/issues/17) と [正式 supersession comment](https://github.com/wthrk/dotfiles/issues/17#issuecomment-5037432321): 統合フローの work item と、旧本文の Password Manager login / OTP を含む `verify-yubikey --all` 契約を BWS-only 契約へ正式に supersede した記録。
- [#38: restore-pass 実装](https://github.com/wthrk/dotfiles/issues/38): `restore-pass` の BWS 取得、SSH agent 経由 clone、停止条件、検証の実装完了条件。
- [#40: password-store-remote の BWS provisioning](https://github.com/wthrk/dotfiles/issues/40): 復旧前提となる `password-store-remote` の BWS create/update 経路の目的・完了条件。
