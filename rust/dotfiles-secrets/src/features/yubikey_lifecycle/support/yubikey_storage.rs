//! YubiKey PIV storage の technical receiver と I/O sequence。
//!
//! ## 出典と適用判断
//!
//! repository 正本は [`secret-recovery-spec.md`](../../../docs/secret-recovery/secret-recovery-spec.md)
//! の「無対話復旧の利用者契約」と
//! [`yubikey-secret-storage-design.md`](../../../docs/secret-recovery/yubikey-secret-storage-design.md)
//! の「PIV 領域」「保存形式」である。ここは application/domain が決定した intent を
//! `yubikey_backend` の technical I/O へ渡すだけで、保存する secret 名、manifest の意味、
//! setup 可否、復旧の順序・停止条件を決定しない。
//!
//! PIV の利用フローは [YubiKey Technical Manual: Smart Card (PIV Compatible)](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/yk5-apps-piv.html)
//! の PIN/touch policy、retired key-management slots、PIV metadata を直接確認する。
//! この module が経由する version 固定 `yubikey` 0.9.0-pre.0 の操作は
//! [`YubiKey::verify_pin` / `YubiKey::authenticate`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs)、
//! [`MgmKey::get_protected`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs)、
//! [`piv::generate` / `piv::metadata`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs)、
//! [`Transaction::fetch_object` / `Transaction::save_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
//! である。PIN-protected management key の session flow は
//! [Yubico PIV PIN-only mode](https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected)
//! と [ykman の固定 source](https://github.com/Yubico/yubikey-manager/blob/4ca60f706af930459138d8dc0f0f953480e1c7a4/ykman/_cli/piv.py#L1461-L1512)
//! を直接照合する。ykman は同一 `PivSession` を保持するが、VERIFY を最後の APDU に戻すための
//! second VERIFY は repository 正本 [`secret-handling.md`](../../../docs/secret-recovery/secret-handling.md#tty-secret-input)
//! の one input / one physical VERIFY 契約に反するため使わない。slot 82 の `PinPolicy::Never` /
//! `TouchPolicy::Always` の意味は [Yubico PIN/touch policies](https://docs.yubico.com/yesdk/users-manual/application-piv/pin-touch-policies.html)
//! に従い、PIN-free recovery private operation と per-operation touch を分離する。`get_protected` は metadata query と protected-data read の `NotFound` origin を
//! public API で区別できないため、その error を B0 bootstrap に使わず停止する。`fetch_object`
//! 由来の `NotFound` 以外の SDK error は、この receiver で成功、
//! 空値、再試行、別 state へ写像せず backend から伝播する。repository test/agent 作業は
//! physical device を使わず feature 隔離 stub だけを使う。

use crate::{
    Result,
    features::yubikey_lifecycle::domain::{
        piv::{PivObjectId, SecretStorageSpec},
        storage::{
            SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
            SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageStatusInspection, SecretStorageWriteInspection, SecretStorageWriteIntent,
        },
    },
    features::yubikey_lifecycle::support::{
        piv_storage::non_empty_payload,
        process_diagnostic,
        yubikey_backend::{self, SecretDeviceIo, SelectedSecretDevice, YubikeyDeviceBackend},
    },
    foundation::protection::{ProtectedSecret, SecretSession},
};
#[derive(Default)]
pub(crate) struct YubikeyStorageBackend {
    piv_management_session: Option<PivManagementSession>,
}

/// 管理 command の一つの対象 serial だけに束縛する PIV 接続と認証状態。
///
/// repository の管理 command は hidden TTY から得た PIN 一入力につき physical `VERIFY` を
/// 一回だけ送る。そのため管理 I/O の inspection、生成、保存、finalize、ローカル確認は同じ
/// `SelectedSecretDevice` を借用する。最初に選ばれた serial だけを command の対象とし、
/// 別 serial は PIN を再利用して reopen / VERIFY せず fail-closed にする。
/// session 開始時の open、VERIFY、protected management key 取得、authenticate の失敗では
/// session を保持せず後続操作を停止する。retry、fallback、PUK、reset は実行しない。
struct PivManagementSession {
    serial: u32,
    device: SelectedSecretDevice,
    state: PivManagementSessionState,
}

/// PIV handle 内で許可する technical な APDU 順序。
///
/// これは use case の成功条件を決める state machine ではない。open 済み handle に対して
/// VERIFY と protected management-key authentication を取り違えたり、認証前に storage
/// operation を通したりしないための backend 側 safety guard である。
#[derive(Clone, Copy, PartialEq, Eq)]
enum PivManagementSessionState {
    Opened,
    Verified,
    Authenticated,
}

fn open_device(serial: u32) -> Result<SelectedSecretDevice> {
    yubikey_backend::open_device_by_serial(&mut YubikeyDeviceBackend::default(), serial)
}
fn with_authenticated_device<T>(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    operation: impl FnOnce(&mut SelectedSecretDevice) -> Result<T>,
) -> Result<T> {
    let session = backend.piv_management_session.as_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "PIV management operation requires a configured PIN-protected management session"
        )
    })?;
    if session.serial != serial {
        anyhow::bail!(
            "PIV management session is bound to YubiKey serial {}; refusing operation for serial {serial}",
            session.serial
        )
    }
    if session.state != PivManagementSessionState::Authenticated {
        anyhow::bail!(
            "PIV management storage operation requires protected management-key authentication"
        )
    }
    operation(&mut session.device)
}

fn with_read_device<T>(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    operation: impl FnOnce(&mut SelectedSecretDevice) -> Result<T>,
) -> Result<T> {
    if backend
        .piv_management_session
        .as_ref()
        .is_some_and(|session| session.serial == serial)
    {
        return with_authenticated_device(backend, serial, operation);
    }
    // PIN を使わない read/recovery I/O は管理操作ではない。`enroll-spare` が spare の認証済み
    // handle を保持中に primary の bootstrap document を読む場合も、primary 用の一時 handle を
    // 別に開き、spare の PIN や management-key 認証をその read に再利用しない。
    let mut device = open_device(serial)?;
    operation(&mut device)
}
pub(crate) fn begin_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    pin: ProtectedSecret,
) -> Result<()> {
    // 最初に選んだ serial の handle だけを保持し、同じ handle に PIN VERIFY、既存 protected
    // management key の取得・認証をこの順で適用する。open、VERIFY、認証のいずれかが失敗した
    // 場合は後続の操作を停止し、別 handle、retry、fallback を開始しない。
    if backend.piv_management_session.is_none() {
        open_piv_management_session(backend, serial)?;
    }
    verify_piv_management_pin(backend, serial, &pin)?;
    authenticate_piv_management_key(backend, serial)
}

/// PIN 変更を伴う `setup` と fresh enrollment の読み取り専用 preflight 用に、同じ handle を
/// current PIN で認証する。
///
/// PIN-free status は slot 82 metadata/certificate と management-key availability を証明しないため、
/// application-wide PIN の変更許可には使わない。ここで current PIN VERIFY と既存 protected
/// management-key authentication を完了してから、同じ handle の完全 storage inspection を許可する。
pub(crate) fn begin_piv_pin_setup_preflight(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    current_pin: &ProtectedSecret,
) -> Result<()> {
    (|| {
        open_piv_management_session(backend, serial)?;
        verify_piv_management_pin(backend, serial, current_pin)?;
        authenticate_piv_management_key(backend, serial)
    })()
    .map_err(opaque_piv_operation_error)
}

/// preflight で保持した同じ handle に PIN 変更 APDU を一度だけ適用する。
pub(crate) fn change_piv_pin(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    current_pin: &ProtectedSecret,
    new_pin: &ProtectedSecret,
) -> Result<()> {
    (|| {
        let session = backend.piv_management_session.as_mut().ok_or_else(|| {
            anyhow::anyhow!("PIV PIN change requires an initialized PIN-change preflight session")
        })?;
        if session.serial != serial {
            anyhow::bail!(
                "PIV management session is bound to YubiKey serial {}; refusing operation for serial {serial}",
                session.serial
            )
        }
        if session.state != PivManagementSessionState::Authenticated {
            anyhow::bail!("PIV PIN change requires an authenticated PIN-change preflight session")
        }
        session.device.change_management_pin(current_pin, new_pin)?;
        // PIN 変更の成功後は current PIN の認証状態を使えない。同じ handle を保持したまま、new PIN の
        // VERIFY を一回だけ行い、既存 protected management key を再取得・認証するまで後続操作を許可しない。
        session.state = PivManagementSessionState::Opened;
        Ok(())
    })()
    .map_err(opaque_piv_operation_error)
}

/// 選択済み serial の PIV handle を開き、未認証 session として保持する。
///
/// `begin_piv_management_session` と PIN-change preflight だけが、この handle に VERIFY と既存
/// protected management-key authentication を同じ順で適用する。application はその高水準 capability
/// 以外の PIV protocol step を要求できない。失敗時に retry、fallback、別 serial への reopen はしない。
fn open_piv_management_session(backend: &mut YubikeyStorageBackend, serial: u32) -> Result<()> {
    if backend.piv_management_session.is_some() {
        anyhow::bail!("PIV management session is already configured")
    }
    process_diagnostic::started(process_diagnostic::Operation::SessionOpen);
    let device = open_device(serial);
    process_diagnostic::returned(process_diagnostic::Operation::SessionOpen, &device);
    let device = device?;
    backend.piv_management_session = Some(PivManagementSession {
        serial,
        device,
        state: PivManagementSessionState::Opened,
    });
    Ok(())
}

/// 開始済み session の handle へ PIN VERIFY を一回だけ適用する。
fn verify_piv_management_pin(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    pin: &ProtectedSecret,
) -> Result<()> {
    let session = backend.piv_management_session.as_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "PIV management operation requires a configured PIN-protected management session"
        )
    })?;
    if session.serial != serial {
        anyhow::bail!(
            "PIV management session is bound to YubiKey serial {}; refusing operation for serial {serial}",
            session.serial
        )
    }
    if session.state != PivManagementSessionState::Opened {
        anyhow::bail!("PIV management PIN VERIFY requires a newly opened session")
    }
    process_diagnostic::started(process_diagnostic::Operation::VerifyInvocation);
    let result = session.device.verify_management_pin(pin);
    process_diagnostic::returned(process_diagnostic::Operation::VerifyInvocation, &result);
    result.map_err(opaque_piv_operation_error)?;
    session.state = PivManagementSessionState::Verified;
    Ok(())
}

/// VERIFY 済み同一 handle で protected management key を取得・認証する。
fn authenticate_piv_management_key(backend: &mut YubikeyStorageBackend, serial: u32) -> Result<()> {
    let session = backend.piv_management_session.as_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "PIV management operation requires a configured PIN-protected management session"
        )
    })?;
    if session.serial != serial {
        anyhow::bail!(
            "PIV management session is bound to YubiKey serial {}; refusing operation for serial {serial}",
            session.serial
        )
    }
    if session.state != PivManagementSessionState::Verified {
        anyhow::bail!("PIV management-key authentication requires a successful PIN VERIFY")
    }
    process_diagnostic::started(process_diagnostic::Operation::ManagementKeyAuthentication);
    let result = session.device.authenticate_protected_management_key();
    process_diagnostic::returned(
        process_diagnostic::Operation::ManagementKeyAuthentication,
        &result,
    );
    result.map_err(opaque_piv_operation_error)?;
    session.state = PivManagementSessionState::Authenticated;
    Ok(())
}

/// PIV SDK/backend failure を PIN、card status、transport detail なしの固定 error にする。
///
/// domain の inspection failure ではなく、VERIFY・protected management-key authentication・PIN change
/// の technical failure 境界にだけ適用する。retry、fallback、状態再分類は行わない。
fn opaque_piv_operation_error(error: anyhow::Error) -> anyhow::Error {
    error.context("YubiKey PIV operation failed")
}
/// 別 serial を更新する前に、利用者が新たに入力した PIN で前 session を置き換える。
///
/// 前 session の device handle と PIN は replacement 時に drop される。新 serial はこの関数内で
/// 直ちに open/VERIFY/authenticate され、first arbitrary operation まで bind を遅延しない。
pub(crate) fn begin_next_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    pin: ProtectedSecret,
) -> Result<()> {
    let previous_serial = backend
        .piv_management_session
        .as_ref()
        .map(|session| session.serial)
        .ok_or_else(|| anyhow::anyhow!("PIV management session has not been started"))?;
    if previous_serial == serial {
        anyhow::bail!(
            "next PIV management session requires a new YubiKey serial; current serial is {serial}"
        )
    }
    backend.piv_management_session = None;
    open_piv_management_session(backend, serial)?;
    verify_piv_management_pin(backend, serial, &pin)?;
    authenticate_piv_management_key(backend, serial)
}
pub(crate) fn inspect_secret_storage_setup(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    probe: &SecretStorageSetupProbe,
) -> Result<SecretStorageSetupInspection> {
    with_authenticated_device(backend, serial, |device| {
        let piv_version = device.piv_application_version();
        let mut manifest_bytes = None;
        let mut present_object_ids = Vec::new();
        let mut nonempty_object_ids = Vec::new();
        for object_id in probe.object_ids() {
            if let Some(payload) = device.read_object(*object_id)? {
                present_object_ids.push(*object_id);
                if !payload.is_empty() {
                    nonempty_object_ids.push(*object_id);
                    if *object_id == PivObjectId::MANIFEST {
                        manifest_bytes = Some(payload);
                    }
                }
            }
        }
        // key metadata、certificate object、SPKI は独立に観測し、support で一つの
        // 「material exists」へ畳み込まない。fresh/ownership/SPKI 一致は domain が決める。
        let reserved_slot_key_exists = device.key_exists()?;
        // slot key の有無だけでは retired slot の certificate を観測できない。certificate read の
        // error は application-wide PIN 変更前の完全 inspection failure として必ず伝播させる。
        let reserved_slot_certificate_exists = device.reserved_slot_certificate_exists()?;
        let slot_public_key_spki = device.slot_public_key_spki()?;
        Ok(SecretStorageSetupInspection {
            reserved_slot_key_exists,
            reserved_slot_certificate_exists,
            slot_public_key_spki,
            piv_version,
            manifest_bytes,
            present_object_ids,
            nonempty_object_ids,
        })
    })
}
pub(crate) fn initialize_secret_storage(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageSetupIntent,
) -> Result<Vec<u8>> {
    let generated_key = with_authenticated_device(backend, serial, |device| {
        intent
            .key_generation_required
            .then(|| device.generate_key())
            .transpose()
    })?;
    if let Some(key) = generated_key {
        return Ok(key);
    }
    with_authenticated_device(backend, serial, |device| {
        device
            .slot_public_key_spki()?
            .ok_or_else(|| anyhow::anyhow!("YubiKey slot 82 public key metadata is unavailable"))
    })
}
pub(crate) fn finalize_secret_storage_setup(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    mut manifest: Vec<u8>,
) -> Result<()> {
    with_authenticated_device(backend, serial, |device| {
        device.write_object(PivObjectId::MANIFEST, &mut manifest)
    })?;
    Ok(())
}
pub(crate) fn clear_secret_storage(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageClearIntent,
) -> Result<Vec<u8>> {
    with_authenticated_device(backend, serial, |device| {
        for object in &intent.object_ids {
            device.empty_object(*object)?
        }
        device.clear_reserved_slot_certificate()?;
        device.generate_key()
    })
}
pub(crate) fn inspect_secret_storage_write(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageWriteInspection> {
    with_authenticated_device(backend, serial, |device| {
        let manifest = device.read_object(PivObjectId::MANIFEST)?;
        let manifest_present = manifest.is_some();
        let manifest_bytes = non_empty_payload(manifest);
        let object = device.read_object(storage.object_id)?;
        let object_present = object.is_some();
        let object_exists = non_empty_payload(object).is_some();
        let slot_public_key_spki = manifest_bytes
            .as_ref()
            .map(|_| device.slot_public_key_spki())
            .transpose()?
            .flatten();
        let reserved_slot_key_exists = if slot_public_key_spki.is_some() {
            true
        } else {
            device.key_exists()?
        };
        Ok(SecretStorageWriteInspection {
            manifest_present,
            manifest_bytes,
            object_present,
            object_exists,
            reserved_slot_key_exists,
            reserved_slot_certificate_exists: device.reserved_slot_certificate_exists()?,
            slot_public_key_spki,
        })
    })
}
/// PIN を要求しない status 用の object 観測を返す。
///
/// 出典: repository の PIN-free status 契約は
/// [`secret-recovery-spec.md` の `verify-yubikey`](../../../docs/secret-recovery/secret-recovery-spec.md)
/// と [`yubikey-secret-storage-design.md` の「PIV 領域」](../../../docs/secret-recovery/yubikey-secret-storage-design.md#piv-領域)。
/// SDK の `Transaction::fetch_object` は
/// [`yubikey` 0.9.0-pre.0 upstream source](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
/// で `StatusWords::NotFoundError` だけを `Error::NotFound` に翻訳する。適用判断として、
/// `read_object` の `None` はその absence だけを表し、成功した empty payload は
/// `object_present=true` / `object_exists=false` と区別する。その他の error は status の
/// 正常状態に捏造せず伝播する。
pub(crate) fn inspect_secret_storage_status(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageStatusInspection> {
    // 通常の `status` は management session を開始せず PIN-free のままにする。provisioning は選択済み
    // serial の session を開始・認証済みなので、その同じ device handle を保持して利用する。
    let inspect = |device: &mut SelectedSecretDevice| -> Result<SecretStorageStatusInspection> {
        let manifest = device.read_object(PivObjectId::MANIFEST)?;
        let manifest_present = manifest.is_some();
        let manifest_bytes = non_empty_payload(manifest);
        let object = device.read_object(storage.object_id)?;
        Ok(SecretStorageStatusInspection {
            manifest_present,
            manifest_bytes,
            object_present: object.is_some(),
            object_exists: non_empty_payload(object).is_some(),
        })
    };
    with_read_device(backend, serial, inspect)
}
pub(crate) fn store_secret(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageWriteIntent,
    secret: &ProtectedSecret,
) -> Result<()> {
    with_authenticated_device(backend, serial, |device| {
        if let Some(key) = intent.slot_public_key_spki {
            device.remember_generated_public_key(key)
        }
        let mut encoded = device.seal_for_storage(intent.storage.clone(), secret)?;
        device.write_object(intent.storage.object_id, &mut encoded)
    })
}
pub(crate) fn inspect_secret_storage_read(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageReadInspection> {
    let _session = SecretSession::start()?;
    with_read_device(backend, serial, |device| {
        Ok(SecretStorageReadInspection {
            manifest_bytes: device.read_object(PivObjectId::MANIFEST)?,
            encoded: device.read_object(storage.object_id)?,
        })
    })
}
pub(crate) fn load_secret(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: &SecretStorageReadIntent,
) -> Result<ProtectedSecret> {
    let _session = SecretSession::start()?;
    with_read_device(backend, serial, |device| {
        device.open_from_storage(intent.storage.clone(), &intent.encoded)
    })
}
