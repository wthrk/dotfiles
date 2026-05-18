//! YubiKey 選択と device open の外部境界。

use crate::Result;

use super::super::{domain, support::protection::InterruptGuard};

/// use case が対象 YubiKey を開くための最小 contract。
pub(crate) trait SecretDeviceFactoryPort {
    type Device: domain::SecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device>;
}
