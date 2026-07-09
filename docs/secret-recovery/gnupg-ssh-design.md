# GnuPG 復元 / gpg-agent SSH support 設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / GnuPG / SSH](./secret-recovery-spec.md#gnupg--ssh) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets restore-gpg` と `dotfiles gpg export-ssh-public-key` の実行境界、ならびに `restore-pass` で利用する gpg-agent SSH support 前提の確認である。

この文書は完成形の設計だけを扱う。

## 目的と保護境界

この機能の目的は、Bitwarden Secrets Manager から取得した `gpg-secret-key-backup` encrypted envelope を、接続中 YubiKey に一致する recipient で復号して安全に鍵リングへ復元し、GPG authentication subkey 由来の SSH 公開鍵を GitHub 登録可能な形式で出力できる状態を作ることである。

保護するもの:

- GPG secret key backup の import 入力と import 後の鍵識別子。
- authentication subkey を SSH identity として使うための gpg-agent SSH support 状態。
- 秘密鍵素材と fingerprint に紐づくセンシティブ情報の漏えい経路（ログ、引数、一時ファイル）。

保護しないもの:

- 利用者が GitHub SSH keys へ登録した後の外部サービス状態。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- `gpg-secret-key-backup` は YubiKey recipient 付き encrypted envelope で保持し、平文の ASCII-armored OpenPGP secret key block をそのまま保存しない。
- encrypted envelope は UTF-8 JSON で保存し、`version: 1` を固定する。top-level は `version` / `metadata` / `recipients` / `ciphertext` の 4 要素とする。
- `metadata` は `primary_fingerprint`（lowercase hex 40 文字、区切りなし）、`exported_at`（UTC RFC3339）、`dek_alg`（`aes-256-gcm` 固定）、`recipient_kek_alg`（`rsa-oaep-sha256` 固定）を必須とする。
- `ciphertext` は `nonce`、`body`、`tag` を base64 文字列で保持する。`nonce` は AES-GCM nonce 12 bytes、`body` は DEK で暗号化した OpenPGP backup bytes、`tag` は AES-GCM authentication tag 16 bytes とする。`tag` を `body` へ連結しない。
- `recipients` は 1 件以上必須とし、要素は `yubikey_serial`（10 進文字列）、`piv_slot`（string, `82` 固定）、`public_key_fingerprint`（PIV slot `82` 公開鍵の DER-encoded SubjectPublicKeyInfo を SHA-256 で digest した lowercase hex 64 文字、区切りなし）、`wrapped_dek`（base64）を必須とする。
- `restore-gpg` は YubiKey から `bitwarden-client-secret` を取得し、Bitwarden Secrets Manager SDK で `gpg-secret-key-backup` encrypted envelope を取得する。
- `restore-gpg` は接続中 YubiKey と envelope recipient の照合に成功した場合のみ data encryption key を unwrap し、復号した backup を import へ渡す。
- GPG 鍵リングの import API は `gpgme` に固定し、通常実装で `gpg` CLI は使わない。
- import 対象は 1 つの primary key と、encryption / authentication / signing subkey を含む OpenPGP transferable secret key であることを検証する。
- `export-ssh-public-key` は authentication subkey 由来の公開鍵を OpenSSH 形式で stdout に出力し、秘密鍵素材は出力しない。
- `restore-gpg` は import 成功後に gpg-agent SSH support 利用可否を確認し、利用不能なら停止する。
- gpg-agent SSH support 利用可否の確認では、SSH agent socket が有効であり、authentication subkey を参照できることを確認する。
- `~/.ssh/id_ed25519` の利用有無は判定条件に含めず、GPG authentication subkey 経由の経路だけを復旧対象にする。
- Home Manager で `gpg-agent.conf` を生成し、`enable-ssh-support` を含む SSH support 設定を恒久的に管理する。
- zsh 環境変数は `GPG_TTY` と `SSH_AUTH_SOCK` を必須とし、`SSH_AUTH_SOCK` は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合のみ上書きする。
- 既存 key の扱いは「停止」を正とし、同一 primary fingerprint の secret key が既に鍵リングにある場合は import 前に停止する（既存 key の上書きは対象外とする）。

## backup export 入力契約

`gpg-secret-key-backup` の envelope 作成入力は、既存環境のローカル鍵リングから得た OpenPGP transferable secret key bytes とする。export 対象は利用者が指定した primary fingerprint 1 件に限定し、export 前に encryption / authentication / signing subkey が揃っていることを検証する。

export は `gpgme` の in-memory export API を使う。通常実装で `gpg --export-secret-keys` などの外部 CLI は使わず、secret key material を argv、shell history、ログ、永続一時ファイルへ出さない。export 直後の bytes は `sequoia-openpgp` で再解析し、導出した primary fingerprint が指定値と一致する場合だけ envelope 化へ進む。

envelope 化では、export bytes をそのまま ASCII armor へ変換せず、AES-256-GCM の DEK で暗号化する。DEK は接続中 YubiKey の slot `82` 公開鍵 recipient ごとに RSA-OAEP-SHA256 で wrap し、`recipients` に保存する。DEK、復号済み backup、export bytes は保護境界内の一時値として扱い、外部 command や永続ファイルへ渡さない。

## recipient 運用 / BWS 更新契約

- primary 登録時は接続中 YubiKey の recipient を 1 件作成し、`recipients` 初期値として保存する。
- spare 追加時は既存 envelope を復号して同一 DEK を使い、spare recipient の `wrapped_dek` を `recipients` へ追加して同一 secret name を更新する（`ciphertext` は変更しない）。この更新も BWS secret の read-modify-write として扱い、更新前に読み出した revision / updatedAt / ETag 相当の更新識別子（取得不可の場合は exact UTF-8 secret value bytes の SHA-256 digest）を保持し、更新直前に再取得した現行値と一致する場合だけ上書きする。
- recipient 照合は `yubikey_serial` と `public_key_fingerprint` の両方一致を必須とし、片方のみ一致は不正扱いで停止する。
- recipient 追加を含む envelope 更新（spare 追加、reencrypt、置換更新）では、更新前に BWS から読み出した現行 secret の revision / updatedAt / ETag 相当の更新識別子を既知値として保持し、更新直前に再取得した現行 secret の更新識別子が一致することを確認する。SDK で更新識別子を取得できない場合は、最初に読み出した exact UTF-8 secret value bytes の SHA-256 digest を保持し、更新直前に再取得した exact value bytes の digest と一致する場合だけ上書きする。`version` と `metadata.primary_fingerprint` だけを stale overwrite 防止条件に使わない。
- BWS 更新は対話実行では project/secret 名と envelope `metadata.primary_fingerprint` を表示して明示確認後に実行し、非対話実行では明示的上書き許可 option がある場合だけ更新する。

## GPG import API 決定

`restore-gpg` の import API は次の呼び出し契約に固定する。

1. `gpgme` の OpenPGP context を生成する。
2. BWS から取得した `gpg-secret-key-backup` encrypted envelope をメモリ上で検証する（version / metadata / recipients / ciphertext）。
3. 接続中 YubiKey と一致する recipient で data encryption key を unwrap し、復号した backup バイト列を得る。
4. `sequoia-openpgp` で復号済みバイト列をインメモリ解析し、import 前に primary fingerprint を導出する。比較に使う fingerprint は、後続の既存鍵照合と import 後再取得でそのまま使える canonical な 16 進文字列表現とする。
5. 復号済み backup から導出した primary fingerprint が envelope `metadata.primary_fingerprint` と一致しない場合は停止する。
6. 同一 primary fingerprint の secret key が既存の鍵リングにある場合は停止する。
7. 復号済みバイト列を `gpgme::Data` に変換し、`Context::import` で鍵リングへ投入する。
8. import result から対象 primary fingerprint を確認し、同一 fingerprint の key を再取得して subkey 検証へ渡す。

envelope 形式検証または data encryption key の unwrap / backup 復号に失敗した場合は、import 処理へ進まず停止する。

この経路では secret key backup をプロセス引数や一時ファイルへ渡さない。`gpg --import` の外部プロセス起動は設計上の対象外とする。

## subkey 検証決定

subkey 検証は「存在する」だけではなく、利用可能状態を確認する。`restore-gpg` は次をすべて満たす場合のみ成功とする。

- primary key が 1 つである。
- secret key material を保持している。
- encryption / authentication / signing capability を持つ subkey がそれぞれ 1 つ以上ある。
- 上記 capability を満たす subkey が `revoked` / `expired` / `disabled` ではない。

検証対象は import 直後に解決した primary fingerprint に紐づく key に限定する。import した key と無関係な既存 key を混在判定しない。

## 鍵リング復元契約

`restore-gpg` は次の順序で処理する。

1. YubiKey から `bitwarden-client-secret` を取得する。
2. Bitwarden Secrets Manager から `gpg-secret-key-backup` encrypted envelope を取得する。
3. 取得値の envelope 形式（version / metadata / recipients / ciphertext）を検証し、接続中 YubiKey と一致する recipient がない場合は停止する。
4. 接続中 YubiKey で data encryption key を unwrap して復号済み backup を得る。
5. 復号済み backup から primary fingerprint を導出し、envelope `metadata.primary_fingerprint` と一致しない場合は停止する。
6. import 前に、同一 primary fingerprint の secret key が既存の鍵リングにあるか確認し、存在する場合は停止する。
7. 復号済み backup を import 入力として `gpgme` へ渡す。
8. import 後に対象鍵の subkey 構成（encryption / authentication / signing）を検証する。
9. authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録する（既登録の場合は冪等）。
10. gpg-agent SSH support が有効で、authentication subkey が SSH identity として利用可能であることを確認する。

復元処理は以下を満たす。

- backup 値は引数経由で外部プロセスへ渡さない。
- backup 値を永続一時ファイルへ書き出さない。
- import 対象鍵の同定は fingerprint を使い、表示時は必要最小限にとどめる。
- subkey 構成検証に失敗した場合は、後続の SSH 公開鍵出力処理へ進まない。

## 公開鍵出力契約

`export-ssh-public-key` は次の契約を満たす。

- 入力はローカル鍵リング上の GPG authentication subkey とする。
- primary key の指定は必須 option `--primary-fingerprint <40-hex-fingerprint>` で受け取り、その primary key に属する authentication subkey から SSH 公開鍵を導出する。
- 出力は OpenSSH 公開鍵 1 行のみとし、機械可読な形式で stdout へ書く。
- stdout が terminal である場合も公開鍵の出力は許可する（秘密情報ではないため）。
- GitHub API 呼び出しや鍵サーバー参照を内部で行わない。

## gpg-agent SSH support 境界

この設計でいう「gpg-agent SSH support 利用可」は、次の要件を同時に満たす状態とする。

- gpg-agent の SSH agent socket 参照先が解決できる。
- その socket を使う SSH agent 経路で authentication subkey が識別可能である。
- `restore-pass` が要求する `git2 + SSH agent` 経路へ引き渡せる前提が整っている。

`restore-gpg` はこの要件を満たさない場合に停止し、`restore-pass` へ進ませない。

### Home Manager `gpg-agent.conf` 決定

`gpg-agent` の SSH support は Home Manager 管理下で有効化する。`nix/modules/` 配下の Home Manager module で `gpg-agent.conf` を生成し、少なくとも次を設定する。

- `enable-ssh-support`
- `pinentry-program`（macOS で利用する pinentry 実体）

authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録する責務は `restore-gpg` が持つ。Home Manager 側は `gpg-agent.conf` の静的設定だけを管理し、鍵ごとの登録状態は管理しない。

利用者が手動で `~/.gnupg/gpg-agent.conf` を編集して状態を分岐させる運用は採用しない。

### zsh 環境変数決定

`config/zsh/env.zsh` で次を定義する。

- `GPG_TTY="$(tty)"`（TTY が取得できる対話シェル）
- `SSH_AUTH_SOCK` は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合のみ上書きし、未存在時は既存値を保持する。設定例:

  ```zsh
  _gpg_agent_sock="${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh"
  [[ -S "$_gpg_agent_sock" ]] && export SSH_AUTH_SOCK="$_gpg_agent_sock"
  unset _gpg_agent_sock
  ```

`restore-gpg` / `restore-pass` の実装側でも SSH agent socket 解決は crate 側ロジックで完結させ、`gpgconf` CLI 呼び出しは採用しない。crate 側は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として確認し、その path が socket でない場合のみ既存 `SSH_AUTH_SOCK` を維持する。

`restore-gpg` / `restore-pass` の実行前提はこの zsh 設定と Home Manager 側 `gpg-agent.conf` が一致していることとする。

## コマンド境界

### `dotfiles secrets restore-gpg`

- `gpg-secret-key-backup` encrypted envelope を取得し、`version` / `metadata` / `recipients` / `ciphertext` を検証する。
- 接続中 YubiKey と一致する recipient を解決し、data encryption key unwrap と backup 復号を完了してから import 前に primary fingerprint をインメモリ導出する。
- 導出した primary fingerprint が envelope `metadata.primary_fingerprint` と一致することを検証する。
- 同一 primary fingerprint の既存鍵リング衝突を確認し、衝突がなければ復号済み backup を GPG secret key として import する。
- encryption / authentication / signing subkey の存在と利用可能状態（revoked / expired / disabled でないこと）を検証する。
- authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録し、既登録ならその状態を維持する（冪等）。
- gpg-agent SSH support 利用可否を確認する。

### `dotfiles gpg export-ssh-public-key`

- `--primary-fingerprint <40-hex-fingerprint>` で指定した primary key の authentication subkey 由来の SSH 公開鍵を出力する。
- 出力は GitHub SSH keys 登録用途に限定する。

## 停止条件

- `bitwarden-client-secret` が取得できない。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` encrypted envelope を取得できない。
- backup envelope の形式検証（version / metadata / recipients / ciphertext）に失敗する。
- 接続中 YubiKey と一致する recipient が存在しない。
- data encryption key の unwrap または backup 復号に失敗する。
- 復号済み backup の primary fingerprint が envelope `metadata.primary_fingerprint` と一致しない。
- import 処理が失敗する。
- 同一 primary fingerprint の secret key が既に鍵リングへ存在する。
- import 後の鍵に encryption / authentication / signing subkey が揃っていない。
- subkey が revoked / expired / disabled で利用不能である。
- gpg-agent SSH support が利用できない。
- authentication subkey 由来の SSH 公開鍵を解決できない。
