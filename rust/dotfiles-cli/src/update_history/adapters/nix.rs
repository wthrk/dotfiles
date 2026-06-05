//! `ClosureDiffPort` を `nix store diff-closures` プロセス実行へ接続する adapter。
//!
//! old/new の closure store path を `nix store diff-closures <old> <new>` へ渡して stdout を捕捉し
//! （`process::run_capture`）、その出力テキストを domain の純粋パーサ [`parse_diff_closures`] へ通して
//! [`VersionDelta`] 列へ翻訳する。version 比較規則・差分種別・size token 除去の業務意味は domain rule に
//! 委ね、本 adapter は「プロセス実行と stdout 取得」という外部 I/O 翻訳だけを担う。

use std::ffi::OsString;

use crate::Result;
use crate::process::run_capture;
use crate::update_history::domain::diff::{VersionDelta, parse_diff_closures};
use crate::update_history::ports::ClosureDiffPort;

/// `nix store diff-closures` 実行を `ClosureDiffPort` 契約へ翻訳する adapter。
pub(in crate::update_history) struct NixClosureDiffAdapter;

impl ClosureDiffPort for NixClosureDiffAdapter {
    fn diff_closures(&self, old_closure: &str, new_closure: &str) -> Result<Vec<VersionDelta>> {
        let args = [
            OsString::from("store"),
            OsString::from("diff-closures"),
            OsString::from(old_closure),
            OsString::from(new_closure),
        ];
        let output = run_capture("nix", args)?;
        parse_diff_closures(&output)
    }
}
