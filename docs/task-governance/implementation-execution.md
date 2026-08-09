# 実装実行規則

この文書は、実装担当が差分を作るときの正本である。

## 実装担当の強制義務

- 親オーケストレーターから実装作業を委譲された時点で、現在の実行者は実装担当である。
- 委譲済みの同じ delegated task について、作業単位の再選定、オーケストレーター役割への切替、追加 subagent への再委譲を行ってはならない。
- 委譲された実装担当は、最初に `.agents/skills/implementation-execution/SKILL.md` を読む。
- 同じ delegated task について `/orchestration` を起動してはならず、`$dotfiles-task-governance` を orchestration、役割変更、作業単位の再選定に使ってはならない。
- `AGENTS.md` や `workflow.md` のオーケストレーター向け指示は、親オーケストレーターが委譲前に満たす条件として読む。

## 着手前参照

実装担当は、委譲内容に応じて次を読む。

- ユーザー指定の GitHub issue / PR / 明示タスク。
- 親オーケストレーターから渡された対象パス、完了条件、差戻し条件、未解消 finding。
- [security-obligations.md](security-obligations.md)。
- コード変更の場合は [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md)。
- secret-recovery が対象の場合は [../secret-recovery/README.md](../secret-recovery/README.md) から必要な仕様・設計・runbook。

## 再読義務

編集前に、次のカテゴリを再読する。

- 変更対象ファイル。
- 変更対象から直接参照される共有型、インターフェース、設定。
- 変更対象の直接呼び出し元と直接呼び出し先。
- 対応テスト。
- 差戻し再実装では、未解消 finding 本文、reviewer role、verdict、file:line references、required fix。

前回読んだ記憶だけで編集してはならない。

## 実装時の判断規則

- 完了条件を満たすために必要な実装を省略してはならない。
- 現行構成と現行アーキテクチャを固定の前提とし、依頼範囲を越える大幅な再構成を実装義務にしない。
- 修正範囲に新規の層違反、責務混在、公開面違反を持ち込んではならない。
- 既存コードの流用可否は、動作有無だけではなく規約適合性で判断する。
- レビュー指摘対応では、指摘箇所だけでなく同一変更セット内の同種欠陥を確認し、見えている未解消欠陥を残さない。

## 削除時の参照追従

- 識別子、step、節、ファイルを削除したら、同じ変更の中でその名前を repo 全体（コード、文書、workflow、コメント、skill、PR 本文）で検索し、参照を残さない。
- 英日で対になる成果物（`SKILL.md` / `SKILL_ja.md`、`AGENTS.md` / `AGENTS_ja.md`）は同じ変更で更新する。
- 削除の代替を説明する記述を新たに書く場合、その記述自体が次の削除対象になっていないかを確認する。

この規則の強制機構は既存のレビュー 4 担当とする。専用の検査やレビュー役割を追加しない。

## 記録義務

実装担当の完了報告または確認記録には、少なくとも次を含める。

- 対象差分識別子。
- 実行コマンドと結果。
- 未実施確認と理由。
- セキュリティ観点の確認結果。
- 実装差分がない場合は、その理由と確認範囲。

repo 内に補助的な confirmation / review artifact を新設しない。

## 検証選択

- 変更対象に関係する検証を選ぶ。
- Markdown のみの変更では、生成文書や記載コマンド検証に関係する場合、または利用者が明示要求した場合を除き、`cargo xtask check` / `cargo xtask check static` を機械的に実行しない。
- コード、Nix、shell、workflow、bootstrap、生成物の変更では既定検証を実行する。
- 検証は dev shell 内で実行する。dev shell 外なら `direnv exec .` を前置する。
- 検証コマンドの一覧と用途は repository root の `README.md` を参照する。

## ローカル生成物の取り扱い

- リポジトリ外の生成済み dotfiles やマシン固有 dotfiles を手編集しない。
- 開発者の実 `~/.config/dotfiles` に書き込んで検証しない。

## 禁止

- 新規に持ち込んだ規約違反を「後で直す前提」で残すこと。
- 再読対象を読まずに実装方針を決めること。
- 委譲済み実装担当が同じ task を再オーケストレーションすること。
- 割り当て作業が許可スコープ内で実際に完了する前に最終応答すること。
