# Feature Boundary Design

この文書は、vertical feature module への移行と機械的 boundary linter のためのアーキテクチャ正本である。[Hexagonal Implementation Rules: 層モデル](hexagonal-implementation-rules.md#層モデル) は各層の一般責務を、[review checklist: 着手前設計照合](review-checklist.md#着手前設計照合) はレビュー観点を定める。本書はそれらを置換せず、物理配置、feature 間境界、設計工程、機械検査を一意に定める。

現行の product flow、公開 CLI、secret-recovery の保存先・値・利用者契約を変更しない。秘密回復の意味と lifecycle は [secret-recovery specification: 目的](../secret-recovery/secret-recovery-spec.md#目的)、保存設計は [YubiKey design: 目的と保護境界](../secret-recovery/yubikey-secret-storage-design.md#目的と保護境界) と [BWS design: BWS Secrets](../secret-recovery/bitwarden-personal-vault-design.md#bws-secrets)、保護境界は [secret handling: 守る対象](../secret-recovery/secret-handling.md#守る対象) を正本とする。

## 物理アーキテクチャ

正規の source layout は feature-first である。層名を crate root に横断的 sibling として並べる flat-layer layout、または `application/` 直下の sibling file を唯一の許可形にする規則は採用しない。

```text
src/
  composition/
    bootstrap.rs             # crate::run だけが入る root composition bootstrap
  features/
    yubikey_lifecycle/
    bws_secrets/
    gpg_backup_recovery/
    password_store/
    provisioning_verification/
    command_facade/
    cli_interaction/
    <feature>/
      <feature>.rs           # feature の private assembly と明示した public port export
      entrypoint/            # 外部 command/API 境界
      presentation/          # feature 固有の input/output 表現
      application/           # use case orchestration
      domain/                # feature の意味・不変条件
      ports/                 # feature が所有する contract（public/ を含む）
      adapters/              # port forwarding
      support/               # feature 専用 technical backend
      composition/           # concrete wiring
      tests/                 # feature test support/integration fixture
  foundation/                # feature 語彙を持たない technical primitive
  shared/                    # 複数 feature が versioned public contract として共有する最小値
  tests/                     # crate 横断 integration test
```

secret-recovery の actual feature root は上記七つである。`secret_recovery` のような単一
root に全 layer を置く構成は、feature 間 import を空集合にして見せるだけで縦境界を
機械的に検査できないため禁止する。cross-feature sequence は root composition と
`command_facade` に限り、他 feature は owner feature の `ports/public/` contract だけを
import する。既存の shared/foundation は feature 名を持たない technical primitive 又は
versioned contract だけに限定し、feature implementation の避難先にしてはならない。

### 現在の全 source ownership inventory

この表は P0/P5 の人手照合用 inventory である。機械的な同一の正本は
[`architecture-boundaries.v1.json`](../../rust/dotfiles-secrets/architecture-boundaries.v1.json)
であり、checker は `src/**/*.rs` の全 file をこの表に対応する owner 一件又は明示 exclusion
へ fail-closed で対応付ける。feature module file、inline test、feature-gated source も「所有者なし」の
例外にしない。

| source path | owner | 許可する横方向 import | 明示した境界 |
| --- | --- | --- | --- |
| `composition/bootstrap.rs` | root composition | 各 feature の concrete と command facade entrypoint | `crate::run` から入り invocation DTO を一回構築して start する唯一の wiring |
| `features/command_facade/entrypoint/**` | command facade / entrypoint | 登録済み feature `ports/public`、自身の application-facing contract | command parse/dispatch/exit だけ。composition/concrete は不可 |
| `features/{bws_secrets,gpg_backup_recovery,password_store,provisioning_verification,yubikey_lifecycle}/application/**` | 各 feature / application | 同 feature domain/ports/foundation、登録済み相手 public port | flow と停止条件のみ。SDK/presentation/concrete は不可 |
| `features/*/domain/**` | 各 feature / domain | 同 feature domain、foundation/shared | value/policy/state。SDK/process/port implementation は不可 |
| `features/*/ports/**` | 各 feature / ports | 同 feature domain/foundation/shared | capability contract のみ。`ports/public/**` は versioned public input/value の明示 re-export 専用 |
| `features/*/adapters/**` | 各 feature / adapter | 同 feature ports/domain、責務所有 presentation/support receiver | trait method から receiver operation への exact forwarding のみ |
| `features/*/presentation/**` | 各 feature / presentation | domain/ports/foundation、注入関数 | prompt/schema/formatting/diagnostic だけ。application/SDK/concrete は不可 |
| `features/*/support/**` | 各 feature / support | foundation と技術 crate、technical boundary type | SDK/device/process/FS/codec。identity/flow/prompt は不可 |
| `features/cli_interaction/**` | cli interaction feature | 自身 ports/presentation/support と登録済み public value | command-neutral process I/O と feature I/O receiver。ほかの機能の private layer は不可 |
| `foundation/**` | foundation | foundation と external technical crate | feature/domain/port 語彙を持たない primitive のみ |
| `shared/contracts/**` | shared | foundation と versioned value dependency | 二 feature 以上に登録された value/contract のみ |
| `secrets_internal_test_stub_contract.rs` | explicit test-stub exclusion | feature-gated integration command contract | manifest に reason/owner/expiry を記載し、production layer と混同しない |

この inventory は「glob があるから所有済み」とは扱わない。新 file を作る、path を移す、feature
間の capability を増やす、又は external crate を追加する変更は、P0/P2/P5/P6 で owner、public
contract consumer、layer allowlist、external crate owner、negative fixture を同時に更新しなければ
ならない。該当しない file は checker が `unknown-source` として停止させる。

`foundation/` は標準 library、外部 technical crate、または他の foundation にだけ依存できる。暗号 primitive、zeroize carrier、codec、OS I/O のように、feature 名、command 名、domain policy、port contract を知らないものだけを置く。`shared/` は少なくとも二つの feature が必要とし、owner・versioning・互換性・公開 consumer を design artifact に明記できる value/port contract だけを置く。便利だからという理由で feature code を移してはならない。feature 固有の外部 SDK backend、prompt、domain rule、use-case 順序は `foundation/` と `shared/` に置けない。

feature root は private assembly と、他 feature 用に明示した `ports/public/` contract だけを保持する。feature-local composition は置かない。feature 内 layer、adapter concrete、support concrete、entrypoint invocation detail、test helper は private である。feature A が feature B の capability を必要とするときは、B が export する public port contract にのみ依存し、B の `application`、`domain`、`adapters`、`support`、`composition`、`presentation`、`entrypoint` を直接 import してはならない。A が B の private type を必要とするなら、contract が不足しているため B の owner が public port を設計する。A に B の実装を写すこと、shared に循環回避用 façade を置くことも禁止する。

### Root bootstrap と feature 起動契約

canonical bootstrap module path は `crate::composition::bootstrap`、physical path は `rust/dotfiles-secrets/src/composition/bootstrap.rs` である。crate-root の唯一の root entry route は `crate::run`（又は同じ public command entry の後継）から `crate::composition::bootstrap::start` へ入り、root bootstrap 自身が concrete runtime と entrypoint-owned invocation DTO を構築して `crate::features::command_facade::entrypoint::start(invocation)` を直接呼ぶ。これは `crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` という唯一の起動順序である。feature-local `compose_for_root` seam は置かない。entrypoint が composition を import することは許可しない。

feature entrypoint の唯一の application input boundary は `entrypoint::start(invocation)` である。`invocation` は entrypoint が所有する DTO であり、command 値と application が必要とする port contract reference だけを保持する。`start` は command 値を application input に変換し、選択された `application::run_*` を一回起動して domain/application の result を exit code へ変換する。concrete receiver、composition runtime、adapter/support/presentation concrete、composition module path は DTO の public surface と entrypoint の import に含めない。composition は concrete receiver から DTO を構築して渡すだけであり、application の順序、parse、prompt、formatting、SDK/device/process 操作、error 分類、retry/fallback を持たない。

Rust の `pub(crate)` は crate 全体から到達可能であり、root-only のアクセス制御にはならない。したがって root `start` と feature `entrypoint::start` を `pub(crate)` にしても、consumer feature による import/call、wrapper を経由する間接 call、又は re-export を許可する根拠にはならない。AST linter と ownership manifest は、上記の唯一の root entry route、root composition から command facade entrypoint への直接 call、許可された importer、re-export 禁止、及び wrapper を含む未許可の call path を強制しなければならない。visibility はこの機械検査の補助であって代替ではない。

`lib.rs`、`composition/bootstrap.rs`、feature module file は concrete receiver、adapter/support concrete、entrypoint invocation detail を `pub` 又は re-export してはならない。外部へ出せるものは feature owner が `ports/public/` に定義し versioned public-contract registry に登録した port/value だけである。root bootstrap は concrete construction、invocation DTO construction、entrypoint start だけを所有し、product flow、SDK call、error classification、retry/fallback、parse、prompt、formatting を持たない。

feature 内では `entrypoint -> application/domain/ports`、`presentation -> domain/ports/(注入された foundation function)`、`application -> domain/ports`、`domain -> feature domain と許可済み shared value`、`ports -> domain/shared contract`、`adapters -> ports + domain + support 又は presentation`、`support -> foundation + external crate` を許可する。root `composition` だけが全 feature の concrete を生成して invocation DTO を構築する。root composition 以外が concrete wiring を import してはならない。adapter は責務所有 receiver への単一 forwarding だけである。依存矢印の逆向き、同一 feature 内でも private layer の横断 import、`support -> application/domain/ports`、`domain -> external SDK type` は不許可である。

### feature 内の責務と crate 所有

| 境界 | 所有する責務 | external crate / SDK の所有 | 禁止例 |
| --- | --- | --- | --- |
| entrypoint | command 定義、引数を invocation DTO へ変換、use case 起動、exit code | なし（CLI parser は root/entrypoint boundary の最小利用だけ） | composition concrete を生成する |
| presentation | prompt、TTY/pipe 選択、schema、formatting、安全な diagnostic | command-neutral foundation function を注入して使う | application を呼ぶ、SDK error を分類する |
| application | flow、caller ごとの停止条件、分岐、外部確認 plan | なし | JSON parse、device 選択実装、secret plaintext 操作 |
| domain | value、policy、不変条件、状態遷移、domain failure | SDK 型を持たない。仕様上必要な純粋 crate は design で根拠化 | process/TTY/network を知る |
| ports | capability contract と最小 request/response | なし | command modality、DTO、SDK type、実装 helper |
| adapters | port trait の exact forwarding | なし。receiver は support/presentation 所有 | local state、変換、retry、error mapping |
| support | feature 専用 external backend と technical conversion | SDK、device、process、filesystem、network、codec crate | business identity、一意性、flow、prompt 方針 |
| composition | concrete の生成と injection | concrete crate 利用はここで選ぶだけ | SDK call、parse、policy、retry |

secret-recovery では `ProtectedSecret` の borrow/move を要する SDK、device、crypto call は feature `support/protection` の専用 operation に閉じる。secret name の意味、0/複数件、必須性、停止条件は domain/application が所有する。BWS access token は YubiKey `bitwarden-client-secret` から内部取得し、recovery と `verify-yubikey --all` は master password、session、OTP、PIV PIN、secret argv/env/interactive input を要求しない。この規則は既存の product design を再定義しない。

## 境界カタログ

各行は設計、実装、review、linter が同じ意味で使う全量カタログである。ここでいう「機械」は AST、module resolution、manifest、import/type-use、visibility、既知の method body shape に限る。業務意味を AST の合否に偽装しない。正本列は repository-relative Markdown link と安定した見出し anchor で表し、scope はその行が定める範囲である。

| 境界 | 正本と scope | 許可 edge / 責務 | 機械検査 | 人が直接確認する不変 handoff / review / test の責務 |
| --- | --- | --- | --- | --- |
| vertical feature | [本書: 物理アーキテクチャ](#物理アーキテクチャ) — feature 間 import | feature root -> 相手 feature の public port/value | owner と cross-feature import | capability の必要性、public contract の意味、consumer 登録 |
| horizontal layer | [Hexagonal rules: 層モデル](hexagonal-implementation-rules.md#層モデル) — feature 内依存 | 許可 graph のみ | import/type-use ownership | 各処理の domain/application/port/support 等の責務分類 |
| public surface | [本書: public port/value contract](#public-portvalue-contract) — export | public port/value のみ | visibility/re-export | stable identifier、互換性、deprecation と consumer 影響 |
| adapter | [Hexagonal rules: 層ごとの責務と成果物](hexagonal-implementation-rules.md#層ごとの責務と成果物) — forwarding | 一つの trait method から所有 receiver の一 operation への exact forwarding | known body shape、import、negative fixture | capability の契約妥当性、SDK error の意味 |
| support / protection | [secret handling: Protection 型](../secret-recovery/secret-handling.md#protection-型) — technical I/O | `ProtectedSecret` と port/domain boundary type の technical な入出力・SDK/device/process/codec | ownership、import/type-use、known body shape | identity、requiredness、uniqueness、success/stop、use-case 順序を support が決めないこと |
| composition | [Hexagonal rules: 層ごとの責務と成果物](hexagonal-implementation-rules.md#層ごとの責務と成果物) — wiring | root `crate::composition::bootstrap` だけが concrete を生成し、entrypoint-owned invocation DTO を構築して `command_facade::entrypoint::start` を直接呼ぶ | import/visibility/call owner、bootstrap ownership manifest、call-path/re-export | product flow、policy、順序、retry/fallback を配線へ入れないこと |
| SDK/process error | [docs governance: 外部 SDK / crate の利用根拠](../docs-governance.md#外部-sdk--crate-の利用根拠) — raw error | support の raw-error carrier と domain/application の product failure を分離し、allowlisted context だけを渡す | SDK type/raw diagnostic の禁止 use、carrier/context schema | API 全 error surface、根拠、分類 owner、identity binding、retry/fallback/停止、negative test |
| secret lifecycle | [secret handling: 外部処理境界](../secret-recovery/secret-handling.md#外部処理境界) — secret | protection 内で generate/save/read/use/dispose を閉鎖 | prohibited sink/type-use | lifecycle、zeroize、direct observation、secret sink negative test |
| state and flow | [secret-recovery spec: 停止条件](../secret-recovery/secret-recovery-spec.md#停止条件) — product state | domain が妥当性、application が順序、support が technical mutation | 構造上の owner のみ | caller matrix、state mutation、success/error/failure/cleanup、最終状態観測 |
| test double | [Hexagonal rules: internal backend stub の配置](hexagonal-implementation-rules.md#internal-backend-stub-の配置) — test exception | tests、又は exact compile-time support stub と forwarding adapter | cfg/ownership/body shape、negative fixture | production 非到達、fixture scope、initial/final datastore 観測 |
| linter and CI | [本書: 静的 boundary linter 仕様](#静的-boundary-linter-仕様) — future enforcement | 導入後の fail-closed required check | manifest/schema/parser/rules/fixtures | unknown semantic claim を review/test へ handoff した証跡 |
| documents and references | [docs governance: 参照資料の直接照合](../docs-governance.md#参照資料の直接照合) — evidence | canonical source への link | link/anchor existence のみ | 原文・適用版・scope の直接読了 |

### support の technical I/O と業務判断

support/protection は `ProtectedSecret`、port request/response、domain value を SDK、device、process、filesystem、codec へ渡し、又はそこから返す technical I/O を担ってよい。その値を受け渡すだけでは support 違反ではない。AST は import、type-use、既知の forwarding/body shape、禁止された SDK type の境界漏出を検査する。

一方、AST は値の意味を証明しない。対象の business identity、必須性、0/1/複数件の一意解決、成功又は停止の決定、caller ごとの use-case 順序、reverse control は domain/application の設計・review・flow test が判定する。support がそれらを選んだり、support から application/domain を制御したりする反例は、linter pass でも不合格である。

## Public port/value contract

feature が外部へ export できるのは `ports/public/` の public port/value だけである。feature module file はその明示 export と private assembly だけを持ち、root bootstrap は private wiring とする。adapter/support concrete、entrypoint invocation detail、test helper、composition receiver は re-export してはならない。

各 public port/value は owner feature、stable identifier、契約 version、登録 consumer、互換性方針を versioned public-contract registry に持つ。owner だけが変更でき、breaking change は新 version の並存又は明示 deprecation を先に設計する。consumer registration がない export、owner 外の変更、互換性 test のない version change は failure とする。negative fixture は feature root から concrete re-export、consumer feature から private module import、root bootstrap の public concrete export、consumer feature が bootstrap または feature entrypoint start を import/call、それらを re-export、又は consumer/wrapper 経由で間接的に bootstrap start へ到達する経路を拒否する。

## 設計工程（P0–P7）

実装前に次を順に確定する。gate を満たさない場合は次工程、編集、SDK error 解釈、不可逆 mutation へ進まない。

| 工程 | 成果物と gate | 反例 / 停止条件 |
| --- | --- | --- |
| P0 Scope | feature owner、public surface、source ownership inventory、対象外の明示根拠 | owner 不明、unowned source、shared 化の理由なしなら停止 |
| P1 Product flow | 全 caller の input -> success/error/failure/cleanup、resource の generate/save/read/use/dispose、state mutation と不可逆境界 | caller、一遷移、cleanup、直接観測の欠落なら停止 |
| P2 Contract | domain value、public port、visibility、feature 間 contract、layer placement | 相手 feature private import 又は SDK type を contract に要求したら再設計 |
| P3 External evidence | crate/version、認証・init/use/cleanup、全 error surface、raw-error carrier と product failure の分類根拠を product flow の後に一次資料へ対応付ける | 仕様と SDK が矛盾、error の意味が未定義、carrier/failure owner 又は identity binding が不明なら opaque failure として停止 |
| P4 Security / I/O | secret carrier、stdin/stdout/stderr/log/argv/env/temp、TTY、device swap/concurrency/process/FS/network boundary | secret exposure、PIN/OTP/session を recovery に要求、device ambiguity があれば停止 |
| P5 Physical design | feature-first path、各 file owner、dependency graph、`crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` root entry route、root bootstrap の唯一の direct entrypoint edge、private composition injection、test placement | layer/feature/support/shared のいずれにも責務を説明できない、bootstrap caller/importer/re-export/call path 又は entrypoint-to-application input boundary を一意に定められない、又は concrete が public export になるなら停止 |
| P6 Verification design | registry schema/storage、checker/linter、unit/integration/direct observation、negative test、CI hook、evidence link | positive test だけ、linter exception が曖昧、state の内部観測だけ、又は future artifact を既存 enforcement と偽るなら停止 |
| P7 Pre-edit approval | P0–P6 coverage/counterexample、immutable handoff、baseline/design identity、承認 | 未否定 counterexample、未解消 finding、handoff 非固定なら編集禁止 |

P1 が SDK 調査より先である。全 flow/caller/resource lifecycle/SDK evidence を確定するまで、便利な API、既存実装、局所 test を設計根拠にしない。設計 artifact には P0–P7 の結果を current-cycle 文書として repo に複製せず、承認済み handoff に保持する。

## 静的 boundary linter 仕様

既存の adapter AST check を含む static checker は Rust AST（`syn`）を使用し、parse、source ownership、宣言済み import/re-export、及び宣言済み direct-call shape の判定のいずれかを完了できなければ fail closed にする。token/line/grep heuristic への fallback は許可しない。Rust の任意 macro 展開や動的 dispatch を完全に証明するものではなく、その意味・到達性は不変 handoff と review の対象として残す。以下は実装 artifact と enforcement の契約である。

1. この checker の対象である `rust/dotfiles-secrets/src/**/*.rs` を versioned schema の ownership manifest で列挙し、feature、foundation/shared、root source、feature-gated source の ownership を全件解決する。unknown path、複数 owner、除外されていない generated source は failure にする。crate 外 integration test、build script、macro expansion はこの checker の対象外であり、P6 の test/review coverage に明示して別途検証する。
2. module/path/import/re-export、visibility、trait implementation、external crate root、direct named-call shape を AST から収集し、feature・layer・test owner を付与する。
3. dependency graph、cross-feature public-port-only、private layer import 禁止、composition-only concrete wiring、external crate owner、adapter forwarding-only、support reverse dependency、public visibility を検査する。bootstrap は manifest が定める `crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` の exact direct call/import edge だけを許可する。entrypoint から composition への import/call、consumer feature/library/CLI からの bootstrap 又は feature entrypoint start の import/call、private 又は public re-export、及び named wrapper/helper を経由する未登録 path を拒否する。macro/dynamic-dispatch の到達性を「検査済み」と偽装せず、P5/P6 の review evidence へ残す。linter が semantic な business rule を証明できない部分は必須 design/review evidence として failure 対象にする。
4. test-only exception は exact である。`#[cfg(test)]` の module-private unit test、又は `secrets-internal-test-stub` の compile-time support-owned backend とそれへの forwarding adapter だけを許可する。production build/runtime 非混入、runtime branch なし、production command path 不変、fixture/dummy 値のみ、initial/final datastore 観測だけを全て AST と build target で確認できなければ例外を適用しない。
5. linter 自身には、禁止 import、private re-export、adapter helper/state、support-to-adapter、domain SDK type、cross-feature private import、unknown source、parse failure、偽装した test exception に加え、consumer feature の bootstrap 又は feature entrypoint start の import/call、consumer/library/CLI module からのそれらの re-export、`entrypoint -> composition`、及び wrapper を経由する間接 bootstrap path の negative fixture を置く。各 test は failure message に path/rule/edge を出す。
6. `cargo xtask check static`（又は後継の同一 required check）を実装し、CI の required hook とする。hook は source identity を固定した対象だけを判定し、linter 未実行・skip・warning は成功にしない。review target hash は各対象ファイルの raw file contents bytes に対する SHA-256 であり、path 正規化又は表示用整形を含めない。required check 化は checker、fixtures、CI workflow、developer command が同じ artifact を読む同一 change の完了後であり、それまでは future enforcement である。

構文上検出できない business/product flow、SDK error の意味、secret lifecycle、state semantics は linter の verdict に含めない。各 catalog 行は、該当する immutable handoff の coverage/counterexample、直接観測、reviewer prompt、又は test responsibility へ明示的に handoff する。linter が「検査不能」を成功や skip に変換することは許可しない。

## Ownership manifest、public-contract registry と CI 契約

canonical ownership manifest は `rust/dotfiles-secrets/architecture-boundaries.v1.json` である。`cargo xtask check static` はこれを読み、`.github/workflows/static-checks.yml` の `static checks` が failure を required check として強制する。code manifest を ownership map が実在しない段階で作成してはならない。

検証 runner の責務も境界として固定する。`cargo xtask check static` は fmt、workspace の test target を含まない check/clippy、`bash -n`、workflow、Nix、architecture AST/adapter gate だけを実行し、`cargo test`、internal-stub CLI integration、provision shell fixture を起動してはならない。これらの実行主体は新設した `cargo xtask check test` に限定し、workspace test、internal-stub CLI integration、provision shell fixture をそれぞれ一回だけ実行する。`cargo xtask check all` の合成順序は `static -> test -> zsh -> integration` とし、各 runner は他 runner の責務を再実行しない。Nix package の `doCheck=false` は維持し、package build と test runner を結合しない。CI workflow、nightly bot の status context、README の command graph はこの分離を同じ名前・同じ順序で参照する。

P0 inventory と P5 physical design が承認され、全 source ownership map を実在の source と照合できた後に、linter 導入 change が canonical artifact `rust/dotfiles-secrets/architecture-boundaries.v1.json` を作成する。同 artifact は schema identifier `architecture-boundaries/v1` を明記し、ownership manifest と versioned public-contract registry を一つの schema で保持する。少なくとも source root/path glob、path kind、feature/layer owner、public-contract owner、stable identifier、contract version、registered consumer、compatibility/deprecation、generated の有無と generator、除外理由と承認 owner、更新責務、checker consumer、CI required-check consumer を全件に持つ。schema validation の docs-owned specification が先に必要な場合は、本書のこの節を正本として追加できるが、実在しない code manifest の代用にはしない。

同 schema は bootstrap ownership を省略してはならない。`bootstrap` object は `root_entry { module_path: "crate", source_path: "rust/dotfiles-secrets/src/lib.rs", symbol: "run" }`、`bootstrap_module { module_path: "crate::composition::bootstrap", source_path: "rust/dotfiles-secrets/src/composition/bootstrap.rs", start_symbol: "start" }`、`entrypoint_starts[] { feature: "command_facade", module_path: "crate::features::command_facade::entrypoint", start_symbol: "start", allowed_direct_callers: ["crate::composition::bootstrap"], allowed_importers: ["crate::composition::bootstrap"], allowed_reexporters: [], invocation_boundary_owner: "entrypoint", concrete_public_export: false }` を持つ。root bootstrap の `allowed_direct_edges` は `["crate::run->crate::composition::bootstrap::start", "crate::composition::bootstrap::start->crate::features::command_facade::entrypoint::start"]` と完全一致しなければならない。`module_path` と `source_path` は canonical path、`symbol` は AST item、`allowed_*` は完全 allowlist であり wildcard を許可しない。checker は AST path/use と named-call shape で各 direct edge を manifest と照合し、unknown caller/importer/re-exporter、欠落した root route、余分な named wrapper/indirect path、entrypoint から composition への edge、又は concrete/invocation detail の public exposure を fail closed にする。macro/dynamic-dispatch の到達性はこの checker の証明範囲外であり P5/P6 の review evidence で否定する。`pub(crate)` はこの schema の値を満たす証跡ではない。

その導入 change で checker、fixtures、developer command、CI workflow を同時に作成する。`cargo xtask check static` はその時点から manifest schema validation、source inventory、AST rule、negative fixture を一つの check として実行し、CI は同じ exact command を required check とする。失敗出力は少なくとも `rule=<id> path=<repository-relative-path> owner=<owner> edge=<from->to> remediation=<action>` を一行で示す。manifest update duty は source の追加・移動・生成方法・public contract を変更する change owner にあり、unknown path、multi-owner、schema version 不明、glob 重複、generated declaration 欠落、除外の理由/owner/期限欠落、manifest にない generated source は、その enforcement 導入後に fail closed にする。generated source は generator と再現 command を明記し、解析不能を除外で隠さない。

## SDK/process diagnostic contract

SDK/process の raw error carrier と利用者へ意味づける product failure は別 contract である。support が owner の raw-error carrier は source error chain を技術的に保持してよいが opaque であり、分類、retry/default/success、presentation、CLI exit code を決めない。boundary に渡せる carrier context schema は `operation`、`feature`、固定 `phase`、allowlist 済み non-secret identifier、opaque origin のみである。raw error text、status、APDU、request/response body、secret、secret-derived length/hash/bytes、URL/envelope 本文を port、domain value、presentation schema、stdout/stderr/log に載せてはならない。

domain/application が owner の product failure は、product flow と一次資料で意味を証明できる allowlisted failure category と上記 non-secret context だけから構成する。category は caller、resource/state transition、不可逆境界、SDK operation identity に結び付ける。意味又は許容遷移を一次資料と product design の両方で示せない raw carrier は、別 category へ写像せず `opaque-external-failure` として fail-closed に停止する。unknown carrier に retry、fallback、default、空値化、state mutation、成功扱い、又は根拠のない failure mapping を与えてはならない。

presentation は allowlisted product failure category と context だけを安全な diagnostic schema と CLI exit code へ変換する。raw carrier を inspect、serialize、復元、又は表示しない。exit code の意味は product failure category に一対一で結び、raw SDK status/error text には結び付けない。support は raw error を表示・分類・retry/default/success へ変換しない。

不変 handoff は SDK/process error ごとに、SDK/version/API symbol・一次資料位置、raw carrier owner、carrier context schema、product failure owner/category、caller、resource/state transition、identity binding、presentation/exit mapping、unknown fail-closed path、retry/fallback/default の不許可、直接観測、negative test responsibility、reviewer と承認 gate を lossless に記録する。P3/P4/P6 の evidence は同一 identity に固定し、承認 reviewer が carrier と failure の混同、未根拠 mapping、raw diagnostic の露出なしを確認するまで実装・CI enforcement へ進めない。negative test は raw SDK error/status/APDU、secret-derived value、任意 error text が public error、diagnostic schema、CLI output に到達しないこと、及び unknown carrier が state mutation/retry/default/成功へ進まないことを確認する。

## Staged migration contract

feature-first 化は次の順でのみ進める。各 stage は前 stage の exit evidence がない限り開始せず、旧構造を暗黙に skip しない。

| Stage | transition check | exit / legacy policy |
| --- | --- | --- |
| inventory | 承認済み ownership map が全 source、generated/exclusion、caller/consumer に解決する | unknown/multi-owner は停止。旧 path は `legacy` と明示し consumer を記録する |
| manifest creation and coverage | P0/P5 inventory 照合後に schema artifact、checker、fixture、CI consumer を同じ change で作り、同じ inventory を読む | coverage 不足は停止。artifact 作成前は CI enforcement を主張しない。旧 path の未記載禁止 |
| public surface freeze | public port/value、owner、version、consumer、compatibility baseline を固定する | private/concrete export は削除対象として登録し、新規 public 化禁止 |
| feature extraction | 一 feature の contract と private assembly を移し cross-feature consumer を public port に切替える | old/new の同時実装は one-way forwarding の期限付き例外だけ。二重 policy 禁止 |
| dependency cleanup | old import、re-export、composition reach-through、test helper reach-through をゼロにする | checker と negative fixture が旧 edge を拒否する |
| legacy deletion | old source、manifest legacy entry、temporary compatibility shim を同一 change で削除する | live consumer、compat test、deprecation exit のいずれかが残れば削除禁止 |

new/old coexistence は、artifact 導入後の migration manifest entry に old path、new owner、registered consumers、開始条件、removal condition、expiry を持つ場合だけ許可する。expiry を過ぎた例外、consumer 不明、old/new の相互 import、legacy path の silent skip は、その linter/CI enforcement 導入後に failure とする。

## Reviewer prompt scope

reviewer は linter pass を business correctness の根拠にしてはならない。review prompt は catalog の全行について、(1) machine result、(2) handoff の flow/caller/state/secret/SDK evidence、(3) direct observation 又は test、(4) counterexample を対応付ける。特に support の `ProtectedSecret`/boundary type I/O が technical transfer にとどまり business identity、requiredness、uniqueness、success-stop、use-case order、reverse control を決めないことを確認する。

secret-recovery の review は [secret handling の実装レビュー観点](../secret-recovery/secret-handling.md#実装レビュー観点) と [secret-recovery spec の停止条件](../secret-recovery/secret-recovery-spec.md#停止条件) を scope とし、BWS-only recovery input contract を変えない。branch、current-cycle identity、作業台帳、進捗手順は architecture document に書かず、必要な task procedure は [task-governance workflow](../task-governance/workflow.md#4-基本フロー) のみを参照する。

## Acceptance と counterexamples

次の受入条件は設計変更ごとに immutable handoff の coverage と counterexample で否定する。ここでの反例は linter fixture だけでなく、レビュー又は直接観測で否定する業務反例も含む。

| Requirement | acceptance | mandatory counterexample |
| --- | --- | --- |
| support boundary | technical I/O は許可し、業務決定は domain/application に残る | support が 0/複数件を選ぶ、停止を決める、application を逆制御する |
| public surface | public port/value だけを stable/versioned contract として export | feature root が concrete を re-export、consumer が private module を import |
| linter limit | AST scope と非機械責務の handoff が catalog/CI にある | linter pass を SDK error/state/secret lifecycle 合格として扱う |
| root bootstrap | `crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start` が invocation DTO を構築して起動する一意な route であり、entrypoint は application input だけを起動して concrete を公開しない | root/feature root が concrete/invocation detail を public export、consumer feature が bootstrap 又は feature entrypoint start を import/call、これらを re-export、entrypoint が composition を import、又は wrapper を経由して未許可の bootstrap path を作る |
| manifest / registry | P0/P5 承認後に canonical path/schema、owner/update/consumer、checker、CI を同じ change で維持する | manifest/CI enforcement の欠落、又は ownership map より先に code manifest を作る |
| migration | six stages の transition/exit/legacy policy が全 change にある | old/new 相互 import、期限なし shim、legacy の silent skip |
| check contract | checker crate、fixtures、CI、developer command、failure output が同期する | CI が warning/skip を成功、fixture が旧構造を許可 |
| diagnostics | raw carrier と product failure の owner/category/context/identity/exit mapping が分離され、unknown は fail-closed | raw SDK error/status/APDU/secret-derived value が output/schema に出る、unknown が retry/default/mapping/success へ進む |
| compatibility | owner-only versioning、consumer registration、compat test がある | owner 外 breaking change、consumer 未登録、deprecation 無し削除 |
| references/review | catalog refs は repository-relative anchor link、review scope が全行を覆う | text-only/unstable reference、原文未読で product flow を決める |

## 編集前チェックリスト

- [ ] feature、owner、public port、foundation/shared の該当可否を決めた。
- [ ] 全 caller と resource lifecycle、success/error/failure/cleanup/state mutation/direct observation を表にした。
- [ ] secret-recovery なら仕様、BWS/YubiKey/GPG design、runbook、secret handling を直接読み、BWS-only recovery 契約を保持した。
- [ ] SDK/crate の version、lifecycle、全 error surface を一次資料へ対応付けた。
- [ ] 各 file を entrypoint/presentation/application/domain/ports/adapters/support/composition/tests の一つへ分類し、dependency edge を許可表と照合した。
- [ ] public visibility と feature 間 contract を確認し、private import/re-export がない。
- [ ] external SDK/process/filesystem/network と secret plaintext の所有を support/protection に閉じた。
- [ ] static linter の所有・例外・negative test・CI hook を変更計画へ含めた。
- [ ] root entry route `crate::run -> crate::composition::bootstrap::start -> command_facade::entrypoint::start`、entrypoint から application input への境界、allowed caller/importer/re-exporter/call path、private composition injection、非公開 concrete surface を決めた。`pub(crate)` だけでは root-only にできないことを manifest/linter contract に記録した。
- [ ] raw-error carrier と product failure の owner、allowlisted category/context schema、identity binding、presentation/exit mapping、unknown fail-closed path を handoff に記録した。
- [ ] manifest/registry/checker/CI が未実装なら future artifact と明記し、P0/P5 ownership map 照合後の同一導入 change にした。
- [ ] 正本への参照位置、直接観測、counterexample の否定根拠、明示除外を immutable handoff に記録した。

このチェックリストは実装開始条件を軽減しない。coverage と counterexample の要求は [implementation execution: 全経路閉鎖不変条件](../task-governance/implementation-execution.md#全経路閉鎖不変条件) を正本とする。
