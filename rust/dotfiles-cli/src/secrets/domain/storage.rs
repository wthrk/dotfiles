//! YubiKey secret storage の I/O 観測値から実行 intent を確定する domain service。
//!
//! adapter はここで作られた intent を PIV I/O へ翻訳するだけに限定し、manifest 判定、
//! object 占有判定、上書き可否、欠落判定を再実装しない。

use crate::Result;

use super::{
    manifest::SecretManifest,
    piv::{PivObjectId, SecretStorageSpec, StorageObjectIds},
};

/// setup 前に adapter が観測すべき予約済み object ID 集合。
pub struct SecretStorageSetupProbe {
    object_ids: Vec<PivObjectId>,
}

/// setup 前の device/storage 観測値。
pub struct SecretStorageSetupInspection {
    pub key_exists: bool,
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

impl SecretStorageSetupIntent {
    /// setup 観測値に domain の未初期化規則を適用し、書き込み intent を作る。
    pub fn from_inspection(inspection: SecretStorageSetupInspection) -> Result<Self> {
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
    /// manifest 初期化済み規則だけを適用する通常書き込み intent を作る。
    pub fn store(
        storage: SecretStorageSpec,
        inspection: SecretStorageWriteInspection,
    ) -> Result<Self> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        Ok(Self { storage })
    }

    /// manifest 初期化済み規則と上書き可否規則を適用する `put` intent を作る。
    pub fn put(
        storage: SecretStorageSpec,
        inspection: SecretStorageWriteInspection,
        force: bool,
    ) -> Result<Self> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        storage
            .name
            .ensure_write_allowed(inspection.object_exists, force)?;
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
}
