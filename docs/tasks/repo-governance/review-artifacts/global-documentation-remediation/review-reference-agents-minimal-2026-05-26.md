# 参照整合レビュー（AGENTS minimal-entry rebuild, 2026-05-26）

判定: 合格

判定要約: 所見なし。`git diff HEAD` で対象差分の実在を独立に確認した上で、AGENTS.md / AGENTS_ja.md のミニマル入口化に伴う全ポインター・見出し・正本移管・両文書同期・dangling 参照不在・循環参照解消・正本重複禁止を検証し、いずれも整合していることを確認した。

## 検証スコープと差分実在確認

- `git diff HEAD --stat` により対象差分の実在を独立に確認した。本レビューのスコープは文書是正変更セットのみ（`AGENTS.md`、`AGENTS_ja.md`、`docs/architecture/hexagonal-implementation-rules.md`、`docs/secret-recovery/implementation-guidelines.md`、`docs/task-governance/README.md`、`docs/task-governance/implementation-execution.md`、`docs/task-governance/security-obligations.md`、`docs/task-governance/workflow.md`、`.agents/skills/dotfiles-task-governance/SKILL.md`、`.agents/skills/orchestration/SKILL.md`、`.agents/skills/orchestration/SKILL_ja.md`）。`rust/` 配下の6ファイルは無関係な別作業であり、スコープ外として除外した。
- 過去の確認記録・レビュー記録（`confirmation-2026-05-26.md`、`review-reference-2026-05-26-v2.md` 等）は判定の代替にせず、実ファイルと `git diff HEAD` で全項目を独立に再検証した。なお先行 `review-reference-2026-05-26-v2.md` は差分を3ファイルと記述するが、現行の実差分はそれより広く（4正本への内容移管 + 2 README ポインター更新 + 3スキル文言修正を含む）、本判定は現行実差分に基づく。

## 根拠

- **(a) ポインター先と見出しの実在**: AGENTS.md「Required References and Canonical Sources」の全10ポインター先ファイルが実在することを確認（`workflow.md`、`implementation-execution.md`、`implementation-review-judgement.md`、`task-completion-judgement.md`、`progress-judgement.md`、`hexagonal-implementation-rules.md`、`review-checklist.md`、`security-obligations.md`、`implementation-guidelines.md`、`secret-recovery/README.md`、`docs-governance.md`、`README.md`）。引用見出しも実在を確認: `workflow.md` の `2. 役割`（L12）・`7. 役割分離`（L93）、`README.md` の `開発環境`（L98）・`内部タスク`（L112）・`検証`（L126）、`hexagonal-implementation-rules.md` の `言語別コードスタイル`（L139, 移管先として新設済み）、`implementation-review-judgement.md` の `必須レビュー担当`（L19, スキル表・SKILL ポインターが引用）。`task-governance/README.md` のポインター更新（`#タスク運用ワークフロー` は H1 L1 に一致）も整合。
- **(b) AGENTS.md / AGENTS_ja.md の意味同期**: 両文書とも同一の7節構成・同一順序（入口説明 / Role-to-Skill Binding / Orchestrator Absolute Prohibitions / Translation Synchronization / Project Overview / Communication / Required References and Canonical Sources）。ポインター10項目が引用先・見出しともに1対1対応。導線文・翻訳同期節も意味的に等価。語数差はあるが意味的同期は成立。
- **(c) 移管先正本の内部整合**: `workflow.md` に `8. ブランチ・コミット・プルリクエスト運用`（ブランチ/コミット/PR 運用）が新設され、節番号繰り下げ（旧 `8. 関連文書` → `9. 関連文書`）も整合。`implementation-execution.md` に `完了・継続義務`・`検証選択`・`ローカル生成物の取り扱い` が新設され、AGENTS ポインター記述と対応。`security-obligations.md` に API トークン/Docker 認証/SSH 秘密鍵/マシン固有状態/Homebrew tap 固定が追記され、AGENTS ポインター記述と対応。`hexagonal-implementation-rules.md` に `言語別コードスタイル`（Rust/Nix/Shell/Lua）が新設され、AGENTS ポインター記述と対応。
- **(d) dangling 参照の不在**: `docs/` / `.agents/` / `README.md` を掃引し、AGENTS.md / AGENTS_ja.md のセクションアンカー（`AGENTS.md#…`）を引く live 参照は0件。削除された旧節（Critical Planning Gate / Code Style / Architecture Constraints / Commit Rules / Development Commands 等）を指す live 参照も0件。一致したのは `review-artifacts/` 配下の過去レビュー/確認記録と `docs/task-governance/review-artifacts/outside-ledger-intake.md` L63 の日付付き実装確認記録のみで、これらは過去状態を記述する append-only 監査記録であり live な相互参照ではない。よって除去に伴う dangling は発生していない。
- **(e) comment-rules 正本方向の整合**: 旧 `hexagonal-implementation-rules.md` の `この文書の comment / doc comment 規則は [AGENTS.md](../../AGENTS.md) の Code Style コメント規則を継承し…` という後方参照は削除済み。代わりに同節は「この comment / doc comment 規則はリポジトリ共通のコメント規約の正本」と明記し、AGENTS.md はコメント/コードスタイル規則について `hexagonal-implementation-rules.md` を指す。方向が一本化され循環・dangling は解消。同文書に残る `継承`（L173）は Lua/Neovim の継承構造温存禁止規則であり、旧後方参照とは無関係。
- **(f) 正本重複禁止の遵守**: AGENTS.md / AGENTS_ja.md からコードスタイル（Rust 2024 / unsafe / Conventional Commits 等）・セキュリティ・コミット規約・開発コマンドの本文は完全に削除され（grep 残存0件）、各正本へ単一移管された。`docs/docs-governance.md` の正本規則・参照規則に適合し、二重正本は残っていない。secret-recovery 進捗規則は `implementation-guidelines.md`（`確認` 節 L98-100）に領域固有規則として収載され、AGENTS の secret-recovery ポインターが同文書を指す。`implementation-execution.md` の一般的「コード差分なし」記録規則とは粒度（汎用 vs secret-recovery 固有の前進遷移ゲート）が異なり、同一事実の二重正本ではない。
- **同一セッション先行是正の温存**: 役割表が実行機構非依存（"Codex" 固有のロール束縛なし）で委譲透送に依存しないこと、`Critical Planning Gate` 節が両文書から除去済みであること、オーケストレーター禁止事項が実行機構非依存の役割規則について `workflow.md` を正本参照することを確認。先行是正は維持されている。

## 軽微な観察（非ブロッキング）

- AGENTS.md / AGENTS_ja.md の `README.md` ポインターは括弧内例示として `nix develop` を挙げるが、`README.md` の `開発環境` 節には `direnv allow .` のみが記載され `nix develop` は逐語的に存在しない。ただしこれは見出し/アンカー参照ではなくポインター内の補足例示であり、`direnv allow .` は同一 devShell をロードするため参照破綻ではない。整合上のブロッキング要因にはあたらない。
