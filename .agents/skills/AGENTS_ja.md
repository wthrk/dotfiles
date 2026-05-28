# AGENTS_ja.md

## 対象範囲

このファイルは `.agents/skills/` 配下のすべてに適用する。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` と意味的に一致させる。
- `AGENTS.md` を編集する場合は、同一変更で `AGENTS_ja.md` も更新する。
- レビュー時に両文書の意味一致を確認する。

## 重要ルール

- リポジトリ作成の `SKILL.md` は、現在の actor/role を曖昧さのない文章で明示的に拘束しなければならない。
- リポジトリ作成の `SKILL.md` は、ファイルごとに主要プローズ言語を 1 つに固定し、英語と日本語の混在プローズを禁止する。
- 単一言語ルールの例外は、file path・identifier・required upstream terms・exact quoted rule text のみとする。
- リポジトリ作成の `SKILL.md` が外部 governing documents に依存する場合は、ローカル解釈や末尾ルールより前に、top-level の governing-sources/required-reading セクションで依存先を宣言しなければならない。
- skill の参照先を変えた場合は、参照文書と見出しの実在を確認する。
- 他ファイルから依存される skill 内容を変えた場合は、被参照の意味が崩れていないことを確認する。

## スキルファイル作成規則

- `SKILL.md`（front matterおよび本文を含む）は英語で記述すること。
- 詳細な指示・規範的内容は `docs/` 以下の適切なファイルに記録すること。`SKILL.md` はそのファイルへの参照のみを記述し、内容をインラインで再掲してはならない。
- `SKILL.md` を作成・変更した場合、同じ変更内で英語版の正確な日本語訳として `SKILL_ja.md` を作成・更新すること。

## 必須参照

- skill を編集する前に、`docs/README.md` を必ず読む。
- skill がリポジトリのワークフローやタスク規則を参照する場合は、`docs/task-governance/README.md` を必ず読む。
- skill が領域別タスク成果物を参照する場合は、`docs/tasks/README.md` を必ず読む。
