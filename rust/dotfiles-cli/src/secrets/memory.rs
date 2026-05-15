//! spare 登録で平文 secret を保持する期間の process / memory 保護。
//!
//! primary から読んだ secret または `--stdin-json` 由来の secret は、spare へ保存する
//! まで core dump 抑止と memory lock の対象に入れる。

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, bail};
use secrecy::ExposeSecret;
use signal_hook::{SigId, consts::signal};
use zeroize::Zeroizing;

use super::storage::{self, BootstrapSecrets, SecretName};
use crate::Result;

static INTERRUPTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
const MEMORY_LOCK_PROBE_LEN: usize = 256 * 1024;

/// 平文 secret を保持する期間だけ有効な signal registration。
pub(crate) struct InterruptGuard {
    sigint: SigId,
    sigterm: SigId,
}

impl InterruptGuard {
    /// SIGINT/SIGTERM は即時終了させず記録し、Drop による unlock/zeroize 後に失敗させる。
    pub(crate) fn install() -> Result<Self> {
        INTERRUPTED.store(false, Ordering::SeqCst);
        let sigint = register_interrupt(signal::SIGINT)?;
        let sigterm = match register_interrupt(signal::SIGTERM) {
            Ok(sigterm) => sigterm,
            Err(err) => {
                signal_hook::low_level::unregister(sigint);
                return Err(err);
            }
        };
        Ok(Self { sigint, sigterm })
    }

    pub(crate) fn interrupted(&self) -> bool {
        INTERRUPTED.load(Ordering::SeqCst)
    }

    /// 長い YubiKey operation の前後で、signal flag を command failure へ反映する。
    pub(crate) fn run_yubikey_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        interrupted_result()?;
        let result = operation();
        interrupted_result()?;
        result
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.sigint);
        signal_hook::low_level::unregister(self.sigterm);
        INTERRUPTED.store(false, Ordering::SeqCst);
    }
}

/// spare 登録で平文 secret を読む前に必要な process / memory 保護を準備する。
pub(crate) struct SecretMemoryGuard {
    bw_email: Option<region::LockGuard>,
    bw_password: Option<region::LockGuard>,
    bws_access_token: Option<region::LockGuard>,
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
            bw_email: None,
            bw_password: None,
            bws_access_token: None,
        })
    }

    /// bootstrap secret 一式を memory lock 対象に登録する。
    pub(crate) fn lock_bootstrap(&mut self, secrets: BootstrapSecrets) -> Result<BootstrapSecrets> {
        interrupted_result()?;
        Ok(BootstrapSecrets {
            bw_email: self.lock_secret(SecretName::BwEmail, secrets.bw_email)?,
            bw_password: self.lock_secret(SecretName::BwPassword, secrets.bw_password)?,
            bws_access_token: self
                .lock_secret(SecretName::BwsAccessToken, secrets.bws_access_token)?,
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
        match name {
            SecretName::BwEmail => self.bw_email = guard,
            SecretName::BwPassword => self.bw_password = guard,
            SecretName::BwsAccessToken => self.bws_access_token = guard,
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

fn lock_memory_range(ptr: *const u8, len: usize) -> Result<region::LockGuard> {
    region::lock(ptr, len).map_err(Into::into)
}
