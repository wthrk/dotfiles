//! RSA-OAEP SHA-256 padding を除去する暗号 utility。

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::Result;

use super::ProtectedSecret;

const OAEP_UNPAD_ERROR: &str = "invalid RSA-OAEP encoded message";
const HASH_LEN: usize = 32;

/// raw RSA decrypt 出力から OAEP-SHA256 を除去して secret payload を復元する。
///
/// `encoded` は RSA modulus byte 長と同じ `key_len` でなければならず、この入口で
/// `encoded.len() == key_len` と OAEP-SHA256 の最小長を検証する。label は空 bytes として
/// SHA-256 hash を照合し、leading byte、masked seed / DB 復元後の padding separator、
/// separator より前の zero padding をすべて満たす場合だけ payload を取り出す。
/// 失敗時は secret slice を caller へ返さず、成功時の payload は新規 `ProtectedSecret` へ
/// コピーしてから返す。
pub(crate) fn unwrap_oaep_sha256(encoded: &[u8], key_len: usize) -> Result<ProtectedSecret> {
    if encoded.len() != key_len || key_len < 2 * HASH_LEN + 2 {
        bail!(OAEP_UNPAD_ERROR);
    }

    let (masked_seed, masked_db) = encoded[1..].split_at(HASH_LEN);
    let seed_mask = mgf1_sha256(masked_db, HASH_LEN)?;
    let seed = xor_with_mask(masked_seed, &seed_mask)?;
    let db_mask = seed.with_secret(|seed| mgf1_sha256(seed, key_len - HASH_LEN - 1))?;
    let db = xor_with_mask(masked_db, &db_mask)?;

    db.with_secret(|db| {
        let label_hash = Sha256::digest([]);
        let label_mismatch = db[..HASH_LEN]
            .iter()
            .zip(label_hash.iter())
            .fold(0u8, |acc, (left, right)| acc | (left ^ right));
        let leading_and_label_valid = encoded[0] == 0 && label_mismatch == 0;

        let rest = &db[HASH_LEN..];
        let (separator, padding_valid) = find_oaep_separator(rest);
        if !leading_and_label_valid || !padding_valid {
            bail!(OAEP_UNPAD_ERROR);
        }
        let separator = separator.context(OAEP_UNPAD_ERROR)?;
        let plain = &rest[separator + 1..];
        let mut protected = ProtectedSecret::new(plain.len())?;
        protected.with_secret_mut(|out| out.copy_from_slice(plain));
        Ok(protected)
    })
}

/// MGF1-SHA256 mask を protected buffer として生成する。
///
/// seed は raw RSA-OAEP 復元処理中の中間値として参照だけを受け取り、生成済み mask は
/// `ProtectedSecret` に閉じる。caller は OAEP 仕様上必要な長さを渡し、失敗時は
/// padding 判定を継続せず上位の OAEP unpad error として扱う。
fn mgf1_sha256(seed: &[u8], len: usize) -> Result<ProtectedSecret> {
    let mut out = ProtectedSecret::new(len)?;
    out.with_secret_mut(|out_bytes| {
        let mut counter = 0u32;
        let mut written = 0;
        while written < len {
            let mut digest = Sha256::new();
            digest.update(seed);
            digest.update(counter.to_be_bytes());
            let block = digest.finalize();
            let chunk_len = (len - written).min(block.len());
            out_bytes[written..written + chunk_len].copy_from_slice(&block[..chunk_len]);
            written += chunk_len;
            counter += 1;
        }
    });
    Ok(out)
}

/// masked bytes と protected mask を XOR し、復元結果を protected buffer に置く。
///
/// 平文化される seed / DB の中間表現を `Vec<u8>` として外へ出さないための境界である。
/// 長さ一致は OAEP 構造を検証した caller の責務で、生成先 buffer はコピー前に
/// `ProtectedSecret::new` で確保する。
fn xor_with_mask(masked: &[u8], mask: &ProtectedSecret) -> Result<ProtectedSecret> {
    debug_assert_eq!(masked.len(), mask.len());
    let mut out = ProtectedSecret::new(masked.len())?;
    out.with_secret_mut(|out_bytes| {
        mask.with_secret(|mask_bytes| {
            for ((dst, left), right) in out_bytes
                .iter_mut()
                .zip(masked.iter())
                .zip(mask_bytes.iter())
            {
                *dst = *left ^ *right;
            }
        });
    });
    Ok(out)
}

/// OAEP DB の PS || 0x01 || message 境界を探し、padding bytes の妥当性を返す。
///
/// separator より前はすべて 0 でなければならず、0x01 が存在しない場合も不正 padding とする。
/// scan 中は separator 発見後も同じ畳み込みを継続し、padding 内容による分岐を増やさない。
/// 返値は caller が label hash / leading byte 判定と合成し、失敗時に secret message slice を
/// 取り出さないための padding 判定境界である。
fn find_oaep_separator(rest: &[u8]) -> (Option<usize>, bool) {
    let mut separator = 0usize;
    let mut before_separator = 1u8;
    let mut seen_separator = 0u8;
    let mut padding_mismatch = 0u8;
    for (index, byte) in rest.iter().copied().enumerate() {
        let is_zero = byte_eq(byte, 0);
        let is_one = byte_eq(byte, 1);
        let first_separator = before_separator & is_one;
        let invalid_padding = before_separator & (1 ^ is_zero) & (1 ^ is_one);
        let separator_mask = 0usize.wrapping_sub(first_separator as usize);

        padding_mismatch |= invalid_padding;
        separator = (separator & !separator_mask) | (index & separator_mask);
        seen_separator |= first_separator;
        before_separator &= 1 ^ first_separator;
    }
    (
        (seen_separator == 1).then_some(separator),
        seen_separator == 1 && padding_mismatch == 0,
    )
}

fn byte_eq(left: u8, right: u8) -> u8 {
    let diff = left ^ right;
    (((diff as u16).wrapping_sub(1) >> 8) & 1) as u8
}
