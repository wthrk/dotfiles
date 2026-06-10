# Bitwarden Secrets Manager 復旧設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / Bitwarden Secrets Manager](./secret-recovery-spec.md#bitwarden-secrets-manager) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets restore-gpg`、`dotfiles secrets restore-pass`、`dotfiles secrets verify-yubikey --check bws` から利用する Bitwarden Secrets Manager 取得経路である。

この文書は完成形の設計だけを扱う。

secret の保護境界、core dump 無効化、paging / memory lock / signal trap の扱い、外部処理が secret の借用または所有 plaintext buffer の move を要求する場合の実装方針は [Secret handling policy](./secret-handling.md) を正本とする。この文書では Bitwarden Secrets Manager の project / secret / API 境界だけを定義する。

application 層の use case orchestration test は `secrets-internal-test-stub` feature から切り離し、port trait 契約で駆動する。secret 値の test-only 観測が必要な場合は [Secret handling policy](./secret-handling.md) の `ProtectedSecret` test-only 最小アクセス許可に従い、production 経路の plaintext 取り出し API として扱わない。

internal backend stub を使う integration test の詳細規則は [Hexagonal Implementation Rules の internal backend stub の配置](../architecture/hexagonal-implementation-rules.md#internal-backend-stub-の配置) を正本とする。本設計で追加する方針は次の最小要件のみとする。

- `bitwarden` SDK と YubiKey SDK は datastore API として扱う。
- test 側は初期 datastore 定義だけを入力し、CLI 実行後は port ごとの最終 datastore 内容のみを `secrets-internal-test-stub` feature 専用の stdout sentinel observation で検証する。
- stdout observation は test-only の明示観測面であり、fixture/spec で与えたダミー secret 値を含めてよい。これは integration test が secret として保存した値の最終 datastore 反映を検証するためであり、production build/runtime の本物 secret 出力経路ではない。
- hidden temp file、output path file、共有 state file に secret 値を残してはならない。
- test 側は stub 内部 state schema や遷移 helper を持たない。BWS/YubiKey の port stub は独立させる。

## 目的と保護境界

この機能の目的は、新規マシン復旧で必要な機械向け secret を Bitwarden Secrets Manager から取得し、必要な復旧処理にだけ受け渡すことである。

保護するもの:

- `bws-access-token` を用いた取得セッション。
- credential（`bws-access-token` / `gpg-secret-key-backup`）の取得値。これらは認証・復号・署名・外部アクセス能力を与える秘密として保護する。
- 取得値（credential と、credential ではないが private な `password-store-remote` を含む）のログ、エラー、診断出力への漏えい。`password-store-remote` は credential ではないが private repository の所在を示す値であり、出力には漏らさない。

保護しないもの:

- 復旧処理で import / clone に渡した後の外部ツール内部状態。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- 復旧本線の取得経路は `BwsClientPort` 境界を通し、実 adapter は Bitwarden Secrets Manager Rust SDK（`bitwarden` crate）を呼び出す。
- `bw` CLI と `bws` CLI は Bitwarden Secrets Manager 取得経路では使わない。
- `bws-access-token` は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。
- SDK 呼び出しで access token の所有 plaintext buffer が必要になる境界は、`support/protection` 内の BWS 専用操作で完了させる。所有 plaintext buffer は `with_secret` 系借用境界内で SDK 呼び出し直前にだけ作り、public API として公開しない。
- credential（`bws-access-token` / `gpg-secret-key-backup`）と、credential ではないが private な `password-store-remote` は、いずれもログ、エラー本文、診断出力に含めない。`password-store-remote` は credential ではないが、private repository の所在を示す値のため出力には漏らさない。
- Bitwarden Secrets Manager 側の保存先 project は `dotfiles-secret-recovery` に固定する。
- project `dotfiles-secret-recovery` は `dotfiles` の provisioning command が登録用 token で見える project 一覧から exact match し、1 件なら既存 project を使い、0 件なら作成し、複数件なら停止する。`scripts/provision-secret-recovery-source.sh` はこの command 境界に委譲し、途中手動 gate で project 作成を要求しない。
- organization / machine account / service account の作成や特定 UI 画面名は、この repository の保存モデル前提にしない。実装とレビューは「保存先・名前・保存する値・読書き責務」を下表で照合し、Bitwarden 側 UI の名称変更や個人利用形態に依存した導線を正本にしない。
- `bws-access-token` は個人運用の Bitwarden Secrets Manager access token とする。provisioning 時の登録用 token と YubiKey に保存する復旧用 token は同一値にせず、YubiKey には復旧時に `dotfiles-secret-recovery` project の必要 secret を読める最小権限の token だけを保存する。
- Bitwarden Secrets Manager で扱う secret name は `gpg-secret-key-backup` と `password-store-remote` に固定する。
- `password-store-remote` の secret note には、provisioning command が登録時に使った access token の非機密 provenance marker（opaque token id）を保存する。
- `dotfiles secrets yubikey put bws-access-token` / `enroll-primary` / `enroll-spare` / `rotate-bws-token` は、候補 token で `password-store-remote` を参照してこの provenance marker を読み、一致時は provisioning token の再利用として停止し、marker 不在でも fail-closed で停止する。
- Bitwarden Secrets Manager の secret 値形式は secret ごとに固定し、`gpg-secret-key-backup` は YubiKey recipient 付き encrypted envelope（UTF-8 JSON）として保存する。
- Bitwarden Secrets Manager 側の project 一覧取得、project 作成、secret 作成・取得・一覧取得は、復旧本線と同じ `BwsClientPort` 境界の内側で扱う。project 名の 0件/1件/複数件判断は application/domain 側で行い、adapter は SDK API 呼び出しへ翻訳する。secret を扱う SDK API 呼び出しは `support/protection` 内の専用操作で完了させる。
- `verify-yubikey --check bws` は、上記 2 secret の取得可否に加え、`gpg-secret-key-backup` envelope schema（`version` / `metadata` / `recipients` / `ciphertext`）の検証、primary/spare の 2 recipient 以上の事前登録状態、接続中 YubiKey に一致する recipient 照合、unwrap を伴わずに判定できる復旧可能性を外部確認として検証する。1 recipient だけの envelope は、接続中 YubiKey に一致していても失敗扱いにする。

## Bitwarden Secrets Manager 配置

Bitwarden Secrets Manager には、この機能専用の project `dotfiles-secret-recovery` を 1 つ置く。この project は新規マシン復旧で機械的に取得する secret だけを置く境界であり、Web service password、TOTP、recovery code、Bitwarden Password Manager CLI credential、YubiKey に保存する bootstrap secret は置かない。`dotfiles` の provisioning command と `scripts/provision-secret-recovery-source.sh` は、project name から 1 件の project ID を解決できればそれを使い、0 件なら同名 project を作成し、複数件なら停止して secret create へ進まない。

この保存モデルは Bitwarden UI の画面名ではなく、project / secret / access token の関係で定義する。レビューでは organization、machine account、service account、特定 UI ラベルの有無を前提条件として要求していないかを確認し、要求している場合は誤った前提として差し戻す。

YubiKey に保存する `bws-access-token` は、復旧時に `dotfiles-secret-recovery` project の必要 secret を読むために使う。provisioning 時に BWS 登録へ使う token は YubiKey に保存せず、登録後に利用しない状態へ移行する。復旧経路で使う token が有効であることは `verify-yubikey --check bws` で確認する。

保存先と保存値は次の表を正本とする。

| 保存先 | 名前 | 保存する値 | 値の扱い | provisioning 時の書込み | 復旧時の読取り |
| --- | --- | --- | --- | --- | --- |
| BWS project | `dotfiles-secret-recovery` | 復旧用 secret だけを置く project。実装は lookup 後の project ID を同一性確認に使う。 | project 名/ID は secret ではない | `gpg-backup register` / `pass-remote register` が project name から ID を解決し、0 件なら作成する | `restore-gpg` / `restore-pass` / `verify-yubikey --check bws` が project name から ID を解決する |
| BWS secret | `gpg-secret-key-backup` | OpenPGP transferable secret key backup を DEK で暗号化し、primary と spare の各 YubiKey の PIV slot `82` recipient public key で DEK を wrap した envelope JSON。 | BWS 上の値は recipient 暗号化済み envelope。平文 GPG secret key は保存しない | `dotfiles secrets gpg-backup register` は設定済み password-store の `.gpg-id` が単一 primary に解決できる場合はその primary を使い、未設定の場合だけ使用可能な secret primary key の 0 件 / 1 件 / 複数件を一意解決する。BWS 同名 secret が 0 件の場合、現行 CLI だけでは primary/spare 2 recipient を同時取得できないため 1 recipient の新規 envelope 作成を拒否して停止する。既存 1 件なら envelope metadata の primary fingerprint、接続中 recipient、primary/spare の 2 recipient 以上が揃うことを確認する。不一致、1 recipient のみ、または複数件なら停止する。 | `dotfiles secrets restore-gpg` が取得し、接続中 YubiKey recipient で unwrap/decrypt する。`verify-yubikey --check bws` は schema/recipient/recoverability と 2 recipient 以上の事前登録状態を unwrap なしで確認する |
| BWS secret | `password-store-remote` | private `password-store` repository の SSH clone URL（`git@github.com:<owner>/<repo>.git`）。 | credential ではないが private repository の所在なので出力には漏らさない。BWS secret value として保存される | `dotfiles secrets pass-remote register` は既存 password-store origin があれば SSH / HTTPS GitHub URL を repository identity として許容し、BWS 登録値には SSH clone URL へ正規化した値を使う。既存 origin がない場合は登録対象 URL を `PasswordStoreRemoteInputPort` から受け取る。未登録なら作成する。既存 1 件は configured origin から導いた期待値と一致するときだけ使用し、configured origin が観測できない場合や不一致の場合は停止する。複数件も停止する。script から URL を argv / pipe / 環境変数で中継しない。 | `dotfiles secrets restore-pass` が取得し、URL 検証後に clone に使う。`verify-yubikey --check bws` は取得可否を確認する |
| YubiKey storage | `bws-access-token` | 復旧時に `dotfiles-secret-recovery` project の必要 secret を読める最小権限の Bitwarden Secrets Manager access token。 | YubiKey 内の encrypted blob として保存し、CLI 引数・ログ・環境変数へ出さない。provisioning の登録用 token と同一値にしない。保存時は `password-store-remote` note の provenance marker と候補 token の opaque token id を照合し、一致または marker 不在なら停止する。 | `dotfiles secrets yubikey enroll-primary` / `enroll-spare`、または `dotfiles secrets yubikey put bws-access-token` が CLI input port から保存する | `restore-gpg` / `restore-pass` / `verify-yubikey --check bws` が YubiKey から読み、BWS read に使う |
| Bitwarden Password Manager | 利用者 vault item | Web service passwords、passkeys、TOTP、recovery codes。`dotfiles` は固定 item 名を定義しない。 | Password Manager の vault データ。BWS project には置かない | `dotfiles` は保存 item を作成しない。利用者が Password Manager 側で管理する | `dotfiles secrets bw-login` は YubiKey の `bw-email` / `bw-password` と OTP で `bw login` / `bw unlock` を行う。BWS secret 取得には使わない |

取得時の lookup は project ID を基準にする。実装は access token で見える project 一覧から name `dotfiles-secret-recovery` を exact match し、対応する project ID を 1 つに解決する。その project ID に属する secret だけを列挙し、secret name `gpg-secret-key-backup` と `password-store-remote` を exact match する。project name は利用者向けの固定名だが、secret 所属判定と取得対象の同一性確認では project ID を正本として扱う。

上記 lookup の責務境界は、処理内容で判定する。固定 project / secret name の意味づけ、secret ID の一意解決、0件/複数件の failure 化、取得対象の過不足判定、setup 済みか何が不足しているかの判断、どの secret を必須とするか、`verify-yubikey --check bws` の外部確認 plan は、単に `support` へ移すだけでは規約適合にならない。実装は、各処理が既存規定上どの境界の責務かを判定し、規定済みの境界に置くこと。

`support/protection` に置けるのは、access token の平文借用、SDK 呼び出し直前の owned plaintext buffer 作成、SDK 呼び出しを安全に完了させる backend 実装依存の技術補助、repository 所有 buffer の zeroize、業務判断を含まない外部 API 型変換に限る。薄い port を保つために lookup 規則を adapter/support へ押し込むこと、adapter を薄くするために support へ逃がすことは禁止する。

BWS 取得経路とは別に、storage backend が暗号化された datastore を内包する場合、port は sealed blob や暗号方式ではなく datastore の保存・取得・状態確認 capability を公開する。暗号化・復号・sealed blob encode/decode・protection・zeroize・core dump 保護は storage backend 内部機能として `support/protection` に閉じてよい。ただし、その内部機能が BWS の固定 project / secret name、必須 secret、setup 状態、取得対象の一意性、0件/複数件 failure、`verify-yubikey --check bws` の検証計画を決める場合は不合格とする。

実装は次の場合に停止する。

- `dotfiles-secret-recovery` project に到達できない。
- name `dotfiles-secret-recovery` の project が複数見える。
- `gpg-secret-key-backup` または `password-store-remote` が存在しない。
- 同じ name の secret が複数見える。
- 取得した 2 secret が同一 project に属していない。
- 余分な BWS secret を復旧処理へ渡そうとしている。

## secret 取得契約

Bitwarden Secrets Manager で取得する対象と利用先は次のとおり。

- `gpg-secret-key-backup`: `dotfiles secrets restore-gpg` で GPG secret key import 入力に使う。
- `password-store-remote`: `dotfiles secrets restore-pass` で private `password-store` repository clone URL に使う。

取得時は以下を満たす。

- `bws-access-token` が空または取得不能なら即時停止する。
- 必須 secret のいずれかが未登録または取得不能なら停止する。
- 取得値は利用先処理へ直接渡し、恒久保存しない。

## secret 値形式

### `gpg-secret-key-backup`

値は UTF-8 JSON の encrypted envelope とする。`version: 1` を固定し、次の schema を必須とする。

- top-level: `version`（number, `1` 固定）/ `metadata` / `recipients` / `ciphertext`
- `metadata`: `primary_fingerprint`（lowercase hex 40 文字、区切りなし）/ `exported_at`（UTC RFC3339）/ `dek_alg`（`aes-256-gcm`）/ `recipient_kek_alg`（`rsa-oaep-sha256`）
- `ciphertext`: `nonce` / `body` / `tag` の base64 文字列。`nonce` は AES-GCM nonce 12 bytes、`body` は DEK で暗号化した OpenPGP backup bytes、`tag` は AES-GCM authentication tag 16 bytes とし、`tag` を `body` へ連結しない。
- `recipients`: schema 上は 1 件以上だが、復旧運用の到達条件として primary と spare の 2 件以上を必須にする。各要素は `yubikey_serial`（string, 10 進）/ `piv_slot`（string, `82` 固定）/ `public_key_fingerprint`（slot `82` 公開鍵の DER-encoded SubjectPublicKeyInfo を SHA-256 で digest した lowercase hex 64 文字、区切りなし）/ `wrapped_dek`（base64）

復旧処理は envelope 形式を検証し、接続中 YubiKey と一致する recipient で data encryption key を unwrap して復号済み backup を得た場合だけ GPG import へ進む。復号済み backup は 1 つの primary key を持つ OpenPGP transferable secret key を表し、その primary key に紐づく encryption / authentication / signing subkey を含む。復号済み backup から導出した primary fingerprint が `metadata.primary_fingerprint` と一致しない場合は停止する。複数 primary key を同じ secret に連結して保存しない。複数 primary key が必要になった場合は、この設計を更新して secret name と検証条件を追加する。

### `password-store-remote`

値は UTF-8 の 1 行文字列で、private `password-store` repository の SSH clone URL とする。この値は認証・復号・署名・外部アクセス能力を与える credential ではないため provisioning 入力では非秘匿として扱い、非表示入力・保護済み buffer を要さない。ただし private repository の所在を示す値であり、ログ・エラー本文・診断出力には含めない。BWS 保存時は未登録の場合だけ SDK の secret create 境界へ secret value として渡し、BWS 側の secret 暗号化保存対象にする。BWS secret value として許可する形式は `git@github.com:<owner>/<repo>.git` だけである。前後空白、改行、HTTPS URL、`ssh://` URL、local path、別 host は許可しない。既存 password-store origin が HTTPS GitHub URL の場合は、origin 自体は repository identity として許容し、BWS へ保存する前に CLI/application 側で SSH clone URL へ正規化する。

`<owner>` は GitHub user / organization 名として `[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?` に一致する値だけを許可する。`<repo>` は GitHub repository 名として `[A-Za-z0-9._-]+` に一致し、`.` または `..` ではなく、slash、colon、空白、制御文字を含まない値だけを許可する。`restore-pass` はこの値を clone URL として使う前に、1 行であること、`git@github.com:` で始まること、`.git` で終わること、`<owner>/<repo>` が上記制約を満たすことを検証する。

## 初期登録手順

`dotfiles` の BWS provisioning コマンド（`gpg-backup register` / `pass-remote register`）は、Bitwarden Secrets Manager の project `dotfiles-secret-recovery` に対して `password-store-remote` の登録と `gpg-secret-key-backup` の照合を行う。実行時に BWS 登録用 access token から見える同名 project を解決し、1 件なら使用、0 件なら作成、複数件なら停止する。BWS 登録用 access token は hidden prompt または pipe（stdin）から保護 buffer（`ProtectedSecret`）へ読み込み、argv・ログ・shell history・永続環境変数・永続一時ファイルへ残してはならない。この token は YubiKey に保存する復旧用 `bws-access-token` と同一値にしない。

provisioning 経路は実 secret を CLI 引数、shell history、ログ、共有 terminal、永続一時ファイルへ残してはならない。入力方式は secret ごとに次のとおり分ける。

- BWS 登録用 access token（実 credential）: hidden prompt（TTY）または pipe（stdin）から保護 buffer（`ProtectedSecret`）へ直接読み込む。値を argv へ載せる CLI 形式は採用しない。この token を YubiKey に保存しない。
- 復旧用 `bws-access-token`（実 credential）: `dotfiles-secret-recovery` project の必要 secret を読める最小権限の token を、`dotfiles secrets yubikey enroll-primary` / `enroll-spare` または `dotfiles secrets yubikey put bws-access-token` で YubiKey に保存する。provisioning script は token を stdin pipe で中継しない。保存後に `verify-yubikey --check bws` で復旧経路の BWS 読取を確認する。
- `pass-remote register` が `password-store-remote` を新規登録する場合、secret note に provisioning access token の provenance marker を併記する。復旧用 `bws-access-token` の保存・更新は、この marker が読めて候補 token と不一致である場合にだけ進める。
- `gpg-backup register`: `gpg-secret-key-backup` の値を provisioning 入力として受け取らない。既存 secret が 1 件だけ存在する前提で、primary fingerprint、接続中 YubiKey recipient、primary/spare の 2 recipient 以上条件を照合する。
- `password-store-remote`（private repository を指す clone URL であり credential ではない）: 認証情報（credential）ではないため provisioning 入力では非秘匿として扱い、非表示入力・保護済み buffer を要さない。`pass-remote register` は既存 password-store origin があれば SSH / HTTPS GitHub URL を repository identity として許容し、BWS secret value には SSH clone URL へ正規化した値を使う。既存 origin がない場合だけ controlling TTY の可視対話入力から値を取得し、値形式（`git@github.com:<owner>/<repo>.git`）を検証する。ただし private repository の所在を示す値であり、ログ・エラー本文・診断出力には含めない（決定事項参照）。

既存 secret は更新しない。`gpg-backup register` は同名 secret が 1 件だけ存在する場合に primary fingerprint、接続中 YubiKey recipient、2 recipient 以上の事前登録状態を検証して既存使用し、同名 secret が 0 件の場合でも 1 recipient の新規 envelope は作成せず停止する。`pass-remote register` は同名 secret が 1 件だけ存在する場合に既存 secret を取得し、configured origin が観測できるときはそこから導いた期待値と一致する場合だけ使用する。不一致なら stale 値として停止する。どちらも既存 1 件では作成や更新へ進まない。値の変更や recipient 追加が必要な場合は、Bitwarden Secrets Manager 側の状態を明示的に整理してから provisioning command を再実行する。

BWS access token を rotate する場合は、Bitwarden Secrets Manager 側で新 token を有効化した後、`dotfiles secrets yubikey rotate-bws-token` で primary と spare の全 YubiKey を更新する。古い token は全 YubiKey 更新後に失効させる。

## コマンド境界

### `dotfiles secrets restore-gpg`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` encrypted envelope を取得する。
- envelope 形式（`version` / `metadata` / `recipients` / `ciphertext`）を検証し、接続中 YubiKey と一致する recipient が存在しない場合は停止する。
- 接続中 YubiKey で data encryption key を unwrap して backup を復号し、復号済み backup から導出した primary fingerprint が envelope `metadata.primary_fingerprint` と一致することを確認する。
- primary fingerprint 一致を確認した場合のみ、復号済み backup を GPG import 処理へ渡して subkey 検証へ進む。

### `dotfiles secrets restore-pass`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `password-store-remote` を取得する。
- clone 前に URL 妥当性と `~/.password-store` 非存在を確認し、clone 処理へ渡す。

### `dotfiles secrets verify-yubikey --check bws`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` の取得可否を確認する。
- `gpg-secret-key-backup` は envelope schema（`version` / `metadata` / `recipients` / `ciphertext`）を検証し、`metadata.primary_fingerprint` 形式（lowercase hex 40 文字、区切りなし）を満たすことを確認する。
- `gpg-secret-key-backup` は primary/spare の 2 recipient 以上を確認したうえで、接続中 YubiKey と一致する recipient（`yubikey_serial` と `public_key_fingerprint` の両一致）を確認し、unwrap なしで判定できる復旧可能性がある場合だけ `ok` とする。1 recipient の envelope は primary 紛失時の復旧経路を満たさないため失敗扱いにする。secret 本文の露出や平文化は行わない。
- 引数なし `verify-yubikey` ではこの外部確認を自動実行せず、状態値 `skipped` を返す。

## 停止条件

- `bws-access-token` が YubiKey から取得できない。
- Bitwarden Secrets Manager SDK の初期化または認証に失敗する。
- `gpg-secret-key-backup` が取得できない。
- `password-store-remote` が取得できない。
- `verify-yubikey --check bws` で外部確認を完了できない。
