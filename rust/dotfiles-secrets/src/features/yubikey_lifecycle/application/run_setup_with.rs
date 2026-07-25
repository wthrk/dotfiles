//! setup の PIV management lifecycle を保持する。

use crate::{
    Result,
    features::yubikey_lifecycle::domain::commands::SetupCommand,
    features::yubikey_lifecycle::ports::public::piv_pin_input::PivPinInputPort,
    features::yubikey_lifecycle::{
        domain::storage::{
            SecretStorageSetupIntent, SecretStorageSetupProbe, is_secret_storage_ownership_unknown,
        },
        ports::{DeviceSerialPort, SecretStoragePort},
    },
};

/// current/new/confirmation をデバイス変更前に検証し、完全な検査の後だけ PIV PIN を変更する。
///
/// PIV PIN はアプリケーション全体の状態であり slot 82 専用ではないため、PIN を使わない status の不完全な
/// 観測では変更を許可しない。current PIN の VERIFY と既存 protected management-key の認証を
/// 同じ handle で完了し、PIV version、slot 82、manifest、予約 object を完全に検査して停止条件を
/// 判定してから `change_piv_pin` を呼ぶ。
pub(crate) fn run_setup(
    command: SetupCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    storage: &mut dyn SecretStoragePort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    device.preflight_device_profile(serial)?;
    let current_pin = piv_pin.read_current_piv_pin_secret()?;
    storage
        .begin_piv_pin_setup_preflight(serial, &current_pin)
        .map_err(opaque_setup_failure)?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = storage
        .inspect_secret_storage_setup(serial, &probe)
        .map_err(opaque_setup_failure)?;
    let intent =
        SecretStorageSetupIntent::for_pin_change(inspection).map_err(opaque_setup_failure)?;
    let new_pin = piv_pin.read_new_piv_pin_confirmation()?;
    storage
        .change_piv_pin(serial, &current_pin, &new_pin)
        .map_err(opaque_setup_failure)?;
    storage
        .begin_piv_management_session(serial, new_pin)
        .map_err(opaque_setup_failure)?;
    if !intent.requires_public_key_spki() {
        return Ok(());
    }
    let public_key_spki = storage
        .initialize_secret_storage(serial, intent.clone())
        .map_err(opaque_setup_failure)?;
    if intent.requires_finalization() {
        storage
            .finalize_secret_storage_setup(
                serial,
                intent
                    .manifest_for_public_key(public_key_spki)
                    .map_err(opaque_setup_failure)?,
            )
            .map_err(opaque_setup_failure)?;
    }
    Ok(())
}

/// confirmation 後の PIV setup failure を固定文言へ閉じる。
///
/// 入力値の validation だけは利用者が修正できるため reader の文言を保つ。認証済み preflight、
/// inspection、PIN 変更、初期化、manifest 確定の失敗は card status や既存状態を推測・露出せず、
/// retry/fallback なしで同じ opaque failure として停止する。
fn opaque_setup_failure(error: anyhow::Error) -> anyhow::Error {
    if is_secret_storage_ownership_unknown(&error) {
        return error
            .context("YubiKey PIV setup failed; manual administrator escalation is required");
    }
    error.context("YubiKey PIV setup failed")
}

#[cfg(test)]
mod tests {
    use crate::{
        features::yubikey_lifecycle::{
            domain::{
                commands::SetupCommand,
                manifest::SecretManifest,
                piv::PivApplicationVersion,
                storage::{SecretStorageSetupInspection, SecretStorageSetupProbe},
            },
            ports::public::{MockDeviceSerialPort, MockSecretStoragePort},
        },
        foundation::protection::ProtectedSecret,
    };
    use mockall::Sequence;

    use super::run_setup;

    fn configure_management_device_fixture(device: &mut MockDeviceSerialPort) {
        let _ = device;
    }

    fn fresh_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            reserved_slot_key_exists: false,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: None,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            manifest_bytes: None,
            present_object_ids: Vec::new(),
            nonempty_object_ids: Vec::new(),
        }
    }

    fn current_pin() -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"123456")
    }

    fn new_pin() -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"654321")
    }

    #[test]
    fn setup_runner_applies_the_confirmed_pins_in_order() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        let mut sequence = Sequence::new();
        device
            .expect_resolve_device_serial()
            .withf(|serial| *serial == Some(2001))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        device
            .expect_preflight_device_profile()
            .withf(|serial| *serial == 2001)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(current_pin);
        storage
            .expect_begin_piv_pin_setup_preflight()
            .withf(|serial: &u32, current: &ProtectedSecret| {
                *serial == 2001 && current.to_test_bytes() == b"123456"
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .withf(|serial: &u32, probe: &SecretStorageSetupProbe| {
                *serial == 2001 && !probe.object_ids().is_empty()
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(fresh_inspection()));
        pin.expect_read_new_piv_pin_confirmation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(new_pin);
        storage
            .expect_change_piv_pin()
            .withf(
                |serial: &u32, current: &ProtectedSecret, new: &ProtectedSecret| {
                    *serial == 2001
                        && current.to_test_bytes() == b"123456"
                        && new.to_test_bytes() == b"654321"
                },
            )
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_begin_piv_management_session()
            .withf(|serial: &u32, new: &ProtectedSecret| {
                *serial == 2001 && new.to_test_bytes() == b"654321"
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .ok_or_else(|| anyhow::anyhow!("fixture v2 manifest must contain SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));

        run_setup(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut storage,
        )
    }

    #[test]
    fn setup_runner_stops_after_inspection_failure_with_an_opaque_error() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        device
            .expect_preflight_device_profile()
            .withf(|serial| *serial == 2001)
            .returning(|_| Ok(()));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .returning(current_pin);
        pin.expect_read_new_piv_pin_confirmation().never();
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| Err(anyhow::anyhow!("raw inspection error")));

        let error = run_setup(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut storage,
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("inspection failure must stop setup"))?;
        assert_eq!(error.to_string(), "YubiKey PIV setup failed");
        Ok(())
    }

    #[test]
    fn setup_runner_stops_after_domain_intent_failure_with_an_opaque_error() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        device
            .expect_preflight_device_profile()
            .withf(|serial| *serial == 2001)
            .returning(|_| Ok(()));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .returning(current_pin);
        pin.expect_read_new_piv_pin_confirmation().never();
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    reserved_slot_key_exists: true,
                    slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                    ..fresh_inspection()
                })
            });

        let error = run_setup(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut storage,
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("nonempty storage must stop before PIN change"))?;
        assert_eq!(
            error.to_string(),
            "YubiKey PIV setup failed; manual administrator escalation is required"
        );
        Ok(())
    }

    #[test]
    fn setup_runner_stops_after_pin_change_failure_with_an_opaque_error() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        device
            .expect_preflight_device_profile()
            .withf(|serial| *serial == 2001)
            .returning(|_| Ok(()));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .returning(current_pin);
        pin.expect_read_new_piv_pin_confirmation()
            .returning(new_pin);
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| Ok(fresh_inspection()));
        storage
            .expect_change_piv_pin()
            .returning(|_, _, _| Err(anyhow::anyhow!("raw PIN change error")));

        let error = run_setup(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut storage,
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("PIN change failure must stop before reauthentication"))?;
        assert_eq!(error.to_string(), "YubiKey PIV setup failed");
        Ok(())
    }
}
