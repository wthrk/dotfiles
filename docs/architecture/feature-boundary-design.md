# Feature Boundary Design

この文書は、feature-first の物理配置と module 境界だけを定める。業務フロー、SDK仕様、secret lifecycle、レビュー工程は各正本を参照する。

## 物理アーキテクチャ

```text
src/
  composition/bootstrap.rs
  features/<feature>/
    application/ domain/ ports/ adapters/ support/
    presentation/ composition/ entrypoint/（必要な feature のみ）
  foundation/ shared/
```

実在する feature root は `yubikey_lifecycle`、`bws_secrets`、`gpg_backup_recovery`、`password_store`、`provisioning_verification`、`cli_interaction`、`command_facade` の7つ。空の `secret_recovery` は作成しない。

## Root bootstrap

root の起動位置は `crate::composition::bootstrap`。唯一の経路は `crate::run -> crate::composition::bootstrap::start -> crate::features::command_facade::entrypoint::start` であり、concrete wiring は bootstrap が所有する。

## feature 内責務

- `application`: use-case の順序と停止条件。
- `domain`: value、policy、不変条件。
- `ports`: capability contract。
- `adapters`: port から support receiver への forwarding。
- `support`: SDK/device/process/filesystem の technical boundary。
- `presentation`: prompt、入力、表示（必要な feature のみ）。
- `entrypoint`: command DTO と dispatch（command facade）。
- `composition`: concrete wiring（root bootstrap）。

## Public port/value

横断して公開できるのは `ports/public/` の登録済み port/value と root bootstrap の起動点だけである。adapter/support concrete、application、domain private type、entrypoint invocation detail は `pub use` しない。その他の実装は `pub(crate)` または private とする。

## 許可 use graph

同一 feature は `application -> domain/ports/foundation`、`adapters -> ports/support`、`support -> foundation/外部SDK`、`presentation -> ports/domain/foundation` の方向だけを許可する。domain は下位 technical layer を参照しない。

feature 間は `command_facade` と root composition の wiring を除き、相手 feature の `ports/public` のみを import する。support/adapters から相手 feature の内部 module を直接 use してはならない。

## 実 module 棚卸しと移行差分

| 現状 | 移行後 |
| --- | --- |
| 7 feature root に分散した実 module | 同じ feature 内で上記層へ分類し、owner manifest と一致させる |
| 空の `secret_recovery` directory | 削除し、参照を作らない |
| command dispatch | `command_facade/entrypoint` |
| concrete runtime wiring | `composition/bootstrap` |

## 単純 linter 規則

AST checker は次だけを fail-closed に検査する。

1. 全 Rust source の owner と feature/layer を解決する。
2. feature 間の `ports/public` 以外の import、内部 module use、support/adapters の横断 private use を拒否する。
3. 公開面外の `pub`/`pub use` と同一 feature の許可 graph 外の use を拒否する。
4. root bootstrap の direct route と未登録 wrapper/re-export を拒否する。

業務意味や SDK error の解釈は [Hexagonal Implementation Rules](hexagonal-implementation-rules.md) と secret-recovery 各正本を参照する。
