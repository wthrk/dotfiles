//! `show` が表示する集約済み履歴の domain summary 値。
//!
//! catch-up 集約後のアプリ単位更新と、再算出した全体重要度・機械見出しをまとめた表示意図の値である。
//! JSON key 名・絵文字配置・整形などの presentation 仕様は持たず（それは adapter の責務）、
//! 「何を表示するか」の意味だけを domain value として保持する。`record` 側の wire 型を再利用し、
//! 表示専用の重複型を作らない。

use super::wire::{PackageUpdate, Severity};

/// `show` 用に集約済みの履歴ビュー（表示意図の domain summary）。
///
/// `packages` は catch-up 集約済み（old=最古・new=最新、change_item 重複排除済み）の安定順リスト、
/// `severity` / `overall` は集約後集合から再算出した全体重要度・機械見出し。空履歴では `packages` が
/// 空で `severity` は `None` になる。adapter はこの値を text / JSON へ翻訳するだけで、意味を足さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryView {
    /// 集約済みアプリ単位更新（安定順）。
    pub(crate) packages: Vec<PackageUpdate>,
    /// 集約後集合から再算出した全体重要度。
    pub(crate) severity: Severity,
    /// 集約後集合から再算出した機械見出し（例: `3アプリ更新: 🔒1 ✨2`）。
    pub(crate) overall: String,
}
