# 更新履歴（update-history）

nightly 自動 bump が記録する更新履歴を `<YYYY-MM>.toml`（月次ファイル）として置くディレクトリ。各マシンは
適用済み pin 由来の dotfiles input source 内のこのディレクトリを読み、`dotfiles update-history [show]` で更新
内容を表示する。

このファイルは導線のみとし、恒久規約・スキーマ・許可パスの正本は以下を参照する（重複させない）。

- 月次ファイル（`<YYYY-MM>.toml`）/ノート取得元レジストリ（`notes-sources.toml`）のスキーマ正本:
  [`rust/dotfiles-cli/src/update_history/wire.rs`](../../rust/dotfiles-cli/src/update_history/wire.rs)
  および [`record.rs`](../../rust/dotfiles-cli/src/update_history/record.rs)。
- 記録（record）の挙動・レジストリ再利用・要約生成: [`record.rs`](../../rust/dotfiles-cli/src/update_history/record.rs)。
- nightly が記録・bump する workflow と許可パス判定:
  [`.github/workflows/nightly-update.yml`](../../.github/workflows/nightly-update.yml)。
