//! 実機 YubiKey PIV セッションを `SecretDevice` port へ接続する adapter。
//!
//! device の開き方・discovery・selection は呼び出し元（`process_boundary`）が担い、
//! この module は開かれた `YubiKey` session 上での PIV 操作だけを行う。

use anyhow::{bail, Context};
use rand_core::OsRng;
use rsa::{pkcs1::DecodeRsaPublicKey, Oaep, RsaPublicKey};
use sha2::Sha256;
use yubikey::{
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
    MgmKey, PinPolicy, TouchPolicy, Version, YubiKey,
};
use zeroize::Zeroizing;

use crate::secrets::{
    domain::PivObjectId,
    ports::SecretDevice,
    support::write_oaep_unpadded_sha256,
};
use crate::Result;

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};

/// 開いた YubiKey PIV session と PIN 検証状態を保持する実機 adapter。
///
/// PIN verification は 1 command 中に同じ session へ再利用する。
pub struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

impl YubikeySecretDevice {
    /// 開いた YubiKey session から device adapter を構築する。
    pub(super) fn from_yubikey(yubikey: YubiKey) -> Self {
        Self {
            yubikey,
            pin_verified: false,
        }
    }

    /// PIV private key operation に必要な PIN verification を実行する。
    ///
    /// 同じ command 中で検証済みの場合は、同じ session の検証状態を再利用する。
    fn verify_pin_once(&mut self, pin: &[u8]) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }

        self.yubikey.verify_pin(pin)?;
        self.pin_verified = true;
        Ok(())
    }

    /// 既定 management key で PIV management auth を実行する。
    ///
    /// 既定鍵運用のリスクは設計資料に明記し、任意 management key 対応は別設計にする。
    fn authenticate_management(&mut self) -> Result<()> {
        let key = MgmKey::get_default(&self.yubikey)?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    /// PIV metadata から secret storage key の public key を取得する。
    ///
    /// private key material は host へ出さない。
    fn public_key(&mut self) -> Result<RsaPublicKey> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")
    }
}

impl SecretDevice for YubikeySecretDevice {
    fn serial(&self) -> u32 {
        self.yubikey.serial().0
    }

    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(_) => Ok(true),
            Err(yubikey::Error::NotFound) => {
                match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
                    Ok(_) => Ok(true),
                    Err(yubikey::Error::NotFound) => Ok(false),
                    Err(err) => Err(err.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        let version = self.yubikey.version();
        if version_lt(version, MIN_PIV_METADATA_VERSION) {
            bail!(
                "YubiKey PIV application version must be at least {}",
                format_version(MIN_PIV_METADATA_VERSION)
            );
        }
        if self.yubikey.get_pin_retries()? == 0 {
            bail!("YubiKey PIN retries are exhausted");
        }
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        self.authenticate_management()
    }

    fn generate_key(&mut self) -> Result<()> {
        self.check_key_generation_preconditions()?;
        self.authenticate_management()?;
        piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Once,
            TouchPolicy::Always,
        )?;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object_id.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.authenticate_management()?;
        self.yubikey.save_object(object_id.value(), value)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        let public = self.public_key()?;
        Ok(public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), key)?)
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        self.verify_pin_once(pin)
    }

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        let decrypted = Zeroizing::new(piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?);
        let mut output = Zeroizing::new(Vec::new());
        write_oaep_unpadded_sha256(&decrypted, 256, &mut *output)?;
        Ok(output)
    }
}

/// 2 つの `yubikey::Version` を semantic version 順で比較する。
///
/// `yubikey::Version` に ordering がないため、PIV metadata 要件は tuple 比較で判定する。
fn version_lt(left: Version, right: Version) -> bool {
    (left.major, left.minor, left.patch) < (right.major, right.minor, right.patch)
}

/// PIV application version を dotted 表記の文字列へ変換する。
fn format_version(version: Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}
