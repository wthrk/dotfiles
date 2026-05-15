//! YubiKey に保存する bootstrap secret の名前、manifest、blob 形式を定義する。
//!
//! PIV data object は読み出し可能な領域なので、平文は置かず、YubiKey の非 export 鍵で
//! wrap した content key と AEAD ciphertext だけを保存する。

use std::collections::BTreeMap;

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadInPlace, Payload},
};
use anyhow::{Context, bail};
use nom::{
    Parser,
    bytes::complete::{tag, take},
    combinator::{all_consuming, map_res, verify},
    number::complete::{be_u8, be_u16, be_u32},
};
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
const BLOB_VERSION: u8 = 1;
const ALGORITHM_AES_256_GCM: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const CONTENT_KEY_LEN: usize = 32;

/// bootstrap secret 本文を明示的な expose が必要な型で保持する。
pub type SecretBytes = SecretBox<Vec<u8>>;

/// manifest を保存する PIV data object ID。
pub const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
/// manifest が dotfiles secret recovery 用であることを示す app id。
pub const MANIFEST_APP: &str = "dotfiles.secret-recovery";
/// bootstrap secret storage 専用の retired key management slot。
pub const KEY_SLOT: &str = "82";

/// YubiKey bootstrap に保存できる secret 名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, EnumIter)]
#[serde(rename_all = "kebab-case")]
pub enum SecretName {
    /// Bitwarden login email。
    BwEmail,
    /// Bitwarden login password。
    BwPassword,
    /// Bitwarden Secrets Manager access token。
    BwsAccessToken,
}

impl SecretName {
    /// bootstrap recovery で扱う secret 名を enum 定義順で列挙する。
    pub fn iter() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }

    /// binary blob に保存する固定 secret id。
    pub fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    /// secret ごとに割り当てた PIV data object ID。
    pub fn object_id(self) -> u32 {
        match self {
            Self::BwEmail => 0x005f_ff17,
            Self::BwPassword => 0x005f_ff18,
            Self::BwsAccessToken => 0x005f_ff19,
        }
    }

    /// binary blob の secret id を型付き secret 名へ戻す。
    pub fn from_secret_id(secret_id: u8) -> Result<Self> {
        match secret_id {
            1 => Ok(Self::BwEmail),
            2 => Ok(Self::BwPassword),
            3 => Ok(Self::BwsAccessToken),
            _ => bail!("unknown YubiKey secret id: {secret_id}"),
        }
    }
}

/// PIV data object に保存する manifest。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretManifest {
    /// manifest format version。
    pub version: u8,
    /// この repository の secret storage manifest であることを示す app id。
    pub app: String,
}

impl SecretManifest {
    /// この repository が認識する manifest sentinel。
    pub fn expected() -> Self {
        Self {
            version: 1,
            app: MANIFEST_APP.to_owned(),
        }
    }

    /// 読み出した manifest が現在の storage format と一致することを確認する。
    pub fn validate_expected(&self) -> Result<()> {
        if self != &Self::expected() {
            bail!("YubiKey secret manifest does not match dotfiles secret-recovery format");
        }

        Ok(())
    }
}

/// PIV object に保存する暗号化済み secret blob。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretBlob {
    /// blob に入っている secret 名。
    pub name: SecretName,
    /// AES-256-GCM nonce。
    pub nonce: [u8; NONCE_LEN],
    /// YubiKey public key で wrap した content encryption key。
    pub wrapped_key: Zeroizing<Vec<u8>>,
    /// secret 本文の ciphertext。
    pub ciphertext: Zeroizing<Vec<u8>>,
    /// AES-256-GCM authentication tag。
    pub tag: [u8; TAG_LEN],
}

/// 実機 YubiKey と fake test double に共通する最小操作。
pub trait SecretDevice {
    /// device 固有の serial。AEAD additional data にも含める。
    fn serial(&self) -> u32;
    /// secret storage 用 PIV key が存在するか確認する。
    fn key_exists(&mut self) -> Result<bool>;
    /// secret storage 用 PIV key を device 内で生成する。
    fn generate_key(&mut self) -> Result<()>;
    /// PIV data object を読み取る。存在しない場合は `None` を返す。
    fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>>;
    /// PIV data object に bytes を保存する。
    fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()>;
    /// content encryption key を device の public key で wrap する。
    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
    /// wrapped content encryption key を device の private key operation で unwrap する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckStatus {
    /// 確認に成功した状態。
    #[serde(rename = "ok")]
    Ok,
    /// 現在の command では実行しない確認項目。
    #[serde(rename = "skipped")]
    Skipped,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnrollSummary {
    /// 登録対象 YubiKey の serial。
    pub serial: u32,
    /// 登録した YubiKey の role。
    pub role: YubikeyRole,
    /// 登録中に完了した確認項目。
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum YubikeyRole {
    /// 正本 secret を最初に登録する primary YubiKey。
    Primary,
    /// primary から再暗号化した secret を持つ spare YubiKey。
    Spare,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifySummary {
    /// 検証対象 YubiKey の serial。
    pub serial: u32,
    /// 実行または省略した確認項目。
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckName {
    /// PIV key と manifest の初期作成。
    Setup,
    /// `bw-email` の保存または復号確認。
    BwEmail,
    /// `bw-password` の保存または復号確認。
    BwPassword,
    /// `bws-access-token` の保存または復号確認。
    BwsAccessToken,
    /// YubiKey local storage 上の 3 secret 復号確認。
    LocalStorage,
    /// Bitwarden Secrets Manager への接続確認。
    Bws,
    /// Bitwarden login secret の妥当性確認。
    BwLogin,
}

/// primary / spare 登録で保存する bootstrap secret 一式。
pub struct BootstrapSecrets {
    /// Bitwarden login email bytes。
    pub bw_email: SecretBytes,
    /// Bitwarden login password bytes。
    pub bw_password: SecretBytes,
    /// Bitwarden Secrets Manager access token bytes。
    pub bws_access_token: SecretBytes,
}

impl BootstrapSecrets {
    /// secret 名に対応する平文 bytes を返す。
    pub fn get(&self, name: SecretName) -> &[u8] {
        match name {
            SecretName::BwEmail => self.bw_email.expose_secret(),
            SecretName::BwPassword => self.bw_password.expose_secret(),
            SecretName::BwsAccessToken => self.bws_access_token.expose_secret(),
        }
    }
}

/// raw bytes を bootstrap secret 用 wrapper に入れる。
pub fn secret_bytes(value: Vec<u8>) -> SecretBytes {
    SecretBox::new(Box::new(value))
}

/// secret storage 用 PIV key と manifest を新規作成する。
///
/// 既存 key または対象 object が存在する場合は、上書きせず停止する。
pub fn setup<D: SecretDevice>(device: &mut D) -> Result<()> {
    if device.key_exists()? {
        bail!("YubiKey PIV slot {KEY_SLOT} already contains a key");
    }

    for object_id in storage_object_ids() {
        if device.read_object(object_id)?.is_some() {
            bail!(
                "YubiKey PIV object {} already exists",
                format_object_id(object_id)
            );
        }
    }

    device.generate_key()?;
    write_manifest(device)
}

/// 1 secret を encrypted blob として YubiKey object に保存する。
///
/// manifest が期待値と一致することを確認し、既存 blob は `force` がない限り拒否する。
pub fn put<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
    force: bool,
) -> Result<()> {
    if secret.is_empty() {
        bail!("{} must not be empty", secret_name(name));
    }

    read_manifest(device)?.validate_expected()?;
    if device.read_object(name.object_id())?.is_some() && !force {
        bail!(
            "{} already exists; pass --force to replace it",
            secret_name(name)
        );
    }

    let blob = encrypt_secret(device, name, secret)?;
    let encoded = blob.encode()?;
    device.write_object(name.object_id(), &encoded)
}

/// 1 secret を YubiKey object から読み出して復号する。
///
/// blob 内の secret id と要求された secret 名が一致しない場合は拒否する。
pub fn get<D: SecretDevice>(device: &mut D, name: SecretName) -> Result<Zeroizing<Vec<u8>>> {
    read_manifest(device)?.validate_expected()?;
    let encoded = device
        .read_object(name.object_id())?
        .with_context(|| format!("{} is not stored on this YubiKey", secret_name(name)))?;
    let blob = SecretBlob::decode(&encoded)
        .with_context(|| format!("failed to decode {}", secret_name(name)))?;
    if blob.name != name {
        bail!(
            "YubiKey secret blob name does not match requested {}",
            secret_name(name)
        );
    }

    decrypt_secret(device, &blob)
}

/// primary または spare として bootstrap secret 一式を登録する。
///
/// setup、3 secret 保存、local verify をこの順に実行し、成功した確認項目だけを
/// summary に含める。
pub fn enroll<D: SecretDevice>(
    device: &mut D,
    role: YubikeyRole,
    secrets: &BootstrapSecrets,
) -> Result<EnrollSummary> {
    for name in SecretName::iter() {
        if secrets.get(name).is_empty() {
            bail!("{} must not be empty", secret_name(name));
        }
    }

    setup(device)?;
    for name in SecretName::iter() {
        put(device, name, secrets.get(name), false)?;
    }
    verify_local_storage(device)?;

    let checks = [
        (CheckName::Setup, CheckStatus::Ok),
        (CheckName::BwEmail, CheckStatus::Ok),
        (CheckName::BwPassword, CheckStatus::Ok),
        (CheckName::BwsAccessToken, CheckStatus::Ok),
        (CheckName::LocalStorage, CheckStatus::Ok),
    ]
    .into_iter()
    .collect();

    Ok(EnrollSummary {
        serial: device.serial(),
        role,
        checks,
    })
}

/// BWS access token だけを置き換え、local storage を再検証する。
pub fn rotate_bws_token<D: SecretDevice>(device: &mut D, token: &[u8]) -> Result<VerifySummary> {
    put(device, SecretName::BwsAccessToken, token, true)?;
    verify_local_storage(device)
}

/// YubiKey 上の manifest と 3 secret の復号可能性を検証する。
///
/// BWS / Bitwarden login の外部通信確認は別 issue の範囲なので、ここでは `skipped`
/// として summary に残す。
pub fn verify_local_storage<D: SecretDevice>(device: &mut D) -> Result<VerifySummary> {
    read_manifest(device)?.validate_expected()?;
    for name in SecretName::iter() {
        let secret = get(device, name)?;
        if secret.is_empty() {
            bail!("{} stored on this YubiKey is empty", secret_name(name));
        }
    }

    let checks = [
        (CheckName::LocalStorage, CheckStatus::Ok),
        (CheckName::Bws, CheckStatus::Skipped),
        (CheckName::BwLogin, CheckStatus::Skipped),
    ]
    .into_iter()
    .collect();

    Ok(VerifySummary {
        serial: device.serial(),
        checks,
    })
}

impl SecretBlob {
    /// secret blob を設計資料の binary wire format に encode する。
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        let wrapped_key_len = u16::try_from(self.wrapped_key.len())
            .context("wrapped YubiKey content key is too large")?;
        let ciphertext_len = u32::try_from(self.ciphertext.len())
            .context("YubiKey secret ciphertext is too large")?;

        Ok(Zeroizing::new(
            [
                BLOB_MAGIC,
                &[BLOB_VERSION, self.name.secret_id(), ALGORITHM_AES_256_GCM],
                &self.nonce,
                &wrapped_key_len.to_be_bytes(),
                &self.wrapped_key,
                &ciphertext_len.to_be_bytes(),
                &self.ciphertext,
                &self.tag,
            ]
            .concat(),
        ))
    }

    /// secret blob を設計資料の binary wire format から decode する。
    pub fn decode(input: &[u8]) -> Result<Self> {
        all_consuming(parse_secret_blob)
            .parse(input)
            .map(|(_, blob)| blob)
            .map_err(|err| anyhow::anyhow!("invalid YubiKey secret blob: {err}"))
    }
}

/// PIV data object ID を manifest 表示用の 8 桁 hex 文字列にする。
fn format_object_id(object_id: u32) -> String {
    format!("0x{object_id:08X}")
}

/// secret 名を manifest / error 表示用の kebab-case 文字列にする。
fn secret_name(name: SecretName) -> String {
    serde_json::to_value(name)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| format!("{name:?}"))
}

/// manifest と全 secret blob の object ID を列挙する。
fn storage_object_ids() -> impl Iterator<Item = u32> {
    std::iter::once(MANIFEST_OBJECT_ID).chain(SecretName::iter().map(SecretName::object_id))
}

/// 期待 manifest を PIV data object に保存する。
fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let manifest = serde_json::to_vec(&SecretManifest::expected())?;
    device.write_object(MANIFEST_OBJECT_ID, &manifest)
}

/// PIV data object から manifest を読み出して JSON として parse する。
fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(MANIFEST_OBJECT_ID)?
        .context("YubiKey secret manifest is missing")?;
    serde_json::from_slice(&manifest).context("failed to parse YubiKey secret manifest")
}

/// 平文 secret を AES-256-GCM で暗号化し、content key を YubiKey public key で wrap する。
fn encrypt_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
) -> Result<SecretBlob> {
    let content_key = Zeroizing::new(rand::random::<[u8; CONTENT_KEY_LEN]>());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = Aes256Gcm::new_from_slice(content_key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let ciphertext_and_tag = Zeroizing::new(
        cipher
            .encrypt(
                aes_gcm::Nonce::from_slice(&nonce),
                Payload {
                    msg: secret,
                    aad: &additional_data(device.serial(), name),
                },
            )
            .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?,
    );
    let tag_offset = ciphertext_and_tag
        .len()
        .checked_sub(TAG_LEN)
        .context("AES-256-GCM output is shorter than its tag")?;
    let (ciphertext, tag) = ciphertext_and_tag.split_at(tag_offset);
    let tag = tag
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;

    let wrapped_key = device.wrap_key(content_key.as_ref())?;

    Ok(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext: Zeroizing::new(ciphertext.to_vec()),
        tag,
    })
}

/// YubiKey private key operation で content key を unwrap し、secret blob を復号する。
fn decrypt_secret<D: SecretDevice>(
    device: &mut D,
    blob: &SecretBlob,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut content_key = device.unwrap_key(&blob.wrapped_key)?;
    if content_key.len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }

    let cipher = Aes256Gcm::new_from_slice(&content_key)
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let mut plaintext = blob.ciphertext.clone();
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&blob.nonce),
            &additional_data(device.serial(), blob.name),
            plaintext.as_mut(),
            aes_gcm::Tag::from_slice(&blob.tag),
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt {}", secret_name(blob.name)))?;
    content_key.zeroize();
    Ok(plaintext)
}

/// blob の入れ替えを検出するため AEAD additional data を構築する。
fn additional_data(serial: u32, name: SecretName) -> Vec<u8> {
    [
        &[BLOB_VERSION, name.secret_id()][..],
        &name.object_id().to_be_bytes(),
        &serial.to_be_bytes(),
    ]
    .concat()
}

/// `docs/secret-recovery/yubikey-secret-storage-design.md` の binary blob format:
/// magic, version, secret_id, algorithm, 12-byte nonce, u16be wrapped_key length,
/// wrapped_key bytes, u32be ciphertext length, ciphertext bytes, 16-byte tag.
fn parse_secret_blob(input: &[u8]) -> nom::IResult<&[u8], SecretBlob> {
    let (input, _) = tag(BLOB_MAGIC).parse(input)?;
    let (input, _) = verify(be_u8, |version| *version == BLOB_VERSION).parse(input)?;
    let (input, name) = map_res(be_u8, SecretName::from_secret_id).parse(input)?;
    let (input, _) = verify(be_u8, |algorithm| *algorithm == ALGORITHM_AES_256_GCM).parse(input)?;
    let (input, nonce) = fixed_bytes::<NONCE_LEN>.parse(input)?;
    let (input, wrapped_key_len) = be_u16(input)?;
    let (input, wrapped_key) = take(wrapped_key_len).parse(input)?;
    let (input, ciphertext_len) = be_u32(input)?;
    let (input, ciphertext) = take(ciphertext_len).parse(input)?;
    let (input, tag) = fixed_bytes::<TAG_LEN>.parse(input)?;

    Ok((
        input,
        SecretBlob {
            name,
            nonce,
            wrapped_key: Zeroizing::new(wrapped_key.to_vec()),
            ciphertext: Zeroizing::new(ciphertext.to_vec()),
            tag,
        },
    ))
}

/// `nom` parser で固定長 byte 配列を読み取る。
fn fixed_bytes<const N: usize>(input: &[u8]) -> nom::IResult<&[u8], [u8; N]> {
    map_res(take(N), <[u8; N]>::try_from).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct FakeDevice {
        serial: u32,
        key_exists: bool,
        objects: BTreeMap<u32, Zeroizing<Vec<u8>>>,
    }

    impl FakeDevice {
        fn new(serial: u32) -> Self {
            Self {
                serial,
                key_exists: false,
                objects: BTreeMap::new(),
            }
        }
    }

    impl SecretDevice for FakeDevice {
        fn serial(&self) -> u32 {
            self.serial
        }

        fn key_exists(&mut self) -> Result<bool> {
            Ok(self.key_exists)
        }

        fn generate_key(&mut self) -> Result<()> {
            self.key_exists = true;
            Ok(())
        }

        fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>> {
            Ok(self.objects.get(&object_id).cloned())
        }

        fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()> {
            self.objects
                .insert(object_id, Zeroizing::new(value.to_vec()));
            Ok(())
        }

        fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            Ok(Zeroizing::new(key.iter().map(|byte| byte ^ 0xa5).collect()))
        }

        fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            self.wrap_key(wrapped_key)
        }
    }

    #[test]
    fn secret_name_rejects_unknown_name() {
        let parsed = serde_json::from_value::<SecretName>(serde_json::json!("github-token"));
        assert!(parsed.is_err());
    }

    #[test]
    fn secret_names_match_design_object_mapping() {
        let objects: BTreeMap<_, _> = SecretName::iter()
            .map(|name| (secret_name(name), format_object_id(name.object_id())))
            .collect();

        assert_eq!(
            objects.get("bw-email").map(String::as_str),
            Some("0x005FFF17")
        );
        assert_eq!(
            objects.get("bw-password").map(String::as_str),
            Some("0x005FFF18")
        );
        assert_eq!(
            objects.get("bws-access-token").map(String::as_str),
            Some("0x005FFF19")
        );
    }

    #[test]
    fn manifest_is_format_sentinel_only() {
        let manifest = SecretManifest::expected();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.app, MANIFEST_APP);
        assert!(manifest.validate_expected().is_ok());
    }

    #[test]
    fn secret_blob_round_trips_binary_format() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwsAccessToken,
            nonce: [7; NONCE_LEN],
            wrapped_key: Zeroizing::new(vec![1, 2, 3]),
            ciphertext: Zeroizing::new(vec![4, 5, 6, 7]),
            tag: [9; TAG_LEN],
        };

        let encoded = blob.encode()?;
        let decoded = SecretBlob::decode(&encoded)?;

        assert_eq!(decoded, blob);
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_trailing_bytes() -> Result<()> {
        let blob = SecretBlob {
            name: SecretName::BwEmail,
            nonce: [1; NONCE_LEN],
            wrapped_key: Zeroizing::new(vec![2]),
            ciphertext: Zeroizing::new(vec![3]),
            tag: [4; TAG_LEN],
        };
        let encoded = Zeroizing::new(
            blob.encode()?
                .iter()
                .copied()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>(),
        );

        assert!(SecretBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn setup_stops_when_storage_object_exists() {
        let mut device = FakeDevice::new(1234);
        device
            .objects
            .insert(MANIFEST_OBJECT_ID, Zeroizing::new(b"occupied".to_vec()));

        assert!(setup(&mut device).is_err());
    }

    #[test]
    fn put_get_and_verify_round_trip_through_device() -> Result<()> {
        let mut device = FakeDevice::new(1234);
        setup(&mut device)?;

        put(&mut device, SecretName::BwEmail, b"user@example.com", false)?;
        put(&mut device, SecretName::BwPassword, b"password", false)?;
        put(&mut device, SecretName::BwsAccessToken, b"token", false)?;

        assert_eq!(
            get(&mut device, SecretName::BwEmail)?.as_slice(),
            b"user@example.com"
        );
        assert_eq!(
            verify_local_storage(&mut device)?
                .checks
                .get(&CheckName::LocalStorage),
            Some(&CheckStatus::Ok)
        );
        Ok(())
    }

    #[test]
    fn put_requires_force_for_existing_secret() -> Result<()> {
        let mut device = FakeDevice::new(1234);
        setup(&mut device)?;
        put(&mut device, SecretName::BwsAccessToken, b"old", false)?;

        assert!(put(&mut device, SecretName::BwsAccessToken, b"new", false).is_err());
        put(&mut device, SecretName::BwsAccessToken, b"new", true)?;
        assert_eq!(
            get(&mut device, SecretName::BwsAccessToken)?.as_slice(),
            b"new"
        );
        Ok(())
    }
}
