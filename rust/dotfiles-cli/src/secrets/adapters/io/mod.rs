//! process / terminal / stdio / report backend を port 契約へ接続する adapter。

mod process_io_adapter;
mod report_adapter;

pub(crate) use process_io_adapter::ProcessIoAdapter;
pub(crate) use report_adapter::JsonReportAdapter;
