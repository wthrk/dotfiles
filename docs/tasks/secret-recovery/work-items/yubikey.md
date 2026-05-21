# #12 YubiKey 秘密情報保存

- 作業種別: `モジュール構造のゼロベース書き換えを含む規約適合リファクタリング`
- 作業目的: `dotfiles secrets yubikey*` と `verify-yubikey` を、現行の動作有無ではなくアーキテクチャ規約への厳密適合を基準に作り直す。責務境界が崩れている箇所を読み直し、モジュール分割、依存方向、入出力境界を再構成すること自体が仕事である。
- 構造完了条件:
  - `CLI` は clap option の型付けと公開 command 名だけを持つ。
  - `application` は use case の順序制御と外部境界呼び出しだけを持つ。
  - `domain` は YubiKey 実機、stdin/stdout、保護メモリ、外部 crate の I/O 型に依存しない。
  - `adapters` は実機 YubiKey と process I/O の接続に限定し、業務判断や use case 順序を持たない。
  - `support` は保護メモリ、補助暗号、割り込み制御などの横断補助だけを持つ。
- 既存実装の流用方針: `既存コードは参照してよいが、責務境界が規約に合わない場合は大幅な再分割、再配置、削除を前提とする。`
- 規約違反の解消対象:
  - `CLI/application/domain/adapters/support` の責務混在
  - use case 順序と low-level storage 操作の結合
  - 実機依存、process I/O 依存、保護メモリ依存の境界漏れ
  - review 時に「動くが構造が規約に合わない」と判定される残存違反
- レビュー合格条件: `アーキテクチャ規約に厳密に適合し、責務境界、依存方向、公開インターフェース境界に違反が残らないこと。`
- 粗粒度進捗注記: `#12` の design PR は `#21` として成立済みであり、現段階の主作業は implementation / code review / validation 面である。
