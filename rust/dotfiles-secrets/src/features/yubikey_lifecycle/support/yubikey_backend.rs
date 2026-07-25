//! YubiKey PIV の device discovery、slot I/O、secret-protection bridge。
//!
//! この module は YubiKey crate handle と technical state を所有する。port trait 実装は
//! adapter にのみ置く。
//!
//! ## 出典と適用判断
//!
//! repository の復旧契約と PIV 保存対象は
//! [`secret-recovery-spec.md`](../../../docs/secret-recovery/secret-recovery-spec.md) の
//! 「無対話復旧の利用者契約」および「責務分担 / YubiKey」、保存形式は
//! [`yubikey-secret-storage-design.md`](../../../docs/secret-recovery/yubikey-secret-storage-design.md)
//! の「PIV 領域」を正本とする。この module はその保存・復号の**技術的** backend だけを
//! 実装し、secret の必須性、対象名の一意解決、復旧手順、成功/停止条件を決めない。
//!
//! vendor の全体像は [YubiKey Technical Manual の PIV section](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/yk5-apps-piv.html)
//! （PIV application、slot 82--95、PIN/touch policy、metadata）を読む。実際に直接使う
//! `yubikey` 0.9.0-pre.0 API は version 固定の upstream source
//! [`YubiKey::open_by_serial` / `YubiKey::authenticate` / `YubiKey::verify_pin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs)、
//! [`piv::generate` / `piv::metadata` / `piv::decrypt_data`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs)、
//! [`Transaction::get_metadata` / `Transaction::fetch_object` / `Transaction::save_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
//! である。適用判断は API ごとに限定する。`fetch_object` の `Error::NotFound` だけを下記
//! `read_object` で object absence へ翻訳する。`piv::metadata` 経由の
//! `Transaction::get_metadata` の `Error::NotFound` は、それ自体を absence の成功にせず
//! certificate を追加観測する契機にだけ使う。他の `yubikey::Error` は意味を推測せず
//! source error のまま伝播する。実機観測をこの判断の根拠にはしない。
//!
//! Management application discovery の transport は `pcsc` 2.9.0 を直接使う。
//! [`Context::establish` / `release`, `Card::connect` / `disconnect`,
//! `Transaction::end` / `transmit`](https://docs.rs/crate/pcsc/2.9.0/source/src/lib.rs)
//! は cleanup も fallible で ownership と error を返す。したがって
//! `establish → connect → transaction → transmit → end → disconnect → release`
//! を明示し、main と cleanup の双方が失敗すれば main error を source chain に残して
//! cleanup detail も失敗として保持する。PC/SC error は candidate absence、retry、success 等へ
//! 再分類しない。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use super::piv_storage::sha256_lowercase_hex;
use crate::{
    Result,
    features::{
        gpg_backup_recovery::ports::public::{ConnectedYubiKey, EnvelopeRecipient},
        yubikey_lifecycle::domain::piv::{
            PivApplicationVersion, PivDeviceProfile, PivObjectId, SecretStorageSpec,
        },
    },
    foundation::protection::{ProtectedSecret, SecretSession},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::{
    features::yubikey_lifecycle::support::yubikey_piv,
    foundation::protection::{sealed_blob, secret_random},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use rsa::{
    RsaPublicKey,
    pkcs1::DecodeRsaPublicKey,
    pkcs8::{DecodePublicKey, EncodePublicKey},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use yubikey::{
    PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

#[derive(Default)]
pub(crate) struct YubikeyDeviceBackend {
    step_binding: Option<DeviceCandidate>,
}
#[derive(Default)]
pub(crate) struct YubikeyRecipientBackend;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceCandidate {
    pub(crate) serial: u32,
    pub(crate) label: String,
    pub(crate) profile: PivDeviceProfile,
}
pub(crate) trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn reserved_slot_certificate_exists(&mut self) -> Result<bool>;
    fn piv_application_version(&self) -> PivApplicationVersion;
    fn verify_management_pin(&mut self, pin: &ProtectedSecret) -> Result<()>;
    fn change_management_pin(
        &mut self,
        current_pin: &ProtectedSecret,
        new_pin: &ProtectedSecret,
    ) -> Result<()>;
    fn authenticate_protected_management_key(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<Vec<u8>>;
    fn slot_public_key_spki(&mut self) -> Result<Option<Vec<u8>>>;
    fn remember_generated_public_key(&mut self, key: Vec<u8>);
    fn read_object(&mut self, object: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn empty_object(&mut self, object: PivObjectId) -> Result<()>;
    fn clear_reserved_slot_certificate(&mut self) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        secret: &ProtectedSecret,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret>;
    fn recipient_public_key_fingerprint(&mut self) -> Result<String>;
    fn wrap_dek(&mut self, dek: &ProtectedSecret) -> Result<Vec<u8>>;
    fn unwrap_dek(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret>;
}
pub(crate) struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
    profile: PivDeviceProfile,
}
impl SelectedSecretDevice {
    pub(crate) fn new(device: impl SecretDeviceIo + 'static, profile: PivDeviceProfile) -> Self {
        Self {
            inner: Box::new(device),
            profile,
        }
    }
}
macro_rules! delegate { ($name:ident($($arg:ident:$typ:ty),*) -> $ret:ty) => { fn $name(&mut self,$($arg:$typ),*) -> $ret { self.inner.$name($($arg),*) } }; }
impl SecretDeviceIo for SelectedSecretDevice {
    delegate!(key_exists() -> Result<bool>);
    delegate!(reserved_slot_certificate_exists() -> Result<bool>);
    fn piv_application_version(&self) -> PivApplicationVersion {
        self.inner.piv_application_version()
    }
    delegate!(verify_management_pin(pin:&ProtectedSecret) -> Result<()>);
    delegate!(change_management_pin(current_pin:&ProtectedSecret,new_pin:&ProtectedSecret) -> Result<()>);
    delegate!(authenticate_protected_management_key() -> Result<()>);
    fn generate_key(&mut self) -> Result<Vec<u8>> {
        self.profile.ensure_pin_free_recovery_supported()?;
        self.inner.generate_key()
    }
    delegate!(slot_public_key_spki() -> Result<Option<Vec<u8>>>);
    fn remember_generated_public_key(&mut self, key: Vec<u8>) {
        self.inner.remember_generated_public_key(key)
    }
    delegate!(read_object(object:PivObjectId) -> Result<Option<Vec<u8>>>);
    delegate!(write_object(object:PivObjectId,value:&mut [u8]) -> Result<()>);
    delegate!(empty_object(object:PivObjectId) -> Result<()>);
    delegate!(clear_reserved_slot_certificate() -> Result<()>);
    delegate!(seal_for_storage(storage:SecretStorageSpec,secret:&ProtectedSecret) -> Result<Vec<u8>>);
    delegate!(open_from_storage(storage:SecretStorageSpec,encoded:&[u8]) -> Result<ProtectedSecret>);
    delegate!(recipient_public_key_fingerprint() -> Result<String>);
    delegate!(wrap_dek(dek:&ProtectedSecret) -> Result<Vec<u8>>);
    delegate!(unwrap_dek(wrapped:&[u8]) -> Result<ProtectedSecret>);
}

pub(crate) fn discover_devices(_: &mut YubikeyDeviceBackend) -> Result<Vec<DeviceCandidate>> {
    discover_devices_uncached()
}

pub(crate) fn bind_discovery_step(backend: &mut YubikeyDeviceBackend, candidate: DeviceCandidate) {
    backend.step_binding = Some(candidate);
}

pub(crate) fn bound_device_profile(
    backend: &YubikeyDeviceBackend,
    serial: u32,
) -> Option<PivDeviceProfile> {
    backend
        .step_binding
        .as_ref()
        .filter(|candidate| candidate.serial == serial)
        .map(|candidate| candidate.profile)
}

pub(crate) fn discover_devices_uncached() -> Result<Vec<DeviceCandidate>> {
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    {
        discover_devices_with_management_application()
    }
    #[cfg(feature = "secrets-internal-test-stub")]
    {
        crate::features::yubikey_lifecycle::support::internal_stub_yubikey::discover_devices()
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn discover_devices_with_management_application() -> Result<Vec<DeviceCandidate>> {
    use pcsc::{Context as PcscContext, Disposition, Protocols, Scope, ShareMode};
    use std::ffi::CString;

    const SELECT_MANAGEMENT_APPLICATION: &[u8] = &[
        0x00, 0xa4, 0x04, 0x00, 0x08, 0xa0, 0x00, 0x00, 0x05, 0x27, 0x47, 0x11, 0x17,
    ];
    // Yubico の GET DEVICE INFORMATION specification が serial、firmware version、form factor
    // を一つの GET DEVICE INFORMATION response として定義する。form-factor byte の bit 7
    // が FIPS Series の公式 signal であり、reader 名や version から推測してはならない。
    //
    // 一次資料:
    // - https://docs.yubico.com/yesdk/users-manual/application-otp/commands-get-device-info.html
    // - https://github.com/Yubico/yubikey-manager/blob/5.9.0/yubikit/management.py
    // 固定した yubikey 0.9.0-pre.0 の reader flow は最初に接続して PIV を SELECT する。確認済みの
    // PIV 対応 YubiKey reader だけを、以下の Management application flow へ渡す。
    let mut candidate_context = yubikey::reader::Context::open()
        .context("failed to establish YubiKey reader discovery context")?;
    let mut candidate_labels = Vec::new();
    for reader in candidate_context
        .iter()
        .context("failed to enumerate YubiKey reader candidates")?
    {
        let label = reader.name().into_owned();
        match reader.open() {
            Ok(yubikey) => {
                yubikey
                    .disconnect(Disposition::LeaveCard)
                    .map_err(|(_, error)| error)
                    .with_context(|| {
                        format!("failed to disconnect YubiKey PIV candidate `{label}`")
                    })?;
                candidate_labels.push(label);
            }
            Err(yubikey::Error::AppletNotFound { applet_name: "PIV" })
            | Err(yubikey::Error::PcscError {
                inner: Some(pcsc::Error::NoSmartcard),
            }) => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("YubiKey candidate discovery failed for reader `{label}`")
                });
            }
        }
    }

    let context = PcscContext::establish(Scope::System)
        .context("failed to establish PC/SC context for YubiKey management discovery")?;
    let main_result = (|| {
        let mut devices = Vec::new();
        for label in candidate_labels {
            let reader = CString::new(label.as_bytes())
                .context("YubiKey PC/SC reader name contained an interior NUL")?;
            let candidate = with_pcsc_card(
                &context,
                &reader,
                ShareMode::Shared,
                Protocols::ANY,
                |transaction| {
                    let select = transmit_management_command(
                        transaction,
                        SELECT_MANAGEMENT_APPLICATION,
                        "SELECT management application",
                        true,
                    )?;
                    let Some(select) = select else {
                        return Ok(None);
                    };
                    let _select = select;
                    let device_information = read_management_device_information(transaction)
                        .with_context(|| {
                            format!("failed to read YubiKey device information from `{label}`")
                        })?;
                    parse_management_device_information(&device_information, label.clone())
                        .map(Some)
                },
            )?;
            if let Some(candidate) = candidate {
                devices.push(candidate);
            }
        }
        Ok(devices)
    })();
    let cleanup = context.release().map_err(|(_, error)| {
        anyhow::Error::new(error).context("failed to release PC/SC context after YubiKey discovery")
    });
    combine_main_and_cleanup(main_result, cleanup)
}

/// pcsc 2.9.0 の fallible cleanup API をすべて明示し、main と cleanup の両 failure を保持する。
///
/// `Transaction::end(LeaveCard)`、`Card::disconnect(LeaveCard)`、caller の `Context::release` は
/// fixed source が Drop の error を捨てると明記するため、正常 path でも Drop 任せにしない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn with_pcsc_card<T>(
    context: &pcsc::Context,
    reader: &std::ffi::CStr,
    share_mode: pcsc::ShareMode,
    protocols: pcsc::Protocols,
    operation: impl FnOnce(&pcsc::Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let mut card = context
        .connect(reader, share_mode, protocols)
        .context("failed to connect to YubiKey PC/SC reader")?;
    let transaction = card
        .transaction()
        .context("failed to begin YubiKey PC/SC transaction")?;
    let main_result = operation(&transaction);
    let transaction_cleanup = match transaction.end(pcsc::Disposition::LeaveCard) {
        Ok(()) => Ok(()),
        Err((transaction, error)) => {
            drop(transaction);
            Err(anyhow::Error::new(error).context("failed to end YubiKey PC/SC transaction"))
        }
    };
    let combined = combine_main_and_cleanup(main_result, transaction_cleanup);
    let card_cleanup = match card.disconnect(pcsc::Disposition::LeaveCard) {
        Ok(()) => Ok(()),
        Err((card, error)) => {
            drop(card);
            Err(anyhow::Error::new(error).context("failed to disconnect YubiKey PC/SC card"))
        }
    };
    combine_main_and_cleanup(combined, card_cleanup)
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn combine_main_and_cleanup<T>(main: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (main, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(anyhow::Error::new(YubikeyCleanupFailure {
            main: None,
            _cleanup: cleanup,
        })),
        (Err(error), Err(cleanup)) => Err(anyhow::Error::new(YubikeyCleanupFailure {
            main: Some(error),
            _cleanup: cleanup,
        })),
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
#[derive(Debug)]
struct YubikeyCleanupFailure {
    main: Option<anyhow::Error>,
    _cleanup: anyhow::Error,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl std::fmt::Display for YubikeyCleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("YubiKey cleanup failed")
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl std::error::Error for YubikeyCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.main
            .as_ref()
            .map(anyhow::Error::as_ref)
            .or_else(|| Some(self._cleanup.as_ref()))
    }
}

#[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
mod pcsc_cleanup_tests {
    use super::combine_main_and_cleanup;

    #[test]
    fn preserves_success_when_main_and_cleanup_succeed() -> crate::Result<()> {
        assert_eq!(combine_main_and_cleanup(Ok(7_u8), Ok(()))?, 7);
        Ok(())
    }

    #[test]
    fn preserves_main_failure_when_cleanup_succeeds() {
        let error = combine_main_and_cleanup::<()>(Err(anyhow::anyhow!("main failure")), Ok(()))
            .expect_err("main failure must propagate");
        assert_eq!(error.to_string(), "main failure");
    }

    #[test]
    fn turns_cleanup_failure_after_success_into_failure() {
        let error = combine_main_and_cleanup(Ok(()), Err(anyhow::anyhow!("cleanup failure")))
            .expect_err("cleanup failure must not become success");
        assert_eq!(error.to_string(), "YubiKey cleanup failed");
    }

    #[test]
    fn preserves_sources_without_rendering_cleanup_detail() {
        let error = combine_main_and_cleanup::<()>(
            Err(anyhow::anyhow!("main failure")),
            Err(anyhow::anyhow!("cleanup failure")),
        )
        .expect_err("both failures must remain a failure");
        assert_eq!(error.to_string(), "YubiKey cleanup failed");
        assert!(
            error
                .chain()
                .any(|source| source.to_string() == "main failure")
        );
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn read_management_device_information(transaction: &pcsc::Transaction<'_>) -> Result<Vec<u8>> {
    const MORE_DATA_TAG: u8 = 0x10;

    // yubikey-manager 5.9.0 `ManagementSession::_do_read_device_info` と同じく page 0 から
    // `TAG_MORE_DATA` が消えるまで読み、各 page の length prefix を独立に検証する。
    let mut page = 0_u8;
    let mut merged_tlvs = Vec::new();
    loop {
        let command = [0x00, 0x1d, page, 0x00, 0x00];
        let encoded =
            transmit_management_command(transaction, &command, "GET DEVICE INFORMATION", false)?
                .context("YubiKey Management application became unavailable")?;
        let (&declared_length, mut tlvs) = encoded
            .split_first()
            .context("YubiKey device information page was empty")?;
        if usize::from(declared_length) != tlvs.len() {
            anyhow::bail!("YubiKey device information page length was inconsistent");
        }
        let mut more_data = false;
        while !tlvs.is_empty() {
            let (&tag, rest) = tlvs
                .split_first()
                .context("YubiKey device information TLV omitted a tag")?;
            let (&length, rest) = rest
                .split_first()
                .context("YubiKey device information TLV omitted a length")?;
            let length = usize::from(length);
            if rest.len() < length {
                anyhow::bail!("YubiKey device information TLV exceeded its page length");
            }
            let (value, remaining) = rest.split_at(length);
            tlvs = remaining;
            if tag == MORE_DATA_TAG {
                if value != [1] {
                    anyhow::bail!("YubiKey device information more-data TLV was invalid");
                }
                more_data = true;
            } else if matches!(tag, 0x02 | 0x04 | 0x05) {
                merged_tlvs.extend_from_slice(&[tag, length as u8]);
                merged_tlvs.extend_from_slice(value);
            }
        }
        if !more_data {
            break;
        }
        page = page
            .checked_add(1)
            .context("YubiKey device information page counter overflowed")?;
    }
    let length = u8::try_from(merged_tlvs.len())
        .context("YubiKey device information exceeded the supported encoded length")?;
    let mut encoded = Vec::with_capacity(merged_tlvs.len() + 1);
    encoded.push(length);
    encoded.extend_from_slice(&merged_tlvs);
    Ok(encoded)
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn transmit_management_command(
    transaction: &pcsc::Transaction<'_>,
    command: &[u8],
    operation: &'static str,
    application_selection: bool,
) -> Result<Option<Vec<u8>>> {
    let mut response = [0_u8; pcsc::MAX_BUFFER_SIZE];
    let response = transaction
        .transmit(command, &mut response)
        .with_context(|| format!("PC/SC transmit failed during {operation}"))?;
    let (data, status) = response
        .split_last_chunk::<2>()
        .context("YubiKey management response omitted the status word")?;
    if *status == [0x90, 0x00] {
        return Ok(Some(data.to_vec()));
    }
    // ISO 7816-4 の 6A82 は文書化された「file/application not found」である。候補除外へ使えるのは
    // SELECT のときだけであり、transport/sharing/unknown status はすべて停止させる。
    if application_selection && *status == [0x6a, 0x82] {
        return Ok(None);
    }
    anyhow::bail!("YubiKey management command {operation} failed")
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn parse_management_device_information(data: &[u8], label: String) -> Result<DeviceCandidate> {
    const SERIAL_TAG: u8 = 0x02;
    const FORM_FACTOR_TAG: u8 = 0x04;
    const VERSION_TAG: u8 = 0x05;
    const FIPS_SERIES_MASK: u8 = 0x80;

    let (&declared_length, tlvs) = data
        .split_first()
        .context("YubiKey device information response was empty")?;
    if usize::from(declared_length) != tlvs.len() {
        anyhow::bail!("YubiKey device information response length was inconsistent");
    }
    let mut serial = None;
    let mut version = None;
    let mut form_factor = None;
    let mut cursor = tlvs;
    while !cursor.is_empty() {
        let (&tag, rest) = cursor
            .split_first()
            .context("YubiKey device information TLV omitted a tag")?;
        let (&length, rest) = rest
            .split_first()
            .context("YubiKey device information TLV omitted a length")?;
        let length = usize::from(length);
        if rest.len() < length {
            anyhow::bail!("YubiKey device information TLV exceeded the response length");
        }
        let (value, remaining) = rest.split_at(length);
        cursor = remaining;
        match tag {
            SERIAL_TAG => {
                if value.is_empty() || value.len() > 4 {
                    anyhow::bail!("YubiKey serial TLV had an invalid length");
                }
                serial = Some(
                    value
                        .iter()
                        .fold(0_u32, |serial, byte| (serial << 8) | u32::from(*byte)),
                );
            }
            VERSION_TAG => {
                let [major, minor, patch] = value else {
                    anyhow::bail!("YubiKey version TLV had an invalid length");
                };
                version = Some(PivApplicationVersion {
                    major: *major,
                    minor: *minor,
                    patch: *patch,
                });
            }
            FORM_FACTOR_TAG => {
                let [value] = value else {
                    anyhow::bail!("YubiKey form-factor TLV had an invalid length");
                };
                form_factor = Some(*value);
            }
            _ => {}
        }
    }
    let serial = serial.context("YubiKey device information omitted serial")?;
    if serial == 0 {
        anyhow::bail!("YubiKey device information returned an invalid zero serial");
    }
    Ok(DeviceCandidate {
        serial,
        label,
        profile: PivDeviceProfile {
            version: version.context("YubiKey device information omitted firmware version")?,
            fips_series: form_factor.context("YubiKey device information omitted form factor")?
                & FIPS_SERIES_MASK
                != 0,
        },
    })
}

#[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
mod management_device_information_tests {
    use super::parse_management_device_information;
    use crate::features::yubikey_lifecycle::domain::piv::PivApplicationVersion;

    #[test]
    fn parses_serial_version_and_fips_from_one_official_tlv_response() -> crate::Result<()> {
        let candidate = parse_management_device_information(
            &[
                0x0e, 0x05, 0x03, 0x05, 0x07, 0x01, 0x02, 0x04, 0x02, 0x37, 0x32, 0x05, 0x04, 0x01,
                0x80,
            ],
            "fixture-reader".to_owned(),
        )?;

        assert_eq!(candidate.serial, 37171717);
        assert_eq!(
            candidate.profile.version,
            PivApplicationVersion {
                major: 5,
                minor: 7,
                patch: 1,
            }
        );
        assert!(candidate.profile.fips_series);
        Ok(())
    }

    #[test]
    fn rejects_missing_form_factor_instead_of_inferring_from_reader_or_version() {
        let result = parse_management_device_information(
            &[
                0x0b, 0x05, 0x03, 0x05, 0x07, 0x01, 0x02, 0x04, 0x02, 0x37, 0x32, 0x05,
            ],
            "YubiKey FIPS-looking-name".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_missing_device_information_version_even_when_select_was_available() {
        let result = parse_management_device_information(
            &[0x09, 0x02, 0x04, 0x02, 0x37, 0x32, 0x05, 0x04, 0x01, 0x80],
            "fixture-reader".to_owned(),
        );

        assert!(result.is_err());
    }
}
pub(crate) fn open_device_by_serial(
    _backend: &mut YubikeyDeviceBackend,
    serial: u32,
) -> Result<SelectedSecretDevice> {
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    {
        let profile = _backend
            .step_binding
            .take()
            .filter(|candidate| candidate.serial == serial)
            .map(|candidate| candidate.profile)
            .or_else(|| {
                discover_devices_uncached()
                    .ok()?
                    .into_iter()
                    .find(|candidate| candidate.serial == serial)
                    .map(|candidate| candidate.profile)
            })
            .ok_or_else(|| {
                anyhow::anyhow!("YubiKey profile could not be bound to serial {serial}")
            })?;
        Ok(SelectedSecretDevice::new(
            YubikeySecretDevice {
                yubikey: YubiKey::open_by_serial(Serial(serial))?,
                generated_public_key: None,
            },
            profile,
        ))
    }
    #[cfg(feature = "secrets-internal-test-stub")]
    {
        crate::features::yubikey_lifecycle::support::internal_stub_yubikey::open_device_by_serial(
            serial,
        )
    }
}
pub(crate) fn open_recipient_device(
    _: &mut YubikeyRecipientBackend,
    serial: u32,
) -> Result<SelectedSecretDevice> {
    open_device_by_serial(&mut YubikeyDeviceBackend::default(), serial)
}
pub(crate) fn resolve_connected_recipient(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
) -> Result<ConnectedYubiKey> {
    let mut device = open_recipient_device(backend, serial)?;
    ConnectedYubiKey::new(
        serial.to_string(),
        &device.recipient_public_key_fingerprint()?,
    )
}
pub(crate) fn wrap_dek_for_recipient(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
    dek: &ProtectedSecret,
) -> Result<EnvelopeRecipient> {
    let mut device = open_recipient_device(backend, serial)?;
    let connected = ConnectedYubiKey::new(
        serial.to_string(),
        &device.recipient_public_key_fingerprint()?,
    )?;
    EnvelopeRecipient::new(&connected, device.wrap_dek(dek)?)
}
pub(crate) fn unwrap_dek(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
    recipient: &EnvelopeRecipient,
) -> Result<ProtectedSecret> {
    let _session = SecretSession::start()?;
    let mut device = open_recipient_device(backend, serial)?;
    device.unwrap_dek(recipient.wrapped_dek())
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
#[cfg(not(feature = "secrets-internal-test-stub"))]
struct YubikeySecretDevice {
    yubikey: YubiKey,
    generated_public_key: Option<Vec<u8>>,
}
#[cfg(not(feature = "secrets-internal-test-stub"))]
impl YubikeySecretDevice {
    fn slot_public_key_spki_from_metadata(&mut self) -> Result<Option<Vec<u8>>> {
        piv::metadata(&mut self.yubikey, SECRET_SLOT)?
            .public
            .map(|p| {
                RsaPublicKey::from_pkcs1_der(p.subject_public_key.raw_bytes())
                    .context("failed to parse YubiKey slot 82 metadata public key")?
                    .to_public_key_der()
                    .context("failed to DER-encode YubiKey slot 82 metadata public key")
                    .map(|v| v.as_bytes().to_vec())
            })
            .transpose()
    }
    fn slot_public_key(&mut self) -> Result<RsaPublicKey> {
        if let Some(key) = self.generated_public_key.as_deref() {
            return RsaPublicKey::from_public_key_der(key)
                .context("failed to parse cached YubiKey secret storage public key");
        }
        let key = self
            .slot_public_key_spki()?
            .ok_or_else(|| anyhow::anyhow!("YubiKey secret storage key metadata is unavailable"))?;
        RsaPublicKey::from_public_key_der(&key)
            .context("failed to parse YubiKey secret storage public key")
    }
    fn wrap_content_key(&mut self, key: &ProtectedSecret) -> Result<Vec<u8>> {
        secret_random::rsa_oaep_encrypt(&self.slot_public_key()?, key)
    }
    fn unwrap_content_key(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret> {
        sealed_blob::unwrap_content_key_from_decrypt(
            || {
                piv::decrypt_data(
                    &mut self.yubikey,
                    wrapped,
                    AlgorithmId::Rsa2048,
                    SECRET_SLOT,
                )
                .map_err(anyhow::Error::new)
            },
            256,
        )
    }
}
#[cfg(not(feature = "secrets-internal-test-stub"))]
impl SecretDeviceIo for YubikeySecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(metadata) => Ok(metadata.public.is_some()),
            Err(yubikey::Error::NotFound) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    fn reserved_slot_certificate_exists(&mut self) -> Result<bool> {
        match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
            Ok(value) => Ok(!value.is_empty()),
            Err(yubikey::Error::NotFound) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    fn piv_application_version(&self) -> PivApplicationVersion {
        let v = self.yubikey.version();
        PivApplicationVersion {
            major: v.major,
            minor: v.minor,
            patch: v.patch,
        }
    }
    fn verify_management_pin(&mut self, pin: &ProtectedSecret) -> Result<()> {
        yubikey_piv::verify_pin(&mut self.yubikey, pin)
    }
    fn change_management_pin(
        &mut self,
        current_pin: &ProtectedSecret,
        new_pin: &ProtectedSecret,
    ) -> Result<()> {
        yubikey_piv::change_pin(&mut self.yubikey, current_pin, new_pin)
    }
    fn authenticate_protected_management_key(&mut self) -> Result<()> {
        let key = yubikey::MgmKey::get_protected(&mut self.yubikey).map_err(anyhow::Error::new)?;
        self.yubikey
            .authenticate(&key)
            .map_err(anyhow::Error::new)?;
        let metadata = piv::metadata(
            &mut self.yubikey,
            SlotId::Management(yubikey::piv::ManagementSlotId::Management),
        )?;
        if metadata.default != Some(false) {
            anyhow::bail!("YubiKey PIN-protected management key metadata is not healthy")
        }
        Ok(())
    }
    fn generate_key(&mut self) -> Result<Vec<u8>> {
        let public = piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Never,
            TouchPolicy::Always,
        )?;
        let encoded = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse generated YubiKey secret storage public key")?
            .to_public_key_der()
            .context("failed to DER-encode generated YubiKey secret storage public key")?
            .as_bytes()
            .to_vec();
        self.generated_public_key = Some(encoded.clone());
        Ok(encoded)
    }
    fn slot_public_key_spki(&mut self) -> Result<Option<Vec<u8>>> {
        self.slot_public_key_spki_from_metadata()
    }
    fn remember_generated_public_key(&mut self, key: Vec<u8>) {
        self.generated_public_key = Some(key)
    }
    /// custom object を読み、crate が定義する absence だけを `None` にする。
    ///
    /// 出典: repository 正本は
    /// [`yubikey-secret-storage-design.md` の「Object IDs」](../../../docs/secret-recovery/yubikey-secret-storage-design.md#object-ids)、
    /// vendor / SDK の正確な根拠は `yubikey` 0.9.0-pre.0
    /// [`Transaction::fetch_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
    /// （`StatusWords::NotFoundError` を `Error::NotFound` にする分岐）である。
    /// 適用判断: `Error::NotFound` だけを object absence として `None` にし、成功した
    /// zero-length payload は physical object が存在する `Some(vec![])` のまま保持する。
    /// そのほかの error は status、permission、device state 等へ再分類せず伝播する。
    fn read_object(&mut self, object: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn write_object(&mut self, object: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.yubikey.save_object(object.value(), value)?;
        Ok(())
    }
    fn empty_object(&mut self, object: PivObjectId) -> Result<()> {
        self.write_object(object, &mut [])
    }
    fn clear_reserved_slot_certificate(&mut self) -> Result<()> {
        self.yubikey
            .save_object(SECRET_SLOT_CERT_OBJECT_ID, &mut [])?;
        self.generated_public_key = None;
        Ok(())
    }
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        secret: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_material_with_key_wrap(
            storage.secret_id,
            secret,
            &storage.additional_data,
            |key| self.wrap_content_key(key),
        )
    }
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        sealed_blob::open_material_with_key_unwrap(
            encoded,
            storage.secret_id,
            |wrapped| self.unwrap_content_key(wrapped),
            &storage.additional_data,
        )
    }
    fn recipient_public_key_fingerprint(&mut self) -> Result<String> {
        let der = self
            .slot_public_key()?
            .to_public_key_der()
            .context("failed to DER-encode YubiKey slot 82 public key")?;
        Ok(sha256_lowercase_hex(der.as_bytes()))
    }
    fn wrap_dek(&mut self, dek: &ProtectedSecret) -> Result<Vec<u8>> {
        self.wrap_content_key(dek)
    }
    fn unwrap_dek(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret> {
        self.unwrap_content_key(wrapped)
    }
}
