//! spare 登録で平文 secret を保持する期間の process / memory 保護。
//!
//! primary から読んだ secret または `--stdin-json` 由来の secret は、spare へ保存する
//! まで core dump 抑止と memory lock の対象に入れる。

use std::{
    collections::BTreeMap,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, bail};
use secrecy::ExposeSecret;
use signal_hook::{SigId, consts::signal};
use zeroize::Zeroizing;

use super::storage::{self, SecretName};
use crate::Result;

/// signal handler から通常の error path へ中断を伝える process-global flag。
static INTERRUPTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
/// signal handler の登録状態を process 全体で 1 組に保つ。
static INTERRUPT_REGISTRATION: LazyLock<Mutex<InterruptRegistration>> =
    LazyLock::new(|| Mutex::new(InterruptRegistration::default()));
const MEMORY_LOCK_PROBE_LEN: usize = 256 * 1024;

/// 平文 secret を保持する期間だけ有効な signal registration。
pub(crate) struct InterruptGuard;

/// ネストした `InterruptGuard` が共有する signal handler 登録状態。
#[derive(Default)]
struct InterruptRegistration {
    /// 現在生存している guard 数。0 から 1 へ変わる時だけ handler を登録する。
    depth: usize,
    /// SIGINT handler の登録 ID。最外側 guard の Drop で解除する。
    sigint: Option<SigId>,
    /// SIGTERM handler の登録 ID。最外側 guard の Drop で解除する。
    sigterm: Option<SigId>,
}

impl InterruptGuard {
    /// SIGINT/SIGTERM は即時終了させず記録し、Drop による unlock/zeroize 後に失敗させる。
    pub(crate) fn install() -> Result<Self> {
        let mut registration = interrupt_registration();
        if registration.depth == 0 {
            INTERRUPTED.store(false, Ordering::SeqCst);
            registration.install_handlers()?;
        }
        registration.depth += 1;
        Ok(Self)
    }

    /// 保護区間の外側へ出る前に、遅延していた SIGINT/SIGTERM を command failure にする。
    pub(crate) fn check_interrupted(&self) -> Result<()> {
        interrupted_result()
    }

    /// 長い YubiKey operation の前後で、signal flag を command failure へ反映する。
    pub(crate) fn run_yubikey_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.check_interrupted()?;
        let result = operation();
        self.check_interrupted()?;
        result
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

/// panic 後も最後の guard が signal handler を解除できるよう、poisoned lock から state を回収する。
fn interrupt_registration() -> MutexGuard<'static, InterruptRegistration> {
    match INTERRUPT_REGISTRATION.lock() {
        Ok(registration) => registration,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl InterruptRegistration {
    /// signal handler は process global なので、最外側 guard だけが登録と解除を担う。
    fn install_handlers(&mut self) -> Result<()> {
        let sigint = register_interrupt(signal::SIGINT)?;
        let sigterm = match register_interrupt(signal::SIGTERM) {
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

    /// 登録済み handler ID を消費し、次の最外側 guard が再登録できる状態に戻す。
    fn unregister_handlers(&mut self) {
        if let Some(sigint) = self.sigint.take() {
            signal_hook::low_level::unregister(sigint);
        }
        if let Some(sigterm) = self.sigterm.take() {
            signal_hook::low_level::unregister(sigterm);
        }
    }
}

/// spare 登録で平文 secret を読む前に必要な process / memory 保護を準備する。
pub(crate) struct SecretMemoryGuard {
    locks: BTreeMap<SecretName, region::LockGuard>,
}

impl SecretMemoryGuard {
    /// secret を読む前に core dump 無効化と memory lock の利用可否を確認する。
    pub(crate) fn prepare() -> Result<Self> {
        rlimit::setrlimit(rlimit::Resource::CORE, 0, 0)
            .context("failed to disable core dumps before reading bootstrap secrets")?;

        let probe = Zeroizing::new(vec![0u8; MEMORY_LOCK_PROBE_LEN]);
        let probe_guard = region::lock(probe.as_ptr(), probe.len())
            .context("failed to lock memory before reading bootstrap secrets")?;
        drop(probe_guard);

        Ok(Self {
            locks: BTreeMap::new(),
        })
    }

    /// 1 secret を受け取った直後に memory lock 対象へ入れる。
    pub(crate) fn lock_secret(
        &mut self,
        name: SecretName,
        secret: storage::SecretBytes,
    ) -> Result<storage::SecretBytes> {
        let guard = lock_secret_memory(&secret)?;
        interrupted_result()?;
        if let Some(guard) = guard {
            self.locks.insert(name, guard);
        } else {
            self.locks.remove(&name);
        }
        Ok(secret)
    }

    pub(crate) fn lock_transient_buffer(
        &self,
        ptr: *const u8,
        len: usize,
    ) -> Result<region::LockGuard> {
        lock_memory_range(ptr, len).context("failed to lock bootstrap secret input memory")
    }
}

fn register_interrupt(signal: i32) -> Result<SigId> {
    signal_hook::flag::register(signal, Arc::clone(&INTERRUPTED))
        .context("failed to install signal handler")
}

/// signal handler が記録した中断を、guard の Drop が走る `Result` error に変換する。
fn interrupted_result() -> Result<()> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        bail!("interrupted while handling bootstrap secrets");
    }

    Ok(())
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

/// stdin JSON など、secret wrapper 化前の一時 buffer を同じ memory lock policy で守る。
fn lock_memory_range(ptr: *const u8, len: usize) -> Result<region::LockGuard> {
    region::lock(ptr, len).map_err(Into::into)
}
