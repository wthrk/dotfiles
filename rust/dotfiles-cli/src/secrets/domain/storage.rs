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
    /// manifest 初期化済み規則と secret 値制約を適用する通常書き込み intent を作る。
    ///
    /// `secret_len` は平文 bytes を domain へ露出させずに、保存対象 secret の値制約だけを
    /// domain rule として判定するために渡す。
    pub fn store(
        storage: SecretStorageSpec,
        inspection: SecretStorageWriteInspection,
        secret_len: usize,
    ) -> Result<Self> {
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
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
        SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        storage
            .name
            .ensure_write_allowed(inspection.object_exists, force)?;
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
