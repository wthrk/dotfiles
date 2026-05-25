# 運用整合レビュー記録（global-documentation-remediation, 2026-05-26 v2）

この文書は、`docs/tasks/tasks.md` / `docs/tasks/repo-governance/tasks.md` の作業項目 `ガバナンス文書整合` に対する 2026-05-26 現行サイクル（v2）の **運用整合レビュー担当** の独立判定記録である。前サイクル記録（`review-operational-2026-05-26.md`）は差分不在を理由に `不合格` を返したが、本記録はそれを引き継がず、現行の working tree を `git diff HEAD` / `git status` と実ファイル読取りで独立に再検証した結果に基づく。

レビュー対象として提示されたのは、AGENTS.md / AGENTS_ja.md の (1) 役割判定の機構依存欠陥（exec という実行機構で役割を確定する記述）の是正、(2) `## Critical Planning Gate` 節と secret-recovery 進捗規則 inline ブロックの除去、および除去規則の `docs/secret-recovery/implementation-guidelines.md` への単一所有化、である。

判定: 要修正

判定要約: 是正差分は実在し、欠陥1（exec-by-mechanism loophole）・欠陥2（計画ゲート節と進捗規則 inline 重複）の主要懸念はいずれも実コード差分として解消済みで、移管規則も正本に実在し silent drop はない。ただし確認記録（`confirmation-2026-05-26.md`）が「移管先は `## 進捗記録の区分と前進規則` 節を新設」と記載するのに対し、実体は既存 `### 確認` 節への追記であり、当該見出しは正本に存在しない。監査証跡（確認記録）が実ファイル構造と不一致であり、auditability に具体的懸念が残るため `合格` にはできない。

根拠:

- 差分の実在を独立確認（前サイクルの前提失敗は解消）:
  - `git status` は `AGENTS.md` / `AGENTS_ja.md` / `docs/secret-recovery/implementation-guidelines.md` の 3 件を modified として示し、`git diff HEAD` は実際の追加・削除を表示する。前サイクル（`review-operational-2026-05-26.md`）が指摘した「差分不在・確認記録の虚偽」は本サイクルで解消されている。
- 欠陥1（機構が役割を決める欠陥 / exec-bypass loophole）は解消・強制可能:
  - 旧文言「Orchestrator constraints ... do not apply to Codex sessions invoked via `exec`」「An agent tool invoked directly via exec mode is the delegated implementation executor」および日本語対応文は、AGENTS.md / AGENTS_ja.md から実差分として削除済み。`grep` で両文書に残存なし（ヒットは過去レビュー記録の引用のみ）。
  - 現行 AGENTS.md:75 / AGENTS_ja.md:75 は「役割は受け取った委譲で決まり実行機構では決まらない」「`exec` で動作していること自体は免除根拠にならない」「orchestration を要するコマンド（タスク実行・継続・進行・完了系で単一委譲タスクにスコープされないもの）を exec エージェントが受領した場合は active-item 選定・委譲・レビューゲートを遵守する」と role-by-delegation を明記。AGENTS.md:114 / AGENTS_ja.md:114（Codex 節）も同趣旨。
  - `docs/` および `.agents/skills/` を掃引した結果、exec という実行機構を役割免除根拠とする記述は他に検出されず（orchestration スキルの Codex 言及はコミット代行ゲートに関するもので役割免除ではない）。exec 経由のオーケストレーション系コマンドが選定・委譲・ゲートを構造的にバイパスできる経路は残っていない。
  - 正規の orchestrator→Codex scoped-implementation 委譲経路は AGENTS.md:108-114 / AGENTS_ja.md:108-114 に温存されており、scoped 実装委譲を受けた Codex はそのタスクの実装担当として直接実行し再委譲しない旨が維持されている。
- 欠陥2（肥大化・正本移管）は実体として解消・規則の silent drop なし:
  - `## Critical Planning Gate` / `## 重要な計画ゲート` 節は AGENTS.md / AGENTS_ja.md から削除済み（`grep` で残存なし）。
  - secret-recovery 進捗規則 inline ブロックは除去され、AGENTS.md:76 / AGENTS_ja.md:76 に `docs/secret-recovery/implementation-guidelines.md` への単一行ポインターが残置されている。AGENTS 側の `コード差分なし` 言及は当該ポインター文内の参照例にとどまり、規則の inline 再掲ではない。
  - 除去された各規則の正本所在を実ファイル読取りで照合した結果、全規則が `docs/secret-recovery/implementation-guidelines.md` に実在する:
    - 文書修正明示時は直接修正優先 / doc-primary は文書差分主成果物可 → 75 行。
    - 対象コードパスは開始点（呼び出し元/先・共有型・port/adapter・テスト追従可）→ 69 行。
    - executable では主成果物は実コード差分・文書のみで前進主張不可 / doc-only 従属 → 71・91 行。
    - 主成果物が `文書差分` の項目は証跡充足下で文書差分前進可 → 72 行。
    - `コード差分なし` は前進根拠に使えない → 97 行（既存）。
    - 進捗更新で `文書整合` と `実装` を分離し `コード差分なし` を記録 → 98 行（本サイクル新規）。
    - コード差分なし時は `確認`/`レビュー` を `未着手` で停止し `実装状態` を据置き → 99 行（本サイクル新規）。
    - 前進遷移は同一変更セットの前提証跡を要し、欠く更新は無効 → 100 行（本サイクル新規）。
  - すなわち除去規則は単一所有化され、強制可能・監査可能な形で正本に存在する。`docs/docs-governance.md`:34「正本を移す場合は旧記述を削除または参照化し、二重正本を残さない」は満たされている。
- ポインター解決性（参照整合の運用面）は問題なし:
  - AGENTS.md:76 / AGENTS_ja.md:76 のポインターはファイル単位（アンカー断片なし）で `docs/secret-recovery/implementation-guidelines.md` を指す。`implementation-guidelines.md#` 形式の見出しアンカー参照は AGENTS 両文書に存在しない。したがって前サイクル参照整合レビューが警告した「移管先見出し未作成による dangling anchor」は、実装が見出しアンカーを使わずファイルポインターを採用したため発生していない。
- 残存懸念（auditability — `要修正` の根拠）:
  - 確認記録 `confirmation-2026-05-26.md`（17・25〜27 行）は、移管先を `docs/secret-recovery/implementation-guidelines.md` の `## 進捗記録の区分と前進規則` 節として「新設・移管」したと記載する。しかし実差分は当該見出しを新設しておらず、3 規則は既存の `### 確認` 節（98〜100 行）へ追記されている。`進捗記録の区分と前進規則` という見出しは正本のどこにも存在しない（`grep` 不一致、ヒットは確認記録・過去レビュー記録のみ）。規則自体は実在し強制可能であるため silent drop ではないが、確認記録という監査証跡が実ファイル構造を誤記しており、後続の判定・進捗更新がこの記載を信頼すると不一致が連鎖する。証跡と実体の不一致は auditability の具体的懸念であり、`スコープ外`/`運用徹底` に格下げしてはならない。
  - 構造整合の副次的懸念: 99・100 行の規則は `実装状態` / `レビュー` の前進遷移を統制し、追記先の `### 確認` 見出しの主題（確認作業）より広い対象を扱う。規則は正本内で発見可能・強制可能であり致命的ではないが、見出し主題と規則対象範囲の不一致は是正時に整理することが望ましい。
- 是正条件（差戻し事項）:
  1. 確認記録 `confirmation-2026-05-26.md` の「`## 進捗記録の区分と前進規則` 節を新設・移管」という記載を、実体（既存 `### 確認` 節へ 3 規則を追記）と一致するよう是正する。証跡（確認記録）と正本ファイル構造の不一致を解消すること。
  2. 任意（推奨）: 99・100 行が `実装状態`/`レビュー` 遷移まで統制する点を踏まえ、これら遷移規則を主題に合致する見出し（例: 進捗遷移を扱う独立小節）へ再配置するか、`### 確認` 節の主題記述を遷移規則を包含する形に整える。実体としての強制可能性は満たされているため必須ではないが、見出し主題と規則対象の整合を高める。
  3. 上記是正はすべて実装担当へ委譲する。本レビュー担当は判定の返却に限定し、ソース/ガバナンスファイルの直接編集・台帳更新・コミットは行わない。
