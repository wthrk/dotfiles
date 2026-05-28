# 参照整合レビュー記録（global-documentation-remediation, v2, 2026-05-26）

本記録は `参照整合レビュー担当（文書是正専用）` による独立再レビューの判定記録である。
先行記録（`review-reference-2026-05-26.md`、v1）は是正未適用を理由に `不合格` を返していたが、本セッションでは作業ツリーを `git diff HEAD` / `git status` および各ファイル本文の直接読取りで再検証した。先行記録・確認記録（`confirmation-2026-05-26.md`）の主張は判定の代替にせず、すべて実リポジトリ状態に対して独立に確認した。

## レビュー対象（実差分で確認）

- `AGENTS.md`（修正済み）
- `AGENTS_ja.md`（修正済み）
- `docs/secret-recovery/implementation-guidelines.md`（3 規則を追加）

判定対象スコープ: 参照整合（リンク・パス参照・相互参照・定義の解決可能性と一貫性）、AGENTS 両文書の意味同期、移管先の内部整合、dangling 参照の有無、正本複製禁止規則への適合。

判定: 合格

判定要約: 所見なし。差分は実在し（先行記録が前提とした「是正未適用」状態は解消済み）、ポインター先は実在し、AGENTS 両文書は意味的に同期し、移管 3 規則は内部整合し、dangling 参照は存在せず、領域固有規則の正本単一所有が確立している。

根拠:

- **差分の実在確認**: `git diff HEAD` は `AGENTS.md` / `AGENTS_ja.md` / `docs/secret-recovery/implementation-guidelines.md` の 3 ファイル変更を示す。`grep` により `Critical Planning Gate` / `重要な計画ゲート` は両 AGENTS ファイルで出現 0 件、見出し `## Critical Planning Gate` / `## 重要な計画ゲート` 節は除去済み。secret-recovery 進捗規則の inline ブロックも除去済み（`コード差分なし` / `実装状態` / `文書整合` / `未着手` の AGENTS 内残存は、新設ポインター行 76 が領域文書の統治対象を列挙する語としての言及のみで、規則の再掲ではない）。先行 v1 記録が前提とした「是正未適用」状態は解消されている。
- **(a) ポインター先の解決**: AGENTS.md / AGENTS_ja.md の新設ポインター行 76（「…follow `docs/secret-recovery/implementation-guidelines.md`」「…`docs/secret-recovery/implementation-guidelines.md` に従う」）が指すファイルは実在する（`ls` で確認）。当該ポインターは特定見出しアンカーを引用していないため、見出しアンカー破綻は発生しない。Codex 節（`## Codex`、両ファイル 106 行）と `## Applying Document Instructions` / `## 文書指示の実行`（両ファイル 67 行）も実在する。
- **(b) AGENTS 両文書の意味同期**: 書換えられた Codex/exec 役割判定文（en/ja とも 75 行）は「役割は委譲で決まり実行機構で決まらない」「`exec` 自体は免除根拠でない」「スコープ限定実装委譲を受けた者は実装担当として直接実行・再委譲禁止」「`exec` でオーケストレーション系コマンドを受けた場合は active-item 選定・委譲・レビューゲート遵守・自己実装禁止」という同一命題を述べ、意味的に一致する。ポインター行（76）・文書編集許可行（77）・`## Codex` 節の書換え文（114 行）も en/ja で同一内容を述べる。除去（計画ゲート節・進捗規則 inline ブロック）と移管も両ファイルで対応している。`Translation Synchronization` / `翻訳同期` 節（両ファイル 51〜57 行）自体は今回不変で、相互整合要件を維持している。英語語数差は問題とせず、意味的等価を確認した。
- **(c) 移管 3 規則の内部整合**: `docs/secret-recovery/implementation-guidelines.md` の `### 確認` 配下に 3 規則（98〜100 行：`文書整合`/`実装` 分離と `コード差分なし` 記録、コード差分不在時の `確認`/`レビュー` 停止と `実装状態` 不前進、前提証跡同時更新を条件とする前進遷移の有効性）が追加されている。直前の既存規則（97 行「`コード差分なし` の暫定記録は前進根拠に使えない」）と矛盾せず、相互補強する。これら 3 規則内の参照語（`確認`/`レビュー`/`実装状態`）は同文書および正本群で定義済みの用語で、未定義参照はない。
- **(d) dangling 参照の不在**: `docs/` および `.agents/` 全体を掃引した結果、除去対象（計画ゲート節・進捗規則ブロック）を指す参照は `review-artifacts/` 配下の過去レビュー/確認記録にしか存在せず（それらは是正対象を記述する記録であり live な相互参照ではない）、live な統治文書・スキルからの参照は 0 件。AGENTS 内のセクションアンカー（`AGENTS.md#…`）を外部から引く参照も存在しない。よって除去に伴う dangling 参照は発生していない。
- **(e) 成果物が新たな破綻参照を導入していない**: 変更された 3 文書が依存する参照先（ポインター先ファイル、`## Codex` 等の自文書見出し、implementation-guidelines.md が参照する `secret-recovery-spec.md` / `yubikey-secret-storage-design.md` / `README.md` 等の兄弟ファイル）はいずれも実在する。
- **(f) 正本複製禁止規則（`docs/docs-governance.md`）への適合**: 領域固有の進捗取り扱い規則は `docs/secret-recovery/implementation-guidelines.md` に単一所有され、AGENTS は当該正本へのポインターのみを保持し「ここで再掲・再解釈してはならない」と明示している。同 governance の「正本を移す場合は旧記述を削除または参照化し、二重正本を残さない」要件を満たしており、二重正本は残存しない。
