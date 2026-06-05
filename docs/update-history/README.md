# 更新履歴（update-history）

このディレクトリには nightly 自動 bump が記録する更新履歴を `<YYYY-MM>.toml`（月次ファイル）として置く。

## 用途

- `nightly-update.yml` の record job が `dotfiles update-history record ... --out docs/update-history/<YYYY-MM>.toml`
  で、bump で更新された各アプリの version（old→new）と「何が変わったか」の構造化変更リストを 1 エントリ追記する。
- 各マシンは適用済み pin 由来の dotfiles input source 内のこのディレクトリを読み、`dotfiles update-history [show]`
  で更新内容を表示する。

1 ファイルに 1 日複数件入りうる（`at` はエントリ単位の RFC3339 タイムスタンプ）。スキーマは
`rust/dotfiles-cli/src/update_history/domain/wire.rs` を正本とする。

## このディレクトリは nightly が変更してよい数少ないパスである

`docs/update-history/**` は `flake.lock` と並んで、nightly 自動 bump PR が変更してよい許可パスである
（[nightly-update.yml](../../.github/workflows/nightly-update.yml) の open-pr job が同一 run 内で
`dotfiles ci verify-bump-lock` をインライン実行して機械判定する）。`.github/**`・ソースなどそれ以外のパスが
nightly PR に混ざると verify-bump-lock が fail し、`static checks` status が投稿されず無人 auto-merge されない。
