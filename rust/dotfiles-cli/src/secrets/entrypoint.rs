//! `dotfiles secrets` の entrypoint 配線境界。
//!
//! CLI command 定義で parse 済みの入力を application use case へ橋渡しし、
//! composition root が所有する adapter catalog を command ごとの port 引数へ分配する。
//! domain rule と外部 API 翻訳は持たない。

mod dispatch;
mod runtime;

use crate::Result;

/// 実 adapter を生成し、parse 済み command を application use case へ渡す。
pub(super) async fn run(options: super::SecretsOptions) -> Result<()> {
    runtime::run(options).await
}
