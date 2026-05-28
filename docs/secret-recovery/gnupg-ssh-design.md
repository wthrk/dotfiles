# GnuPG 復元 / gpg-agent SSH support 設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / GnuPG / SSH](./secret-recovery-spec.md#gnupg--ssh) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets restore-gpg` と `dotfiles gpg export-ssh-public-key` の実行境界、ならびに `restore-pass` で利用する gpg-agent SSH support 前提の確認である。

この文書は完成形の設計だけを扱う。

## 目的と保護境界

この機能の目的は、Bitwarden Secrets Manager から取得した `gpg-secret-key-backup` を安全に鍵リングへ復元し、GPG authentication subkey 由来の SSH 公開鍵を GitHub 登録可能な形式で出力できる状態を作ることである。

保護するもの:

- GPG secret key backup の import 入力と import 後の鍵識別子。
- authentication subkey を SSH identity として使うための gpg-agent SSH support 状態。
- 秘密鍵素材と fingerprint に紐づくセンシティブ情報の漏えい経路（ログ、引数、一時ファイル）。

保護しないもの:

- 利用者が GitHub SSH keys へ登録した後の外部サービス状態。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- `restore-gpg` は YubiKey から `bws-access-token` を取得し、Bitwarden Secrets Manager SDK で `gpg-secret-key-backup` を取得する。
- GPG 鍵リングの import API は `gpgme` に固定し、通常実装で `gpg` CLI は使わない。
- import 対象は 1 つの primary key と、encryption / authentication / signing subkey を含む OpenPGP transferable secret key であることを検証する。
- `export-ssh-public-key` は authentication subkey 由来の公開鍵を OpenSSH 形式で stdout に出力し、秘密鍵素材は出力しない。
- `restore-gpg` は import 成功後に gpg-agent SSH support 利用可否を確認し、利用不能なら停止する。
- gpg-agent SSH support 利用可否の確認では、SSH agent socket が有効であり、authentication subkey を参照できることを確認する。
- `~/.ssh/id_ed25519` の利用有無は判定条件に含めず、GPG authentication subkey 経由の経路だけを復旧対象にする。
- Home Manager で `gpg-agent.conf` を生成し、`enable-ssh-support` を含む SSH support 設定を恒久的に管理する。
- zsh 環境変数は `GPG_TTY` と `SSH_AUTH_SOCK` を必須とし、`SSH_AUTH_SOCK` は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合のみ上書きする。
- 既存 key の扱いは「停止」を正とし、同一 primary fingerprint の secret key が既に鍵リングにある場合は import 前に停止する（既存 key 上書きは本 issue では扱わない）。

## GPG import API 決定

`restore-gpg` の import API は次の呼び出し契約に固定する。

1. `gpgme` の OpenPGP context を生成する。
2. BWS から取得した `gpg-secret-key-backup` をメモリ上のバイト列として保持する。
3. `sequoia-openpgp` でバイト列をインメモリ解析し、import 前に primary fingerprint を導出する。比較に使う fingerprint は、後続の既存鍵照合と import 後再取得でそのまま使える canonical な 16 進文字列表現とする。
4. 同一 primary fingerprint の secret key が既存の鍵リングにある場合は停止する。
5. バイト列を `gpgme::Data` に変換し、`Context::import` で鍵リングへ投入する。
6. import result から対象 primary fingerprint を確認し、同一 fingerprint の key を再取得して subkey 検証へ渡す。

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

1. YubiKey から `bws-access-token` を取得する。
2. Bitwarden Secrets Manager から `gpg-secret-key-backup` を取得する。
3. import 前に、同一 primary fingerprint の secret key が既存の鍵リングにあるか確認し、存在する場合は停止する。
4. 取得値を import 入力として `gpgme` へ渡す。
5. import 後に対象鍵の subkey 構成（encryption / authentication / signing）を検証する。
6. authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録する（既登録の場合は冪等）。
7. gpg-agent SSH support が有効で、authentication subkey が SSH identity として利用可能であることを確認する。

復元処理は以下を満たす。

- backup 値は引数経由で外部プロセスへ渡さない。
- backup 値を永続一時ファイルへ書き出さない。
- import 対象鍵の同定は fingerprint を使い、表示時は必要最小限にとどめる。
- subkey 構成検証に失敗した場合は、後続の SSH 公開鍵出力処理へ進まない。

## 公開鍵出力契約

`export-ssh-public-key` は次の契約を満たす。

- 入力はローカル鍵リング上の GPG authentication subkey とする。
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

- `gpg-secret-key-backup` を取得し、import 前に primary fingerprint をインメモリ導出して既存の鍵リングの衝突を確認する。
- 衝突がなければ GPG secret key を import する。
- encryption / authentication / signing subkey の存在と利用可能状態（revoked / expired / disabled でないこと）を検証する。
- authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録し、既登録ならその状態を維持する（冪等）。
- gpg-agent SSH support 利用可否を確認する。

### `dotfiles gpg export-ssh-public-key`

- authentication subkey 由来の SSH 公開鍵を出力する。
- 出力は GitHub SSH keys 登録用途に限定する。

## 停止条件

- `bws-access-token` が取得できない。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` を取得できない。
- import 処理が失敗する。
- 同一 primary fingerprint の secret key が既に鍵リングへ存在する。
- import 後の鍵に encryption / authentication / signing subkey が揃っていない。
- subkey が revoked / expired / disabled で利用不能である。
- gpg-agent SSH support が利用できない。
- authentication subkey 由来の SSH 公開鍵を解決できない。
