//! 実機 YubiKey を secret storage の device trait に接続する adapter。
//!
//! PIV PIN verification は 1 command 内で 1 回だけ行い、management key authentication
//! は setup / object write の直前に閉じ込める。

use std::{
    io::{self, IsTerminal, Write},
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
    input::{read_hidden, read_terminal_line_until, wait_for_enter},
    memory::InterruptGuard,
    oaep::oaep_unpad_sha256,
    storage::SecretDevice,
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

/// 1 command 内で共有する実機 YubiKey transaction state。
pub(crate) struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

/// serial 指定または対話選択で 1 本の YubiKey を開く。
///
/// 非対話実行では secret 読み込み前に対象 device を確定するため、serial 指定を必須にする。
pub(crate) fn open_device(serial: Option<u32>) -> Result<YubikeySecretDevice> {
    if serial.is_none() && !io::stdin().is_terminal() {
        bail!("pass --serial in non-interactive use");
    }

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
    if serial.is_none() && !io::stdin().is_terminal() {
        bail!("pass --serial in non-interactive use");
    }

    if interrupt.interrupted() {
        bail!("interrupted while handling bootstrap secrets");
    }
    let yubikey = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        select_interactive_yubikey_until(deadline, interrupt)?
    };
    if interrupt.interrupted() {
        bail!("interrupted while handling bootstrap secrets");
    }

    Ok(YubikeySecretDevice {
        yubikey,
        pin_verified: false,
    })
}

/// `enroll-spare` で primary の 3 secret を読み終えた後に spare を開く。
///
/// `--spare-serial` があればその YubiKey を直接開く。対話実行で serial 指定がなければ、
/// primary を抜いて spare を挿すための Enter 待ちを挟む。非対話実行では差し替え
/// prompt を出せないため、`--spare-serial` を必須にする。
pub(crate) fn open_spare_device(
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    if spare_serial.is_none() && !io::stdin().is_terminal() {
        bail!("pass --spare-serial in non-interactive use");
    }

    if let Some(spare_serial) = spare_serial {
        let device = open_device(Some(spare_serial))?;
        ensure_spare_serial(&device, primary_serial)?;
        return Ok(device);
    }

    let deadline = Instant::now() + SPARE_WAIT_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            bail!("timed out waiting for spare YubiKey");
        }

        if primary_serial.is_some() {
            eprintln!("Insert the spare YubiKey, then press Enter.");
            wait_for_enter(deadline, interrupt)?;
        }

        let device = open_device_until(None, deadline, interrupt)?;
        if ensure_spare_serial(&device, primary_serial).is_ok() {
            return Ok(device);
        }

        eprintln!("The selected YubiKey is the primary; replace it with the spare.");
    }
}

/// spare として開いた YubiKey が primary と同一 serial でないことを確認する。
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
    select_interactive_yubikey_with_input(None)
}

fn select_interactive_yubikey_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<YubiKey> {
    select_interactive_yubikey_with_input(Some((deadline, interrupt)))
}

fn select_interactive_yubikey_with_input(
    timed_input: Option<(Instant, &InterruptGuard)>,
) -> Result<YubiKey> {
    let mut context = yubikey::Context::open()?;
    let keys: Vec<_> = context
        .iter()?
        .filter_map(|reader| {
            let name = reader.name().into_owned();
            reader.open().ok().map(|yubikey| (name, yubikey))
        })
        .collect();

    match keys.as_slice() {
        [] => bail!("no YubiKey detected"),
        [_] => {
            let (_, yubikey) = keys
                .into_iter()
                .next()
                .context("single selected YubiKey disappeared")?;
            Ok(yubikey)
        }
        [_, ..] => {
            if !io::stdin().is_terminal() {
                bail!("multiple YubiKeys detected; pass a serial option in non-interactive use");
            }

            eprintln!("Select YubiKey:");
            for (index, (reader, yubikey)) in keys.iter().enumerate() {
                eprintln!("{}: serial {} ({reader})", index + 1, yubikey.serial());
            }
            eprint!("number: ");
            io::stderr().flush()?;

            let input = if let Some((deadline, interrupt)) = timed_input {
                read_terminal_line_until(deadline, interrupt)?
            } else {
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input
            };
            let selected = input.trim().parse::<usize>().context("invalid selection")?;
            if selected == 0 || selected > keys.len() {
                bail!("selected YubiKey is out of range");
            }
            let (_, yubikey) = keys
                .into_iter()
                .nth(selected - 1)
                .context("selected YubiKey disappeared")?;
            Ok(yubikey)
        }
    }
}

impl YubikeySecretDevice {
    /// PIV private key operation に必要な PIN verification を遅延実行する。
    fn verify_pin_once(&mut self) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }

        let pin = read_hidden("YubiKey PIN: ")?;
        self.yubikey.verify_pin(&pin)?;
        self.pin_verified = true;
        Ok(())
    }

    /// YubiKey の既定 management key で PIV 書き込み操作を認証する。
    fn authenticate_management(&mut self) -> Result<()> {
        let key = MgmKey::get_default(&self.yubikey)?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    /// secret storage 用 slot に生成済みの RSA public key を取得する。
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

    fn generate_key(&mut self) -> Result<()> {
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

    fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match self.yubikey.fetch_object(object_id) {
            Ok(value) => Ok(Some(value)),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()> {
        self.authenticate_management()?;
        let mut value = Zeroizing::new(value.to_vec());
        self.yubikey.save_object(object_id, &mut value)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let public = self.public_key()?;
        let wrapped = public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), key)?;
        Ok(Zeroizing::new(wrapped))
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.verify_pin_once()?;
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

fn version_lt(left: Version, right: Version) -> bool {
    (left.major, left.minor, left.patch) < (right.major, right.minor, right.patch)
}

fn format_version(version: Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}
