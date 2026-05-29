//! secrets adapter 層の公開境界。
//!
//! backend grouping に対応した adapter module だけを宣言し、entrypoint へは port trait を実装する
//! adapter 型のみを公開する。stub backend は compile-time feature でのみ接続される。

mod bw;
mod io;
mod yubikey;

pub(crate) use bw::BwsClientAdapter;
pub(crate) use io::{JsonReportAdapter, ProcessIoAdapter};
pub(crate) use yubikey::{DeviceSelectionAdapter, StorageAdapter};
