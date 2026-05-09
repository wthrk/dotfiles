//! 公開 CLI、xtask、統合テストで同じ扱いにする必要がある共通処理。
//!
//! 外部コマンドのログ表記、PATH 探索、ホスト名正規化だけを置く。業務ロジックを入れず、
//! 各クレート間で微妙に違うユーティリティが増えるのを防ぐ。

pub mod command;
pub mod host;
pub mod path;

pub type Result<T> = anyhow::Result<T>;
