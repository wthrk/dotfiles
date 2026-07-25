//! YubiKey secret storage の I/O 観測値から実行 intent を確定する domain service。
//!
//! adapter はここで作られた intent を PIV I/O へ翻訳するだけに限定し、manifest 判定、
//! object 占有判定、上書き可否、欠落判定を再実装しない。

use crate::Result;
use crate::foundation::protection::ProtectedSecret;
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
    /// slot 82 metadata が公開鍵を返したという独立観測。
    pub reserved_slot_key_exists: bool,
    /// slot 82 certificate object が non-empty だったという独立観測。
    pub reserved_slot_certificate_exists: bool,
    /// slot 82 metadata が返した実公開鍵 SPKI。manifest との照合は domain が行う。
    pub slot_public_key_spki: Option<Vec<u8>>,
    pub piv_version: PivApplicationVersion,
    /// non-empty manifest payload。zero-length object は `None` のまま
    /// `present_object_ids` で physical presence を保持する。
    pub manifest_bytes: Option<Vec<u8>>,
    /// SDK が `NotFound` 以外で返した予約 object ID。zero-length object も含む。
    pub present_object_ids: Vec<PivObjectId>,
    /// non-empty payload を持つ予約 object ID。
    pub nonempty_object_ids: Vec<PivObjectId>,
}

/// domain rule を通過した storage 初期化 intent。
#[derive(Clone)]
pub struct SecretStorageSetupIntent {
    pub key_generation_required: bool,
    manifest_update_required: bool,
    piv_pin_change_required: bool,
    enrollment_state: Option<SecretStorageEnrollmentState>,
}

/// enrollment が受け入れる storage lifecycle state。
///
/// `Fresh` は slot/object/manifest が物理的に空の状態、`InitializedV2` は slot 82 key と
/// version 2 manifest が確定済みの状態だけを表す。version 1 と manifestless partial state は
/// enrollment の暗黙 migration/resume 対象に含めない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SecretStorageEnrollmentState {
    Fresh,
    InitializedV2,
}

/// この機能が予約する storage だけを破棄する intent。
#[derive(Clone)]
pub struct SecretStorageClearIntent {
    pub object_ids: Vec<PivObjectId>,
}

/// secret 書き込み前の storage 観測値。
pub struct SecretStorageWriteInspection {
    /// manifest object の physical presence。zero-length payload も `true`。
    pub manifest_present: bool,
    /// non-empty manifest payload。
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
    /// manifest object の physical presence。zero-length payload も `true`。
    pub manifest_present: bool,
    /// non-empty manifest payload。
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

/// 認証済み完全 inspection でも repository ownership を証明できない予約 slot 状態。
///
/// manifest と予約 object が空なのに slot 82 key/certificate が存在する場合、その key が
/// repository の途中失敗で生成されたものか別用途の既存 key かを区別できない。caller は同一 command
/// で retry、resume、clear、上書き、再初期化を行わず、管理者の手動確認へ escalation する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretStorageOwnershipUnknown;

impl fmt::Display for SecretStorageOwnershipUnknown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "YubiKey secret storage ownership cannot be established; manual administrator escalation is required",
        )
    }
}

impl Error for SecretStorageOwnershipUnknown {}

/// ownership 不明の固定 failure が error chain に含まれるかを判定する。
pub fn is_secret_storage_ownership_unknown(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<SecretStorageOwnershipUnknown>())
}

/// 観測済み予約 storage 不整合だけを表す typed error かを判定する。
///
/// この判定は `status` inspection を最後まで取得して domain rule が不整合と確定した場合だけに
/// 使う。device discovery、PC/SC / USB transport、SDK error、slot preflight の失敗を clear の
/// 根拠へ変換してはならない。provisioning と明示 `clear --yes` は、この型だけを destructive
/// transition の許可根拠にする。
pub fn is_observed_storage_invalid(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<SecretStorageStatusInvalid>())
}

/// `put` が、予約済み領域が完全に未初期化であることを観測したことを表す domain error。
///
/// これは低水準 `put` の互換的な public exit-code 用の型であり、provisioning script は
/// `setup` の許可根拠に使わない。script は fresh primary を `enroll-primary`、fresh spare を
/// `enroll-spare` の高水準 command に委譲する。manifest 欠落に予約済み object / key /
/// certificate のいずれかが残る不正状態や、
/// I/O 失敗はこの型へ写像しない。
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

    /// PIN-free status が空に見える場合の認証済み inspection から ownership 不明を拒否する。
    ///
    /// status は slot を観測しないため、この判定を status invalid の根拠には使わない。clear caller は
    /// key/certificate-only state で本 error を返した後、slot/object を変更してはならない。
    pub fn reject_unknown_ownership(inspection: &SecretStorageSetupInspection) -> Result<()> {
        reject_key_only_unknown_ownership(inspection)
    }
}

impl SecretStorageSetupIntent {
    /// PIN 変更前の setup 専用管理 preflight から初期化 intent を作る。
    ///
    /// PIV PIN は application-wide であるため、PIN-free `status` の object 観測を変更許可の
    /// 根拠にしない。current PIN VERIFY と既存 protected management-key authentication を済ませた
    /// 同一 handle で、PIV version、slot 82 key/certificate、manifest、全予約 object を観測し、
    /// 完全に空の場合だけ PIN 変更を許可する。観測 error と partial state は opaque failure のまま
    /// 停止し、PIN 変更後にあらためて gate を評価しない。
    pub fn for_pin_change(inspection: SecretStorageSetupInspection) -> Result<Self> {
        reject_key_only_unknown_ownership(&inspection)?;
        (|| {
            validate_secret_storage_setup_preconditions(inspection.piv_version)?;
            if reserved_slot_material_exists(&inspection)
                || inspection.manifest_bytes.is_some()
                || !inspection.present_object_ids.is_empty()
            {
                anyhow::bail!("setup storage preflight rejected");
            }
            Ok(Self {
                key_generation_required: true,
                manifest_update_required: true,
                piv_pin_change_required: true,
                enrollment_state: Some(SecretStorageEnrollmentState::Fresh),
            })
        })()
        .map_err(|error| error.context("YubiKey PIV setup failed"))
    }

    /// enrollment 用に、fresh storage の初期化または initialized storage の既存 key 利用を決める。
    ///
    /// `clear` 後の空 v2 manifest は initialized storage である。enroll はその既存 slot 82 key を
    /// 使って token を保存できるが、実際の object 空状態と SPKI 一致は secret 取得前の write
    /// inspection で判定する。version 1 と manifestless partial state は setup/enrollment の
    /// migration/resume 対象にせず停止する。
    pub fn for_enrollment(inspection: SecretStorageSetupInspection) -> Result<Self> {
        reject_key_only_unknown_ownership(&inspection)?;
        validate_secret_storage_setup_preconditions(inspection.piv_version)?;
        if let Some(manifest_bytes) = inspection.manifest_bytes.as_deref() {
            let manifest = SecretManifest::decode_initialized(Some(manifest_bytes))?;
            if manifest.version != 2 {
                anyhow::bail!("refusing to enroll into version 1 YubiKey secret storage");
            }
            if !inspection.reserved_slot_key_exists {
                anyhow::bail!("refusing to enroll into YubiKey storage without slot 82 key");
            }
            if inspection.slot_public_key_spki.as_deref() != manifest.slot_public_key_spki() {
                anyhow::bail!(
                    "refusing to enroll into YubiKey storage with mismatched slot 82 key"
                );
            }
            return Ok(Self {
                key_generation_required: false,
                manifest_update_required: false,
                piv_pin_change_required: false,
                enrollment_state: Some(SecretStorageEnrollmentState::InitializedV2),
            });
        }
        if reserved_slot_material_exists(&inspection) || !inspection.present_object_ids.is_empty() {
            anyhow::bail!("refusing to enroll into partial YubiKey secret storage");
        }
        let piv_pin_change_required = !reserved_slot_material_exists(&inspection)
            && inspection.manifest_bytes.is_none()
            && inspection.present_object_ids.is_empty();
        Ok(Self {
            key_generation_required: !inspection.reserved_slot_key_exists,
            manifest_update_required: true,
            piv_pin_change_required,
            enrollment_state: Some(SecretStorageEnrollmentState::Fresh),
        })
    }

    /// physical fresh storage の初期化に実 SPKI が必要かを返す。
    pub fn requires_public_key_spki(&self) -> bool {
        self.manifest_update_required
    }

    /// v2 manifest を確定すべきかを返す。
    pub fn requires_finalization(&self) -> bool {
        self.manifest_update_required
    }

    /// enrollment が完全に fresh な storage を初期化する前に PIN 変更 lifecycle を要するか返す。
    ///
    /// `clear` 後の空 v2 manifest は initialized storage であり、ここでは `false` になる。
    /// その場合 enrollment は設定済み PIN による management session だけを使い、`setup` を
    /// 再実行しない。
    pub fn requires_piv_pin_change(&self) -> bool {
        self.piv_pin_change_required
    }

    /// enrollment が secret 取得前の initialized-v2 write preflight を必要とするか返す。
    pub fn requires_initialized_write_preflight(&self) -> bool {
        self.enrollment_state == Some(SecretStorageEnrollmentState::InitializedV2)
    }

    /// source provisioning が既存 InitializedV2 だけを使用できることを確定する。
    ///
    /// `provision-bws-token` は setup/enrollment の代替ではない。Fresh、version 1、
    /// manifestless/zero-length partial、slot material 不在は token input 前に拒否する。
    pub fn for_initialized_provisioning(inspection: SecretStorageSetupInspection) -> Result<Self> {
        let intent = Self::for_enrollment(inspection)?;
        if !intent.requires_initialized_write_preflight() {
            anyhow::bail!(
                "provision-bws-token requires initialized version 2 YubiKey secret storage"
            );
        }
        Ok(intent)
    }

    /// 実際に観測または生成した SPKI だけから v2 manifest bytes を作る。
    pub fn manifest_for_public_key(&self, public_key_spki: Vec<u8>) -> Result<Vec<u8>> {
        SecretManifest::v2(public_key_spki)?.encode()
    }
}

/// manifest/object が空で予約 key/certificate だけが残る状態を ownership 不明として固定する。
fn reject_key_only_unknown_ownership(inspection: &SecretStorageSetupInspection) -> Result<()> {
    if reserved_slot_material_exists(inspection)
        && inspection.manifest_bytes.is_none()
        && inspection.nonempty_object_ids.is_empty()
    {
        return Err(anyhow::Error::new(SecretStorageOwnershipUnknown));
    }
    Ok(())
}

fn reserved_slot_material_exists(inspection: &SecretStorageSetupInspection) -> bool {
    inspection.reserved_slot_key_exists || inspection.reserved_slot_certificate_exists
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

    /// initialized version 2 enrollment の secret 取得前 preflight を構築する。
    ///
    /// manifest/SPKI/slot の完全照合に加え、対象 bootstrap object が logical empty である場合だけ
    /// enrollment を許可する。既存 non-empty object は rotate/`put --force` 以外で上書きしない。
    pub fn preflight_initial_enrollment(
        storage: SecretStorageSpec,
        inspection: &SecretStorageWriteInspection,
    ) -> Result<Self> {
        if inspection.object_exists {
            anyhow::bail!(
                "refusing to overwrite existing YubiKey bootstrap secret during enrollment"
            );
        }
        Self::from_initialized_manifest(storage, inspection)
    }

    /// secret 取得後に初期 enrollment の値制約だけを適用する。
    pub fn with_initial_enrollment_secret_len(self, secret_len: usize) -> Result<Self> {
        self.storage.ensure_plaintext_len(secret_len)?;
        Ok(self)
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
            anyhow::anyhow!("YubiKey secret storage manifest version 1 is unsupported")
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
            && !inspection.manifest_present
            && !inspection.object_present
            && !inspection.reserved_slot_key_exists
            && !inspection.reserved_slot_certificate_exists
        {
            return Err(anyhow::Error::new(SecretStorageUninitialized));
        }
        if inspection.manifest_bytes.is_none()
            && !inspection.object_exists
            && (inspection.reserved_slot_key_exists || inspection.reserved_slot_certificate_exists)
        {
            return Err(anyhow::Error::new(SecretStorageOwnershipUnknown));
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
    /// 高水準 enroll が完全な physical fresh state の初期化直後にだけ使う初期書き込み intent。
    ///
    /// caller は認証済み完全 inspection で fresh を確定してから key を生成しており、この constructor
    /// を initialized/v1/manifestless partial state の上書きや resume に流用してはならない。
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
        let mut manifest_present = false;
        let mut reserved_object_exists = false;
        for (storage, inspection) in inspections {
            manifest_present |= inspection.manifest_present;
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
            if stored.is_empty() && !reserved_object_exists && !manifest_present {
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
    use super::super::piv::SecretName;
    use super::*;

    fn secret_with_len(len: usize) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(&vec![0; len])
    }

    fn fixture_spki() -> Result<Vec<u8>> {
        SecretManifest::fixture_v2()
            .slot_public_key_spki
            .ok_or_else(|| anyhow::anyhow!("fixture v2 manifest must contain SPKI"))
    }

    fn expected_manifest_bytes() -> Result<Vec<u8>> {
        SecretManifest::fixture_v2().encode()
    }

    fn minimum_piv_version() -> PivApplicationVersion {
        PivApplicationVersion::minimum_for_secret_storage()
    }

    fn clean_setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            reserved_slot_key_exists: false,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: None,
            piv_version: minimum_piv_version(),
            manifest_bytes: None,
            present_object_ids: Vec::new(),
            nonempty_object_ids: Vec::new(),
        }
    }

    fn write_inspection(manifest_bytes: Option<Vec<u8>>) -> Result<SecretStorageWriteInspection> {
        let manifest_present = manifest_bytes.is_some();
        Ok(SecretStorageWriteInspection {
            manifest_present,
            manifest_bytes,
            object_present: false,
            object_exists: false,
            reserved_slot_key_exists: false,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(fixture_spki()?),
        })
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
    fn setup_pin_change_requires_key_generation_for_fresh_storage() -> Result<()> {
        let intent = SecretStorageSetupIntent::for_pin_change(clean_setup_inspection())?;

        assert!(intent.key_generation_required);
        assert!(intent.requires_public_key_spki());
        assert_eq!(
            SecretManifest::decode(&intent.manifest_for_public_key(fixture_spki()?,)?)?,
            SecretManifest::fixture_v2()
        );
        Ok(())
    }

    #[test]
    fn setup_pin_change_rejects_initialized_and_physically_present_storage() -> Result<()> {
        let initialized = SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            manifest_bytes: Some(expected_manifest_bytes()?),
            ..clean_setup_inspection()
        };
        assert!(SecretStorageSetupIntent::for_pin_change(initialized).is_err());

        let occupied = SecretStorageSetupInspection {
            present_object_ids: vec![PivObjectId::MANIFEST],
            ..clean_setup_inspection()
        };
        assert!(SecretStorageSetupIntent::for_pin_change(occupied).is_err());
        Ok(())
    }

    #[test]
    fn enrollment_intent_reuses_initialized_storage_and_rejects_manifestless_partial_state()
    -> Result<()> {
        let existing_manifest = SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            manifest_bytes: Some(expected_manifest_bytes()?),
            ..clean_setup_inspection()
        };
        let initialized = SecretStorageSetupIntent::for_enrollment(existing_manifest)?;
        assert!(!initialized.key_generation_required);
        assert!(!initialized.requires_finalization());
        assert!(!initialized.requires_piv_pin_change());

        let partial = SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            present_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
            nonempty_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
            ..clean_setup_inspection()
        };
        let error = error_message(SecretStorageSetupIntent::for_enrollment(partial))?;
        assert!(error.contains("partial"));

        let v1 = SecretManifest {
            version: 1,
            app: super::super::manifest::MANIFEST_APP.to_owned(),
            slot_public_key_spki: None,
        }
        .encode()?;
        let v1_error = error_message(SecretStorageSetupIntent::for_enrollment(
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                manifest_bytes: Some(v1),
                present_object_ids: vec![PivObjectId::MANIFEST],
                nonempty_object_ids: vec![PivObjectId::MANIFEST],
                ..clean_setup_inspection()
            },
        ))?;
        assert!(v1_error.contains("version 1"));
        Ok(())
    }

    #[test]
    fn fresh_enrollment_requires_the_setup_pin_change_lifecycle() -> Result<()> {
        let intent = SecretStorageSetupIntent::for_enrollment(clean_setup_inspection())?;

        assert!(intent.key_generation_required);
        assert!(intent.requires_finalization());
        assert!(intent.requires_piv_pin_change());
        Ok(())
    }

    #[test]
    fn key_only_partial_state_requires_manual_escalation_for_every_management_intent() -> Result<()>
    {
        let inspection = || SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            ..clean_setup_inspection()
        };

        for result in [
            SecretStorageSetupIntent::for_pin_change(inspection()).map(|_| ()),
            SecretStorageSetupIntent::for_enrollment(inspection()).map(|_| ()),
            SecretStorageSetupIntent::for_initialized_provisioning(inspection()).map(|_| ()),
            SecretStorageClearIntent::reject_unknown_ownership(&inspection()),
        ] {
            let error = result.expect_err("key-only state must stop");
            assert!(is_secret_storage_ownership_unknown(&error));
            assert!(
                error
                    .to_string()
                    .contains("manual administrator escalation")
            );
        }
        Ok(())
    }

    #[test]
    fn certificate_only_partial_state_requires_manual_escalation_for_every_management_intent()
    -> Result<()> {
        let inspection = || SecretStorageSetupInspection {
            reserved_slot_certificate_exists: true,
            ..clean_setup_inspection()
        };

        for result in [
            SecretStorageSetupIntent::for_pin_change(inspection()).map(|_| ()),
            SecretStorageSetupIntent::for_enrollment(inspection()).map(|_| ()),
            SecretStorageSetupIntent::for_initialized_provisioning(inspection()).map(|_| ()),
            SecretStorageClearIntent::reject_unknown_ownership(&inspection()),
        ] {
            let error = result.expect_err("certificate-only state must stop");
            assert!(is_secret_storage_ownership_unknown(&error));
        }
        Ok(())
    }

    #[test]
    fn enrollment_rejects_manifest_and_observed_slot_spki_mismatch() -> Result<()> {
        let mut mismatched_spki = fixture_spki()?;
        let last = mismatched_spki
            .last_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture SPKI must not be empty"))?;
        *last ^= 1;
        let error = error_message(SecretStorageSetupIntent::for_enrollment(
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                slot_public_key_spki: Some(mismatched_spki),
                manifest_bytes: Some(expected_manifest_bytes()?),
                ..clean_setup_inspection()
            },
        ))?;
        assert!(error.contains("mismatched slot 82 key"));
        Ok(())
    }

    #[test]
    fn version_one_manifest_is_never_migrated_by_setup_enrollment_or_provisioning() -> Result<()> {
        let v1 = SecretManifest {
            version: 1,
            app: super::super::manifest::MANIFEST_APP.to_owned(),
            slot_public_key_spki: None,
        }
        .encode()?;
        let inspection = || SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            manifest_bytes: Some(v1.clone()),
            present_object_ids: vec![PivObjectId::MANIFEST],
            nonempty_object_ids: vec![PivObjectId::MANIFEST],
            ..clean_setup_inspection()
        };
        assert!(SecretStorageSetupIntent::for_pin_change(inspection()).is_err());
        assert!(SecretStorageSetupIntent::for_enrollment(inspection()).is_err());
        assert!(SecretStorageSetupIntent::for_initialized_provisioning(inspection()).is_err());
        Ok(())
    }

    #[test]
    fn zero_length_manifest_is_partial_for_every_setup_flow() -> Result<()> {
        let inspection = || SecretStorageSetupInspection {
            present_object_ids: vec![PivObjectId::MANIFEST],
            nonempty_object_ids: Vec::new(),
            ..clean_setup_inspection()
        };
        assert!(SecretStorageSetupIntent::for_pin_change(inspection()).is_err());
        assert!(SecretStorageSetupIntent::for_enrollment(inspection()).is_err());
        assert!(SecretStorageSetupIntent::for_initialized_provisioning(inspection()).is_err());
        Ok(())
    }

    #[test]
    fn status_distinguishes_absent_zero_length_and_nonempty_objects() -> Result<()> {
        let status = SecretStorageStatus::from_inspections([(
            storage(),
            SecretStorageStatusInspection {
                manifest_present: false,
                manifest_bytes: None,
                object_present: false,
                object_exists: false,
            },
        )])?;
        assert!(status.stored().is_empty());

        for invalid in [
            SecretStorageStatusInspection {
                manifest_present: true,
                manifest_bytes: None,
                object_present: false,
                object_exists: false,
            },
            SecretStorageStatusInspection {
                manifest_present: false,
                manifest_bytes: None,
                object_present: true,
                object_exists: false,
            },
        ] {
            assert!(SecretStorageStatus::from_inspections([(storage(), invalid)]).is_err());
        }

        let logical_empty = SecretStorageStatus::from_inspections([(
            storage(),
            SecretStorageStatusInspection {
                manifest_present: true,
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_present: true,
                object_exists: false,
            },
        )])?;
        assert!(logical_empty.stored().is_empty());

        let stored = SecretStorageStatus::from_inspections([(
            storage(),
            SecretStorageStatusInspection {
                manifest_present: true,
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_present: true,
                object_exists: true,
            },
        )])?;
        assert_eq!(stored.stored(), &[SecretName::BitwardenClientSecret]);
        Ok(())
    }

    #[test]
    fn store_intent_requires_initialized_manifest_and_non_empty_secret() -> Result<()> {
        let storage = storage();
        let intent = SecretStorageWriteIntent::store(
            storage.clone(),
            write_inspection(Some(expected_manifest_bytes()?))?,
            1,
        )?;
        assert_eq!(intent.storage, storage);

        let missing_manifest_error = error_message(SecretStorageWriteIntent::store(
            storage.clone(),
            write_inspection(None)?,
            1,
        ))?;
        assert!(missing_manifest_error.contains("manifest is missing"));

        let empty_secret_error = error_message(SecretStorageWriteIntent::store(
            storage,
            write_inspection(Some(expected_manifest_bytes()?))?,
            0,
        ))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }

    #[test]
    fn fresh_initial_enroll_store_keeps_secret_length_rule() -> Result<()> {
        let storage = storage();
        let intent =
            SecretStorageWriteIntent::initial_enroll_store(storage.clone(), 1, fixture_spki()?)?;
        assert_eq!(intent.storage, storage);

        let empty_secret_error = error_message(SecretStorageWriteIntent::initial_enroll_store(
            storage,
            0,
            fixture_spki()?,
        ))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }

    #[test]
    fn initialized_enrollment_preflight_requires_empty_object_and_matching_v2_spki() -> Result<()> {
        let storage = storage();
        let empty = write_inspection(Some(expected_manifest_bytes()?))?;
        let intent =
            SecretStorageWriteIntent::preflight_initial_enrollment(storage.clone(), &empty)?;
        assert_eq!(intent.storage, storage);

        let nonempty = SecretStorageWriteInspection {
            object_present: true,
            object_exists: true,
            ..write_inspection(Some(expected_manifest_bytes()?))?
        };
        let error = error_message(SecretStorageWriteIntent::preflight_initial_enrollment(
            storage, &nonempty,
        ))?;
        assert!(error.contains("refusing to overwrite"));
        Ok(())
    }

    #[test]
    fn put_intent_applies_overwrite_policy_before_accepting_existing_object() -> Result<()> {
        let storage = storage();
        let existing_object = SecretStorageWriteInspection {
            manifest_present: true,
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_present: true,
            object_exists: true,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(fixture_spki()?),
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
                manifest_present: true,
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_present: true,
                object_exists: true,
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(fixture_spki()?),
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
            &write_inspection(None)?,
            false,
        )
        .expect_err("completely empty storage must require setup");
        assert!(
            error
                .chain()
                .any(|cause| cause.is::<SecretStorageUninitialized>())
        );

        let mut manifestless_key = write_inspection(None)?;
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
            manifest_present: true,
            manifest_bytes: Some(expected_manifest_bytes()?),
            object_present: true,
            object_exists: true,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(fixture_spki()?),
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
                manifest_present: true,
                manifest_bytes: Some(expected_manifest_bytes()?),
                object_present: true,
                object_exists: true,
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(fixture_spki()?),
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
        assert!(
            decode_error
                .chain()
                .any(|source| source.to_string() == "ciphertext rejected")
        );

        intent.validate_loaded_secret(&secret_with_len(1)?)?;
        let empty_secret = secret_with_len(0)?;
        let empty_secret_error = error_message(intent.validate_loaded_secret(&empty_secret))?;
        assert!(empty_secret_error.contains("must not be empty"));
        Ok(())
    }
}
