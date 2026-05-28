# Bitwarden Password Manager CLI login 設計

この文書は、[secret-recovery-spec.md](./secret-recovery-spec.md) の [責務分担 / Bitwarden Password Manager](./secret-recovery-spec.md#bitwarden-password-manager) を具体化する到達設計仕様を定義する恒久文書である。対象は `dotfiles secrets bw-login` と `dotfiles secrets verify-yubikey --check bw-login` で扱う Bitwarden Password Manager の CLI login / unlock 経路である。

この文書は完成形の設計だけを扱う。

## 目的と保護境界

この機能の目的は、YubiKey に保存した `bw-email` と `bw-password`、利用者が手元の YubiKey で生成した OTP を使い、Bitwarden Password Manager へ CLI login / unlock し、後続の `bw` CLI 操作に必要な session を呼び出し元へ受け渡すことである。

保護するもの:

- YubiKey から読み出した `bw-password`。
- `bw unlock --raw` で得た `BW_SESSION`。
- OTP、master password、session token のログ・エラー・診断出力への漏えい。

保護しないもの:

- Bitwarden の公式 CLI が通常挙動として保持する login state や local cache。
- `BW_SESSION` を受け取った呼び出し元 shell / child process 側の取り扱い。
- 実行中 host が侵害された状態でのメモリ露出。

## 決定事項

- Bitwarden Password Manager で `bw` CLI を使う範囲は `status` / `login` / `unlock` / `logout` に限定する。
- 通常の login email は YubiKey 内の `bw-email` を使い、override が必要な場合だけ `--email <email>` を許可する。
- master password は `BW_PASSWORD` として `bw` 子プロセスにだけ渡し、親プロセスや永続環境変数へ残さない。
- OTP は controlling terminal で利用者に入力させる。CLI 引数、設定ファイル、review artifact、永続環境変数では受け取らない。
- `dotfiles secrets bw-login` は login / unlock 完了後に shell で評価可能な `BW_SESSION` export 断片だけを stdout に出力する。
- `dotfiles secrets verify-yubikey --check bw-login` は同じ login / unlock 経路で外部確認を行うが、`BW_SESSION` を利用者へ返さず、確認後に `bw logout` まで実行して終了する。
- Bitwarden account 側の 2FA / passkey / OTP には primary と spare の両方の YubiKey が事前登録済みであることを前提にし、この登録自体は自動化しない。

## 責務分担

### entrypoint / CLI

- `dotfiles secrets bw-login` の option 解析、`--email` override の受理、stdout が terminal かどうかの事前確認を担う。
- `verify-yubikey --check bw-login` の option から email override 要否を application へ渡す。
- shell export 断片や verify summary の公開出力形式を決める。

### application

- `bw-email` / `bw-password` の取得順序を持つ。
- OTP 入力要求、`bw status` 判定、`bw login` 実行要否、`bw unlock` 実行、verify 時の `bw logout` cleanup までの use case 順序を持つ。
- Bitwarden CLI の status と target email が整合しない場合の停止条件を持つ。

### external command adapter

- `bw status`、`bw login`、`bw unlock --raw`、`bw logout` の process 実行だけを担う。
- `BW_PASSWORD` の子プロセス環境注入、stdout/stderr capture、status 出力の decode を担う。
- Bitwarden CLI 固有の exit status / stderr / JSON 形式を application が直接知らなくて済むよう翻訳する。

## `dotfiles secrets bw-login`

### コマンドフロー

1. `--email` があればそれを target email とし、なければ YubiKey から `bw-email` を取得する。
2. YubiKey から `bw-password` を取得する。
3. controlling terminal に OTP 入力 prompt を表示し、利用者が primary または spare の YubiKey で生成した OTP を入力する。
4. `bw status` を確認する。
5. status が `unauthenticated` なら `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` を実行する。
6. status が `locked` または `unlocked` なら、Bitwarden CLI が保持している account email が target email と一致する場合だけ login を省略して `unlock` へ進む。
7. `bw unlock --passwordenv BW_PASSWORD --raw` を実行し、非空の session token を得る。
8. stdout に shell で評価可能な `export BW_SESSION='...'` 断片だけを出力して終了する。

### status 判定

- `unauthenticated`: full login / unlock を行う。
- `locked`: account email が target email と一致する場合のみ unlock を行う。
- `unlocked`: account email が target email と一致する場合のみ新しい `BW_SESSION` を得るため unlock を行う。
- target email と不一致の account が既に login 済みなら停止し、利用者に `bw logout` を要求する。

### OTP 入力

- OTP は controlling terminal からだけ受け付ける。
- OTP は可視入力でよいが、CLI 引数や永続環境変数には載せない。
- trailing newline は 1 つだけ除去し、それ以外の bytes は変更しない。
- terminal を開けない場合は login を始める前に停止する。

## `dotfiles secrets verify-yubikey --check bw-login`

- `bw-login` と同じ secret 取得順序、email override 方針、OTP 入力方法を使う。
- login / unlock が成功し、非空の session token を取得できた時点で Bitwarden Password Manager 到達確認は `ok` とする。
- verify 用 check は `BW_SESSION` を stdout / stderr に出さない。
- verify 用 check は成功時も失敗時も session token を破棄し、成功後は `bw logout` を実行して local login state を残さない。
- verify 経路で `bw logout` cleanup に失敗した場合、その確認は `failed` とする。

## `BW_PASSWORD` / `BW_SESSION` の寿命

- `BW_PASSWORD` は `bw login` と `bw unlock` を起動する瞬間だけ子プロセス環境へ設定し、親プロセス側では protected secret として必要最小限だけ保持する。
- `BW_PASSWORD` を profile、shell rc、temporary file、review artifact、コマンド引数へ書き出さない。
- `BW_SESSION` は `bw unlock --raw` の stdout から受け取った直後から、`bw-login` の stdout 出力または verify cleanup 完了までだけ保持する。
- `bw-login` は `BW_SESSION` を shell export 断片として stdout に返すだけで、現在の shell やファイルへ自動保存しない。
- `verify-yubikey --check bw-login` は `BW_SESSION` を呼び出し元へ返さない。

## 出力形式

### `dotfiles secrets bw-login`

- stdout には shell で `eval "$(dotfiles secrets bw-login)"` できる shell-escaped な `export BW_SESSION='...'` 断片だけを出力する。
- stdout が terminal の場合は session token が画面や scrollback に残るため拒否し、pipe / command substitution / redirect を要求する。
- stderr には進行状況や案内を出してよいが、email 以外の secret、OTP、session token を含めない。

### `dotfiles secrets verify-yubikey --check bw-login`

- `verify-yubikey` の summary では `bw-login` check を `ok` / `failed` / `skipped` の機械可読状態で返す。
- `--check bw-login` を明示したのに check を完了できない場合、`skipped` を成功扱いにせず `failed` にする。
- summary や error には secret 本文、OTP、session token を含めない。

## 停止条件

- `bw-email` または `bw-password` を YubiKey から取得できない。
- `--email` override が空文字または Bitwarden target account として扱えない。
- OTP 入力に必要な controlling terminal を開けない。
- `bw` CLI が利用できない。
- `bw status` が target email と不一致の login state を示す。
- `bw login` が認証失敗、2FA 未登録、OTP 不正、ネットワークエラー等で完了しない。
- `bw unlock --raw` が失敗する、または空の session token を返す。
- `dotfiles secrets bw-login` の stdout が terminal である。
- `verify-yubikey --check bw-login` の cleanup `bw logout` が失敗する。

## manual validation 契約

primary / spare のどちらでも、manual validation は次の形を基準にする。

```sh
eval "$(dotfiles secrets bw-login)"
bw sync
bw logout
unset BW_SESSION
```

- primary と spare の両方で同じ validation を行えることを implementation PR の manual check で確認する。
- `bw sync` が失敗した場合でも `bw logout` と `unset BW_SESSION` を実行し、session を shell と local login state に残さない。
- validation 中も `bw-password`、OTP、`BW_SESSION` を shell history、ログ、review artifact に残さない。
