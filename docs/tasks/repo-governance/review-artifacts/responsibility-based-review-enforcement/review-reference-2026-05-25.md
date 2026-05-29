# 参照整合レビュー記録（責務基準レビュー強制への是正） — 2026-05-25

この記録は、作業項目 `責務基準レビュー強制への是正`（文書是正・文書主成果物）に対する参照整合レビュー担当（文書是正専用）の独立判定である。fresh セッションとして、`git status` / `git diff` および対象ファイルの実体読取りにより全項目を再検証した。過去のレビュー記録・確認記録・実装担当報告は判定の代替にしていない。

判定: 合格

判定要約: 所見なし

根拠:

- 検証範囲（実体確認した変更）: `git status` で確認した working-tree 変更 — modified: `docs/architecture/review-checklist.md`・`.agents/skills/test-review/SKILL.md`・`docs/task-governance/implementation-review-judgement.md`・`.agents/skills/orchestration/SKILL.md`・`.agents/skills/orchestration/SKILL_ja.md`・`.agents/skills/dotfiles-task-governance/SKILL.md`・`.agents/skills/dotfiles-task-governance/SKILL_ja.md`・`.agents/skills/implementation-review-judgement/SKILL.md`・`AGENTS.md`・`AGENTS_ja.md`・`docs/secret-recovery/implementation-guidelines.md`・`docs/task-governance/workflow.md`・`docs/tasks/tasks.md`・`docs/tasks/repo-governance/tasks.md`・`docs/tasks/repo-governance/work-items/README.md`・`docs/tasks/repo-governance/review-artifacts/README.md`、untracked: 新規 `.agents/skills/architectural-consistency-review/SKILL.md`・新規 work-def `docs/tasks/repo-governance/work-items/responsibility-based-review-enforcement.md`・新規 `confirmation.md`。
- 確認した参照種別: リンク・ファイルパス参照・見出しアンカー・定義語の一貫性・SKILL.md frontmatter および Required Reading Order・正本複製禁止。
- 委譲参照の解決: 是正各所（`workflow.md` 7節コミットゲート行、`orchestration/SKILL.md`、`orchestration/SKILL_ja.md`、`dotfiles-task-governance/SKILL.md`、`dotfiles-task-governance/SKILL_ja.md`、`docs/secret-recovery/implementation-guidelines.md` 48行、`AGENTS.md`/`AGENTS_ja.md` の役割表セル）が指す `docs/task-governance/implementation-review-judgement.md` は実在し、委譲先見出し `## 必須レビュー担当` が同文書 19 行に存在する（アンカー解決）。
- 役割名の一貫性: 日本語役割名 `アーキテクチャ整合レビュー担当` は変更全文書で統一して使用（16 箇所）。スキルパス slug `architectural-consistency-review`（4 箇所）は実在ディレクトリ `.agents/skills/architectural-consistency-review/` と一致。英語名 `architectural-consistency review` も整合。揺れなし。
- root-ledger エントリのリンク/アンカー: `docs/tasks/tasks.md` 新規エントリの `作業定義文書`→`repo-governance/work-items/responsibility-based-review-enforcement.md#責務基準レビュー強制への是正` は work-item H1 `# 責務基準レビュー強制への是正` に解決。`確認記録`→`repo-governance/review-artifacts/responsibility-based-review-enforcement/confirmation.md` は実在。`領域台帳/履歴`→`repo-governance/tasks.md#repo-global-ガバナンス文書整合タスク` は area ledger H1 `# repo-global ガバナンス文書整合タスク` に解決。`対象文書パス` 8 件はいずれも repo root から実在。
- area ledger 配線: `docs/tasks/repo-governance/tasks.md` 新規エントリのリンク（`work-items/...#責務基準レビュー強制への是正`、`review-artifacts/responsibility-based-review-enforcement/confirmation.md`）が解決。`work-items/README.md`・`review-artifacts/README.md` の追加導線リンクも実在先に解決し、root/area 間でフィールドが整合。
- 新規スキルの「担当スキルファイル一覧」配線: `.agents/skills/implementation-review-judgement/SKILL.md` の一覧 8 担当（構造・仕様適合・セキュリティ・運用整合・テスト・ドキュメント・アーキテクチャ整合・参照整合）すべてのスキルパスが実在ファイルに解決。`docs/task-governance/implementation-review-judgement.md` の必須レビュー担当集合（実装差分 7 担当）と整合。新規 `architectural-consistency-review/SKILL.md` は `.agents/skills/` 配下の正本から到達可能。
- review-checklist.md のアンカー/内部参照: 新設 `責務基準の判定原則` 内のクロスリンク `[hexagonal-implementation-rules.md の哲学](../../../../architecture/hexagonal-implementation-rules.md#哲学)` は同ディレクトリ実在ファイルの見出し `## 哲学`（9 行）に解決。引用文「visibility はシンボルの見え方を制御するが、そのコードが属すべき層の責務を変えない」は正本 25 行に実在し改変なし。`tests/` 確認手順の語内参照「手順問1」は同文書 `責務基準の判定原則` の定義「問1」（17 行）と整合。
- SKILL.md 固有チェック（新規 `architectural-consistency-review/SKILL.md`）: frontmatter に `name`・`description` が存在しスキルの実態（全体整合判定）と整合。`Required Reading Order` が存在し実行に必要な 5 参照（`docs/README.md`・`docs/task-governance/README.md`・`docs/architecture/hexagonal-implementation-rules.md`・`docs/architecture/review-checklist.md`・`docs/task-governance/implementation-review-judgement.md`）を欠落なく列挙。判定フォーマットは `implementation-review-judgement.md` へ委譲し複製せず、哲学は `hexagonal-implementation-rules.md` を正本参照し再記述しない旨を明記（正本複製禁止に適合）。
- AGENTS.md/AGENTS_ja.md の同期: 両者の役割表セルは意味的に同一（閉じた 4 担当列挙を撤廃し正本「必須レビュー担当」へ委譲）。両ファイルとも `## Translation Synchronization` / `## 翻訳同期` 節を持ち、委譲先パスと見出しが両者で解決。
- confirmation.md（新規）の参照: 記載されたファイルパス・見出しアンカーはいずれも実在先に解決し、新規 broken reference を導入していない。before-state 証跡として引用された旧 4 担当列挙文字列はライブ規則ではなく証跡であり、参照整合の対象規則ではない。
- 検証結論: 変更/新規文書内のリンク・ファイルパス・クロスリファレンス・定義語・アンカーはすべて実在ターゲットへ解決し、定義と使用に不一致なし。参照整合性は維持されている。
