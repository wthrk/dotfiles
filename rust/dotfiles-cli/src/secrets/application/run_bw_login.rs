//! bw-login の取得・実行順序を固定し、`bw` CLI 実行と secret 入出力の実装詳細を port 境界の外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bw_login::BwLoginSummary,
        commands::BwLoginCommand,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
    support::protection::ProtectedSecret,
};

/// YubiKey 由来の `bw-email` / `bw-password` と OTP で Bitwarden Password Manager に login / unlock する。
///
/// 設計（spec L176-178）の手順を順序制御として固定する。device 解決 → 必要なら PIN 入力 →
/// `bw-email`（override 未指定時は YubiKey から取得、指定時は override port で取得）と `bw-password` を
/// YubiKey storage から取得 → OTP を端末入力 → `bw login ... --method 3 --code <otp>` の後
/// `bw unlock --raw` を port 経由で実行 → 結果を report、という順序を application に固定する。
///
/// 順序を application に置くのは次の責務境界を保護するためである。master password は `BW_PASSWORD` env で
/// のみ子プロセスへ渡し保存しない、`bw` CLI 引数組み立てと `BW_SESSION` 取り回しは adapter 側へ閉じる、という
/// secret 入出力境界（secret-handling.md）と外部 command 境界の分離を、use case 手順から分けて維持する。
/// login / unlock のいずれかが失敗した場合は停止条件として error を伝播し、後続処理へ進ませない。
#[expect(
    clippy::too_many_arguments,
    reason = "bw-login は device/pin/storage/email-override/otp/bw-login/report の port を順序適用する単一 use case"
)]
pub(crate) fn run_bw_login<D, P, S, E, O, L, R>(
    command: BwLoginCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    email_override: &E,
    otp_input: &O,
    bw_login: &L,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    E: ports::BwLoginEmailOverridePort,
    O: ports::BwLoginOtpInputPort,
    L: ports::BwLoginPort,
    R: ports::ReportPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // 1. login email を解決する。override 未指定時は YubiKey の `bw-email` を、指定時は override port が
    //    argv の email を保護 carrier 化した値を使う（override 採否は use case 判断、carrier 化は adapter）。
    let email = match &command.email {
        Some(email) => email_override.protect_bw_login_email(email)?,
        None => load_yubikey_secret(SecretName::BwEmail, serial, storage_port, pin.as_ref())?,
    };

    // 2. master password を YubiKey から取得する。以後 `BW_PASSWORD` env でのみ子プロセスへ渡す。
    let password = load_yubikey_secret(SecretName::BwPassword, serial, storage_port, pin.as_ref())?;

    // 3. YubiKey OTP を端末から取得する（spec L178 の `--method 3 --code <otp>`）。
    let otp = otp_input.read_bw_login_otp()?;

    // 4. `bw login` の後 `bw unlock` を実行する。login / unlock のいずれかが失敗した場合は停止する。
    let summary: BwLoginSummary = bw_login.login_and_unlock(&email, &password, &otp)?;

    // 5. login / unlock の成立を report する。`BW_SESSION` 値そのものは port から返さない。
    report.write_bw_login_report(&summary)
}

/// 指定 secret を YubiKey storage の read 経路（inspect → intent → load → validate）で取得する。
fn load_yubikey_secret<S>(
    name: SecretName,
    serial: u32,
    storage_port: &mut S,
    pin: Option<&ProtectedSecret>,
) -> Result<ProtectedSecret>
where
    S: ports::SecretStoragePort,
{
    let storage = name.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let secret = storage_port
        .load_secret(serial, &intent, pin)
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    //! bw-login の順序制御と停止条件を mockall + Sequence で検証する単体テスト。
    //!
    //! device / storage / email-override / otp / bw-login / report backend を port mock で差し替え、
    //! device 解決→（PIN）→ email/password 取得→OTP 取得→login+unlock→report という順序と、
    //! login/unlock 失敗時に停止して report を書かないことを検証する。`--email` override 経路では
    //! YubiKey の bw-email を読まず override port を使うことも検証する。test double は持ち込まない。

    use crate::secrets::{
        domain::{commands::BwLoginCommand, manifest::SecretManifest, piv::SecretName},
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_bw_login;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> crate::secrets::domain::storage::SecretStorageReadInspection {
        crate::secrets::domain::storage::SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// YubiKey の bw-email / bw-password を順に取得する read 経路を期待値として登録する。
    fn expect_email_and_password(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
    ) {
        for name in [SecretName::BwEmail, SecretName::BwPassword] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .in_sequence(sequence)
                .withf(move |_, storage| storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .in_sequence(sequence)
                .withf(move |_, intent, _| intent.storage.name == name)
                .returning(move |_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"user@example.com"),
                        SecretName::BwPassword => material(b"master-password"),
                        SecretName::BwsAccessToken => material(b"access-token"),
                    })
                });
        }
    }

    #[test]
    fn bw_login_runs_full_order_with_yubikey_email() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_email_and_password(&mut storage, &mut sequence);
        // override 未指定なので override port は呼ばない。
        let mut email_override = ports::MockBwLoginEmailOverridePort::new();
        email_override.expect_protect_bw_login_email().times(0);
        let mut otp = ports::MockBwLoginOtpInputPort::new();
        otp.expect_read_bw_login_otp()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok("123456".to_owned()));
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|email, _password, otp| {
                email == &material(b"user@example.com") && otp == "123456"
            })
            .returning(|_, _, _| {
                Ok(crate::secrets::domain::bw_login::BwLoginSummary::established())
            });
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_bw_login_report()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|summary| summary.logged_in && summary.unlocked)
            .returning(|_| Ok(()));

        run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email: None,
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &email_override,
            &otp,
            &bw_login,
            &report,
        )
    }

    #[test]
    fn bw_login_uses_override_email_and_skips_yubikey_email() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        // override 指定時は YubiKey の bw-email を読まず、bw-password のみ読む。
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .withf(|_, storage| storage.name == SecretName::BwPassword)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .withf(|_, intent, _| intent.storage.name == SecretName::BwPassword)
            .returning(|_, _, _| Ok(material(b"master-password")));
        let mut email_override = ports::MockBwLoginEmailOverridePort::new();
        email_override
            .expect_protect_bw_login_email()
            .times(1)
            .withf(|email| email == "override@example.com")
            .returning(|_| Ok(material(b"override@example.com")));
        let mut otp = ports::MockBwLoginOtpInputPort::new();
        otp.expect_read_bw_login_otp()
            .times(1)
            .returning(|| Ok("654321".to_owned()));
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .withf(|email, _password, _otp| email == &material(b"override@example.com"))
            .returning(|_, _, _| {
                Ok(crate::secrets::domain::bw_login::BwLoginSummary::established())
            });
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_bw_login_report()
            .times(1)
            .returning(|_| Ok(()));

        run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email: Some("override@example.com".to_owned()),
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &email_override,
            &otp,
            &bw_login,
            &report,
        )
    }

    #[test]
    fn bw_login_stops_and_skips_report_when_login_fails() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let mut sequence = mockall::Sequence::new();
        expect_email_and_password(&mut storage, &mut sequence);
        let email_override = ports::MockBwLoginEmailOverridePort::new();
        let mut otp = ports::MockBwLoginOtpInputPort::new();
        otp.expect_read_bw_login_otp()
            .returning(|| Ok("123456".to_owned()));
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .returning(|_, _, _| anyhow::bail!("bw login failed"));
        let mut report = ports::MockReportPort::new();
        // login 失敗時は report を書かずに停止する。
        report.expect_write_bw_login_report().times(0);

        let result = run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email: None,
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &email_override,
            &otp,
            &bw_login,
            &report,
        );

        assert!(result.is_err(), "a bw login failure must stop bw-login");
    }
}
