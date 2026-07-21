//! manifest の互換条件と bootstrap 文書対応を domain で固定し、storage 判定の揺れを防ぐ。
//!
//! SPKI decode は `rsa` 0.9.10 `DecodePublicKey` を使う。固定 version source と DER parse error を
//! opaque failure として扱う規則は
//! [`external-sdk-evidence.md`](../../../../docs/secret-recovery/external-sdk-evidence.md#rust-support-crate-secret-recovery-直接利用)
//! を参照する。

use std::collections::BTreeMap;

use anyhow::Result;
use rsa::{RsaPublicKey, pkcs8::DecodePublicKey, traits::PublicKeyParts};

use crate::support::protection::ProtectedSecret;

use super::{
    piv::{PivObjectId, SecretName, SecretStorageSpec},
    wire::ManifestWire,
};

/// bootstrap secret JSON 各 field に許可する最大 byte 長。
pub const BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT: usize = 16 * 1024;

/// manifest が dotfiles secret recovery 用であることを示す app id。
pub(crate) const MANIFEST_APP: &str = "dotfiles.secret-recovery";

/// PIV object に保存する secret storage manifest。
///
/// `version` と `app` の組が storage format の識別子であり、期待値との一致が初期化済み判定の不変条件になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretManifest {
    /// manifest format version。
    pub version: u8,
    /// この repository の secret storage manifest であることを示す app id。
    pub app: String,
    /// slot 82 の DER-encoded SubjectPublicKeyInfo。v2 では必須の非 secret metadata。
    pub slot_public_key_spki: Option<Vec<u8>>,
}

/// bootstrap enrollment で受け取る secret document の純粋な domain 表現。
///
/// 各 field は `ProtectedSecret` として保持し、平文文字列化は domain の責務に含めない。
pub struct BootstrapSecretDocument {
    pub bw_email: ProtectedSecret,
    pub bw_password: ProtectedSecret,
    pub bitwarden_client_id: ProtectedSecret,
    pub bitwarden_client_secret: ProtectedSecret,
}

impl SecretManifest {
    /// slot 82 の実際の公開鍵 metadata を持つ version 2 manifest を構築する。
    pub fn v2(slot_public_key_spki: Vec<u8>) -> Result<Self> {
        Self::validate_slot_public_key_spki(&slot_public_key_spki)?;
        Ok(Self {
            version: 2,
            app: MANIFEST_APP.to_owned(),
            slot_public_key_spki: Some(slot_public_key_spki),
        })
    }

    /// manifest が現在の storage format と一致することを確認する。
    ///
    /// `expected()` と一致しない値は互換性違反として失敗する。
    /// 呼び出し側は manifest の存在確認をこの検証の前後どちらで行うかを文脈に応じて選ぶ責務を負う。
    pub fn validate_expected(&self) -> Result<()> {
        let valid_v1 =
            self.version == 1 && self.app == MANIFEST_APP && self.slot_public_key_spki.is_none();
        let valid_v2 = self.version == 2 && self.app == MANIFEST_APP;
        if !valid_v1 && !valid_v2 {
            return Err(invalid_data(
                "YubiKey secret manifest does not match dotfiles secret-recovery format",
            )
            .into());
        }
        if self.version == 2 {
            let spki = self.slot_public_key_spki.as_deref().ok_or_else(|| {
                invalid_data("YubiKey secret manifest is missing slot 82 public key")
            })?;
            Self::validate_slot_public_key_spki(spki)?;
        }

        Ok(())
    }

    /// slot 82 に許可する RSA2048 SubjectPublicKeyInfo DER だけを受け入れる。
    pub fn validate_slot_public_key_spki(spki: &[u8]) -> Result<()> {
        let public_key = RsaPublicKey::from_public_key_der(spki)
            .map_err(|_| invalid_data("YubiKey slot 82 public key is not DER SPKI"))?;
        if public_key.size() != 256 {
            return Err(invalid_data("YubiKey slot 82 public key must be RSA2048").into());
        }
        Ok(())
    }

    pub fn slot_public_key_spki(&self) -> Option<&[u8]> {
        self.slot_public_key_spki.as_deref()
    }

    /// setup 実行前に storage layout が未初期化状態かを確認する。
    ///
    /// key slot、manifest object、予約済み object 群の関係だけを判定し、device への読み書き自体は含まない。
    /// 正常な version 2 storage は no-op とし、version 1 だけを metadata から v2 へ移行する。
    ///
    /// 新規 storage は key 生成後の実 SPKI で manifest を作る。key だけ、または manifest だけが
    /// 残る状態はこの経路で推測修復せず、呼び出し側が clear 可能な不整合として扱う。
    pub fn setup_requires_manifest_update(
        key_exists: bool,
        manifest_bytes: Option<&[u8]>,
        occupied_object_ids: &[PivObjectId],
    ) -> Result<bool> {
        if key_exists {
            if let Some(manifest_bytes) = manifest_bytes {
                let manifest = Self::decode(manifest_bytes)?;
                manifest.validate_expected()?;
                return Ok(manifest.version == 1);
            }
            return Err(invalid_data("YubiKey secret manifest is missing").into());
        }

        if let Some(object_id) = occupied_object_ids.first() {
            return Err(
                invalid_data(format!("YubiKey PIV object {} already exists", object_id)).into(),
            );
        }

        Ok(true)
    }

    /// manifest object が存在し、期待する storage format と一致することを確認する。
    ///
    /// `None` は未初期化ではなく「必要な manifest が欠落した異常」として失敗する。
    /// decode 後は `validate_expected` を必ず通し、期待形式以外の manifest を受け入れない。
    pub fn decode_initialized(manifest_bytes: Option<&[u8]>) -> Result<Self> {
        let manifest_bytes =
            manifest_bytes.ok_or_else(|| invalid_data("YubiKey secret manifest is missing"))?;
        let manifest = Self::decode(manifest_bytes)?;
        manifest.validate_expected()?;
        Ok(manifest)
    }

    /// manifest を JSON wire format に encode する。
    ///
    /// 現行 version では `ManifestWire` へ直列化する。
    /// 呼び出し側は返却 byte 列を manifest object 以外へ流用しない責務を負う。
    pub fn encode(&self) -> Result<Vec<u8>> {
        ManifestWire {
            version: self.version,
            app: self.app.clone(),
            slot_public_key_spki: self.slot_public_key_spki.clone(),
        }
        .encode_json()
    }

    /// manifest を JSON wire format から decode する。
    ///
    /// JSON 構造が壊れている場合と `app` sentinel が一致しない場合は失敗する。
    /// 呼び出し側はこの結果に対して必要に応じて `validate_expected` を追加で適用する責務を負う。
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let manifest = ManifestWire::decode_json(bytes)?;

        if manifest.app != MANIFEST_APP {
            return Err(invalid_data("failed to parse YubiKey secret manifest").into());
        }

        Ok(Self {
            version: manifest.version,
            app: manifest.app,
            slot_public_key_spki: manifest.slot_public_key_spki,
        })
    }
}

#[cfg(any(test, feature = "secrets-internal-test-stub"))]
impl SecretManifest {
    /// test-only の RSA2048 SPKI を持つ version 2 manifest を構築する。
    pub(crate) fn fixture_v2() -> Self {
        // 固定の公開 RSA2048 DER SPKI を使う。process ごとの RSA key generation は
        // feature-stub CLI child の起動を十秒単位で遅らせ、timeout が実際の child
        // lifecycle 不良かを判別不能にする。この値は test fixture の公開鍵だけであり、
        // 対応する private key や運用 secret は含まない。
        const FIXTURE_SPKI: &[u8] = &[
            0x30, 0x82, 0x01, 0x22, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d,
            0x01, 0x01, 0x01, 0x05, 0x00, 0x03, 0x82, 0x01, 0x0f, 0x00, 0x30, 0x82, 0x01, 0x0a,
            0x02, 0x82, 0x01, 0x01, 0x00, 0xb8, 0x12, 0x6e, 0x24, 0xa5, 0xf9, 0x88, 0xec, 0xeb,
            0xbd, 0xc8, 0x1c, 0xcc, 0x42, 0x02, 0xb7, 0xa2, 0x83, 0x2d, 0xf6, 0xa1, 0x9d, 0x53,
            0x03, 0x93, 0x4f, 0x56, 0x71, 0x71, 0x3a, 0x37, 0x43, 0x30, 0xab, 0xfd, 0xa2, 0x3f,
            0x05, 0xba, 0xcc, 0xd2, 0x15, 0x27, 0x07, 0xc2, 0x26, 0x8b, 0xa1, 0x9d, 0x3f, 0xd8,
            0xcd, 0x8f, 0xc3, 0x78, 0x80, 0xbc, 0x7e, 0xe0, 0x8a, 0x6f, 0xcc, 0x5c, 0x7d, 0x18,
            0x6b, 0xf6, 0x90, 0xa1, 0x9c, 0x1d, 0xf0, 0x95, 0xa7, 0x55, 0xd1, 0x5e, 0x38, 0x96,
            0xf9, 0x9f, 0xac, 0x54, 0xde, 0x9e, 0x2d, 0x62, 0x03, 0x4e, 0x7e, 0x9f, 0x7f, 0x88,
            0xe2, 0x9c, 0xc9, 0x14, 0xac, 0x58, 0x1c, 0x73, 0x3f, 0xac, 0x0c, 0xf8, 0x0f, 0x6c,
            0x37, 0x4f, 0x5d, 0xa5, 0x66, 0x2c, 0xd0, 0xb4, 0x13, 0x94, 0x5f, 0xd2, 0xb6, 0xff,
            0x09, 0x45, 0x96, 0xfc, 0xfa, 0x2a, 0x53, 0xa4, 0xe4, 0x03, 0xdf, 0xc4, 0x41, 0xd0,
            0xce, 0x14, 0xdd, 0x1e, 0x71, 0x86, 0x0c, 0x3c, 0xd8, 0x3f, 0x85, 0x9d, 0xe9, 0x3b,
            0xe3, 0xae, 0xbb, 0xce, 0x13, 0x23, 0x5c, 0x10, 0xe1, 0xfb, 0xb9, 0xe3, 0x07, 0xb0,
            0x92, 0xb3, 0x3f, 0x38, 0x4e, 0x83, 0xde, 0x17, 0x81, 0x07, 0xab, 0x3e, 0x83, 0x2a,
            0x25, 0x03, 0x97, 0x26, 0x92, 0x54, 0x24, 0x59, 0xb0, 0x6d, 0xa3, 0x3b, 0xe5, 0x17,
            0x51, 0xc7, 0xb5, 0x56, 0x07, 0x1a, 0x47, 0x20, 0x64, 0x58, 0x4e, 0xa1, 0xa6, 0xb9,
            0xe6, 0xce, 0xea, 0x73, 0x45, 0x87, 0x02, 0x55, 0xe3, 0x3c, 0xb6, 0xdf, 0x2e, 0xac,
            0x44, 0xe3, 0xcc, 0xd3, 0xe4, 0x2d, 0x87, 0xc5, 0xa7, 0xf0, 0x05, 0xd0, 0x80, 0xc5,
            0x6e, 0x8b, 0xd8, 0xdb, 0x08, 0x2f, 0xc5, 0x06, 0x6d, 0xa3, 0x72, 0xcc, 0x8c, 0x4f,
            0x94, 0xb8, 0x36, 0xf1, 0xe9, 0xdc, 0xa2, 0xfc, 0x41, 0x02, 0x03, 0x01, 0x00, 0x01,
        ];
        match Self::v2(FIXTURE_SPKI.to_vec()) {
            Ok(manifest) => manifest,
            Err(error) => panic!("test SPKI must be valid: {error}"),
        }
    }
}

impl BootstrapSecretDocument {
    /// protected JSON field map から bootstrap document を構築する。
    ///
    /// JSON field 名と domain secret の対応は bootstrap document schema の業務規則であり、
    /// adapter は JSON decode 後の map を渡すだけに限定する。
    pub fn from_field_map(mut fields: BTreeMap<String, ProtectedSecret>) -> Result<Self> {
        let missing = |field: &str| anyhow::anyhow!("JSON field `{field}` is missing");
        let bw_email = fields
            .remove("bw-email")
            .ok_or_else(|| missing("bw-email"))?;
        let bw_password = fields
            .remove("bw-password")
            .ok_or_else(|| missing("bw-password"))?;
        let bitwarden_client_id = fields
            .remove("bitwarden-client-id")
            .ok_or_else(|| missing("bitwarden-client-id"))?;
        let bitwarden_client_secret = fields
            .remove("bitwarden-client-secret")
            .ok_or_else(|| missing("bitwarden-client-secret"))?;

        Ok(Self {
            bw_email,
            bw_password,
            bitwarden_client_id,
            bitwarden_client_secret,
        })
    }

    /// 既に取得済みの `ProtectedSecret` 群から bootstrap document を構築する。
    pub fn from_secret_materials(
        bw_email: &ProtectedSecret,
        bw_password: &ProtectedSecret,
        bitwarden_client_id: &ProtectedSecret,
        bitwarden_client_secret: &ProtectedSecret,
    ) -> Result<Self> {
        Ok(Self {
            bw_email: ProtectedSecret::try_clone(bw_email)?,
            bw_password: ProtectedSecret::try_clone(bw_password)?,
            bitwarden_client_id: ProtectedSecret::try_clone(bitwarden_client_id)?,
            bitwarden_client_secret: ProtectedSecret::try_clone(bitwarden_client_secret)?,
        })
    }

    /// storage spec と復号済み secret の対応から bootstrap document を復元する。
    ///
    /// PIV object の読み出し順や変数名ではなく、`SecretStorageSpec::name` が持つ
    /// domain 対応を正本にして document field へ戻す。
    pub fn from_storage_materials(
        entries: [(SecretStorageSpec, ProtectedSecret); 4],
    ) -> Result<Self> {
        let mut bw_email = None;
        let mut bw_password = None;
        let mut bitwarden_client_id = None;
        let mut bitwarden_client_secret = None;

        for (storage, secret) in entries {
            let target = match storage.name {
                SecretName::BwEmail => &mut bw_email,
                SecretName::BwPassword => &mut bw_password,
                SecretName::BitwardenClientId => &mut bitwarden_client_id,
                SecretName::BitwardenClientSecret => &mut bitwarden_client_secret,
            };
            if target.replace(secret).is_some() {
                return Err(invalid_data(format!(
                    "duplicate bootstrap secret storage entry for {}",
                    storage.name
                ))
                .into());
            }
        }

        let missing = |name: SecretName| {
            invalid_data(format!(
                "bootstrap secret storage entry for {name} is missing"
            ))
        };
        Ok(Self {
            bw_email: bw_email.ok_or_else(|| missing(SecretName::BwEmail))?,
            bw_password: bw_password.ok_or_else(|| missing(SecretName::BwPassword))?,
            bitwarden_client_id: bitwarden_client_id
                .ok_or_else(|| missing(SecretName::BitwardenClientId))?,
            bitwarden_client_secret: bitwarden_client_secret
                .ok_or_else(|| missing(SecretName::BitwardenClientSecret))?,
        })
    }

    /// bootstrap document の 4 secrets を storage 固定順の `(SecretStorageSpec, value)` で返す。
    ///
    /// document field と YubiKey storage object の対応は domain rule なので、use case は
    /// field 名から object id / AAD 規則を再構築せず、この対応を保存手順へ適用する。
    pub fn storage_entries(&self, serial: u32) -> [(SecretStorageSpec, &ProtectedSecret); 4] {
        [
            (SecretName::BwEmail.storage_spec(serial), &self.bw_email),
            (
                SecretName::BwPassword.storage_spec(serial),
                &self.bw_password,
            ),
            (
                SecretName::BitwardenClientId.storage_spec(serial),
                &self.bitwarden_client_id,
            ),
            (
                SecretName::BitwardenClientSecret.storage_spec(serial),
                &self.bitwarden_client_secret,
            ),
        ]
    }
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_format_sentinel_only() {
        let manifest = SecretManifest::fixture_v2();

        assert_eq!(manifest.version, 2);
        assert_eq!(manifest.app, MANIFEST_APP);
        assert!(manifest.validate_expected().is_ok());
    }

    #[test]
    fn manifest_rejects_non_der_spki() {
        assert!(SecretManifest::v2(vec![1, 2, 3]).is_err());
    }

    #[test]
    fn manifest_rejects_non_rsa2048_spki() {
        use rand_core::OsRng;
        use rsa::{RsaPrivateKey, pkcs8::EncodePublicKey};

        let public_key_spki = RsaPrivateKey::new(&mut OsRng, 1024)
            .expect("test RSA1024 key")
            .to_public_key()
            .to_public_key_der()
            .expect("test SPKI")
            .as_bytes()
            .to_vec();

        assert!(SecretManifest::v2(public_key_spki).is_err());
    }
}
