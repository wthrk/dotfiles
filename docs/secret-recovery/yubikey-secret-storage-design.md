# YubiKey 秘密情報保存設計

この文書は、#12「YubiKey 秘密情報保存」の design PR で確定する仕様を定義する。対象は `bw-email`、`bw-password`、`bws-access-token` を YubiKey に保存し、復旧コマンドから安全に取得するための `dotfiles secrets yubikey` サブコマンドである。

## 決定事項

- PIV 操作には Rust crate `yubikey` を使う。
- 平文 secret は PIV data object に保存しない。
- YubiKey 上に専用の PIV 鍵を生成し、secret はローカルで envelope encryption した blob として custom PIV data object に保存する。
- 専用 PIV 鍵は retired key management slot `82` を使う。
- data object は Yubico が application 用に確保している custom data object range から `0x005FFF00` から `0x005FFF03` までを使う。
- スペア YubiKey は同じ PIV 秘密鍵を複製せず、各 YubiKey で専用鍵を生成して同じ secret を個別に保存する。
- 通常の primary / spare 登録には `enroll-primary` / `enroll-spare` を使い、低水準の `setup` / `put` / `get` を直接並べる手順にしない。
- `dotfiles secrets verify-yubikey` で、挿さっている YubiKey が bootstrap secret を復号できることを確認する。
- `dotfiles secrets yubikey setup` は既存の PIV credential や data object と衝突した場合に停止する。
- `put` は同名 secret が存在する場合、`--force` が指定されていなければ停止する。
- `get` は復旧コマンド内部の利用を主用途とし、直接実行時は stdout に secret 本文だけを出力する。

## 採用 crate

`yubikey` crate を採用する。この crate は PC/SC 経由で YubiKey PIV を操作し、PIV 鍵、PIN verification、object read/write の API を提供する。Yubico 公式 Rust SDK ではないため、実装では `dotfiles-core` 側に薄い adapter を置き、CLI 層から crate 型を直接公開しない。

実装 PR では次を満たす。

- `yubikey` の version を明示的に固定する。
- object read/write API が feature gate を要求する場合、その feature は YubiKey adapter module だけで使う。
- reset、PIN/PUK 変更、management key 変更、既存 key 削除の API は adapter から公開しない。
- hardware なしの unit test は fake adapter で行う。
- 実機検証は read-only 確認と、この機能用 object / slot への opt-in 書き込みに限定する。

## PIV 領域

### Slot

専用 PIV 鍵には retired key management slot `82` を使う。標準用途の `9A`、`9C`、`9D`、`9E` は使わない。`82` に既存 key または certificate がある場合、`setup` は停止する。

鍵は YubiKey 上で生成する。秘密鍵 material は export しない。鍵種別は `RSA2048` とし、content encryption key の wrap / unwrap にだけ使う。PIV の RSA decrypt operation は raw RSA として扱い、OAEP padding は host 側で処理する。OAEP の hash と MGF1 hash は SHA-256 に固定する。implementation PR では `yubikey` crate の PIV decrypt API が raw decrypt bytes を返す前提で OAEP unpad を実装し、対応できない場合は design PR に戻して方式を見直す。

PIN policy は `Once`、touch policy は `Always` とする。1 コマンド内では PIN verification を 1 回に抑え、secret 復号操作ごとに YubiKey touch を要求する。例えば `enroll-spare` は primary 側の 3 secret 読み出しで 3 回、spare 側の local verify で 3 回の touch が発生する。連続した復旧コマンドでも touch を省略しない。

### Object IDs

| Object ID | 用途 |
| --- | --- |
| `0x005FFF00` | dotfiles secret storage manifest |
| `0x005FFF01` | `bw-email` encrypted blob |
| `0x005FFF02` | `bw-password` encrypted blob |
| `0x005FFF03` | `bws-access-token` encrypted blob |

PIV data object は app 独自データを置けるが、読み出し自体は secret 保護境界にしない。そのため data object に置くのは暗号化済み blob と manifest だけにする。

## スペア YubiKey

この文書で扱うスペア YubiKey は、`dotfiles` 独自の bootstrap secret storage に限る。Bitwarden、GitHub、Google、Apple など外部サービスの FIDO2 / passkey / U2F / OTP 登録は各サービス側で primary と spare を別々に登録する。OATH TOTP は同じ TOTP secret / QR code を primary と spare の両方に登録する。

スペア YubiKey は事前登録を必須にする。primary YubiKey の紛失後に、primary だけに保存されていた `bw-email`、`bw-password`、`bws-access-token` からスペアを後付け作成することはできない。

同じ PIV 秘密鍵を複製して複数 YubiKey に入れる運用は採用しない。各 YubiKey で slot `82` に別々の non-exportable key を生成し、同じ secret をその YubiKey の public key で個別に wrap して保存する。

スペア作成手順は次の 1 コマンドにまとめる。

```sh
dotfiles secrets yubikey enroll-spare
```

`enroll-spare` は次を一連の処理として実行する。

1. primary YubiKey を選択し、PIN verification と touch を経て `bw-email`、`bw-password`、`bws-access-token` を復号する。
2. spare YubiKey を選択する。primary と spare を同時接続できない場合は、primary 読み出し後に spare へ差し替えさせる。
3. spare の専用 PIV slot / object が未使用であることを確認し、必要なら setup を行う。
4. primary から読み出した secret を、spare 用の新しい content encryption key と nonce で再暗号化し、spare の public key で key wrap して保存する。
5. local verify を実行し、spare 単体で 3 種類の secret を復号できることを確認する。

secret はプロセスメモリ上の zeroize 可能な buffer にだけ保持し、CLI 引数、ログ、一時ファイル、環境変数には残さない。通常の `enroll-spare` は利用者に `bw-email`、`bw-password`、`bws-access-token` の再入力を要求しない。

spare に保存する blob は primary の ciphertext や wrapped key を流用しない。AEAD additional data には spare の serial と保存先 object ID を使い、primary 由来の serial が spare 側の blob に残らないようにする。

primary 読み出し後に spare へ差し替える間も、平文 secret は zeroize 可能な memory guard の内側だけに置く。正常終了、error、timeout、Ctrl-C などの interrupt path では必ず zeroize する。panic message、debug 表示、error context には secret 本文を含めない。`enroll-spare` は平文 secret を保持している間、core dump を無効化し、可能な環境では `mlock` 相当で buffer を lock する。memory lock に失敗した場合は、secret を読み出す前に停止する。

YubiKey の選択は対話を基本にする。1 本だけ接続されている場合はその YubiKey を対象にする。複数本接続されている場合は serial と識別情報を表示して選択させる。非対話実行では `--primary-serial <serial>` と `--spare-serial <serial>` で対象を明示する。

primary の初期登録も同じ考え方にし、通常は次のコマンドだけを使う。

```sh
dotfiles secrets yubikey enroll-primary
```

`bws-access-token` を rotate した場合は、primary とすべての spare に対して次を実行する。

```sh
dotfiles secrets yubikey rotate-bws-token
```

`rotate-bws-token` は新しい token を一度だけ受け取り、対象 YubiKey を対話的に選択させながら primary と spare を更新する。各 YubiKey への保存後に local verify と `verify-yubikey --check bws` 相当の接続確認を行う。非対話実行では `--serial <serial>` を指定して 1 本ずつ更新する。

外部サービスの登録状況は YubiKey PIV object からは検証できないため、`setup` / `put` / `get` の成功は GitHub、Bitwarden、Google、Apple などで spare key が登録済みであることを保証しない。

## 保存形式

Manifest は JSON とし、UTF-8 bytes を PIV data object に保存する。

```json
{
  "version": 1,
  "app": "dotfiles.secret-recovery",
  "key_slot": "82",
  "objects": {
    "bw-email": "0x005FFF01",
    "bw-password": "0x005FFF02",
    "bws-access-token": "0x005FFF03"
  }
}
```

Secret blob は binary format とする。先頭に ASCII magic と version を置き、以降は structured binary として parse する。

```text
DOTFILES-YK-SECRET\0
version: u8 = 1
secret_id: u8
algorithm: u8 = 1
nonce: [u8; 12]
wrapped_key_len: u16
wrapped_key: bytes
ciphertext_len: u32
ciphertext: bytes
tag: [u8; 16]
```

Envelope encryption は次の役割分担にする。

- `algorithm = 1` は AES-256-GCM を表す。
- `secret_id` は `1 = bw-email`、`2 = bw-password`、`3 = bws-access-token` を表す。
- secret 本文はランダムな 32-byte content encryption key で AEAD 暗号化する。
- AES-256-GCM の nonce は 12 bytes、tag は 16 bytes に固定する。format 互換性を単純に保つため、nonce / tag の可変長 field は持たない。
- content encryption key は slot `82` の RSA public key で wrap する。
- `get` は PIV private key operation で content encryption key を unwrap し、AEAD で secret 本文を復号する。
- AEAD additional data には `version`、`secret_id`、object ID、YubiKey serial を含め、blob の入れ替えを検出する。

平文 secret は `String` ではなく zeroize 可能な byte buffer として扱う。ログ、error context、debug 表示に secret 本文や復号済み buffer を含めない。

## コマンド仕様

### `dotfiles secrets yubikey setup`

`setup` は低水準コマンドであり、通常は `enroll-primary` / `enroll-spare` から内部的に実行する。直接実行時は次を確認する。

- YubiKey が 1 本だけ接続されていればそれを対象にする。複数本ある場合は serial と識別情報を表示して選択させる。非対話実行では `--serial <serial>` を要求する。
- PIV application version が利用条件を満たすこと。
- slot `82` に既存 key / certificate がないこと。
- `0x005FFF00`、`0x005FFF01`、`0x005FFF02`、`0x005FFF03` に既存 data object がないこと。
- PIN retries が 0 ではないこと。
- management key authentication が可能なこと。

確認後、slot `82` に専用鍵を生成し、manifest を保存する。既存の FIDO2 / OTP / OpenPGP / PIV credential は reset しない。衝突がある場合に自動削除や上書きはしない。

複数本の YubiKey を運用する場合でも、`setup` は指定された 1 本だけを変更する。接続中の他 YubiKey へ同時に書き込む batch mode は実装しない。

### `dotfiles secrets yubikey put <name>`

`put` は低水準コマンドであり、通常の primary / spare 登録では `enroll-primary` / `enroll-spare` を使う。`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。それ以外は CLI parsing 後の validation で拒否する。

secret 入力は次の順で受け付ける。

- default: hidden prompt
- `--stdin`: stdin から 1 secret を読む

CLI 引数で secret 本文は受け取らない。stdin 入力時も trailing newline は 1 つだけ除去し、それ以外の bytes は保持する。

保存先 object に既存 blob がある場合は `--force` がない限り停止する。`--force` がある場合も、manifest の app / version / slot が一致しない場合は停止する。

### `dotfiles secrets yubikey get <name>`

`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。PIN verification と touch を経て secret を復号し、stdout に secret 本文だけを出力する。stderr には進行状況を出さない。取得失敗時の error には secret name までを含め、secret 本文、ciphertext、wrapped key は含めない。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として登録する高水準コマンドである。これは bootstrap secret の正本を最初に登録する操作なので、`bw-email`、`bw-password`、`bws-access-token` を prompt から受け取る。`bw-email` は通常表示 prompt、`bw-password` と `bws-access-token` は hidden prompt にする。非対話または migration 用に限り `--stdin-json` を許可する。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として登録する高水準コマンドである。primary から bootstrap secret を読み出して spare に再暗号化する操作にまとめ、利用者が低水準コマンドを手順として並べたり、secret を再入力したりしなくてよいようにする。

`--stdin-json` は primary YubiKey が利用できないが、別経路で正本 secret を持っている場合の recovery / migration 用に限る。この場合だけ次の JSON を stdin から 1 回だけ受け取る。

```json
{
  "bw-email": "user@example.com",
  "bw-password": "secret",
  "bws-access-token": "secret"
}
```

入力 bytes をログや一時ファイルへ残さない。JSON parse 後の secret は zeroize 可能な buffer として扱う。

`enroll-primary` / `enroll-spare` は成功時に secret 本文を出さず、次の summary だけを出力する。`role` は `primary` または `spare` のいずれかで、複数 spare の識別には YubiKey serial を使う。

```json
{
  "serial": 12345678,
  "role": "primary",
  "checks": {
    "setup": "ok",
    "bw_email": "ok",
    "bw_password": "ok",
    "bws_access_token": "ok",
    "local_storage": "ok"
  }
}
```

```json
{
  "serial": 87654321,
  "role": "spare",
  "checks": {
    "setup": "ok",
    "bw_email": "ok",
    "bw_password": "ok",
    "bws_access_token": "ok",
    "local_storage": "ok"
  }
}
```

### `dotfiles secrets yubikey rotate-bws-token`

指定 YubiKey の `bws-access-token` だけを更新する。`--force` は不要にし、このコマンド自体が rotate intent を表す。更新後は local verify と BWS 接続確認を実行する。BWS client API が未実装の段階では local verify までを実装し、#13 で BWS 接続確認を接続する。

### `dotfiles secrets verify-yubikey`

挿さっている YubiKey が復旧入口として使えるか確認する。#12 の implementation PR では YubiKey local storage の確認までを実装し、Bitwarden Secrets Manager への接続確認は #13、Bitwarden Password Manager login / unlock 確認は #16、統合された end-to-end check は #17 で完成させる。

引数:

- `--serial <serial>`: 非対話実行時に対象 YubiKey を指定する。対話実行では、1 本だけ接続されていれば自動選択し、複数本接続時は一覧から選択させる。
- `--check bws`: `bws-access-token` で Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` を取得できることを確認する。
- `--check bw-login`: `bw-email`、`bw-password`、入力された YubiKey OTP で Bitwarden Password Manager の login / unlock ができることを確認する。override が必要な場合だけ `--email <email>` を許可する。
- `--all`: local storage、BWS、Bitwarden login の全確認を行う。通常は YubiKey 内の `bw-email` を使う。

出力は machine-readable な summary にし、secret 本文、access token、Bitwarden session token は出力しない。

```json
{
  "serial": 12345678,
  "checks": {
    "local_storage": "ok",
    "bws": "ok",
    "bw_login": "skipped"
  }
}
```

local storage check は次を確認する。

- manifest が存在し、app、version、slot、object mapping が期待値と一致する。
- `bw-email`、`bw-password`、`bws-access-token` の blob が存在する。
- PIN verification と touch を経て 3 種類の secret を復号できる。
- 復号した secret は空ではない。

このコマンドは GitHub、Google、Apple など外部サービスの FIDO2 / passkey / U2F 登録状況を検証しない。

## 停止条件

- YubiKey が見つからない。
- 非対話実行で複数 YubiKey が接続され、`--serial` または用途別 serial option が指定されていない。
- 指定 serial の YubiKey が見つからない。
- PIV application が利用できない。
- PIN retries が 0。
- management key authentication に失敗する。
- slot `82` に既存 key または certificate がある。
- 使用予定 object ID に既存 data object がある。
- `enroll-primary` / `enroll-spare` の途中で setup、保存、local verify のいずれかに失敗した。
- `enroll-spare` で primary と spare の serial が同一である。
- `enroll-spare` の差し替え待ちで spare YubiKey が検出できない、または timeout した。
- `enroll-spare` で平文 secret を保持する前に core dump 無効化または memory lock の準備に失敗した。
- `rotate-bws-token` 後の local verify または BWS 接続確認に失敗した。
- manifest が存在するが app、version、slot、object mapping が期待値と一致しない。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、`--force` が指定されていない。
- secret 入力が空。
- secret blob の magic、version、secret id、AEAD additional data が一致しない。
- 復号または認証 tag 検証に失敗した。
- `verify-yubikey` で必須 check が失敗した。

## テスト方針

Unit test は fake YubiKey adapter で行う。

- 許可 name と拒否 name。
- 対話実行で複数 YubiKey がある場合に一覧から選択できること。
- 非対話実行で複数 YubiKey がある場合に serial option なしで停止すること。
- manifest parse / serialize。
- object ID mapping。
- `put` の既存 blob 検出と `--force`。
- `enroll-primary` が setup、3 secret 保存、local verify を順に実行すること。
- `enroll-spare` が primary 読み出し、spare setup、spare への再暗号化保存、local verify を順に実行すること。
- `enroll-spare` が primary / spare 同一 serial と spare 待ち timeout を拒否すること。
- `enroll-spare` の error / interrupt path で平文 buffer が zeroize されること。
- `rotate-bws-token` が `bws-access-token` だけを更新し、`bw-email` と `bw-password` を変更しないこと。
- `verify-yubikey` local storage check の正常系と missing manifest / missing blob / decrypt failure。
- empty secret の拒否。
- blob magic / version / secret id mismatch の拒否。
- adapter error から利用者向け error context への変換。
- secret 本文が error 表示に含まれないこと。

実機 validation は implementation PR で手順を明記する。対象は専用 slot / object が空の検証用 YubiKey に限定し、reset、credential 削除、既存領域上書きは行わない。

## 参考

- Yubico PIV slots: https://docs.yubico.com/yesdk/users-manual/application-piv/slots.html
- Yubico PIV data objects: https://docs.yubico.com/yesdk/users-manual/application-piv/piv-objects.html
- Yubico PIV tool object/slot reference: https://docs.yubico.com/software/yubikey/tools/pivtool/piv-tool-command.html
- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
- `yubikey` crate docs: https://docs.rs/yubikey/latest/yubikey/
