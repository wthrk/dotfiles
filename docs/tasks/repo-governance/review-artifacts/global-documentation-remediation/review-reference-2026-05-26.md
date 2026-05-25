# global-documentation-remediation 参照整合レビュー記録（2026-05-26）

この文書は、作業項目 `ガバナンス文書整合`（`docs/tasks/repo-governance/tasks.md` / root 台帳 `docs/tasks/tasks.md`）に対する 2026-05-26 現行サイクルの **参照整合レビュー担当（文書是正専用）** の独立判定記録である。`AGENTS.md` / `AGENTS_ja.md` の肥大化是正（Codex/exec 役割判定の委譲基準化、および領域固有 secret-recovery 詳細の正本移管）を対象として委譲された。

判定: 不合格

判定要約: 委譲された是正差分が working tree にもコミット履歴にも存在せず、確認記録が主張する変更がすべて実リポジトリ状態と矛盾する。移管先見出しが未作成のため正本複製禁止規則も未充足であり、是正対象の参照整合性を満たさない。

根拠:

- **是正差分が不在**: `git status` / `git diff HEAD` を独立に実行した結果、tracked file の変更は 0 件。working tree でのコミットされていない変更は、untracked な確認記録 `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-26.md` の 1 件のみ。`AGENTS.md`・`AGENTS_ja.md`・`docs/secret-recovery/implementation-guidelines.md` は HEAD と完全一致しており、是正は適用されていない。
- **計画ゲート節が未削除**: 確認記録は `## Critical Planning Gate` / `## 重要な計画ゲート` を削除したと主張するが、`AGENTS.md` 36 行目に `## Critical Planning Gate` が、`AGENTS_ja.md` 36 行目に `## 重要な計画ゲート` が現存する。除去されていない。
- **移管先見出しが未作成（参照整合違反）**: 確認記録は除去規則を `docs/secret-recovery/implementation-guidelines.md` の `## 進捗記録の区分と前進規則` 節へ新設・移管したと主張するが、当該文書に `進捗記録の区分と前進規則` という見出しは存在しない。移管先見出しが存在しないため、これを指すポインターは解決不能（dangling reference）となる。
- **ポインター化が未実施**: 確認記録は `## Applying Document Instructions` / `## 文書指示の実行` 内の secret-recovery 進捗規則ブロックを 1 行のポインターへ置換したと主張するが、`AGENTS.md` 86〜90 行目に進捗規則ブロックがそのまま現存する。`implementation-guidelines` への既存言及（AGENTS.md 38/40/56/74 行、AGENTS_ja.md 38/40/56/74 行）はいずれも従来からある計画依頼向けポインターであり、移管に伴う新規ポインターではない。
- **Codex/exec 役割判定が機構依存のまま**: 委譲趣旨は役割を「委譲」基準で判定する是正だが、`AGENTS.md` 84 行目および `AGENTS_ja.md` 84 行目は依然として実行機構（`running in exec mode` / `exec モードで動作する`、`invoked directly via exec mode` / `exec モードで直接起動された`）を役割判定根拠としており、是正されていない。
- **正本複製禁止規則が未充足**: `docs/docs-governance.md` の参照規則「正本を移す場合は旧記述を削除または参照化し、二重正本を残さない」に照らすと、領域固有詳細は `implementation-guidelines.md` へ単一所有化されておらず、AGENTS 側に残存したままである。単一所有の達成自体が未実施のため、本是正の目的が満たされていない。
- **確認記録が実在しない成果物を参照**: untracked な `confirmation-2026-05-26.md` は、対象差分識別子 `working-tree-current-2026-05-26` および節削除・ポインター化・新設節への移管といった成果物の存在を前提に記述されているが、いずれも実リポジトリ状態に存在しない。確認記録が指す変更が解決不能であり、参照整合上も不整合である。
- **AGENTS 両文書の同期状態**: 現行の committed 状態において `AGENTS.md` と `AGENTS_ja.md` は相互に意味整合している（計画ゲート節・Codex 節・進捗規則ブロックともに対応）。ただしこれは是正前の整合であり、是正未適用であることの裏付けにとどまる。
- **掃引結果**: `docs/` および `.agents/` に対し、除去対象とされた計画ゲート/進捗規則を指す参照は、是正が未適用であるため dangling は発生していない。逆に言えば、是正を適用した場合に移管先見出しを用意せずポインターのみ追加すると dangling が発生する状態にある。

備考（役割境界）: 本記録は判定の返却に限定する。是正の実装・再実行は実装担当 subagent へ委譲すること。本担当は source/governance ファイルの直接編集・台帳更新・コミットを行わない。
