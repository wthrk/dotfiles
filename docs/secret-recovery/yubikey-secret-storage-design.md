# YubiKey 秘密情報保存設計

この文書は、#12「YubiKey 秘密情報保存」の design PR で確定する仕様を定義する。対象は `bw-email`、`bw-password`、`bws-access-token` を YubiKey に保存し、復旧コマンドから安全に取得するための `dotfiles secrets yubikey` サブコマンドである。

## 目的と保護境界

この機能の目的は、新規マシン復旧に必要な bootstrap secret を、YubiKey がなければ復号できない形で保存することである。PIV data object は読み出し自体を secret 保護境界にしない。今回使う custom data object は PIN なしで読めるものとして扱い、そこには平文 secret も平文 content encryption key も置かない。

保護するもの:

- YubiKey PIV data object から読み出された encrypted blob。
- blob の backup、copy、log、diagnostic dump。
- PIN verification と touch を通せない状態での `wrapped_key`。

保護しないもの:

- 復号を許可した実行中 host の memory。
- 復号後に stdout や外部 command に渡された secret。
- YubiKey、PIN、touch 操作を攻撃者が同時に利用できる状況。

この境界のため、保存形式は envelope encryption にする。secret 本文はランダムな content encryption key で AES-256-GCM 暗号化し、その content encryption key は YubiKey 内の non-exportable PIV private key に対応する public key で wrap する。永続保存される blob は `nonce`、`wrapped_key`、`ciphertext`、`tag` だけであり、復号には YubiKey の private key operation が必要になる。

## 決定事項

- PIV 操作には Rust crate `yubikey` を使う。
- bootstrap secret 本文は `secrecy` の secret wrapper で保持し、明示的な expose なしに参照できないようにする。
- 平文 secret は PIV data object に保存しない。
- 平文 content encryption key も PIV data object に保存しない。
- YubiKey 上に専用の PIV 鍵を生成し、secret はローカルで envelope encryption した blob として custom PIV data object に保存する。
- 専用 PIV 鍵は retired key management slot `82` を使う。
- data object は YubiKey が undefined DataTag として受け付ける範囲から `0x005FFF16` から `0x005FFF19` までを使う。
- manifest は format sentinel としてだけ使う。slot や object ID の解釈を manifest で動的に変えない。
- スペア YubiKey は同じ PIV 秘密鍵を複製せず、各 YubiKey で専用鍵を生成して同じ secret を個別に保存する。
- 通常の primary / spare 登録には `enroll-primary` / `enroll-spare` を使い、低水準の `setup` / `put` / `get` を直接並べる手順にしない。
- `dotfiles secrets verify-yubikey` で、挿さっている YubiKey が bootstrap secret を復号できることを確認する。
- `dotfiles secrets yubikey setup` は既存の PIV credential や data object と衝突した場合に停止する。
- `put` は同名 secret が存在する場合、`--force` が指定されていなければ停止する。
- `get` は復旧コマンド内部の利用を主用途とし、直接実行時は stdout に secret 本文だけを出力する。

## 採用 crate

`yubikey` crate を採用する。この crate は PC/SC 経由で YubiKey PIV を操作し、PIV 鍵、PIN verification、object read/write の API を提供する。Yubico 公式 Rust SDK ではないため、実装では CLI 側の adapter に crate 型を閉じ込め、storage logic から直接公開しない。

secret memory handling は役割ごとに crate を分ける。

- `secrecy`: bootstrap secret 本文の型。`ExposeSecret` 経由でだけ中身へ触り、Debug 表示や不用意な複製を避け、drop 時に zeroize する。
- `zeroize`: encrypted blob、wrapped key、PIV operation の一時 buffer など、`secrecy` の外側に残る byte buffer の zeroize。
- `rlimit`: `enroll-spare` で secret を読む前に core dump を無効化する。
- `region`: `enroll-spare` で primary から読んだ 3 secret、または `--stdin-json` 由来の 3 secret を memory lock する。

YubiKey adapter は次を満たす。

- `yubikey` の version を明示的に固定する。
- object read/write API が feature gate を要求する場合、その feature は YubiKey adapter module だけで使う。
- reset、PIN/PUK 変更、management key 変更、既存 key 削除の API は adapter から公開しない。
- hardware なしの unit test は fake adapter で行う。
- 実機検証は read-only 確認と、この機能用 object / slot への opt-in 書き込みに限定する。

## PIV 領域

### Slot

専用 PIV 鍵には retired key management slot `82` を使う。標準用途の `9A`、`9C`、`9D`、`9E` は使わない。`82` に既存 key または certificate がある場合、`setup` は停止する。

鍵は YubiKey 上で生成する。秘密鍵 material は export しない。鍵種別は `RSA2048` とし、content encryption key の wrap / unwrap にだけ使う。host は PIV private key そのものを読まず、`wrapped_key` の unwrap に必要な private key operation だけを YubiKey に依頼する。

PIV の RSA decrypt operation は raw RSA として扱い、OAEP padding は host 側で処理する。OAEP の hash と MGF1 hash は SHA-256 に固定する。`yubikey` crate の PIV decrypt API から得た raw decrypt bytes は、secret storage adapter 境界で OAEP unpad して content key に戻す。

PIN policy は `Once`、touch policy は `Always` とする。1 コマンド内では PIN verification を 1 回に抑え、secret 復号操作ごとに YubiKey touch を要求する。例えば `enroll-spare` は primary 側の 3 secret 読み出しで 3 回、spare 側の local verify で 3 回の touch が発生する。連続した復旧コマンドでも touch を省略しない。

### Object IDs

| Object ID | 用途 |
| --- | --- |
| `0x005FFF16` | dotfiles secret storage manifest |
| `0x005FFF17` | `bw-email` encrypted blob |
| `0x005FFF18` | `bw-password` encrypted blob |
| `0x005FFF19` | `bws-access-token` encrypted blob |

PIV data object は app 独自データを置けるが、今回使う object は PIN なしで読めるものとして扱う。そのため data object に置くのは manifest と暗号化済み blob だけにする。平文 secret や平文 content encryption key を置くと、object を読めるだけで復号できるため禁止する。

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
2. primary 読み出しが完了した直後に spare YubiKey を選択する。primary と spare を同時接続できない場合は、この時点で primary を抜いて spare を挿し、prompt で Enter を押させる。
3. spare の専用 PIV slot / object が未使用であることを確認し、必要なら setup を行う。
4. primary から読み出した secret を、spare 用の新しい content encryption key と nonce で再暗号化し、spare の public key で key wrap して保存する。
5. local verify を実行し、spare 単体で 3 種類の secret を復号できることを確認する。

secret はプロセスメモリ上の secret wrapper にだけ保持し、CLI 引数、ログ、一時ファイル、環境変数には残さない。通常の `enroll-spare` は利用者に `bw-email`、`bw-password`、`bws-access-token` の再入力を要求しない。

spare に保存する blob は primary の ciphertext、nonce、wrapped key を流用しない。spare の PIV public key に対して新しい content encryption key を wrap し、AEAD additional data には spare の serial と保存先 object ID を使う。これにより、primary 由来の serial や blob を spare 側に持ち込まない。

primary 読み出し後に spare へ差し替える間も、平文 secret は `secrecy` の wrapper と memory guard の内側だけに置く。正常終了、error、timeout、Ctrl-C などの interrupt path では必ず zeroize する。panic message、debug 表示、error context には secret 本文を含めない。`enroll-spare` は secret を読む前に core dump を無効化し、`mlock` 相当の memory lock が使えることを確認する。準備に失敗した場合は、primary YubiKey や stdin から secret を読み始める前に停止する。

YubiKey の選択は対話を基本にする。1 本だけ接続されている場合はその YubiKey を対象にする。複数本接続されている場合は serial と識別情報を表示して選択させる。非対話実行では `--primary-serial <serial>` と `--spare-serial <serial>` で対象を明示する。

primary の初期登録も同じ考え方にし、通常は次のコマンドだけを使う。

```sh
dotfiles secrets yubikey enroll-primary
```

`bws-access-token` を rotate した場合は、primary とすべての spare に対して次を実行する。

```sh
dotfiles secrets yubikey rotate-bws-token
```

`rotate-bws-token` は新しい token を一度だけ受け取り、対象 YubiKey を対話的に選択させながら primary と spare を更新する。各 YubiKey への保存後に local verify を行う。BWS 接続確認は `verify-yubikey --check bws` 側の確認項目であり、local secret storage の検証とは別の確認として summary に残す。非対話実行では `--serial <serial>` を指定して 1 本ずつ更新し、token は `--stdin` で渡せる。

外部サービスの登録状況は YubiKey PIV object からは検証できないため、`setup` / `put` / `get` の成功は GitHub、Bitwarden、Google、Apple などで spare key が登録済みであることを保証しない。

## 保存形式

YubiKey の data object には次の 2 種類だけを保存する。

- manifest: この YubiKey が dotfiles secret storage の対応 format を持つことを示す sentinel。
- secret blob: secret ごとに保存する envelope encryption 済み binary blob。

slot、object ID、secret id の対応は実装側の固定仕様であり、manifest を読んで動的に変えない。

Manifest は JSON とし、UTF-8 bytes を PIV data object に保存する。

```json
{
  "version": 1,
  "app": "dotfiles.secret-recovery"
}
```

Manifest は format sentinel としてだけ使う。`version` と `app` が期待値と一致しなければ、その YubiKey はこの実装の storage として扱わない。

Secret blob は binary format とする。先頭に ASCII magic と version を置き、以降は structured binary として parse する。

```text
DOTFILES-YK-SECRET\0
version: u8 = 1
secret_id: u8
algorithm: u8 = 1
nonce: [u8; 12]
wrapped_key_len: u16be
wrapped_key: bytes
ciphertext_len: u32be
ciphertext: bytes
tag: [u8; 16]
```

各 field はこの順序で連結する。`wrapped_key` は直前の `wrapped_key_len` bytes、`ciphertext` は直前の `ciphertext_len` bytes とする。末尾に追加 bytes がある blob は拒否する。

Envelope encryption は次の役割分担にする。

- `algorithm = 1` は AES-256-GCM を表す。
- `secret_id` は `1 = bw-email`、`2 = bw-password`、`3 = bws-access-token` を表す。
- secret 本文は secret ごとに生成するランダムな 32-byte content encryption key で AEAD 暗号化する。
- AES-256-GCM の nonce は 12 bytes、tag は 16 bytes に固定する。format 互換性を単純に保つため、nonce / tag の可変長 field は持たない。
- content encryption key は slot `82` の RSA public key で wrap し、平文では保存しない。
- `get` は PIV private key operation で content encryption key を unwrap し、AEAD で secret 本文を復号する。
- AEAD additional data には `version`、`secret_id`、object ID、YubiKey serial を含め、blob の入れ替えを検出する。

保存時の blob が漏れた場合でも、slot `82` の private key operation を通せなければ `wrapped_key` は content encryption key に戻せない。復号時には host memory 上に content encryption key と平文 secret が一時的に現れるため、この方式は実行中 host の compromise を防ぐものではない。

平文 secret は `String` ではなく `secrecy` の secret wrapper に入れた byte buffer として扱う。ログ、error context、debug 表示に secret 本文や復号済み buffer を含めない。暗号化 blob や PIV operation の一時 buffer など wrapper の外に出る bytes は `zeroize` 対象にする。

## コマンド仕様

### `dotfiles secrets yubikey setup`

`setup` は低水準コマンドであり、通常は `enroll-primary` / `enroll-spare` から内部的に実行する。直接実行時は次を確認する。

- YubiKey が 1 本だけ接続されていればそれを対象にする。複数本ある場合は serial と識別情報を表示して選択させる。非対話実行では `--serial <serial>` を要求する。
- PIV application version が利用条件を満たすこと。
- slot `82` に既存 key / certificate がないこと。
- `0x005FFF16`、`0x005FFF17`、`0x005FFF18`、`0x005FFF19` に既存 data object がないこと。
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

保存先 object に既存 blob がある場合は `--force` がない限り停止する。`--force` がある場合も、manifest の app / version が一致しない場合は停止する。

### `dotfiles secrets yubikey get <name>`

`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。PIN verification と touch を経て secret を復号し、stdout に secret 本文だけを出力する。stderr には進行状況を出さない。取得失敗時の error には secret name までを含め、secret 本文、ciphertext、wrapped key は含めない。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として登録する高水準コマンドである。これは bootstrap secret の正本を最初に登録する操作なので、`bw-email`、`bw-password`、`bws-access-token` を prompt から受け取る。`bw-email` は通常表示 prompt、`bw-password` と `bws-access-token` は hidden prompt にする。非対話または migration 用に限り `--stdin-json` を許可する。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として登録する高水準コマンドである。primary から bootstrap secret を読み出して spare に再暗号化する操作にまとめ、利用者が低水準コマンドを手順として並べたり、secret を再入力したりしなくてよいようにする。

通常実行では、まず primary YubiKey を選択して 3 種類の secret を復号する。復号が終わった直後に spare YubiKey の選択へ進む。YubiKey を 1 本ずつしか接続できない環境では、この時点で primary を抜き、spare を挿して Enter を押す。同時接続できる環境では、spare の serial を対話選択するか `--spare-serial <serial>` で明示する。非対話実行では `--primary-serial <serial>` と `--spare-serial <serial>` を指定する。

`--stdin-json` は primary YubiKey が利用できないが、別経路で正本 secret を持っている場合の recovery / migration 用に限る。この場合だけ次の JSON を stdin から 1 回だけ受け取る。

```json
{
  "bw-email": "user@example.com",
  "bw-password": "secret",
  "bws-access-token": "secret"
}
```

入力 bytes をログや一時ファイルへ残さない。JSON parse 後の secret は `secrecy` の secret wrapper として扱う。

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

指定 YubiKey の `bws-access-token` だけを更新する。対話実行では新しい token を一度だけ読み取り、primary と spare を順に選択して更新する。同一 serial を同じ実行内で再選択した場合は停止する。非対話実行では `--serial` で 1 本だけを更新し、token は `--stdin` で受け取れる。更新後は local verify を実行する。BWS 接続確認は外部サービス確認項目として summary に含め、local secret storage の検証と区別する。

### `dotfiles secrets verify-yubikey`

挿さっている YubiKey が復旧入口として使えるか確認する。local storage 確認では YubiKey 上の manifest と 3 secret の復号可能性を検証する。BWS と Bitwarden login は外部サービス確認項目として summary に含め、local storage の検証結果と区別する。

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

- manifest が存在し、app、version が期待値と一致する。
- `bw-email`、`bw-password`、`bws-access-token` の blob が存在する。
- blob の magic、version、algorithm、secret id、length field が妥当である。
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
- `enroll-spare` で平文 secret を読む前に core dump 無効化または memory lock の準備に失敗した。
- `rotate-bws-token` 後の local verify に失敗した。
- manifest が存在するが app、version が期待値と一致しない。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、`--force` が指定されていない。
- secret 入力が空。
- secret blob の magic、version、algorithm、secret id、length field、AEAD additional data が一致しない。
- 復号または認証 tag 検証に失敗した。
- `verify-yubikey` で必須 check が失敗した。

## テスト方針

Unit test は fake YubiKey adapter で行う。

- 許可 name と拒否 name。
- 対話実行で複数 YubiKey がある場合に一覧から選択できること。
- 非対話実行で複数 YubiKey がある場合に serial option なしで停止すること。
- manifest parse / serialize。
- manifest が slot / object mapping を持たない sentinel であること。
- 固定 object ID mapping。
- `put` の既存 blob 検出と `--force`。
- blob parser が trailing bytes と不正 length を拒否すること。
- `enroll-primary` が setup、3 secret 保存、local verify を順に実行すること。
- `enroll-spare` が primary 読み出し、spare setup、spare への再暗号化保存、local verify を順に実行すること。
- `enroll-spare` が primary / spare 同一 serial と spare 待ち timeout を拒否すること。
- `enroll-spare` が secret 読み込み前に core dump 無効化と memory lock probe を実行すること。
- `enroll-spare` の error / interrupt path で secret wrapper が zeroize されること。
- `rotate-bws-token` が `bws-access-token` だけを更新し、`bw-email` と `bw-password` を変更しないこと。
- `verify-yubikey` local storage check の正常系と missing manifest / missing blob / decrypt failure。
- empty secret の拒否。
- blob magic / version / secret id mismatch の拒否。
- adapter error から利用者向け error context への変換。
- secret 本文が error 表示に含まれないこと。

実機 validation は、専用 slot / object が空の検証用 YubiKey に限定する。reset、credential 削除、既存領域上書きを含む検証は行わない。

## 参考

- Yubico PIV slots: https://docs.yubico.com/yesdk/users-manual/application-piv/slots.html
- Yubico PIV data objects: https://docs.yubico.com/yesdk/users-manual/application-piv/piv-objects.html
- Yubico PIV tool object/slot reference: https://docs.yubico.com/software/yubikey/tools/pivtool/piv-tool-command.html
- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
- `yubikey` crate docs: https://docs.rs/yubikey/latest/yubikey/
