//! 平文 bytes の生存期間に紐づける process 保護と zeroize 境界。

use std::{collections::BTreeMap, future::Future, pin::Pin};

use anyhow::{Context, bail};
use zeroize::Zeroizing;

pub(crate) mod buffer;
pub(crate) mod bws;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod gpg_backup;
#[cfg(not(feature = "secrets-internal-test-stub"))]
mod oaep;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod sealed_blob;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod secret_random;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod yubikey_piv;

use crate::Result;

/// core dump 抑止を secret 入力前に確立する process guard。
struct SecretProcessGuard;

/// 平文 bytes を読む use case 全体の process 保護境界。
pub(crate) struct SecretSession {
    process: SecretProcessGuard,
}

/// secret 本文を session lifetime に閉じ込める保護済み所有値。
///
/// ** メソッドを拡張してコピーのためのセキュリティホールを新築してはならない**
/// 平文 bytes は `with_secret` / `with_secret_mut` の借用中だけ公開する。
/// `Zeroizing` はこの protection 境界の内部実装詳細であり、外部層へ所有権や型を露出しない。
/// Drop 時の zeroize はこの所有型の責務である。memory lock は成功時だけ保持する補助であり、
/// secret 保護成立の必須条件として扱わない。
pub struct ProtectedSecret {
    value: Zeroizing<Vec<u8>>,
    _lock: Option<region::LockGuard>,
}

impl PartialEq for ProtectedSecret {
    fn eq(&self, other: &Self) -> bool {
        self.with_secret(|left| other.with_secret(|right| left == right))
    }
}

impl Eq for ProtectedSecret {}

impl ProtectedSecret {
    /// zeroize 対象 buffer を指定長で新規確保する。
    ///
    /// 返値は全 byte が 0 で初期化される。
    pub(crate) fn new(len: usize) -> Result<Self> {
        Self::allocate_destination(len)
    }

    /// destination へ secret bytes を複製する唯一のコピー境界。
    ///
    /// `ProtectedSecret` の複製は、コピー先を確保してから平文 bytes を借用中だけ書き込む
    /// この経路に限定する。確保に失敗した場合は `Err` を返し、途中状態の protected value を
    /// 返さない。
    ///
    /// caller が `with_secret` から直接コピー経路を作ると、zeroize 管理外の buffer を作れるため
    /// 禁止する。新しい複製 API が必要な場合も、この関数を経由して所有境界を維持する。
    pub(crate) fn try_clone(from: &Self) -> Result<Self> {
        let mut this = Self::new(from.len())?;
        this.with_secret_mut(|to| from.with_secret(|from| to.copy_from_slice(from)));
        Ok(this)
    }

    /// 指定長の destination buffer を確保する。
    fn allocate_destination(len: usize) -> Result<Self> {
        let lock_len = len.max(1);
        let mut value = vec![0u8; lock_len];
        let lock = try_lock(value.as_ptr(), lock_len);
        value.truncate(len);
        Ok(Self {
            value: Zeroizing::new(value),
            _lock: lock,
        })
    }

    /// 平文 bytes を closure の実行中だけ借用として公開する。
    ///
    /// クロージャー内でデータを外部被保護バッファにコピーしてはならない
    pub(in crate::support::protection) fn with_secret<R>(
        &self,
        borrow: impl FnOnce(&[u8]) -> R,
    ) -> R {
        borrow(self.value.as_slice())
    }

    /// 平文 bytes を closure の実行中だけ mutable 借用として公開する。
    ///
    /// クロージャー内でデータを外部被保護バッファにコピーしてはならない
    pub(in crate::support::protection) fn with_secret_mut<R>(
        &mut self,
        borrow: impl FnOnce(&mut [u8]) -> R,
    ) -> R {
        borrow(self.value.as_mut_slice())
    }

    /// UTF-8 secret text を async 外部処理の完了まで借用境界内へ閉じる。
    ///
    /// SDK などが owned plaintext buffer を要求する場合、この closure 内で buffer 作成と
    /// `.await` まで完了させ、repository 側の所有 buffer を closure 外へ持ち出さない。
    pub(in crate::support::protection) async fn with_secret_utf8_async<R>(
        &self,
        borrow: impl for<'a> FnOnce(&'a str) -> Pin<Box<dyn Future<Output = Result<R>> + 'a>>,
    ) -> Result<R> {
        let text =
            std::str::from_utf8(self.value.as_slice()).context("secret is not valid UTF-8")?;
        borrow(text).await
    }

    /// UTF-8 secret text を protection backend closure の実行中だけ借用として公開する。
    ///
    /// 外部 secret datastore から返った plaintext を、用途別 backend 操作内で検証済み値へ変換するための
    /// 境界である。closure は平文文字列を返値や外部 buffer として持ち出してはならず、検証済み value
    /// または error だけを返す。
    pub(in crate::support::protection) fn with_secret_utf8<R>(
        &self,
        borrow: impl FnOnce(&str) -> Result<R>,
    ) -> Result<R> {
        let text =
            std::str::from_utf8(self.value.as_slice()).context("secret is not valid UTF-8")?;
        borrow(text)
    }

    /// 保持中 secret の byte 長を返す。
    ///
    /// 長さ情報のみを露出し、平文 bytes 本体は返さない境界を維持する。
    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    /// `#[cfg(test)]` または `secrets-internal-test-stub` で使う secret 観測口として、test bytes から
    /// 保護値を作る。
    ///
    /// production build では公開されず、通常経路の plaintext 取り出し / 投入 API として扱わない。
    /// stub build では internal backend stub が DEK round-trip 等の test 観測のためにだけ使う。
    #[cfg(any(test, feature = "secrets-internal-test-stub"))]
    pub(crate) fn from_test_bytes(bytes: &[u8]) -> Result<Self> {
        let mut secret = Self::new(bytes.len())?;
        secret.with_secret_mut(|out| out.copy_from_slice(bytes));
        Ok(secret)
    }

    /// `#[cfg(test)]` または `secrets-internal-test-stub` で使う secret 観測口として、
    /// 保持 bytes を test/stub 側へ複製する。
    ///
    /// production build では公開されず、外部処理境界での plaintext 取り出し許可ではない。
    #[cfg(any(test, feature = "secrets-internal-test-stub"))]
    #[cfg_attr(
        all(test, not(feature = "secrets-internal-test-stub")),
        expect(dead_code)
    )]
    pub(crate) fn to_test_bytes(&self) -> Vec<u8> {
        self.with_secret(|bytes| bytes.to_vec())
    }

    /// secret JSON bytes を field 単位の locked `ProtectedSecret` map へ復元する。
    ///
    /// 入力 JSON は `with_secret` の借用中だけ parse し、serde が作る平文 `String` は
    /// `Zeroizing<String>` としてこの関数の stack frame 内に閉じる。各 field は
    /// `field_limit` を超えた時点で `Err` にし、長すぎる平文を protected map へ格納しない。
    ///
    /// 成功時は各 field value を新しい locked `ProtectedSecret` へコピーして返す。返却後に
    /// caller が保持するのは protection 境界内の値だけであり、一時 `String` は Drop 時に
    /// zeroize される。
    pub(crate) fn decode_json_string_map(
        &self,
        field_limit: usize,
    ) -> Result<BTreeMap<String, Self>> {
        let object = self.with_secret(|bytes| {
            serde_json::from_slice::<BTreeMap<String, Zeroizing<String>>>(bytes)
                .context("failed to decode protected JSON object")
        })?;

        let mut fields = BTreeMap::new();
        for (key, text) in object {
            if text.len() > field_limit {
                bail!("JSON field `{key}` exceeds maximum length");
            }
            let mut secret = Self::new(text.len())
                .with_context(|| format!("failed to protect JSON field `{key}`"))?;
            secret.with_secret_mut(|out| out.copy_from_slice(text.as_bytes()));
            fields.insert(key, secret);
        }
        Ok(fields)
    }
}

impl SecretSession {
    /// secret 入力前に core dump 抑止を確立する。
    pub(crate) fn start() -> Result<Self> {
        Ok(Self {
            process: SecretProcessGuard::prepare()?,
        })
    }

    /// 一時入力 buffer の memory range を best-effort で lock する。
    pub(super) fn lock_transient_buffer(
        &self,
        ptr: *const u8,
        len: usize,
    ) -> Option<region::LockGuard> {
        self.process.lock_transient_buffer(ptr, len)
    }

    /// allocation と任意の lock guard を `ProtectedSecret` へ再結合する。
    ///
    /// この境界で raw `Vec<u8>` は `Zeroizing` 管理へ入り、lock guard がある場合は
    /// `ProtectedSecret` の所有物として保持される。
    pub(super) fn protect_locked_secret_value(
        &self,
        value: Vec<u8>,
        lock: Option<region::LockGuard>,
    ) -> ProtectedSecret {
        ProtectedSecret {
            value: Zeroizing::new(value),
            _lock: lock,
        }
    }
}

impl SecretProcessGuard {
    /// core dump 抑止を初期化し、secret の crash dump 永続化経路を閉じる。
    fn prepare() -> Result<Self> {
        rlimit::setrlimit(rlimit::Resource::CORE, 0, 0).context("failed to disable core dumps")?;
        Ok(Self)
    }

    /// 一時入力 buffer の生メモリ範囲を best-effort で lock する。
    fn lock_transient_buffer(&self, ptr: *const u8, len: usize) -> Option<region::LockGuard> {
        try_lock(ptr, len)
    }
}

fn try_lock(ptr: *const u8, len: usize) -> Option<region::LockGuard> {
    region::lock(ptr, len).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn assert_secret_bytes_eq(actual: &[u8], expected: &[u8], label: &str) {
        let actual_digest: [u8; 32] = Sha256::digest(actual).into();
        let expected_digest: [u8; 32] = Sha256::digest(expected).into();

        assert_eq!(actual.len(), expected.len(), "{label} length mismatch");
        assert_eq!(actual_digest, expected_digest, "{label} digest mismatch");
    }

    fn protected_from_test_bytes(bytes: &[u8]) -> Result<ProtectedSecret> {
        let mut secret = ProtectedSecret::new(bytes.len())?;
        secret.with_secret_mut(|out| out.copy_from_slice(bytes));
        Ok(secret)
    }

    #[test]
    fn try_clone_returns_independent_locked_copy() -> Result<()> {
        let mut original = protected_from_test_bytes(b"abc")?;
        let cloned = ProtectedSecret::try_clone(&original)?;

        original.with_secret_mut(|bytes| bytes.copy_from_slice(b"xyz"));

        cloned.with_secret(|bytes| {
            assert_secret_bytes_eq(bytes, b"abc", "cloned secret");
        });
        original.with_secret(|bytes| {
            assert_secret_bytes_eq(bytes, b"xyz", "original secret");
        });
        Ok(())
    }

    #[test]
    fn try_clone_copy_survives_source_drop() -> Result<()> {
        let cloned = {
            let original = protected_from_test_bytes(b"persist")?;
            ProtectedSecret::try_clone(&original)?
        };

        cloned.with_secret(|bytes| {
            assert_secret_bytes_eq(bytes, b"persist", "cloned secret");
        });
        Ok(())
    }
}
