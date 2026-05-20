# 新規マシン秘密情報復旧基盤

この文書は、新しい macOS マシンで `dotfiles` を導入したあと、開発に必要な秘密情報基盤を復旧する到達仕様を定義する恒久仕様文書である。ここでは完成形の仕様だけを定義する。対象は GnuPG secret key、GPG authentication subkey による GitHub SSH identity、private `password-store` repository、`pass` の利用環境、Bitwarden Password Manager の CLI login である。

復旧の入口には YubiKey を使う。YubiKey には Bitwarden master password と Bitwarden Secrets Manager access token を保存する。Bitwarden Secrets Manager には GPG secret key backup と `password-store` の remote URL を保存する。GPG secret key を復元したあと、GPG authentication subkey を SSH identity として使い、GitHub から private `password-store` repository を SSH clone する。

## 目的

- 新規マシンで秘密情報基盤を再構築する手順を `dotfiles` CLI に集約する。
- 復旧に必要な bootstrap secret を YubiKey と Bitwarden Secrets Manager に分離して保持する。
- GitHub API や 鍵サーバー に依存せず、GPG authentication subkey 由来の SSH identity で private repository を取得する。
- 平文 secret を CLI 引数、ログ、一時ファイル、永続環境変数に残さない。
- 破壊的な YubiKey reset や既存 認証情報 の削除を自動化しない。

## 復旧対象

- GnuPG secret key
- GPG encryption subkey による `pass` 復号環境
- GPG authentication subkey による GitHub SSH identity
- GPG signing subkey による Git signing 環境
- private `password-store` repository
- Bitwarden Password Manager の CLI login / unlock

## スペア YubiKey 運用

スペア YubiKey は YubiKey 本体を複製するものではない。各外部サービス には primary と spare をそれぞれ登録し、この repository 独自の bootstrap secret は primary と spare の両方に同じ値を保存する。

外部サービス の登録は `dotfiles` CLI では自動化しない。Yubico の一般方針どおり、primary と spare は同時期に用意し、サービスごとの security / 2FA / passkey 設定から両方を登録する。

対象ごとのスペア作成方法と、この repository での扱いは次のとおり。

- `FIDO2 / passkey / U2F`: GitHub、Bitwarden、Google、Apple など各 service で primary と spare を別々に登録する。手順として記録し、サービスアカウント への登録は自動化しない。
- `Yubico OTP`: OTP を要求する service で primary と spare を別々に登録する。Bitwarden CLI login の入力方法と検証手順は `dotfiles secrets bw-login` の仕様として定義し運用する。
- `OATH TOTP`: 同じ TOTP secret / QR code を primary と spare の両方に登録する。既存 secret を取り出せない場合は サービス側で TOTP を再設定する。TOTP secret はこの repository に保存しない。
- `bw-email`: primary と spare の両方に同じ Bitwarden login email を保存する。`dotfiles secrets yubikey enroll-primary` / `enroll-spare` で登録する。
- `bw-password`: primary と spare の両方に同じ Bitwarden master password を保存する。`dotfiles secrets yubikey enroll-primary` / `enroll-spare` で登録する。
- `bws-access-token`: primary と spare の両方に同じ Bitwarden Secrets Manager access token を保存し、rotate 時は全 YubiKey を更新する。`dotfiles secrets yubikey enroll-primary` / `enroll-spare` で登録し、rotate 時は `rotate-bws-token` で更新する。
- `GPG secret key`: YubiKey には載せず、Bitwarden Secrets Manager の backup から `restore-gpg` で復元する。
- `GitHub SSH identity`: YubiKey には載せず、復元した GPG authentication subkey 由来の SSH 公開鍵 を使う。`dotfiles gpg export-ssh-public-key` で出力する。
- `password-store`: YubiKey には載せず、GitHub から clone し、復元した GPG key で復号する。`restore-pass` で復元する。

primary YubiKey の紛失後に、primary だけに保存されていた bootstrap secret から spare を後付け作成することはできない。復旧可能性を維持するには、Bitwarden recovery code と各 service の recovery code を別経路で保管し、spare YubiKey を事前登録しておく。

## Secret の置き場所

保存場所ごとの secret と用途は次のとおり。

- `YubiKey`: `bw-email` を保存し、Bitwarden Password Manager の CLI login / unlock に使う。
- `YubiKey`: `bw-password` を保存し、Bitwarden Password Manager の CLI login / unlock に使う。
- `YubiKey`: `bws-access-token` を保存し、Bitwarden Secrets Manager から復旧情報を取得する。
- `Bitwarden Secrets Manager`: `gpg-secret-key-backup` を保存し、GPG secret key 復元に使う。
- `Bitwarden Secrets Manager`: `password-store-remote` を保存し、private `password-store` repository の clone URL として使う。
- `Bitwarden Password Manager`: Web service passwords、passkeys、TOTP、recovery codes を保存し、利用者向け password manager として使う。
- `pass` / `~/.password-store`: Bitwarden CLI API `client_id` / `client_secret` と UNIX 運用 secret を保存し、CLI やローカル運用に使う。
- `GitHub`: GPG authentication subkey 由来の SSH 公開鍵 を保持し、private repository clone に使う。

## 責務分担

### YubiKey

YubiKey は復旧入口の bootstrap secret を保持する。対象は `bw-email`、`bw-password`、`bws-access-token` の 3 種類だけである。

YubiKey 操作は Rust crate から行い、`ykman` CLI は使わない。PIV の reset や global state を破壊する操作は実装しない。書き込み対象はこの機能用に確保した領域だけに限定し、既存の FIDO2 / OTP / OpenPGP（公開鍵規格） / PIV 認証情報 を reset しない。既存領域と衝突する場合は停止する。
書き込みは management key 認証を前提にし、既定 management key のまま運用しない。既定 key のままでは想定外の上書きリスクを抑止できないため、専用領域を運用する前に非既定 management key への変更を必須にする。

詳細設計は [YubiKey 秘密情報保存設計](./yubikey-secret-storage-design.md) に置く。

### Bitwarden Secrets Manager

Bitwarden Secrets Manager は復旧に必要な機械向け secret を保持する。対象は `gpg-secret-key-backup` と `password-store-remote` である。

復旧本線では公式 `bitwarden` Rust SDK を使う。`bw` CLI は Bitwarden Secrets Manager からの取得には使わない。access token は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。

詳細設計は [Bitwarden Secrets Manager 復旧設計](./bitwarden-secrets-manager-design.md) に置く。

### Bitwarden Password Manager

Bitwarden Password Manager は Web service passwords、passkeys、TOTP、recovery codes を保持する。CLI 操作は login / unlock だけを対象にし、`bw` CLI の用途は `dotfiles secrets bw-login` に限定する。

Bitwarden login email と master password は YubiKey から取得し、master password は `BW_PASSWORD` として子プロセスにだけ渡す。`BW_PASSWORD` は保存しない。Bitwarden account 自体の 2FA / passkey には primary と spare の両方を事前登録する。`BW_SESSION` の扱いは `bw-login` のコマンド仕様に従う。

### GnuPG / SSH

GPG key は software key として運用する。GPG key material は YubiKey に入れない。GPG secret key backup は Bitwarden Secrets Manager に保存する。

`pass` には encryption subkey を使う。GitHub SSH identity には authentication subkey を使う。Git signing には signing subkey を使う。GitHub private repository の取得は GPG authentication subkey による SSH clone で行い、GitHub API は使わない。鍵サーバー は使わない。既存の `~/.ssh/id_ed25519` は新規運用では使わない。

GPG 鍵リング 操作は `gpgme` を使う。OpenPGP（公開鍵規格） 公開鍵 操作が必要な場合は `sequoia-openpgp` を使う。`gpg` CLI は通常実装では使わない。

### Git

private `password-store` repository の clone は `git2` と SSH agent を使う。`git` CLI は復旧本線では使わない。SSH agent には gpg-agent の SSH support を使い、GPG authentication subkey 由来の identity を GitHub に提示する。

## 到達仕様の復旧フロー

1. `dotfiles secrets yubikey enroll-primary` で primary YubiKey に必要な bootstrap secret を登録し、ローカル確認 まで実行する。
2. スペア YubiKey がある場合は、`dotfiles secrets yubikey enroll-spare` で primary から bootstrap secret を読み出し、spare に再暗号化して保存し、ローカル確認 まで実行する。
3. Bitwarden、GitHub、Google、Apple など YubiKey を使う外部サービス に primary と spare を登録する。
4. `dotfiles secrets verify-yubikey` で、挿さっている YubiKey に必要な bootstrap secret があることを確認する。BWS と Bitwarden Password Manager の外部確認は、`--check bws`、`--check bw-login`、`--all` を使って実行する。
5. `bws-access-token` を rotate した場合は `dotfiles secrets yubikey rotate-bws-token` で primary とすべての spare を更新する。対話実行では利用者が対象 YubiKey を選び、要約 の serial で対象全本の更新完了を確認する。非対話実行では `--serial` と `--stdin` を指定して 1 本ずつ更新する。
6. `dotfiles secrets restore-gpg` で Bitwarden Secrets Manager から GPG secret key backup を取得し、GPG secret key を import する。
7. `dotfiles gpg export-ssh-public-key` で GPG authentication subkey 由来の SSH 公開鍵 を出力し、GitHub SSH keys に登録する。
8. `dotfiles secrets restore-pass` で Bitwarden Secrets Manager から `password-store-remote` を取得し、GPG authentication subkey 経由の SSH で private `password-store` repository を clone する。
9. `dotfiles secrets bw-login` で Bitwarden Password Manager に login / unlock する。

## 到達仕様のコマンド一覧

以下のコマンド節は、最終到達状態で備える公開インターフェースを定義する。現行の実装状況や即時利用可否はこの節では扱わない。

### `dotfiles secrets yubikey setup`

到達仕様では、YubiKey 5 PIV の利用前提を確認し、この機能で使う保存領域を確認する手段として提供する。既存の FIDO2 / OTP / OpenPGP（公開鍵規格） / PIV 認証情報 は reset しない。既存領域と衝突する場合は停止する。通常の利用者向け導線では `enroll-primary` / `enroll-spare` から内部的に扱う。

### `dotfiles secrets yubikey put <name>`

YubiKey に secret を保存する。`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。secret 本文は hidden prompt または stdin から受け取る。平文を CLI 引数、ログ、一時ファイルに残さない。同名 secret の上書きには明示 option を必要とする。通常の primary / spare 登録では直接使わず、`enroll-primary` / `enroll-spare` を使う。
このコマンドは入力前に manifest と既存 object の状態を検証し、`--force` なしで上書きが必要な場合は secret を読まずに停止する。

### `dotfiles secrets yubikey get <name>`

YubiKey から指定 secret を取得する。`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。YubiKey PIV PIN を要求し、stdout に secret を出力する。stdout が terminal の場合は平文が画面や scrollback に残るため拒否し、pipe または redirect 先が明示された実行だけを許可する。通常は他の `dotfiles secrets` コマンド内部で使う。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として初期登録する。接続中の YubiKey を選択し、専用 PIV 領域を setup し、`bw-email`、`bw-password`、`bws-access-token` を prompt から受け取り、保存後に ローカル確認 を実行する。非対話または移行用途に限り stdin からの入力を許可する。
`--stdin-json` で bootstrap secret を渡す場合も、PIN は JSON 用 stdin ではなく controlling terminal から読む。PIN 検証 と保存先の事前確認に失敗した場合は、JSON payload を読み始める前に停止する。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として初期登録する。通常は primary YubiKey から `bw-email`、`bw-password`、`bws-access-token` を読み出し、spare YubiKey の 公開鍵 で再暗号化して保存する。利用者に bootstrap secret の再入力を要求しない。外部サービス の FIDO2 / passkey / U2F / OTP 登録は自動化しない。
`--stdin-json` で bootstrap secret を渡す場合も、PIN は JSON 用 stdin ではなく controlling terminal から読む。PIN 検証 と保存先の事前確認に失敗した場合は、JSON payload を読み始める前に停止する。

### `dotfiles secrets yubikey rotate-bws-token`

指定 YubiKey の `bws-access-token` を更新し、更新後に ローカル確認 を実行する。BWS 接続確認は ローカル保管 の検証とは別の外部確認として扱う。primary と spare を複数本運用する場合は、新しい token を一度だけ読み取り、利用者が更新対象の YubiKey を選ぶ。出力 要約 の serial を確認し、対象全本が更新済みになるまで選択を続ける。非対話実行では `--serial` で 1 本だけを更新し、token は `--stdin` で渡せる。
token 入力前に ローカル保管 の復号可能性を確認し、更新不能な状態では新しい token を読まずに停止する。
`--stdin` で token を渡す場合も、PIN は token 用 stdin ではなく controlling terminal から読む。

### `dotfiles secrets verify-yubikey`

到達仕様では、挿さっている YubiKey が復旧入口として使えるか確認する機能として提供する。1 本だけ接続されている場合はその YubiKey を対象にし、複数本接続されている場合は serial と識別情報を表示して選択させる。非対話実行では `--serial <serial>` で対象を明示する。secret 本文は stdout / stderr に出力しない。

到達仕様の確認項目:

- `bw-email`、`bw-password`、`bws-access-token` が YubiKey に保存され、PIN 検証 と touch を経て復号できる。

このコマンドは ローカル保管 確認だけを実行する。`--check bws`、`--check bw-login`、`--all` は外部確認を要求する option なので、利用できない場合は明示的に失敗する。引数なし実行の 要約 では外部確認項目を機械可読状態値 `skipped` として残す。要約の状態値は `ok` と `skipped` を使い、表示文言は別層で扱う。

このコマンドは GitHub、Google、Apple など外部サービス の FIDO2 / passkey / U2F 登録状況を検証しない。外部サービス の spare key 登録は各サービスの設定画面で確認する。

### `dotfiles secrets restore-gpg`

YubiKey から `bws-access-token` を取得し、Bitwarden Secrets Manager SDK で `gpg-secret-key-backup` を取得する。GPG secret key を import し、encryption / authentication / signing subkey の存在を検証する。最後に `gpg-agent` SSH support が使えることを確認する。

### `dotfiles secrets restore-pass`

YubiKey から `bws-access-token` を取得し、Bitwarden Secrets Manager SDK で `password-store-remote` を取得する。`~/.password-store` が存在しないことを確認し、GPG authentication subkey 経由の SSH で private repository を clone する。clone 後に `pass` が store を読めることを確認する。

### `dotfiles secrets bw-login`

YubiKey から `bw-email` と `bw-password` を取得し、YubiKey OTP を入力させる。`bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` を実行し、続けて `bw unlock --passwordenv BW_PASSWORD --raw` を実行する。`BW_PASSWORD` は保存しない。`BW_SESSION` の扱いはこのコマンド仕様で定義する。通常は YubiKey 内の `bw-email` を使い、override が必要な場合だけ `--email <email>` を許可する。

### `dotfiles gpg export-ssh-public-key`

GPG authentication subkey 由来の SSH 公開鍵 を stdout に出力する。GitHub SSH keys に登録するために使う。GitHub API は呼ばない。

## API / Command Policy

領域ごとの API / command policy は次のとおり。

- `YubiKey`: Rust crate を使い、`ykman` CLI、PIV reset、既存 認証情報 削除は使わない。
- `Bitwarden Secrets Manager`: 公式 `bitwarden` Rust SDK を使い、復旧本線で `bw` CLI は使わない。
- `Bitwarden Password Manager`: login / unlock 用の `bw` CLI を使い、secret 取得や永続保存用途で `bw` CLI は使わない。
- `GnuPG`: `gpgme`、必要時の `sequoia-openpgp` を使い、通常実装で `gpg` CLI と鍵サーバーは使わない。
- `Git`: `git2` と SSH agent を使い、復旧本線で `git` CLI と GitHub API は使わない。

## 停止条件

- YubiKey の専用保存領域が利用できない、または既存 認証情報 と衝突する。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、明示的な上書き option が指定されていない。
- Bitwarden Secrets Manager から必要な secret が取得できない。
- `verify-yubikey` で YubiKey 内の bootstrap secret 確認に失敗する。
- `verify-yubikey --check bws`、`verify-yubikey --check bw-login`、`verify-yubikey --all` のいずれかで、Bitwarden Secrets Manager または Bitwarden Password Manager への到達確認に失敗する。
- `enroll-primary --stdin-json`、`enroll-spare --stdin-json`、`rotate-bws-token --stdin` で PIN 入力に必要な controlling terminal を開けない。
- `rotate-bws-token` の同一実行内で同一 serial を重複更新しようとした。
- import 対象の GPG secret key に encryption / authentication / signing subkey が揃っていない。
- `gpg-agent` SSH support が利用できない。
- `~/.password-store` が既に存在する。
- `password-store-remote` が private repository の clone URL として妥当でない。
- Bitwarden CLI login / unlock に必要な `bw` CLI、OTP、認証情報が揃っていない。

## 参考

- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
