//! `dotfiles secrets` の device 層。
//!
//! 実機 YubiKey discovery と PIV adapter を `ports::SecretDevice` へ接続する。PIN や
//! secret の入力は application 層で取得し、この層は reader / serial 選択と PIV 操作の
//! error contract を固定する。

use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context};
use rand_core::OsRng;
use rsa::{pkcs1::DecodeRsaPublicKey, Oaep, RsaPublicKey};
use sha2::Sha256;
use yubikey::{
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
    MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
};
use zeroize::Zeroizing;

use crate::secrets::{
    domain::PivObjectId,
    ports::SecretDevice,
    support::{protection::InterruptGuard, write_oaep_unpadded_sha256},
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
const SPARE_WAIT_TIMEOUT_ERROR: &str = "timed out waiting for spare YubiKey";

type SelectCandidateFn<'a> = dyn Fn(&[YubikeySelectionCandidate<'_>], Option<(Instant, &InterruptGuard)>) -> Result<usize>
    + 'a;
type WaitForSpareReplacementFn<'a> = dyn Fn(Instant, &InterruptGuard) -> Result<()> + 'a;

pub(super) struct YubikeyInteraction<'a> {
    pub(super) select_candidate: &'a SelectCandidateFn<'a>,
    pub(super) wait_for_spare_replacement: &'a WaitForSpareReplacementFn<'a>,
}

pub(super) struct YubikeySelectionCandidate<'a> {
    pub(super) reader: &'a str,
    pub(super) serial: u32,
}

enum InteractiveDiscovery {
    Found(Vec<(String, YubiKey)>),
    NoDevice,
    OpenError {
        reader: String,
        source: yubikey::Error,
    },
}

type ReaderOpenAttempt<T> = (String, std::result::Result<T, (String, yubikey::Error)>);

/// 開いた YubiKey PIV session と PIN 検証状態を保持する実機 adapter。
///
/// PIN verification は 1 command 中に同じ session へ再利用する。
pub(crate) struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

/// serial 指定または対話選択で 1 本の YubiKey を開く。
pub(super) fn open_device(
    serial: Option<u32>,
    io: &YubikeyInteraction<'_>,
) -> Result<YubikeySecretDevice> {
    let yubikey = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        select_interactive_yubikey(io)?
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
    io: &YubikeyInteraction<'_>,
) -> Result<YubikeySecretDevice> {
    interrupt.check_interrupted()?;
    let yubikey = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        open_interactive_device_until(deadline, interrupt, io)?
    };
    interrupt.check_interrupted()?;

    Ok(YubikeySecretDevice {
        yubikey,
        pin_verified: false,
    })
}

/// deadline まで対話選択可能な YubiKey を待って開く。
///
/// 未挿入状態は再試行し、reader open error は即時に呼び出し側へ返す。
fn open_interactive_device_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
    io: &YubikeyInteraction<'_>,
) -> Result<YubiKey> {
    loop {
        interrupt.check_interrupted()?;

        match select_interactive_yubikey_until(deadline, interrupt, io) {
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

/// spare 登録対象の YubiKey を開く。
///
/// `--spare-serial` があればその YubiKey を直接開く。対話実行で serial 指定がなければ、
/// まず接続済み候補から選択させる。選択結果が primary と同じ serial の場合は
/// 差し替えを促して Enter 待ちに進む。非対話実行時の `--spare-serial` 必須条件は caller 側で検証する。
pub(super) fn open_spare_device(
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
    io: &YubikeyInteraction<'_>,
) -> Result<YubikeySecretDevice> {
    if let Some(spare_serial) = spare_serial {
        let device = open_device(Some(spare_serial), io)?;
        ensure_spare_serial(&device, primary_serial)?;
        return Ok(device);
    }

    let deadline = Instant::now() + SPARE_WAIT_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            bail!(SPARE_WAIT_TIMEOUT_ERROR);
        }
        let device = open_device_until(None, deadline, interrupt, io)?;
        if ensure_spare_serial(&device, primary_serial).is_ok() {
            return Ok(device);
        }

        (io.wait_for_spare_replacement)(deadline, interrupt)?;
    }
}

/// spare 登録対象が primary と別 serial か確認する。
///
/// 同一 serial の場合は、secret 再保存を始める前に失敗する。
fn ensure_spare_serial(device: &YubikeySecretDevice, primary_serial: Option<u32>) -> Result<()> {
    if Some(device.serial()) == primary_serial {
        bail!("primary and spare YubiKey serial must be different");
    }

    Ok(())
}

/// 接続中の YubiKey から対話的に 1 本を選ぶ。
///
/// 検出結果が 1 本の場合はそのまま選び、複数本ある場合は reader 名と serial を
/// 表示して番号入力を求める。
fn select_interactive_yubikey(io: &YubikeyInteraction<'_>) -> Result<YubiKey> {
    select_interactive_yubikey_with_input(None, false, io).map_err(map_select_interactive_error)
}

/// deadline 付きの spare 待機中に、接続中の YubiKey から 1 本を選ぶ。
///
/// 選択 prompt は secret 保持中の deadline と interrupt policy を共有する。
fn select_interactive_yubikey_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
    io: &YubikeyInteraction<'_>,
) -> InteractiveSelectResult<YubiKey> {
    select_interactive_yubikey_with_input(Some((deadline, interrupt)), true, io)
}

/// 接続中の YubiKey discovery 結果を 1 本の選択結果へ変換する。
///
/// timed input が指定された場合は、複数候補の選択入力にも同じ中断と期限の契約を適用する。
fn select_interactive_yubikey_with_input(
    timed_input: Option<(Instant, &InterruptGuard)>,
    allow_no_device: bool,
    io: &YubikeyInteraction<'_>,
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
                let selected = (io.select_candidate)(&candidates, timed_input)
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

/// PC/SC reader の discovery 結果を、選択可能な device 状態へ分類する。
///
/// reader open error は保持し、権限や PC/SC 障害を no-device と誤報しない。
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

/// 通常 error を対話選択用 error 型へ包む。
///
/// no-device は spare 待機で再試行可能な状態として別 variant に分離する。
fn interactive_select_error(error: impl Into<anyhow::Error>) -> InteractiveSelectError {
    InteractiveSelectError::Other(error.into())
}

/// 対話選択用 error を利用者向け error へ戻す。
///
/// 通常の対話選択では no-device sentinel を再試行せず、検出失敗として返す。
fn map_select_interactive_error(err: InteractiveSelectError) -> anyhow::Error {
    match err {
        InteractiveSelectError::NoDevice => anyhow::anyhow!("no YubiKey detected"),
        InteractiveSelectError::Other(err) => err,
    }
}

/// reader open attempts を discovery 状態へ分類する。
///
/// reader が見えているのに開けない状態は、no-device ではなく最初の open error として残す。
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
            Err((_name, _err)) => {}
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
    /// PIV private key operation に必要な PIN verification を実行する。
    ///
    /// 同じ command 中で検証済みの場合は、同じ session の検証状態を再利用する。
    fn verify_pin_once(&mut self, pin: &[u8]) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }

        self.yubikey.verify_pin(pin)?;
        self.pin_verified = true;
        Ok(())
    }

    /// 既定 management key で PIV management auth を実行する。
    ///
    /// 既定鍵運用のリスクは設計資料に明記し、任意 management key 対応は別設計にする。
    fn authenticate_management(&mut self) -> Result<()> {
        let key = MgmKey::get_default(&self.yubikey)?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    /// PIV metadata から secret storage key の public key を取得する。
    ///
    /// private key material は host へ出さない。
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

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object_id.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.authenticate_management()?;
        self.yubikey.save_object(object_id.value(), value)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        let public = self.public_key()?;
        Ok(public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), key)?)
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        self.verify_pin_once(pin)
    }

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        let decrypted = Zeroizing::new(piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?);
        let mut output = Zeroizing::new(Vec::new());
        write_oaep_unpadded_sha256(&decrypted, 256, &mut *output)?;
        Ok(output)
    }
}

/// 2 つの `yubikey::Version` を semantic version 順で比較する。
///
/// `yubikey::Version` に ordering がないため、PIV metadata 要件は tuple 比較で判定する。
fn version_lt(left: Version, right: Version) -> bool {
    (left.major, left.minor, left.patch) < (right.major, right.minor, right.patch)
}

/// PIV application version を dotted 表記の文字列へ変換する。
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
