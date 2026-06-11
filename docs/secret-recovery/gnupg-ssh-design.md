# GnuPG SSH Design

`restore-gpg` は個人 Bitwarden vault から encrypted envelope を取得し、接続中 YubiKey の recipient で DEK を unwrap して GPG secret key backup を復号する。

復号済み backup から primary fingerprint を導出し、envelope metadata と一致しない場合は停止する。既存の同一 primary secret key が鍵リングにある場合も import しない。

import 後は authentication subkey の keygrip を gpg-agent SSH key list へ登録し、SSH agent socket と authentication subkey 公開鍵の識別を確認する。失敗時は best-effort で import 済み key を削除して error chain を返す。

fingerprint、secret key material、DEK、vault secret、URL は log/error/report に出さない。
