//! `dotfiles secrets` の entrypoint 配線境界。
//!
//! CLI command 定義で parse 済みの入力を application use case へ橋渡しし、起動時に
//! 必要な adapter 所有関係を確定する。domain rule と外部 API 翻訳は持たない。

mod adapter_catalog;
mod dispatch;
mod runtime;

use crate::Result;

/// 実 adapter を生成し、parse 済み command を application use case へ渡す。
pub(super) async fn run(options: super::SecretsOptions) -> Result<()> {
    runtime::run(options).await
}
