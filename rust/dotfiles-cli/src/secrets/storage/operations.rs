//! YubiKey bootstrap secret storage の操作フロー。
//!
//! setup / put / get / enroll / verify の境界を維持し、manifest 整合性確認と呼び出し
//! エラー契約をこのモジュールで一元管理する。

use anyhow::{Context, bail};
use zeroize::Zeroizing;

use crate::Result;

use super::crypto::{decrypt_secret, encrypt_secret};
use super::model::{
    BootstrapSecrets, CheckName, CheckStatus, EnrollSummary, KEY_SLOT, MANIFEST_OBJECT_ID,
    SecretBlob, SecretDevice, SecretManifest, SecretName, VerifySummary, YubikeyRole,
    format_object_id, storage_object_ids,
};

/// エラーメッセージと summary では serde/CLI と同じ kebab-case 名を使う。
pub(crate) fn secret_name(name: SecretName) -> String {
    name.to_string()
}

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

    for object_id in storage_object_ids() {
        if device.read_object(object_id)?.is_some() {
            bail!(
                "YubiKey PIV object {} already exists",
                format_object_id(object_id)
            );
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
        bail!("{} must not be empty", secret_name(name));
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
        bail!(
            "{} already exists; pass --force to replace it",
            secret_name(name)
        );
    }
    Ok(())
}

/// 1 secret を YubiKey object から読み出して復号する。
///
/// blob 内の secret id と要求された secret 名が一致しない場合は拒否する。
pub fn get<D: SecretDevice>(device: &mut D, name: SecretName) -> Result<Zeroizing<Vec<u8>>> {
    read_manifest(device)?.validate_expected()?;
    let encoded = device
        .read_object(name.object_id())?
        .with_context(|| format!("{} is not stored on this YubiKey", secret_name(name)))?;
    let blob = SecretBlob::decode(&encoded)
        .with_context(|| format!("failed to decode {}", secret_name(name)))?;
    if blob.name != name {
        bail!(
            "YubiKey secret blob name does not match requested {}",
            secret_name(name)
        );
    }

    decrypt_secret(device, &blob)
}

/// primary または spare として bootstrap secret 一式を登録する。
///
/// setup、3 secret 保存、local verify をこの順に実行し、成功した確認項目だけを
/// summary に含める。
pub fn enroll<D: SecretDevice>(
    device: &mut D,
    role: YubikeyRole,
    secrets: &BootstrapSecrets,
) -> Result<EnrollSummary> {
    for name in SecretName::iter() {
        if secrets.get(name).is_empty() {
            bail!("{} must not be empty", secret_name(name));
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

/// BWS access token だけを置き換え、local storage を再検証する。
pub fn rotate_bws_token<D: SecretDevice>(device: &mut D, token: &[u8]) -> Result<VerifySummary> {
    put(device, SecretName::BwsAccessToken, token, true)?;
    verify_local_storage(device)
}

/// `rotate-bws-token` の token 入力前に、local storage が更新可能か確認する。
pub fn check_rotate_preconditions<D: SecretDevice>(device: &mut D) -> Result<()> {
    verify_local_storage(device)?;
    device.check_management_auth_preconditions()
}

/// YubiKey 上の manifest と 3 secret の復号可能性を検証する。
///
/// local storage verification は外部 service へ接続せず、BWS / Bitwarden login を未実行項目として残す。
pub fn verify_local_storage<D: SecretDevice>(device: &mut D) -> Result<VerifySummary> {
    read_manifest(device)?.validate_expected()?;
    for name in SecretName::iter() {
        let secret = get(device, name)?;
        if secret.is_empty() {
            bail!("{} stored on this YubiKey is empty", secret_name(name));
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

/// manifest は secret blob より先に書き、以後の put/get/verify の storage sentinel にする。
fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let manifest = serde_json::to_vec(&SecretManifest::expected())?;
    device.write_object(MANIFEST_OBJECT_ID, &manifest)
}

/// manifest が存在しない YubiKey は secret storage 未初期化として扱う。
fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(MANIFEST_OBJECT_ID)?
        .context("YubiKey secret manifest is missing")?;
    serde_json::from_slice(&manifest).context("failed to parse YubiKey secret manifest")
}
