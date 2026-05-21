# AGENTS_ja.md

## 対象範囲

このファイルは `.agents/skills/` 配下のすべてに適用する。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` と意味的に一致させる。
- `AGENTS.md` を編集する場合は、同一変更で `AGENTS_ja.md` も更新する。
- レビュー時に両文書の意味一致を確認する。

## 重要ルール

- skill は薄く保ち、恒久的な規則は `docs/` に置く。
- `docs/` の規範文を skill に重複させない。
- skill の参照先を変えた場合は、参照文書と見出しの実在を確認する。
- 他ファイルから依存される skill 内容を変えた場合は、被参照の意味が崩れていないことを確認する。

## 必須参照

- skill を編集する前に、`docs/README.md` を必ず読む。
- skill がリポジトリのワークフローやタスク規則を参照する場合は、`docs/task-governance/README.md` を必ず読む。
- skill が領域別タスク成果物を参照する場合は、`docs/tasks/README.md` を必ず読む。
