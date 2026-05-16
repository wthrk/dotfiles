//! 平文 secret を扱う間だけ有効にする process / memory 保護。

use std::{
    marker::PhantomData,
    ops::Deref,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, bail};
use signal_hook::{SigId, consts::signal};
use zeroize::Zeroizing;

pub(crate) mod buffer;

pub(crate) use buffer::ProtectedInputBuffer;

use crate::Result;

static INTERRUPTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
static INTERRUPT_REGISTRATION: LazyLock<Mutex<InterruptRegistration>> =
    LazyLock::new(|| Mutex::new(InterruptRegistration::default()));
const MEMORY_LOCK_PROBE_LEN: usize = 256 * 1024;

/// 平文 secret を保持する区間だけ SIGINT/SIGTERM の既定動作を遅延する。
pub(crate) struct InterruptGuard;

#[derive(Default)]
struct InterruptRegistration {
    depth: usize,
    sigint: Option<SigId>,
    sigterm: Option<SigId>,
}

impl InterruptGuard {
    /// ネスト時は最外側だけが handler を登録し、最後の guard の Drop で解除する。
    pub(crate) fn install() -> Result<Self> {
        let mut registration = interrupt_registration();
        if registration.depth == 0 {
            INTERRUPTED.store(false, Ordering::SeqCst);
            registration.install_handlers()?;
        }
        registration.depth += 1;
        Ok(Self)
    }

    /// 保護区間の途中で受けた SIGINT/SIGTERM を、明示的な失敗として返す。
    pub(crate) fn check_interrupted(&self) -> Result<()> {
        interrupted_result()
    }

    /// YubiKey operation 前後に signal flag を確認し、途中中断を後続処理へ進めない。
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

/// 平文 secret を読む前に core dump と mlock 利用可否を確定する。
struct SecretMemoryGuard;

/// 平文 secret を扱う use case の signal / memory 保護境界。
pub(crate) struct SecretSession {
    interrupt: InterruptGuard,
    memory: SecretMemoryGuard,
}

/// secret 値と memory lock guard を同じ所有値として保持する。
///
/// `Deref` で借用できるのはこの値の生存中だけで、unlock は inner value の Drop 後に走る。
pub(crate) struct Protected<'session, T> {
    value: T,
    _lock: Option<region::LockGuard>,
    _session: PhantomData<&'session SecretSession>,
}

impl SecretSession {
    /// secret 入力前に signal handler、core dump 抑止、mlock probe を同じ境界で確立する。
    pub(crate) fn start() -> Result<Self> {
        Ok(Self {
            interrupt: InterruptGuard::install()?,
            memory: SecretMemoryGuard::prepare()?,
        })
    }

    /// 保護区間の途中で受けた SIGINT/SIGTERM を、明示的な失敗として返す。
    pub(crate) fn check_interrupted(&self) -> Result<()> {
        self.interrupt.check_interrupted()
    }

    /// YubiKey operation 前後に signal flag を確認し、途中中断を後続処理へ進めない。
    pub(crate) fn run_yubikey_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.interrupt.run_yubikey_operation(operation)
    }

    /// device adapter など既存境界へ、同じ保護スコープの interrupt guard だけを貸し出す。
    pub(crate) fn interrupt(&self) -> &InterruptGuard {
        &self.interrupt
    }

    /// secret 値をこの session より長生きできない memory lock 付き所有値へ移す。
    pub(crate) fn protect_value<T>(
        &self,
        value: T,
        expose: impl FnOnce(&T) -> &[u8],
    ) -> Result<Protected<'_, T>> {
        let lock = lock_secret_memory(expose(&value))?;
        self.protect_locked_value(value, lock)
    }

    /// 読み込み時点で lock 済みの allocation から作った値は、同じ guard を引き継ぐ。
    pub(crate) fn protect_locked_value<T>(
        &self,
        value: T,
        lock: Option<region::LockGuard>,
    ) -> Result<Protected<'_, T>> {
        let protected = Protected {
            value,
            _lock: lock,
            _session: PhantomData,
        };
        interrupted_result()?;
        Ok(protected)
    }

    /// JSON parse 前の入力 buffer は storage 型へ移る前から同じ mlock policy で守る。
    pub(super) fn lock_transient_buffer(
        &self,
        ptr: *const u8,
        len: usize,
    ) -> Result<region::LockGuard> {
        self.memory.lock_transient_buffer(ptr, len)
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

impl<T> Deref for Protected<'_, T> {
    type Target = T;

    /// 保護値の借用は `Protected<T>` の生存期間に縛り、unlock より後へ延ばさない。
    fn deref(&self) -> &Self::Target {
        &self.value
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

fn lock_secret_memory(secret: &[u8]) -> Result<Option<region::LockGuard>> {
    if secret.is_empty() {
        return Ok(None);
    }

    region::lock(secret.as_ptr(), secret.len())
        .map(Some)
        .context("failed to lock bootstrap secret memory")
}

fn lock_memory_range(ptr: *const u8, len: usize) -> Result<region::LockGuard> {
    region::lock(ptr, len).map_err(Into::into)
}
