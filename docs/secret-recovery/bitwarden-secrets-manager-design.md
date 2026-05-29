# Bitwarden Secrets Manager 復旧設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / Bitwarden Secrets Manager](./secret-recovery-spec.md#bitwarden-secrets-manager) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets restore-gpg`、`dotfiles secrets restore-pass`、`dotfiles secrets verify-yubikey --check bws` から利用する Bitwarden Secrets Manager 取得経路である。

この文書は完成形の設計だけを扱う。

secret の保護境界、core dump 無効化、paging / memory lock / signal trap の扱い、外部処理が secret の借用または所有 plaintext buffer の move を要求する場合の実装方針は [Secret handling policy](./secret-handling.md) を正本とする。この文書では Bitwarden Secrets Manager の project / secret / API 境界だけを定義する。

application 層の use case orchestration test は `secrets-internal-test-stub` feature から切り離し、port trait 契約で駆動する。secret 値の test-only 観測が必要な場合は [Secret handling policy](./secret-handling.md) の `ProtectedSecret` test-only 最小アクセス許可に従い、production 経路の plaintext 取り出し API として扱わない。

## 目的と保護境界

この機能の目的は、新規マシン復旧で必要な機械向け secret を Bitwarden Secrets Manager から取得し、必要な復旧処理にだけ受け渡すことである。

保護するもの:

- `bws-access-token` を用いた取得セッション。
- `gpg-secret-key-backup` と `password-store-remote` の取得値。
- 取得値のログ、エラー、診断出力への漏えい。

保護しないもの:

- 復旧処理で import / clone に渡した後の外部ツール内部状態。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- 復旧本線の取得経路は `BwsClientPort` 境界を通し、実 adapter は Bitwarden Secrets Manager Rust SDK（`bitwarden` crate）を呼び出す。
- `bw` CLI と `bws` CLI は Bitwarden Secrets Manager 取得経路では使わない。
- `bws-access-token` は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。
- SDK 呼び出しで access token の所有 plaintext buffer が必要になる境界は、`support/protection` 内の BWS 専用操作で完了させる。所有 plaintext buffer は `with_secret` 系借用境界内で SDK 呼び出し直前にだけ作り、public API として公開しない。
- `bws-access-token`、`gpg-secret-key-backup`、`password-store-remote` はログ、エラー本文、診断出力に含めない。
- Bitwarden Secrets Manager 側の保存先 project は `dotfiles-secret-recovery` に固定する。
- `bws-access-token` は machine account `dotfiles-secret-recovery-reader` の token とし、`dotfiles-secret-recovery` project への読み取りだけを許可する。
- Bitwarden Secrets Manager で扱う secret name は `gpg-secret-key-backup` と `password-store-remote` に固定する。
- Bitwarden Secrets Manager の secret 値は JSON envelope や独自 metadata を持たず、下記の値形式をそのまま保存する。
- Bitwarden Secrets Manager 側の project / secret 作成・更新・一覧取得は、復旧本線と同じ `BwsClientPort` 境界の内側で扱う。application/domain/port 契約は変更せず、secret を扱う SDK API 呼び出しは `support/protection` 内の専用操作で完了させる。
- `verify-yubikey --check bws` は、上記 2 secret を取得できることを外部確認として検証する。

## Bitwarden Secrets Manager 配置

Bitwarden Secrets Manager には、この機能専用の project `dotfiles-secret-recovery` を 1 つ作る。この project は新規マシン復旧で機械的に取得する secret だけを置く境界であり、Web service password、TOTP、recovery code、Bitwarden Password Manager CLI credential、YubiKey に保存する bootstrap secret は置かない。

machine account は `dotfiles-secret-recovery-reader` を使う。この machine account は `dotfiles-secret-recovery` project の secret 読み取りだけを許可し、secret 作成、更新、削除、他 project 参照、organization 全体参照を許可しない。YubiKey に保存する `bws-access-token` は、この machine account の access token だけである。

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

値は UTF-8 の ASCII-armored OpenPGP secret key block とする。値全体が `gpg --export-secret-keys --armor` 相当の出力であり、先頭に `-----BEGIN PGP PRIVATE KEY BLOCK-----`、末尾に `-----END PGP PRIVATE KEY BLOCK-----` を含む。base64 で再包装しない。JSON、TOML、YAML、複数 field を持つ wrapper、圧縮 archive、暗号化済み archive は使わない。

復旧処理はこの値を GPG import 入力としてそのまま渡す。値は 1 つの primary key を持つ OpenPGP transferable secret key を表し、その primary key に紐づく encryption / authentication / signing subkey を含む。複数 primary key を同じ secret に連結して保存しない。複数 primary key が必要になった場合は、この設計を更新して secret name と検証条件を追加する。

### `password-store-remote`

値は UTF-8 の 1 行文字列で、private `password-store` repository の SSH clone URL とする。許可する形式は `git@github.com:<owner>/<repo>.git` だけである。前後空白、改行、HTTPS URL、`ssh://` URL、local path、別 host は許可しない。

`<owner>` は GitHub user / organization 名として `[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?` に一致する値だけを許可する。`<repo>` は GitHub repository 名として `[A-Za-z0-9._-]+` に一致し、`.` または `..` ではなく、slash、colon、空白、制御文字を含まない値だけを許可する。`restore-pass` はこの値を clone URL として使う前に、1 行であること、`git@github.com:` で始まること、`.git` で終わること、`<owner>/<repo>` が上記制約を満たすことを検証する。

## 初期登録手順

Bitwarden Secrets Manager 側の project / secret 初期登録は、`dotfiles` の BWS provisioning 経路で自動化してよい。この経路は復旧本線ではなく管理 plane の bootstrap であり、公式 `bitwarden` Rust SDK の project / secret create・update・list API を使う。`bws` CLI は復旧本線・provisioning のどちらでも利用しない。provisioning 用 access token は初期登録後に失効させる。復旧本線で YubiKey に保存する token は、読み取り専用の `dotfiles-secret-recovery-reader` token だけである。

machine account `dotfiles-secret-recovery-reader` の作成、project `dotfiles-secret-recovery` への read-only 割当、reader access token の発行は `dotfiles` provisioning の自動化対象外とする。これらは Bitwarden 管理画面または Bitwarden が公式に提供する machine account / access token 管理 API で行う。`dotfiles` provisioning は、発行済み reader token を YubiKey へ保存し、その token で BWS secret を取得できることを検証するだけである。

1. provisioning 用 access token を使い、公式 `bitwarden` Rust SDK で organization ID を 1 つ指定し、その organization 内の project 一覧を取得する。name `dotfiles-secret-recovery` が存在しなければ、その organization ID の project として作る。既に存在する場合は project ID を確認し、同一 organization 内に同名 project が複数ないことを確認する。
2. provisioning 用 access token を使い、公式 `bitwarden` Rust SDK で secret `gpg-secret-key-backup` を作成または更新し、ASCII-armored OpenPGP secret key block を値として保存する。
3. provisioning 用 access token を使い、公式 `bitwarden` Rust SDK で secret `password-store-remote` を作成または更新し、`git@github.com:<owner>/<repo>.git` 形式の private `password-store` repository URL を値として保存する。
4. 手動または公式管理 API で machine account `dotfiles-secret-recovery-reader` を作り、project `dotfiles-secret-recovery` への読み取りだけを許可する。
5. 手動または公式管理 API で `dotfiles-secret-recovery-reader` の access token を発行する。この token は発行時にだけ表示され、後から再取得できないため、直後に `dotfiles secrets yubikey enroll-primary` / `enroll-spare` の `bws-access-token` として YubiKey に保存する。
6. provisioning 用 access token を失効させる。
7. `dotfiles secrets verify-yubikey --check bws` を実行し、`dotfiles-secret-recovery-reader` token で `gpg-secret-key-backup` と `password-store-remote` を取得できることを確認する。

provisioning 経路は実 secret を CLI 引数、shell history、ログ、共有 terminal、永続一時ファイルへ残してはならない。`gpg-secret-key-backup` と `password-store-remote` の入力は hidden prompt、pipe、または保護済み buffer へ直接読み込む。値を argv へ載せる CLI 形式は採用しない。

既存 secret を更新する場合、provisioning 経路は更新前に以下を検証し、満たせない場合は停止する。

- 更新対象 secret が project name `dotfiles-secret-recovery` から解決した project ID に属している。
- 同じ secret name が同一 project 内に複数存在しない。
- 更新後の値が本設計の値形式を満たす。
- 対話実行では上書き対象 secret name と project name を表示し、利用者の明示確認を得てから更新する。
- 非対話実行では明示的な上書き許可 option が指定されている場合だけ更新する。

access token を rotate した場合は、Bitwarden Secrets Manager 側で新 token を有効化した後、`dotfiles secrets yubikey rotate-bws-token` で primary と spare の全 YubiKey を更新する。古い token は全 YubiKey 更新後に失効させる。

## コマンド境界

### `dotfiles secrets restore-gpg`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` を取得する。
- 取得値を GPG import 処理へ渡し、subkey 検証へ進む。

### `dotfiles secrets restore-pass`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `password-store-remote` を取得する。
- clone 前に URL 妥当性と `~/.password-store` 非存在を確認し、clone 処理へ渡す。

### `dotfiles secrets verify-yubikey --check bws`

- YubiKey から `bws-access-token` を取得する。
- Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` の取得可否を確認する。
- 引数なし `verify-yubikey` ではこの外部確認を自動実行せず、状態値 `skipped` を返す。

## 停止条件

- `bws-access-token` が YubiKey から取得できない。
- Bitwarden Secrets Manager SDK の初期化または認証に失敗する。
- `gpg-secret-key-backup` が取得できない。
- `password-store-remote` が取得できない。
- `verify-yubikey --check bws` で外部確認を完了できない。
