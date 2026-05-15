//! `dotfiles secrets` が YubiKey bootstrap secret を登録、取得、検証する処理。
//!
//! secret 本文は引数やログに出さず、prompt、stdin、YubiKey PIV operation の間だけ
//! zeroize 可能な buffer に保持する。

mod storage;

use std::io::{self, IsTerminal, Read, Write};

use anyhow::{Context, bail};
use clap::{Args, Subcommand, ValueEnum};
use rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use secrecy::ExposeSecret;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use storage::{BootstrapSecrets, CheckName, CheckStatus, SecretDevice, SecretName, YubikeyRole};
use yubikey::{
    MgmKey, PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);

#[derive(Args)]
/// GPG、pass、Bitwarden 復旧に必要な秘密情報を扱う。
pub(crate) struct SecretsOptions {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
/// `dotfiles secrets` 配下の操作種別。
enum SecretsCommand {
    Yubikey(YubikeyOptions),
    VerifyYubikey(VerifyYubikeyOptions),
}

#[derive(Args)]
/// YubiKey PIV 領域に bootstrap secret を保存、取得、検証する。
struct YubikeyOptions {
    #[command(subcommand)]
    command: YubikeyCommand,
}

#[derive(Subcommand)]
/// YubiKey PIV storage の高水準 command と低水準 command。
enum YubikeyCommand {
    Setup(SerialOptions),
    Put(PutOptions),
    Get(GetOptions),
    EnrollPrimary(EnrollPrimaryOptions),
    EnrollSpare(EnrollSpareOptions),
    RotateBwsToken(SerialOptions),
}

#[derive(Args)]
/// 単一 YubiKey を serial で指定する option。
struct SerialOptions {
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// 1 secret を指定した YubiKey に保存する低水準 command の option。
struct PutOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
/// 1 secret を指定した YubiKey から取得する低水準 command の option。
struct GetOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// primary YubiKey に bootstrap secret 一式を初期登録する option。
struct EnrollPrimaryOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    stdin_json: bool,
}

#[derive(Args)]
/// spare YubiKey に primary 由来の bootstrap secret 一式を登録する option。
struct EnrollSpareOptions {
    #[arg(long)]
    primary_serial: Option<u32>,
    #[arg(long)]
    spare_serial: Option<u32>,
    #[arg(long)]
    stdin_json: bool,
}

#[derive(Args)]
/// YubiKey に保存された secret と外部確認項目を検証する option。
struct VerifyYubikeyOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long, value_enum)]
    check: Vec<VerifyCheck>,
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
/// `verify-yubikey --check` で追加する外部確認項目。
enum VerifyCheck {
    Bws,
    BwLogin,
}

#[derive(Deserialize)]
/// `--stdin-json` で受け取る bootstrap secret 入力。
struct BootstrapSecretsJson {
    #[serde(rename = "bw-email")]
    bw_email: String,
    #[serde(rename = "bw-password")]
    bw_password: String,
    #[serde(rename = "bws-access-token")]
    bws_access_token: String,
}

/// CLI で parse 済みの `dotfiles secrets` command を実行する。
pub(crate) fn run(options: SecretsOptions) -> Result<()> {
    match options.command {
        SecretsCommand::Yubikey(options) => run_yubikey(options),
        SecretsCommand::VerifyYubikey(options) => run_verify_yubikey(options),
    }
}

/// `dotfiles secrets yubikey` 配下の command を実行する。
///
/// 低水準 command は単一 secret または storage setup だけを扱い、高水準 command は
/// primary / spare 登録と local verify までを一連の操作として扱う。
fn run_yubikey(options: YubikeyOptions) -> Result<()> {
    match options.command {
        YubikeyCommand::Setup(options) => {
            let mut device = open_device(options.serial)?;
            storage::setup(&mut device)
        }
        YubikeyCommand::Put(options) => {
            let mut device = open_device(options.serial)?;
            let secret = read_secret_for_put(options.name, options.stdin)?;
            storage::put(&mut device, options.name, &secret, options.force)
        }
        YubikeyCommand::Get(options) => {
            let mut device = open_device(options.serial)?;
            let secret = storage::get(&mut device, options.name)?;
            write_secret_to_stdout(&secret)?;
            Ok(())
        }
        YubikeyCommand::EnrollPrimary(options) => {
            let mut device = open_device(options.serial)?;
            let secrets = read_bootstrap_secrets(options.stdin_json, None)?;
            let summary = storage::enroll(&mut device, YubikeyRole::Primary, &secrets)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare(options),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token(options),
    }
}

/// primary から secret 一式を読み出し、spare に再暗号化して登録する。
///
/// 通常実行では primary YubiKey から 3 secret を復号し終えた後に spare 選択へ進む。
/// `--stdin-json` は primary が使えない migration / recovery 用で、この場合だけ
/// stdin の secret 一式を正本として扱う。
fn run_enroll_spare(options: EnrollSpareOptions) -> Result<()> {
    let mut memory = SecretMemoryGuard::prepare()?;
    let (bootstrap, primary_serial) = if options.stdin_json {
        if options.spare_serial.is_none() && !io::stdin().is_terminal() {
            bail!("pass --spare-serial in non-interactive use");
        }
        (
            read_bootstrap_secrets(true, Some(&mut memory))?,
            options.primary_serial,
        )
    } else {
        let mut primary = open_device(options.primary_serial)?;
        let primary_serial = primary.serial();
        let secrets = BootstrapSecrets {
            bw_email: memory.lock_secret(
                SecretName::BwEmail,
                protect_zeroizing_secret(storage::get(&mut primary, SecretName::BwEmail)?),
            )?,
            bw_password: memory.lock_secret(
                SecretName::BwPassword,
                protect_zeroizing_secret(storage::get(&mut primary, SecretName::BwPassword)?),
            )?,
            bws_access_token: memory.lock_secret(
                SecretName::BwsAccessToken,
                protect_zeroizing_secret(storage::get(&mut primary, SecretName::BwsAccessToken)?),
            )?,
        };
        (secrets, Some(primary_serial))
    };

    let mut spare = open_spare_device(options.spare_serial, primary_serial)?;

    let summary = storage::enroll(&mut spare, YubikeyRole::Spare, &bootstrap)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// BWS token を 1 本または対話で選んだ複数本の YubiKey に保存する。
///
/// 非対話実行では `--serial` で 1 本だけを更新する。対話実行で serial 指定がない場合は
/// token を一度だけ受け取り、利用者が終了を選ぶまで YubiKey 選択と更新を繰り返す。
fn run_rotate_bws_token(options: SerialOptions) -> Result<()> {
    if let Some(serial) = options.serial {
        let mut device = open_device(Some(serial))?;
        let token = read_hidden("bws-access-token: ")?;
        let summary = storage::rotate_bws_token(&mut device, &token)?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let mut device = open_device(None)?;
    let token = read_hidden("bws-access-token: ")?;
    let mut summaries = vec![storage::rotate_bws_token(&mut device, &token)?];

    while prompt_yes_no("Update another YubiKey? [y/N] ")? {
        let mut device = open_device(None)?;
        summaries.push(storage::rotate_bws_token(&mut device, &token)?);
    }

    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

/// local storage の復号確認を行い、指定された外部確認項目を summary に含める。
///
/// 現時点の外部確認は placeholder として `skipped` を返し、YubiKey 上の bootstrap
/// secret が復号できることだけを実際に検証する。
fn run_verify_yubikey(options: VerifyYubikeyOptions) -> Result<()> {
    if options.all && !options.check.is_empty() {
        bail!("--all and --check cannot be used together");
    }

    let mut device = open_device(options.serial)?;
    let mut summary = storage::verify_local_storage(&mut device)?;

    for check in options.check {
        let key = match check {
            VerifyCheck::Bws => CheckName::Bws,
            VerifyCheck::BwLogin => CheckName::BwLogin,
        };
        summary.checks.insert(key, CheckStatus::Skipped);
    }
    if options.all {
        summary.checks.insert(CheckName::Bws, CheckStatus::Skipped);
        summary
            .checks
            .insert(CheckName::BwLogin, CheckStatus::Skipped);
    }

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// `put` 用の secret を prompt または stdin から読み取る。
///
/// CLI 引数では secret 本文を受け取らない。stdin では末尾改行を 1 つだけ除去し、
/// それ以外の bytes は保存対象として保持する。
fn read_secret_for_put(name: SecretName, stdin: bool) -> Result<Zeroizing<Vec<u8>>> {
    if stdin {
        read_one_stdin_secret()
    } else {
        read_hidden(&format!("{}: ", secret_name(name)))
    }
}

/// CLI 引数の secret 名を storage model の closed set に変換する。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

/// secret 名を CLI 表示用の kebab-case 文字列に変換する。
fn secret_name(name: SecretName) -> String {
    serde_json::to_value(name)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| format!("{name:?}"))
}

/// bootstrap secret 一式を prompt または JSON stdin から読み取る。
///
/// prompt では email だけを表示入力にし、password と BWS token は hidden prompt で
/// 受け取る。`--stdin-json` は migration / recovery 用の非対話入口である。
fn read_bootstrap_secrets(
    stdin_json: bool,
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    if stdin_json {
        let mut input = Zeroizing::new(String::new());
        io::stdin().read_to_string(&mut input)?;
        let parsed: BootstrapSecretsJson = serde_json::from_str(&input)?;
        input.zeroize();
        let secrets = BootstrapSecrets {
            bw_email: storage::secret_bytes(parsed.bw_email.into_bytes()),
            bw_password: storage::secret_bytes(parsed.bw_password.into_bytes()),
            bws_access_token: storage::secret_bytes(parsed.bws_access_token.into_bytes()),
        };
        return lock_bootstrap_secrets(secrets, memory);
    }

    let mut email = Zeroizing::new(String::new());
    eprint!("bw-email: ");
    io::stderr().flush()?;
    io::stdin().read_line(&mut email)?;
    let mut email = std::mem::take(&mut *email).into_bytes();
    trim_one_trailing_newline(&mut email);

    let secrets = BootstrapSecrets {
        bw_email: storage::secret_bytes(email),
        bw_password: protect_zeroizing_secret(read_hidden("bw-password: ")?),
        bws_access_token: protect_zeroizing_secret(read_hidden("bws-access-token: ")?),
    };
    lock_bootstrap_secrets(secrets, memory)
}

/// 必要に応じて bootstrap secret buffer を memory lock 対象に登録する。
fn lock_bootstrap_secrets(
    secrets: BootstrapSecrets,
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    if let Some(memory) = memory {
        return memory.lock_bootstrap(secrets);
    }

    Ok(secrets)
}

/// `Zeroizing<Vec<u8>>` から secret wrapper へ所有権を移す。
fn protect_zeroizing_secret(mut secret: Zeroizing<Vec<u8>>) -> storage::SecretBytes {
    storage::secret_bytes(std::mem::take(&mut *secret))
}

/// 端末に表示しない prompt から 1 secret を読み取る。
fn read_hidden(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// stdin 全体を 1 secret として読み取り、末尾改行だけを正規化する。
fn read_one_stdin_secret() -> Result<Zeroizing<Vec<u8>>> {
    let mut input = Zeroizing::new(Vec::default());
    io::stdin().read_to_end(&mut input)?;
    trim_one_trailing_newline(&mut input);
    Ok(input)
}

/// stdin 由来の secret から terminal 入力で混入しやすい末尾改行を 1 つだけ除く。
fn trim_one_trailing_newline(input: &mut Vec<u8>) {
    if input.ends_with(b"\n") {
        input.pop();
        if input.ends_with(b"\r") {
            input.pop();
        }
    }
}

/// spare 登録で平文 secret を読む前に必要な process / memory 保護を準備する。
///
/// core dump を無効化し、`mlock` 相当の memory lock が使えることを probe する。
/// secret buffer は読み込み直後に `lock` で登録し、guard の drop 時に unlock する。
struct SecretMemoryGuard {
    bw_email: Option<region::LockGuard>,
    bw_password: Option<region::LockGuard>,
    bws_access_token: Option<region::LockGuard>,
}

impl SecretMemoryGuard {
    /// secret を読む前に core dump 無効化と memory lock の利用可否を確認する。
    fn prepare() -> Result<Self> {
        rlimit::setrlimit(rlimit::Resource::CORE, 0, 0)
            .context("failed to disable core dumps before reading bootstrap secrets")?;

        let probe = [0u8; 1];
        let probe_guard = region::lock(probe.as_ptr(), probe.len())
            .context("failed to lock memory before reading bootstrap secrets")?;
        drop(probe_guard);

        Ok(Self {
            bw_email: None,
            bw_password: None,
            bws_access_token: None,
        })
    }

    /// bootstrap secret 一式を memory lock 対象に登録する。
    fn lock_bootstrap(&mut self, secrets: BootstrapSecrets) -> Result<BootstrapSecrets> {
        Ok(BootstrapSecrets {
            bw_email: self.lock_secret(SecretName::BwEmail, secrets.bw_email)?,
            bw_password: self.lock_secret(SecretName::BwPassword, secrets.bw_password)?,
            bws_access_token: self
                .lock_secret(SecretName::BwsAccessToken, secrets.bws_access_token)?,
        })
    }

    /// 1 secret を受け取った直後に memory lock 対象へ入れる。
    fn lock_secret(
        &mut self,
        name: SecretName,
        secret: storage::SecretBytes,
    ) -> Result<storage::SecretBytes> {
        let guard = lock_secret_memory(&secret)?;
        match name {
            SecretName::BwEmail => self.bw_email = guard,
            SecretName::BwPassword => self.bw_password = guard,
            SecretName::BwsAccessToken => self.bws_access_token = guard,
        }
        Ok(secret)
    }
}

/// 空でない secret buffer を memory lock する。
fn lock_secret_memory(secret: &storage::SecretBytes) -> Result<Option<region::LockGuard>> {
    let secret = secret.expose_secret();
    if secret.is_empty() {
        return Ok(None);
    }

    region::lock(secret.as_ptr(), secret.len())
        .map(Some)
        .context("failed to lock bootstrap secret memory")
}

/// serial 指定または対話選択で 1 本の YubiKey を開く。
///
/// 非対話実行では曖昧な選択を避けるため serial 指定を必須にする。
fn open_device(serial: Option<u32>) -> Result<YubikeySecretDevice> {
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

/// `enroll-spare` で primary の 3 secret を読み終えた後に spare を開く。
///
/// `--spare-serial` があればその YubiKey を直接開く。対話実行で serial 指定がなければ、
/// primary を抜いて spare を挿すための Enter 待ちを挟む。非対話実行では差し替え
/// prompt を出せないため、`--spare-serial` を必須にする。
fn open_spare_device(
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
) -> Result<YubikeySecretDevice> {
    if spare_serial.is_none() && !io::stdin().is_terminal() {
        bail!("pass --spare-serial in non-interactive use");
    }

    if let Some(spare_serial) = spare_serial {
        let device = open_device(Some(spare_serial))?;
        ensure_spare_serial(&device, primary_serial)?;
        return Ok(device);
    }

    loop {
        if primary_serial.is_some() {
            eprintln!("Insert the spare YubiKey, then press Enter.");
            wait_for_enter()?;
        }

        let device = open_device(None)?;
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

/// YubiKey 差し替え prompt の Enter 入力を待つ。
///
/// stdin が terminal でない場合は prompt による同期ができないため失敗させる。
fn wait_for_enter() -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("pass --spare-serial in non-interactive use");
    }

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(())
}

/// 対話 command の継続確認を読む。
fn prompt_yes_no(prompt: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

/// 低水準 `get` command の唯一の出力として secret bytes を標準出力へ渡す。
fn write_secret_to_stdout(secret: &[u8]) -> Result<()> {
    let mut input = secret;
    io::copy(&mut input, &mut io::stdout())?;
    Ok(())
}

/// 接続中の YubiKey を対話的に 1 本選択する。
///
/// 1 本だけ検出された場合はそのまま選び、複数本ある場合は reader 名と serial を
/// 表示して番号入力を求める。
fn select_interactive_yubikey() -> Result<YubiKey> {
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

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
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

/// 実機 YubiKey を `storage::SecretDevice` に接続する adapter。
///
/// PIV PIN verification は 1 command 内で 1 回だけ行い、management key authentication
/// は setup / object write の直前に閉じ込める。
struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
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
    /// 実機 YubiKey の serial を返す。
    fn serial(&self) -> u32 {
        self.yubikey.serial().0
    }

    /// secret storage 用 PIV slot に鍵 metadata が存在するか確認する。
    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(_) => Ok(true),
            Err(yubikey::Error::NotFound) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// secret storage 用 PIV slot に non-exportable RSA2048 鍵を生成する。
    fn generate_key(&mut self) -> Result<()> {
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

    /// 指定 PIV data object を読み取る。
    fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match self.yubikey.fetch_object(object_id) {
            Ok(value) => Ok(Some(value)),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// 指定 PIV data object に manifest または encrypted blob を保存する。
    fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()> {
        self.authenticate_management()?;
        let mut value = value.to_vec();
        self.yubikey.save_object(object_id, &mut value)?;
        Ok(())
    }

    /// host 側で生成した content encryption key を YubiKey public key で wrap する。
    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let public = self.public_key()?;
        let wrapped = public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), key)?;
        Ok(Zeroizing::new(wrapped))
    }

    /// YubiKey private key operation で content encryption key を unwrap する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.verify_pin_once()?;
        let decrypted = piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?;
        oaep_unpad_sha256(&decrypted, 256)
    }
}

/// YubiKey の raw RSA decrypt 結果から RSA-OAEP SHA-256 padding を検証して外す。
fn oaep_unpad_sha256(encoded: &[u8], key_len: usize) -> Result<Zeroizing<Vec<u8>>> {
    let hash_len = 32;
    if encoded.len() != key_len || key_len < 2 * hash_len + 2 {
        bail!("invalid RSA-OAEP encoded message length");
    }
    if encoded[0] != 0 {
        bail!("invalid RSA-OAEP leading byte");
    }

    let (masked_seed, masked_db) = encoded[1..].split_at(hash_len);
    let seed_mask = mgf1_sha256(masked_db, hash_len);
    let seed = Zeroizing::new(
        masked_seed
            .iter()
            .zip(seed_mask)
            .map(|(left, right)| left ^ right)
            .collect::<Vec<u8>>(),
    );
    let db_mask = mgf1_sha256(&seed, key_len - hash_len - 1);
    let db = Zeroizing::new(
        masked_db
            .iter()
            .zip(db_mask)
            .map(|(left, right)| left ^ right)
            .collect::<Vec<u8>>(),
    );

    let label_hash = Sha256::digest([]);
    if db.get(..hash_len) != Some(label_hash.as_slice()) {
        bail!("invalid RSA-OAEP label hash");
    }

    let rest = &db[hash_len..];
    let separator = rest
        .iter()
        .position(|byte| *byte == 1)
        .context("invalid RSA-OAEP separator")?;
    if rest[..separator].iter().any(|byte| *byte != 0) {
        bail!("invalid RSA-OAEP padding string");
    }

    Ok(Zeroizing::new(rest[separator + 1..].to_vec()))
}

/// RSA-OAEP SHA-256 で使う MGF1 mask を生成する。
fn mgf1_sha256(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut counter = 0u32;
    while out.len() < len {
        let mut digest = Sha256::new();
        digest.update(seed);
        digest.update(counter.to_be_bytes());
        out.extend_from_slice(&digest.finalize());
        counter += 1;
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_one_trailing_newline() {
        let mut value = b"secret\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret");

        let mut value = b"secret\n\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret\n");
    }

    #[test]
    fn oaep_unpad_round_trips_rsa_oaep_sha256() -> Result<()> {
        let message = b"test-content-encryption-key";
        let encoded = oaep_pad_sha256_for_test(message, 256);
        let decoded = oaep_unpad_sha256(&encoded, 256)?;
        assert_eq!(decoded.as_slice(), message);
        Ok(())
    }

    #[test]
    fn oaep_unpad_rejects_invalid_padding() {
        let encoded: Vec<u8> = std::iter::once(1)
            .chain(std::iter::repeat_n(0u8, 255))
            .collect();
        assert!(oaep_unpad_sha256(&encoded, 256).is_err());
    }

    fn oaep_pad_sha256_for_test(message: &[u8], key_len: usize) -> Vec<u8> {
        let hash_len = 32usize;
        let ps_len = key_len - message.len() - (2 * hash_len) - 2;
        let label_hash = Sha256::digest([]);

        let db: Vec<u8> = label_hash
            .as_slice()
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0u8, ps_len))
            .chain(std::iter::once(1))
            .chain(message.iter().copied())
            .collect();

        let seed = [0x42u8; 32];
        let db_mask = mgf1_sha256(&seed, key_len - hash_len - 1);
        let masked_db: Vec<u8> = db
            .iter()
            .zip(db_mask)
            .map(|(left, right)| left ^ right)
            .collect();

        let seed_mask = mgf1_sha256(&masked_db, hash_len);
        let masked_seed: Vec<u8> = seed
            .iter()
            .zip(seed_mask)
            .map(|(left, right)| left ^ right)
            .collect();

        std::iter::once(0)
            .chain(masked_seed)
            .chain(masked_db)
            .collect()
    }
}
