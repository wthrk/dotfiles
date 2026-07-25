//! `gpg-secret-key-backup` envelope の DEK 生成と、DEK での backup 本体 AES-256-GCM 暗復号を
//! protection 境界内で完了する backend 操作。
//!
//! ここでは DEK・平文 backup・復号済み backup を `ProtectedSecret` の借用境界内だけで扱い、平文 buffer を
//! 返す public API や汎用 consumer API を作らない。envelope schema 検証・recipient 照合・fingerprint
//! 照合などの業務規則は domain 側に閉じ、この module は AEAD primitive 呼び出しと protected buffer 操作
//! という技術境界だけを担う。`body` と detached `tag` は連結しない（envelope schema に合わせる）。
//! AAD は使わず、AES-256-GCM の authentication tag で改ざんを検出する。
//! GPGME / Sequoia / entropy の固定 source と採用フローは
//! [`external-sdk-evidence.md`](../../../../../docs/secret-recovery/external-sdk-evidence.md#gpgme--sequoia-openpgp)
//! および同文書の Rust support crate 表を根拠とする。

use anyhow::{Context, bail};
use gpgme::{ExportMode, Protocol};
use sequoia_openpgp::{
    Cert,
    cert::{CertParser, prelude::*},
    parse::Parse,
    policy::StandardPolicy,
    types::RevocationStatus,
};
use zeroize::Zeroize;

use crate::{
    Result,
    features::gpg_backup_recovery::domain,
    foundation::{
        aead::{AES_GCM_NONCE_LEN, aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
        protection::{ProtectedSecret, secret_random},
    },
};

/// envelope `metadata.dek_alg`（`aes-256-gcm`）に対応する DEK の byte 長。
const DEK_LEN: usize = 32;

/// gpgme で指定 fingerprint の OpenPGP transferable secret key を in-memory export し、保護値として返す。
///
/// secret key material は gpgme export buffer から直接 locked `ProtectedSecret` へ複製し、`gpg` CLI・argv・
/// 永続一時ファイルを経由しない。export buffer は関数 stack frame 内に閉じ、caller へ平文 buffer を返さない。
/// `fingerprint` は lowercase hex の primary fingerprint 文字列とする（domain 値からの取り出しは caller 責務）。
pub(crate) fn export_secret_key(fingerprint: &str) -> Result<ProtectedSecret> {
    let mut context = open_context()?;
    let mut data = gpgme::Data::new().context("failed to allocate gpgme export buffer")?;
    context
        .export([fingerprint], ExportMode::SECRET, &mut data)
        .context("failed to export GPG secret key")?;
    // `try_into_bytes` が返す `Vec<u8>` は secret key material を保持する。`ProtectedSecret` へ複製した
    // 後に必ず zeroize し、empty / alloc 失敗の早期 return 経路でも平文 buffer を drop 前に消す。
    let mut bytes = data
        .try_into_bytes()
        .context("failed to read exported GPG secret key bytes")?;
    if bytes.is_empty() {
        bytes.zeroize();
        bail!("exported GPG secret key is empty");
    }
    let secret = match ProtectedSecret::new(bytes.len()) {
        Ok(mut secret) => {
            secret.with_secret_mut(|out| out.copy_from_slice(&bytes));
            secret
        }
        Err(error) => {
            bytes.zeroize();
            return Err(error);
        }
    };
    bytes.zeroize();
    Ok(secret)
}

/// 復号済み backup bytes を sequoia-openpgp で解析し、certificate/subkey の raw facts を返す。
///
/// 解析は `ProtectedSecret` の借用中だけ行い、OpenPGP transferable secret key の平文 byte を関数外へ出さない。
/// `Cert::from_bytes` 一件の parse 成功だけでは復旧能力を証明しない。Sequoia 1.22.0 の公式例に従い
/// `CertParser::from_bytes` で入力全体を消費して一 certificate に限定し、`StandardPolicy` と現在時刻で
/// support は recovery policy を決めず、certificate revocation と各 subkey の
/// supported/alive/revocation/secret/capability facts を domain へ渡す。
/// repository 正本が採用する software key は passphrase-free なので、certificate の primary key を含む
/// 全 secret key packet が `SecretKeyMaterial::Unencrypted` であることを、この technical parser が
/// facts 化より前に確認する。encrypted または secret material を確認できない packet は、pinentry を
/// 起動して補うことなく export/BWS 保存/import 前に opaque failure として停止する。
///
/// Sources:
/// - https://docs.rs/sequoia-openpgp/1.22.0/sequoia_openpgp/cert/struct.CertParser.html
/// - https://gitlab.com/sequoia-pgp/sequoia/-/blob/v1.22.0/openpgp/examples/generate-encrypt-decrypt.rs
/// - https://gitlab.com/sequoia-pgp/sequoia/-/blob/v1.22.0/openpgp/examples/sign-detached.rs
pub(crate) fn inspect_backup(
    backup: &ProtectedSecret,
) -> Result<crate::features::gpg_backup_recovery::domain::gpg_restore::OpenPgpBackupFacts> {
    backup.with_secret(|bytes| {
        let mut certs = CertParser::from_bytes(bytes)
            .context("failed to initialize OpenPGP backup parser")?
            .collect::<sequoia_openpgp::Result<Vec<Cert>>>()
            .context("failed to parse every OpenPGP certificate in backup bytes")?;
        if certs.len() != 1 {
            bail!("OpenPGP backup must contain exactly one transferable secret key");
        }
        let cert = certs
            .pop()
            .context("OpenPGP backup parser returned no certificate")?;
        ensure_unprotected_secret_packets(&cert)?;
        let primary_fingerprint =
            domain::gpg_backup::PrimaryFingerprint::parse(&cert.fingerprint().to_hex())?;
        let policy = StandardPolicy::new();
        let certificate_revocation = map_revocation(cert.revocation_status(&policy, None));
        let valid = cert
            .with_policy(&policy, None)
            .context("OpenPGP backup certificate failed StandardPolicy validation")?;
        let subkeys = valid
            .keys()
            .subkeys()
            .map(|key| domain::gpg_restore::OpenPgpSubkeyFacts {
                supported: key.key().pk_algo().is_supported(),
                alive: key.alive().is_ok(),
                revocation: map_revocation(key.revocation_status()),
                secret: key.key().has_secret(),
                signing: key.for_signing(),
                authentication: key.for_authentication(),
                storage_encryption: key.for_storage_encryption(),
                transport_encryption: key.for_transport_encryption(),
            })
            .collect();
        Ok(domain::gpg_restore::OpenPgpBackupFacts {
            primary_fingerprint,
            certificate_revocation,
            subkeys,
        })
    })
}

/// transferable secret key の primary key と全 subkey に unencrypted secret material があることを確認する。
///
/// Sequoia 1.22.0 の [`Key::optional_secret`] と [`SecretKeyMaterial::is_encrypted`] に従う。
/// encrypted/absent material を passphrase、pinentry、空 passphrase で補完しない。ここは packet の
/// technical observation だけを担い、capability/revocation の policy は domain facts へ委ねる。
///
/// [`Key::optional_secret`]: https://docs.rs/crate/sequoia-openpgp/1.22.0/source/src/packet/key.rs
/// [`SecretKeyMaterial::is_encrypted`]: https://docs.rs/crate/sequoia-openpgp/1.22.0/source/src/packet/key.rs
fn ensure_unprotected_secret_packets(cert: &Cert) -> Result<()> {
    for key in cert.keys() {
        let secret = key
            .key()
            .optional_secret()
            .context("OpenPGP backup contains a key packet without secret material")?;
        if secret.is_encrypted() {
            anyhow::bail!("OpenPGP backup contains passphrase-protected secret key material");
        }
    }
    Ok(())
}

fn map_revocation(status: RevocationStatus<'_>) -> domain::gpg_restore::OpenPgpRevocation {
    match status {
        RevocationStatus::Revoked(_) => domain::gpg_restore::OpenPgpRevocation::Revoked,
        RevocationStatus::CouldBe(_) => domain::gpg_restore::OpenPgpRevocation::CouldBe,
        RevocationStatus::NotAsFarAsWeKnow => {
            domain::gpg_restore::OpenPgpRevocation::NotAsFarAsWeKnow
        }
    }
}

/// 復号済み backup bytes を gpgme で鍵リングへ import し、import 結果の primary fingerprint（hex）を返す。
///
/// backup bytes は `ProtectedSecret` の借用中だけ gpgme `Data` へ渡し、平文 buffer を関数外へ出さない。
/// import 対象を fingerprint で同定するため、import result の最初の imported fingerprint を返す。
/// `gpgme` 0.11.0 `Import::fingerprint` は raw 値が無い場合を `Err(None)`、非 UTF-8 を
/// `Err(Some(Utf8Error))` で表すため、いずれも別 fingerprint へ推測変換せず failure として伝播する。
/// 出典: <https://docs.rs/crate/gpgme/0.11.0/source/src/results.rs#L88-L91>。
pub(crate) fn import_secret_key(backup: &ProtectedSecret) -> Result<String> {
    let mut context = open_context()?;
    backup.with_secret(|bytes| {
        let data =
            gpgme::Data::from_bytes(bytes).context("failed to wrap GPG backup bytes for import")?;
        let result = context
            .import(data)
            .context("failed to import GPG secret key")?;
        let import = result
            .imports()
            .next()
            .context("GPG import did not report an imported key")?;
        import
            .fingerprint()
            .map(str::to_owned)
            .map_err(|error| match error {
                Some(error) => anyhow::Error::new(error)
                    .context("GPG imported key fingerprint is not valid UTF-8"),
                None => anyhow::anyhow!("GPG imported key fingerprint is absent"),
            })
    })
}

/// store 内 sample entry（`*.gpg`）の読み取り上限（byte）。`pass` entry は OpenPGP message であり数 KB
/// 程度に収まる。これを大きく超える入力は異常（巨大ファイルや symlink 経由で差し替わった endless file）と
/// して拒否し、`/dev/zero` のような無限長 source での hang/OOM を断つ。store 内の現実的な entry を誤って
/// 拒否しないよう余裕を持った上限とする。
const STORE_ENTRY_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// store 内 `pass` entry（OpenPGP message）を gpgme で復号できることを protection 境界内で確認する。
///
/// entry は `symlink_metadata`（link を辿らない）で regular file と dev/ino を確認してから `File::open` し、
/// 開いた fd の fstat の dev/ino 一致を照合して読む。これにより、store 走査（regular file 判定）から読取りまで
/// の間に同一 path が symlink へ差し替えられても、`std::fs::read` のような path 再 open で store 外（例:
/// `/dev/zero` で hang/OOM、外部の復号可能ファイルで偽の可読性成功）へ抜ける TOCTOU を閉じる。symlink・
/// special file・上限超過は読まずに拒否し、可読性確認を失敗（= store を「読めない」）と判定させる。
/// 復号した平文（`pass` entry の中身）は locked buffer 上でだけ扱い、zeroize して破棄する。stdout・log・
/// 一時ファイル・caller のいずれへも平文を返さない。復号に成功すれば `Ok(())`、復号できなければ context
/// 付き `Err` を返す。entry の業務的意味（recipient 妥当性・store 構造）はこの module で判定しない。
pub(crate) fn verify_can_decrypt(entry_path: &std::path::Path) -> Result<()> {
    let mut context = open_context()?;
    let ciphertext = read_regular_file_nofollow(entry_path)
        .context("failed to read password-store entry for decryption check")?;
    let mut input = gpgme::Data::from_bytes(&ciphertext)
        .context("failed to wrap password-store entry bytes")?;
    let mut output = gpgme::Data::new()
        .context("failed to allocate gpgme buffer for password-store decryption check")?;
    context
        .decrypt(&mut input, &mut output)
        .context("failed to decrypt password-store entry with the restored GPG key")?;
    // 復号できた平文は確認以外に使わない。buffer へ取り出した後に必ず zeroize する。
    let mut plaintext = output
        .try_into_bytes()
        .context("failed to read decrypted password-store entry bytes")?;
    plaintext.zeroize();
    Ok(())
}

/// final component が symlink でない regular file を、dev/ino 照合 + size cap で安全に読み、bytes を返す。
///
/// 走査時点で `symlink_metadata`（link を辿らない）を取り、final component が regular file であること・
/// size が [`STORE_ENTRY_MAX_BYTES`] 以内であることを確認してから `File::open` し、開いた fd の fstat の
/// `dev`/`ino` が走査時点と一致することを照合する。走査↔open の間に final component が別 inode（store 外を
/// 指す symlink や special file）へ差し替えられていれば、`File::open` がその symlink を辿り fd の dev/ino が
/// 走査時点と一致しない（または special file で `is_file` が偽になる）ため検出して停止し、path 再 open による
/// symlink 追従の TOCTOU を閉じる。これは `O_NOFOLLOW`（final component のみ保護）と同等の保護範囲を
/// `std::os::unix::fs::MetadataExt` だけで達成し、libc/OS 数値ハードコードへ依存しない。
///
/// 照合済みの fd からだけ読み、path を再 open しない。`metadata().len()` と読取り長の双方を
/// [`STORE_ENTRY_MAX_BYTES`] で上限化して、`len` を 0 と詐称する special file（`/dev/zero` 等）経由の
/// endless read（hang/OOM）も断つ。
fn read_regular_file_nofollow(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    // 走査時点の metadata は link を辿らずに取得する。final component が regular file 以外なら停止する。
    let pre = path
        .symlink_metadata()
        .context("failed to stat file (refusing to follow a symlink)")?;
    if !pre.file_type().is_file() {
        bail!("path is not a regular file; refusing to follow it");
    }
    if pre.len() > STORE_ENTRY_MAX_BYTES {
        bail!("file is unexpectedly large (> {STORE_ENTRY_MAX_BYTES} bytes); refusing to read it");
    }
    // open は final component が走査後に symlink へ差し替えられていればそれを辿る。直後に fd の fstat を取り、
    // 走査時点の dev/ino と一致しなければ TOCTOU として停止する（読まない）。
    let file = std::fs::File::open(path).context("failed to open file")?;
    let post = file.metadata().context("failed to stat file")?;
    if !post.file_type().is_file() || pre.dev() != post.dev() || pre.ino() != post.ino() {
        bail!("file changed between stat and open (possible symlink swap); refusing to read it");
    }
    // size cap を `metadata().len()` だけに頼らず、read 自体も上限 + 1 byte で打ち切る（special file 等で
    // len が 0 報告でも endless に読めない）。上限を超えたら異常として拒否する。
    let mut bytes = Vec::new();
    let read = (&file)
        .take(STORE_ENTRY_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("failed to read file")?;
    if read as u64 > STORE_ENTRY_MAX_BYTES {
        bail!("file is unexpectedly large (> {STORE_ENTRY_MAX_BYTES} bytes); refusing to read it");
    }
    Ok(bytes)
}

/// OpenPGP protocol の gpgme context を生成する。
fn open_context() -> Result<gpgme::Context> {
    gpgme::Context::from_protocol(Protocol::OpenPgp).context("failed to create gpgme context")
}

/// AES-256-GCM の DEK を locked `ProtectedSecret` として生成する。
///
/// 生成後の所有権は protection 境界内に閉じ、caller へ raw buffer を返さない。
pub(crate) fn generate_dek() -> Result<ProtectedSecret> {
    secret_random::random_secret(DEK_LEN)
}

/// 平文 backup を DEK で AES-256-GCM 暗号化し、`(nonce, body, tag)` を返す。
///
/// nonce は 96-bit を毎回新規生成し、同一 DEK での再利用を避ける。`body` は平文 backup を clone した
/// protected buffer 上で in-place 暗号化した bytes で、`tag` は detached tag（`body` へ連結しない）。
pub(crate) fn encrypt_backup_body(
    dek: &ProtectedSecret,
    backup: &ProtectedSecret,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let cipher = dek.with_secret(aes_256_gcm_from_key)?;
    let mut nonce = [0u8; AES_GCM_NONCE_LEN];
    secret_random::fill_os_random(&mut nonce, "GPG backup nonce")?;
    let mut body = ProtectedSecret::try_clone(backup)?;
    let tag = body.with_secret_mut(|bytes| encrypt_detached(&cipher, &nonce, &[], bytes))?;
    let body_bytes = body.with_secret(<[u8]>::to_vec);
    Ok((nonce.to_vec(), body_bytes, tag.to_vec()))
}

/// envelope `ciphertext`（nonce/body/tag）を DEK で AES-256-GCM 復号し、復号済み backup を返す。
///
/// 復号先は body 長の locked `ProtectedSecret` として確保し、tag 検証はその buffer 上で in-place に行う。
/// 認証失敗時は plaintext を返さず、buffer 内容に意味を持たせない。
pub(crate) fn decrypt_backup_body(
    dek: &ProtectedSecret,
    nonce: &[u8],
    body: &[u8],
    tag: &[u8],
) -> Result<ProtectedSecret> {
    let cipher = dek.with_secret(aes_256_gcm_from_key)?;
    let mut backup = ProtectedSecret::new(body.len())?;
    backup.with_secret_mut(|out| out.copy_from_slice(body));
    backup
        .with_secret_mut(|bytes| decrypt_detached(&cipher, nonce, &[], bytes, tag))
        .context("failed to decrypt gpg backup body")?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use sequoia_openpgp::{cert::prelude::CertBuilder, serialize::Serialize};

    use super::*;

    fn protected_secret_from_bytes(bytes: &[u8]) -> Result<ProtectedSecret> {
        let mut secret = ProtectedSecret::new(bytes.len())?;
        secret.with_secret_mut(|destination| destination.copy_from_slice(bytes));
        Ok(secret)
    }

    fn serialize_transferable_secret_key(cert: &Cert) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        cert.as_tsk()
            .serialize(&mut bytes)
            .context("failed to serialize test transferable secret key")?;
        Ok(bytes)
    }

    #[test]
    fn parses_exactly_one_recovery_capable_transferable_secret_key() -> Result<()> {
        let (cert, _) = CertBuilder::new()
            .add_userid("recovery@example.invalid")
            .add_signing_subkey()
            .add_authentication_subkey()
            .add_storage_encryption_subkey()
            .generate()
            .context("failed to generate recovery-capable test certificate")?;
        let bytes = serialize_transferable_secret_key(&cert)?;
        let backup = protected_secret_from_bytes(&bytes)?;

        let parsed = inspect_backup(&backup)?.ensure_recovery_capabilities()?;

        assert_eq!(parsed.as_str(), cert.fingerprint().to_hex().to_lowercase());
        Ok(())
    }

    #[test]
    fn rejects_transferable_secret_key_without_required_recovery_subkeys() -> Result<()> {
        let (cert, _) = CertBuilder::new()
            .add_userid("insufficient@example.invalid")
            .add_signing_subkey()
            .generate()
            .context("failed to generate insufficient test certificate")?;
        let bytes = serialize_transferable_secret_key(&cert)?;
        let backup = protected_secret_from_bytes(&bytes)?;

        let error = inspect_backup(&backup)
            .and_then(domain::gpg_restore::OpenPgpBackupFacts::ensure_recovery_capabilities)
            .expect_err("certificate without authentication and encryption subkeys must fail");

        assert!(
            error.to_string().contains(
                "OpenPGP backup lacks a supported, alive, non-revoked secret signing, authentication, or encryption subkey"
            )
        );
        Ok(())
    }

    #[test]
    fn rejects_more_than_one_transferable_secret_key() -> Result<()> {
        let (first, _) = CertBuilder::new()
            .add_userid("first@example.invalid")
            .add_signing_subkey()
            .add_authentication_subkey()
            .add_transport_encryption_subkey()
            .generate()
            .context("failed to generate first test certificate")?;
        let (second, _) = CertBuilder::new()
            .add_userid("second@example.invalid")
            .add_signing_subkey()
            .add_authentication_subkey()
            .add_transport_encryption_subkey()
            .generate()
            .context("failed to generate second test certificate")?;
        let mut bytes = serialize_transferable_secret_key(&first)?;
        bytes.extend(serialize_transferable_secret_key(&second)?);
        let backup = protected_secret_from_bytes(&bytes)?;

        let error =
            inspect_backup(&backup).expect_err("backup containing multiple certificates must fail");

        assert_eq!(
            error.to_string(),
            "OpenPGP backup must contain exactly one transferable secret key"
        );
        Ok(())
    }

    #[test]
    fn rejects_passphrase_protected_secret_packets_before_capability_policy() -> Result<()> {
        let (cert, _) = CertBuilder::new()
            .set_password(Some("fixture-passphrase".into()))
            .add_userid("protected@example.invalid")
            .add_signing_subkey()
            .add_authentication_subkey()
            .add_storage_encryption_subkey()
            .generate()
            .context("failed to generate protected test certificate")?;
        let bytes = serialize_transferable_secret_key(&cert)?;
        let backup = protected_secret_from_bytes(&bytes)?;

        let error = inspect_backup(&backup)
            .expect_err("protected secret packet must fail before import or BWS storage");
        assert_eq!(
            error.to_string(),
            "OpenPGP backup contains passphrase-protected secret key material"
        );
        Ok(())
    }

    /// 固定した実際の passphrase-protected transferable secret key を parser 境界で拒否する。
    ///
    /// fixture は pgp 0.19.0 の `tests/key-with-password-123.asc` を固定版 source から転載した test-only
    /// dummy key である。同 crate の `test_locked_key` は Password `123` による unlock を確認している。
    /// 実行時 key generation を避けることで S2K cost・entropy・時刻を回帰の実行時間へ持ち込まない。
    /// 出典: https://docs.rs/crate/pgp/0.19.0/source/tests/key-with-password-123.asc および
    /// https://docs.rs/crate/pgp/0.19.0/source/tests/key_test.rs 。
    #[test]
    fn rejects_fixed_actual_passphrase_protected_transferable_secret_key() -> Result<()> {
        let backup = protected_secret_from_bytes(include_bytes!(
            "../../../../tests/fixtures/passphrase-protected-tsk.asc"
        ))?;

        let error = inspect_backup(&backup)
            .expect_err("protected transferable secret key must fail at the parser boundary");

        assert_eq!(
            error.to_string(),
            "OpenPGP backup contains passphrase-protected secret key material"
        );
        Ok(())
    }
}
