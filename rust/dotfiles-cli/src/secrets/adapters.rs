//! `dotfiles secrets` adapter 層の module 境界。
//!
//! adapter 実装は port grouping に合わせ、`yubikey`、`bw`、`io` へ分割する。この root は
//! entrypoint が必要な port 実装型だけを再公開し、外部技術の翻訳本文や runtime wiring は持たない。

mod bw;
mod io;
mod yubikey;

pub(crate) use bw::BwsClientAdapter;
pub(crate) use io::{JsonReportAdapter, ProcessIoAdapter};
pub(crate) use yubikey::{DeviceSelectionAdapter, StorageAdapter};
