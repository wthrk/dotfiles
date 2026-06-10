# YubiKey 秘密情報保存設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / YubiKey](./secret-recovery-spec.md#yubikey) を具体化する到達設計仕様を定義する恒久文書である。対象は `bw-email`、`bw-password`、`bws-access-token` を YubiKey に保存し、復旧コマンドから安全に取得するための `dotfiles secrets yubikey` サブコマンドである。

この文書は完成形の設計だけを扱う。

secret の保護境界、core dump 無効化、paging / memory lock / signal trap の扱い、外部処理が secret の借用または所有 plaintext buffer の move を要求する場合の実装方針は [Secret handling policy](./secret-handling.md) を正本とする。この文書では YubiKey PIV 保存形式とコマンド契約だけを定義する。

## 目的と保護境界

この機能の目的は、新規マシン復旧に必要な bootstrap secret を、YubiKey がなければ復号できない形で保存することである。PIV data object は読み出し自体を secret 保護境界にしない。今回使う custom data object は PIN なしで読めるものとして扱い、そこには平文 secret も平文 content encryption key も置かない。

保護するもの:

- YubiKey PIV data object から読み出された encrypted blob。
- blob の backup、copy、log、diagnostic dump。
- PIN 検証 と touch を通せない状態での `wrapped_key`。

保護しないもの:

- 復号を許可した実行中 host の memory。
- 復号後に stdout や外部 command に渡された secret。
- YubiKey、PIN、touch 操作を攻撃者が同時に利用できる状況。

この境界のため、保存形式は envelope encryption にする。secret 本文はランダムな content encryption key で AES-256-GCM 暗号化し、その content encryption key は YubiKey 内の non-exportable PIV private key に対応する public key で wrap する。永続保存される blob は `nonce`、`wrapped_key`、`ciphertext`、`tag` だけであり、復号には YubiKey の private key operation が必要になる。

## 決定事項

- PIV 操作には Rust crate `yubikey` を使う。
- bootstrap secret 本文は `ProtectedSecret` で保持し、zeroize と protection 内借用の所有境界から外へ出さない。
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
- `get` は復旧コマンド内部の利用を主用途とし、直接実行時は pipe または redirect された stdout にだけ secret 本文を出力する。
- 書き込み操作は YubiKey の management key で認証する。現行実装は factory-default management key を使って認証する。
- 非既定 management key への切替、取得、注入は将来の安全条件候補だが、現行実装の完了条件には含めていない。

## 採用 crate

`yubikey` crate を採用する。この crate は PC/SC 経由で YubiKey PIV を操作し、PIV 鍵、PIN 検証、object read/write の API を提供する。Yubico 公式 Rust SDK ではないため、実装では CLI 側の adapter に crate 型を閉じ込め、storage logic から直接公開しない。

secret memory handling は役割ごとに crate を分ける。

- `zeroize`: bootstrap secret 本文、content encryption key、復号済み secret buffer など、平文 secret material を保持する byte buffer の zeroize。
- `rlimit`: CLI が secret を読み始める前に core dump を無効化する。実装は secret 入力境界で `SecretSession::start()` を開始し、`ProtectedInputBuffer` / `ProtectedSecret` へ平文 secret を入れる前に process guard を確立する。これは crash dump 永続化経路の削減であり、実行中 memory compromise への防御や平文 secret の argv/env/log 露出許可ではない。

YubiKey adapter は次を満たす。

- `yubikey` の version を明示的に固定する。
- object read/write API が feature gate を要求する場合、その feature は YubiKey adapter module だけで使う。
- reset、PIN/PUK 変更、management key 変更、既存 key 削除の API は adapter から公開しない。
- hardware なしの 単体テスト は fake adapter で行う。
- 実機検証は 読み取り専用 確認と、この機能用 object / slot への opt-in 書き込みに限定する。

## PIV 領域

### Slot

専用 PIV 鍵には retired key management slot `82` を使う。標準用途の `9A`、`9C`、`9D`、`9E` は使わない。`82` に既存 key または certificate がある場合、`setup` は停止する。

鍵は YubiKey 上で生成する。秘密鍵 material は export しない。鍵種別は `RSA2048` とし、content encryption key の wrap / unwrap にだけ使う。host は PIV private key そのものを読まず、`wrapped_key` の unwrap に必要な private key operation だけを YubiKey に依頼する。

PIV の RSA decrypt operation は raw RSA として扱い、OAEP padding は host 側で処理する。OAEP の hash と MGF1 hash は SHA-256 に固定する。`yubikey` crate の PIV decrypt API から得た raw decrypt bytes は、secret storage adapter 境界で OAEP unpad して content key に戻す。`rsa` crate は raw RSA 復号結果に対する OAEP unpad API を公開していないため、OAEP unpad は CLI 側で最小実装を持つ。この実装は invalid padding の判定で separator 位置による短絡を避けるが、constant-time primitive として扱わない。Manger 攻撃に対する境界は、復号対象を 32-byte content encryption key に限定し、各 unwrap に YubiKey の PIN 検証、touch policy、PIV private operation を要求することで oracle としての利用回数と自動化を制限する。

PIN policy は `Once`、touch policy は `Always` とする。1 コマンド内では PIN 検証 を 1 回に抑え、secret 復号操作ごとに YubiKey touch を要求する。例えば `enroll-spare` は spare 側の ローカル確認 で 3 回の touch が発生する。連続した復旧コマンドでも touch を省略しない。

### Object IDs

Object ID と用途の対応は次のとおり。

- `0x005FFF16`: dotfiles secret storage manifest
- `0x005FFF17`: `bw-email` encrypted blob
- `0x005FFF18`: `bw-password` encrypted blob
- `0x005FFF19`: `bws-access-token` encrypted blob

PIV data object は app 独自データを置けるが、今回使う object は PIN なしで読めるものとして扱う。そのため data object に置くのは manifest と暗号化済み blob だけにする。平文 secret や平文 content encryption key を置くと、object を読めるだけで復号できるため禁止する。

## スペア YubiKey

この文書で扱うスペア YubiKey は、`dotfiles` 独自の bootstrap secret storage に限る。Bitwarden、GitHub、Google、Apple など外部サービスの FIDO2 / passkey / U2F / OTP 登録は各サービス側で primary と spare を別々に登録する。OATH TOTP は同じ TOTP secret / QR code を primary と spare の両方に登録する。

スペア YubiKey は事前登録を必須にする。primary YubiKey の紛失後に、primary だけに保存されていた `bw-email`、`bw-password`、`bws-access-token` からスペアを後付け作成することはできない。

同じ PIV 秘密鍵を複製して複数 YubiKey に入れる運用は採用しない。各 YubiKey で slot `82` に別々の non-exportable key を生成し、同じ secret をその YubiKey の public key で個別に wrap して保存する。

スペア作成手順は CLI prompt/input port から bootstrap secret を入力する 1 コマンドにまとめる。

```sh
dotfiles secrets yubikey enroll-spare
```

`enroll-spare` は次を一連の処理として実行する。

1. spare YubiKey だけが接続されていることを確認する。
2. spare の専用 PIV slot / object が未使用であることを確認し、必要なら setup を行う。
3. CLI prompt/input port から `bw-email`、`bw-password`、`bws-access-token` を読み取る。
4. 入力 secret を、spare 用の新しい content encryption key と nonce で暗号化し、spare の public key で key wrap して保存する。
5. ローカル確認 を実行し、spare 単体で 3 種類の secret を復号できることを確認する。

secret はプロセスメモリ上の `ProtectedSecret` にだけ保持し、CLI 引数、ログ、一時ファイル、環境変数には残さない。

spare に保存する blob は primary の ciphertext、nonce、wrapped key を流用しない。spare の PIV public key に対して新しい content encryption key を wrap し、AEAD additional data には spare の serial と保存先 object ID を使う。これにより、primary 由来の serial や blob を spare 側に持ち込まない。

正常終了、エラー、Ctrl-C などの path では所有値の Drop と zeroize によって破棄へ進める。panic message、debug 表示、エラー context には secret 本文を含めない。

YubiKey の選択は「接続中の対象が 1 本だけであること」を基本にする。1 本だけ接続されている場合はその YubiKey を対象にする。複数本接続されている場合は識別子や識別情報を表示せず停止し、対象 1 本だけを接続して再実行させる。非対話実行でも複数本接続時に対象指定を受け付ける手順にはせず、対象を 1 本だけ接続する運用に寄せる。

primary の初期登録も同じ考え方にし、通常は次のコマンドだけを使う。

```sh
dotfiles secrets yubikey enroll-primary
```

`bws-access-token` を rotate した場合は、primary とすべての spare に対して次を実行する。

```sh
dotfiles secrets yubikey rotate-bws-token
```

`rotate-bws-token` は新しい token を一度だけ受け取り、接続中の YubiKey が 1 本だけであることを確認しながら更新する。各 YubiKey への保存後に、その 1 本に対してだけ ローカル確認 を行う。利用者は primary とすべての spare を 1 本ずつ接続して個別に更新する。BWS 接続確認は `verify-yubikey --check bws` 側の確認項目であり、ローカル保管 の検証とは別の確認として扱う。非対話実行でも複数本接続時は対象指定を受け付けず停止し、token は `--stdin` で渡せる。

外部サービスの登録状況は YubiKey PIV object からは検証できないため、`setup` / `put` / `get` の成功は GitHub、Bitwarden、Google、Apple などで 予備キー が登録済みであることを保証しない。

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
- AEAD additional data には `version`、`secret_id`、object ID、接続 device の保存形式上の識別値を含め、blob の入れ替えを検出する。

保存時の blob が漏れた場合でも、slot `82` の private key operation を通せなければ `wrapped_key` は content encryption key に戻せない。復号時には host memory 上に content encryption key と平文 secret が一時的に現れるため、この方式は実行中 host の compromise を防ぐものではない。

平文 secret は `String` ではなく `ProtectedSecret` の byte buffer として扱う。ログ、エラー context、debug 表示に secret 本文や復号済み buffer を含めない。暗号化済み blob は平文 secret material の保護境界には含めず、diagnostics では byte 列を redaction する。

## 到達仕様のコマンド定義

この節は最終到達状態で提供するコマンド契約を定義する。現行実装の利用可否を示す手順書としては扱わない。

### `dotfiles secrets yubikey setup`

`setup` は低水準コマンドであり、通常は `enroll-primary` / `enroll-spare` から内部的に実行する。直接実行時は次を確認する。

- YubiKey が 1 本だけ接続されていればそれを対象にする。複数本ある場合は識別子や識別情報を表示せず停止し、対象 1 本だけを接続して再実行させる。非対話実行でも対象指定を受け付ける手順にはしない。
- PIV application version が利用条件を満たすこと。
- slot `82` に既存 key / certificate がないこと。
- `0x005FFF16`、`0x005FFF17`、`0x005FFF18`、`0x005FFF19` に既存 data object がないこと。
- PIN retries が 0 ではないこと。
- management key authentication が可能なこと。現行実装では factory-default management key で認証できること。

確認後、slot `82` に専用鍵を生成し、manifest を保存する。既存の FIDO2 / OTP / OpenPGP / PIV credential は reset しない。衝突がある場合に自動削除や上書きはしない。

複数本の YubiKey を運用する場合でも、`setup` は指定された 1 本だけを変更する。接続中の他 YubiKey へ同時に書き込む batch mode は実装しない。

### `dotfiles secrets yubikey put <name>`

`put` は低水準コマンドであり、通常の primary / spare 登録では `enroll-primary` / `enroll-spare` を使う。`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。それ以外は CLI parsing 後の 検証 で拒否する。

secret 入力は次の順で受け付ける。

- default: hidden prompt
- `--stdin`: stdin から 1 secret を読む

CLI 引数で secret 本文は受け取らない。`--stdin` は pipe または redirect された stdin だけを受け付け、TTY stdin では hidden prompt を使わせるため失敗させる。stdin 入力時も trailing newline は 1 つだけ除去し、それ以外の bytes は保持する。

保存先 object に既存 blob がある場合は `--force` がない限り停止する。`--force` がある場合も、manifest の app / version が一致しない場合は停止する。
`bws-access-token` を保存する場合は、候補 token 自身で BWS project `dotfiles-secret-recovery` の `password-store-remote` note を読み、そこに保存された provisioning token provenance marker と候補 token の opaque token id を照合する。一致した場合は provisioning token の再利用として停止し、marker が無い場合も fail-closed で停止する。

### `dotfiles secrets yubikey get <name>`

`<name>` は `bw-email`、`bw-password`、`bws-access-token` のみ許可する。PIN 検証 と touch を経て secret を復号し、stdout に secret 本文だけを出力する。stdout が terminal の場合は、画面や scrollback に平文 secret が残るため拒否する。stderr には進行状況を出さない。取得失敗時の エラー には secret name までを含め、secret 本文、ciphertext、wrapped key は含めない。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として登録する高水準コマンドである。これは bootstrap secret の正本を最初に登録する操作なので、`bw-email`、`bw-password`、`bws-access-token` を CLI prompt/input port から受け取る。`bw-email` は通常表示 prompt、`bw-password` と `bws-access-token` は hidden prompt にする。bootstrap secret payload を stdin から渡す option は提供しない。`bws-access-token` は `put` と同じ provenance marker gate を通し、provisioning 用 token と同一値、または marker 不在の token は拒否する。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として登録する高水準コマンドである。YubiKey の識別子指定を使わず、接続中の 1 本だけを対象に CLI prompt/input port から受け取った bootstrap secret を保存する。複数本が同時接続されている場合は対象を選ばせず停止し、対象 1 本だけを接続して再実行させる。非対話実行でも複数本接続時に対象指定を受け付ける手順にはしない。primary YubiKey から secret を読み出す経路や stdin payload は使わない。

CLI prompt/input port が受け取った secret bytes をログや一時ファイルへ残さない。secret は入力境界で
`ProtectedSecret` として扱う。行入力の終端改行は入力境界で除去し、保存対象の値には含めない。
`bws-access-token` は `put` と同じ provenance marker gate を通し、provisioning 用 token と同一値、または marker 不在の token は拒否する。

`enroll-primary` / `enroll-spare` は成功時に secret 本文や YubiKey 識別子を出さず、次の要約だけを出力する。`role` は `primary` または `spare` のいずれかである。

```json
{
  "role": "primary",
  "checks": [
    { "name": "setup", "status": "ok" },
    { "name": "bw-email", "status": "ok" },
    { "name": "bw-password", "status": "ok" },
    { "name": "bws-access-token", "status": "ok" },
    { "name": "local-storage", "status": "ok" }
  ]
}
```

```json
{
  "role": "spare",
  "checks": [
    { "name": "setup", "status": "ok" },
    { "name": "bw-email", "status": "ok" },
    { "name": "bw-password", "status": "ok" },
    { "name": "bws-access-token", "status": "ok" },
    { "name": "local-storage", "status": "ok" }
  ]
}
```

### `dotfiles secrets yubikey rotate-bws-token`

指定 YubiKey の `bws-access-token` だけを更新する。対話実行では新しい token を一度だけ読み取り、接続中の YubiKey が 1 本だけであることを確認して更新する。複数本が同時接続されている場合は対象を選ばせず停止する。primary とすべての spare は 1 本ずつ接続して個別に更新する。非対話実行でも複数本接続時は対象指定を受け付けず停止し、token は `--stdin` で受け取れる。更新前に ローカル保管 が復号可能な状態かを確認し、更新不能なら token を読まずに停止する。更新後は更新したその 1 本に対してだけ ローカル確認 を実行し、要約もその 1 本の local-storage 検証結果だけを返す。BWS 接続確認は ローカル保管 とは別の外部確認として扱う。新しい token は `put` と同じ provenance marker gate を通し、provisioning 用 token と同一値、または marker 不在の token は拒否する。

### `dotfiles secrets verify-yubikey`

挿さっている YubiKey が復旧入口として使えるか確認する。ローカル保管 確認では YubiKey 上の manifest と 3 secret の復号可能性を検証する。BWS と Bitwarden login は外部サービス確認項目として 要約 に含め、ローカル保管 の検証結果と区別する。

引数:

- `--check bws`: `bws-access-token` で Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` を取得できることに加え、`gpg-secret-key-backup` envelope schema（`version` / `metadata` / `recipients` / `ciphertext`）と `metadata.primary_fingerprint` 形式（lowercase hex 40 文字、区切りなし）を検証し、primary/spare の 2 recipient 以上が事前登録されていること、接続中 YubiKey に一致する recipient（`yubikey_serial` と `public_key_fingerprint` の両一致）があることを照合して、unwrap なしで判定できる復旧可能性を確認する外部確認項目。1 recipient の envelope は接続中 YubiKey に一致していても失敗扱いにする。secret 本文の平文化や unwrap は行わず、利用できない場合は失敗する。
- `--check bw-login`: `bw-email`、`bw-password`、入力された YubiKey OTP で Bitwarden Password Manager の login / unlock ができることを確認する外部確認項目。email override が必要な場合は `--email <email>` を使う。
- `--all`: ローカル保管確認と外部確認を含む全確認項目を実行する。指定した確認項目のいずれかが利用できない場合は失敗する。

外部確認を明示要求した場合（`--check bws`、`--check bw-login`、`--all`）は、`skipped` を成功扱いにせず、外部確認が利用できないことを エラー として返す。引数なしの `verify-yubikey` は ローカル保管 のみ検証し、外部確認項目を `skipped` として 要約 に残す。

出力は 機械可読 な 要約 にし、状態値は `ok` / `failed` / `skipped` を使う。表示文言は別層で扱い、JSON の状態値を翻訳しない。secret 本文、access token、Bitwarden session token は出力しない。

```json
{
  "checks": [
    { "name": "local-storage", "status": "ok" },
    { "name": "bws", "status": "skipped" },
    { "name": "bw-login", "status": "skipped" }
  ]
}
```

ローカル保管確認 は次を確認する。

- manifest が存在し、app、version が期待値と一致する。
- `bw-email`、`bw-password`、`bws-access-token` の blob が存在する。
- blob の magic、version、algorithm、secret id、length field が妥当である。
- PIN 検証 と touch を経て 3 種類の secret を復号できる。
- 復号した secret は空ではない。

このコマンドは GitHub、Google、Apple など外部サービスの FIDO2 / passkey / U2F 登録状況を検証しない。

## 停止条件

- YubiKey が見つからない。
- 複数 YubiKey が接続されている。
- PIV application が利用できない。
- PIN retries が 0。
- management key authentication に失敗する。
- slot `82` に既存 key または certificate がある。
- 使用予定 object ID に既存 data object がある。
- `enroll-primary` / `enroll-spare` の途中で setup、保存、ローカル確認 のいずれかに失敗した。
- `enroll-primary` / `enroll-spare` の途中失敗で setup 済みの部分状態（manifest または一部 secret object）が残ることがある。この状態は回復可能で、同一 YubiKey では `put --force` で不足分を埋めるか、運用手順として専用領域を初期化して再 enroll する。
- `rotate-bws-token --stdin` で PIN 入力に必要な controlling terminal を開けない。
- `rotate-bws-token` 後の ローカル確認 に失敗した。
- manifest が存在するが app、version が期待値と一致しない。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、`--force` が指定されていない。
- `put` の入力前 precondition（manifest 不一致、`--force` なし上書き要求）に失敗した。
- secret 入力が空。
- secret blob の magic、version、algorithm、secret id、length field、AEAD additional data が一致しない。
- 復号または認証 tag 検証に失敗した。
- `verify-yubikey` で必須 check が失敗した。

## テスト方針

単体テスト は fake YubiKey adapter で行う。

- 許可 name と拒否 name。
- 複数 YubiKey がある場合に識別子表示や一覧選択へ進まず停止すること。
- manifest parse / serialize。
- manifest が slot / object mapping を持たない sentinel であること。
- 固定 object ID mapping。
- `put` の既存 blob 検出と `--force`。
- blob parser が trailing bytes と不正 length を拒否すること。
- `enroll-primary` が setup、3 secret 保存、ローカル確認 を順に実行すること。
- `enroll-spare` が spare setup、spare への暗号化保存、ローカル確認 を順に実行すること。
- `enroll-spare` の エラー path で `ProtectedSecret` が zeroize されること。
- `rotate-bws-token` が `bws-access-token` だけを更新し、`bw-email` と `bw-password` を変更しないこと。
- `verify-yubikey` ローカル保管確認 の正常系と missing manifest / missing blob / decrypt failure。
- empty secret の拒否。
- blob magic / version / secret id mismatch の拒否。
- adapter エラー から利用者向け エラー context への変換。
- secret 本文が エラー 表示に含まれないこと。

実機 検証 は、専用 slot / object が空の検証用 YubiKey に限定する。reset、credential 削除、既存領域上書きを含む検証は行わない。

## 参考

- Yubico PIV slots: https://docs.yubico.com/yesdk/users-manual/application-piv/slots.html
- Yubico PIV data objects: https://docs.yubico.com/yesdk/users-manual/application-piv/piv-objects.html
- Yubico PIV tool object/slot reference: https://docs.yubico.com/software/yubikey/tools/pivtool/piv-tool-command.html
- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
- `yubikey` crate docs: https://docs.rs/yubikey/latest/yubikey/
