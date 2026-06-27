//! enroll-primary(prompt) の順序を固定し、入力 I/O 変更を storage 手順から分離して誤登録を防ぐ。

use crate::Result;
use crate::secrets::{
    domain::{
        commands::EnrollPrimaryCommand,
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

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
///
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込める。use case は単一接続確認と setup を secret 入力前に
/// 完了し、setup 不能な YubiKey では bootstrap secret を読まずに停止する。
pub(crate) async fn run_enroll_primary_with_prompt<I, P, S, R>(
    command: EnrollPrimaryCommand,
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
    let pin_for_setup_and_load = if device.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    storage_port.initialize_secret_storage(
        serial,
        setup_intent.clone(),
        pin_for_setup_and_load.as_ref(),
    )?;
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
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent, pin_for_setup_and_load.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::primary_completed())
}

/// enroll-primary(prompt) の use case 順序を port trait mock だけで検証する。
#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::EnrollPrimaryCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
            verification::{CheckName, CheckStatus},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_primary_with_prompt;

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

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// 正常系では setup 後に 2 bootstrap secret だけを読み、保存・検証・report の順序で完了する。
    #[tokio::test]
    async fn enroll_primary_prompt_stores_verifies_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, pin| pin.is_none())
            .returning(|_, _, _| Ok(()));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"password")));
        storage
            .expect_store_secret()
            .times(2)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let pin_input = ports::io::MockPinInputPort::new();
        for name in [
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .withf(move |serial, intent, _| *serial == 2001 && intent.storage.name == name)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BitwardenClientId => material(b"email"),
                        SecretName::BitwardenClientSecret => material(b"password"),
                    })
                });
        }
        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok))
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await
    }

    /// setup inspection 失敗時は secret input を読まず、YubiKey storage 変更へ進ませない。
    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_setup_inspection_fails() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(0)
            .returning(|_| Ok(false));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(0);
        let pin_input = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("setup inspect failed")));
        storage.expect_initialize_secret_storage().times(0);
        let report = ports::io::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await;

        assert!(result.is_err(), "setup failure must stop before input");
    }

    /// initialize 失敗時は bootstrap secret 入力前に停止し、未初期化 device へ値を載せない。
    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_initialize_fails_before_input() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(0);
        secret_input.expect_read_bitwarden_client_secret().times(0);
        let pin_input = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("initialize failed")));
        storage.expect_store_secret().times(0);
        let report = ports::io::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await;

        assert!(result.is_err(), "initialize failure must stop before input");
    }

    /// device が PIN を要求する場合だけ input port から PIN を読み、検証 load へ渡す。
    #[tokio::test]
    async fn enroll_primary_prompt_reads_pin_when_required() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        device
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(true));
        let mut pin_input = ports::io::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, pin| {
                pin.map(ProtectedSecret::to_test_bytes).as_deref() == Some(&b"123456"[..])
            })
            .returning(|_, _, _| Ok(()));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"password")));
        storage
            .expect_store_secret()
            .times(2)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        for name in [
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .in_sequence(&mut sequence)
                .withf(move |_, storage| storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .in_sequence(&mut sequence)
                .withf(move |_, intent, pin| {
                    intent.storage.name == name
                        && pin.map(ProtectedSecret::to_test_bytes).as_deref()
                            == Some(&b"123456"[..])
                })
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BitwardenClientId => material(b"email"),
                        SecretName::BitwardenClientSecret => material(b"password"),
                    })
                });
        }
        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await
    }

    /// secret store 失敗時は finalize と検証へ進ませず、失敗前の順序境界を固定する。
    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_store_fails() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
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
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("store failed")));
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_read().times(0);
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
        let mut report = ports::io::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await;

        assert!(result.is_err(), "store failure must stop before verify");
    }

    /// 保存後の検証 load 失敗時は report 成功を書かず、decode error chain を caller へ返す。
    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_verify_load_fails() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
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
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("verify failed")));
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
        let mut report = ports::io::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
        .await;

        assert!(result.is_err(), "verify failure must stop before report");
    }
}
