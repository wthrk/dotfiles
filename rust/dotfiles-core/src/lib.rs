//! 公開 CLI、xtask、統合テストで同じ扱いにする必要がある共通処理。
//!
//! 外部コマンドのログ表記、PATH 探索、ホスト名正規化など、複数 crate から同じ不変条件で
//! 扱う必要がある処理を置く。

pub mod command;
pub mod host;
pub mod path;

pub type Result<T> = anyhow::Result<T>;
