//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! use case orchestration から concrete 境界実装を分離し、application 本体は順序制御だけに集中させる。

use anyhow::Context;

use super::{open_device, open_spare_device, DeviceBackend, YubikeySecretDevice};
use crate::{
    secrets::{ports::SecretsBoundary, support::protection::InterruptGuard},
    Result,
};

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(crate) struct RealSecretsBoundary {
    pub(crate) backend: DeviceBackend,
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = YubikeySecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        open_device(&mut self.backend, serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device> {
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for spare YubiKey")?;
        open_spare_device(&mut self.backend, spare_serial, primary_serial, &interrupt)
    }
}
