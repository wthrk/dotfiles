//! storage モジュール分割後も維持する回帰テスト群。
//!
//! 公開 API の互換性と、wire format・暗号境界・操作フローの不変条件をまとめて検証する。

use std::collections::BTreeMap;

use anyhow::Context;
use zeroize::Zeroizing;

use super::model::{
    CheckName, CheckStatus, MANIFEST_APP, MANIFEST_OBJECT_ID, SecretBlob, SecretManifest,
};
use super::*;
use crate::Result;

struct FakeDevice {
    serial: u32,
    key_exists: bool,
    management_auth_ok: bool,
    management_auth_check_calls: usize,
    management_auth_write_calls: usize,
    write_fail_after: Option<usize>,
    objects: BTreeMap<u32, Zeroizing<Vec<u8>>>,
}

impl FakeDevice {
    fn new(serial: u32) -> Self {
        Self {
            serial,
            key_exists: false,
            management_auth_ok: true,
            management_auth_check_calls: 0,
            management_auth_write_calls: 0,
            write_fail_after: None,
            objects: BTreeMap::new(),
        }
    }

    fn set_write_fail_after(&mut self, allowed_writes: usize) {
        self.write_fail_after = Some(allowed_writes);
    }

    /// 実機は object 書き込みごとに management key 認証を要求するため、Fake でも同条件にする。
    fn authenticate_management_for_write(&mut self) -> Result<()> {
        self.management_auth_write_calls += 1;
        if !self.management_auth_ok {
            anyhow::bail!("management key authentication failed");
        }
        if let Some(remaining) = &mut self.write_fail_after {
            if *remaining == 0 {
                anyhow::bail!("management key authentication failed");
            }
            *remaining -= 1;
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

    fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()> {
        self.authenticate_management_for_write()?;
        self.objects
            .insert(object_id, Zeroizing::new(value.to_vec()));
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(key.iter().map(|byte| byte ^ 0xa5).collect()))
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.wrap_key(wrapped_key)
    }
}

#[test]
fn secret_name_rejects_unknown_name() {
    let parsed = serde_json::from_value::<SecretName>(serde_json::json!("github-token"));
    assert!(parsed.is_err());
}

#[test]
fn secret_names_match_design_object_mapping() {
    let objects: BTreeMap<_, _> = SecretName::iter()
        .map(|name| (secret_name(name), model::format_object_id(name.object_id())))
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
        nonce: [7; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![1, 2, 3]),
        ciphertext: Zeroizing::new(vec![4, 5, 6, 7]),
        tag: [9; model::TAG_LEN],
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
        nonce: [1; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![2]),
        ciphertext: Zeroizing::new(vec![3]),
        tag: [4; model::TAG_LEN],
    };
    let encoded = Zeroizing::new(
        blob.encode()?
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>(),
    );

    assert!(SecretBlob::decode(&encoded).is_err());
    Ok(())
}

#[test]
fn secret_blob_rejects_wrapped_key_length_larger_than_payload() -> Result<()> {
    let blob = SecretBlob {
        name: SecretName::BwEmail,
        nonce: [1; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![2, 3, 4]),
        ciphertext: Zeroizing::new(vec![5, 6]),
        tag: [7; model::TAG_LEN],
    };
    let mut encoded = blob.encode()?;
    let offset = model::BLOB_MAGIC.len() + 1 + 1 + 1 + model::NONCE_LEN;
    encoded[offset..offset + 2].copy_from_slice(&(4u16).to_be_bytes());

    assert!(SecretBlob::decode(&encoded).is_err());
    Ok(())
}

#[test]
fn secret_blob_rejects_wrapped_key_length_smaller_than_payload() -> Result<()> {
    let blob = SecretBlob {
        name: SecretName::BwEmail,
        nonce: [1; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![2, 3, 4]),
        ciphertext: Zeroizing::new(vec![5, 6]),
        tag: [7; model::TAG_LEN],
    };
    let mut encoded = blob.encode()?;
    let offset = model::BLOB_MAGIC.len() + 1 + 1 + 1 + model::NONCE_LEN;
    encoded[offset..offset + 2].copy_from_slice(&(2u16).to_be_bytes());

    assert!(SecretBlob::decode(&encoded).is_err());
    Ok(())
}

#[test]
fn secret_blob_rejects_ciphertext_length_larger_than_payload() -> Result<()> {
    let blob = SecretBlob {
        name: SecretName::BwEmail,
        nonce: [1; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![2, 3, 4]),
        ciphertext: Zeroizing::new(vec![5, 6]),
        tag: [7; model::TAG_LEN],
    };
    let mut encoded = blob.encode()?;
    let wrapped_len_offset = model::BLOB_MAGIC.len() + 1 + 1 + 1 + model::NONCE_LEN;
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
        nonce: [1; model::NONCE_LEN],
        wrapped_key: Zeroizing::new(vec![2, 3, 4]),
        ciphertext: Zeroizing::new(vec![5, 6]),
        tag: [7; model::TAG_LEN],
    };
    let mut encoded = blob.encode()?;
    let wrapped_len_offset = model::BLOB_MAGIC.len() + 1 + 1 + 1 + model::NONCE_LEN;
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
        .insert(MANIFEST_OBJECT_ID, Zeroizing::new(b"occupied".to_vec()));

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
fn enroll_rejects_empty_secret_before_setup() {
    let mut device = FakeDevice::new(1234);
    let secrets = BootstrapSecrets {
        bw_email: secret_bytes(b"user@example.com".to_vec()),
        bw_password: secret_bytes(Vec::new()),
        bws_access_token: secret_bytes(b"token".to_vec()),
    };

    assert!(enroll(&mut device, YubikeyRole::Primary, &secrets).is_err());
    assert!(!device.key_exists);
    assert!(device.objects.is_empty());
}

#[test]
fn put_get_and_verify_round_trip_through_device() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;

    put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;
    put(&mut device, SecretName::BwPassword, b"password", false)?;
    put(&mut device, SecretName::BwsAccessToken, b"token", false)?;

    assert_eq!(
        get(&mut device, SecretName::BwEmail)?.as_slice(),
        b"user@example.com"
    );
    assert_eq!(
        verify_local_storage(&mut device)?
            .checks
            .get(&CheckName::LocalStorage),
        Some(&CheckStatus::Ok)
    );
    assert_eq!(
        verify_local_storage(&mut device)?
            .checks
            .get(&CheckName::Bws),
        Some(&CheckStatus::Skipped)
    );
    assert_eq!(
        verify_local_storage(&mut device)?
            .checks
            .get(&CheckName::BwLogin),
        Some(&CheckStatus::Skipped)
    );
    Ok(())
}

#[test]
fn put_requires_force_for_existing_secret() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;
    put(&mut device, SecretName::BwsAccessToken, b"old", false)?;

    assert!(put(&mut device, SecretName::BwsAccessToken, b"new", false).is_err());
    put(&mut device, SecretName::BwsAccessToken, b"new", true)?;
    assert_eq!(
        get(&mut device, SecretName::BwsAccessToken)?.as_slice(),
        b"new"
    );
    Ok(())
}

#[test]
fn put_uses_management_auth_for_each_secret_write() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;
    device.management_auth_write_calls = 0;

    put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;
    put(&mut device, SecretName::BwPassword, b"password", false)?;

    assert_eq!(device.management_auth_write_calls, 2);
    Ok(())
}

#[test]
fn rotate_bws_token_preserves_other_secrets() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;
    put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;
    put(&mut device, SecretName::BwPassword, b"password", false)?;
    put(&mut device, SecretName::BwsAccessToken, b"old-token", false)?;

    let summary = rotate_bws_token(&mut device, b"new-token")?;

    assert_eq!(
        get(&mut device, SecretName::BwEmail)?.as_slice(),
        b"user@example.com"
    );
    assert_eq!(
        get(&mut device, SecretName::BwPassword)?.as_slice(),
        b"password"
    );
    assert_eq!(
        get(&mut device, SecretName::BwsAccessToken)?.as_slice(),
        b"new-token"
    );
    assert_eq!(
        summary.checks.get(&CheckName::LocalStorage),
        Some(&CheckStatus::Ok)
    );
    Ok(())
}

#[test]
fn rotate_uses_management_auth_for_token_replacement() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;
    put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;
    put(&mut device, SecretName::BwPassword, b"password", false)?;
    put(&mut device, SecretName::BwsAccessToken, b"old-token", false)?;
    device.management_auth_write_calls = 0;

    rotate_bws_token(&mut device, b"new-token")?;

    assert_eq!(device.management_auth_write_calls, 1);
    Ok(())
}

#[test]
fn enroll_fails_when_management_auth_breaks_during_secret_writes() {
    let mut device = FakeDevice::new(1234);
    device.set_write_fail_after(1);
    let secrets = BootstrapSecrets {
        bw_email: secret_bytes(b"user@example.com".to_vec()),
        bw_password: secret_bytes(b"password".to_vec()),
        bws_access_token: secret_bytes(b"token".to_vec()),
    };

    let result = enroll(&mut device, YubikeyRole::Primary, &secrets);

    assert!(result.is_err());
    assert_eq!(device.management_auth_check_calls, 1);
    assert!(device.management_auth_write_calls >= 2);
}

#[test]
fn decryption_fails_when_blob_is_replayed_to_different_serial() -> Result<()> {
    let mut source = FakeDevice::new(1234);
    setup(&mut source)?;
    put(&mut source, SecretName::BwEmail, b"user@example.com", false)?;

    let mut replay = FakeDevice::new(5678);
    replay.key_exists = true;
    replay.objects.insert(
        MANIFEST_OBJECT_ID,
        source
            .read_object(MANIFEST_OBJECT_ID)?
            .context("missing manifest")?,
    );
    replay.objects.insert(
        SecretName::BwEmail.object_id(),
        source
            .read_object(SecretName::BwEmail.object_id())?
            .context("missing secret blob")?,
    );

    assert!(get(&mut replay, SecretName::BwEmail).is_err());
    Ok(())
}

#[test]
fn decryption_fails_when_secret_blob_name_and_object_are_swapped() -> Result<()> {
    let mut device = FakeDevice::new(1234);
    setup(&mut device)?;
    put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;

    let email_blob = device
        .read_object(SecretName::BwEmail.object_id())?
        .context("missing bw-email blob")?;
    let mut tampered = SecretBlob::decode(&email_blob)?;
    tampered.name = SecretName::BwPassword;
    let tampered_encoded = tampered.encode()?;
    device.write_object(SecretName::BwPassword.object_id(), &tampered_encoded)?;

    assert!(get(&mut device, SecretName::BwPassword).is_err());
    Ok(())
}
