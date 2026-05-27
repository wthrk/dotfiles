//! 平文 bytes の生存期間に紐づける process / memory 保護。

use std::{
    collections::BTreeMap,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, bail};
use signal_hook::{SigId, consts::signal};
use zeroize::Zeroizing;

pub(crate) mod buffer;
mod oaep;
pub(crate) mod sealed_blob;
pub(crate) mod secret_consumer;
pub(crate) mod secret_random;

use crate::Result;
static INTERRUPTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
static INTERRUPT_REGISTRATION: LazyLock<Mutex<InterruptRegistration>> =
    LazyLock::new(|| Mutex::new(InterruptRegistration::default()));
const MEMORY_LOCK_PROBE_LEN: usize = 256 * 1024;

/// SIGINT/SIGTERM を flag として記録する保護区間 guard。
pub(crate) struct InterruptGuard;

#[derive(Default)]
struct InterruptRegistration {
    depth: usize,
    sigint: Option<SigId>,
    sigterm: Option<SigId>,
}

impl InterruptGuard {
    /// SIGINT/SIGTERM handler を登録した保護区間を開始する。
    ///
    /// ネスト時は最外側の guard が handler を登録し、最後の guard の Drop で解除する。
    pub(crate) fn install() -> Result<Self> {
        let mut registration = interrupt_registration();
        if registration.depth == 0 {
            INTERRUPTED.store(false, Ordering::SeqCst);
            registration.install_handlers()?;
        }
        registration.depth += 1;
        Ok(Self)
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        let mut registration = interrupt_registration();
        if registration.depth == 0 {
            return;
        }
        registration.depth -= 1;
        if registration.depth == 0 {
            registration.unregister_handlers();
            INTERRUPTED.store(false, Ordering::SeqCst);
        }
    }
}

fn interrupt_registration() -> MutexGuard<'static, InterruptRegistration> {
    match INTERRUPT_REGISTRATION.lock() {
        Ok(registration) => registration,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl InterruptRegistration {
    /// 2 種類の signal handler を同一トランザクションで登録する。
    ///
    /// `SIGTERM` 登録が失敗した場合は `SIGINT` だけが残らないよう巻き戻し、
    /// 「両方登録できた時だけ保護区間を開始する」停止条件を維持する。
    /// process 全体に signal handler を入れる副作用を、この registration 管理下に限定する。
    ///
    /// 複数 use case が同一 process で連続実行されても handler の重複登録を避け、
    /// 途中失敗時に片側だけ残る状態を作らないことを安全境界として維持する。
    fn install_handlers(&mut self) -> Result<()> {
        let sigint = signal_hook::flag::register(signal::SIGINT, Arc::clone(&INTERRUPTED))
            .context("failed to install signal handler")?;
        let sigterm = match signal_hook::flag::register(signal::SIGTERM, Arc::clone(&INTERRUPTED))
            .context("failed to install signal handler")
        {
            Ok(sigterm) => sigterm,
            Err(err) => {
                signal_hook::low_level::unregister(sigint);
                return Err(err);
            }
        };
        self.sigint = Some(sigint);
        self.sigterm = Some(sigterm);
        Ok(())
    }

    fn unregister_handlers(&mut self) {
        if let Some(sigint) = self.sigint.take() {
            signal_hook::low_level::unregister(sigint);
        }
        if let Some(sigterm) = self.sigterm.take() {
            signal_hook::low_level::unregister(sigterm);
        }
    }
}

/// core dump 抑止と mlock 利用可否を保持する memory guard。
struct SecretMemoryGuard;

/// 平文 bytes を読む use case 全体の signal / memory 保護境界。
pub(crate) struct SecretSession {
    _interrupt: InterruptGuard,
    memory: SecretMemoryGuard,
}

/// secret 本文を session lifetime に閉じ込める保護済み所有値。
///
/// ** メソッドを拡張してコピーのためのセキュリティホールを新築してはならない**
/// 平文 bytes は `with_secret` / `with_secret_mut` の借用中だけ公開する。
/// `Zeroizing` はこの protection 境界の内部実装詳細であり、外部層へ所有権や型を露出しない。
/// memory lock は入力 session から引き継いだ値、または clone/copy 用の locked destination
/// として確保した値で保持し、Drop 時に zeroize を実行する。
pub struct ProtectedSecret {
    value: Zeroizing<Vec<u8>>,
    _lock: region::LockGuard,
}

impl PartialEq for ProtectedSecret {
    fn eq(&self, other: &Self) -> bool {
        self.with_secret(|left| other.with_secret(|right| left == right))
    }
}

impl Eq for ProtectedSecret {}

impl ProtectedSecret {
    /// zeroize 対象の locked buffer を指定長で新規確保する。
    ///
    /// 返値は全 byte が 0 で初期化され、plaintext 書き込み前に memory lock 済みである。
    pub(crate) fn new(len: usize) -> Result<Self> {
        Self::allocate_locked_destination(len)
    }

    /// 唯一許されたバッファコピー
    pub(crate) fn try_clone(from: &Self) -> Result<Self> {
        let mut this = Self::new(from.len())?;
        this.with_secret_mut(|to| from.with_secret(|from| to.copy_from_slice(from)));
        Ok(this)
    }

    /// 指定長の destination buffer を確保し、plaintext copy 前に memory lock を取得する。
    fn allocate_locked_destination(len: usize) -> Result<Self> {
        let lock_len = len.max(1);
        let mut value = vec![0u8; lock_len];
        let lock = region::lock(value.as_ptr(), lock_len)
            .context("failed to lock protected secret memory")?;
        value.truncate(len);
        Ok(Self {
            value: Zeroizing::new(value),
            _lock: lock,
        })
    }

    /// 平文 bytes を closure の実行中だけ借用として公開する。
    ///
    /// クロージャー内でデータを外部被保護バッファにコピーしてはならない
    pub(in crate::secrets::support::protection) fn with_secret<R>(
        &self,
        borrow: impl FnOnce(&[u8]) -> R,
    ) -> R {
        borrow(self.value.as_slice())
    }

    /// 平文 bytes を closure の実行中だけ mutable 借用として公開する。
    ///
    /// クロージャー内でデータを外部被保護バッファにコピーしてはならない
    pub(in crate::secrets::support::protection) fn with_secret_mut<R>(
        &mut self,
        borrow: impl FnOnce(&mut [u8]) -> R,
    ) -> R {
        borrow(self.value.as_mut_slice())
    }

    /// 保持中 secret の byte 長を返す。
    ///
    /// 長さ情報のみを露出し、平文 bytes 本体は返さない境界を維持する。
    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    /// JSON object を `String -> ProtectedSecret` へ変換する。
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
    /// signal handler、core dump 抑止、mlock probe を同じ保護境界で確立する。
    pub(crate) fn start() -> Result<Self> {
        Ok(Self {
            _interrupt: InterruptGuard::install()?,
            memory: SecretMemoryGuard::prepare()?,
        })
    }

    /// 一時入力 buffer の memory range を現在の session で lock する。
    pub(super) fn lock_transient_buffer(
        &self,
        ptr: *const u8,
        len: usize,
    ) -> Result<region::LockGuard> {
        self.memory.lock_transient_buffer(ptr, len)
    }

    /// lock 済み allocation の所有権を、session lifetime に紐づく secret 値へ移す。
    pub(super) fn protect_locked_secret_value(
        &self,
        value: Vec<u8>,
        lock: region::LockGuard,
    ) -> Result<ProtectedSecret> {
        let protected = ProtectedSecret {
            value: Zeroizing::new(value),
            _lock: lock,
        };
        if INTERRUPTED.load(Ordering::SeqCst) {
            bail!("operation interrupted");
        }
        Ok(protected)
    }
}

impl SecretMemoryGuard {
    /// core dump 抑止と mlock probe をまとめて初期化し、以後の秘密入力保護を成立させる。
    ///
    /// この境界で失敗した場合は保護前提が崩れるため、secret 入力処理へ進ませない。
    fn prepare() -> Result<Self> {
        rlimit::setrlimit(rlimit::Resource::CORE, 0, 0).context("failed to disable core dumps")?;

        let probe = Zeroizing::new(vec![0u8; MEMORY_LOCK_PROBE_LEN]);
        let probe_guard =
            region::lock(probe.as_ptr(), probe.len()).context("failed to lock process memory")?;
        drop(probe_guard);

        Ok(Self)
    }

    /// 一時入力 buffer の生メモリ範囲を lock し、swap 退避と core dump 露出を抑止する。
    fn lock_transient_buffer(&self, ptr: *const u8, len: usize) -> Result<region::LockGuard> {
        region::lock(ptr, len).context("failed to lock input buffer memory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(bytes, b"abc");
        });
        original.with_secret(|bytes| {
            assert_eq!(bytes, b"xyz");
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
            assert_eq!(bytes, b"persist");
        });
        Ok(())
    }
}
