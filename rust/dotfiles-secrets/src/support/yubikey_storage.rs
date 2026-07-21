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
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct YubikeyStorageBackend {
    generated_public_keys: BTreeMap<u32, Vec<u8>>,
    piv_management_session: Option<PivManagementSession>,
}

/// 管理 command の一つの対象 serial だけに束縛する PIV 接続と認証状態。
///
/// repository の管理 command は hidden TTY から得た PIN 一入力につき physical `VERIFY` を
/// 一回だけ送る。そのため管理 I/O の inspection、生成、保存、finalize、ローカル確認は同じ
/// `SelectedSecretDevice` を借用する。最初に選ばれた serial だけを command の対象とし、
/// 別 serial は PIN を再利用して reopen / VERIFY せず fail-closed にする。
struct PivManagementSession {
    pin: ProtectedSecret,
    serial: Option<u32>,
    device: Option<SelectedSecretDevice>,
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
    if let Some(selected_serial) = session.serial {
        if selected_serial != serial {
            anyhow::bail!(
                "PIV management session is bound to YubiKey serial {selected_serial}; refusing to reuse its PIN for serial {serial}"
            )
        }
    }
    if session.device.is_none() {
        let mut device = open_device(serial)?;
        // `verify_pin` → protected management-key read → authenticate は、この device の
        // command-local first management I/O で一度だけ行う。ykman の PIN-protected flow が
        // 同じ session を継続することは参照するが、同 tool の「VERIFY を最後の APDU にする」
        // ための second VERIFY は、repository 正本の one input/one physical VERIFY に反する
        // ため実装しない。
        device.check_management_auth_preconditions(Some(&session.pin))?;
        session.serial = Some(serial);
        session.device = Some(device);
    }
    let device = session
        .device
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("PIV management session device was not retained"))?;
    operation(device)
}

fn with_read_device<T>(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    operation: impl FnOnce(&mut SelectedSecretDevice) -> Result<T>,
) -> Result<T> {
    if backend
        .piv_management_session
        .as_ref()
        .is_some_and(|session| session.serial == Some(serial))
    {
        return with_authenticated_device(backend, serial, operation);
    }
    let mut device = open_device(serial)?;
    operation(&mut device)
}
pub(crate) fn begin_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    pin: ProtectedSecret,
) -> Result<()> {
    if backend.piv_management_session.is_some() {
        anyhow::bail!("PIV management session is already configured")
    }
    backend.piv_management_session = Some(PivManagementSession {
        pin,
        serial: None,
        device: None,
    });
    Ok(())
}
/// 別 serial を更新する前に、利用者が新たに入力した PIN で前 session を置き換える。
///
/// 前 session の `ProtectedSecret` と device handle は replacement 時に drop される。caller は
/// device serial を解決し、その serial が first operation で bind されるまでこの関数だけで
/// physical device operation を発生させない。
pub(crate) fn begin_next_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    pin: ProtectedSecret,
) -> Result<()> {
    if backend.piv_management_session.is_none() {
        anyhow::bail!("PIV management session has not been started")
    }
    backend.piv_management_session = Some(PivManagementSession {
        pin,
        serial: None,
        device: None,
    });
    Ok(())
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
        backend.generated_public_keys.insert(serial, key.clone());
        return Ok(key);
    }
    if let Some(key) = backend.generated_public_keys.get(&serial).cloned() {
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
    backend.generated_public_keys.remove(&serial);
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
    // Ordinary `status` has no management session and remains PIN-free. The provisioning use
    // case has already established one, so its initial observation must bind and retain that
    // same device instead of opening an unauthenticated throwaway connection before clear/setup.
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
