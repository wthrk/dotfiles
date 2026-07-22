//! `dotfiles secrets` の entrypoint 配線境界。
//!
//! CLI command 定義で parse 済みの入力を application use case へ橋渡しし、
//! composition root が所有する adapter catalog を command ごとの port 引数へ分配する。
//! domain rule と外部 API 翻訳は持たない。

mod dispatch;

use crate::Result;

/// parse 済み command を domain command に変換し、composition が生成した port 群で起動する。
///
/// concrete backend の生成・所有は composition に限り、ここは CLI 入力境界と command mapping だけを担う。
pub(super) async fn run(
    options: super::SecretsOptions,
    runtime: &mut crate::composition::SecretsRuntime,
) -> Result<()> {
    dispatch::dispatch(options, runtime).await
}
