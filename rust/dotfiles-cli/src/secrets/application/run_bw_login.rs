//! bw-login の YubiKey secret 取得順序と email override 判断を application に固定し、`bw` CLI 実行詳細を
//! port 境界の外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bw_login::{BwLoginEmail, BwLoginSummary, BwOtp},
        commands::BwLoginCommand,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
    support::protection::{ProtectedSecret, bw_login},
};

/// YubiKey から `bw-email` / `bw-password` を取得し、OTP を入力させて Bitwarden Password Manager CLI に
/// login / unlock する（spec L178）。
///
/// secret 取得順序と email override 判断（YubiKey の `bw-email` を使うか `--email` override を使うか）は
/// application が持つ。`bw login` / `bw unlock` の子プロセス実行、master password の `BW_PASSWORD` env 注入、
/// session key 取得は `BwLoginPort` 境界へ閉じる。master password は port へ保護値として渡し、application は
/// その平文を取り出さない。`BW_SESSION`（session key）は disk / dotfile へ永続化せず、report で利用者へ surface
/// するだけにする。OTP は単回トークンのため可視入力で読み、argv に載る（spec L178）。
#[expect(
    clippy::too_many_arguments,
    reason = "bw-login は device/pin/storage/otp-input/bw-login/report の port を順序適用する単一 use case"
)]
pub(crate) async fn run_bw_login<D, P, S, O, B, R>(
    command: BwLoginCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    otp_input: &O,
    bw_login_port: &B,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    O: ports::BwOtpInputPort,
    B: ports::BwLoginPort,
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

    // login email を決める。`--email` override が指定された場合は YubiKey の `bw-email` を読まず override を使う。
    // override は非秘匿の plain 文字列で、domain rule で argv 安全性を検証する。override が無い場合だけ YubiKey の
    // `bw-email` を読み出し、protection 境界の内側で検証済み email へ翻訳する（平文を application へ取り出さない）。
    let email = match &command.email_override {
        Some(value) => BwLoginEmail::parse(value)?,
        None => {
            let stored_email =
                load_yubikey_secret(serial, SecretName::BwEmail, storage_port, pin.as_ref())?;
            bw_login::parse_email(&stored_email)?
        }
    };

    // master password を YubiKey から保護値として読み出す。平文は取り出さず、そのまま port へ渡す。
    let password = load_yubikey_secret(serial, SecretName::BwPassword, storage_port, pin.as_ref())?;

    // YubiKey OTP を可視入力で読み、domain rule で検証する（argv に載る単回トークン）。
    let otp = BwOtp::parse(&otp_input.read_bw_otp()?)?;

    // `bw login` / `bw unlock` を port 経由で実行する。master password は port の `BW_PASSWORD` env 境界でだけ
    // 子プロセスへ渡る。session key（`BW_SESSION` 値）を受け取り、report で利用者へ surface する。
    let session = bw_login_port
        .login_and_unlock(&email, &password, &otp)
        .await?;
    report.write_bw_login_report(&BwLoginSummary { session })
}

/// YubiKey storage の read 経路（inspect → intent → load → validate）で指定 secret を取得する。
fn load_yubikey_secret<S>(
    serial: u32,
    name: SecretName,
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
    //! bw-login の secret 取得順序・email override・OTP 入力・port 呼び出し・report を mockall + Sequence で
    //! 検証する単体テスト。`bw` CLI（`BwLoginPort`）/ storage / OTP 入力を port mock で差し替え、master password が
    //! port へ保護値として渡ること、`--email` override で YubiKey の `bw-email` を読まないことを検証する。
    //! test double は持ち込まない。

    use crate::secrets::{
        domain::{
            bw_login::{BwLoginEmail, BwOtp, BwSessionKey},
            commands::BwLoginCommand,
            manifest::SecretManifest,
            piv::SecretName,
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_bw_login;

    const SESSION: &str = "SESSIONKEY==";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// 指定 secret 名の inspect → load を 1 回ずつ期待し、名前ごとの test 値を返す。
    fn expect_storage_secret(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
        name: SecretName,
        value: &'static [u8],
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, storage| *actual_serial == serial && storage.name == name)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, intent, _| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(move |_, _, _| Ok(material(value)));
    }

    #[tokio::test]
    async fn bw_login_reads_yubikey_email_and_logs_in() -> crate::Result<()> {
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
        // override 無しでは bw-email を読み、続けて bw-password を読む。
        expect_storage_secret(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwEmail,
            b"user@example.com",
        );
        expect_storage_secret(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwPassword,
            b"master-password",
        );

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(
                |email: &BwLoginEmail, password: &ProtectedSecret, otp: &BwOtp| {
                    email.as_str() == "user@example.com"
                        && otp.as_str() == "cccccbtdvuotp"
                        && *password == material(b"master-password")
                },
            )
            .returning(|_, _, _| Ok(BwSessionKey::parse(SESSION).expect("session")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_bw_login_report()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|summary| summary.session.as_str() == SESSION)
            .returning(|_| Ok(()));

        run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email_override: None,
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &otp_input,
            &bw_login,
            &report,
        )
        .await
    }

    #[tokio::test]
    async fn bw_login_uses_email_override_without_reading_stored_email() -> crate::Result<()> {
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
        // override 指定時は bw-email を読まず、bw-password だけを読む。
        storage
            .expect_inspect_secret_storage_read()
            .withf(|_, storage| storage.name == SecretName::BwEmail)
            .times(0);
        storage
            .expect_inspect_secret_storage_read()
            .withf(|_, storage| storage.name == SecretName::BwPassword)
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .withf(|_, intent, _| intent.storage.name == SecretName::BwPassword)
            .times(1)
            .returning(|_, _, _| Ok(material(b"master-password")));

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .withf(|email: &BwLoginEmail, _, _| email.as_str() == "override@example.com")
            .returning(|_, _, _| Ok(BwSessionKey::parse(SESSION).expect("session")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_bw_login_report()
            .times(1)
            .returning(|_| Ok(()));

        run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email_override: Some("  override@example.com  ".to_owned()),
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &otp_input,
            &bw_login,
            &report,
        )
        .await
    }

    #[tokio::test]
    async fn bw_login_stops_when_login_fails_without_reporting() {
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
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .returning(|_, _, _| Ok(material(b"master-password")));

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .returning(|_, _, _| anyhow::bail!("bw login failed"));

        let mut report = ports::MockReportPort::new();
        // login 失敗時は report を書かない。
        report.expect_write_bw_login_report().times(0);

        let result = run_bw_login(
            BwLoginCommand {
                serial: Some(2001),
                email_override: None,
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &otp_input,
            &bw_login,
            &report,
        )
        .await;

        assert!(result.is_err(), "bw login failure must stop bw-login");
    }
}
