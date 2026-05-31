//! `gpg-secret-key-backup` envelope の DEK 生成と、DEK での backup 本体 AES-256-GCM 暗復号を
//! protection 境界内で完了する backend 操作。
//!
//! ここでは DEK・平文 backup・復号済み backup を `ProtectedSecret` の借用境界内だけで扱い、平文 buffer を
//! 返す public API や汎用 consumer API を作らない。envelope schema 検証・recipient 照合・fingerprint
//! 照合などの業務規則は domain 側に閉じ、この module は AEAD primitive 呼び出しと protected buffer 操作
//! という技術境界だけを担う。`body` と detached `tag` は連結しない（envelope schema に合わせる）。
//! AAD は使わず、AES-256-GCM の authentication tag で改ざんを検出する。

use anyhow::{Context, bail};
use gpgme::{ExportMode, Protocol};
use rand::RngCore;
use sequoia_openpgp::{Cert, parse::Parse};

use crate::Result;
use crate::secrets::support::aead::{
    AES_GCM_NONCE_LEN, aes_256_gcm_from_key, decrypt_detached, encrypt_detached,
};
use crate::secrets::support::protection::{ProtectedSecret, secret_random};

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
    let bytes = data
        .try_into_bytes()
        .context("failed to read exported GPG secret key bytes")?;
    if bytes.is_empty() {
        bail!("exported GPG secret key is empty");
    }
    let mut secret = ProtectedSecret::new(bytes.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(&bytes));
    Ok(secret)
}

/// 復号済み backup bytes を sequoia-openpgp でインメモリ解析し、primary fingerprint の uppercase hex を返す。
///
/// 解析は `ProtectedSecret` の借用中だけ行い、OpenPGP transferable secret key の平文 byte を関数外へ出さない。
pub(crate) fn parse_primary_fingerprint_hex(backup: &ProtectedSecret) -> Result<String> {
    backup.with_secret(|bytes| {
        let cert = Cert::from_bytes(bytes).context("failed to parse OpenPGP backup bytes")?;
        Ok(cert.fingerprint().to_hex())
    })
}

/// 復号済み backup bytes を gpgme で鍵リングへ import し、import 結果の primary fingerprint（hex）を返す。
///
/// backup bytes は `ProtectedSecret` の借用中だけ gpgme `Data` へ渡し、平文 buffer を関数外へ出さない。
/// import 対象を fingerprint で同定するため、import result の最初の imported fingerprint を返す。
pub(crate) fn import_secret_key(backup: &ProtectedSecret) -> Result<String> {
    let mut context = open_context()?;
    backup.with_secret(|bytes| {
        let data =
            gpgme::Data::from_bytes(bytes).context("failed to wrap GPG backup bytes for import")?;
        let result = context
            .import(data)
            .context("failed to import GPG secret key")?;
        result
            .imports()
            .filter_map(|import| import.fingerprint().ok().map(str::to_owned))
            .next()
            .context("GPG import did not report an imported key fingerprint")
    })
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
    rand::rng().fill_bytes(&mut nonce);
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
        .map_err(|_| anyhow::anyhow!("failed to decrypt gpg backup body"))?;
    Ok(backup)
}
