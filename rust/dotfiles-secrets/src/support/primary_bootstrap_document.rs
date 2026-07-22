//! primary YubiKey storage を spare enrollment document として読む technical backend。
//!
//! source の選択は entrypoint が行う。この backend は選択済み primary storage を一回読むための
//! device discovery と PIV read I/O だけを持ち、spare enrollment の setup、保存、finalize、報告の
//! 順序を決めない。

use crate::{
    Result,
    domain::{
        manifest::{BootstrapSecretDocumentInput, BootstrapSecretDocumentSourceInput},
        piv::SecretStorageSpec,
        storage::SecretStorageReadIntent,
    },
    support::{
        adapter_backend::PrimaryBootstrapDocumentSourceBackend,
        yubikey_backend::YubikeyDeviceBackend,
        yubikey_device_serial,
        yubikey_storage::{self, YubikeyStorageBackend},
    },
};

/// requested primary を解決して bootstrap document の protected input を読む。
///
/// primary storage の read は management session を必要としないため、spare の PIN-protected
/// session と receiver を共有しない。解決済み serial は application へ返し、同一 device を source と
/// destination に使うことの判定は command domain rule に委ねる。
pub(crate) fn read_bootstrap_secret_document(
    _: &mut PrimaryBootstrapDocumentSourceBackend,
    requested_primary_serial: Option<u32>,
) -> Result<BootstrapSecretDocumentSourceInput> {
    let primary_serial = yubikey_device_serial::resolve_device_serial(
        &mut YubikeyDeviceBackend,
        requested_primary_serial,
    )?;
    let [storage_spec] = SecretStorageSpec::all_for_serial(primary_serial);
    let mut storage = YubikeyStorageBackend::default();
    let inspection =
        yubikey_storage::inspect_secret_storage_read(&mut storage, primary_serial, &storage_spec)?;
    let intent = SecretStorageReadIntent::from_inspection(storage_spec, inspection)?;
    let secret = yubikey_storage::load_secret(&mut storage, primary_serial, &intent)
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    Ok(BootstrapSecretDocumentSourceInput {
        input: BootstrapSecretDocumentInput::BitwardenClientSecret(secret),
        resolved_primary_serial: Some(primary_serial),
    })
}
