//! YubiKey secret storage の I/O 観測値から実行 intent を確定する domain service。
//!
//! adapter はここで作られた intent を PIV I/O へ翻訳するだけに限定し、manifest 判定、
//! object 占有判定、上書き可否、欠落判定を再実装しない。

use crate::Result;
use crate::support::protection::ProtectedSecret;
use std::{error::Error, fmt};

use super::{
    manifest::SecretManifest,
    piv::{
        PivApplicationVersion, PivObjectId, SecretName, SecretStorageSpec, StorageObjectIds,
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
    pub manifest_bytes: Option<Vec<u8>>,
    pub occupied_object_ids: Vec<PivObjectId>,
}

/// domain rule を通過した storage 初期化 intent。
#[derive(Clone)]
pub struct SecretStorageSetupIntent {
    pub key_generation_required: bool,
    manifest_update_required: bool,
}

/// この機能が予約する storage だけを破棄する intent。
#[derive(Clone)]
pub struct SecretStorageClearIntent {
    pub object_ids: Vec<PivObjectId>,
}

/// secret 書き込み前の storage 観測値。
pub struct SecretStorageWriteInspection {
    pub manifest_bytes: Option<Vec<u8>>,
    /// SDK `fetch_object` が `NotFound` 以外で返したか。zero-length payload
    /// も physical PIV object として `true` にする。
    pub object_present: bool,
    /// non-empty encrypted blob が存在するか。`put --force` と `status` の
    /// 保存済み secret 判定は物理 object の存在ではなくこちらを使う。
    pub object_exists: bool,
    pub reserved_slot_key_exists: bool,
    pub reserved_slot_certificate_exists: bool,
    /// adapter が PIV metadata から観測した slot 82 の公開鍵 SPKI。取得不能も観測値として表す。
    pub slot_public_key_spki: Option<Vec<u8>>,
}

/// PIN も management-key authentication も行わない `status` の観測値。
///
/// `status` は recovery read path のまま、予約 PIV data object と manifest だけを列挙する。
/// slot metadata / key presence は GET METADATA の authorization contract を使わずに証明できないため、
/// この value に含めない。slot と manifest の SPKI 一致は管理 PIN を使う write/setup preflight と、
/// actual decrypt を行う recovery/verify path が検証する。
pub struct SecretStorageStatusInspection {
    pub manifest_bytes: Option<Vec<u8>>,
    /// SDK `fetch_object` が `NotFound` 以外で返したか。zero-length payload
    /// も physical PIV object として `true` にする。
    pub object_present: bool,
    /// non-empty encrypted blob が存在するか。保存済み secret 判定は物理 object
    /// の存在ではなくこちらを使う。
    pub object_exists: bool,
}

/// domain rule を通過した secret 書き込み intent。
#[derive(Clone)]
pub struct SecretStorageWriteIntent {
    pub storage: SecretStorageSpec,
    pub slot_public_key_spki: Option<Vec<u8>>,
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

/// YubiKey に設定済みの bootstrap secret 名一覧。
///
/// secret 本文や暗号化 blob を保持せず、設定済み object の名前だけを表す。
pub struct SecretStorageStatus {
    stored: Vec<super::piv::SecretName>,
}

/// `status` が観測済みの予約 storage 不整合を検出したことを表す domain error。
///
/// device discovery、PC/SC、USB など観測自体の失敗には使わない。呼び出し側はこの型だけを
/// 安定した CLI 終了コードへ変換できるため、script は観測不能な失敗を storage 不正と誤認しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretStorageStatusInvalid;

impl fmt::Display for SecretStorageStatusInvalid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("YubiKey secret storage is invalid")
    }
}

impl Error for SecretStorageStatusInvalid {}

/// `put` が、予約済み領域が完全に未初期化であることを観測したことを表す domain error。
///
/// provisioning script はこの型だけを `setup` の許可根拠にする。manifest 欠落に予約済み
/// object / key / certificate のいずれかが残る不正状態や、I/O 失敗はここへ写像しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretStorageUninitialized;

impl fmt::Display for SecretStorageUninitialized {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("YubiKey secret storage is uninitialized")
    }
}

impl Error for SecretStorageUninitialized {}

/// local storage 検証で読み出すべき secret 集合。
///
/// 「保存済み YubiKey storage が完了している」と判定するために必要な対象集合は
/// domain rule であり、use case はこの plan を順序制御へ適用するだけに限定する。
pub struct SecretStorageVerificationPlan {
    targets: [SecretStorageSpec; 1],
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
    /// 無対話復旧に必要な BWS access token だけを検証対象に構築する。
    pub fn for_serial(serial: u32) -> Self {
        Self {
            targets: [SecretName::BitwardenClientSecret.storage_spec(serial)],
        }
    }

    /// 検証対象 storage spec を安定順で返す。
    pub fn into_targets(self) -> [SecretStorageSpec; 1] {
        self.targets
    }
}

impl SecretStorageClearIntent {
    /// manifest と bootstrap secret object のみを clear 対象にする。
    pub fn expected() -> Self {
        Self {
            object_ids: StorageObjectIds::iter().collect(),
        }
    }

    pub fn manifest_for_generated_public_key(&self, public_key_spki: Vec<u8>) -> Result<Vec<u8>> {
        SecretManifest::v2(public_key_spki)?.encode()
    }
}

impl SecretStorageSetupIntent {
    /// setup 観測値に domain の未初期化規則を適用し、書き込み intent を作る。
    pub fn from_inspection(inspection: SecretStorageSetupInspection) -> Result<Self> {
        validate_secret_storage_setup_preconditions(inspection.piv_version)?;
        let manifest_update_required = SecretManifest::setup_requires_manifest_update(
            inspection.key_exists,
            inspection.manifest_bytes.as_deref(),
            &inspection.occupied_object_ids,
        )?;
        Ok(Self {
            key_generation_required: !inspection.key_exists,
            manifest_update_required,
        })
    }

    /// enrollment 用に、未初期化または manifest 未確定の partial storage だけを初期化対象とする。
    ///
    /// `setup` は正常な v2 storage に対して no-op だが、enroll は既存 bootstrap secret を
    /// 上書きしてはならない。したがって manifest が存在する storage は format の正否に関わらず
    /// enrollment 対象から除外する。manifest がまだ確定していない partial state は、途中失敗した
    /// enrollment を安全に再開するために許可する。
    pub fn for_enrollment(inspection: SecretStorageSetupInspection) -> Result<Self> {
        validate_secret_storage_setup_preconditions(inspection.piv_version)?;
        if inspection.manifest_bytes.is_some() {
            anyhow::bail!(
                "refusing to enroll into YubiKey secret storage with an existing manifest"
            );
        }
        Ok(Self {
            key_generation_required: !inspection.key_exists,
            manifest_update_required: true,
        })
    }

    /// 実 SPKI が必要な初期化または v1 移行かを返す。
    pub fn requires_public_key_spki(&self) -> bool {
        self.manifest_update_required
    }

    /// v2 manifest を確定すべきかを返す。
    pub fn requires_finalization(&self) -> bool {
        self.manifest_update_required
    }

    /// 実際に観測または生成した SPKI だけから v2 manifest bytes を作る。
    pub fn manifest_for_public_key(&self, public_key_spki: Vec<u8>) -> Result<Vec<u8>> {
        SecretManifest::v2(public_key_spki)?.encode()
    }
}

impl SecretStorageWriteIntent {
    /// secret 入力前に、manifest と slot 公開鍵照合に必要な intent を構築する。
    pub fn preflight_put(
        storage: SecretStorageSpec,
        inspection: &SecretStorageWriteInspection,
        force: bool,
    ) -> Result<Self> {
        Self::ensure_put_preconditions(&storage, inspection, force)?;
        Self::from_initialized_manifest(storage, inspection)
    }

    /// token 入力前に、通常更新の slot 公開鍵照合に必要な intent を構築する。
    pub fn preflight_store(
        storage: SecretStorageSpec,
        inspection: &SecretStorageWriteInspection,
    ) -> Result<Self> {
        Self::ensure_store_preconditions(inspection)?;
        Self::from_initialized_manifest(storage, inspection)
    }

    fn from_initialized_manifest(
        storage: SecretStorageSpec,
        inspection: &SecretStorageWriteInspection,
    ) -> Result<Self> {
        let manifest_spki = Self::validated_slot_public_key_spki(inspection)?;
        Ok(Self {
            storage,
            slot_public_key_spki: Some(manifest_spki),
        })
    }

    fn validated_slot_public_key_spki(
        inspection: &SecretStorageWriteInspection,
    ) -> Result<Vec<u8>> {
        let manifest = SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())?;
        let manifest_spki = manifest.slot_public_key_spki().ok_or_else(|| {
            anyhow::anyhow!("YubiKey secret storage manifest v1 cannot be used without migration")
        })?;
        SecretManifest::validate_slot_public_key_spki(manifest_spki)?;
        let observed_spki = inspection
            .slot_public_key_spki
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("YubiKey slot 82 public key metadata is unavailable"))?;
        SecretManifest::validate_slot_public_key_spki(observed_spki)?;
        if observed_spki != manifest_spki {
            anyhow::bail!("YubiKey slot 82 public key does not match secret storage manifest");
        }
        Ok(manifest_spki.to_owned())
    }

    /// secret 長に依存しない通常保存 preflight を実行する。
    ///
    /// rotate など入力取得前に拒否可能な storage 状態を確認したい use case は、本検査を
    /// secret 入力より前に通し、token payload を不要に process へ取り込まない。
    pub fn ensure_store_preconditions(inspection: &SecretStorageWriteInspection) -> Result<()> {
        Self::validated_slot_public_key_spki(inspection)?;
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
        if inspection.manifest_bytes.is_none()
            && !inspection.object_present
            && !inspection.reserved_slot_key_exists
            && !inspection.reserved_slot_certificate_exists
        {
            return Err(anyhow::Error::new(SecretStorageUninitialized));
        }
        Self::from_initialized_manifest(storage.clone(), inspection)?;
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
        Self::from_initialized_manifest(storage, &inspection)
    }

    /// manifest 確定前の enrollment object 書き込み intent を作る。
    ///
    /// 高水準 enroll は全 secret object を上書き可能な初期書き込みとして保存してから manifest を
    /// 確定する。これにより、manifest なし partial state は同じ enroll 再実行で再開できる。
    pub fn initial_enroll_store(
        storage: SecretStorageSpec,
        secret_len: usize,
        slot_public_key_spki: Vec<u8>,
    ) -> Result<Self> {
        storage.ensure_plaintext_len(secret_len)?;
        SecretManifest::validate_slot_public_key_spki(&slot_public_key_spki)?;
        Ok(Self {
            storage,
            slot_public_key_spki: Some(slot_public_key_spki),
        })
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
        Self::from_initialized_manifest(storage, &inspection)
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
    pub fn validate_loaded_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        self.storage.ensure_plaintext_len(secret.len())
    }
}

impl SecretStorageStatus {
    /// manifest 初期化済み規則を確認し、各予約 object の存在から設定済み secret 名を構築する。
    ///
    /// 正常な manifest がある storage は、予約 object の任意 subset を保存済み状態として扱う。
    /// `status` は完了性を判定せず、実際に保存済みの名前だけを報告する。manifest 不正・欠落時の
    /// 予約 object の不整合を storage 不正として返す。PIN を要求しない status は slot metadata / key
    /// presence を観測しないため、それらの整合性をここで主張しない。
    pub fn from_inspections(
        inspections: impl IntoIterator<Item = (SecretStorageSpec, SecretStorageStatusInspection)>,
    ) -> Result<Self> {
        let mut stored = Vec::new();
        let mut manifest_missing = false;
        let mut reserved_object_exists = false;
        for (storage, inspection) in inspections {
            if inspection.manifest_bytes.is_none() {
                manifest_missing = true;
            } else {
                // `status` が既に読み出した予約 manifest の形式不正は storage 不正であり、
                // transport / device discovery 失敗とは区別する。
                let manifest =
                    SecretManifest::decode_initialized(inspection.manifest_bytes.as_deref())
                        .map_err(|_| anyhow::Error::new(SecretStorageStatusInvalid))?;
                let _ = manifest;
            }
            if inspection.object_exists {
                stored.push(storage.name);
            }
            reserved_object_exists |= inspection.object_present;
        }
        if manifest_missing {
            if stored.is_empty() && !reserved_object_exists {
                return Ok(Self { stored });
            }
            return Err(anyhow::Error::new(SecretStorageStatusInvalid));
        }
        Ok(Self { stored })
    }

    /// 安定順で設定済みの secret 名を返す。
    pub fn stored(&self) -> &[super::piv::SecretName] {
        &self.stored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::piv::SecretName;

    fn secret_with_len(len: usize) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(&vec![0; len]).expect("test secret")
    }

    fn expected_manifest_bytes() -> Result<Vec<u8>> {
        SecretManifest::fixture_v2().encode()
    }

    fn minimum_piv_version() -> PivApplicationVersion {
        PivApplicationVersion::minimum_for_secret_storage()
    }

    fn clean_setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: minimum_piv_version(),
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    fn write_inspection(manifest_bytes: Option<Vec<u8>>) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes,
            object_present: false,
            object_exists: false,
            reserved_slot_key_exists: false,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("SPKI"),
            ),
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
        SecretName::BitwardenClientSecret.storage_spec(12_345)
    }

    fn error_message<T>(result: Result<T>) -> Result<String> {
        match result {
            Ok(_) => anyhow::bail!("domain rule unexpectedly accepted invalid input"),
            Err(error) => Ok(error.to_string()),
        }
    }

    #[test]
    fn setup_intent_requires_key_generation_for_uninitialized_storage() -> Result<()> {
        let intent = SecretStorageSetupIntent::from_inspection(clean_setup_inspection())?;

        assert!(intent.key_generation_required);
        assert!(intent.requires_public_key_spki());
        assert_eq!(
            SecretManifest::decode(
                &intent.manifest_for_public_key(
                    SecretManifest::fixture_v2()
                        .slot_public_key_spki
                        .expect("fixture SPKI"),
                )?
            )?,
            SecretManifest::fixture_v2()
        );
        Ok(())
    }

    #[test]
    fn setup_intent_leaves_normal_v2_storage_unchanged_but_rejects_occupied_uninitialized_storage()
    -> Result<()> {
        let initialized = SecretStorageSetupInspection {
            key_exists: true,
            manifest_bytes: Some(expected_manifest_bytes()?),
            ..clean_setup_inspection()
        };
        let initialized_intent = SecretStorageSetupIntent::from_inspection(initialized)?;
        assert!(!initialized_intent.key_generation_required);
        assert!(
            !initialized_intent.requires_public_key_spki(),
            "normal v2 storage must not require a metadata read or manifest rewrite"
        );

        let occupied = SecretStorageSetupInspection {
            occupied_object_ids: vec![PivObjectId::MANIFEST],
            ..clean_setup_inspection()
        };
        let occupied_error = error_message(SecretStorageSetupIntent::from_inspection(occupied))?;
        assert!(occupied_error.contains("already exists"));
        Ok(())
    }

    #[test]
    fn enrollment_intent_rejects_manifested_storage_but_allows_manifestless_partial_state()
    -> Result<()> {
        let existing_manifest = SecretStorageSetupInspection {
            key_exists: true,
            manifest_bytes: Some(expected_manifest_bytes()?),
            ..clean_setup_inspection()
        };
        let error = error_message(SecretStorageSetupIntent::for_enrollment(existing_manifest))?;
        assert!(error.contains("refusing to enroll"));

        let partial = SecretStorageSetupInspection {
            key_exists: true,
            occupied_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
            ..clean_setup_inspection()
        };
        let intent = SecretStorageSetupIntent::for_enrollment(partial)?;
        assert!(!intent.key_generation_required);
        assert!(intent.requires_finalization());
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
    fn setup_rejects_key_without_manifest_instead_of_guessing_a_storage_format() -> Result<()> {
        let key_exists_without_manifest = SecretStorageSetupInspection {
            key_exists: true,
            ..clean_setup_inspection()
        };

        let error = error_message(SecretStorageSetupIntent::from_inspection(
            key_exists_without_manifest,
        ))?;

        assert!(error.contains("manifest is missing"));
        Ok(())
    }

    #[test]
    fn setup_migrates_only_v1_manifest_with_observed_public_key_metadata() -> Result<()> {
        let v1 = SecretManifest {
            version: 1,
            app: crate::domain::manifest::MANIFEST_APP.to_owned(),
            slot_public_key_spki: None,
        }
        .encode()?;
        let intent = SecretStorageSetupIntent::from_inspection(SecretStorageSetupInspection {
            key_exists: true,
            manifest_bytes: Some(v1),
            ..clean_setup_inspection()
        })?;

        assert!(!intent.key_generation_required);
        assert!(intent.requires_public_key_spki());
        assert!(intent.requires_finalization());
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
    fn initial_enroll_store_allows_manifestless_retry_but_keeps_secret_length_rule() -> Result<()> {
        let storage = storage();
        let intent = SecretStorageWriteIntent::initial_enroll_store(
            storage.clone(),
            1,
            SecretManifest::fixture_v2()
                .slot_public_key_spki
                .expect("SPKI"),
        )?;
        assert_eq!(intent.storage, storage);

        let empty_secret_error = error_message(SecretStorageWriteIntent::initial_enroll_store(
            storage,
            0,
            SecretManifest::fixture_v2()
                .slot_public_key_spki
                .expect("SPKI"),
        ))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }

    #[test]
    fn put_intent_applies_overwrite_policy_before_accepting_existing_object() -> Result<()> {
        let storage = storage();
        let existing_object = SecretStorageWriteInspection {
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_present: true,
            object_exists: true,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("SPKI"),
            ),
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
                object_present: true,
                object_exists: true,
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(
                    SecretManifest::fixture_v2()
                        .slot_public_key_spki
                        .expect("SPKI"),
                ),
            },
            true,
            1,
        )?;
        assert_eq!(forced.storage, storage);
        Ok(())
    }

    #[test]
    fn put_preflight_identifies_only_completely_uninitialized_storage() -> Result<()> {
        let error = SecretStorageWriteIntent::ensure_put_preconditions(
            &storage(),
            &write_inspection(None),
            false,
        )
        .expect_err("completely empty storage must require setup");
        assert!(
            error
                .chain()
                .any(|cause| cause.is::<SecretStorageUninitialized>())
        );

        let mut manifestless_key = write_inspection(None);
        manifestless_key.reserved_slot_key_exists = true;
        let error = SecretStorageWriteIntent::ensure_put_preconditions(
            &storage(),
            &manifestless_key,
            false,
        )
        .expect_err("manifestless key residue must remain an ordinary failure");
        assert!(
            !error
                .chain()
                .any(|cause| cause.is::<SecretStorageUninitialized>())
        );
        Ok(())
    }

    #[test]
    fn put_requires_force_for_existing_secret() -> Result<()> {
        let storage = storage();
        let existing_object = SecretStorageWriteInspection {
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_present: true,
            object_exists: true,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("SPKI"),
            ),
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
                object_present: true,
                object_exists: true,
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(
                    SecretManifest::fixture_v2()
                        .slot_public_key_spki
                        .expect("SPKI"),
                ),
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
                .contains("failed to decode bitwarden-client-secret")
        );
        assert!(decode_error.to_string().contains("ciphertext rejected"));

        intent.validate_loaded_secret(&secret_with_len(1))?;
        let empty_secret_error = error_message(intent.validate_loaded_secret(&secret_with_len(0)))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }
}
