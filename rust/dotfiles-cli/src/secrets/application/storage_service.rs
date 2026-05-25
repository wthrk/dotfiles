//! YubiKey bootstrap secret storage の操作フロー。
//!
//! setup / put / get / enroll / verify は同じ manifest sentinel と PIV object mapping を
//! 共有する。device adapter の実装差は `SecretDevice` trait に閉じ、ここでは永続書き込みの
//! 順序と検証結果の JSON 契約を固定する。
//!
//! 暗号処理（AEAD 暗号化・複号）と manifest の JSON serialize/deserialize はこのモジュール内に
//! 閉じ込め、adapter 具体型へは依存しない。

use std::io::Write;

use anyhow::{bail, Context};
use rand::Rng;

use crate::secrets::application::summary::{CheckName, CheckStatus, EnrollSummary, YubikeyRole};
use crate::secrets::domain::{
    PivObjectId, SecretBlob, SecretManifest, SecretName, StorageObjectIds, CONTENT_KEY_LEN,
    KEY_SLOT, NONCE_LEN,
};
use crate::secrets::ports::SecretDevice;
use crate::secrets::support::{
    aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
    protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
};
use crate::Result;

/// expected manifest を PIV object へ書き込む。
///
/// manifest は secret blob より先に書き、以後の put/get/verify が storage 所有権を判定する sentinel にする。
fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let mut manifest = serde_json::to_vec(&SecretManifest::expected())?;
    device.write_object(PivObjectId::MANIFEST, &mut manifest)
}

/// PIV object から manifest を読み出して parse する。
///
/// manifest が存在しない YubiKey は secret storage 未初期化として扱う。
fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(PivObjectId::MANIFEST)?
        .context("YubiKey secret manifest is missing")?;
    serde_json::from_slice(&manifest).context("failed to parse YubiKey secret manifest")
}

/// secret 本文を per-secret content key で暗号化し、保存用 blob を構築する。
///
/// content key は device public key で wrap し、AEAD additional data には secret 名由来の
/// 保存 context を使う。
fn encrypt_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
    session: &SecretSession,
) -> Result<SecretBlob> {
    let mut content_key = ProtectedInputBuffer::new(CONTENT_KEY_LEN, session)?;
    content_key.write_all(&[0; CONTENT_KEY_LEN])?;
    rand::rng().fill(content_key.as_mut_slice());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = aes_256_gcm_from_key(content_key.as_slice())?;
    let mut ciphertext = ProtectedInputBuffer::new(secret.len(), session)?;
    ciphertext.write_all(secret)?;
    let tag = encrypt_detached(
        &cipher,
        &nonce,
        &name.additional_data(device.serial()),
        ciphertext.as_mut_slice(),
    )?;

    let wrapped_key = device.wrap_key(content_key.as_slice())?;

    Ok(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext: ciphertext.as_slice().to_vec(),
        tag,
    })
}

/// 保存用 blob を検証し、secret 本文を保護済み値へ復号する。
///
/// 復号先 allocation は session の memory lock 範囲に含め、平文は `ProtectedSecret` の
/// closure API 以外へ渡さない。
fn decrypt_secret_protected<'session, D: SecretDevice>(
    device: &mut D,
    blob: &SecretBlob,
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let mut content_key = ProtectedInputBuffer::new(CONTENT_KEY_LEN + 1, session)?;
    let unwrapped_key = device.unwrap_key(&blob.wrapped_key)?;
    if unwrapped_key.len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }
    content_key.write_all(&*unwrapped_key)?;

    let cipher = aes_256_gcm_from_key(content_key.as_slice())?;
    let mut input = ProtectedInputBuffer::new(blob.ciphertext.len(), session)?;
    input.write_all(&blob.ciphertext)?;
    decrypt_detached(
        &cipher,
        &blob.nonce,
        &blob.name.additional_data(device.serial()),
        input.as_mut_slice(),
        &blob.tag,
    )
    .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob.name))?;
    input.into_protected_secret(session)
}

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
    let blob =
        SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {}", name))?;
    if blob.name != name {
        bail!("YubiKey secret blob name does not match requested {}", name);
    }
    Ok(blob)
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anyhow::Context;
    use zeroize::Zeroizing;

    use crate::secrets::domain::{
        PivObjectId, SecretBlob, SecretManifest, SecretName, BLOB_MAGIC, MANIFEST_APP, NONCE_LEN,
        TAG_LEN,
    };
    use crate::secrets::ports::SecretDevice;
    use crate::secrets::support::protection::SecretSession;
    use crate::Result;

    struct FakeDevice {
        serial: u32,
        key_exists: bool,
        management_auth_ok: bool,
        management_auth_check_calls: usize,
        management_auth_write_calls: usize,
        objects: BTreeMap<PivObjectId, Vec<u8>>,
    }

    impl FakeDevice {
        fn new(serial: u32) -> Self {
            Self {
                serial,
                key_exists: false,
                management_auth_ok: true,
                management_auth_check_calls: 0,
                management_auth_write_calls: 0,
                objects: BTreeMap::new(),
            }
        }

        /// 実機は object 書き込みごとに management key 認証を要求するため、Fake でも同条件にする。
        fn authenticate_management_for_write(&mut self) -> Result<()> {
            self.management_auth_write_calls += 1;
            if !self.management_auth_ok {
                anyhow::bail!("management key authentication failed");
            }
            Ok(())
        }
    }

    impl SecretDevice for FakeDevice {
        fn serial(&self) -> u32 {
            self.serial
        }

        fn key_exists(&mut self) -> Result<bool> {
            Ok(self.key_exists)
        }

        fn check_key_generation_preconditions(&mut self) -> Result<()> {
            Ok(())
        }

        fn check_management_auth_preconditions(&mut self) -> Result<()> {
            self.management_auth_check_calls += 1;
            if self.management_auth_ok {
                Ok(())
            } else {
                anyhow::bail!("management key authentication failed");
            }
        }

        fn generate_key(&mut self) -> Result<()> {
            self.key_exists = true;
            Ok(())
        }

        fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
            Ok(self.objects.get(&object_id).cloned())
        }

        fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
            self.authenticate_management_for_write()?;
            self.objects.insert(object_id, value.to_vec());
            Ok(())
        }

        fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
            Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
        }

        fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
            Ok(())
        }

        fn requires_pin_input(&self) -> bool {
            true
        }

        fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            Ok(Zeroizing::new(self.wrap_key(wrapped_key)?))
        }
    }

    fn with_stored_secret<R>(
        device: &mut FakeDevice,
        name: SecretName,
        borrow: impl FnOnce(&[u8]) -> R,
    ) -> Result<R> {
        let session = SecretSession::start()?;
        let secret = super::get_protected(device, name, &session)?;
        Ok(secret.with_secret(borrow))
    }

    #[test]
    fn secret_name_rejects_unknown_name() {
        let parsed = serde_json::from_value::<SecretName>(serde_json::json!("github-token"));
        assert!(parsed.is_err());
    }

    #[test]
    fn secret_names_match_design_object_mapping() {
        let objects: BTreeMap<_, _> = SecretName::iter()
            .map(|name| (name.to_string(), name.object_id().to_string()))
            .collect();

        assert_eq!(
            objects.get("bw-email").map(String::as_str),
            Some("0x005FFF17")
        );
        assert_eq!(
            objects.get("bw-password").map(String::as_str),
            Some("0x005FFF18")
        );
        assert_eq!(
            objects.get("bws-access-token").map(String::as_str),
            Some("0x005FFF19")
        );
    }

    #[test]
    fn manifest_is_format_sentinel_only() {
        let manifest = SecretManifest::expected();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.app, MANIFEST_APP);
        assert!(manifest.validate_expected().is_ok());
    }

    #[test]
    fn secret_blob_round_trips_binary_format() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwsAccessToken,
            nonce: [7; NONCE_LEN],
            wrapped_key: vec![1, 2, 3],
            ciphertext: vec![4, 5, 6, 7],
            tag: [9; TAG_LEN],
        };

        let encoded = blob.encode()?;
        let decoded = SecretBlob::decode(&encoded)?;

        assert_eq!(decoded, blob);
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_trailing_bytes() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: vec![2],
            ciphertext: vec![3],
            tag: [4; TAG_LEN],
        };
        let encoded = blob
            .encode()?
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_wrapped_key_length_larger_than_payload() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: vec![2, 3, 4],
            ciphertext: vec![5, 6],
            tag: [7; TAG_LEN],
        };
        let mut encoded = blob.encode()?;
        let offset = BLOB_MAGIC.len() + 1 + 1 + 1 + NONCE_LEN;
        encoded[offset..offset + 2].copy_from_slice(&(4u16).to_be_bytes());

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_wrapped_key_length_smaller_than_payload() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: vec![2, 3, 4],
            ciphertext: vec![5, 6],
            tag: [7; TAG_LEN],
        };
        let mut encoded = blob.encode()?;
        let offset = BLOB_MAGIC.len() + 1 + 1 + 1 + NONCE_LEN;
        encoded[offset..offset + 2].copy_from_slice(&(2u16).to_be_bytes());

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_ciphertext_length_larger_than_payload() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: vec![2, 3, 4],
            ciphertext: vec![5, 6],
            tag: [7; TAG_LEN],
        };
        let mut encoded = blob.encode()?;
        let wrapped_len_offset = BLOB_MAGIC.len() + 1 + 1 + 1 + NONCE_LEN;
        let ciphertext_len_offset = wrapped_len_offset + 2 + blob.wrapped_key.len();
        encoded[ciphertext_len_offset..ciphertext_len_offset + 4]
            .copy_from_slice(&(3u32).to_be_bytes());

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_ciphertext_length_smaller_than_payload() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: vec![2, 3, 4],
            ciphertext: vec![5, 6],
            tag: [7; TAG_LEN],
        };
        let mut encoded = blob.encode()?;
        let wrapped_len_offset = BLOB_MAGIC.len() + 1 + 1 + 1 + NONCE_LEN;
        let ciphertext_len_offset = wrapped_len_offset + 2 + blob.wrapped_key.len();
        encoded[ciphertext_len_offset..ciphertext_len_offset + 4]
            .copy_from_slice(&(1u32).to_be_bytes());

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn setup_stops_when_storage_object_exists() {
        let mut device = FakeDevice::new(1234);
        device
            .objects
            .insert(PivObjectId::MANIFEST, b"occupied".to_vec());

        assert!(super::setup(&mut device).is_err());
    }

    #[test]
    fn setup_stops_when_key_exists_without_manifest() {
        let mut device = FakeDevice::new(1234);
        device.key_exists = true;

        assert!(super::setup(&mut device).is_err());
    }

    #[test]
    fn setup_stops_when_management_auth_precondition_fails() {
        let mut device = FakeDevice::new(1234);
        device.management_auth_ok = false;

        assert!(super::setup(&mut device).is_err());
        assert!(!device.key_exists);
    }

    #[test]
    fn setup_uses_management_auth_for_precondition_and_manifest_write() -> Result<()> {
        let mut device = FakeDevice::new(1234);

        super::setup(&mut device)?;

        assert_eq!(device.management_auth_check_calls, 1);
        assert_eq!(device.management_auth_write_calls, 1);
        Ok(())
    }

    #[test]
    fn put_get_and_verify_round_trip_through_device() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;

        super::put(
            &mut device,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwPassword,
            b"password",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"token",
            false,
            &session,
        )?;

        with_stored_secret(&mut device, SecretName::BwEmail, |secret| {
            assert_eq!(secret, b"user@example.com")
        })?;
        for name in SecretName::iter() {
            with_stored_secret(&mut device, name, |secret| assert!(!secret.is_empty()))?;
        }
        Ok(())
    }

    #[test]
    fn put_requires_force_for_existing_secret() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;
        super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"old",
            false,
            &session,
        )?;

        assert!(super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"new",
            false,
            &session
        )
        .is_err());
        super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"new",
            true,
            &session,
        )?;
        with_stored_secret(&mut device, SecretName::BwsAccessToken, |secret| {
            assert_eq!(secret, b"new")
        })?;
        Ok(())
    }

    #[test]
    fn put_uses_management_auth_for_each_secret_write() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;
        device.management_auth_write_calls = 0;

        super::put(
            &mut device,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwPassword,
            b"password",
            false,
            &session,
        )?;

        assert_eq!(device.management_auth_write_calls, 2);
        Ok(())
    }

    #[test]
    fn rotate_bws_token_preserves_other_secrets() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;
        super::put(
            &mut device,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwPassword,
            b"password",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"old-token",
            false,
            &session,
        )?;

        super::replace_bws_token(&mut device, b"new-token", &session)?;

        with_stored_secret(&mut device, SecretName::BwEmail, |secret| {
            assert_eq!(secret, b"user@example.com")
        })?;
        with_stored_secret(&mut device, SecretName::BwPassword, |secret| {
            assert_eq!(secret, b"password")
        })?;
        with_stored_secret(&mut device, SecretName::BwsAccessToken, |secret| {
            assert_eq!(secret, b"new-token")
        })?;
        Ok(())
    }

    #[test]
    fn rotate_uses_management_auth_for_token_replacement() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;
        super::put(
            &mut device,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwPassword,
            b"password",
            false,
            &session,
        )?;
        super::put(
            &mut device,
            SecretName::BwsAccessToken,
            b"old-token",
            false,
            &session,
        )?;
        device.management_auth_write_calls = 0;

        super::replace_bws_token(&mut device, b"new-token", &session)?;

        assert_eq!(device.management_auth_write_calls, 1);
        Ok(())
    }

    #[test]
    fn decryption_fails_when_blob_is_replayed_to_different_serial() -> Result<()> {
        let session = SecretSession::start()?;
        let mut source = FakeDevice::new(1234);
        super::setup(&mut source)?;
        super::put(
            &mut source,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;

        let mut replay = FakeDevice::new(5678);
        replay.key_exists = true;
        replay.objects.insert(
            PivObjectId::MANIFEST,
            source
                .read_object(PivObjectId::MANIFEST)?
                .context("missing manifest")?,
        );
        replay.objects.insert(
            SecretName::BwEmail.object_id(),
            source
                .read_object(SecretName::BwEmail.object_id())?
                .context("missing secret blob")?,
        );

        assert!(with_stored_secret(&mut replay, SecretName::BwEmail, |_| ()).is_err());
        Ok(())
    }

    #[test]
    fn decryption_fails_when_secret_blob_name_and_object_are_swapped() -> Result<()> {
        let session = SecretSession::start()?;
        let mut device = FakeDevice::new(1234);
        super::setup(&mut device)?;
        super::put(
            &mut device,
            SecretName::BwEmail,
            b"user@example.com",
            false,
            &session,
        )?;

        let email_blob = device
            .read_object(SecretName::BwEmail.object_id())?
            .context("missing bw-email blob")?;
        let mut tampered = SecretBlob::decode(&email_blob)?;
        tampered.name = SecretName::BwPassword;
        let mut tampered_encoded = tampered.encode()?;
        device.write_object(SecretName::BwPassword.object_id(), &mut tampered_encoded)?;

        assert!(with_stored_secret(&mut device, SecretName::BwPassword, |_| ()).is_err());
        Ok(())
    }
}
