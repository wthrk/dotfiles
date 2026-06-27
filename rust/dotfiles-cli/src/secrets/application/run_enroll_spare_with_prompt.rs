//! enroll-spare(prompt) の順序を固定し、spare 登録でも shell/stdin payload 経由を使わせない。

use crate::Result;
use crate::secrets::{
    domain::{
        commands::EnrollSpareCommand,
        enrollment::EnrollSummary,
        manifest::BootstrapSecretDocument,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// prompt 入力で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary から secret を読み出す旧設計と stdin payload を使う旧境界を廃止し、spare でも CLI の
/// secret input port だけから値を受け取る。use case は単一接続確認と setup を secret 入力前に完了し、
/// setup 不能な YubiKey では bootstrap secret を読まずに停止する。
pub(crate) async fn run_enroll_spare_with_prompt<I, P, S, R>(
    command: EnrollSpareCommand,
    device: &mut impl ports::yubikey::YubiKeyDevicePort,
    secret_input: &I,
    pin_input: &P,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    I: ports::io::SecretInputPort,
    P: ports::io::PinInputPort,
    S: ports::yubikey::SecretStoragePort,
    R: ports::io::ReportPort,
{
    let _ = command;
    let serial = device.resolve_device_serial()?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    storage_port.initialize_secret_storage(serial, setup_intent.clone(), None)?;
    let bitwarden_client_id = secret_input.read_bitwarden_client_id_secret()?;
    let bitwarden_client_secret = secret_input.read_bitwarden_client_secret()?;
    let document = BootstrapSecretDocument::from_secret_materials(
        &bitwarden_client_id,
        &bitwarden_client_secret,
    )?;
    for (storage, value) in document.storage_entries(serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        storage_port.store_secret(serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(serial, setup_intent)?;
    let pin = if device.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent, pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::spare_completed())
}

/// enroll-spare(prompt) が primary 読み出しや stdin 経由へ戻らないことを port mock で検証する。
#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::EnrollSpareCommand, piv::PivApplicationVersion,
            storage::SecretStorageSetupInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_spare_with_prompt;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            pin_retries: 3,
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    /// spare 登録でも input port 由来の 2 bootstrap secret だけを保存する。
    #[tokio::test]
    async fn enroll_spare_prompt_stores_two_bootstrap_secrets() -> crate::Result<()> {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2002));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| Ok(material(b"password")));
        let pin_input = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_store_secret()
            .times(2)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(2)
            .returning(|_, _| {
                Ok(
                    crate::secrets::domain::storage::SecretStorageReadInspection {
                        manifest_bytes: Some(
                            crate::secrets::domain::manifest::SecretManifest::expected()
                                .encode()
                                .expect("manifest"),
                        ),
                        encoded: Some(vec![1]),
                    },
                )
            });
        storage
            .expect_load_secret()
            .times(2)
            .returning(|_, _, _| Ok(material(b"loaded")));
        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .returning(|_| Ok(()));

        run_enroll_spare_with_prompt(
            EnrollSpareCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await
    }
}
