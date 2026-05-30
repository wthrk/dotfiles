//! entrypoint の runtime session と adapter catalog 初期化を扱う composition 境界。
//!
//! support session の開始と adapter 所有関係をここに閉じ、root module と command dispatch へ
//! secret protection の初期化責務を混ぜない。

use crate::{
    Result,
    secrets::{
        adapters::{
            BwsClientAdapter, DeviceSelectionAdapter, JsonReportAdapter, ProcessIoAdapter,
            StorageAdapter,
        },
        support::protection::SecretSession,
    },
};

use super::dispatch;

/// entrypoint runtime が所有する実 adapter 群。
///
/// `adapters/` 配下の composition helper や catalog 公開面へ依存せず、entrypoint 内部で
/// use case dispatch に必要な port 実装群だけを束ねる。
pub(super) struct RuntimePorts {
    pub(super) device: DeviceSelectionAdapter,
    pub(super) spare_device: DeviceSelectionAdapter,
    pub(super) device_pin_policy: DeviceSelectionAdapter,
    pub(super) process_io: ProcessIoAdapter,
    pub(super) storage: StorageAdapter,
    pub(super) report: JsonReportAdapter,
    pub(super) bws_client: BwsClientAdapter,
}

impl RuntimePorts {
    /// production command path 用の実 adapter 群を構築する。
    fn production() -> Self {
        Self {
            device: DeviceSelectionAdapter::default(),
            spare_device: DeviceSelectionAdapter::default(),
            device_pin_policy: DeviceSelectionAdapter::default(),
            process_io: ProcessIoAdapter::default(),
            storage: StorageAdapter::default(),
            report: JsonReportAdapter::default(),
            bws_client: BwsClientAdapter,
        }
    }
}

/// secret 保護 session を開始して adapter catalog を構築し、parse 済み command を dispatch する。
pub(super) async fn run(options: super::super::SecretsOptions) -> Result<()> {
    let _session = SecretSession::start()?;
    let mut ports = RuntimePorts::production();
    dispatch::dispatch(options, &mut ports).await
}
