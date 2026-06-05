//! `HistoryReportPort` を stdout または任意の writer への text / JSON 出力へ接続する adapter。
//!
//! 集約済み [`HistoryView`] を、重要度連動の text（全体見出し → severity バッジ → アプリ別 version と
//! 変更項目）または生 JSON へ翻訳して出力する。絵文字凡例・category 別グルーピング・破壊的/セキュリティ
//! 先頭という presentation 仕様はこの adapter に閉じる。`text` は信頼境界外の自由文のため、リンク化・実行・
//! エスケープ解釈をせずプレーン文字列としてそのまま出力する（prompt injection 表示契約）。意味づけ（重要度
//! 算出・集約）は domain で済んでおり、本 adapter は表示形式の決定だけを担う。
//!
//! `show` command は stdout へ直接書く [`StdoutHistoryReportAdapter`] を、auto 適用後の要約振り分け
//! （tty なら端末、非 tty なら `pending-summary` ファイル）は同じ render を任意 sink へ流す
//! [`WriterHistoryReportAdapter`] を使う。どちらも同一 render 関数を共有し、表示形式の決定を二重化しない。

use std::io::Write;

use serde::Serialize;

use crate::Result;
use crate::update_history::domain::view::HistoryView;
use crate::update_history::domain::wire::{ChangeCategory, ChangeItem, PackageUpdate, Severity};
use crate::update_history::ports::HistoryReportPort;

/// category 別グルーピング/ソートの安定順（破壊的・セキュリティを先頭に置く）。
const CATEGORY_ORDER: [ChangeCategory; 6] = [
    ChangeCategory::Security,
    ChangeCategory::Breaking,
    ChangeCategory::Deprecation,
    ChangeCategory::DefaultChange,
    ChangeCategory::Feature,
    ChangeCategory::Fix,
];

/// stdout への履歴表示を `HistoryReportPort` 契約へ翻訳する adapter。
pub(in crate::update_history) struct StdoutHistoryReportAdapter;

impl HistoryReportPort for StdoutHistoryReportAdapter {
    fn write_history(&self, view: &HistoryView, json: bool) -> Result<()> {
        println!("{}", render(view, json)?);
        Ok(())
    }
}

/// 任意の `io::Write` sink へ履歴表示を書き出す adapter（`StdoutHistoryReportAdapter` と render を共有）。
///
/// auto 適用後の要約を、tty なら stdout、非 tty なら `pending-summary` ファイルへ振り分けるために使う。
/// 出力先選択（tty 判定）と pending-summary の追記契約は呼び出し側（flat `update` module）の責務であり、
/// 本 adapter は与えられた sink へ同一 render 結果を 1 ブロックとして書き、表示形式は stdout 版と二重化しない。
/// sink は `RefCell` で内部可変に保持する（`HistoryReportPort::write_history` は `&self` 契約のため）。
pub(in crate::update_history) struct WriterHistoryReportAdapter<W> {
    /// 描画結果の書き込み先（pending-summary ファイル・捕捉バッファなど）。
    sink: std::cell::RefCell<W>,
}

impl<W: Write> WriterHistoryReportAdapter<W> {
    /// 書き込み先 sink を束ねた adapter を作る。
    pub(in crate::update_history) fn new(sink: W) -> Self {
        Self {
            sink: std::cell::RefCell::new(sink),
        }
    }
}

impl<W: Write> HistoryReportPort for WriterHistoryReportAdapter<W> {
    fn write_history(&self, view: &HistoryView, json: bool) -> Result<()> {
        // 1 回の show 駆動につき 1 ブロックを書くだけで、adapter 自体は表示状態を持たない。
        writeln!(self.sink.borrow_mut(), "{}", render(view, json)?)?;
        Ok(())
    }
}

/// `json` 指定で生 JSON、未指定で重要度連動 text を組み立てる共有 render。
fn render(view: &HistoryView, json: bool) -> Result<String> {
    if json {
        render_json(view)
    } else {
        Ok(render_text(view))
    }
}

/// 各 severity の見出しバッジ表現（text 表示用）。
fn severity_badge(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "[critical] 🔒",
        Severity::Major => "[major] ⚠️",
        Severity::Minor => "[minor]",
        Severity::None => "[none]",
    }
}

/// 各変更カテゴリの絵文字凡例（表示契約）。
fn category_emoji(category: ChangeCategory) -> &'static str {
    match category {
        ChangeCategory::Security => "🔒",
        ChangeCategory::Breaking => "⚠️",
        ChangeCategory::Deprecation => "🗑️",
        ChangeCategory::DefaultChange => "🔧",
        ChangeCategory::Feature => "✨",
        ChangeCategory::Fix => "🐛",
    }
}

/// 重要度連動の text 表示を組み立てる（全体見出し → severity バッジ → アプリ別 version + 変更項目）。
fn render_text(view: &HistoryView) -> String {
    if view.packages.is_empty() {
        return "更新履歴はありません".to_string();
    }
    let mut lines = Vec::new();
    lines.push(format!(
        "{} {}",
        severity_badge(view.severity),
        view.overall
    ));
    for package in &view.packages {
        lines.push(render_package_heading(package));
        for category in CATEGORY_ORDER {
            for item in items_in_category(&package.change_items, category) {
                lines.push(render_change_item(item));
            }
        }
    }
    lines.join("\n")
}

/// `name old → new`（不在側は `∅`）と任意の notes URL を 1 行で表す。
fn render_package_heading(package: &PackageUpdate) -> String {
    let old = package.old.as_deref().unwrap_or("∅");
    let new = package.new.as_deref().unwrap_or("∅");
    match &package.notes_url {
        Some(url) => format!("  {} {old} → {new} ({url})", package.name),
        None => format!("  {} {old} → {new}", package.name),
    }
}

/// 絵文字付きの変更項目 1 行。`text` はプレーン表示し、`ref` があれば末尾へ付す。
fn render_change_item(item: &ChangeItem) -> String {
    let emoji = category_emoji(item.category);
    match &item.ref_url {
        Some(url) => format!("    {emoji} {} ({url})", item.text),
        None => format!("    {emoji} {}", item.text),
    }
}

/// 指定 category の変更項目を出現順で返す。
fn items_in_category(
    items: &[ChangeItem],
    category: ChangeCategory,
) -> impl Iterator<Item = &ChangeItem> {
    items.iter().filter(move |item| item.category == category)
}

/// 生データ（JSON）表現を組み立てる。`--json` 用。
fn render_json(view: &HistoryView) -> Result<String> {
    // wire の `PackageUpdate` をそのまま JSON 化し、severity/overall を併記する presentation DTO。
    #[derive(Serialize)]
    struct JsonView<'a> {
        severity: &'a Severity,
        overall: &'a str,
        packages: &'a [PackageUpdate],
    }
    let dto = JsonView {
        severity: &view.severity,
        overall: &view.overall,
        packages: &view.packages,
    };
    Ok(serde_json::to_string_pretty(&dto)?)
}

#[cfg(test)]
mod tests {
    //! text 表示の severity バッジ・絵文字・category 順・プレーン text 出力と JSON 直列化を固定する。

    use super::{render_json, render_text};
    use crate::update_history::domain::view::HistoryView;
    use crate::update_history::domain::wire::{
        ChangeCategory, ChangeItem, ChangeKind, PackageUpdate, Severity,
    };

    fn view() -> HistoryView {
        HistoryView {
            packages: vec![PackageUpdate {
                name: "openssl".to_string(),
                old: Some("3.0.0".to_string()),
                new: Some("3.0.1".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                notes_url: Some("https://github.com/openssl/openssl".to_string()),
                change_items: vec![
                    ChangeItem {
                        category: ChangeCategory::Feature,
                        text: "新機能".to_string(),
                        ref_url: None,
                    },
                    ChangeItem {
                        category: ChangeCategory::Security,
                        text: "CVE 修正".to_string(),
                        ref_url: Some("https://github.com/openssl/openssl/pull/1".to_string()),
                    },
                ],
            }],
            severity: Severity::Critical,
            overall: "1アプリ更新: 🔒1 ✨1".to_string(),
        }
    }

    #[test]
    fn text_lists_security_before_feature_with_badges() {
        let rendered = render_text(&view());
        let security_pos = rendered.find("CVE 修正").expect("security line present");
        let feature_pos = rendered.find("新機能").expect("feature line present");
        // 破壊的・セキュリティ先頭の契約: security が feature より前。
        assert!(security_pos < feature_pos);
        assert!(rendered.contains("[critical] 🔒"));
        assert!(rendered.contains("🔒 CVE 修正"));
        assert!(rendered.contains("openssl 3.0.0 → 3.0.1"));
    }

    #[test]
    fn empty_view_reports_no_history() {
        let empty = HistoryView {
            packages: Vec::new(),
            severity: Severity::None,
            overall: "0アプリ更新".to_string(),
        };
        assert_eq!(render_text(&empty), "更新履歴はありません");
    }

    #[test]
    fn json_contains_severity_and_packages() -> crate::Result<()> {
        let rendered = render_json(&view())?;
        assert!(rendered.contains("\"severity\": \"critical\""));
        assert!(rendered.contains("\"name\": \"openssl\""));
        Ok(())
    }

    #[test]
    fn writer_adapter_writes_same_text_to_sink() -> crate::Result<()> {
        // auto 経路は同じ render を任意 sink（pending-summary ファイル等）へ流す。stdout 版と表示形式を共有する。
        use super::WriterHistoryReportAdapter;
        use crate::update_history::ports::HistoryReportPort;

        let mut buffer: Vec<u8> = Vec::new();
        let adapter = WriterHistoryReportAdapter::new(&mut buffer);
        adapter.write_history(&view(), false)?;

        let written = String::from_utf8(buffer)?;
        // sink へ書かれた内容は stdout 版と同一の重要度連動 text（末尾に改行を 1 つ付す）。
        assert_eq!(written, format!("{}\n", render_text(&view())));
        assert!(written.contains("[critical] 🔒"));
        Ok(())
    }
}
