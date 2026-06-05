//! `update-history` の application 層。
//!
//! `record`（diff → ノート取得 → LLM 抽出 → サニタイズ → 記録）と `show`（読み出し → 範囲選択 →
//! catch-up 集約 → severity 再算出 → 出力）の use case orchestration を各 `run_*` に 1 つずつ持つ。
//! application は domain rule と port capability の適用順序だけを担い、具体 I/O（プロセス・HTTP・
//! ファイル・端末・JSON）は port 境界の裏（adapter）へ閉じる。

pub(crate) mod run_record;
pub(crate) mod run_show;
