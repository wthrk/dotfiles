# repo-global ガバナンス文書整合

- 作業種別: `恒久文書整理`
- 作業目的: リポジトリ全体のガバナンス文書群を、仕様、運用規約、タスク台帳、証跡の責務に沿って整理し、領域外タスクへの混在を解消する。
- 構造完了条件:
  - リポジトリ全体向け文書差分の追跡が area 固有台帳から分離されている。
  - 台帳、作業定義、証跡の導線が `docs/tasks/repo-governance/` で完結している。
  - root active ledger の正本を `docs/tasks/tasks.md` として参照し、active work item から `docs/tasks/repo-governance/*` と `docs/task-governance/*` へ導線が接続されている。
  - 同一変更セットで扱う governed materials（governing source と included documentation targets）の列挙が、実差分スコープと一致している。
- 既存実装の流用方針: `既存の確認・レビュー証跡は破棄せず移設し、差分識別子と担当記録を保持する。`
- 規約違反の解消対象:
  - 領域固有台帳への repo-global 文書差分の混在
  - 追跡責務の不整合によるレビュー循環
- 同一変更セットで扱う文書カテゴリ:
  - governing sources: `docs/tasks/tasks.md`, `docs/task-governance/*`, `docs/tasks/repo-governance/*`
  - included documentation targets: `.agents/skills/AGENTS.md`, `.agents/skills/AGENTS_ja.md`, `.agents/skills/dotfiles-task-governance/SKILL.md`, `.agents/skills/implementation-execution/SKILL.md`, `.agents/skills/implementation-review-judgement/SKILL.md`, `.agents/skills/task-completion-judgement/SKILL.md`, `AGENTS*`, `docs/docs-governance.md`, `docs/tasks/README.md`, `docs/secret-recovery/implementation-guidelines.md`, secret-recovery 配下の移管支援/移管証跡文書
- 現行差分スコープ（working-tree-current-2026-05-22）: total 28 paths（tracked 25 + untracked 3。untracked: `docs/tasks/tasks.md`, `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md`, `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md`）。tracked 側は `.agents/skills/*.md`（6件）, `AGENTS.md`, `AGENTS_ja.md`, `docs/docs-governance.md`, `docs/task-governance/*.md`（6件）, `docs/tasks/README.md`, `docs/tasks/repo-governance/README.md`, `docs/tasks/repo-governance/tasks.md`, `docs/tasks/repo-governance/work-items/global-documentation-remediation.md`, `docs/tasks/repo-governance/review-artifacts/README.md`, `docs/tasks/repo-governance/issue-01-progress.md`, `docs/secret-recovery/implementation-guidelines.md`, `docs/tasks/secret-recovery/{README.md,tasks.md}`, `docs/tasks/secret-recovery/review-artifacts/_review-template.md`
- 現行サイクル証跡（2026-05-22）: `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/{confirmation-2026-05-22.md,review-2026-05-22.md}`
- 境界条件: `docs/secret-recovery/implementation-guidelines.md` は active cross-area documentation target として変更セットに含めるが、repo-governance の判定正本（governing source）には含めない。
- レビュー合格条件: `repo-global 文書整合作業が専用台帳で追跡され、repo-governance が area 固有ガバナンス規約へ依存しないこと。`
