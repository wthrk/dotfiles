//! YubiKey bootstrap secret storage の操作フロー。
//!
//! setup / put / get / enroll / verify は同じ manifest sentinel と PIV object mapping を
//! 共有する。device adapter の実装差は `SecretDevice` trait に閉じ、ここでは永続書き込みの
//! 順序と検証結果の JSON 契約を固定する。

use crate::Result;
use crate::secrets::support::protection::{ProtectedSecret, SecretSession};
use anyhow::{Context, bail};

use crate::secrets::adapters::storage_io;
use crate::secrets::application::blob_crypto::{decrypt_secret_protected, encrypt_secret};
use crate::secrets::application::{CheckName, CheckStatus, EnrollSummary, YubikeyRole};
use crate::secrets::domain::{KEY_SLOT, SecretBlob, SecretManifest, SecretName, StorageObjectIds};
use crate::secrets::ports::SecretDevice;

/// secret storage 用 PIV key と manifest を新規作成する。
///
/// 既存 key または対象 object が存在する場合は、上書きせず停止する。
pub fn setup<D: SecretDevice>(device: &mut D) -> Result<()> {
    check_setup_preconditions(device)?;
    device.generate_key()?;
    write_manifest(device)
}

/// setup が永続書き込みを開始できる device 状態か確認する。
///
/// key 生成条件、management auth、既存 key、予約済み object の衝突を検証する。
pub fn check_setup_preconditions<D: SecretDevice>(device: &mut D) -> Result<()> {
    device.check_key_generation_preconditions()?;
    device.check_management_auth_preconditions()?;

    if device.key_exists()? {
        if let Ok(manifest) = read_manifest(device) {
            manifest.validate_expected()?;
            bail!("YubiKey secret storage is already initialized");
        }

        bail!("YubiKey PIV slot {KEY_SLOT} already contains a key");
    }

    for object_id in StorageObjectIds::iter() {
        if device.read_object(object_id)?.is_some() {
            bail!("YubiKey PIV object {} already exists", object_id);
        }
    }

    Ok(())
}

/// 1 secret を encrypted blob として YubiKey object に保存する。
///
/// manifest が期待値と一致することを確認し、既存 blob は `force` がない限り拒否する。
pub fn put<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
    force: bool,
    session: &SecretSession,
) -> Result<()> {
    if secret.is_empty() {
        bail!("{} must not be empty", name);
    }

    check_put_target_writable(device, name, force)?;

    let blob = encrypt_secret(device, name, secret, session)?;
    session.check_interrupted()?;
    let mut encoded = blob.encode()?;
    device.write_object(name.object_id(), &mut encoded)
}

/// `put` 実行前に検証できる保存条件を確認する。
pub fn check_put_preconditions<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    force: bool,
) -> Result<()> {
    check_put_target_writable(device, name, force)
}

/// `put` 系操作の共通保存条件を確認する。
///
/// manifest 整合性、management auth、既存 blob の上書き可否を検証する。
fn check_put_target_writable<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    force: bool,
) -> Result<()> {
    read_manifest(device)?.validate_expected()?;
    device.check_management_auth_preconditions()?;
    if device.read_object(name.object_id())?.is_some() && !force {
        bail!("{} already exists; pass --force to replace it", name);
    }
    Ok(())
}

/// 1 secret を YubiKey object から読み出して保護済み値へ復号する。
///
/// blob 内の secret id と要求された secret 名が一致しない場合は拒否する。
pub fn get_protected<'session, D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let blob = read_secret_blob(device, name)?;
    decrypt_secret_protected(device, &blob, session)
}

fn read_secret_blob<D: SecretDevice>(device: &mut D, name: SecretName) -> Result<SecretBlob> {
    read_manifest(device)?.validate_expected()?;
    let encoded = device
        .read_object(name.object_id())?
        .with_context(|| format!("{} is not stored on this YubiKey", name))?;
    storage_io::decode_secret_blob(&encoded, name)
}

/// 登録直後の summary 初期値を構築する。
///
/// local verify は application の保護境界で実行するため、初期値では `local_storage` を未確認として扱う。
pub fn enroll_summary(serial: u32, role: YubikeyRole) -> EnrollSummary {
    let checks = [
        (CheckName::Setup, CheckStatus::Ok),
        (CheckName::BwEmail, CheckStatus::Ok),
        (CheckName::BwPassword, CheckStatus::Ok),
        (CheckName::BwsAccessToken, CheckStatus::Ok),
        (CheckName::LocalStorage, CheckStatus::Skipped),
    ]
    .into_iter()
    .collect();

    EnrollSummary {
        serial,
        role,
        checks,
    }
}

/// BWS access token の blob を置き換える。
///
/// local verify は呼び出し側の保護境界で実行する。
pub fn replace_bws_token<D: SecretDevice>(
    device: &mut D,
    token: &[u8],
    session: &SecretSession,
) -> Result<()> {
    put(device, SecretName::BwsAccessToken, token, true, session)
}

/// expected manifest を PIV object へ書き込む。
///
/// manifest は secret blob より先に書き、以後の put/get/verify が storage 所有権を判定する sentinel にする。
fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let mut manifest = storage_io::encode_expected_manifest()?;
    device.write_object(storage_io::manifest_object_id(), &mut manifest)
}

/// PIV object から manifest を読み出して parse する。
///
/// manifest が存在しない YubiKey は secret storage 未初期化として扱う。
fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(storage_io::manifest_object_id())?
        .context("YubiKey secret manifest is missing")?;
    storage_io::decode_manifest(&manifest)
}
