# 新規マシン秘密情報復旧基盤

この文書は、新しい macOS マシンで `dotfiles` を導入したあと、開発に必要な秘密情報基盤を復旧する到達仕様を定義する恒久仕様文書である。ここでは完成形の仕様だけを定義する。対象は GnuPG secret key、GPG authentication subkey による GitHub SSH identity、private `password-store` repository、`pass` の利用環境である。Bitwarden Password Manager の login は repository の CLI surface 外である。

復旧の入口には YubiKey を使う。YubiKey には Bitwarden Secrets Manager access token の `bitwarden-client-secret` だけを保存する。GPG secret key backup と `password-store` の remote URL は BWS project `dotfiles-secret-recovery` から取得する。GPG secret key を復元したあと、GPG authentication subkey を SSH identity として使い、GitHub から private `password-store` repository を SSH clone する。

この文書で強化する要件は設計/仕様契約であり、現行 Rust 実装およびテストが本書の全要件を満たしたことを示すものではない。実装・テストでの充足は、ユーザー指定の GitHub issue、PR、または明示タスクで段階的に反映する。

secret の保護境界、core dump 無効化、paging / memory lock / signal trap の扱い、外部処理が secret の借用または所有 plaintext buffer の move を要求する場合の実装方針は [Secret handling policy](./secret-handling.md) を正本とする。この仕様文書では復旧対象とコマンド契約だけを定義し、secret handling の詳細を再掲しない。

## 目的

- 新規マシンで秘密情報基盤を再構築する手順を `dotfiles` CLI に集約する。
- 復旧に必要な bootstrap secret を YubiKey と Bitwarden Secrets Manager に分離して保持する。
- GitHub API や 鍵サーバー に依存せず、GPG authentication subkey 由来の SSH identity で private repository を取得する。
- 平文 secret を CLI 引数、ログ、一時ファイル、永続環境変数に残さない。
- 破壊的な YubiKey reset や既存 認証情報 の削除を自動化しない。

## 無対話復旧の利用者契約

利用者は **YubiKey を挿して復旧コマンドを実行するだけ** で復旧を開始できる。`restore-gpg` と `restore-pass`、および復旧前提を確認する `verify-yubikey --check bws` / `verify-yubikey --all` は、YubiKey に保存された `bitwarden-client-secret` を内部で一時利用する。利用者に master password、session、PIV PIN、environment variable、argv の secret、YubiKey OTP、またはそれらを渡す対話入力を追加で要求してはならない。

YubiKey に保存する bootstrap secret は `bitwarden-client-secret` だけである。repository は Bitwarden Password Manager login command を公開せず、email、master password、OTP、session を YubiKey 保存・CLI recovery input・enroll 出力・spare copy・status/clear lifecycle に含めない。復旧中に読む YubiKey 保存値、BWS access token、取得した envelope、session を含む credential は stdout、stderr、log、一時ファイル、永続 environment へ出力・保存せず、必要な use の後に破棄する。CLI option の `--serial` は非秘匿の対象選択だけに使え、secret を argv に渡す許可ではない。

[#11](https://github.com/wthrk/dotfiles/issues/11) の復旧到達目的（GPG、SSH、private `password-store` の復旧）を実現するための統合確認が [#17](https://github.com/wthrk/dotfiles/issues/17) である。`verify-yubikey --all` はその統合確認を BWS recovery prerequisite に限定して具体化するものであり、Password Manager login、email、master password、OTP、session を追加必須にしてはならない。この節と「Secret の置き場所」が issue の要約・旧実装・テスト期待と矛盾する場合は本仕様を優先する。

## 復旧対象

- GnuPG secret key
- GPG encryption subkey による `pass` 復号環境
- GPG authentication subkey による GitHub SSH identity
- GPG signing subkey による Git signing 環境
- private `password-store` repository

## スペア YubiKey 運用

スペア YubiKey は YubiKey 本体を複製するものではない。各外部サービス には primary と spare をそれぞれ登録し、この repository 独自の bootstrap secret は primary と spare の両方に同じ値を保存する。

外部サービス の登録は `dotfiles` CLI では自動化しない。Yubico の一般方針どおり、primary と spare は同時期に用意し、サービスごとの security / 2FA / passkey 設定から両方を登録する。

対象ごとのスペア作成方法と、この repository での扱いは次のとおり。

- `FIDO2 / passkey / U2F`: GitHub、Bitwarden、Google、Apple など各 service の account security 設定で primary と spare を別々に登録する。物理キー登録は手順として記録し、`dotfiles` CLI では自動化しない。
- `Yubico OTP`: OTP を要求する service で primary と spare を別々に登録する。Bitwarden CLI login は復旧フローではないため、この登録を無対話復旧の前提にしない。
- `OATH TOTP`: 同じ TOTP secret / QR code を primary と spare の両方に登録する。既存 secret を取り出せない場合は サービス側で TOTP を再設定する。TOTP secret はこの repository に保存しない。
- `bitwarden-client-secret`: primary と spare の両方に同じ Bitwarden Secrets Manager access token を保存し、BWS の読取・作成・更新に使う。token の入力は YubiKey storage へ保存・更新する経路だけで行い、BWS provisioning / recovery command は YubiKey から取得する。rotate 時は全 YubiKey を更新する。
- `GPG secret key`: YubiKey には載せず、Bitwarden Secrets Manager の backup から `restore-gpg` で復元する。
- `GitHub SSH identity`: YubiKey には載せず、復元した GPG authentication subkey 由来の SSH 公開鍵 を使う。`dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` で出力する。
- `password-store`: YubiKey には載せず、GitHub から clone し、復元した GPG key で復号する。`restore-pass` で復元する。

primary YubiKey の紛失後に、primary だけに保存されていた bootstrap secret から spare を後付け作成することはできない。復旧可能性を維持するには、Bitwarden recovery code と各 service の recovery code を別経路で保管し、spare YubiKey を事前登録しておく。

## Secret の置き場所

保存場所ごとの secret と用途は次のとおり。

- `YubiKey`: `bitwarden-client-secret` だけを保存し、Bitwarden Secrets Manager の読取・作成・更新に使う。
- `Bitwarden Secrets Manager`: project `dotfiles-secret-recovery` に `gpg-secret-key-backup` と `password-store-remote` を保存する。`gpg-secret-key-backup` は YubiKey recipient 付き encrypted envelope として保存し、BWS secret value 取得だけで plaintext 復旧完了にしない。`password-store-remote` は credential ではないが private repository の所在を示す値であり出力には漏らさない。
- `Bitwarden Password Manager`: Web service passwords、passkeys、TOTP、recovery codes を保存し、利用者向け password manager として使う。
- `pass` / `~/.password-store`: Bitwarden Password Manager CLI API `client_id` / `client_secret` と UNIX 運用 secret を保存し、ローカル運用に使う。これは recovery CLI surface 外であり、復旧 command はこの値を読まず、BWS access token として代用しない。
- `GitHub`: GPG authentication subkey 由来の SSH 公開鍵 を保持し、private repository clone に使う。

この仕様の保存モデルは、保存先・名前・保存する値で定義する。organization / machine account / service account の作成や特定 UI 画面名は、この repository の実装前提ではない。Bitwarden Secrets Manager について実装・レビューで照合する正本は project `dotfiles-secret-recovery`、secret `gpg-secret-key-backup` / `password-store-remote`、YubiKey storage `bitwarden-client-secret` の関係であり、UI の導線名ではない。

## 責務分担

### YubiKey

YubiKey は復旧入口の bootstrap secret である `bitwarden-client-secret` だけを保持し、Bitwarden Secrets Manager の読取・作成・更新に使う。

YubiKey 操作は Rust crate から行い、`ykman` CLI は使わない。PIV の reset や global state を破壊する操作は実装しない。書き込み対象はこの機能用に確保した領域だけに限定し、既存の FIDO2 / OTP / OpenPGP（公開鍵規格） / PIV 認証情報 を reset しない。既存領域と衝突する場合は停止する。
書き込みは management key 認証を前提にし、既定 management key のまま運用しない。既定 key のままでは想定外の上書きリスクを抑止できないため、専用領域を運用する前に非既定 management key への変更を必須にする。

factory-default management key を使う運用は暫定前提にしてはならない。非既定 management key への切替、取得、注入を満たせない場合は、その作業単位の完了条件として明示的に扱う。

詳細設計は [YubiKey 秘密情報保存設計](./yubikey-secret-storage-design.md) に置く。

### Bitwarden Secrets Manager

Bitwarden Secrets Manager は、復旧に必要な取得対象を保持する。対象は project `dotfiles-secret-recovery` 内の `gpg-secret-key-backup`（YubiKey recipient 付き encrypted envelope。認証・復号・署名能力を与える credential）と `password-store-remote`（private `password-store` repository の clone URL。credential ではないが private repository の所在を示す値であり、出力には漏らさない）である。

復旧本線と provisioning 経路では公式 `bitwarden` Rust SDK を使う。`bw` CLI は BWS secret の取得には使わない。access token は YubiKey から取得し、必要な API 呼び出しの範囲だけで保持する。`pass-remote register`、`gpg-backup register`、`gpg-backup add-spare` など BWS を読む/書く Rust command は token を prompt / stdin で受け取らず、YubiKey storage の `bitwarden-client-secret` から取得する。

詳細設計は [Bitwarden Secrets Manager 復旧設計](./bitwarden-personal-vault-design.md) に置く。

### Bitwarden Password Manager

Bitwarden Password Manager は Web service passwords、passkeys、TOTP、recovery codes を保持する。repository はその email/master-password login を CLI に実装しない。Bitwarden account 自体の 2FA / passkey には primary と spare の両方を事前登録する。email / master password はこの repository の YubiKey storage と復旧 command の入力にしない。

### GnuPG / SSH

GPG key は software key として運用する。GPG key material は YubiKey に入れない。GPG secret key backup は Bitwarden Secrets Manager に保存する。

`pass` には encryption subkey を使う。GitHub SSH identity には authentication subkey を使う。Git signing には signing subkey を使う。GitHub private repository の取得は GPG authentication subkey による SSH clone で行い、GitHub API は使わない。鍵サーバー は使わない。既存の `~/.ssh/id_ed25519` は新規運用では使わない。

GPG 鍵リング 操作は `gpgme` を使う。OpenPGP（公開鍵規格） 公開鍵 操作が必要な場合は `sequoia-openpgp` を使う。`gpg` CLI は通常実装では使わない。

詳細設計は [GnuPG 復元 / gpg-agent SSH support 設計](./gnupg-ssh-design.md) に置く。

### Git

private `password-store` repository の clone は `git2` と SSH agent を使う。`git` CLI は復旧本線では使わない。SSH agent には gpg-agent の SSH support を使い、GPG authentication subkey 由来の identity を GitHub に提示する。

## 到達仕様の復旧フロー

1. `dotfiles secrets yubikey enroll-primary` で primary YubiKey に必要な bootstrap secret を登録し、ローカル確認 まで実行する。
2. スペア YubiKey がある場合は、`dotfiles secrets yubikey enroll-spare` で primary から bootstrap secret を読み出し、spare に再暗号化して保存し、ローカル確認 まで実行する。
3. Bitwarden、GitHub、Google、Apple など YubiKey を使う外部サービス に primary と spare を登録する。
4. `dotfiles secrets verify-yubikey` で、挿さっている YubiKey に必要な bootstrap secret があることを確認する。復旧前提の外部確認は BWS だけであり、`--check bws` または `--all` を使う。これは追加入力を要求しない。
5. client-secret を rotate した場合は `dotfiles secrets yubikey rotate-bws-token` で primary とすべての spare を更新する。
6. `dotfiles secrets restore-gpg` で Bitwarden Secrets Manager から GPG secret key backup を取得し、YubiKey recipient 付き encrypted envelope を検証・復号して GPG secret key を import する。
7. `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` で GPG authentication subkey 由来の SSH 公開鍵 を出力し、GitHub SSH keys に登録する。
8. `dotfiles secrets restore-pass` で Bitwarden Secrets Manager から `password-store-remote` を取得し、GPG authentication subkey 経由の SSH で clone する。

## 到達仕様のコマンド一覧

以下のコマンド節は、最終到達状態で備える公開インターフェースを定義する。現行の実装状況や即時利用可否はこの節では扱わない。

### `dotfiles secrets yubikey setup`

到達仕様では、YubiKey 5 PIV の利用前提を確認し、この機能で使う保存領域を確認する手段として提供する。既存の FIDO2 / OTP / OpenPGP（公開鍵規格） / PIV 認証情報 は reset しない。既存領域と衝突する場合は停止する。通常の利用者向け導線では `enroll-primary` / `enroll-spare` から内部的に扱う。

`setup` は管理操作であるため、設定済み PIV PIN を controlling TTY の hidden prompt から 1 回だけ受け取る。PIN は同じ process の PIN-protected management key 認証だけに一時利用し、stdin、argv、environment、stdout、stderr、log、一時ファイルへ出さない。復旧 read path と異なり PIN 入力が必要である。

### `dotfiles secrets yubikey put <name>`

YubiKey に secret を保存する。`<name>` は `bitwarden-client-secret` だけを許可する。secret 本文は hidden prompt または stdin から受け取る。平文を CLI 引数、ログ、一時ファイルに残さない。同名 secret の上書きには明示 option を必要とする。通常の primary / spare 登録では直接使わず、`enroll-primary` / `enroll-spare` を使う。
このコマンドは入力前に manifest と既存 object の状態を検証し、`--force` なしで上書きが必要な場合は secret を読まずに停止する。

### `dotfiles secrets yubikey status`

YubiKey に保存すべき bootstrap secret のうち、設定済みの名前を stdout へ 1 行ずつ出力する。secret 本文、encrypted blob、PIN は扱わない。専用 data object 領域が完全に空なら成功・空出力とする。正常な manifest がある状態では、bootstrap secret の設定済み集合が任意の subset（空集合を含む）でも成功とし、設定済み名だけを出力する。manifest 欠落なのに予約 object が残る状態、または manifest 不正は停止する。`status` は PIN / management-key authentication / GET METADATA を行わず、slot key/certificate 存在や manifest SPKI 一致を主張しない。この無対話列挙契約の外にある slot 整合性は、PIN を使う管理 preflight と実際の unwrap path が確認する。

`status` は PIV PIN を要求してはならない。PIN prompt が発生した場合は成功扱いにせず、実装回帰として停止する。

### provisioning script の終了コード契約

provisioning script が状態遷移の根拠にしてよい公開終了コードは次だけである。

- `42`: `yubikey status` が予約 storage の観測済み不整合を検出した。script はこの場合だけ `clear` へ移行してよい。
- `43`: `yubikey put` が完全に未初期化の専用領域を検出した。script はこの場合だけ `setup` 後に同じ `put` を再試行してよい。

これ以外の終了コードは状態に分類せず停止する。USB / PCSC / device discovery / serial 解決などの観測失敗を `42` または `43` に変換してはならない。

### `dotfiles secrets yubikey clear --yes`

再登録前の管理コマンドである。明示した `--serial <serial>`、または serial 未指定時に単一接続として解決できる YubiKey について、この機能用の manifest / bootstrap secret custom data object と slot `82` certificate を clear し、続けて slot `82` の専用 key を再生成する。これは既存 key を消去するのではなく、再生成によって置換する操作である。予約外の PIV object / slot、FIDO2、OTP、OpenPGP は変更しない。無 PIN の `status` が clear の根拠にできるのは、manifest 欠落かつ予約 object が残る状態、または manifest 不正だけである。slot key/certificate の残存・欠落は `status` が観測しないため終了コード 42 に分類せず、PIN を使う管理 preflight で検出して停止する。正常な manifest の任意の bootstrap secret subset では `clear` せず、保存済み secret を維持する。

`clear` は `--yes` の確認後にだけ、設定済み PIV PIN を controlling TTY の hidden prompt から受け取り、PIN-protected management key で認証する。確認不成立時は PIN を読まない。

### YubiKey PIV PIN の利用境界

PIV 管理操作である `setup`、`put`、`clear`、`enroll-primary`、`enroll-spare`、`rotate-bws-token` は、設定済み PIV PIN を controlling TTY の hidden prompt から受け取り、fresh handle ごとに `verify_pin`、PIN-protected management key の取得、management-key authentication を順に行う。wrong/blocked/opaque PIN error は default key、PUK、reset、retry へ fallback せず停止する。

`status`、`verify-yubikey`、`restore-gpg`、`restore-pass`、GPG backup/BWS provisioning の YubiKey 読み出し・復号経路は PIN を要求してはならない。特に無対話復旧の利用者契約を管理操作へ拡張しない。

### `dotfiles secrets yubikey enroll-primary`

primary YubiKey を復旧入口として初期登録する。1 本だけ接続されている YubiKey または `--serial <serial>` で明示された YubiKey を対象にし、専用 PIV 領域を setup し、`bitwarden-client-secret` だけを hidden prompt から受け取り、保存後にローカル確認を実行する。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。非対話または migration 用途に限り stdin からの入力を許可する。

PIV PIN 利用は [YubiKey PIV PIN の利用境界](#yubikey-piv-pin-の利用境界) に従う。`enroll-primary --stdin-json` でも PIN は JSON 用 stdin から読まず、controlling TTY の hidden prompt だけから受け取る。

### `dotfiles secrets yubikey enroll-spare`

spare YubiKey を復旧入口として初期登録する。通常は primary YubiKey から `bitwarden-client-secret` を読み出し、spare YubiKey の公開鍵で再暗号化して保存する。利用者に bootstrap secret の再入力を要求しない。外部サービスの FIDO2 / passkey / U2F / OTP 登録は自動化しない。
`--stdin-json` で bootstrap secret を渡す場合も、PIN は JSON payload に含めず、controlling TTY の hidden prompt だけから受け取る。

### `dotfiles secrets yubikey rotate-bws-token`

指定 YubiKey の `bitwarden-client-secret` を更新し、更新後に ローカル確認 を実行する。BWS 接続確認は ローカル保管 の検証とは別の外部確認として扱う。primary と spare を複数本運用する場合は、新しい token を一度だけ読み取り、更新ステップごとに 1 本だけ接続されている YubiKey または `--serial <serial>` で明示された YubiKey を更新する。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。serial 未指定の対話実行で同一実行内の継続 prompt に進む場合も、次の更新前に対象 YubiKey だけを接続する。複数本を接続したまま進める場合は同一実行で継続せず、`--serial` を指定して 1 本ずつ実行する。出力 要約 の serial を確認し、対象全本が更新済みになるまで更新する。非対話実行では `--serial` で 1 本だけを更新し、token は `--stdin` で渡せる。
token 入力前に ローカル保管 の復号可能性を確認し、更新不能な状態では新しい token を読まずに停止する。
`--stdin` で token を渡す場合も、PIN は token 用 stdin から読まず、controlling TTY の hidden prompt だけから受け取る。

### `dotfiles secrets verify-yubikey`

到達仕様では、挿さっている YubiKey が復旧入口として使えるか確認する機能として提供する。1 本だけ接続されている場合はその YubiKey を対象にし、serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。非対話実行では `--serial <serial>` で対象を明示する。secret 本文は stdout / stderr に出力しない。

到達仕様の確認項目:

- `bitwarden-client-secret` が YubiKey に保存され、touch を経て復号できる。

このコマンドはローカル保管確認だけを実行する。`--check bws` と `--all` は無対話の BWS 外部確認を要求する option なので、利用できない場合は明示的に失敗する。引数なし実行の要約では BWS 外部確認を機械可読状態値 `skipped` として残す。要約の状態値は `ok` と `skipped` を使い、表示文言は別層で扱う。email、master password、YubiKey OTP、session はこの command の外部確認に含めない。

このコマンドは GitHub、Google、Apple など外部サービス の FIDO2 / passkey / U2F 登録状況を検証しない。外部サービス の spare key 登録は各サービスの設定画面で確認する。

### `dotfiles secrets restore-gpg`

1. YubiKey から `bitwarden-client-secret` を取得する。
2. BWS project `dotfiles-secret-recovery` から `gpg-secret-key-backup` encrypted envelope を取得する。
3. envelope 形式（version / metadata / recipients / ciphertext）を検証し、接続中 YubiKey と一致する recipient が存在しない場合は停止する。
4. 接続中 YubiKey で data encryption key を unwrap し、復号済み backup を得る。
5. import 前に primary fingerprint をインメモリ導出し、envelope `metadata.primary_fingerprint` と一致しない場合は停止する。
6. 同一 primary fingerprint の secret key が既に鍵リングに存在する場合は停止する。
7. 復号済み backup を GPG secret key として import する。
8. encryption / authentication / signing subkey の存在と利用可能状態（revoked / expired / disabled でないこと）を検証する。
9. authentication subkey の keygrip を gpg-agent の SSH key list（`sshcontrol` 相当）へ登録する。既登録の場合はその状態を維持する（冪等）。
10. `gpg-agent` SSH support が有効で、authentication subkey が SSH identity として利用可能であることを確認する。

### `dotfiles secrets restore-pass`

YubiKey から `bitwarden-client-secret` を取得し、BWS project `dotfiles-secret-recovery` から `password-store-remote` を取得する。`~/.password-store` が存在しないことを確認し、GPG authentication subkey 経由の SSH で private repository を clone する。clone 後に `pass` が store を読めることを確認する。

### `dotfiles gpg export-ssh-public-key`

`--primary-fingerprint <40-hex-fingerprint>` で指定した primary key の GPG authentication subkey 由来の SSH 公開鍵 を stdout に出力する。GitHub SSH keys に登録するために使う。GitHub API は呼ばない。

## API / Command Policy

領域ごとの API / command policy は次のとおり。

- `YubiKey`: Rust crate を使い、`ykman` CLI、PIV reset、既存 認証情報 削除は使わない。
- `Bitwarden Secrets Manager`: 公式 `bitwarden` Rust SDK を使い、復旧本線で `bw` CLI は使わない。
- `Bitwarden Password Manager`: repository の recovery CLI surface 外である。email/master-password login 用の `bw` CLI、OTP、session、credential は、BWS secret の取得・保存・`verify-yubikey`・復旧のいずれにも使わない。
- `GnuPG`: `gpgme`、必要時の `sequoia-openpgp` を使い、通常実装で `gpg` CLI と鍵サーバーは使わない。
- `Git`: `git2` と SSH agent を使い、復旧本線で `git` CLI と GitHub API は使わない。

## 停止条件

- YubiKey の専用保存領域が利用できない、または既存 認証情報 と衝突する。
- 許可されていない secret name が指定された。
- 同名 secret が存在し、明示的な上書き option が指定されていない。
- BWS project `dotfiles-secret-recovery` から必要な secret が取得できない。
- `verify-yubikey` で YubiKey 内の bootstrap secret 確認に失敗する。
- `verify-yubikey --check bws` または `verify-yubikey --all` で Bitwarden Secrets Manager の無対話復旧前提確認に失敗する。
- `rotate-bws-token` の同一実行内で同一 serial を重複更新しようとした。
- `gpg-secret-key-backup` の envelope 形式検証（version / metadata / recipients / ciphertext）に失敗する。
- 接続中 YubiKey と一致する recipient が存在しない。
- data encryption key の unwrap または backup 復号に失敗する。
- 復号済み backup の primary fingerprint が envelope `metadata.primary_fingerprint` と一致しない。
- import 対象の GPG secret key に encryption / authentication / signing subkey が揃っていない、またはいずれかが revoked / expired / disabled で利用不能である。
- 同一 primary fingerprint の secret key が既に鍵リングへ存在する。
- `gpg-agent` SSH support が利用できない。
- authentication subkey 由来の SSH 公開鍵を解決できない。
- `~/.password-store` が既に存在する。
- `password-store-remote` が private repository の clone URL として妥当でない。

`bw` CLI の email/master-password login、OTP、session、credential の有無は、repository の recovery command の停止条件ではない。これらは Bitwarden Password Manager の別製品面であり、本仕様が定める BWS-only の復旧状態遷移へ入力・fallback・確認項目として混在させない。

[#11](https://github.com/wthrk/dotfiles/issues/11) と [#17](https://github.com/wthrk/dotfiles/issues/17) は、GPG、SSH、private `password-store` の復旧目的および統合作業単位を示す外部 issue である。repository 内で復旧 command の保存対象、入力、停止条件、検証対象を定める正本は本仕様とここから参照する設計文書であり、issue 本文の旧記述がこの BWS-only 契約と異なる場合は本仕様が supersede する。外部 issue 本体はこの repository の変更では編集しない。したがって外部 issue を単独で読む利用者には旧記述が残るリスクがあり、実装者・レビュー担当は必ず本仕様を併読して本契約を適用する。

## 参考

- Yubico Getting Started with Your YubiKey: https://support.yubico.com/hc/en-us/articles/5041539306780-Getting-Started-with-Your-YubiKey
- Yubico Authenticator spare YubiKey tips: https://docs.yubico.com/software/yubikey/tools/authenticator/auth-guide/tips.html
