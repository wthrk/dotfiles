# dotfiles

Nix flake として再利用できる `nix-darwin` + `home-manager` 管理 dotfiles。

ユーザーごとのローカル flake を `~/.config/dotfiles/flake.nix` に生成し、その flake から Home Manager と nix-darwin を適用します。

## 初回導入

macOS の新規環境では `scripts/bootstrap.sh` を使います。

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/main/scripts/bootstrap.sh | bash
```

特定の commit / tag に固定する場合:

```sh
curl -fsSL https://raw.githubusercontent.com/wthrk/dotfiles/<tag-or-commit>/scripts/bootstrap.sh | DOTFILES_BOOTSTRAP_SOURCE_REF=<tag-or-commit> bash
```

bootstrap は必要に応じて Nix を用意し、ローカル flake の生成と適用まで実行します。

既存の Home Manager 管理対象ファイルは `*.before-home-manager` に退避してから置き換えます。

sudo の Touch ID / Apple Watch 認証は nix-darwin 適用後に有効になります。初回 bootstrap で Nix や nix-darwin を入れる前の sudo 認証は、通常のパスワード入力が必要になる場合があります。

## 更新と適用

導入済みの環境では、通常 `dotfiles update` で最新版を取り込んでから設定を適用します。`update` はローカル
flake の `dotfiles` input だけを更新（`nix flake update dotfiles --flake <config_dir>`）し、repo の committed
lock にある nixpkgs / Homebrew tap pin へ追随してから、既存の `switch` と同じ適用処理を行います（対象省略時は
`all` として Home Manager、続いて nix-darwin を適用。非 macOS では `dotfiles update home` を使ってください）。

```sh
dotfiles update
```

ローカルに適用状態ファイルや marker は持たず、冪等性は nix 自身の lock / profile と switch の冪等性に委ねます。
適用された更新の概要（version 差分と change_items）は repo の `docs/update-history/*.toml`（nightly CI が記録）
にあり、`dotfiles update-history show` でいつでも閲覧できます。

更新せずに、現在のローカル flake のまま再適用する場合は `switch` を使います。`update` / `switch` はいずれも
適用対象を取り、`home` / `darwin` で部分適用できます（対象省略時は `all`）。

```sh
dotfiles switch
dotfiles switch home
dotfiles switch darwin
dotfiles switch all
```

適用時に呼ばれるコマンド:

```sh
home-manager switch --flake ~/.config/dotfiles#<user>
sudo darwin-rebuild switch --flake ~/.config/dotfiles#<host>
```

## ローカル flake の生成

bootstrap を使わずにローカル flake だけを作る場合は `dotfiles init` を使います。

```sh
nix run github:wthrk/dotfiles -- init
nix run github:wthrk/dotfiles -- init --user alice --host macbook --system aarch64-darwin
nix run github:wthrk/dotfiles -- init --source github:wthrk/dotfiles --force
```

## 秘密情報復旧

新規マシン復旧用の bootstrap secret は YubiKey PIV 領域に保存します。通常は primary 登録と spare 登録だけを使います。

```sh
dotfiles secrets yubikey enroll-primary
dotfiles secrets yubikey enroll-spare
dotfiles secrets verify-yubikey
dotfiles secrets yubikey rotate-bws-token
```

`enroll-spare` は `--primary-serial <serial>` で明示された primary、または 1 本だけ接続されている primary YubiKey から bootstrap secret を読み出します。読み出し直後に、1 本だけ接続されている YubiKey または `--spare-serial <serial>` で明示された YubiKey を spare として解決します。primary / spare それぞれの解決時に serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止します。1 本ずつしか接続できない場合は、この時点で primary を抜いて spare を挿し、表示された prompt で Enter を押します。非対話実行では `--primary-serial` と `--spare-serial` を指定します。

`rotate-bws-token` は対話実行では新しい token を一度だけ読み取り、更新ステップごとに 1 本だけ接続されている YubiKey または `--serial <serial>` で明示された YubiKey を更新します。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止します。serial 未指定の対話実行で継続 prompt に進む場合も、次の更新前に対象 YubiKey だけを接続します。複数本を接続したまま進める場合は同一実行で継続せず、`--serial` を指定して 1 本ずつ実行します。summary に出た serial を見て、primary とすべての spare が更新済みであることを確認してください。非対話実行では `--serial` と `--stdin` を指定して 1 本ずつ更新します。

`setup`、`put`、`get` は低水準コマンドです。直接使う場合でも secret 本文は CLI 引数では受け取らず、prompt または stdin から読みます。`get` は stdout が terminal の場合は平文出力を拒否するため、pipe または redirect 先を明示します。

### GPG 鍵リング復元と SSH 公開鍵

新規マシンでは `restore-gpg` で `gpg-secret-key-backup` encrypted envelope を取得し、接続中 YubiKey で復号して GPG 鍵リングへ復元します。import 後に authentication subkey の keygrip を gpg-agent の SSH key list へ登録し、gpg-agent SSH support が利用可能であることを確認します。

```sh
dotfiles secrets restore-gpg
dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>
```

`export-ssh-public-key` は GPG authentication subkey 由来の OpenSSH 公開鍵 1 行を stdout に出力します。秘密鍵素材は出力せず、GitHub SSH keys 登録用途に使います。

### password-store 復元

GPG 鍵リング復元と SSH 公開鍵の GitHub 登録が済んだら、`restore-pass` で private `password-store` repository を復元します。Bitwarden Secrets Manager から `password-store-remote`（`git@github.com:<owner>/<repo>.git` 形式）を取得し、`~/.password-store` が存在しないことを確認してから、GPG authentication subkey 経由の SSH agent 認証で clone します。clone 後に store が `pass` から読めること（`.gpg-id` が存在し、store entry を復元済み秘密鍵で実際に復号できること）を確認します。`.gpg-id` の複数 recipient（共有・予備鍵）や email・user-id 形式の recipient も対応し、entry が無い空 store ではいずれか 1 つの recipient の秘密鍵を保持していれば読めるとみなします。

```sh
dotfiles secrets restore-pass
```

clone は `git2` と SSH agent だけを使い、`git` CLI と GitHub API は使いません。gpg-agent SSH support の利用可否（SSH agent socket の解決と authentication subkey の識別）は `restore-gpg` が確認・gate するため、`restore-pass` はその setup を信頼して clone します。`~/.password-store` が既に存在する場合、remote URL が GitHub SSH clone URL でない場合、接続先 `github.com` の SSH host key が GitHub 公表の host key と一致しない場合、clone 後 store を `pass` が読めない場合（store entry を復号できない、または空 store でいずれの recipient の秘密鍵も持たない場合）は停止します。

backup envelope の登録・recipient 追加と `password-store-remote` の登録は provisioning 経路で行います。

```sh
dotfiles secrets gpg-backup register --primary-fingerprint <40-hex-fingerprint> [--serial <serial>]
dotfiles secrets gpg-backup add-spare [--unwrap-serial <serial>] --spare-serial <serial> [--yes]
dotfiles secrets pass-remote register [--serial <serial>] [--url git@github.com:<owner>/<repo>.git] [--yes]
```

これらの provisioning コマンドは Bitwarden Secrets Manager への読書き（secret の read/create/update）に、YubiKey storage の `bitwarden-client-secret` を使います。実行前に Bitwarden Secrets Manager 側で project `dotfiles-secret-recovery` を作成し、YubiKey に保存した token から同名 project が 1 件だけ見える状態にしてください。`dotfiles` / `scripts/provision-secret-recovery-source.sh` は project を作成せず、見つからない場合または複数見える場合は停止します。BWS access token の hidden prompt / stdin secret input は `dotfiles secrets yubikey put bitwarden-client-secret`、`enroll-primary`、`rotate-bws-token` など YubiKey storage へ保存・更新する経路だけで行います。`pass-remote register` / `gpg-backup register` / `gpg-backup add-spare` は token を prompt / stdin で受け取らず、CLI 引数（argv）・ログ・shell history・永続環境変数・永続一時ファイルへ載せません。

`register` は既存環境の GPG secret key を encrypted envelope 化し、接続中 YubiKey の recipient を 1 件作って Bitwarden Secrets Manager へ登録します。`add-spare` は既存 envelope を復号して同一 DEK を spare YubiKey の recipient へ追加し、stale overwrite 防止の更新識別子が一致する場合だけ更新します。`gpg-backup register` は `--serial` の YubiKey から BWS token を読み、同じ YubiKey の recipient を作ります。`gpg-backup add-spare` は `--unwrap-serial` の YubiKey から BWS token を読み、既存 recipient による DEK unwrap にも同じ YubiKey を使います。`pass-remote register` は private `password-store` repository の clone URL（`git@github.com:<owner>/<repo>.git` 形式）を Bitwarden Secrets Manager の復旧 project へ create または update する保管コマンドで、`--serial` の YubiKey から BWS token を読みます。serial 未指定時はいずれも単一接続だけを自動解決し、複数接続では fail-closed します。clone URL は認証・復号・署名・外部アクセス能力を与える credential ではないため、provisioning 入力では非秘匿として扱い、`--url <value>` 引数・可視プロンプト（対話実行で入力をエコー）・pipe（stdin）のいずれの方式でも入力できます（`--url` を指定すればその値を使い、未指定なら terminal で可視プロンプト・非 terminal で pipe から読みます）。ただし private repository の所在を示す値のため、ログ・エラー本文・診断出力には含めません。既存値の上書きは stale overwrite 防止の更新識別子が一致する場合だけ行います。これらの provisioning コマンドを非対話実行で上書き更新する場合は `--yes` を指定します。

gpg-agent の SSH support 設定（`gpg-agent.conf` の `enable-ssh-support` と `pinentry-program`）は Home Manager 管理です。`config/zsh/env.zsh` は `GPG_TTY` を設定し、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合だけ `SSH_AUTH_SOCK` を上書きします。

### Bitwarden Password Manager login

`bw-login` は Bitwarden Password Manager の CLI に login / unlock します。接続中の YubiKey から `bw-email` と `bw-password` を取得し、YubiKey OTP を入力させたうえで、`bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>`（2FA method 3 / YubiKey OTP）と `bw unlock --passwordenv BW_PASSWORD --raw` を `bw` CLI で実行します。`bw` CLI はこの login / unlock だけに許可された唯一の外部 CLI 例外で、secret 取得や永続保存用途には使いません。

```sh
dotfiles secrets bw-login [--serial <serial>] [--email <email>]
```

master password は子プロセスの `BW_PASSWORD` 環境変数でだけ渡し、CLI 引数（argv）・ログ・永続保存には載せません。`BW_PASSWORD` は保存しません。login email は通常 YubiKey 内の `bw-email` を使い、override が必要な場合だけ `--email <email>` を指定します。複数本の YubiKey が接続されている場合は `--serial <serial>` で対象を固定します。

`bw unlock --raw` が出力する `BW_SESSION` 値はコマンドが利用者へ surface するだけで、disk や dotfiles へ永続化しません。表示された値を使って `export BW_SESSION=...` を実行すれば、以降の `bw` 操作の session として使えます。

`bw-login` は Bitwarden account 側の 2FA / passkey 登録を自動化しません。primary と spare の両方の YubiKey をあらかじめ Bitwarden account の 2FA（Yubico OTP）として登録しておく必要があります（spare YubiKey の Bitwarden 側登録は `bw-login` では行いません）。

#### primary / spare の manual login validation

primary と spare のどちらの YubiKey でも login / unlock が完了することを、次の手順で確認します。`bw-login` は status 分岐をせず無条件で `bw login` → `bw unlock` を実行するため、各 manual validation は `bw` CLI が未ログイン状態（unauthenticated）であることを前提とします。直前の validation などで既に login 済みの場合は、`bw` CLI が「already logged in」で失敗するのを避けるため、次の YubiKey を検証する前に operator が `bw logout` を実行してから再実行してください。これは operator が手動で行う検証手順上の前提を示す注記であり、tool 側の `bw` CLI 用途を login / unlock 以外へ広げるものではありません。

1. primary YubiKey を接続し、`dotfiles secrets bw-login`（または `dotfiles secrets verify-yubikey --check bw-login`）を実行して、OTP 入力後に login / unlock が成功することを確認します。
2. primary を抜いて spare YubiKey を接続します。直前の手順で `bw` CLI が login 済みのままの場合は、ここで operator が `bw logout` を実行して未ログイン状態に戻してから、同じコマンドを実行します。複数本を同時接続する場合は `--serial <serial>` で spare を対象に指定し、spare でも login / unlock が成功することを確認します。
3. `dotfiles secrets verify-yubikey --check bw-login` と `dotfiles secrets verify-yubikey --all` は、`bw-login` と同じ Bitwarden Password Manager の login / unlock 到達確認を実行します（session key は確認専用のため surface せず破棄します）。引数なしの `dotfiles secrets verify-yubikey` は ローカル保管 確認だけを行い、外部の bw-login 項目は機械可読状態値 `skipped` のまま残します。

`bw-login` が受け付けるフラグは `--serial` と `--email` だけです。`verify-yubikey` は `--serial` に加えて外部確認を要求する option `--check bws` / `--check bw-login` / `--all`（到達できない場合は明示的に失敗します）を受け付け、`--email` は bw-login 確認の login email override にのみ適用されます。

## ロールバック

`nix-darwin`:

```sh
sudo darwin-rebuild --list-generations
sudo darwin-rebuild switch --rollback
```

Home Manager:

```sh
home-manager generations
home-manager switch --rollback
```

## 開発環境

このリポジトリを編集する場合は、初回だけ direnv を許可します。

```sh
direnv allow .
```

このディレクトリでは `direnv` が flake の devShell を読み込みます。検証や内部開発タスクは `cargo xtask` で実行します。

```sh
cargo xtask check
```

## 内部タスク

`xtask` はこの repository の開発用です。

```sh
cargo xtask apply
```

Home Manager のみ部分適用:

```sh
cargo xtask apply home-manager
```

## 検証

```sh
cargo xtask check
```

通常の静的検証を実行します。Rust、Nix、shell script、GitHub Actions workflow を確認します。

すべて実行する場合:

```sh
cargo xtask check all
```

個別に実行する場合:

```sh
cargo xtask check static
cargo xtask check zsh
```

Tart VM を使う runtime 検証:

```sh
cargo xtask check runtime
```

## nightly 自動 bump とゲート

`.github/workflows/nightly-update.yml` が nightly に repo の `flake.lock` を bump（nixpkgs と
Homebrew tap input のみ。framework input は bump しない）し、更新履歴を `docs/update-history/<YYYY-MM>.toml`
へ記録して自動 PR を起票・auto-merge します。各マシンはこの bump 済み pin に `dotfiles update` で追随します。

各アプリの「何が変わったか」概要は上流のリリースノートから取得しますが、ノートの置き場は一律に機械取得
できないため、(1) 機械的に取れるものは Releases API / changelog から取得し、(2) 取れないものは GitHub
Models ではなく OpenAI API（`async-openai` crate）の AI エージェントに探させます。概要取得は nightly の GitHub secret
`OPEN_AI_API_KEY` を要し、未設定（ローカル等）やノートが取れない場合はそのパッケージを version-only
（version old→new + notes_url のみ）としてその場で確定記録します（1 回の record で全変更パッケージを処理し
きり、夜をまたいで埋め直しません）。さらに、**どこからノートを取得したか
（provenance）を `docs/update-history/notes-sources.toml`（ノート取得元レジストリ）に保存**し、次回以降の
record はこのレジストリを最優先参照して同じ取得元を再利用します（再探索しません）。これにより AI 探索は
新規/未知パッケージと自己修復（取得元が移動した等）に限定され、回を追って OpenAI API の呼び出しが逓減します。
レジストリはパッケージ名昇順の決定論ソートで diff を最小化し、`docs/update-history/**` 配下にあるため nightly
の commit 許可パス内で repo に入ります。AI 由来の取得元 URL も含め、保存・再取得時は必ず許可ホスト https に
限定して検証します。

PR 起票・auto-merge は **`GITHUB_TOKEN`（`github.token`）で完結**します。別途 GitHub App を作って secret を
仕込む必要はありません。GITHUB_TOKEN が起票/push した PR では GitHub が `on: pull_request` の workflow
（必須 check）を発火しない既知の制約があるため、`nightly-update.yml` の open-pr job が **同一 run 内で
セキュリティチェック `cargo xtask ci verify-bump-lock` をインライン実行**し、合格時のみ PR head commit へ
`static checks` という commit status を投稿して required check を満たします。`static checks` は
`.github/workflows/static-checks.yml` の job 名であり、適用済み「main」ruleset の required context と context 名で
突合します。

インラインのセキュリティチェックは、PR の base..head 全 commit 履歴に対して次を機械判定します。

- 変更パスが `flake.lock` と `docs/update-history/**` だけであること（`.github/**`・ソースが混ざれば fail）。
- `flake.lock` 差分が許可 input 集合（nixpkgs と tap 4 本）の rev 変更だけで、想定外 input の追加・
  source 改変・framework input の rev 変更が無いこと。加えて、許可 input でも rev が変わらないまま
  `narHash` / `lastModified` だけが動く（同一 rev の取得物すり替え＝content swap）変更は fail にします。

このチェックは **検査者と検査対象を分離**します。判定バイナリ（`cargo xtask ci verify-bump-lock`）は nightly
workflow 自身の信頼 ref の checkout からビルドし、検査対象は base..head の lock 差分（git データ）です。PR 作業
ツリーの dotfiles を検査主体にしないため、悪意ある lock 改変があっても判定主体は信頼コードのままです。判定
ロジックは Rust の純粋核（`rust/xtask/src/ci/bump_lock.rs`）に置き、unit test で固定しています。チェックが fail すると
`static checks` status は投稿されず、required check が満たされないため無人 auto-merge は成立しません
（fail-closed・人手レビュー経路へ送られます）。許可パスが `flake.lock` + `docs/update-history/**` に限定される
ため、nightly PR が `.github/**`（workflow/guard）を変更しようとしてもこのチェックで fail します。

合格後、open-pr job は `@codex review` コメントで codex 自動レビューを起動し（Copilot は GitHub 側のネイティブ
code review で走ります）、`gh pr merge --auto --squash` で auto-merge を有効化します。Copilot/Codex のレビュー
充足と required status（`static checks`）満了でマージされます。

### 残留制約（実 GitHub でのみ最終確認できる）

この App 不要フローには、実 GitHub でしか確定できない前提があります（マージ後に `workflow_dispatch`
`dry_run=false` で検証します）。

- GITHUB_TOKEN で POST した commit status `static checks` が、適用済み ruleset の required context と確実に
  名前突合してマージ条件を満たすか。
- `gh pr merge --auto` を GITHUB_TOKEN 権限（`pull-requests: write`）で有効化できるか（repo 設定で
  auto-merge が有効である必要があります）。
- `copilot_code_review` が bot / GITHUB_TOKEN 起票 PR で発火するか。

いずれも未充足ならマージは保留され、無人で main に入りません（人手レビューへ送られます）。

#### インライン `verify-bump-lock` の適用範囲（threat-model）

インラインの `cargo xtask ci verify-bump-lock` は、**nightly workflow が自分で起票する bump PR にのみ適用**されます
（open-pr job が同一 run 内で実行するため）。第三者が `nightly/bump-*` prefix で**直接起票した PR には
`verify-bump-lock` は走りません**（全 PR を横断検査していた `nightly-bump-guard.yml` の required check は App 廃止に
伴い削除済みです）。そうした攻撃者起票 PR のマージ阻止は、bypass 不能な「main」ruleset の必須 `static checks` と
Copilot/Codex の自動レビューに依存します（加えて `.github/**` の改変はそもそも許可パス外として弾かれます）。より
強い保護が必要なら、App 不要の per-PR guard workflow（`on: pull_request` で nightly-prefix PR に `verify-bump-lock`
を実行し、その結果を「main」ruleset の required check に加える）を別途有効化できます。本仕様は App / secret 不要を
維持するため、この per-PR guard は既定では有効化していません。

## Homebrew cask の固定状況（無人 upgrade の明示受容）

auto-update 経路は switch 時に `brew upgrade` を実行して installed cask/formula を tap rev の pin へ追従させます。
`homebrew.nix` で `greedyCasks = true` を有効化しているため、`auto_updates true` / `version :latest` の cask も
upgrade 対象になり、全 cask が tap pin へ決定論的に収束します（`dotfiles update-history` の差分にも現れます）。
tap rev は cask の「定義」を固定し、ダウンロード成果物の固定性は cask 側の `sha256` 指定に依存します。現在の
宣言 cask は `auto_updates true` のものも含め全て `sha256` で成果物を明示固定しているため、greedy 有効下でも無人
upgrade が差し替える成果物は tap rev で再現的に固定されます。

現在の宣言 cask の固定状況を明示受容します。

| cask | tap | sha256 固定 | auto_updates | 無人 upgrade 対象 | 成果物の固定 |
|---|---|---|---|---|---|
| `azookey` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `font-cica` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `yubico-authenticator` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `bitwarden` | homebrew/cask | あり | **true** | 対象（greedy） | tap rev で固定（再現的） |
| `codex-app` | homebrew/cask | あり（arm） | **true** | 対象（greedy） | tap rev で固定（再現的） |
| `ghostty` | homebrew/cask | あり | **true** | 対象（greedy） | tap rev で固定（再現的） |

greedy 有効化の前提は「全 cask が sha256 固定」です。`sha256 :no_check`（未固定成果物）の cask を足すと、greedy
有効下では未固定成果物が無人差し替えされうるため、`dotfiles update-history record` 経路の brew モジュールが tap
rev の cask `.rb` を検査し、`sha256 :no_check` があれば fail-closed で停止します（cask 名を添えて中断）。cask を
追加する際は、対象 cask の `.rb` が `sha256 "<hash>"` で固定されている（`sha256 :no_check` でない）ことを確認して
ください。固定できない cask は `homebrew.nix` の `casks` から外し、必要なら手動更新へ寄せます。auto-update が cask
を上げた事実は、nightly CI が記録する `docs/update-history/*.toml`（`dotfiles update-history show` で閲覧）に更新
アプリとして現れ、無人差し替えが不可視にならないようにしています。
