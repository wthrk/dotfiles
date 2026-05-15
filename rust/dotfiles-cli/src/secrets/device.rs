//! `dotfiles secrets` の device 層。
//!
//! この層は実機 YubiKey discovery と PIV adapter を `storage::SecretDevice` へ接続する。
//! 端末選択 UI、process 保護の interrupt guard、OAEP 補助、storage trait だけに依存し、
//! command orchestration や bootstrap 入力 schema には依存しない。

use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use sha2::Sha256;
use yubikey::{
    MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};
use zeroize::Zeroizing;

use super::{
    storage::{PivObjectId, SecretDevice},
    util::{
        oaep::oaep_unpad_sha256,
        protection::InterruptGuard,
        terminal::{
            SPARE_SERIAL_NONINTERACTIVE_ERROR, SPARE_WAIT_TIMEOUT_ERROR, YubikeySelectionCandidate,
            select_yubikey_candidate, stdin_is_terminal, wait_for_spare_replacement,
        },
    },
};
use crate::Result;

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};
const SPARE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
const SPARE_DETECT_POLL_INTERVAL: Duration = Duration::from_millis(200);

enum InteractiveDiscovery {
    Found(Vec<(String, YubiKey)>),
    NoDevice,
    OpenError {
        reader: String,
        source: yubikey::Error,
    },
}

type ReaderOpenAttempt<T> = (String, std::result::Result<T, (String, yubikey::Error)>);

/// 1 command 内で共有する実機 YubiKey transaction state。
pub(crate) struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

/// serial 指定または対話選択で 1 本の YubiKey を開く。
///
/// 非対話実行では secret 読み込み前に対象 device を確定するため、serial 指定を必須にする。
pub(crate) fn open_device(serial: Option<u32>) -> Result<YubikeySecretDevice> {
    require_serial_for_noninteractive(serial)?;

    let yubikey = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        select_interactive_yubikey()?
    };

    Ok(YubikeySecretDevice {
        yubikey,
        pin_verified: false,
    })
}

fn open_device_until(
    serial: Option<u32>,
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    require_serial_for_noninteractive(serial)?;

    interrupt.check_interrupted()?;
    let yubikey = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        open_interactive_device_until(deadline, interrupt)?
    };
    interrupt.check_interrupted()?;

    Ok(YubikeySecretDevice {
        yubikey,
        pin_verified: false,
    })
}

/// primary 抜去直後の 0 本状態を許容し、待機期限までは検出を再試行する。
fn open_interactive_device_until(deadline: Instant, interrupt: &InterruptGuard) -> Result<YubiKey> {
    loop {
        interrupt.check_interrupted()?;

        match select_interactive_yubikey_until(deadline, interrupt) {
            Ok(yubikey) => return Ok(yubikey),
            Err(InteractiveSelectError::NoDevice) => {
                let now = Instant::now();
                if now >= deadline {
                    bail!(SPARE_WAIT_TIMEOUT_ERROR);
                }
                let sleep_duration =
                    SPARE_DETECT_POLL_INTERVAL.min(deadline.saturating_duration_since(now));
                thread::sleep(sleep_duration);
            }
            Err(InteractiveSelectError::Other(err)) => return Err(err),
        }
    }
}

/// `enroll-spare` で primary の 3 secret を読み終えた後に spare を開く。
///
/// `--spare-serial` があればその YubiKey を直接開く。対話実行で serial 指定がなければ、
/// まず接続済み候補から選択させる。選択結果が primary と同じ serial の場合だけ、
/// 差し替えを促して Enter 待ちに進む。非対話実行では差し替え prompt を出せないため、
/// `--spare-serial` を必須にする。
pub(crate) fn open_spare_device(
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    require_spare_serial_for_noninteractive(spare_serial)?;

    if let Some(spare_serial) = spare_serial {
        let device = open_device(Some(spare_serial))?;
        ensure_spare_serial(&device, primary_serial)?;
        return Ok(device);
    }

    let deadline = Instant::now() + SPARE_WAIT_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            bail!(SPARE_WAIT_TIMEOUT_ERROR);
        }
        let device = open_device_until(None, deadline, interrupt)?;
        if ensure_spare_serial(&device, primary_serial).is_ok() {
            return Ok(device);
        }

        wait_for_spare_replacement(deadline, interrupt)?;
    }
}

/// 非対話実行では YubiKey 選択 prompt に入る前に対象 serial を要求する。
fn require_serial_for_noninteractive(serial: Option<u32>) -> Result<()> {
    if serial.is_none() && !stdin_is_terminal() {
        bail!("pass --serial in non-interactive use");
    }

    Ok(())
}

/// spare 差し替え prompt が使えない入力元では spare serial を必須にする。
fn require_spare_serial_for_noninteractive(spare_serial: Option<u32>) -> Result<()> {
    if spare_serial.is_none() && !stdin_is_terminal() {
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }

    Ok(())
}

/// spare 登録では primary と同じ serial を、secret 再保存の前に拒否する。
fn ensure_spare_serial(device: &YubikeySecretDevice, primary_serial: Option<u32>) -> Result<()> {
    if Some(device.serial()) == primary_serial {
        bail!("primary and spare YubiKey serial must be different");
    }

    Ok(())
}

/// 接続中の YubiKey を対話的に 1 本選択する。
///
/// 1 本だけ検出された場合はそのまま選び、複数本ある場合は reader 名と serial を
/// 表示して番号入力を求める。
fn select_interactive_yubikey() -> Result<YubiKey> {
    select_interactive_yubikey_with_input(None, false).map_err(map_select_interactive_error)
}

/// spare 待機中の対話選択では、deadline と interrupt を入力待ちにも適用する。
fn select_interactive_yubikey_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> InteractiveSelectResult<YubiKey> {
    select_interactive_yubikey_with_input(Some((deadline, interrupt)), true)
}

/// 複数 YubiKey 選択時だけ、secret 保持中の deadline / interrupt 境界を入力待ちへ渡す。
fn select_interactive_yubikey_with_input(
    timed_input: Option<(Instant, &InterruptGuard)>,
    allow_no_device: bool,
) -> InteractiveSelectResult<YubiKey> {
    let mut context = yubikey::Context::open().map_err(interactive_select_error)?;
    let discovery =
        discover_interactive_yubikeys(&mut context).map_err(interactive_select_error)?;

    match discovery {
        InteractiveDiscovery::NoDevice if allow_no_device => Err(InteractiveSelectError::NoDevice),
        InteractiveDiscovery::NoDevice => Err(interactive_select_error(anyhow::anyhow!(
            "no YubiKey detected"
        ))),
        InteractiveDiscovery::OpenError { reader, source } => {
            let err = anyhow::Error::from(source)
                .context(format!("failed to open YubiKey reader '{reader}'"));
            Err(interactive_select_error(err))
        }
        InteractiveDiscovery::Found(keys) => match keys.as_slice() {
            [_] => {
                let (_, yubikey) = keys
                    .into_iter()
                    .next()
                    .context("single selected YubiKey disappeared")
                    .map_err(interactive_select_error)?;
                Ok(yubikey)
            }
            [_, ..] => {
                let candidates = keys
                    .iter()
                    .map(|(reader, yubikey)| YubikeySelectionCandidate {
                        reader,
                        serial: yubikey.serial().0,
                    })
                    .collect::<Vec<_>>();
                let selected = select_yubikey_candidate(&candidates, timed_input)
                    .map_err(interactive_select_error)?;
                let (_, yubikey) = keys
                    .into_iter()
                    .nth(selected)
                    .context("selected YubiKey disappeared")
                    .map_err(interactive_select_error)?;
                Ok(yubikey)
            }
            [] => Err(interactive_select_error(anyhow::anyhow!(
                "no YubiKey detected"
            ))),
        },
    }
}

/// reader open error を保持し、権限や PC/SC 障害を no-device と誤報しない。
fn discover_interactive_yubikeys(context: &mut yubikey::Context) -> Result<InteractiveDiscovery> {
    let attempts = context
        .iter()?
        .map(|reader| {
            let name = reader.name().into_owned();
            let opened = reader.open().map_err(|err| (name.clone(), err));
            (name, opened)
        })
        .collect::<Vec<_>>();
    classify_interactive_discovery(attempts)
}

enum InteractiveSelectError {
    NoDevice,
    Other(anyhow::Error),
}

type InteractiveSelectResult<T> = std::result::Result<T, InteractiveSelectError>;

/// 対話選択だけが許す no-device sentinel と通常 error path を分ける。
fn interactive_select_error(error: impl Into<anyhow::Error>) -> InteractiveSelectError {
    InteractiveSelectError::Other(error.into())
}

/// 通常の対話選択では no-device sentinel を利用者向け error に戻す。
fn map_select_interactive_error(err: InteractiveSelectError) -> anyhow::Error {
    match err {
        InteractiveSelectError::NoDevice => anyhow::anyhow!("no YubiKey detected"),
        InteractiveSelectError::Other(err) => err,
    }
}

/// 開けた YubiKey を優先し、1 本も開けない場合だけ最初の open error を返す。
fn classify_interactive_discovery(
    attempts: Vec<ReaderOpenAttempt<YubiKey>>,
) -> Result<InteractiveDiscovery> {
    let mut keys = Vec::new();
    let mut first_open_error = None;
    for (reader, opened) in attempts {
        match opened {
            Ok(yubikey) => keys.push((reader, yubikey)),
            Err((name, err)) if first_open_error.is_none() => {
                first_open_error = Some((name, err));
            }
            Err(_) => {}
        }
    }

    if !keys.is_empty() {
        return Ok(InteractiveDiscovery::Found(keys));
    }

    if let Some((reader, source)) = first_open_error {
        return Ok(InteractiveDiscovery::OpenError { reader, source });
    }

    Ok(InteractiveDiscovery::NoDevice)
}

impl YubikeySecretDevice {
    /// PIV private key operation に必要な PIN verification を 1 command で 1 回だけ行う。
    fn verify_pin_once(&mut self, pin: &[u8]) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }

        self.yubikey.verify_pin(pin)?;
        self.pin_verified = true;
        Ok(())
    }

    /// 本実装は既定 management key 固定運用を前提とし、個別 key を使う YubiKey は対象外とする。
    ///
    /// 秘密復旧フローでは既定鍵以外を対象外とし、任意 management key 対応として扱わない。
    fn authenticate_management(&mut self) -> Result<()> {
        let key = MgmKey::get_default(&self.yubikey)?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    /// PIV metadata から取得した public key だけを使い、private key material は host へ出さない。
    fn public_key(&mut self) -> Result<RsaPublicKey> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")
    }
}

impl SecretDevice for YubikeySecretDevice {
    fn serial(&self) -> u32 {
        self.yubikey.serial().0
    }

    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(_) => Ok(true),
            Err(yubikey::Error::NotFound) => {
                match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
                    Ok(_) => Ok(true),
                    Err(yubikey::Error::NotFound) => Ok(false),
                    Err(err) => Err(err.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        let version = self.yubikey.version();
        if version_lt(version, MIN_PIV_METADATA_VERSION) {
            bail!(
                "YubiKey PIV application version must be at least {}",
                format_version(MIN_PIV_METADATA_VERSION)
            );
        }
        if self.yubikey.get_pin_retries()? == 0 {
            bail!("YubiKey PIN retries are exhausted");
        }
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        self.authenticate_management()
    }

    fn generate_key(&mut self) -> Result<()> {
        self.check_key_generation_preconditions()?;
        self.authenticate_management()?;
        piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Once,
            TouchPolicy::Always,
        )?;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match self.yubikey.fetch_object(object_id.value()) {
            Ok(value) => Ok(Some(value)),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &[u8]) -> Result<()> {
        self.authenticate_management()?;
        let mut value = Zeroizing::new(value.to_vec());
        self.yubikey.save_object(object_id.value(), &mut value)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let public = self.public_key()?;
        let wrapped = public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), key)?;
        Ok(Zeroizing::new(wrapped))
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        self.verify_pin_once(pin)
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        let decrypted = piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?;
        let decrypted = Zeroizing::new(decrypted);
        oaep_unpad_sha256(&decrypted, 256)
    }
}

/// `yubikey::Version` に ordering がないため、PIV metadata 要件だけ tuple 比較する。
fn version_lt(left: Version, right: Version) -> bool {
    (left.major, left.minor, left.patch) < (right.major, right.minor, right.patch)
}

/// PIV application version を user-facing error に出す dotted 表記へ変換する。
fn format_version(version: Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_empty_attempts_for_test(
        attempts: Vec<ReaderOpenAttempt<()>>,
    ) -> InteractiveDiscovery {
        let mut first_open_error = None;
        for (_, opened) in attempts {
            if let Err((name, err)) = opened {
                first_open_error = first_open_error.or(Some((name, err)));
            }
        }
        if let Some((reader, source)) = first_open_error {
            return InteractiveDiscovery::OpenError { reader, source };
        }
        InteractiveDiscovery::NoDevice
    }

    #[test]
    fn classify_interactive_discovery_returns_no_device_without_readers() {
        let result = classify_empty_attempts_for_test(Vec::new());
        assert!(matches!(result, InteractiveDiscovery::NoDevice));
    }

    #[test]
    fn classify_interactive_discovery_prefers_first_open_error_when_no_opened_key() -> Result<()> {
        let attempts = vec![
            (
                "reader-a".to_string(),
                Err(("reader-a".to_string(), yubikey::Error::NotFound)),
            ),
            (
                "reader-b".to_string(),
                Err(("reader-b".to_string(), yubikey::Error::NotFound)),
            ),
        ];
        let result = classify_empty_attempts_for_test(attempts);
        match result {
            InteractiveDiscovery::OpenError { reader, source } => {
                assert_eq!(reader, "reader-a");
                assert!(matches!(source, yubikey::Error::NotFound));
            }
            _ => bail!("expected open error"),
        }
        Ok(())
    }
}
