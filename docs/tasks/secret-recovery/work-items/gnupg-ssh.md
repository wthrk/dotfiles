# #14 GPG 復元 / gpg-agent SSH 対応

- 作業種別: `規約適合リファクタリングを伴う機能実装`
- 作業目的: `restore-gpg` と `export-ssh-public-key` の経路を、GPG / SSH の外部依存を境界化した形で実装する。
- 構造完了条件:
  - GPG / SSH の実体依存は adapter / port へ閉じる。
  - `application` は復旧順序だけを持つ。
  - `domain` は鍵リング実装や process I/O へ依存しない。
- 既存実装の流用方針: `外部依存の境界が曖昧な箇所は流用せず再分割する。`
- 規約違反の解消対象:
  - 外部 crypto / SSH 依存の境界漏れ
  - use case 順序と low-level 操作の結合
  - domain のインフラ依存
- レビュー合格条件: `GnuPG / SSH のインフラ依存が隔離され、アーキテクチャ規約違反が残っていないこと。`
