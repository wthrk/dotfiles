//! application storage service が保つ wire format、暗号境界、YubiKey 書き込み認証契約を検証する。

use std::collections::BTreeMap;

use crate::Result;
use crate::secrets::application::storage_service::{get_protected, put, replace_bws_token, setup};
use crate::secrets::domain::{
    BLOB_MAGIC, MANIFEST_APP, NONCE_LEN, PivObjectId, SecretBlob, SecretManifest, SecretName,
    TAG_LEN,
};
use crate::secrets::ports::SecretDevice;
use crate::secrets::support::protection::SecretSession;
use anyhow::Context;

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

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>> {
        self.wrap_key(wrapped_key)
    }
}

fn with_stored_secret<R>(
    device: &mut FakeDevice,
    name: SecretName,
    borrow: impl FnOnce(&[u8]) -> R,
) -> Result<R> {
    let session = SecretSession::start()?;
    let secret = get_protected(device, name, &session)?;
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

    assert!(setup(&mut device).is_err());
}

#[test]
fn setup_stops_when_key_exists_without_manifest() {
    let mut device = FakeDevice::new(1234);
    device.key_exists = true;

    assert!(setup(&mut device).is_err());
}

#[test]
fn setup_stops_when_management_auth_precondition_fails() {
    let mut device = FakeDevice::new(1234);
    device.management_auth_ok = false;

    assert!(setup(&mut device).is_err());
    assert!(!device.key_exists);
}

#[test]
fn setup_uses_management_auth_for_precondition_and_manifest_write() -> Result<()> {
    let mut device = FakeDevice::new(1234);

    setup(&mut device)?;

    assert_eq!(device.management_auth_check_calls, 1);
    assert_eq!(device.management_auth_write_calls, 1);
    Ok(())
}

#[test]
fn put_get_and_verify_round_trip_through_device() -> Result<()> {
    let session = SecretSession::start()?;
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;

    put(
        &mut device,
        SecretName::BwEmail,
        b"user@example.com",
        false,
        &session,
    )?;
    put(
        &mut device,
        SecretName::BwPassword,
        b"password",
        false,
        &session,
    )?;
    put(
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
    setup(&mut device)?;
    put(
        &mut device,
        SecretName::BwsAccessToken,
        b"old",
        false,
        &session,
    )?;

    assert!(
        put(
            &mut device,
            SecretName::BwsAccessToken,
            b"new",
            false,
            &session
        )
        .is_err()
    );
    put(
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
    setup(&mut device)?;
    device.management_auth_write_calls = 0;

    put(
        &mut device,
        SecretName::BwEmail,
        b"user@example.com",
        false,
        &session,
    )?;
    put(
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
    setup(&mut device)?;
    put(
        &mut device,
        SecretName::BwEmail,
        b"user@example.com",
        false,
        &session,
    )?;
    put(
        &mut device,
        SecretName::BwPassword,
        b"password",
        false,
        &session,
    )?;
    put(
        &mut device,
        SecretName::BwsAccessToken,
        b"old-token",
        false,
        &session,
    )?;

    replace_bws_token(&mut device, b"new-token", &session)?;

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
    setup(&mut device)?;
    put(
        &mut device,
        SecretName::BwEmail,
        b"user@example.com",
        false,
        &session,
    )?;
    put(
        &mut device,
        SecretName::BwPassword,
        b"password",
        false,
        &session,
    )?;
    put(
        &mut device,
        SecretName::BwsAccessToken,
        b"old-token",
        false,
        &session,
    )?;
    device.management_auth_write_calls = 0;

    replace_bws_token(&mut device, b"new-token", &session)?;

    assert_eq!(device.management_auth_write_calls, 1);
    Ok(())
}

#[test]
fn decryption_fails_when_blob_is_replayed_to_different_serial() -> Result<()> {
    let session = SecretSession::start()?;
    let mut source = FakeDevice::new(1234);
    setup(&mut source)?;
    put(
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
    setup(&mut device)?;
    put(
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
