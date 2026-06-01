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
use zeroize::Zeroize;

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
