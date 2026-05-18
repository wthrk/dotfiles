//! `dotfiles secrets` application 層が依存する外部 port。
//!
//! device 操作と terminal I/O を用途別 trait に分け、application は command ごとに必要な
//! contract だけへ依存する。

pub(crate) mod device;
pub(crate) mod io;
