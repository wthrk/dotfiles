//! YubiKey secret storage の I/O 観測値から実行 intent を確定する domain service。
//!
//! adapter はここで作られた intent を PIV I/O へ翻訳するだけに限定し、manifest 判定、
//! object 占有判定、上書き可否、欠落判定を再実装しない。

use crate::Result;

use super::{
    manifest::SecretManifest,
    material::SecretMaterial,
    piv::{
        PivApplicationVersion, PivObjectId, SecretStorageSpec, StorageObjectIds,
        validate_secret_storage_setup_preconditions,
    },
};

/// setup 前に adapter が観測すべき予約済み object ID 集合。
pub struct SecretStorageSetupProbe {
    object_ids: Vec<PivObjectId>,
}

/// setup 前の device/storage 観測値。
pub struct SecretStorageSetupInspection {
    pub key_exists: bool,
    pub piv_version: PivApplicationVersion,
    pub pin_retries: u8,
    pub manifest_bytes: Option<Vec<u8>>,
    pub occupied_object_ids: Vec<PivObjectId>,
}

/// domain rule を通過した storage 初期化 intent。
pub struct SecretStorageSetupIntent {
    pub manifest_bytes: Vec<u8>,
}

/// secret 書き込み前の storage 観測値。
pub struct SecretStorageWriteInspection {
    pub manifest_bytes: Option<Vec<u8>>,
    pub object_exists: bool,
}

/// domain rule を通過した secret 書き込み intent。
pub struct SecretStorageWriteIntent {
    pub storage: SecretStorageSpec,
}

/// secret 読み出し前の storage 観測値。
pub struct SecretStorageReadInspection {
    pub manifest_bytes: Option<Vec<u8>>,
    pub encoded: Option<Vec<u8>>,
}

/// domain rule を通過した secret 読み出し intent。
pub struct SecretStorageReadIntent {
    pub storage: SecretStorageSpec,
    pub encoded: Vec<u8>,
}

/// local storage 検証で読み出すべき secret 集合。
///
/// 「保存済み YubiKey storage が完了している」と判定するために必要な対象集合は
/// domain rule であり、use case はこの plan を順序制御へ適用するだけに限定する。
pub struct SecretStorageVerificationPlan {
    targets: [SecretStorageSpec; 3],
}

impl SecretStorageSetupProbe {
    /// 現行 storage version が予約する object ID 集合を返す。
    pub fn expected() -> Self {
        Self {
            object_ids: StorageObjectIds::iter().collect(),
        }
    }

    /// adapter が占有状態を確認する object ID を返す。
    pub fn object_ids(&self) -> &[PivObjectId] {
        &self.object_ids
    }
}

impl SecretStorageVerificationPlan {
    /// 指定 serial の local storage 完了検証対象を構築する。
    pub fn for_serial(serial: u32) -> Self {
        Self {
            targets: SecretStorageSpec::all_for_serial(serial),
        }
    }

    /// 検証対象 storage spec を安定順で返す。
    pub fn into_targets(self) -> [SecretStorageSpec; 3] {
        self.targets
    }
}

impl SecretStorageSetupIntent {
    /// setup 観測値に domain の未初期化規則を適用し、書き込み intent を作る。
    pub fn from_inspection(inspection: SecretStorageSetupInspection) -> Result<Self> {
        validate_secret_storage_setup_preconditions(
            inspection.piv_version,
            inspection.pin_retries,
        )?;
        SecretManifest::ensure_setup_allowed(
            inspection.key_exists,
            inspection.manifest_bytes.as_deref(),
            &inspection.occupied_object_ids,
        )?;
        Ok(Self {
            manifest_bytes: SecretManifest::expected().encode()?,
        })
    }
}

impl SecretStorageWriteIntent {
    /// secret 長に依存しない通常保存 preflight を実行する。
    ///
    /// rotate など入力取得前に拒否可能な storage 状態を確認したい use case は、本検査を
    /// secret 入力より前に通し、token payload を不要に process へ取り込まない。
    pub fn ensure_store_preconditions(inspection: &SecretStorageWriteInspection) -> Result<()> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        Ok(())
    }

    /// `put` のうち secret 長に依存しない manifest / object / force 規則を事前検査する。
    ///
    /// stdin など一度読むと戻せない secret 入力では、本検査を入力取得前に通して、
    /// 拒否可能な storage 状態で secret payload を process へ取り込まない。
    pub fn ensure_put_preconditions(
        storage: &SecretStorageSpec,
        inspection: &SecretStorageWriteInspection,
        force: bool,
    ) -> Result<()> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        storage
            .name
            .ensure_write_allowed(inspection.object_exists, force)
    }

    /// manifest 初期化済み規則と secret 値制約を適用する通常書き込み intent を作る。
    ///
    /// `secret_len` は平文 bytes を domain へ露出させずに、保存対象 secret の値制約だけを
    /// domain rule として判定するために渡す。
    pub fn store(
        storage: SecretStorageSpec,
        inspection: SecretStorageWriteInspection,
        secret_len: usize,
    ) -> Result<Self> {
        Self::ensure_store_preconditions(&inspection)?;
        storage.ensure_plaintext_len(secret_len)?;
        Ok(Self { storage })
    }

    /// manifest 初期化済み規則、上書き可否規則、secret 値制約を適用する `put` intent を作る。
    ///
    /// `secret_len` は protected secret の長さだけを使い、平文 bytes の内容は domain へ渡さない。
    pub fn put(
        storage: SecretStorageSpec,
        inspection: SecretStorageWriteInspection,
        force: bool,
        secret_len: usize,
    ) -> Result<Self> {
        Self::ensure_put_preconditions(&storage, &inspection, force)?;
        storage.ensure_plaintext_len(secret_len)?;
        Ok(Self { storage })
    }
}

impl SecretStorageReadIntent {
    /// manifest 初期化済み規則と対象 object の存在規則を適用する読み出し intent を作る。
    pub fn from_inspection(
        storage: SecretStorageSpec,
        inspection: SecretStorageReadInspection,
    ) -> Result<Self> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        let encoded = inspection.encoded.ok_or_else(|| storage.missing_error())?;
        Ok(Self { storage, encoded })
    }

    /// 復号処理の失敗を対象 secret の domain error へ変換する。
    pub fn decode_error(&self, error: anyhow::Error) -> anyhow::Error {
        self.storage.decode_error(error)
    }

    /// 復号済み secret が対象 storage の値制約を満たすことを確認する。
    pub fn validate_loaded_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.storage.ensure_plaintext_len(secret.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::domain::piv::SecretName;

    struct TestSecret {
        len: usize,
    }

    fn secret_with_len(len: usize) -> SecretMaterial {
        SecretMaterial::from_backend(
            TestSecret { len },
            |secret| secret.len,
            |secret| Ok(TestSecret { len: secret.len }),
        )
    }

    fn expected_manifest_bytes() -> Result<Vec<u8>> {
        SecretManifest::expected().encode()
    }

    fn minimum_piv_version() -> PivApplicationVersion {
        PivApplicationVersion::minimum_for_secret_storage()
    }

    fn clean_setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: minimum_piv_version(),
            pin_retries: 1,
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    fn write_inspection(manifest_bytes: Option<Vec<u8>>) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes,
            object_exists: false,
        }
    }

    fn read_inspection(
        manifest_bytes: Option<Vec<u8>>,
        encoded: Option<Vec<u8>>,
    ) -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes,
            encoded,
        }
    }

    fn storage() -> SecretStorageSpec {
        SecretName::BwEmail.storage_spec(12_345)
    }

    fn error_message<T>(result: Result<T>) -> Result<String> {
        match result {
            Ok(_) => anyhow::bail!("domain rule unexpectedly accepted invalid input"),
            Err(error) => Ok(error.to_string()),
        }
    }

    #[test]
    fn setup_intent_accepts_uninitialized_storage_and_emits_expected_manifest() -> Result<()> {
        let intent = SecretStorageSetupIntent::from_inspection(clean_setup_inspection())?;
        let manifest = SecretManifest::decode(&intent.manifest_bytes)?;

        assert_eq!(manifest, SecretManifest::expected());
        Ok(())
    }

    #[test]
    fn setup_intent_rejects_existing_manifest_or_occupied_object() -> Result<()> {
        let initialized = SecretStorageSetupInspection {
            key_exists: true,
            manifest_bytes: Some(expected_manifest_bytes()?),
            ..clean_setup_inspection()
        };
        let initialized_error =
            error_message(SecretStorageSetupIntent::from_inspection(initialized))?;
        assert!(initialized_error.contains("already initialized"));

        let occupied = SecretStorageSetupInspection {
            occupied_object_ids: vec![PivObjectId::MANIFEST],
            ..clean_setup_inspection()
        };
        let occupied_error = error_message(SecretStorageSetupIntent::from_inspection(occupied))?;
        assert!(occupied_error.contains("already exists"));
        Ok(())
    }

    #[test]
    fn setup_stops_when_storage_object_exists() -> Result<()> {
        let occupied = SecretStorageSetupInspection {
            occupied_object_ids: vec![PivObjectId::MANIFEST],
            ..clean_setup_inspection()
        };

        let error = error_message(SecretStorageSetupIntent::from_inspection(occupied))?;

        assert!(error.contains("already exists"));
        Ok(())
    }

    #[test]
    fn setup_stops_when_key_exists_without_manifest() -> Result<()> {
        let key_exists_without_manifest = SecretStorageSetupInspection {
            key_exists: true,
            ..clean_setup_inspection()
        };

        let error = error_message(SecretStorageSetupIntent::from_inspection(
            key_exists_without_manifest,
        ))?;

        assert!(error.contains("PIV slot is already initialized"));
        Ok(())
    }

    #[test]
    fn store_intent_requires_initialized_manifest_and_non_empty_secret() -> Result<()> {
        let storage = storage();
        let intent = SecretStorageWriteIntent::store(
            storage.clone(),
            write_inspection(Some(expected_manifest_bytes()?)),
            1,
        )?;
        assert_eq!(intent.storage, storage);

        let missing_manifest_error = error_message(SecretStorageWriteIntent::store(
            storage.clone(),
            write_inspection(None),
            1,
        ))?;
        assert!(missing_manifest_error.contains("manifest is missing"));

        let empty_secret_error = error_message(SecretStorageWriteIntent::store(
            storage,
            write_inspection(Some(expected_manifest_bytes()?)),
            0,
        ))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }

    #[test]
    fn put_intent_applies_overwrite_policy_before_accepting_existing_object() -> Result<()> {
        let storage = storage();
        let existing_object = SecretStorageWriteInspection {
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_exists: true,
        };

        let overwrite_error = error_message(SecretStorageWriteIntent::put(
            storage.clone(),
            existing_object,
            false,
            1,
        ))?;
        assert!(overwrite_error.contains("pass --force"));

        let forced = SecretStorageWriteIntent::put(
            storage.clone(),
            SecretStorageWriteInspection {
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_exists: true,
            },
            true,
            1,
        )?;
        assert_eq!(forced.storage, storage);
        Ok(())
    }

    #[test]
    fn put_requires_force_for_existing_secret() -> Result<()> {
        let storage = storage();
        let existing_object = SecretStorageWriteInspection {
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_exists: true,
        };

        let error = error_message(SecretStorageWriteIntent::put(
            storage.clone(),
            existing_object,
            false,
            1,
        ))?;
        assert!(error.contains("pass --force"));

        let forced = SecretStorageWriteIntent::put(
            storage.clone(),
            SecretStorageWriteInspection {
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_exists: true,
            },
            true,
            1,
        )?;
        assert_eq!(forced.storage, storage);
        Ok(())
    }

    #[test]
    fn read_intent_requires_initialized_manifest_and_existing_encoded_blob() -> Result<()> {
        let storage = storage();
        let encoded = b"encoded blob".to_vec();
        let intent = SecretStorageReadIntent::from_inspection(
            storage.clone(),
            read_inspection(Some(expected_manifest_bytes()?), Some(encoded.clone())),
        )?;
        assert_eq!(intent.storage, storage);
        assert_eq!(intent.encoded, encoded);

        let missing_blob_error = error_message(SecretStorageReadIntent::from_inspection(
            storage.clone(),
            read_inspection(Some(expected_manifest_bytes()?), None),
        ))?;
        assert!(missing_blob_error.contains("is not stored"));

        let missing_manifest_error = error_message(SecretStorageReadIntent::from_inspection(
            storage,
            read_inspection(None, Some(b"encoded blob".to_vec())),
        ))?;
        assert!(missing_manifest_error.contains("manifest is missing"));
        Ok(())
    }

    #[test]
    fn read_intent_maps_decode_error_and_validates_loaded_secret_length() -> Result<()> {
        let intent = SecretStorageReadIntent::from_inspection(
            storage(),
            read_inspection(
                Some(expected_manifest_bytes()?),
                Some(b"encoded blob".to_vec()),
            ),
        )?;

        let decode_error = intent.decode_error(anyhow::anyhow!("ciphertext rejected"));
        assert!(
            decode_error
                .to_string()
                .contains("failed to decode bw-email")
        );
        assert!(decode_error.to_string().contains("ciphertext rejected"));

        intent.validate_loaded_secret(&secret_with_len(1))?;
        let empty_secret_error = error_message(intent.validate_loaded_secret(&secret_with_len(0)))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }
}
