# YubiKey 秘密情報保存設計

この文書は、#12「YubiKey 秘密情報保存」の design PR で確定する仕様を定義する。対象は `bw-password` と `bws-access-token` を YubiKey に保存し、復旧コマンドから安全に取得するための `dotfiles secrets yubikey` サブコマンドである。

## 決定事項

- PIV 操作には Rust crate `yubikey` を使う。
- 平文 secret は PIV data object に保存しない。
- YubiKey 上に専用の PIV 鍵を生成し、secret はローカルで envelope encryption した blob として custom PIV data object に保存する。
- 専用 PIV 鍵は retired key management slot `82` を使う。
- data object は PIV の undefined tag 範囲から `0x005FDF10` から `0x005FDF12` までを使う。
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

鍵は YubiKey 上で生成する。秘密鍵 material は export しない。鍵種別は `RSA2048` とし、content encryption key の wrap / unwrap にだけ使う。RSA padding は OAEP を使う。implementation PR では `yubikey` crate の PIV decrypt API と対応 padding を確認し、対応できない場合は design PR に戻して方式を見直す。

PIN policy は `Always`、touch policy は `Cached` とする。`get` 実行時には PIN verification を要求し、secret 復号操作では YubiKey touch を要求する。ただし接続された YubiKey firmware が `Cached` touch を扱えない場合は `Always` に落とし、touch なしにはしない。

### Object IDs

| Object ID | 用途 |
| --- | --- |
| `0x005FDF10` | dotfiles secret storage manifest |
| `0x005FDF11` | `bw-password` encrypted blob |
| `0x005FDF12` | `bws-access-token` encrypted blob |

PIV data object は app 独自データを置けるが、読み出し自体は secret 保護境界にしない。そのため data object に置くのは暗号化済み blob と manifest だけにする。

## 保存形式

Manifest は JSON とし、UTF-8 bytes を PIV data object に保存する。

```json
{
  "version": 1,
  "app": "dotfiles.secret-recovery",
  "key_slot": "82",
  "objects": {
    "bw-password": "0x005FDF11",
    "bws-access-token": "0x005FDF12"
  }
}
```

Secret blob は binary format とする。先頭に ASCII magic と version を置き、以降は structured binary として parse する。

```text
DOTFILES-YK-SECRET\0
version: u8 = 1
name_len: u8
name: utf8 bytes
algorithm: u8
nonce_len: u8
nonce: bytes
wrapped_key_len: u16
wrapped_key: bytes
ciphertext_len: u32
ciphertext: bytes
tag_len: u8
tag: bytes
```

Envelope encryption は次の役割分担にする。

- secret 本文はランダムな content encryption key で AEAD 暗号化する。
- content encryption key は slot `82` の RSA public key で wrap する。
- `get` は PIV private key operation で content encryption key を unwrap し、AEAD で secret 本文を復号する。
- AEAD additional data には `version`、`name`、object ID、YubiKey serial を含め、blob の入れ替えを検出する。

平文 secret は `String` ではなく zeroize 可能な byte buffer として扱う。ログ、error context、debug 表示に secret 本文や復号済み buffer を含めない。

## コマンド仕様

### `dotfiles secrets yubikey setup`

`setup` は次を確認する。

- YubiKey が 1 本だけ選択できること。複数本ある場合は `--serial <serial>` を要求する。
- PIV application version が利用条件を満たすこと。
- slot `82` に既存 key / certificate がないこと。
- `0x005FDF10`、`0x005FDF11`、`0x005FDF12` に既存 data object がないこと。
- PIN retries が 0 ではないこと。
- management key authentication が可能なこと。

確認後、slot `82` に専用鍵を生成し、manifest を保存する。既存の FIDO2 / OTP / OpenPGP / PIV credential は reset しない。衝突がある場合に自動削除や上書きはしない。

### `dotfiles secrets yubikey put <name>`

`<name>` は `bw-password` または `bws-access-token` のみ許可する。それ以外は CLI parsing 後の validation で拒否する。

secret 入力は次の順で受け付ける。

- default: hidden prompt
- `--stdin`: stdin から 1 secret を読む

CLI 引数で secret 本文は受け取らない。stdin 入力時も trailing newline は 1 つだけ除去し、それ以外の bytes は保持する。

保存先 object に既存 blob がある場合は `--force` がない限り停止する。`--force` がある場合も、manifest の app / version / slot が一致しない場合は停止する。

### `dotfiles secrets yubikey get <name>`

`<name>` は `bw-password` または `bws-access-token` のみ許可する。PIN verification と touch を経て secret を復号し、stdout に secret 本文だけを出力する。stderr には進行状況を出さない。取得失敗時の error には secret name までを含め、secret 本文、ciphertext、wrapped key は含めない。

## 停止条件

- YubiKey が見つからない。
- 複数 YubiKey が接続され、`--serial` が指定されていない。
- 指定 serial の YubiKey が見つからない。
- PIV application が利用できない。
- PIN retries が 0。
- management key authentication に失敗する。
- slot `82` に既存 key または certificate がある。
- 使用予定 object ID に既存 data object がある。
- manifest が存在するが app、version、slot、object mapping が期待値と一致しない。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、`--force` が指定されていない。
- secret 入力が空。
- secret blob の magic、version、name、AEAD additional data が一致しない。
- 復号または認証 tag 検証に失敗した。

## テスト方針

Unit test は fake YubiKey adapter で行う。

- 許可 name と拒否 name。
- manifest parse / serialize。
- object ID mapping。
- `put` の既存 blob 検出と `--force`。
- empty secret の拒否。
- blob magic / version / name mismatch の拒否。
- adapter error から利用者向け error context への変換。
- secret 本文が error 表示に含まれないこと。

実機 validation は implementation PR で手順を明記する。対象は専用 slot / object が空の検証用 YubiKey に限定し、reset、credential 削除、既存領域上書きは行わない。

## 参考

- Yubico PIV slots: https://docs.yubico.com/yesdk/users-manual/application-piv/slots.html
- Yubico PIV data objects: https://docs.yubico.com/yesdk/users-manual/application-piv/piv-objects.html
- Yubico PIV tool object/slot reference: https://docs.yubico.com/software/yubikey/tools/pivtool/piv-tool-command.html
- `yubikey` crate docs: https://docs.rs/yubikey/latest/yubikey/
