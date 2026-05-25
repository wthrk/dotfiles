//! 平文 bytes の生存期間に紐づける process / memory 保護。

use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex, MutexGuard,
    },
};

use anyhow::{bail, Context};
use signal_hook::{consts::signal, SigId};
use zeroize::Zeroizing;

pub(crate) mod buffer;

pub(crate) use buffer::ProtectedInputBuffer;

use crate::Result;

type SecretBytes = Zeroizing<Vec<u8>>;

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

    /// 保護区間中に記録された SIGINT/SIGTERM を error として返す。
    pub(crate) fn check_interrupted(&self) -> Result<()> {
        interrupted_result()
    }

    /// operation を実行し、前後で interrupt flag を確認する。
    ///
    /// operation 中に記録された中断は後続処理へ進めず error として返す。
    pub(crate) fn run_operation<T>(
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

fn interrupt_registration() -> MutexGuard<'static, InterruptRegistration> {
    match INTERRUPT_REGISTRATION.lock() {
        Ok(registration) => registration,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl InterruptRegistration {
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
    interrupt: InterruptGuard,
    memory: SecretMemoryGuard,
}

/// secret 本文を session lifetime に閉じ込める保護済み所有値。
///
/// 平文 bytes は `with_secret` の借用中だけ公開し、Drop 時の zeroize と memory unlock は
/// 同じ所有値の破棄順序に従う。
pub(crate) struct ProtectedSecret<'session> {
    value: SecretBytes,
    _lock: Option<region::LockGuard>,
    _session: PhantomData<&'session SecretSession>,
}

impl ProtectedSecret<'_> {
    /// 平文 bytes を closure の実行中だけ借用として公開する。
    pub(crate) fn with_secret<R>(&self, borrow: impl FnOnce(&[u8]) -> R) -> R {
        borrow(self.value.as_ref())
    }
}

impl SecretSession {
    /// signal handler、core dump 抑止、mlock probe を同じ保護境界で確立する。
    pub(crate) fn start() -> Result<Self> {
        Ok(Self {
            interrupt: InterruptGuard::install()?,
            memory: SecretMemoryGuard::prepare()?,
        })
    }

    /// 現在の保護区間で記録された SIGINT/SIGTERM を error として返す。
    pub(crate) fn check_interrupted(&self) -> Result<()> {
        self.interrupt.check_interrupted()
    }

    /// operation を実行し、前後で interrupt flag を確認する。
    pub(crate) fn run_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.interrupt.run_operation(operation)
    }

    /// 同じ保護 scope の interrupt guard を貸し出す。
    pub(crate) fn interrupt(&self) -> &InterruptGuard {
        &self.interrupt
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
        value: SecretBytes,
        lock: Option<region::LockGuard>,
    ) -> Result<ProtectedSecret<'_>> {
        let protected = ProtectedSecret {
            value,
            _lock: lock,
            _session: PhantomData,
        };
        interrupted_result()?;
        Ok(protected)
    }
}

impl SecretMemoryGuard {
    fn prepare() -> Result<Self> {
        rlimit::setrlimit(rlimit::Resource::CORE, 0, 0)
            .context("failed to disable core dumps before reading bootstrap secrets")?;

        let probe = Zeroizing::new(vec![0u8; MEMORY_LOCK_PROBE_LEN]);
        let probe_guard = region::lock(probe.as_ptr(), probe.len())
            .context("failed to lock memory before reading bootstrap secrets")?;
        drop(probe_guard);

        Ok(Self)
    }

    fn lock_transient_buffer(&self, ptr: *const u8, len: usize) -> Result<region::LockGuard> {
        lock_memory_range(ptr, len).context("failed to lock bootstrap secret input memory")
    }
}

fn register_interrupt(signal: i32) -> Result<SigId> {
    signal_hook::flag::register(signal, Arc::clone(&INTERRUPTED))
        .context("failed to install signal handler")
}

fn interrupted_result() -> Result<()> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        bail!("interrupted while handling bootstrap secrets");
    }

    Ok(())
}

fn lock_memory_range(ptr: *const u8, len: usize) -> Result<region::LockGuard> {
    region::lock(ptr, len).map_err(Into::into)
}
