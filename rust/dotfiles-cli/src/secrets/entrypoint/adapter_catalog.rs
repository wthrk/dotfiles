//! entrypoint runtime が所有する実 adapter catalog。
//!
//! この module は adapter concrete の生成と所有だけを担い、CLI dispatch や use case の呼び出し順序を
//! 持たない。各 field は port trait を実装する型であり、application には個別 port として渡す。

use crate::secrets::adapters;

/// 実 adapter 群を所有し、use case ごとに必要な port 引数へ分配する entrypoint 配線。
pub(super) struct EntrypointPorts {
    pub(super) device: adapters::DeviceSelectionAdapter,
    pub(super) spare_device: adapters::DeviceSelectionAdapter,
    pub(super) device_pin_policy: adapters::DeviceSelectionAdapter,
    pub(super) process_io: adapters::ProcessIoAdapter,
    pub(super) storage: adapters::StorageAdapter,
    pub(super) report: adapters::JsonReportAdapter,
    pub(super) bws_client: adapters::BwsClientAdapter,
}

impl EntrypointPorts {
    /// production command path 用の実 adapter catalog を構築する。
    ///
    /// stub backend が有効な integration test でも runtime flag は使わず、adapter 内部の
    /// compile-time feature selection によって同じ catalog 型が port 契約を満たす。
    pub(super) fn production() -> Self {
        Self {
            device: adapters::DeviceSelectionAdapter::default(),
            spare_device: adapters::DeviceSelectionAdapter::default(),
            device_pin_policy: adapters::DeviceSelectionAdapter::default(),
            process_io: adapters::ProcessIoAdapter::default(),
            storage: adapters::StorageAdapter::default(),
            report: adapters::JsonReportAdapter::default(),
            bws_client: adapters::BwsClientAdapter,
        }
    }
}
