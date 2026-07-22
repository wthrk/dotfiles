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
    domain::{
        piv::{PivObjectId, SecretStorageSpec},
        storage::{
            SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
            SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageStatusInspection, SecretStorageWriteInspection, SecretStorageWriteIntent,
        },
    },
    support::{
        piv_storage::non_empty_payload,
        protection::{ProtectedSecret, SecretSession},
        yubikey_backend::{self, SecretDeviceIo, SelectedSecretDevice, YubikeyDeviceBackend},
    },
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
    yubikey_backend::open_device_by_serial(&mut YubikeyDeviceBackend, serial)
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
    let mut device = open_device(serial)?;
    operation(&mut device)
}
pub(crate) fn begin_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    pin: ProtectedSecret,
) -> Result<()> {
    open_piv_management_session(backend, serial)?;
    verify_piv_management_pin(backend, serial, pin)?;
    authenticate_piv_management_key(backend, serial)
}

/// 選択済み serial の PIV handle を開き、未認証 session として保持する。
///
/// `begin_piv_management_session` だけがこの handle に VERIFY と protected management-key
/// authentication を同じ順で適用する。application はその高水準 capability 以外の PIV protocol
/// step を要求できない。失敗時に retry、fallback、別 serial への reopen はしない。
fn open_piv_management_session(backend: &mut YubikeyStorageBackend, serial: u32) -> Result<()> {
    if backend.piv_management_session.is_some() {
        anyhow::bail!("PIV management session is already configured")
    }
    let device = open_device(serial)?;
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
    pin: ProtectedSecret,
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
    session.device.verify_management_pin(&pin)?;
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
    session.device.authenticate_protected_management_key()?;
    session.state = PivManagementSessionState::Authenticated;
    Ok(())
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
    verify_piv_management_pin(backend, serial, pin)?;
    authenticate_piv_management_key(backend, serial)
}
pub(crate) fn inspect_secret_storage_setup(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    probe: &SecretStorageSetupProbe,
) -> Result<SecretStorageSetupInspection> {
    with_authenticated_device(backend, serial, |device| {
        let piv_version = device.piv_application_version();
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let slot_public_key_spki = manifest_bytes
            .as_ref()
            .map(|_| device.slot_public_key_spki())
            .transpose()?
            .flatten();
        let key_exists = if slot_public_key_spki.is_some() {
            true
        } else {
            device.key_exists()? || device.reserved_slot_certificate_exists()?
        };
        let mut occupied_object_ids = Vec::new();
        for object_id in probe.object_ids() {
            if device.read_object(*object_id)?.is_some() {
                occupied_object_ids.push(*object_id)
            }
        }
        Ok(SecretStorageSetupInspection {
            key_exists,
            piv_version,
            manifest_bytes,
            occupied_object_ids,
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
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
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
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let object = device.read_object(storage.object_id)?;
        Ok(SecretStorageStatusInspection {
            manifest_bytes,
            object_present: object.is_some(),
            object_exists: non_empty_payload(object).is_some(),
        })
    };
    if backend.piv_management_session.is_some() {
        with_authenticated_device(backend, serial, inspect)
    } else {
        let mut device = open_device(serial)?;
        inspect(&mut device)
    }
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
