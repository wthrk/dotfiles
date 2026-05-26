//! 平文 bytes の生存期間に紐づける process / memory 保護。

use std::{
    io::Write,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, bail};
use signal_hook::{SigId, consts::signal};
use zeroize::Zeroizing;

pub(crate) mod buffer;

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
    _interrupt: InterruptGuard,
    memory: SecretMemoryGuard,
}

/// secret 本文を session lifetime に閉じ込める保護済み所有値。
///
/// 平文 bytes は `with_secret` / `with_secret_mut` の借用中だけ公開する。
/// memory lock は `SecretSession::protect_locked_secret_value` 経由で渡された場合のみ保持され、
/// lock の有無に関わらず Drop 時に zeroize を実行する。
pub struct ProtectedSecret {
    value: Zeroizing<Vec<u8>>,
    _lock: Option<region::LockGuard>,
}

impl ProtectedSecret {
    /// 既存呼び出し経路互換のために生 bytes から保護値を構築する。
    ///
    /// 新規コードでは `from_vec` を優先し、この関数は API 移行期間の薄い互換境界として使う。
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self::from_vec(bytes)
    }

    /// bytes 所有権を受け取り、保護済み secret として保持する。
    pub(crate) fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            value: Zeroizing::new(bytes),
            _lock: None,
        }
    }

    /// 平文 bytes を closure の実行中だけ借用として公開する。
    pub(crate) fn with_secret<R>(&self, borrow: impl FnOnce(&[u8]) -> R) -> R {
        borrow(self.value.as_slice())
    }

    /// 既存呼び出し名互換のために `with_secret` へ委譲する。
    ///
    /// 借用生存期間は closure 実行中に限定され、呼び出し側は参照を外へ保持してはならない。
    pub(crate) fn with_bytes<R>(&self, borrow: impl FnOnce(&[u8]) -> R) -> R {
        self.with_secret(borrow)
    }

    /// 平文 bytes を closure の実行中だけ mutable 借用として公開する。
    pub(crate) fn with_secret_mut<R>(&mut self, borrow: impl FnOnce(&mut [u8]) -> R) -> R {
        borrow(self.value.as_mut_slice())
    }

    /// 保持中 secret の byte 長を返す。
    ///
    /// 長さ情報のみを露出し、平文 bytes 本体は返さない境界を維持する。
    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    /// 保護済み secret の所有権を Vec<u8> として取り出す。
    ///
    /// 呼び出し後に元の `ProtectedSecret` は残らないため、不要な平文複製を作らずに
    /// domain 値へ所有権を移譲できる。
    pub(crate) fn into_vec(self) -> Vec<u8> {
        let mut wrapped = std::mem::ManuallyDrop::new(self);
        let mut value = std::mem::take(&mut wrapped.value);
        let vec = std::mem::take(&mut *value);
        if let Some(lock) = wrapped._lock.take() {
            drop(lock);
        }
        vec
    }
}

impl AsRef<[u8]> for ProtectedSecret {
    fn as_ref(&self) -> &[u8] {
        self.value.as_slice()
    }
}

impl Write for ProtectedSecret {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.value.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.value.flush()
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
        lock: Option<region::LockGuard>,
    ) -> Result<ProtectedSecret> {
        let protected = ProtectedSecret {
            value: Zeroizing::new(value),
            _lock: lock,
        };
        interrupted_result()?;
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
        lock_memory_range(ptr, len).context("failed to lock input buffer memory")
    }
}

/// signal hook crate への登録をこの境界に閉じ込める。
///
/// 外部 interaction（プロセス signal handler 登録）の失敗を `anyhow::Result` に正規化し、
/// 呼び出し側が crate 固有 error 型へ依存しないようにする。
/// signal-hook への具体登録をこの関数で閉じ込め、呼び出し側の責務を
/// 「保護区間開始の可否判定」だけに固定する。
///
/// ここでの失敗は保護境界の不成立を意味するため、recover せず即座に
/// `Result::Err` として上位へ返す。
fn register_interrupt(signal: i32) -> Result<SigId> {
    signal_hook::flag::register(signal, Arc::clone(&INTERRUPTED))
        .context("failed to install signal handler")
}

/// support 層の停止条件境界。
///
/// signal handler が立てた中断フラグを集約判定し、true の場合は処理継続を禁止して
/// `Err` を返す。caller は保護区間の主要ステップ境界ごとに本関数を呼び、以降の
/// memory 処理や外部 I/O を続行しない責務を負う。
fn interrupted_result() -> Result<()> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        bail!("operation interrupted");
    }

    Ok(())
}

/// support 層の安全性境界として生メモリ範囲の lock を一元化する。
///
/// 呼び出し側は有効な pointer/length ペアのみを渡し、取得した guard を必要期間
/// 保持する責務を負う。lock 失敗は保護境界不成立として扱い、recover せず上位へ返す。
fn lock_memory_range(ptr: *const u8, len: usize) -> Result<region::LockGuard> {
    region::lock(ptr, len).map_err(Into::into)
}
