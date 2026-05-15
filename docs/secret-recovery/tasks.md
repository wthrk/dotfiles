# 新規マシン秘密情報復旧基盤タスク

この文書は、新規マシン秘密情報復旧基盤を実装するための main issue / sub-issue 分割を定義する。各 sub-issue は design PR、design review、implementation PR、code review、validation、done の順に進める。

## Main Issue

```text
新規マシン秘密情報復旧基盤を実装する
```

目的は、新しい macOS マシンで `dotfiles` 導入後に GPG、GitHub SSH identity、private `password-store` repository、`pass`、Bitwarden Password Manager CLI login を復旧できる基盤を実装することである。

進捗管理 issue:

- #11 新規マシン秘密情報復旧基盤を実装する

## First PR

最初の PR は documentation-only とし、全体設計と実装タスク構造だけを追加する。

Branch:

```text
docs/secret-recovery-plan
```

Title:

```text
docs: 新規マシン秘密情報復旧基盤の設計を追加
```

Contents:

- `docs/secret-recovery/README.md`
- `docs/secret-recovery/tasks.md`

Validation:

```sh
cargo xtask check static
```

現在の進捗:

- First PR #19 は merge 済み。
- 検証済み: `direnv exec . cargo xtask check static`
- 次は #12 YubiKey 秘密情報保存の design PR に進む。

完了時の issue 管理:

- first PR が merge されたら、#11 に first PR 完了と検証結果をコメントする。
- #11 は秘密情報復旧基盤全体の main issue なので、first PR 完了時点では close しない。
- #12 から #18 は、それぞれの design PR、implementation PR、validation が完了した時点で close する。
- #12 から #18 がすべて close され、統合フローと最終ドキュメント整理の完了が確認できた時点で #11 を close する。

## 共通ワークフロー

各 sub-issue は次の流れで進める。

1. Design PR
2. Design review
3. Implementation PR
4. Code review
5. Validation
6. Done

Design PR では crate / API / 保存形式 / 停止条件 / 検証方法を確定する。Implementation PR では設計で決めた範囲だけを実装し、unit test と必要な manual validation を追加する。

## Sub-Issues

### 1. YubiKey secret storage

Issue:

- #12 YubiKey 秘密情報保存

目的:

YubiKey に `bw-email`、`bw-password`、`bws-access-token` を保存し、復旧コマンドから安全に取得できるようにする。

Design PR で決めること:

- YubiKey PIV 操作に使う Rust crate
- secret 保存形式
- 使用する PIV slot / object / identifier
- スペア YubiKey に同じ bootstrap secret を事前配布する方法
- 挿さっている YubiKey が必要な secret を保持し、外部 service へ接続できることを確認する test command の分担
- 外部サービス側の spare key 登録と、この repository が自動化する範囲の境界
- 既存 YubiKey 設定と衝突した場合の停止条件
- `bw-email` / `bw-password` / `bws-access-token` の name validation
- secret 入力方法
- 上書き時の option

Design PR の成果物:

- `docs/secret-recovery/yubikey-secret-storage-design.md`

Implementation PR の完了条件:

- `dotfiles secrets yubikey setup` を実装する。
- `dotfiles secrets yubikey put <name>` を実装する。
- `dotfiles secrets yubikey get <name>` を実装する。
- `dotfiles secrets yubikey enroll-primary` を実装する。
- `dotfiles secrets yubikey enroll-spare` を実装する。
- `dotfiles secrets yubikey rotate-bws-token` を実装する。
- `dotfiles secrets verify-yubikey` の local storage check を実装する。
- 通常の対話実行では、1 本だけ接続されている YubiKey を自動選択し、複数本接続時は一覧から選択させる。
- 非対話実行では serial option で対象を明示させる。
- primary と spare の両方に同じ bootstrap secret を登録できる。
- `enroll-spare` は primary から secret を読み出し、spare に再暗号化して保存する。通常手順で secret の再入力を要求しない。
- 許可された name だけを受け付ける。
- secret 本文を CLI 引数、ログ、一時ファイルに残さない。
- 同名 secret の上書きには明示 option を要求する。
- unit test を追加する。

検証観点:

- 実機検証は read-only 確認と専用領域への書き込みに限定する。
- `verify-yubikey` で `bw-email`、`bw-password`、`bws-access-token` を復号できることを確認する。
- `enroll-spare` だけで primary 読み出し、spare setup、spare への再暗号化保存、local verify が完了することを確認する。
- reset / credential 削除 / 既存領域上書きを含む検証は行わない。
- 既存の FIDO2 / OTP / OpenPGP / PIV credential に影響しないことを確認する。

### 2. Bitwarden Secrets Manager client

Issue:

- #13 Bitwarden Secrets Manager クライアント

目的:

Bitwarden Secrets Manager から `gpg-secret-key-backup` と `password-store-remote` を取得する client API を提供する。

Design PR で決めること:

- 公式 `bitwarden` Rust SDK の利用方法
- `gpg-secret-key-backup` / `password-store-remote` の取得 API
- access token の保持範囲
- fake client によるテスト方式

Implementation PR の完了条件:

- Secrets Manager client を実装する。
- fake client を実装する。
- unit test を追加する。
- `restore-gpg` / `restore-pass` から使える API を提供する。
- `dotfiles secrets verify-yubikey --check bws` から使える接続確認 API を提供する。
- 復旧本線で `bw` CLI に依存しない。

検証観点:

- fake client で正常系、secret 不在、認証失敗を検証する。
- `verify-yubikey --check bws` で `gpg-secret-key-backup` と `password-store-remote` を取得できることを検証する。
- access token が不要な範囲に保持されないことを確認する。
- SDK の error を利用者向けの context 付き error に変換する。

### 3. GPG restore / gpg-agent SSH support

Issue:

- #14 GPG 復元 / gpg-agent SSH 対応

目的:

Bitwarden Secrets Manager に保存した GPG secret key backup を import し、`pass`、GitHub SSH、Git signing に必要な subkey が使えることを確認する。

Design PR で決めること:

- GPG import に使う `gpgme` API
- subkey 検証方法
- Home Manager で管理する `gpg-agent.conf`
- zsh の `GPG_TTY` / `SSH_AUTH_SOCK` 設定
- 既存 key がある場合の停止条件

Implementation PR の完了条件:

- `dotfiles secrets restore-gpg` を実装する。
- `dotfiles gpg export-ssh-public-key` を実装する。
- Home Manager 設定を追加または更新する。
- import 後に encryption / authentication / signing subkey を検証する。
- `gpg-agent` SSH support の利用可否を確認する。
- unit test を追加する。

検証観点:

- manual GPG validation を行う。
- manual SSH validation を行う。
- 既存 key がある場合に設計どおり停止する。
- zsh startup behavior、TAB bindings、fzf-tab、autosuggestions、syntax highlighting、PATH handling に影響がある場合は `cargo xtask check zsh` を実行する。

### 4. password-store restore

Issue:

- #15 password-store 復元

目的:

GPG authentication subkey による SSH identity を使い、GitHub から private `password-store` repository を clone して `pass` を利用可能にする。

Design PR で決めること:

- `git2` + SSH agent の実装方法
- `password-store-remote` の validation
- `~/.password-store` 既存時の停止条件
- clone 後の確認方法

Implementation PR の完了条件:

- `dotfiles secrets restore-pass` を実装する。
- `git2` clone を実装する。
- `~/.password-store` が既に存在する場合に停止する。
- clone 後に `pass` が store を読めることを確認する。
- unit test を追加する。

検証観点:

- fake remote または fixture で clone 処理を検証する。
- manual GitHub SSH clone validation を行う。
- GitHub API と `git` CLI に依存していないことを確認する。

### 5. Bitwarden Password Manager CLI login

Issue:

- #16 Bitwarden Password Manager CLI ログイン

目的:

YubiKey に保存した Bitwarden master password と YubiKey OTP を使い、Bitwarden Password Manager に CLI login / unlock できるようにする。

Design PR で決めること:

- `bw` CLI を使う範囲を login / unlock に限定する明文化
- YubiKey OTP 入力方法
- Bitwarden account に primary と spare の YubiKey が登録済みであることの前提と validation 手順
- `dotfiles secrets verify-yubikey --check bw-login` の挙動
- `BW_PASSWORD` / `BW_SESSION` の寿命
- 出力形式

Implementation PR の完了条件:

- `dotfiles secrets bw-login` を実装する。
- `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` を実行する。
- `bw unlock --passwordenv BW_PASSWORD --raw` を実行する。
- `dotfiles secrets verify-yubikey --check bw-login` を実装する。
- `BW_PASSWORD` を保存しない。
- primary と spare のどちらでも manual login validation を行える手順を文書化する。
- unit test を追加する。

検証観点:

- manual Bitwarden OTP login validation を行う。
- `verify-yubikey --check bw-login` が secret、password、session token を出力しないことを確認する。
- `bw` CLI の利用範囲が login / unlock に限定されていることを確認する。
- `BW_PASSWORD` がログ、引数、一時ファイル、永続環境変数に残らないことを確認する。
- `BW_SESSION` の出力と寿命が設計どおりであることを確認する。

### 6. 新規マシン復旧フロー統合

Issue:

- #17 新規マシン復旧フロー統合

目的:

個別コマンドを新規マシン復旧手順として接続し、失敗時に安全に停止し、再実行できる状態にする。

Design PR で決めること:

- 初期セットアップ手順
- 新規マシン復旧手順
- validation checklist
- `dotfiles secrets verify-yubikey --all` を復旧前 check として組み込むかどうか
- 失敗時の停止条件と再実行手順
- `scripts/bootstrap.sh` と接続するかどうか

Implementation PR の完了条件:

- 統合手順書を更新する。
- 必要な接続実装を追加する。
- `verify-yubikey --all` の end-to-end validation 結果を記録する。
- restore validation 結果を記録する。
- bootstrap behavior に変更がある場合は `README.md` を更新する。

検証観点:

- fresh-machine 相当の runtime check を検討する。
- bootstrap、first-run behavior、host switching、cross-machine assumptions に影響する場合は `cargo xtask check runtime` を実行する。
- 失敗後の再実行で secret や repository を破壊しないことを確認する。

### 7. 最終ドキュメント整理

Issue:

- #18 最終ドキュメント整理

目的:

秘密情報復旧基盤の一連の作業が終わった後に、`docs/` から作業用の進捗メモ、ブランチ名、PR 作業メモ、issue 運用メモを取り除き、恒久的なドキュメントだけを残す。

実施タイミング:

#12 から #17 までの design PR、implementation PR、validation が完了し、統合フローの完了が確認された後に実施する。

完了条件:

- `docs/` から作業用の進捗メモを取り除く。
- `docs/` からブランチ名、PR 作業メモ、issue 運用メモを取り除く。
- 最終的に `docs/` へ残す内容を、恒久的な設計、復旧フロー、タスク構造、検証方針だけにする。
- 整理後のドキュメントで `cargo xtask check static` を通す。

## 進行上の前提

- 実装は sub-issue ごとに design PR と implementation PR に分ける。
- 実際の進捗は GitHub issue / PR で管理する。
- docs には最終成果物としての設計とタスク構造だけを置く。
- PR grouping と commit grouping は別に判断する。
- user-visible commands、bootstrap behavior、module boundaries、zsh key behavior を変える場合は `README.md` を更新する。
