# YubiKey 秘密情報保存設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / YubiKey](./secret-recovery-spec.md#yubikey) を具体化する到達設計仕様を定義する恒久文書である。復旧に保存する対象は Bitwarden Secrets Manager access token の `bitwarden-client-secret` だけであり、YubiKey を挿して復旧コマンドを実行する利用者契約を実現するために `dotfiles secrets yubikey` サブコマンドが安全に取得する。slot `82` の `TouchPolicy::Always` による physical touch は許容するが、recovery は PIV/OpenPGP PIN、GPG passphrase、pinentry、token 入力を要求しない。

この文書は完成形の設計だけを扱う。

secret の保護境界、core dump 無効化、paging / memory lock / signal trap の扱い、外部処理が secret の借用または所有 plaintext buffer の move を要求する場合の実装方針は [Secret handling policy](./secret-handling.md) を正本とする。この文書では YubiKey PIV 保存形式とコマンド契約だけを定義する。

## 目的と保護境界

この機能の目的は、新規マシン復旧に必要な bootstrap secret を、YubiKey がなければ復号できない形で保存することである。PIV data object は読み出し自体を secret 保護境界にしない。今回使う custom data object は PIN なしで読めるものとして扱い、そこには平文 secret も平文 content encryption key も置かない。

保護するもの:

- YubiKey PIV data object から読み出された encrypted blob。
- blob の backup、copy、log、diagnostic dump。
- touch を通せない状態での `wrapped_key`。

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
- data object は YubiKey が undefined DataTag として受け付ける範囲から `0x005FFF16` から `0x005FFF1a` までを使う。
- manifest は format sentinel としてだけ使う。slot や object ID の解釈を manifest で動的に変えない。
- スペア YubiKey は同じ PIV 秘密鍵を複製せず、各 YubiKey で専用鍵を生成して同じ secret を個別に保存する。
- 通常の primary / spare 登録には `enroll-primary` / `enroll-spare` を使い、低水準の `setup` / `put` を直接並べる手順にしない。primary で一度だけ入力した BWS token は `enroll-spare` が primary から読み、再入力せず spare へ再暗号化する。`provision-bws-token` は InitializedV2 の既存 source storage 確認・logical-empty object 保存に限り、repair/setup/clear を行わない。
- `dotfiles secrets verify-yubikey` で、挿さっている YubiKey が bootstrap secret を復号できることを確認する。
- `dotfiles secrets yubikey setup` は既存の PIV credential や data object と衝突した場合に停止する。
- `put` は同名 secret が存在する場合、`--force` が指定されていなければ停止する。
- secret 本文を stdout に出力する `get` コマンドは提供しない。設定済みの値の確認には `status` を使い、secret 名だけを出力する。
- 書き込み操作は YubiKey の management key で認証する。既定 key のまま運用する YubiKey では、PIN と touch を通せなくても既知の management key でこの機能の PIV object を上書きできるため、非既定 management key を使う運用を前提にする。
- factory-default management key を使う運用は暫定前提にしない。非既定 management key への切替、取得、注入は YubiKey 保存方式の安全条件として扱う。

## 採用 crate

`yubikey` crate を採用する。この crate は PC/SC 経由で YubiKey PIV を操作し、PIV 鍵、PIN 検証、object read/write の API を提供する。Yubico 公式 Rust SDK ではないため、実装では CLI 側の adapter に crate 型を閉じ込め、storage logic から直接公開しない。

secret memory handling は役割ごとに crate を分ける。

- `zeroize`: bootstrap secret 本文、content encryption key、復号済み secret buffer など、平文 secret material を保持する byte buffer の zeroize。
- `rlimit`: `enroll-spare` で secret を読む前に core dump を無効化する。

YubiKey adapter は次を満たす。

- `yubikey` の version を明示的に固定する。
- object read/write API が feature gate を要求する場合、その feature は YubiKey adapter module だけで使う。
- `setup` と物理的に完全な fresh storage を初期化する enrollment の PIN 変更以外では、reset、PIN/PUK 変更、既存 key 削除、management key 変更の API は adapter から公開しない。この lifecycle は既存 PIN-protected management key を前提に PIV application-wide PIN だけを変更し、management key 自体を置換しない。
- repository の test と agent 作業では物理 YubiKey / PC/SC reader を使用しない。検証は `secrets-internal-test-stub` feature で compile-time に隔離された internal backend stub だけで行い、production binary と runtime に stub を混入させない。
- 実機への読み取り・書き込み・setup・clear・reset を「opt-in 検証」としても repository の test、agent 作業、通常の検証手順に許可しない。device-specific な確認が将来必要になっても、この設計だけを根拠に実行せず、別の明示 task と人間による承認済み手順で扱う。

## PIV 領域

### 管理フローの状態・操作モデル

管理 command は次の状態と操作順だけを許可する。矢印の途中で失敗した場合は後続入力・操作へ進まず、
同一 command で retry、resume、clear、fallback を行わない。

```text
resolved device
  -> read-only device capability preflight
  -> current PIN input
  -> one PIV handle: VERIFY -> protected management-key authentication
  -> complete raw inspection:
       PIV version + slot key + slot certificate + slot SPKI
       + manifest physical/non-empty state + every reserved object
  -> domain state:
       Fresh | InitializedV2 | Unsupported | OwnershipUnknown | Invalid

Fresh:
  -> new PIN input -> confirmation
  -> change PIN -> new PIN VERIFY -> protected management-key authentication
  -> key generation -> secret store -> manifest finalization -> local verification

InitializedV2:
  -> write preflight -> secret store -> local verification
```

support backend は slot key、certificate、SPKI を独立した raw fact として返し、`material exists` のような
合成判定を行わない。fresh、repository ownership、manifest/SPKI 整合、initialized 判定は domain が行う。
device capability preflight は Yubico Management application の read-only GET DEVICE INFORMATION を使い、
serial、firmware version、form factor を同一 response から取得する。form factor の FIPS bit を firmware
version や reader 名から推測してはならない。

### Slot

専用 PIV 鍵には retired key management slot `82` を使う。標準用途の `9A`、`9C`、`9D`、`9E` は使わない。`82` に既存 key または certificate がある場合、`setup` は停止する。

鍵は YubiKey 上で生成する。秘密鍵 material は export しない。鍵種別は `RSA2048` とし、content encryption key の wrap / unwrap にだけ使う。host は PIV private key そのものを読まず、`wrapped_key` の unwrap に必要な private key operation だけを YubiKey に依頼する。

PIV の RSA decrypt operation は raw RSA として扱い、OAEP padding は host 側で処理する。OAEP の hash と MGF1 hash は SHA-256 に固定する。`yubikey` crate の PIV decrypt API から得た raw decrypt bytes は、secret storage adapter 境界で OAEP unpad して content key に戻す。`rsa` crate は raw RSA 復号結果に対する OAEP unpad API を公開していないため、OAEP unpad は CLI 側で最小実装を持つ。この実装は invalid padding の判定で separator 位置による短絡を避けるが、constant-time primitive として扱わない。Manger 攻撃に対する境界は、復号対象を 32-byte content encryption key に限定し、YubiKey touch によって oracle としての利用回数と自動化を制限する。復号 read path は PIV PIN の入力・検証を行わない。

管理操作（`put`、`clear`、enroll、rotate、`provision-bws-token`）は hidden TTY PIN → `verify_pin` → `MgmKey::get_protected` → `YubiKey::authenticate` の順に、**最初に選んだ serial へ command 内で一つだけ保持する PIV handle** で実行する。`setup` は current/new/confirmation の hidden TTY PIN（6--8 ASCII alphanumeric bytes）を device mutation 前に検証する。物理的に完全な fresh storage を初期化する enrollment は current PIN だけを先に hidden input し、同じ handle で current PIN VERIFY →既存 `MgmKey::get_protected` →`YubiKey::authenticate` →PIV version、slot `82` key/certificate、manifest、全予約 object の完全管理 inspection を行う。完全な空領域と判定した場合だけ new/confirmation を hidden input して検証し、保持済み current と三値をそろえて `YubiKey::change_pin` を一回適用する。続けて new PIN VERIFY 一回、既存 `MgmKey::get_protected`、`YubiKey::authenticate`、generate、finalize を同じ handle で行う。initialized enrollment は new/confirmation と PIN change を行わず、current-PIN session を継続する。PIN 変更後に inspection/gate を残さない。PIV PIN は application-wide state であり slot 82 専用ではないため、PIN-free status の不完全観測を変更許可に使わない。`change_pin` failure を含む SDK error は retry/PUK/reset/default-key fallback/partial-state resume に写像せず停止し、management key を generate、`set_protected`、置換しない。認証後の inspection、generate、store、finalize、同 command の local verification は同じ handle を使い、operation ごとに reopen / VERIFY しない。`provision-bws-token` は InitializedV2 の完全 inspection、status、必要時保存、同じ handle での復号検証だけを行い、fresh、version 1、zero-length/manifestless partial、ownership 不明、typed invalid を clear/setup/repair しない。PIN 入力一回につき physical VERIFY は一回だけであり、ykman の PIN-protected flow が同一 session で行う「VERIFY を最後の APDU に戻す」second VERIFY は採用しない。serial 未指定時は共通の device discovery が接続中の単一 YubiKey を command の先頭で一回だけ解決し、その同じ device を対象にする。管理 session 開始後に別 serial が要求された場合は、その session では PIN を再利用して新しい handle / VERIFY を作らず停止する。`rotate-bws-token` の継続だけは次 serial の解決後に新しい hidden TTY PIN を読み、前 session を drop して新しい single-device session を開始する。read / unwrap / status / verify / recovery path は PIN も management key も default key も使わない。wrong、blocked、opaque error は retry、PUK、reset、default key fallback をせず停止する。slot `82` は `PinPolicy::Never` と `TouchPolicy::Always` で生成する。`Never` は復旧 private-key operation に PIN を追加要求しない policy、`Always` は各 unwrap に touch を要求する policyであり、管理操作の PIN-protected management-key 認証とは別である。接続なし・複数接続は引き続き停止する。

PIN-free status は slot を観測せず、storage invalid や repository ownership を主張しない。認証済み完全 inspection で manifest/object 空かつ slot `82` key/certificate ありを検出した場合は ownership 不明の固定 opaque failure と manual administrator escalation で停止する。`setup`、enroll、`provision-bws-token`、`clear --yes` は同一 command で retry、resume、clear、slot overwrite/delete、再初期化へ進まない。`put --force` も valid initialized storage の既存 blob 上書きだけを許可し、この partial state の回復には使わない。

### Object IDs

Object ID と用途の対応は次のとおり。

- `0x005FFF16`: dotfiles secret storage manifest
- `0x005FFF19`: `bitwarden-client-secret` encrypted blob

PIV data object は app 独自データを置けるが、今回使う object は PIN なしで読めるものとして扱う。そのため data object に置くのは manifest と暗号化済み blob だけにする。平文 secret や平文 content encryption key を置くと、object を読めるだけで復号できるため禁止する。

## スペア YubiKey

この文書で扱うスペア YubiKey は、`dotfiles` 独自の BWS recovery credential storage に限る。外部サービスの FIDO2 / passkey / U2F / OTP / OATH TOTP 登録は repository の CLI surface 外であり、この storage の保存対象・復旧停止条件に含めない。

スペア YubiKey は事前登録を必須にする。primary YubiKey の紛失後に、primary だけに保存されていた `bitwarden-client-secret` からスペアを後付け作成することはできない。

同じ PIV 秘密鍵を複製して複数 YubiKey に入れる運用は採用しない。各 YubiKey で slot `82` に別々の non-exportable key を生成し、同じ secret をその YubiKey の public key で個別に wrap して保存する。

スペア作成手順は次の 1 コマンドにまとめる。

```sh
dotfiles secrets yubikey enroll-spare
```

`enroll-spare` は次を一連の処理として実行する。

1. primary / spare YubiKey をそれぞれ serial 明示または単一接続 device として解決し、同一 serial でないことを確認する。
2. primary secret を復号する前に、spare の current PIN VERIFY と既存 protected management-key authentication を完了し、PIV version、slot `82` key/certificate、manifest version、対象 object、slot SPKI を完全 inspection する。許可状態は physical fresh、または対象 object が logical empty で manifest/slot SPKI が一致する initialized version 2 だけとする。
3. touch を経て primary から `bitwarden-client-secret` を復号する。
4. spare の setup を行い、primary から読み出した secret を、spare 用の新しい content encryption key と nonce で再暗号化し、spare の public key で key wrap して保存する。
5. ローカル確認を実行し、spare 単体で `bitwarden-client-secret` を復号できることを確認する。

secret はプロセスメモリ上の `ProtectedSecret` にだけ保持し、CLI 引数、ログ、一時ファイル、環境変数には残さない。通常の `enroll-spare` は利用者に `bitwarden-client-secret` の再入力を要求しない。

spare に保存する blob は primary の ciphertext、nonce、wrapped key を流用しない。spare の PIV public key に対して新しい content encryption key を wrap し、AEAD additional data には spare の serial と保存先 object ID を使う。これにより、primary 由来の serial や blob を spare 側に持ち込まない。

primary の secret を読み出した後も、平文 secret は `ProtectedSecret` の内側だけに置く。正常終了、エラー、Ctrl-C などの path では所有値の Drop と zeroize によって破棄へ進める。panic message、debug 表示、エラー context には secret 本文を含めない。`enroll-spare` は secret を読む前に core dump を無効化する。

YubiKey の選択は serial 明示を基本にする。1 本だけ接続されている場合はその YubiKey を対象にする。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。利用者は `--primary-serial <serial>` と `--spare-serial <serial>` で対象を明示して再実行する。

primary の初期登録も同じ考え方にし、通常は次のコマンドだけを使う。

```sh
dotfiles secrets yubikey enroll-primary
```

`bitwarden-client-secret` を rotate した場合は、primary とすべての spare に対して次を実行する。

```sh
dotfiles secrets yubikey rotate-bws-token
```

`rotate-bws-token` は新しい token を一度だけ受け取り、更新ステップごとに 1 本だけ接続されている YubiKey または `--serial <serial>` で明示された YubiKey を更新する。各 YubiKey への保存後に ローカル確認 を行う。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。serial 未指定の対話実行で同一実行内の継続 prompt に進む場合も、次の更新前に対象 YubiKey だけを接続し、serial 解決後にその YubiKey 用の新しい PIV PIN を controlling TTY から読む。前の session の PIN を別 serial へ再利用しない。複数本を接続したまま進める場合は同一実行で継続せず、`--serial <serial>` を指定して 1 本ずつ実行する。利用者は 要約 に出た serial で primary とすべての spare が更新済みであることを確認する。BWS 接続確認は `verify-yubikey --check bws` 側の確認項目であり、ローカル保管 の検証とは別の確認として 要約 に残す。非対話実行では `--serial <serial>` と `--stdin` を指定して 1 本ずつ更新する。

外部サービスの登録状況は YubiKey PIV object からは検証できないため、`setup` / `put` / `status` の成功は GitHub、Bitwarden、Google、Apple などで 予備キー が登録済みであることを保証しない。

## 保存形式

YubiKey の data object には次の 2 種類だけを保存する。

- manifest: この YubiKey が dotfiles secret storage の対応 format を持つことを示す sentinel。
- secret blob: secret ごとに保存する envelope encryption 済み binary blob。

slot、object ID、secret id の対応は実装側の固定仕様であり、manifest を読んで動的に変えない。

Manifest は JSON とし、UTF-8 bytes を PIV data object に保存する。version 2 では slot `82` の DER-encoded SubjectPublicKeyInfo (SPKI) を非 secret metadata として持つ。これは別 process の `setup` / `put` が同じ non-exportable key を使うための公開鍵復元情報であり、private key、content encryption key、secret 本文は含まない。通常の `put` / token 更新では manifest の SPKI と、`yubikey` crate `0.9.0-pre.0` の [`piv::metadata`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs) が返す現在の slot 公開鍵がともに必須であり、両者を検証して一致させる。

version 1 manifest は低水準 `setup` を含む全現行 management write / enrollment command で停止し、暗黙 migration しない。通常の secret 書込みでは metadata または manifest SPKI の不在、不正、あるいは不一致を検出した時点で fail-closed とし、secret 入力・暗号化・object 書込みの前へ進まない。`clear` は custom object を物理 delete したと仮定しない。固定 `yubikey` crate の [`Transaction::save_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs) は NIST [SP 800-73pt2-5 §3.3.1 / Table 10](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-73pt2-5.pdf) の `PUT DATA` tag `53` replacement を送るため、clear は zero-length value で各 custom object の content を置換する。この成功 empty payload は SDK `NotFound` と同一ではない。`setup` の衝突検査は physical object presence を用い、`put` / `status` が報告する保存済み secret は non-empty encrypted blob を用いる。clear は新しい slot `82` key の生成戻り SPKI で空の version 2 manifest を確定するため、以後は initialized enrollment/provisioning を使い、`setup` を再実行しない。certificate、追加 PIV data object、slot `82` の署名操作は public key の保存・復元に使わない。

```json
{
  "version": 2,
  "app": "dotfiles.secret-recovery",
  "slot_public_key_spki": [48, 130, 1, 34]
}
```

`slot_public_key_spki` は DER byte 列の JSON array であり、上記の値は形式だけを示す短縮例である。

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
- `secret_id` は `1 = bitwarden-client-secret` を表す。
- secret 本文は secret ごとに生成するランダムな 32-byte content encryption key で AEAD 暗号化する。
- AES-256-GCM の nonce は 12 bytes、tag は 16 bytes に固定する。format 互換性を単純に保つため、nonce / tag の可変長 field は持たない。
- content encryption key は slot `82` の RSA public key で wrap し、平文では保存しない。
- secret を必要とする内部の復号経路は、PIV private key operation で content encryption key を unwrap し、AEAD で secret 本文を復号する。これは公開 `status` コマンドの処理ではない。
- AEAD additional data には `version`、`secret_id`、object ID、YubiKey serial を含め、blob の入れ替えを検出する。

保存時の blob が漏れた場合でも、slot `82` の private key operation を通せなければ `wrapped_key` は content encryption key に戻せない。復号時には host memory 上に content encryption key と平文 secret が一時的に現れるため、この方式は実行中 host の compromise を防ぐものではない。

平文 secret は `String` ではなく `ProtectedSecret` の byte buffer として扱う。ログ、エラー context、debug 表示に secret 本文や復号済み buffer を含めない。暗号化済み blob は平文 secret material の保護境界には含めず、diagnostics では byte 列を redaction する。

## 到達仕様のコマンド定義

この節は最終到達状態で提供するコマンド契約を定義する。現行実装の利用可否を示す手順書としては扱わない。

### `dotfiles secrets yubikey setup`

`setup` は低水準コマンドである。`enroll-primary` / `enroll-spare` はこの command を内部呼出しせず、同じ initialization lifecycle を各 use case の順序制御として実行する。`setup` の直接実行時は次を確認する。

- YubiKey が 1 本だけ接続されていればそれを対象にする。serial 未指定で複数本ある場合は一覧表示や選択へ進まず停止し、`--serial <serial>` を要求する。
- PIV application version が利用条件を満たすこと。
- slot `82` に既存 key / certificate がないこと。
- `0x005FFF16`（manifest）と `0x005FFF19`（`bitwarden-client-secret` blob）に、この機能が所有する既存 data object がないこと。

旧 format が利用した `0x005FFF17` / `0x005FFF18` は現行 format の予約 object ではない。現行仕様は
`bitwarden-client-secret` 一値だけを保存し、既存 device の不明な object をこの機能が所有・移行・clear
する根拠はないため、setup/status/clear/enroll の観測・変更対象に含めない。PIV custom data object の
`PUT DATA` は指定 object を完全置換する操作であり、固定 `yubikey` crate の `save_object` は任意 object の
delete API を公開しないことは [外部 SDK 統合の一次資料](external-sdk-evidence.md#yubikey-piv--yubikey-crate)
に記録した NIST / Yubico 一次資料を根拠とする。
- PIN retries が 0 ではないこと。
- management key authentication が可能なこと。factory-default management key 認証を暫定前提にせず、非既定 management key への切替、取得、注入が成立すること。

確認後、slot `82` に `yubikey` crate `0.9.0-pre.0` の [`piv::generate`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs) で専用鍵を生成し、manifest を保存する。既存の FIDO2 / OTP / OpenPGP / PIV credential は reset しない。衝突がある場合に自動削除や上書きはしない。

`setup` は current/new/confirmation の hidden TTY PIN を mutation 前に検証する。物理的に完全な fresh storage を初期化する enrollment は current PIN だけを hidden input し、同一 handle で current PIN VERIFY、既存 protected management-key authentication、PIV version・slot `82` key/certificate・manifest・全予約 object の完全 inspection を通過する。fresh と確定した場合だけ new/confirmation を検証し、保持済み current と三値をそろえて PIV application-wide PIN を `change_pin` する。PIN-free `status` はこの global mutation の許可根拠にしない。PIN 変更後は同じ handle で new PIN VERIFY、既存 protected management-key authentication、鍵生成、manifest 確定だけを行う。initialized enrollment は new/confirmation を読まず、設定済み PIN による management session を継続する。initialized storage の他の管理 command も設定済み PIN を hidden TTY からだけ受け取り、いずれも既に設定済みの PIN-protected management key を使う。固定 `yubikey` crate の `MgmKey::get_protected` は management metadata query と protected data read の error origin を public API で区別しないため、`NotFound` を B0 の protected-key 不在と断定しない。全 error で停止し、factory-default key の認証、`generate_for`、`set_protected` を自動実行しない。将来、別途承認した移行で `set_protected` を用いる場合も、その `Ok(())` は metadata 更新までの完全成功を保証しないため、fresh handle で PIN verify、protected key 取得、management authentication、metadata `default == Some(false)` をすべて確認できるまで slot / object 操作へ進まない。根拠は [外部 SDK 統合の一次資料](external-sdk-evidence.md#yubikey-piv--yubikey-crate) の固定 source 確認である。

複数本の YubiKey を運用する場合でも、`setup` は指定された 1 本だけを変更する。接続中の他 YubiKey へ同時に書き込む batch mode は実装しない。

### `dotfiles secrets yubikey put <name>`

`put` は低水準コマンドであり、通常の primary / spare 登録では `enroll-primary` / `enroll-spare` を使う。`<name>` は `bitwarden-client-secret` だけを許可する。それ以外は CLI parsing 後の検証で拒否する。

secret 入力は次の順で受け付ける。

- default: hidden prompt
- `--stdin`: stdin から 1 secret を読む

CLI 引数で secret 本文は受け取らない。`--stdin` は pipe または redirect された stdin だけを受け付け、TTY stdin では hidden prompt を使わせるため失敗させる。stdin 入力時も trailing newline は 1 つだけ除去し、それ以外の bytes は保持する。

保存先 object に既存 blob がある場合は `--force` がない限り停止する。`--force` がある場合も、manifest の app / version が一致しない場合は停止する。

### `dotfiles secrets yubikey status`

予約 bootstrap secret object を確認し、YubiKey に保存済みの名前を固定順で stdout に 1 行ずつ出力する。PIN 検証、management-key authentication、GET METADATA、touch、復号は行わず、secret 本文、ciphertext、wrapped key は出力しない。完全に空の予約 data object 領域は成功・空出力とする。正常な manifest がある状態では、保存済み bootstrap secret が任意の subset（空集合を含む）でも成功とし、存在する名前だけを出力する。manifest 欠落なのに予約 data object が残る状態、または不正 manifest は停止する。

`status` は slot `82` key/certificate の存在、metadata、manifest SPKI との一致を観測・主張しない。Yubico の GET METADATA 文書は command と返却 TLV を定義するが、PIN-protected management-key 運用を含む全 PIV state で無認証 status が安全に metadata を読めるという契約を定義しないためである。slot/manifest の SPKI 一致は管理 PIN を使う `setup` / `put` / enroll / rotate の preflight と、実際に unwrap する recovery/verify path が確認する。PIN prompt が発生した場合は成功扱いにせず、実装回帰として停止する。

### clear

clear は予約済み manifest / secret object と slot 82 certificate を消去し、slot 82 の専用 key を再生成して置換し、その生成済み SPKI を持つ空の version 2 manifest を同じ管理操作で確定する。既存 key を消去する操作は行わない。予約外の PIV 領域および FIDO2 / OTP / OpenPGP は対象外である。したがって clear 成功後に `setup` を実行してはならず、実行した場合は initialized state の衝突として停止する。`--yes` の確認後にだけ hidden TTY PIV PIN を受け取り、PIN-protected management key で認証する。

provisioning script は `clear` または `setup` の終了コード契約を利用せず、[spec の高水準 enrollment 契約](secret-recovery-spec.md#provisioning-source-の高水準-enrollment-契約) に従って primary を `enroll-primary`、spare を `enroll-spare` で登録する。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として登録する高水準コマンドである。これは BWS recovery credential の正本を最初に登録する操作なので、`bitwarden-client-secret` だけを hidden prompt から受け取る。token input より前に current PIN VERIFY、既存 protected management-key authentication、完全 inspection を通し、physical fresh または対象 object が logical empty で manifest/slot SPKI が一致する initialized version 2 だけを許可する。non-empty `bitwarden-client-secret`、version 1、manifestless partial state は token を読まず停止する。物理的に完全な fresh storage では token の取得、decode、document parse、全 field/domain validation を先に完了し、その後だけ new/confirmation input、PIN change、新 PIN authentication、initialize/store/finalize を連続して行う。入力不正によって PIN 変更済みの partial state を作らない。`clear` 後を含む initialized storage では設定済み PIN の management session だけを使い、PIN を変更せず token 保存へ進む。非対話または migration 用に限り `--stdin-json` を許可する。
`--stdin-json` を使う場合も PIV PIN は JSON payload に含めず、controlling TTY の hidden prompt だけから受け取る。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として登録する高水準コマンドである。primary から bootstrap secret を読み出して spare に再暗号化して保存し、利用者が低水準コマンドを並べたり secret を再入力したりしない高水準操作である。primary decrypt より前に spare の current PIN VERIFY、既存 protected management-key authentication、完全 inspection を通し、physical fresh または対象 object が logical empty で manifest/slot SPKI が一致する initialized version 2 だけを許可する。non-empty `bitwarden-client-secret`、version 1、manifestless partial state は primary secret を読まず停止する。spare が物理的に完全な fresh storage の場合は、primary read/decrypt または supplied JSON の decode、document parse、全 field/domain validation を先に完了し、その後だけ new/confirmation input、PIN change、新 PIN authentication、initialize/store/finalize を連続して行う。`clear` 後を含む initialized storage では設定済み PIN の management session のみを使う。

通常実行では、primary / spare の serial を解決し、spare の secret storage が enrollment 可能であることを事前検査してから、primary から `bitwarden-client-secret` を復号する。primary と spare を同時に接続して `--primary-serial <serial>` と `--spare-serial <serial>` を指定する。両方を接続すると複数 device となるため、serial を省略すると停止する。現行の prompt 実行は、primary の復号後に spare を接続する差し替え待ちや Enter prompt を提供しない。したがって、YubiKey を 1 本ずつしか接続できない環境では通常実行を完了できない。

`--stdin-json` は primary YubiKey が利用できないが、別経路で正本 secret を持っている場合の recovery / migration 用に限る。この場合だけ次の JSON を stdin から 1 回だけ受け取る。
`enroll-primary` と `enroll-spare --stdin-json` は bootstrap secret を JSON payload から通常どおり読み、PIV PIN は payload ではなく controlling TTY の hidden prompt から受け取る。

```json
{
  "bitwarden-client-secret": "secret"
}
```

入力 bytes をログや一時ファイルへ残さない。JSON parse 後の secret は `ProtectedSecret` として扱う。
JSON 文字列の値は JSON escape（`\n`、`\\`、`\uXXXX` など）を decode した bytes をそのまま保存し、行入力用の trailing newline 除去は適用しない。

`enroll-primary` / `enroll-spare` は成功時に secret 本文を出さず、次の 要約 だけを出力する。`role` は `primary` または `spare` のいずれかで、複数 spare の識別には YubiKey serial を使う。

```json
{
  "serial": 12345678,
  "role": "primary",
  "checks": {
    "setup": "ok",
    "bitwarden_client_secret": "ok",
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
    "bitwarden_client_secret": "ok",
    "local_storage": "ok"
  }
}
```

### `dotfiles secrets yubikey rotate-bws-token`

指定 YubiKey の `bitwarden-client-secret` だけを更新する。対話実行では新しい token を一度だけ読み取り、更新ステップごとに 1 本だけ接続されている YubiKey または `--serial <serial>` で明示された YubiKey を更新する。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。serial 未指定の対話実行で同一実行内の継続 prompt に進む場合も、次の更新前に対象 YubiKey だけを接続し、serial 解決後にその YubiKey 用の新しい PIV PIN を controlling TTY から読む。前 session の PIN を別 serial へ再利用しない。複数本を接続したまま進める場合は同一実行で継続せず、`--serial` を指定して 1 本ずつ実行する。primary とすべての spare は、出力 要約 の serial で対象全本の更新完了を確認する。非対話実行では `--serial` で 1 本だけを更新し、token は `--stdin` で受け取れる。更新前に ローカル保管 が復号可能な状態かを確認し、更新不能なら token を読まずに停止する。更新後は ローカル確認 を実行する。BWS 接続確認は ローカル保管 とは別の外部確認として扱う。

### `dotfiles secrets verify-yubikey`

挿さっている YubiKey が復旧入口として使えるか確認する。ローカル保管確認では、無対話 BWS recovery prerequisite である `bitwarden-client-secret` の manifest 整合・復号可能性だけを検証する。BWS だけを外部サービス確認項目として 要約 に含め、ローカル保管 の検証結果と区別する。

引数:

- `--serial <serial>`: 対象 YubiKey を指定する。serial 未指定時は、1 本だけ接続されていれば自動選択し、複数本接続時は一覧表示や選択へ進まず停止する。
- `--check bws`: `bitwarden-client-secret` で Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` を取得できることに加え、`gpg-secret-key-backup` envelope schema（`version` / `metadata` / `recipients` / `ciphertext`）と `metadata.primary_fingerprint` 形式（lowercase hex 40 文字、区切りなし）を検証し、接続中 YubiKey に一致する recipient（`yubikey_serial` と `public_key_fingerprint` の両一致）を照合して、unwrap なしで判定できる復旧可能性（少なくとも一致 recipient の存在）を確認する外部確認項目。secret 本文の平文化や unwrap は行わず、利用できない場合は失敗する。
- `verify-yubikey` は BWS recovery credential と BWS recovery object だけを確認する。Bitwarden Password Manager login、email、master password、YubiKey OTP、session は無対話復旧の入力・分岐・停止条件ではなく、`--check` / `--all` の対象に含めない。
- `--all`: ローカル保管確認と外部確認を含む全確認項目を実行する。指定した確認項目のいずれかが利用できない場合は失敗する。

外部確認を明示要求した場合（`--check bws`、`--all`）は、`skipped` を成功扱いにせず、外部確認が利用できないことを エラー として返す。引数なしの `verify-yubikey` は ローカル保管 のみ検証し、BWS 外部確認を `skipped` として 要約 に残す。

出力は 機械可読 な 要約 にし、状態値は `ok` と `skipped` を使う。表示文言は別層で扱い、JSON の状態値を翻訳しない。secret 本文、access token、Bitwarden session token は出力しない。

```json
{
  "serial": 12345678,
  "checks": {
    "local_storage": "ok",
    "bws": "skipped"
  }
}
```

ローカル保管確認 は次を確認する。

- manifest が存在し、app、version が期待値と一致する。
- `bitwarden-client-secret` の blob が存在する。
- blob の magic、version、algorithm、secret id、length field が妥当である。
- touch を経て `bitwarden-client-secret` を復号できる。
- 復号した secret は空ではない。

このコマンドは GitHub、Google、Apple など外部サービスの FIDO2 / passkey / U2F 登録状況を検証しない。

## 停止条件

- YubiKey が見つからない。
- 複数 YubiKey が接続され、`--serial` または用途別 serial option が指定されていない。
- 指定 serial の YubiKey が見つからない。
- PIV application が利用できない。
- PIN retries が 0。
- management key authentication に失敗する。
- slot `82` に既存 key または certificate がある。
- 使用予定 object ID に既存 data object がある。
- `enroll-primary` / `enroll-spare` の途中で setup、保存、ローカル確認 のいずれかに失敗した。
- `enroll-primary` / `enroll-spare` の途中失敗で setup 済みの部分状態（slot key、manifest、または secret object）が残ることがある。通常 enrollment はこれを暗黙 resume、`put --force` 相当の上書き、clear、slot overwrite/delete、再初期化、reset/PUK/default-key fallback へ写像せず停止する。特に manifest/object 空かつ slot `82` key/certificate ありの key-only state は、ownership marker がなく既存他用途 key と直前生成 key を一意に区別できないため、`setup` / enroll / `provision-bws-token` / `clear --yes` のすべてで固定 opaque failure と manual administrator escalation にする。管理者は別途 repository ownership と backup を確認して repository 外の手順を選び、CLI は同一 command の retry/resume や回復操作を自動実行しない。
- `enroll-spare` で primary と spare の serial が同一である。
- `enroll-spare` で primary / spare の serial 解決、または primary secret を読む前の spare storage 事前検査に失敗した。
- `enroll-spare` で平文 secret を読む前に core dump 無効化に失敗した。
- `rotate-bws-token` の同一実行内で同一 serial を重複更新しようとした。
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

単体テストと integration test は `secrets-internal-test-stub` feature で compile-time に隔離した internal backend stub だけで行う。物理 YubiKey / PC/SC reader は repository の test、agent 作業、通常の検証手順で使用しない。

- 許可 name と拒否 name。
- serial 未指定で複数 YubiKey がある場合に一覧表示や選択へ進まず停止すること。
- serial option 指定時に指定 YubiKey だけを対象にすること。
- manifest parse / serialize。
- manifest が slot / object mapping を持たない sentinel であること。
- 固定 object ID mapping。
- `put` の既存 blob 検出と `--force`。
- blob parser が trailing bytes と不正 length を拒否すること。
- `enroll-primary` が setup、`bitwarden-client-secret` の保存、ローカル確認を順に実行すること。
- `enroll-spare` が primary / spare serial 解決、spare storage 事前検査、primary 読み出し、spare setup、spare への再暗号化保存、ローカル確認を順に実行すること。
- `enroll-spare` が primary / spare 同一 serial と、primary secret 読み出し前の spare storage 事前検査失敗を拒否すること。
- `enroll-spare` が secret 読み込み前に core dump 無効化を実行すること。
- `enroll-spare` の エラー path で `ProtectedSecret` が zeroize されること。
- `rotate-bws-token` が `bitwarden-client-secret` だけを更新すること。
- `verify-yubikey` ローカル保管確認 の正常系と missing manifest / missing blob / decrypt failure。
- empty secret の拒否。
- blob magic / version / secret id mismatch の拒否。
- adapter エラー から利用者向け エラー context への変換。
- secret 本文が エラー 表示に含まれないこと。

物理 YubiKey を使う実機検証は、この repository の test、agent 作業、通常の検証手順の対象外である。将来 device-specific な確認を必要とする場合も、この文書を実行許可にせず、repository 外で人間が別途承認した手順だけを用いる。

## 参考

- Yubico PIV slots: https://docs.yubico.com/yesdk/users-manual/application-piv/slots.html
- Yubico PIV data objects: https://docs.yubico.com/yesdk/users-manual/application-piv/piv-objects.html
- Yubico PIV tool object/slot reference: https://docs.yubico.com/software/yubikey/tools/pivtool/piv-tool-command.html
- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
- Yubico PIV PIN-only mode, `PIN-protected` / `Management key authentication`: https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected , https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#management-key-authentication
