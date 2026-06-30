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

新規マシン復旧用の bootstrap secret は YubiKey PIV 領域に保存します。保存対象は `bitwarden-client-id` と `bitwarden-client-secret` だけです。

```sh
dotfiles secrets yubikey enroll-primary
dotfiles secrets yubikey enroll-spare
dotfiles secrets verify-yubikey
```

`enroll-primary` と `enroll-spare` は、接続中の YubiKey が 1 本だけであることを前提に、CLI secret input port から `bitwarden-client-id` と `bitwarden-client-secret` を受け取って保存します。secret 本文は CLI 引数では受け取りません。`setup`、`put`、`get` は低水準コマンドで、`get` は stdout が terminal の場合に平文出力を拒否します。

復旧対象の `gpg-secret-key-backup` と `password-store-remote` はユーザー個人の Bitwarden vault に保存します。dotfiles は vault へのアクセスを CLI ではなく SDK/API adapter 境界で扱い、shell script は credential、URL、fingerprint、API key を argv/stdin/env で中継しません。

```sh
dotfiles secrets restore-gpg
dotfiles gpg export-ssh-public-key
dotfiles secrets restore-pass
```

`restore-gpg` は個人 vault から encrypted envelope を取得し、接続中 YubiKey で復号して GPG 鍵リングへ復元します。import 後に authentication subkey の keygrip を gpg-agent の SSH key list へ登録し、gpg-agent SSH support が利用可能であることを確認します。

`restore-pass` は個人 vault から `password-store-remote` を取得し、`~/.password-store` が存在しないことを確認してから、GPG authentication subkey 経由の SSH agent 認証で clone します。clone 後に store が `pass` から読めることを確認します。

backup envelope の照合と `password-store-remote` の登録は provisioning 経路で行います。

```sh
dotfiles secrets gpg-backup register
dotfiles secrets pass-remote register
```

`gpg-backup register` は既存の `gpg-secret-key-backup` encrypted envelope が解決済み primary fingerprint、接続中 YubiKey recipient、primary/spare の 2 recipient 以上条件を満たすか確認します。`pass-remote register` は configured origin を優先し、origin が無い場合だけ CLI/app 側の controlling TTY input port で `password-store-remote` を受けます。URL はログ、エラー本文、診断出力に含めません。

vault への到達確認は `dotfiles secrets verify-yubikey --check vault` または `--all` で行います。

gpg-agent の SSH support 設定（`gpg-agent.conf` の `enable-ssh-support` と `pinentry-program`）は Home Manager 管理です。`config/zsh/env.zsh` は `GPG_TTY` を設定し、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在する場合だけ `SSH_AUTH_SOCK` を上書きします。

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
