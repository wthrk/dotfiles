//! YubiKey bootstrap secret storage の操作フロー。
//!
//! setup / put / get / enroll / verify は同じ manifest sentinel と PIV object mapping を
//! 共有する。device adapter の実装差は `SecretDevice` trait に閉じ、ここでは永続書き込みの
//! 順序と検証結果の JSON 契約を固定する。

use crate::Result;
use anyhow::{Context, bail};

use super::crypto::{decrypt_secret, encrypt_secret};
use super::model::{
    BootstrapSecretSource, CheckName, CheckStatus, EnrollSummary, KEY_SLOT, PivObjectId,
    SecretBlob, SecretBytes, SecretDevice, SecretManifest, SecretName, StorageObjectIds,
    VerifySummary, YubikeyRole,
};

/// secret storage 用 PIV key と manifest を新規作成する。
///
/// 既存 key または対象 object が存在する場合は、上書きせず停止する。
pub fn setup<D: SecretDevice>(device: &mut D) -> Result<()> {
    check_setup_preconditions(device)?;
    device.generate_key()?;
    write_manifest(device)
}

/// secret 入力前に、setup が永続書き込みを開始できる device 状態か確認する。
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
) -> Result<()> {
    if secret.is_empty() {
        bail!("{} must not be empty", name);
    }

    check_put_target_writable(device, name, force)?;

    let blob = encrypt_secret(device, name, secret)?;
    let encoded = blob.encode()?;
    device.write_object(name.object_id(), &encoded)
}

/// `put` 実行前に、secret 入力なしで検証できる保存条件を確認する。
pub fn check_put_preconditions<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    force: bool,
) -> Result<()> {
    check_put_target_writable(device, name, force)
}

/// `put` 系 command の共通事前条件として、manifest 整合性と上書き可否を確認する。
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

/// 1 secret を YubiKey object から読み出して復号する。
///
/// blob 内の secret id と要求された secret 名が一致しない場合は拒否する。
pub fn get<D: SecretDevice>(device: &mut D, name: SecretName) -> Result<SecretBytes> {
    read_manifest(device)?.validate_expected()?;
    let encoded = device
        .read_object(name.object_id())?
        .with_context(|| format!("{} is not stored on this YubiKey", name))?;
    let blob =
        SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {}", name))?;
    if blob.name != name {
        bail!("YubiKey secret blob name does not match requested {}", name);
    }

    decrypt_secret(device, &blob)
}

/// primary または spare として bootstrap secret 一式を登録する。
///
/// setup、3 secret 保存、local verify の順序を固定し、成功した確認項目を summary に含める。
pub fn enroll<D: SecretDevice, S: BootstrapSecretSource>(
    device: &mut D,
    role: YubikeyRole,
    secrets: &S,
) -> Result<EnrollSummary> {
    for name in SecretName::iter() {
        if secrets.get(name).is_empty() {
            bail!("{} must not be empty", name);
        }
    }

    setup(device)?;
    for name in SecretName::iter() {
        put(device, name, secrets.get(name), false)?;
    }
    verify_local_storage(device)?;

    let checks = [
        (CheckName::Setup, CheckStatus::Ok),
        (CheckName::BwEmail, CheckStatus::Ok),
        (CheckName::BwPassword, CheckStatus::Ok),
        (CheckName::BwsAccessToken, CheckStatus::Ok),
        (CheckName::LocalStorage, CheckStatus::Ok),
    ]
    .into_iter()
    .collect();

    Ok(EnrollSummary {
        serial: device.serial(),
        role,
        checks,
    })
}

/// BWS access token を置き換えた後、同じ device 上の local storage 整合性を再検証する。
pub fn rotate_bws_token<D: SecretDevice>(device: &mut D, token: &[u8]) -> Result<VerifySummary> {
    put(device, SecretName::BwsAccessToken, token, true)?;
    verify_local_storage(device)
}

/// token を読む前に、更新対象 YubiKey が dotfiles storage として初期化済みか確認する。
pub fn check_rotate_preconditions<D: SecretDevice>(device: &mut D) -> Result<()> {
    verify_local_storage(device)?;
    device.check_management_auth_preconditions()
}

/// YubiKey 上の manifest と 3 secret の復号可能性を検証する。
///
/// local storage verification は外部 service に接続せず、remote check は未実行項目として残す。
pub fn verify_local_storage<D: SecretDevice>(device: &mut D) -> Result<VerifySummary> {
    read_manifest(device)?.validate_expected()?;
    for name in SecretName::iter() {
        let secret = get(device, name)?;
        if secret.is_empty() {
            bail!("{} stored on this YubiKey is empty", name);
        }
    }

    let checks = [
        (CheckName::LocalStorage, CheckStatus::Ok),
        (CheckName::Bws, CheckStatus::Skipped),
        (CheckName::BwLogin, CheckStatus::Skipped),
    ]
    .into_iter()
    .collect();

    Ok(VerifySummary {
        serial: device.serial(),
        checks,
    })
}

/// manifest は secret blob より先に書き、以後の put/get/verify が storage 所有権を判定する sentinel にする。
fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let manifest = serde_json::to_vec(&SecretManifest::expected())?;
    device.write_object(PivObjectId::MANIFEST, &manifest)
}

/// manifest が存在しない YubiKey は secret storage 未初期化として扱う。
fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(PivObjectId::MANIFEST)?
        .context("YubiKey secret manifest is missing")?;
    serde_json::from_slice(&manifest).context("failed to parse YubiKey secret manifest")
}
