# 運用整合レビュー（AGENTS minimal-entry rebuild, 2026-05-26）

判定: 要修正

判定要約: AGENTS.md の縮小と詳細の正本移設は概ね実在・整合しているが、AGENTS.md が「`docs/task-governance/workflow.md`（`2. 役割`・`7. 役割分離`）が所有する」と明記する **transport-agnostic role rule**（直接 exec モードで起動された外部エージェントツールは実装担当として直接実装する／オーケストレーター自己実行禁止に拘束されない、という役割確定規則）が、指し示された正本見出しにも他のどの正本にも存在しない。指し示しが宙吊り（dangling pointer）であり、かつ役割分離上の実運用規則が唯一の所有先から消失しているため、強制可能性・監査可能性に具体的懸念が残る。

根拠:

- **差分の実在確認（合格部分）**: `git diff HEAD -- AGENTS.md` を確認。AGENTS.md は 275 行規模の削減（旧 ~278 行 → 新 ~74 行）で実在する。AGENTS_ja.md も同等に縮小され、新 AGENTS.md と意味的に一致している（役割対応表・オーケストレーター絶対禁止事項・翻訳同期・プロジェクト概要・コミュニケーション・必須参照と正本の各節が対応）。rust/ 配下の変更はスコープ外として判定対象から除外した。
- **移設の着地確認（合格部分）**: 削除された各節が正本側に実在することを各正本を直接読んで確認した。
  - Branches/Commit/PR → `docs/task-governance/workflow.md` `8. ブランチ・コミット・プルリクエスト運用`（lines 122–149、ブランチ運用・コミット運用・プルリクエスト運用の3小節）に着地。README.md 見出しも `8. ...プルリクエスト運用` を反映して更新済み。
  - Architecture Constraints + Code Style → `docs/architecture/hexagonal-implementation-rules.md`。言語別コードスタイル（Rust/Nix/Shell/Lua）が `## 言語別コードスタイル`（lines 144–177、正本宣言付き）に新設され、adapter `pub` 最小化・application の concrete I/O 禁止・層別制約の file-name 規則優越は既存の `## 層と公開範囲`/`## 依存方向`（lines 21–29, 55, 74, 104–120）が AGENTS.md より厳格に既に所有している。comment/doc-comment 規則も同文書（lines 121–139）が正本である旨を明記。
  - Security → `docs/task-governance/security-obligations.md` `## 基本義務`（API トークン・Docker 認証状態・SSH 秘密鍵・アプリセッションファイルの非コミット、Home Manager へのマシン固有可変状態禁止、Homebrew tap 固定）に着地。
  - Instruction Compliance / Testing / external-dotfiles handling → `docs/task-governance/implementation-execution.md` `## 完了・継続義務`・`## 検証選択`・`## ローカル生成物の取り扱い`（lines 53–75）に着地。
  - Setup/Dev Commands → `README.md`（`## 開発環境` line 98 / `## 内部タスク` line 112 / `## 検証` line 126）が所有。AGENTS.md の引用見出し（開発環境 / 内部タスク / 検証）はすべて実在。
  - secret-recovery handling → `docs/secret-recovery/implementation-guidelines.md`（文書整合と実装の分離・`コード差分なし` の前進不可・前提証跡同時更新規則を追記）に着地。
- **引用見出しの実在確認（合格部分）**: AGENTS.md が引用する見出しはすべて実在し正確である。workflow.md の `2. 役割`(line 12)・`6. コミット着手ゲート`(line 81)・`7. 役割分離`(line 93)・`8. ...プルリクエスト運用`(line 122)、README.md の `開発環境`/`内部タスク`/`検証`。引用先 README（`docs/task-governance/README.md`・`docs/secret-recovery/README.md`）も実在。
- **オーケストレーター中核の保持確認（合格部分）**: 役割対応表とオーケストレーター絶対禁止事項（直接編集禁止・実装判定目的の読み取り禁止・テスト/検証実行禁止・自己判定禁止・追加許可請求禁止、許可行為は active-item 選定/fresh subagent 起動/失敗記録の3点）は AGENTS.md / AGENTS_ja.md に正しく残存し、`/orchestration` skill 及び workflow.md `7. 役割分離` でも強制されている。スキルファイルの差分は vendor 名一般化（Codex → subagent）のみで、必須レビュー役割集約規則の強制内容に変更はない。
- **要修正の根拠（dangling pointer かつ規則消失）**: AGENTS.md（および AGENTS_ja.md）の「オーケストレーター役割 — 絶対禁止事項」末尾は、`The detailed role-separation philosophy, delegation obligations, **transport-agnostic role rule**, and failure-handling rules are owned by docs/task-governance/workflow.md (2. 役割, 7. 役割分離); follow that document.` と明記する。しかし指し示された `2. 役割`(lines 12–22) および `7. 役割分離`(lines 93–121) を直接読んだ結果、transport-agnostic role rule は存在しない。旧 AGENTS.md から削除された当該規則（"When an external agent tool running in exec mode receives a task-execution command, it acts as the implementation executor role — not as the orchestrator … An agent tool invoked directly via exec mode is the delegated implementation executor and must perform implementation work directly."）は、`grep -rln` でリポジトリ全体（docs/・.agents/・.claude/・AGENTS*.md、過去レビュー記録を除く）を確認した結果、どの正本にも survive していない。workflow.md line 116 の唯一の `外部 exec モードツール` への言及は「sandbox 制約でコミットできない場合でもレビューゲートを免除しない」というコミット代行規則であり、役割確定規則ではない。
- **強制可能性・監査可能性への影響**: workflow.md `2. 役割` line 20–21 は逆方向の規則（「オーケストレーション進行中、現在の実行者はオーケストレーター役割に拘束され…自己実行してはならない」）のみを所有し、直接起動された exec モードツールに対する例外（実装担当として直接実装すべき）を持たない。このため、(a) 監査者が AGENTS.md のポインタを辿って `2. 役割`/`7. 役割分離` を開いても transport-agnostic role rule を発見できず、単一所有による監査可能性が破綻する。(b) 直接 exec モードで起動された実行ツールがこれら正本のみを読んだ場合、自身をオーケストレーター拘束下にあると誤認し、実装を委譲しようとして「自己実行禁止」と「委譲先が自分しかいない」の矛盾に陥り得る——役割分離の実運用上の強制可能性に具体的懸念が生じる。`docs/docs-governance.md` 参照規則「正本を移す場合は旧記述を削除または参照化し、二重正本を残さない／参照はファイルパスと見出し名で行う」に照らし、見出し名で指し示しながら当該見出しに実体がない状態は不適合である。
- **解消条件（差戻し事項）**: 次のいずれかを満たすこと。(1) transport-agnostic role rule（直接 exec モード起動ツール = 実装担当として直接実装、オーケストレーター自己実行禁止の非適用）を `docs/task-governance/workflow.md` の `2. 役割` または `7. 役割分離` に正本として復元し、AGENTS.md のポインタが指す見出しに実体を持たせる。または (2) 当該規則がもはや governance 上不要と確定するなら、AGENTS.md / AGENTS_ja.md のオーケストレーター禁止事項末尾の `transport-agnostic role rule` という被参照語を削除し、宙吊りポインタを解消する。いずれの是正も実装担当へ委譲すること（本レビュー担当は判定のみで、直接編集・コミットは行わない）。
- なお本記録は finding を伴うため、集約規則に従い `合格` を併記しない。
