//! `update-history` の domain 層。
//!
//! 更新履歴 TOML の wire/ドメイン型、変更カテゴリからの severity 機械算出、catch-up での
//! アプリ単位集約、overall 機械見出し生成、nix eval 由来 name→version マップの純粋比較を定義する。
//!
//! ここに置くのは「外部実装を差し替えても変わらない業務規則」だけである。TOML ファイル I/O、
//! nix/brew プロセス実行、リリースノート取得、LLM 抽出は port 契約の裏（adapter）の責務であり、
//! domain は `toml` クレートの具体 API へ依存しない（serde derive のみ付与し、encode/decode は adapter）。

pub(crate) mod aggregate;
pub(crate) mod build;
pub(crate) mod commands;
pub(crate) mod diff;
pub(crate) mod selection;
pub(crate) mod severity;
pub(crate) mod validate;
pub(crate) mod view;
pub(crate) mod wire;
