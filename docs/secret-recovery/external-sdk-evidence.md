# 外部 SDK 統合の一次資料

この文書は secret-recovery が直接統合する外部 SDK / device / service の公式フローを、実装・レビューから参照できる正本としてまとめる。一般規則（資料を実際に開くこと、全エラー面を確認すること、根拠のない fallback を禁止すること）は [docs-governance の外部 SDK / crate の利用根拠](../docs-governance.md#外部-sdk--crate-の利用根拠) を正本とし、この文書はそれを置換しない。

各リンクは 2026-07-21 に直接確認した。dependency version は root `Cargo.toml` / `Cargo.lock` に固定されたものを対象にする。SDK version を更新する変更では、該当 SDK の全体資料、API/source、sample、error 資料を再読し、この文書と該当コードの doc comment を同じ変更で更新する。

## 共通の実装・レビュー手順

外部 SDK 呼び出しを新設・変更・レビューする時は、次の順に元資料を直接開いて確認する。

1. vendor の product / architecture / operating-flow 文書を読み、認証主体、権限境界、状態を確定する。
2. vendor の quick start または公式 sample を読み、初期化から cleanup までの API 順序を確定する。
3. 使用する version の API documentation または versioned upstream source で、request / response data model と全 error surface を確認する。
4. 各 error 値について、一次資料が許す遷移だけを実装する。資料に意味がなければ opaque failure として source error を保持して伝播し、retry、default、空値化、別エラーへの写像、成功扱い、状態変更をしない。
5. 実装コメントには本書の該当節と API symbol を、レビューには実際に読んだ URL・節・source location を記す。

実機観測、検索要約、error text、二次解説は、この判断の根拠にしない。

## YubiKey PIV / `yubikey` crate

### 読む一次資料

- [Yubico Product Documentation](https://docs.yubico.com/) — product / SDK / tool documentation の公式入口。PIV integration の出発点として Technical Manual と tool guide の両方を辿る。
- [YubiKey Technical Manual](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/index.html) — YubiKey applications / protocols の全体構成と firmware 差分の入口。
- [Smart Card (PIV Compatible)](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/yk5-apps-piv.html) — PIV application、default management key の firmware 別 algorithm、slot 82--95 の用途、PIV metadata（firmware 5.3.0+）と返却 TLV を定義する。
- [YubiKey Manager PIV Commands](https://docs.yubico.com/software/yubikey/tools/ykman/PIV_Commands.html) — management key が key generation を含む管理操作を保護すること、key generation の public-key output、PIN / touch policy を示す公式 sample / operation guide。
- [NIST SP 800-73pt2-5](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-73pt2-5.pdf) §3.1.2 / §3.3.1、Table 10 — `GET DATA` は成功時に data value を含む tag `53` TLV を返し、`6A82` は data object not found、`PUT DATA` は一つの data object の content を完全置換し、通常 object の replacement data は tag `53` であることを定義する。
- [Yubico PIV Tool: Read/Write Objects](https://developers.yubico.com/yubico-piv-tool/Actions/read_write_objects.html) — raw PIV object の read/write と write 時の management-key authentication を示す Yubico 公式 tool sample。任意 object の delete operation はこの API 面にない。
- [`yubikey` 0.9.0-pre.0 README](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/README.md) — 本 crate が PIV host-side driver であり `untested` feature は未検証であることを明記する upstream source。
- [`MgmKey::get_default` / `MgmKey::get_protected` / `MgmKey::set_protected`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs) と [`YubiKey::verify_pin` / `YubiKey::authenticate`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs) — PIN-protected management-key の取得、B0 bootstrap、PIN verification、authentication API と error return を確認する固定 crate source。
- [`piv::generate`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs)、[`piv::metadata`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs)、[`Error`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs) — key generation、metadata、crate が公開する `#[non_exhaustive] Error` 全面の version 固定 source。
- [`Transaction::fetch_object` / `Transaction::save_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs) — `fetch_object` は status word `6A82` だけを `Error::NotFound` に写像し、成功 GET DATA の tag `53` value を空のまま返す。`save_object` は `PUT DATA` の tag `53` を構成するだけで、arbitrary data object delete API は公開しない。

### 採用フローと注意点

1. PC/SC context で reader を列挙し、serial を決めてから `YubiKey::open_by_serial` で対象 device を開く。serial 未指定で複数候補を自動選択しないのは repository の device-selection policy であり、SDK の error 意味付けではない。
2. PIV 管理操作は management key により保護される。通常は hidden TTY PIN を `YubiKey::verify_pin` へ渡してから `MgmKey::get_protected` と `YubiKey::authenticate` を同じ fresh handle で行う。`MgmKey::get_default` は firmware-compatible な**既定** key を返すだけである。B0 の完全 bootstrap 候補は、management-slot metadata の `default == Some(true)` と `get_protected` が正確に `Error::NotFound` を返す場合だけ default key で認証し、ランダム key を `set_protected` する。PIN error、`GenericError` を含むその他 error、metadata の未知値では fallback せず停止する。`set_protected` の後は handle を reopen し、protected key の取得・authentication・metadata `default == Some(false)` を確認できるまで slot/object 操作へ進まない。
3. key generation は `piv::generate(slot, algorithm, pin_policy, touch_policy)` を使い、返った public key を SPKI へ正規化する。slot 82--95 は vendor 文書上 retired key-management slot であるため、repository が slot 82 を専用領域として使うことは domain design であり、他用途の key を自動削除・上書きする根拠にはならない。
4. metadata は firmware 5.3.0 以上で公開鍵、policy、origin 等を返す。manifest と metadata の public key が欠落・不正・不一致なら、secret の入力、暗号化、PIV object 書込みへ進まない。
5. `yubikey` crate は `untested` feature を未検証と明記する。repository の test、agent 作業、通常の検証では物理 YubiKey を使わず、feature 隔離 stub だけを使う。device-specific な確認が将来必要でも、この文書を実行許可にせず、repository 外で人間が別途承認した手順だけを用いる。
6. custom data object を clear する時は、固定 crate が提供する `save_object(object_id, &[])` による zero-length tag `53` replacement を使う。これは SDK `NotFound` を返す物理 delete ではない。adapter は successful empty payload を physical object presence として保持し、`setup` の collision 判定に渡す。一方、`put` の overwrite guard と `status` の saved-secret report は non-empty encrypted blob の有無を別に判定する。`clear` は空 replacement 後に slot 82 key を生成してその SPKI を含む v2 manifest を必ず確定するため、次 process の `setup` は initialized storage として no-op になる。

### Error handling

`yubikey::Error` は `#[non_exhaustive]` であり、`AuthenticationError`、`NotFound`、`WrongPin`、PC/SC error 等の値を公開する。adapter は `fetch_object` / metadata の**戻り値が正確に** `Error::NotFound` の場合だけ、対象 PIV object が未保存であるという port 表現へ翻訳する。`GenericError` を含むその他すべての SDK error は、存在しない slot / object、認証済み、retry 可、既存 object、成功相当のいずれにも分類せず、context を加える場合にも source error を保持して失敗として返す。

`piv::generate` は slot の既存 key を保護する存在確認 API ではない。setup / clear の上書き可否は、generate 前の inspection と domain intent で決め、SDK error 後に「既存だから成功」と再解釈しない。`fetch_object` の `Ok(empty)` も `NotFound`、未使用、または成功相当には再分類しない。上記の明示した storage-flow 上の logical blob 判定以外で、empty payload を resource absence へ写像してはならない。

## Bitwarden Secrets Manager SDK

### 読む一次資料

- [Bitwarden Help Center](https://bitwarden.com/help/) — Password Manager と Secrets Manager の product documentation 入口。両者を同一の vault flow とみなさない。
- [Developer Quick Start](https://bitwarden.com/help/developer-quick-start/) — Secrets Manager の product-level flow。SDK 利用前に Secrets Manager Quick Start を読むよう導く。
- [Secrets Manager SDK](https://bitwarden.com/help/secrets-manager-sdk/) — access token で認証し、single secret / project 内 secret / project の取得・list を行う SDK の用途と操作面。
- [Machine Accounts](https://bitwarden.com/help/machine-accounts/) — access token は machine account に属し、project assignment と `Can read` / `Can read, write` が programmatic read / create / edit の権限境界であることを定義する。
- [Secrets](https://bitwarden.com/help/secrets/) — secret は一つの project にだけ所属し、machine account への project access が programmatic secret access を与えることを定義する。
- [`bitwarden` 2.1.0 `lib.rs`](https://docs.rs/crate/bitwarden/2.1.0/source/src/lib.rs) — `Client::new`、`AccessTokenLoginRequest`、`auth().login_access_token`、`secrets().list` の official crate sample。
- [`bitwarden-sm` 3.0.0 `SecretsManagerClient::get_access_token_organization`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client.rs)、[`SecretsClient`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client_secrets.rs)、[`ProjectsClient`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client_projects.rs) — 実際に `bitwarden` 2.1.0 が公開する client / access-token organization の `Option` 戻り値 / `get` / `list_by_project` / `create` / `update` API と各 `Result<_, SecretsManagerError>`。
- [`SecretsManagerError`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/error.rs) と [`SecretCreateRequest`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/create.rs) / [`SecretPutRequest`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/secrets/update.rs) — validation / crypto / chrono / API / missing-field error 面と request data model。
- [Server Geographies](https://bitwarden.com/help/server-geographies/) — EU region の `identityUri=https://identity.bitwarden.eu` と `apiUri=https://api.bitwarden.eu`。本 repository の BWS client は EU tenant のため、この組を `ClientSettings` に固定する。US / self-hosted tenant へ無根拠に fallback しない。

### 採用フローと注意点

1. `Client::new(Some(ClientSettings))` の後、repository 所有かつ zeroize する `AccessTokenLoginRequest` で `login_access_token` を完了させる。`bitwarden` 2.1.0 の crate sample が `ClientSettings` と `DeviceType::SDK` を指定するため、repository も同じ device type を使う。access token は credential bytes として**trim・改行除去・case 変換をせず** request に渡す。SDK / vendor 資料はその正規化を定義しないためである。access token は machine account の project permissions を越える権限を与えない。
2. 認証済み client から token に対応する organization ID を取得し、`ProjectsClient::list` で候補を取得する。`SecretsManagerClient::get_access_token_organization` の `None` は SDK が scope ID を返さなかった failure であり、default organization / 空 UUID / 別 project で補完しない。application/domain が project name の一意解決を行う。
3. 解決済み project ID だけを `SecretsClient::list_by_project` へ渡し、application/domain が secret key の一意解決を行う。全 organization の secret を探索して別 project から補完しない。
4. value が必要な時は `SecretsClient::get` を呼び、protection 境界内で解析または `Zeroizing<String>` に移す。value と access token は CLI 引数、ログ、error context、恒久文書へ出さない。
5. create は access-token organization と選択済み project ID を request に入れる。update は直前に `get` した response の `organization_id` と `project_id` を確認し、response の project が caller の解決済み project と一致し、かつ domain guard が revision/value を照合した時だけ `update` する。response に `project_id` がない、または project が変わった場合は failure とし、caller の project ID を補う fallback をしない。

### Error handling

使用 API は `Result<_, SecretsManagerError>` を返す。version 3.0.0 source の `SecretsManagerError` は `Validation`、`Crypto`、`Chrono`、`Api`、`MissingField` を定義する。adapter はこれらを rate limit、not found、permission denied、transient、success 等へ文字列で再分類しない。`anyhow::Context` は operation 名を加えるだけで source error を保持し、retry / fallback / empty result / default project assignment を行わない。

## Bitwarden Password Manager CLI (`bw`)

この製品面は Secrets Manager SDK とは別である。repository は `bw` child process や Password Manager login command を実装せず、SDK の project / machine-account 認証に代用しない。無対話復旧、`restore-gpg`、`restore-pass`、`verify-yubikey --all` の flow に混在させない。

### 読む一次資料

- [Password Manager CLI](https://bitwarden.com/help/cli/) — CLI の公式入口、server configuration、session の前提。
- [CLI Authentication via API Key](https://bitwarden.com/help/personal-api-key/) — API key は automated workflow 向けで、vault data を扱うには通常 `unlock` が続き、master password の代替ではない。
- [Password Manager CLI: Login / Unlock](https://bitwarden.com/help/cli/#log-in) — email/master-password login は session key を同時に生成し、API key / SSO login のみ後続 `unlock` を要するという状態遷移を定義する。
- [Password Manager CLI: global options](https://bitwarden.com/help/cli/#global-options) — `--raw` は descriptive message でなく raw output を返す global option。

### 採用フローと error handling

1. 公式 CLI surface は two-factor code を `--code <code>` command argument として定義する。repository の secret policy は OTP を argv、environment、stdout/stderr、temporary file へ渡さないため、この CLI flow を repository command に採用しない。
2. API key flow（`bw login --apikey`）と SSO flow（`bw login --sso`）は、公式が後続 `unlock` を必要とする別の状態遷移である。repository は client ID / client secret / browser SSO input を受け付けず、email/master-password flow と fallback・混在・推測分岐しない。
3. adapter は `bw` child process を起動しないため stdout/stderr/session credential を取得・検証・出力しない。

## GPGME / Sequoia OpenPGP

`gpgme` 0.11.0 と `sequoia-openpgp` 1.21.2 は GPG keyring / OpenPGP packet の adapter 実装で直接利用する。全体フローは [GPGME manual](https://www.gnupg.org/documentation/manuals/gpgme/) と [Sequoia OpenPGP book](https://book.sequoia-pgp.org/) を、API と error は [gpgme 0.11.0 docs](https://docs.rs/gpgme/0.11.0/gpgme/) と [sequoia-openpgp 1.21.2 docs](https://docs.rs/sequoia-openpgp/1.21.2/sequoia_openpgp/) を読む。

`gpgme::Error::NO_SECKEY` は GPGME の [Error Codes](https://www.gnupg.org/documentation/manuals/gpgme/Error-Codes.html) が secret key 不在として定義するため、`secret_key_exists` / recipient availability の false へだけ翻訳できる。EOF、I/O error、parse error、その他の GPGME / Sequoia error は secret key 不在へ写像せず伝播する。該当 API の変更では key import / export / keygrip / gpg-agent SSH flow も [gnupg-ssh-design.md](gnupg-ssh-design.md) と併読する。

## libgit2 / `git2`

`git2` 0.20.4 は private `password-store` の SSH-agent clone に直接利用する。全体の clone / callback flow は [libgit2 clone API](https://libgit2.org/docs/guides/101-samples/), [git2 `RepoBuilder`](https://docs.rs/git2/0.20.4/git2/build/struct.RepoBuilder.html), [git2 `RemoteCallbacks`](https://docs.rs/git2/0.20.4/git2/struct.RemoteCallbacks.html), [git2 `Cred::ssh_key_from_agent`](https://docs.rs/git2/0.20.4/git2/struct.Cred.html#method.ssh_key_from_agent) を読む。

`RemoteCallbacks` の credentials / certificate callback が返す error は clone failure として伝播する。SSH agent fallback、host-key 検証の省略、既存 `~/.password-store` の上書き、clone error を成功扱いすることは許可しない。repository 固有の socket / GitHub host-key / destination policy は [gnupg-ssh-design.md](gnupg-ssh-design.md) と実装の出典コメントを正本とする。

## Rust support crate（secret-recovery 直接利用）

以下は `rust/dotfiles-secrets/Cargo.toml` が直接利用する crate の固定版、使用 API、一次資料である。各 source URL は docs.rs が公開する当該 version の upstream source であり、`Cargo.lock` の解決 version と照合する。これらは外部 service の product flow を持たない library だが、暗号・乱数・メモリ保護・codec・OS I/O の各 API が成功・失敗時に何を返すかは該当 source を読んで確認する。資料に個別 error の意味がない場合は、docs-governance に従い source error を opaque failure として伝播する。

| crate（lock version） | 使用 API / flow | 直接確認する一次資料 | error / state の扱い |
| --- | --- | --- | --- |
| `aes-gcm` 0.10.3 | `Aes256Gcm::new` → `AeadInPlace::encrypt_in_place` / `decrypt_in_place` | [`aes-gcm` source](https://docs.rs/crate/aes-gcm/0.10.3/source/src/lib.rs) と [`AeadInPlace`](https://docs.rs/aead/0.5.2/aead/trait.AeadInPlace.html) | decrypt failure を key/nonce/ciphertext のいずれかへ再分類せず、復号失敗として停止する。 |
| `rsa` 0.9.10 | `Oaep::new`、`RsaPublicKey::encrypt`、`RsaPrivateKey::decrypt`、PKCS#8/SPKI decode | [`rsa` 0.9.10 source](https://docs.rs/crate/rsa/0.9.10/source/src/lib.rs) と [`Oaep`](https://docs.rs/rsa/0.9.10/rsa/struct.Oaep.html) | encrypt/decrypt/DER error を鍵不在・recipient 不在へ写像しない。 |
| `rand` 0.9.4 / `rand_core` 0.6.4 | `RngCore::fill_bytes`、`OsRng` | [`rand` 0.9.4 `RngCore`](https://docs.rs/rand/0.9.4/rand/trait.RngCore.html)、[`rand_core` 0.6.4 `OsRng`](https://docs.rs/rand_core/0.6.4/rand_core/struct.OsRng.html) | OS entropy 取得失敗を固定値・再利用値で補わず停止する。 |
| `rand` 0.10.1 / `rand_core` 0.10.1 / `getrandom` 0.4.2 | `rngs::SysRng` を `MgmKey::generate_for` へ渡し、PIN-protected management key を生成する | [`rand` 0.10.1 `SysRng`](https://docs.rs/rand/0.10.1/rand/rngs/struct.SysRng.html)、[`rand` 0.10.1 source (`rngs` re-export)](https://docs.rs/crate/rand/0.10.1/source/src/rngs/mod.rs)、[`yubikey` 0.9.0-pre.0 `MgmKey::generate_for`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs) | `SysRng` は fallible `TryRng`。ただし upstream `generate_for` は RNG failure を `yubikey::Error::KeyError` へ変換するため、adapter は entropy / device / key state を推測せずこの SDK error を opaque に停止する。固定値、再利用値、retry は使わない。 |
| `zeroize` 1.8.2 | `Zeroize` / `Zeroizing` の所有 buffer lifetime | [`zeroize` 1.8.2 source](https://docs.rs/crate/zeroize/1.8.2/source/src/lib.rs) | Drop による zeroization は secret lifetime の補助であり、外部 API error の握りつぶし理由にしない。 |
| `bincode` 2.0.1 | `config::standard`、serde encode/decode | [`bincode` 2.0.1 source](https://docs.rs/crate/bincode/2.0.1/source/src/lib.rs) | decode error は破損・旧形式・攻撃を推測せず、storage decode failure として停止する。 |
| `serde` 1.0.228 / `serde_json` 1.0.149 | derive、`from_slice` / `to_vec` / `json!` | [`serde` 1.0.228 source](https://docs.rs/crate/serde/1.0.228/source/src/lib.rs)、[`serde_json` 1.0.149 source](https://docs.rs/crate/serde_json/1.0.149/source/src/lib.rs) | serialization / parse error は入力値や remote response の意味を推測せず伝播する。 |
| `uuid` 1.23.1 | `Uuid::parse_str` / `FromStr` | [`uuid` 1.23.1 source](https://docs.rs/crate/uuid/1.23.1/source/src/lib.rs) | malformed ID は local validation failure。空 ID / default UUID で補完しない。 |
| `filedescriptor` 0.8.3 | `poll`、`FileDescriptor`、`AsRawFileDescriptor` | [`filedescriptor` 0.8.3 source](https://docs.rs/crate/filedescriptor/0.8.3/source/src/lib.rs) | poll / descriptor error を EOF や user cancellation に再分類しない。 |
| `crossterm` 0.29.0 | terminal raw-mode / prompt I/O | [`crossterm` 0.29.0 source](https://docs.rs/crate/crossterm/0.29.0/source/src/lib.rs) | terminal restore guard は original I/O error を隠さない。 |
| `region` 3.0.2 / `rlimit` 0.10.2 / `scopeguard` 1.2.0 | memory lock / core-dump limit / cleanup guard | [`region` source](https://docs.rs/crate/region/3.0.2/source/src/lib.rs)、[`rlimit` source](https://docs.rs/crate/rlimit/0.10.2/source/src/lib.rs)、[`scopeguard` source](https://docs.rs/crate/scopeguard/1.2.0/source/src/lib.rs) | protection setup / restore failure は成功扱いせず停止し、cleanup failure で主 failure を置換しない。 |
| `sha2` 0.10.9 | `Sha256` digest for non-secret guards | [`sha2` 0.10.9 source](https://docs.rs/crate/sha2/0.10.9/source/src/lib.rs) | digest は equality guard 専用で、authenticity / freshness の根拠へ拡張しない。 |
| `tokio` 1.52.3 | async BWS port calls | [`tokio` 1.52.3 source](https://docs.rs/crate/tokio/1.52.3/source/src/lib.rs) | task / I/O error を retryable と推測せず、caller に伝播する。 |
| `syn` 2.0.117 | static adapter-boundary verifier の `parse_file` / `Item` AST | [`syn` 2.0.117 source](https://docs.rs/crate/syn/2.0.117/source/src/lib.rs) と [`parse_file`](https://docs.rs/syn/2.0.117/syn/fn.parse_file.html) | parse failure は source を許可扱いせず、構造検証 failure として停止する。未解析の token / line heuristic へ fallback しない。 |

該当 source の使用箇所は [`support/aead.rs`](../../rust/dotfiles-secrets/src/support/aead.rs)、[`support/protection/sealed_blob.rs`](../../rust/dotfiles-secrets/src/support/protection/sealed_blob.rs)、[`support/protection/secret_random.rs`](../../rust/dotfiles-secrets/src/support/protection/secret_random.rs)、[`support/process_io.rs`](../../rust/dotfiles-secrets/src/support/process_io.rs)、[`support/protection/buffer.rs`](../../rust/dotfiles-secrets/src/support/protection/buffer.rs)、[`domain/manifest.rs`](../../rust/dotfiles-secrets/src/domain/manifest.rs) である。各 file の module doc comment は本節と使用 API symbol を相互参照する。

## Rust support crate（CLI / update-history 直接利用）

`rust/dotfiles-cli/Cargo.toml` が直接利用する crate も、secret-recovery command の composition root または update-history の外部 service 境界を構成する。これらの利用フローと error policy は次の固定版 source を読む。

| crate（lock version） | 使用 API / flow | 直接確認する一次資料 | error / state の扱い |
| --- | --- | --- | --- |
| `async-openai` 0.28.3 | `Client` → chat completion request | [`async-openai` 0.28.3 source](https://docs.rs/crate/async-openai/0.28.3/source/src/lib.rs) と [OpenAI API overview](https://platform.openai.com/docs/overview) | HTTP / SDK error を status text だけで transient・rate-limit・success に再分類しない。 |
| `reqwest` 0.12.28 / `http` 1.4.1 | blocking HTTP request → `Response::status` → `StatusCode::is_success` | [`reqwest` 0.12.28 `RequestBuilder::send`](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.RequestBuilder.html#method.send)、[`Response::status`](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.Response.html#method.status)、[`http::StatusCode::is_success`](https://docs.rs/http/1.4.1/http/status/struct.StatusCode.html#method.is_success) | transport `Error` と non-2xx は API-specific official contract がない限り opaque failure として伝播する。HTTP 404 を URL 許可・Cask 不在・削除へ写像しない。 |
| `backoff` 0.4.0 | retry policy primitive | [`backoff` 0.4.0 source](https://docs.rs/crate/backoff/0.4.0/source/src/lib.rs) | retry は caller が一次資料で transient と確認した error だけに適用する。 |
| `url` 2.5.8 / `percent-encoding` 2.3.2 | URL parse / component encoding | [`url` source](https://docs.rs/crate/url/2.5.8/source/src/lib.rs)、[`percent-encoding` source](https://docs.rs/crate/percent-encoding/2.3.2/source/src/lib.rs) | parse failure を検索 query / default host で補完しない。 |
| `toml` 0.9.12+spec-1.1.0 | configuration parse / serialize | [`toml` source](https://docs.rs/crate/toml/0.9.12/source/src/lib.rs) | parse error は partial config を採用せず停止する。 |
| `rustix` 1.1.4 | OS process / filesystem primitive、test child の `process::setsid` | [`rustix` source](https://docs.rs/crate/rustix/1.1.4/source/src/lib.rs)、[`process::setsid` source](https://docs.rs/crate/rustix/1.1.4/source/src/process/id.rs) | errno の意味は Rustix / OS 一次資料で確認した範囲だけを分岐し、その他は伝播する。`setsid` failure は no-TTY test setup failure として伝播し、TTY あり・なしの成功状態へ写像しない。 |
| `clap` 4.6.1 / `anyhow` 1.0.102 | CLI parse / contextual error chain | [`clap` source](https://docs.rs/crate/clap/4.6.1/source/src/lib.rs)、[`anyhow` source](https://docs.rs/crate/anyhow/1.0.102/source/src/lib.rs) | parse error を recovery input として継続せず、`Context` で source error を除去しない。 |
| `portable-pty` 0.9.0（dev only） | integration test の `native_pty_system` → `Child::try_wait` / `wait` / `kill` | [`portable-pty` 0.9.0 `Child`](https://docs.rs/portable-pty/0.9.0/portable_pty/trait.Child.html) | test-only PTY child の `IoResult` は test failure として伝播する。timeout 時だけ test harness が `kill` 後に `wait` し、production state・SDK error 意味へ写像しない。 |
| `mockall` 0.13.1（dev only） | port trait の `#[automock]` と `Sequence` expectation | [`mockall` 0.13.1](https://docs.rs/mockall/0.13.1/mockall/) | test-only expectation / sequence failure は test failure であり、production code に mock state、SDK error 分類、retry/fallback を持ち込まない。 |

### update-history HTTP / GitHub Releases / Cask flow

product flow の正本は [#47](https://github.com/wthrk/dotfiles/issues/47) と root [README の nightly 自動 bump](../../README.md#nightly-自動-bump-とゲート) である。`record` は lock bump の nix / brew version delta を一件の履歴へ記録し、Cask の本文は release notes ではなく version / `sha256 :no_check` guard の入力としてだけ読む。release notes が成功応答でも空なら version-only を記録できるが、HTTP / transport / JSON error は本文不在、resource 不在、削除、rate limit 可、または成功へ再分類しない。

1. `notes::http_get_once` は `reqwest::blocking::RequestBuilder::send` の transport result と `Response::status` を受け取る。一回の `record` では、status 本文の文字列照合、403 / 429 / 5xx の独自 retry、固定 backoff を使わない。GitHub は rate-limit response の retry を `Retry-After`、`x-ratelimit-remaining`、`x-ratelimit-reset` に従わせるため、これらを完全に実装しない経路で推測 retry を追加してはならない。根拠は [GitHub REST troubleshooting: rate-limit errors](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#rate-limit-errors) と [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28#exceeding-the-rate-limit) である。
2. `safe_https_fetch` は `StatusCode::is_success` が定義する 2xx の非空本文だけを返す。空の成功本文だけが「取得済みだがノート本文なし」の `None` であり、non-2xx と transport error は `Err` である。`reqwest` API は non-success を resource state に変換しない。GitHub は private resource への未認証 request にも 404 を返し得るため、404 を Cask 不在または URL の許可状態に写像しない。根拠は [StatusCode::is_success](https://docs.rs/http/1.4.1/http/status/struct.StatusCode.html#method.is_success) と [GitHub REST troubleshooting: 404](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api?apiVersion=2022-11-28#404-not-found-for-an-existing-resource) である。
3. `fetch_releases_page` / `fetch_release_api` は GitHub `List releases` の成功 status 200 と response array を直接照合する。response JSON の構文、array/object shape、必須 `tag_name` の異常は外部応答 failure として伝播する。`name` / `body` の null・欠落だけを API response model の値なしとして空に正規化する。根拠は [List releases](https://docs.github.com/en/rest/releases/releases?apiVersion=2022-11-28#list-releases) の status / response schema である。
4. `brew::fetch_cask_rb` は構造的に許可されない URL を error にし、non-2xx、transport error、空本文を Cask state へ変換しない。`diff_casks` は両 rev の quoted `version "..."` と new rev の `sha256` guard を取得できた時だけ upgrade / downgrade を記録する。404、`version :latest`、構文不正、取得失敗を Added / Removed / unchanged にしない。Cask が存在するかという product state は GitHub HTTP status ではなく、この update-history flow の外で管理する。
