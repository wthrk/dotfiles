//! entrypoint runtime が所有する実 adapter catalog。
//!
//! この module は adapter concrete の生成と所有だけを担い、CLI dispatch や use case の呼び出し順序を
//! 持たない。各 field は port trait を実装する型であり、application には個別 port として渡す。

use crate::secrets::adapters::{
    bw::BwsClientAdapter,
    io::{JsonReportAdapter, ProcessIoAdapter},
    yubikey::{DeviceSelectionAdapter, StorageAdapter},
};

/// 実 adapter 群を所有し、use case ごとに必要な port 引数へ分配する entrypoint 配線。
pub(super) struct EntrypointPorts {
    pub(super) device: DeviceSelectionAdapter,
    pub(super) spare_device: DeviceSelectionAdapter,
    pub(super) device_pin_policy: DeviceSelectionAdapter,
    pub(super) process_io: ProcessIoAdapter,
    pub(super) storage: StorageAdapter,
    pub(super) report: JsonReportAdapter,
    pub(super) bws_client: BwsClientAdapter,
}

impl EntrypointPorts {
    /// production command path 用の実 adapter catalog を構築する。
    ///
    /// stub backend が有効な integration test でも runtime flag は使わず、adapter 内部の
    /// compile-time feature selection によって同じ catalog 型が port 契約を満たす。
    pub(super) fn production() -> Self {
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
