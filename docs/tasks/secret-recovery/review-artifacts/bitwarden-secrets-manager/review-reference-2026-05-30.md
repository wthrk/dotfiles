# 参照整合レビュー記録（PR #33 / Issue #30 current-cycle 文書差分）2026-05-30

- レビュー担当: 参照整合レビュー担当（文書是正専用）
- 対象リポジトリ/ブランチ: `/Users/ya/works/dotfiles` / `refactor/secrets-structure-issue-30-main`
- 現行 HEAD: `a1a36cc`（作業ツリー clean）
- レビュー対象: 本サイクル文書差分 `git diff 5ff5e54..a1a36cc -- '*.md'`
  - `docs/architecture/hexagonal-implementation-rules.md`
  - `docs/architecture/review-checklist.md`
  - `docs/task-governance/implementation-review-judgement.md`
  - `docs/secret-recovery/bitwarden-secrets-manager-design.md`
  - `.agents/skills/architectural-consistency-review/SKILL.md`・`SKILL_ja.md`
  - `.agents/skills/dotfiles-task-governance/SKILL.md`・`SKILL_ja.md`
  - `.agents/skills/structural-review/SKILL.md`・`SKILL_ja.md`
  - `.agents/skills/test-review/SKILL.md`・`SKILL_ja.md`
  - `docs/task-governance/review-artifacts/outside-ledger-intake.md`
  - `docs/tasks/tasks.md`・`docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md`
  - `docs/tasks/secret-recovery/review-artifacts/bitwarden-secrets-manager/confirmation.md`・`review.md`

判定: 合格
判定要約: 所見なし

根拠:
- 新規見出し `### internal backend stub の配置`（`docs/architecture/hexagonal-implementation-rules.md:74`）が実在し、これを参照する 3 箇所のアンカーリンク `hexagonal-implementation-rules.md#internal-backend-stub-の配置`（`review-checklist.md:177`/`:187`、`bitwarden-secrets-manager-design.md:11`）はすべて解決可能。旧見出し `Rust private module 用 test-only bridge` とその旧アンカー `#rust-private-module-用-test-only-bridge` への残存参照は `docs/` `.agents/` 全体に無し（dangling anchor なし）。
- 例示パス改名 `adapters/io/report.rs`（`hexagonal-implementation-rules.md:177` 付近）は実ファイルとして存在。変更文書内に旧 `piv_io` / `bws_client` を現行ライブパスとして指す記述は残存無し。
- 変更ledger（`docs/tasks/tasks.md`・`docs/tasks/secret-recovery/tasks.md`・work-item・confirmation・review）で現行（非削除）として列挙された対象コードパス（`adapters/bw.rs`、`adapters/bw/internal_stub.rs`、`adapters/io{,/process,/report}.rs`、`adapters/yubikey/{selected_device,device_serial_adapter,storage_adapter}.rs`、`ports/{bw,io,yubikey}.rs`、`domain/bws.rs`、`entrypoint/{dispatch,runtime}.rs`、`src/secrets_internal_test_stub_contract.rs`）は全て実在。`（削除）` 注記付きの `adapters.rs`・`domain/values.rs`・`tests/secrets_internal_stub/...` は実際に不在で、変更追跡 ledger の削除エントリとして一貫しており、参照不整合ではない。
- ledger 間クロスリンクのアンカーが一致: work-item `#13-bitwarden-secrets-manager-クライアント`（h1「#13 Bitwarden Secrets Manager クライアント」）、review `#bitwarden-secrets-manager-レビュー記録`（h1「Bitwarden Secrets Manager レビュー記録」）、area tasks `#新規マシン秘密情報復旧基盤タスク`。design doc の `./secret-handling.md`、`implementation-guidelines.md#...` 等の相対リンク先も実在。
- `secrets-internal-test-stub` feature は `rust/dotfiles-cli/Cargo.toml:20` に定義済みで、設計文書/skill 内の feature 名参照は実態と整合。
- docs-governance 適合: 変更された 4 skill（structural-review・test-review・architectural-consistency-review・dotfiles-task-governance）の追記は、internal backend stub の許可条件を skill 内へ複製せず、正本 `hexagonal-implementation-rules.md` / `review-checklist.md` を参照する形に揃っており、正本複製禁止規則に適合。README/正本の責務混在や二重正本も新規に発生していない。
- SKILL.md frontmatter（`name`/`description`）はいずれも実態と整合し、`Required Reading Order`（dotfiles-task-governance は `Required Reading` ナビゲーション）を保持。今回の差分は本文一文追記または定義文言更新のみで、frontmatter・必須参照の欠落は生じていない。
- 補助記録（confirmation/review/outside-ledger）の自己 hash・current-cycle 文言・exact file-set・台帳間同期の完全一致は、スキル規則に従い不合格根拠とせず、参照解決可能性と定義一貫性のみで判定した。
