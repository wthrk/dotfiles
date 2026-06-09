# 更新履歴（update-history）

このディレクトリには nightly 自動 bump が記録する更新履歴を `<YYYY-MM>.toml`（月次ファイル）として置く。

## 用途

- `nightly-update.yml` の record job が `dotfiles update-history record ... --out docs/update-history/<YYYY-MM>.toml`
  で、bump で更新された各アプリの version（old→new）と「何が変わったか」の構造化変更リストを 1 エントリ追記する。
- 各マシンは適用済み pin 由来の dotfiles input source 内のこのディレクトリを読み、`dotfiles update-history [show]`
  で更新内容を表示する。

1 ファイルに 1 日複数件入りうる（`at` はエントリ単位の RFC3339 タイムスタンプ）。スキーマは
`rust/dotfiles-cli/src/update_history/wire.rs` を正本とする。

record job は同一実行で `notes-sources.toml`（ノート取得元レジストリ）も同ディレクトリへ書く。これはパッケージ
ごとに「どこからリリースノートを取得したか（取得元 URL + origin: 機械解決 / AI 探索 / 未発見）」を学習・再利用
するためのレジストリで、決定論的にパッケージ名昇順でソートして書き出す（diff 最小化）。次回 record はこれを最優先
参照し、再利用 hit したパッケージは保存取得元を直接 fetch した seed ノートを要約するだけで済むため、AI 探索を
新規/未知/自己修復のみへ限定して OpenAI API（`async-openai`）の呼び出しを逓減させる。`docs/update-history/**` は
nightly が変更してよい許可パス内なので、レジストリも同経路で repo に入り次回 record が参照できる。レジストリ型
（`NotesSourceRegistry` / `NotesSourceEntry`）の正本は `rust/dotfiles-cli/src/update_history/record.rs` とする。

## このディレクトリは nightly が変更してよい数少ないパスである

`docs/update-history/**` は `flake.lock` と並んで、nightly 自動 bump PR が変更してよい許可パスである
（[nightly-update.yml](../../.github/workflows/nightly-update.yml) の open-pr job が同一 run 内で
`dotfiles ci verify-bump-lock` をインライン実行して機械判定する）。`.github/**`・ソースなどそれ以外のパスが
nightly PR に混ざると verify-bump-lock が fail し、`static checks` status が投稿されず無人 auto-merge されない。
