use std::fmt;

use anyhow::Result;

use super::piv::SecretName;

/// secret blob の先頭で dotfiles wire format を識別する magic bytes。
pub(crate) const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
/// 現在の binary blob format version。
pub(crate) const BLOB_VERSION: u8 = 1;
/// blob header に保存する AES-256-GCM algorithm id。
pub(crate) const ALGORITHM_AES_256_GCM: u8 = 1;
/// AES-GCM nonce の固定長。
pub const NONCE_LEN: usize = 12;
/// AES-GCM tag の固定長。
pub const TAG_LEN: usize = 16;
/// per-secret content encryption key の byte 長。
pub const CONTENT_KEY_LEN: usize = 32;

#[derive(Clone, PartialEq, Eq)]
/// YubiKey secret 1 件分の永続化単位を表す wire blob。
///
/// `SecretBlob` は暗号化済み payload と復号検証に必要な metadata を
/// domain で一体として扱うための型であり、平文 secret を保持しない。
pub struct SecretBlob {
    pub name: SecretName,
    pub nonce: [u8; NONCE_LEN],
    pub wrapped_key: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub tag: [u8; TAG_LEN],
}

impl fmt::Debug for SecretBlob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretBlob")
            .field("name", &self.name)
            .field(
                "nonce",
                &format_args!("<redacted:{} bytes>", self.nonce.len()),
            )
            .field(
                "wrapped_key",
                &format_args!("<redacted:{} bytes>", self.wrapped_key.len()),
            )
            .field(
                "ciphertext",
                &format_args!("<redacted:{} bytes>", self.ciphertext.len()),
            )
            .field("tag", &format_args!("<redacted:{} bytes>", self.tag.len()))
            .finish()
    }
}

impl SecretBlob {
    /// secret blob を binary wire format version 1 へ直列化する。
    ///
    /// version, secret id, algorithm id, nonce, wrapped-key, ciphertext, tag を固定順で格納する。
    /// 呼び出し側は返却 bytes を同 version の `decode` にのみ渡す責務を負う。
    pub fn encode(&self) -> Result<Vec<u8>> {
        let wrapped_key_len = u16::try_from(self.wrapped_key.len()).map_err(|error| {
            invalid_data(format!("wrapped YubiKey content key is too large: {error}"))
        })?;
        let ciphertext_len = u32::try_from(self.ciphertext.len()).map_err(|error| {
            invalid_data(format!("YubiKey secret ciphertext is too large: {error}"))
        })?;

        let total_len = BLOB_MAGIC.len()
            + 3
            + self.nonce.len()
            + 2
            + self.wrapped_key.len()
            + 4
            + self.ciphertext.len()
            + self.tag.len();
        let mut encoded = Vec::with_capacity(total_len);
        encoded.extend_from_slice(BLOB_MAGIC);
        encoded.extend_from_slice(&[BLOB_VERSION, self.name.secret_id(), ALGORITHM_AES_256_GCM]);
        encoded.extend_from_slice(&self.nonce);
        encoded.extend_from_slice(&wrapped_key_len.to_be_bytes());
        encoded.extend_from_slice(&self.wrapped_key);
        encoded.extend_from_slice(&ciphertext_len.to_be_bytes());
        encoded.extend_from_slice(&self.ciphertext);
        encoded.extend_from_slice(&self.tag);
        Ok(encoded)
    }

    /// binary wire format version 1 から secret blob を復元する。
    ///
    /// magic/version/algorithm の整合、各長さフィールド、末尾一致を検証し、
    /// どれか 1 つでも崩れていれば失敗する。
    pub fn decode(input: &[u8]) -> Result<Self> {
        let mut cursor = 0usize;

        let magic = take_exact(input, &mut cursor, BLOB_MAGIC.len())?;
        if magic != BLOB_MAGIC {
            return invalid_blob();
        }

        let version = take_u8(input, &mut cursor)?;
        if version != BLOB_VERSION {
            return invalid_blob();
        }

        let name = SecretName::from_secret_id(take_u8(input, &mut cursor)?)
            .map_err(|_| invalid_data("invalid YubiKey secret blob"))?;

        let algorithm = take_u8(input, &mut cursor)?;
        if algorithm != ALGORITHM_AES_256_GCM {
            return invalid_blob();
        }

        let nonce_bytes = take_exact(input, &mut cursor, NONCE_LEN)?;
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(nonce_bytes);

        let wrapped_key_len = usize::from(take_u16(input, &mut cursor)?);
        let wrapped_key = take_exact(input, &mut cursor, wrapped_key_len)?.to_vec();

        let ciphertext_len = usize::try_from(take_u32(input, &mut cursor)?)
            .map_err(|_| invalid_data("invalid YubiKey secret blob"))?;
        let ciphertext = take_exact(input, &mut cursor, ciphertext_len)?.to_vec();

        let tag_bytes = take_exact(input, &mut cursor, TAG_LEN)?;
        let mut tag = [0u8; TAG_LEN];
        tag.copy_from_slice(tag_bytes);

        if cursor != input.len() {
            return invalid_blob();
        }

        Ok(Self {
            name,
            nonce,
            wrapped_key,
            ciphertext,
            tag,
        })
    }
}

fn invalid_blob<T>() -> Result<T> {
    Err(invalid_data("invalid YubiKey secret blob").into())
}

/// 固定長 chunk を `cursor` 位置から切り出し、足りない場合は wire 破損として失敗する。
fn take_exact<'a>(input: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| invalid_data("invalid YubiKey secret blob"))?;
    if end > input.len() {
        return invalid_blob();
    }
    let chunk = &input[*cursor..end];
    *cursor = end;
    Ok(chunk)
}

fn take_u8(input: &[u8], cursor: &mut usize) -> Result<u8> {
    Ok(take_exact(input, cursor, 1)?[0])
}

fn take_u16(input: &[u8], cursor: &mut usize) -> Result<u16> {
    let bytes = take_exact(input, cursor, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn take_u32(input: &[u8], cursor: &mut usize) -> Result<u32> {
    let bytes = take_exact(input, cursor, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}
