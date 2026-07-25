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
- 外部 SDK / crate を利用、変更、またはその失敗時挙動を扱う場合は、[../docs-governance.md の外部 SDK / crate の利用根拠](../docs-governance.md#外部-sdk--crate-の利用根拠)。
- コード変更の場合は [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md) と [../architecture/review-checklist.md](../architecture/review-checklist.md)。
- secret-recovery が対象の場合は [../secret-recovery/README.md](../secret-recovery/README.md) から必要な仕様・設計・runbook。

### 基本設計から SDK 利用へ進む必須順序

実装・テスト・外部 SDK 調査の前に、作業単位の issue / PR / 明示タスクと対象領域の正本仕様・基本設計・runbook を直接読み、目的、storage target、各 secret / credential の generate → save → read → use → dispose、前提・成功・失敗時の状態遷移、利用者入力と出力、禁止操作を再構成する。secret-recovery では最低限 `../secret-recovery/README.md`、`secret-recovery-spec.md`、対象に応じた `bitwarden-personal-vault-design.md`、`yubikey-secret-storage-design.md`、`gnupg-ssh-design.md`、`initial-provisioning-runbook.md`、`secret-handling.md` を読む。

その後に限り、正本で確定した操作を実現する手段として vendor / SDK の全体フロー、仕様、公式サンプル、API documentation、versioned source を読む。SDK 資料は product の目的、保存対象、状態遷移、禁止事項を置換しない。正本と SDK が矛盾する、または正本が必要な遷移を定めない場合は、推測実装・テスト・エラー分類をせず設計判断を要求する。詳細な一次資料の直接照合は [../docs-governance.md の基本設計から SDK 利用へ進む順序](../docs-governance.md#基本設計から-sdk-利用へ進む順序) を正本とする。

## 不変 implementation handoff と Design artifact

repository-authored の成果物（code、文書、skill、設定、script を含む）を変更する作業単位は、実装開始前に一つの**不変 implementation handoff**として承認済み design artifact を持たなければならない。handoff は作業単位の正本である外部 issue / PR、利用者の明示タスク、または委譲入力に置き、repo 内へ current-cycle artifact、review artifact、進捗台帳として新設しない。委譲入力と実装担当の報告は、handoff の URL、メッセージ識別子、または再読可能な所在、承認者、承認、適用する baseline/design identity を特定する。

handoff は、実装担当へ渡した後から S1 の実装完了自己照合まで内容を追加・要約・差替えしてはならない。要件、設計、finding、対象 source が変わった場合は、その場で既存 handoff を補修して編集を続けず、変更をすべて織り込んだ新しい handoff を作り、承認を記録し、実装担当が最初から再読する。未固定 handoff、要約だけの修正依頼、以前の会話の記憶、途中 reviewer の finding は編集指示にならない。

handoff/design artifact は次を一つの end-to-end 設計・実装指示として lossless に含む。個々の項目を「既存資料を参照」とだけ書くことは足りず、参照先と該当位置を特定する。

- 最上位 objective、完了条件、利用者が入口から終了までに行う操作と受け取る結果を含む E2E flow。
- 正本仕様・基本設計・runbook・architecture 規約・issue/PR の canonical reference と該当 section / line。
- 承認済み design の identity、承認者と承認、下記「比較 identity」で定義する baseline/design identity と S1 完了 review-candidate identity、それぞれの比較範囲と取得時点。
- 既存 review / 調査からの未解消 finding **全件**について、reviewer role、verdict、finding 本文、file:line、required fix、相互依存を欠落なく列挙した remediation 入力。
- 各 resource / secret の generate、save、read、use、dispose、利用者 input/output、状態 mutation、不可逆境界。
- 各 flow の success、error、failure、cleanup、caller、state mutation、test、直接観測、document evidence の対応。文書主成果物では、文書 flow、利用者・役割、参照経路、必要証跡、明示除外を同じ coverage と counterexample に必須項目として含める。SDK、layer、device、lifecycle その他の項目を非適用とする場合は、理由と正本根拠を添えた明示除外だけを許し、適用項目は省略してはならない。
- 同じ capability・state・外部作用を持つ caller を漏れなく列挙する caller matrix と、各 caller の適用可否。
- domain、application、port、adapter、support、presentation、composition、entrypoint の layer ownership、依存方向、入力境界、adapter forwarding の判定。
- device、reader、device swap、同時実行を含む外部状態。
- 固定した SDK / crate version ごとの vendor 全体 flow、認証・初期化・終了・cleanup、呼出 API の全 error surface、公式一次資料 URL と API symbol / specification section / versioned source location。
- acceptance criteria、accept/reject gate、各 gate を判定する secret を出さない直接観測。
- [architecture review checklist の着手前設計照合](../architecture/review-checklist.md#着手前設計照合) に従い、必須 reviewer checklist の全観点へ適用した counterexample checklist と各反例の否定根拠。

項目を複数文書へ分ける場合も、artifact の入口から全項目へ到達でき、適用版と相互関係を一意に特定できなければならない。

### 比較 identity

この節は source identity の正本である。repository-authored source は、tracked source と、`.gitignore` に一致しない untracked source だけを指す。ignored の実行生成物（例: `cargo target`）およびその他の ignored path は source identity から明示的に除外する。実行環境、cache、toolchain、生成物の観測は source identity と混同せず、必要なら検証時の execution/cache identity として別記録にする。

取得時点を UTC で記録し、NUL 区切りの次の決定的な byte stream を SHA-256 に掛ける。値に NUL を含めない Git ref、branch、digest は UTF-8 で、path は Git が返す raw path bytes で記録する。

1. `source-identity-v2\0`、`head\0<git rev-parse HEAD>\0`、`branch\0<git branch --show-current>\0`。
2. `status-porcelain-v1-source-sha256\0<SHA-256(git status --porcelain=v1 -uall -z)>\0`、`staged-binary-diff-against-head-sha256\0<SHA-256(git diff --binary --cached HEAD)>\0`、`unstaged-binary-diff-sha256\0<SHA-256(git diff --binary)>\0`。status は ignored path を含めない Git の通常の source 状態として扱う。
3. `untracked-source-manifest-sha256\0<SHA-256(manifest)>\0`。manifest は `git ls-files --others --exclude-standard -z` の順序で、各 path について `untracked-path\0<path>\0path-kind\0<file|directory|symlink>\0content-sha256\0<SHA-256(file bytes; directory は empty bytes)>\0` を連結する。
4. 上記 header の後へ同じ manifest record 群を連結した全 byte stream の SHA-256 を `source_identity_sha256` とする。

開始時には baseline/design identity を取得し、base/head、branch、status、staged/unstaged range、untracked-source manifest、`source_identity_sha256`、比較コマンド、取得時点を handoff 報告へ記録する。これは編集開始前の design の比較基準であり、S1 で承認済み handoff に沿って行う意図済み編集によって失効しない。S1 自己照合の完了時には同じ全値を review-candidate identity として改めて固定する。S2、S3、S4 は baseline ではなく review-candidate の全値を同一形式で比較する。review-candidate 固定後に source、index、worktree、比較範囲または取得方法が変われば、既存の coverage、counterexample、自己照合、検証結果を根拠にしてはならず、S1 へ戻って新しい review-candidate を作る。

## 実装開始条件

repository-authored の成果物を編集する前に、実装担当は委譲入力に記録された不変 implementation handoff/design artifact の所在、identity、承認、baseline/design identity、取得時点を報告し、全行を直接再読する。handoff が存在しない、未承認、未固定、必須項目が欠ける、現在の作業範囲と適用版を特定できない場合は、編集、review、commit、push、重い検証を開始してはならない。S1 の意図済み編集は baseline/design identity を失効させず、review-candidate 固定後の source、index、worktree 変更だけが S1 への差戻し条件である。

### 全経路閉鎖不変条件

利用者が明示して禁止した「穴が開く方法で実装すること」および「その方法で実装された差分を受け入れること」を、実装、自己照合、review、集約、完了判定、commit へ共通して適用する。この文書では**穴**を、変更が影響する end-to-end flow、境界、失敗、cleanup、caller、state mutation、test / 直接観測、または一次資料根拠のいずれかを未照合のまま、局所的な修正・確認・判定だけで受入れ可能にしてしまう状態と定義する。

実装開始前に、実装担当は全 flow と全境界について、success、error、failure、cleanup、caller、state mutation、test / 直接観測、document evidence、一次資料を一つの coverage と counterexample へ対応付けなければならない。文書主成果物では文書 flow、役割、参照経路、必要証跡、明示除外も必須であり、SDK、layer、device、lifecycle その他の非適用は理由と正本根拠を持つ明示除外以外では許さない。局所修正の設計、変更箇所だけの確認、または一部 caller の列挙では編集を開始できない。counterexample は、各影響経路が閉じていない場合に何が誤って受入れられるかを示し、正本根拠と直接観測で否定する。

新しい変更は、既存の不変 handoff に統合した coverage と counterexample が全影響を閉じ、未確認行・未否定反例・未解消 finding を残さないまで review に進めない。変更を「一括」「包括的」等と呼ぶこと、変更件数を減らすこと、局所 build / test の一部通過、既存実装との類似は、この不変条件の充足または証跡の代替にならない。

その後、実装担当は予定する設計へ、対象差分に適用される必須 reviewer のチェックリストをすべて適用する。適用時は [architecture review checklist の着手前設計照合](../architecture/review-checklist.md#着手前設計照合) に従い、成功、失敗、cleanup、全 caller、不可逆境界、外部状態、一次資料根拠について反例を構成して、予定する責務配置・依存方向・入力 port・adapter forwarding と照合する。

反例を一つでも否定できない、必要な caller または外部状態を列挙できない、不可逆操作より前の停止条件を示せない、または SDK 利用根拠を一次資料へ対応付けられない場合は編集を開始してはならない。設計を修正して同じ照合をやり直すか、正本設計が不足する場合は設計判断を要求する。S2 の事後レビュー、build、test、既存実装、実機観測はこの開始条件の代替にならない。

この照合は実装担当の応答または handoff に、design artifact の所在、identity、承認、baseline/design identity と取得時点、S1 完了 review-candidate identity、全行読了、適用した reviewer checklist、counterexample checklist、coverage table、未解決事項を最小記録として残し、repo 内に current-cycle の表や review artifact を新設しない。coverage table は handoff の必須項目ごとに「参照した根拠、変更箇所、caller、test / 直接観測、判定」を対応付け、文書主成果物では文書 flow、役割、参照経路、必要証跡、明示除外も同じ行へ記す。未確認の行を残さない。

実装担当は、coverage table が全行を満たし、未解決 finding がなく、全経路閉鎖不変条件に必要な counterexample を否定し、S1 の自己照合を終えるまで、S2 review、commit、push、完了報告を起動または依頼してはならない。途中で reviewer finding が届いた場合も、固定された S2 の worktree を変更しない。集約後にのみ、全 finding を新しい不変 handoff へ lossless に統合して次の S1 を開始する。

## 再読義務

編集前に、次のカテゴリを再読する。

- 変更対象ファイル。
- 変更対象から直接参照される共有型、インターフェース、設定。
- 変更対象の直接呼び出し元と直接呼び出し先。
- 対応テスト。
- 外部 SDK / crate を扱う場合は、使用 API、利用フロー、成功・失敗時の状態遷移、および呼び出し API の全エラー面に対応する公式一次資料。URL と API symbol / 仕様節 / source location を特定し、資料が個別の意味を定義しないエラーは opaque な失敗として伝播し、未確認の意味付けを実装しない。
- 基本設計から再構成した全 flow を、変更後の code / test / doc comment と直接照合する。各 resource / secret について generate、save、read、use、dispose と、その入力、出力、failure、cleanup が正本の順序・禁止事項を満たすことを確認する。
- 差戻し再実装では、未解消 finding 本文、reviewer role、verdict、file:line references、required fix。

前回読んだ記憶だけで編集してはならない。

S1 の通常確認として、実装担当は完了報告の直前に、task・領域の正本仕様/基本設計/runbook・適用 architecture 規約・差分の根拠資料・変更後の全差分と直接の呼び出し元/先・対応テスト・verification の結果/未実施項目を直接再読し、handoff の coverage table を最終化する。この自己照合では全経路閉鎖不変条件の各 coverage 行と counterexample を変更後の根拠へ再対応付け、局所的な pass が残る穴を隠していないことを確認する。外部 SDK / crate を扱う場合は、[../docs-governance.md の参照資料の直接照合](../docs-governance.md#参照資料の直接照合) と [外部 SDK / crate の利用根拠](../docs-governance.md#外部-sdk--crate-の利用根拠) に従い、利用 flow と全 error 面の一次資料を確認する。引用した URL、API symbol、仕様節、source location は reviewer が原文へ辿れる形で残す。

この確認で未読、未確認、未解決 finding があれば S1 内で remediation し、完了報告を行わない。これは追加 stage や独立 gate ではなく、必須の独立 reviewer と [implementation-review-judgement.md](implementation-review-judgement.md) の review/集約/remediation を代替・緩和しない。

## 実装時の判断規則

- 完了条件を満たすために必要な実装を省略してはならない。
- 現行構成と現行アーキテクチャを固定の前提とし、依頼範囲を越える大幅な再構成を実装義務にしない。
- 修正範囲に新規の層違反、責務混在、公開面違反を持ち込んではならない。
- 既存コードの流用可否は、動作有無だけではなく規約適合性で判断する。
- レビュー指摘対応では、指摘箇所だけでなく同一変更セット内の同種欠陥を確認し、見えている未解消欠陥を残さない。

## 記録義務

実装担当の完了報告または確認記録には、少なくとも次を含める。

- 対象差分識別子。
- 実行コマンドと結果。
- 未実施確認と理由。
- セキュリティ観点の確認結果。
- S1 通常確認として直接再読した対象、対象ごとの判定、未解決 finding（なければ「なし」）。
- 不変 implementation handoff/design artifact の所在、identity、承認、baseline/design identity と取得時点、S1 完了 review-candidate identity、全行読了、適用した reviewer checklist、着手前 counterexample checklist、最終 coverage table。
- 外部 SDK / crate を扱った場合は、確認した一次資料の URL と位置、および各利用フロー・エラー判断への対応。
- 実装差分がない場合は、その理由と確認範囲。

repo 内に補助的な confirmation / review artifact を新設しない。

## 検証選択

- 変更対象に関係する検証を選ぶ。
- S1 の修正と自己照合が未完了の間は、全 workspace test、全 feature matrix、Nix package build などの重いまたは全体検証を実行してはならない。まず handoff coverage の未充足、未解消 finding、対象変更の局所検証を解消する。重い検証は S1 完了後に、accept/reject gate として必要なものだけを実行する。
- 不変 handoff の coverage table 全行、実装、文書、self-reconciliation が未完了の間は、局所 `cargo test` / `cargo check`、build、formatter も起動してはならない。未完差分ごとに compile を繰り返して incremental lock と review 対象の source identity を不安定にすることを防ぐため、対象検証は S1 完了後に固定した差分へ一回だけ実行する。構文・型・format の途中確認を理由にこの順序を崩してはならない。
- 最終検証の開始時に diff identity を記録し、formatter、check、test、script を含む最終検証が完了するまで repository-authored source を変更してはならない。検証中に source、生成物、format、index、worktree が変わった場合、その実行結果は対象差分の根拠に採用せず、S1 の自己照合へ戻る。finding または検証失敗への修正は、全検証プロセスを停止・終了してから一括で行い、新しい固定差分から最終検証を再開する。
- 最終検証は Nix dev shell 内で、accept/reject に必要な**一つの command**だけを固定差分へ実行する。開始前に command と `CARGO_TARGET_DIR`、`RUSTFLAGS`（未設定ならその事実）、Rust toolchain、Cargo profile、対象 package、features を execution/cache identity として記録し、終了まで変更してはならない。`/tmp` 等への `CARGO_TARGET_DIR` 分離、別 target directory、別 `RUSTFLAGS`、別 toolchain、profile、package、features、または CLI と workspace command の混在は、同じ最終検証の根拠に採用してはならない。必要な acceptance command が複数ある場合は、実装担当が一つへ絞るか、S1 へ戻って handoff の accept/reject gate を修正する。
- 最終検証の test / build は、その一つの command に process group と時間上限を持たせる。上限内に完了しない、child process が残る、CPU 無進捗になる、または execution/cache identity が変わる場合は、実装担当が起動した process group 全体だけを終了し、結果を inconclusive として不採用にして S1 へ戻る。親だけを終了して child test binary、rustc、incremental lock を残してはならない。fixture が不必要な鍵生成、KDF、network、実機操作、sleep を含んで上限を超える場合は、product boundary を失わない最小の deterministic fixture / test-only observation へ実装を是正してから新しい固定差分で再検証する。
- Markdown のみの変更では、生成文書や記載コマンド検証に関係する場合、または利用者が明示要求した場合を除き、`cargo xtask check` / `cargo xtask check static` を機械的に実行しない。
- コード、Nix、shell、workflow、bootstrap、生成物の変更では既定検証を実行する。
- 検証は dev shell 内で実行する。dev shell 外なら `direnv exec .` を前置する。
- 検証コマンドの一覧と用途は repository root の `README.md` を参照する。
- 検証 runner の責務を混在させてはならない。`cargo xtask check static` は fmt、test target を含まない
  Rust check/clippy、`bash -n`、workflow、Nix、AST/adapter gate だけを実行し、`cargo test`、internal-stub
  CLI integration、provision shell fixture を起動しない。実行を伴う三つの検証は `cargo xtask check test` が
  workspace test、internal-stub CLI integration、provision shell fixture を各一回だけ所有する。`all` は
  `static -> test -> zsh -> integration` の明示合成であり、runner 間の重複実行、互換 alias、旧経路の追加を
  許可しない。static/test の失敗は必ず非 0 とし、child process の cleanup を維持する。

## ローカル生成物の取り扱い

- リポジトリ外の生成済み dotfiles やマシン固有 dotfiles を手編集しない。
- 開発者の実 `~/.config/dotfiles` に書き込んで検証しない。

## 禁止

- 新規に持ち込んだ規約違反を「後で直す前提」で残すこと。
- 再読対象を読まずに実装方針を決めること。
- 委譲済み実装担当が同じ task を再オーケストレーションすること。
- 割り当て作業が許可スコープ内で実際に完了する前に最終応答すること。

## 検証プロセスと lock の固定手順

cargo、Nix、test、静的検査を起動する前に、担当者自身のプロセスを `ps` で PID・PPID・command とともに列挙する。stale または重複したプロセスは、子プロセスを先に終了してから親を終了し、artifact/target lock が消えたことを確認する。

検証ごとに `CARGO_TARGET_DIR`、`RUSTFLAGS`、toolchain、profile、package、features、実行 command を固定し、同じ検証を並列起動しない。lock 待ち、未完了、タイムアウト、出力欠落は合格扱いにせず、終了コードと実テスト件数を取得するまで未完了とする。

検証終了後も子プロセス、親プロセス、artifact/target lock を再確認する。lock が残る場合は次の検証を開始せず、PID・PPID・command と終了処理を記録する。
