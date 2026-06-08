//! `RecordDiagnosticsPort` を stderr への診断ログ出力へ接続する adapter。
//!
//! record use case が集計した縮退・provenance 経路の件数を、無人パイプライン（nightly record job）の CI ログへ
//! 1 行ずつ書き出す境界である。「何を観測させるか」（件数の意味）は application/port が決め、本 adapter は
//! それを **stderr へどう整形して書くか**という presentation/I/O だけを担う（`eprintln!` を application から
//! 排除し、診断出力先を adapter に閉じる）。診断は観測専用であり、出力失敗が record の成否に影響しないよう
//! best-effort（`eprintln!` は失敗を呼び出し側へ返さない）にする。

use crate::update_history::ports::RecordDiagnosticsPort;

/// record の診断サマリを stderr へ書き出す adapter（`RecordDiagnosticsPort` 実装）。
///
/// CI ログ（nightly record job の stderr）に縮退件数・provenance 内訳を残し、token 失効・レート枯渇による
/// version-only 全滅や、どの経路でノートを得たかを後から観測可能にする。状態を持たない zero-sized adapter。
pub(in crate::update_history) struct StderrRecordDiagnosticsAdapter;

impl RecordDiagnosticsPort for StderrRecordDiagnosticsAdapter {
    fn report_budget_skipped(&self, skipped: usize) {
        eprintln!(
            "GitHub Models extract: budget exhausted, {skipped} packages recorded version-only"
        );
    }

    fn report_notes_summary(
        &self,
        summarized: usize,
        version_only: usize,
        registry_hits: usize,
        mechanical_found: usize,
        ai_discovered: usize,
    ) {
        eprintln!("notes: {summarized} packages summarized, {version_only} version-only");
        eprintln!(
            "notes provenance: {registry_hits} registry-reused, {mechanical_found} mechanical, {ai_discovered} ai-discovered"
        );
    }
}
