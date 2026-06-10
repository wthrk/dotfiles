# 新規マシン展開のための初期プロビジョニング & 復旧 runbook

`secret-recovery` 一式を新規マシンへ展開できる状態にするための、ソース（プロビジョニング元）環境での初期登録と、新規マシンでの復旧の実行手順。各工程を **`dotfiles` コマンドで実現済みか / コマンドが無い手動操作か** で厳密に分ける。

正本（挙動・停止条件・形式・token モデルの定義）:
[secret-recovery-spec.md](secret-recovery-spec.md) /
[bitwarden-secrets-manager-design.md](bitwarden-secrets-manager-design.md#初期登録手順) /
[yubikey-secret-storage-design.md](yubikey-secret-storage-design.md) /
[gnupg-ssh-design.md](gnupg-ssh-design.md)

## 凡例

- **[CMD]**: `dotfiles` / `dotfiles secrets` のコマンド、またはこの repository の provisioning script で実現済み。手順としてはそのコマンドまたは script を実行する。
- **[外部確認]**: provisioning script の本体工程では停止しない外部確認事項。アカウントの 2FA / passkey 登録や recovery code 保管など、各サービス側の物理操作・アカウント操作を指す。
- **[対話]**: PIN / secret / touch / passphrase の対話入力を伴うため実端末で実行する。

## コマンド有無の対応表（この runbook の基準）

| 工程 | 実現 |
| --- | --- |
| 環境適用（home-manager / nix-darwin） | **[CMD]** `dotfiles switch [home\|darwin\|all]` |
| YubiKey PIV 初期化 | **[CMD]** `dotfiles secrets yubikey setup` |
| `gpg-secret-key-backup` の既存 2 recipient envelope 照合 | **[CMD]** `dotfiles secrets gpg-backup register` |
| `password-store-remote` 保存（clone URL → BWS） | **[CMD]** `dotfiles secrets pass-remote register` |
| YubiKey へ bootstrap secret 登録 | **[CMD]** `dotfiles secrets yubikey enroll-primary` / `enroll-spare` |
| YubiKey / BWS / bw-login 検証 | **[CMD]** `dotfiles secrets verify-yubikey --all` |
| GPG 復元 | **[CMD]** `dotfiles secrets restore-gpg` |
| authentication subkey 由来 SSH 公開鍵の出力 | **[CMD]** `dotfiles gpg export-ssh-public-key` |
| `password-store` の SSH clone | **[CMD]** `dotfiles secrets restore-pass` |
| Bitwarden Password Manager login / unlock | **[CMD]** `dotfiles secrets bw-login` |
| BWS token rotate を全 YubiKey へ反映 | **[CMD]** `dotfiles secrets yubikey rotate-bws-token` |
| GPG secret key の生成または選択 | **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |
| Bitwarden Secrets Manager project `dotfiles-secret-recovery` の作成 | **[CMD][対話]** `dotfiles secrets ...` provisioning command または `scripts/provision-secret-recovery-source.sh`（0 件なら作成、1 件なら使用、複数なら停止） |
| private `password-store` repo の作成、local store 初期化、remote 設定・push | **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |
| GitHub への SSH 公開鍵登録 | **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |
| 各サービスの YubiKey 2FA・FIDO2 登録 | **[外部確認]**（spec: 物理キー登録は自動化しない） |
| Bitwarden Secrets Manager への `password-store-remote` 登録と `gpg-secret-key-backup` 照合 | **[CMD]** `dotfiles secrets gpg-backup register` / `pass-remote register`、または **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |
| YubiKey への `bws-access-token` 保存 | **[CMD][対話]** `dotfiles secrets yubikey put bws-access-token`、または **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |

---

## Phase A — ソース環境での初期プロビジョニング（一度だけ）

> primary / spare の YubiKey 2 本を同時期に用意する。primary 紛失後に spare を後付けできないため spare を事前登録する（spec「スペア YubiKey 運用」）。

1. **[CMD]** 環境適用: `dotfiles switch home`（必要なら `dotfiles switch all`）。gpg-agent SSH support（`enable-ssh-support`）が有効になる。

2. **[外部確認]** Bitwarden Password Manager のアカウント（login email / master password）を用意し、account の 2FA として primary と spare の YubiKey を別々に登録する。
   - Bitwarden Web vault に login し、Account settings / Security / Two-step login 相当の設定カテゴリを開く。
   - Two-step login の provider 一覧で Security key / YubiKey / passkey 相当を選び、登録開始操作を行う。provider 名は UI 変更で揺れるため、FIDO2/WebAuthn security key を登録する項目を選ぶ。
   - primary YubiKey を挿し、ブラウザまたは OS の security key prompt で認証を進め、要求されたら YubiKey をタッチする。登録名には `primary` と分かる名前を付け、保存後に provider の登録済み key 一覧へ表示されることを確認する。
   - spare YubiKey に差し替え、同じ provider で追加登録を行う。登録名には `spare` と分かる名前を付け、primary とは別の登録済み key として表示されることを確認する。同じ物理キーを 2 回登録した状態で済ませない。
   - Recovery code / account recovery code の表示または再生成操作を行い、値そのものをこの repository に書かず、Bitwarden 外の安全な保管先へ保存する。保存後は recovery code の保管場所と更新日だけを作業メモに残し、code 本体は残さない。
   - いったん Web vault から logout し、login email / master password と primary YubiKey で再 login できることを確認する。spare も別ブラウザ session または再 logout 後の login で確認し、両方の key が実際の 2FA prompt で使える状態にする。
   - `bw-email` / `bw-password` を YubiKey に保存する前に、login email / master password と 2FA 登録状態を再確認する。後段の `verify-yubikey --all` は YubiKey 内の bootstrap secret と Bitwarden login / unlock 経路を確認するが、Bitwarden account 側に spare security key が残っているかは Web vault 側で確認する。

3. **[CMD]** GPG secret key は script が解決または作成する。要件: 1 つの primary key と **encryption / authentication / signing** capability の subkey をそれぞれ持ち、いずれも revoked/expired/disabled でないこと（gnupg-ssh-design L32/L80-81）。
   - `scripts/provision-secret-recovery-source.sh` は、設定済み `password-store` に `.gpg-id` があれば、その recipient がすべて単一の GPG primary key に解決できる場合だけ secret key を検証して使う。`.gpg-id` 内の recipient が複数 primary に解決される場合は停止する。`.gpg-id` がない、または空の場合は、local GPG secret key が 1 件だけならそれを使い、local key がなければ新規 GPG secret key を作成する。複数の eligible local key がある場合は停止し、先に password-store を対象 key で初期化させる。既存 key は revoked / expired / disabled でないものだけを採用する。
   - script は解決または作成した primary key が revoked / expired / disabled でないことを確認し、encryption / authentication / signing subkey が不足していれば追加する。既存 key と既存 `.gpg-id` が有効な場合は作り直さない。
   - 新規作成時の UID は `git config --global user.name` / `user.email`、local user/host の順に導出する。ユーザー入力を要求しない。
   - `gpg --list-secret-keys --with-subkey-fingerprint --keyid-format long` 相当で対象 key を表示し、primary fingerprint が lowercase hex 40 文字として控えられること、各 subkey が revoked / expired / disabled になっていないことを確認する。
   - 鍵バックアップ操作と revocation certificate 作成操作を実行し、secret key backup と revocation certificate を repository 外の安全な保管先へ保存する。保管先名、作成日、primary fingerprint だけを作業メモに残し、secret key material や passphrase はこの repository に書かない。

4. **[CMD][対話] または [外部確認]** GitHub に private な `password-store` repository を用意し（clone URL は `git@github.com:<owner>/<repo>.git` 形式）、local `password-store` を初期化または確認してから remote を設定して push する。
   - `scripts/provision-secret-recovery-source.sh` を使う場合、`.gpg-id` がない、または空の local store では、前段で解決した GPG secret key で `pass init` を実行して `.gpg-id` を作成する。設定済み `.gpg-id` がある場合は recipient の secret key を検証してそのまま使う。
   - script は既存 `origin` がある場合、その SSH / HTTPS clone URL から GitHub repository identity を解決して使う。HTTPS origin は既存 Git remote として尊重し、上書きしない。BWS の `password-store-remote` へ保存する値は `dotfiles secrets pass-remote register` が既存 origin を repository identity として観測し、CLI/application 側で `git@github.com:<owner>/<repo>.git` へ正規化した値を使う。script は導出した SSH clone URL を argv、pipe、環境変数で中継しない。既存 `origin` と登録対象 repository が矛盾する場合は上書きせず停止する。
   - script は既存 `origin` がない場合、GitHub active account から `<login>/password-store` を導出する。GitHub repository が存在しなければ private repository として作成し、存在する場合は private であることを確認して使う。既存 repository を削除・再作成しない。
   - script は local `password-store` が Git repository でなければ `pass git init` を実行し、初期 branch/upstream を設定して push する。既存 Git repository がある場合は現在 branch を維持する。既存 upstream が remote として解決できる場合は push 先 remote が登録対象 repository と一致することを確認してから push し、不一致または remote URL 未解決なら上書きせず停止する。upstream がない、または remote として解決できない形式の場合は、検証済み `origin` に対して現在 branch の upstream を設定する。
   - 別途行う場合は、GitHub の Repositories / New repository 相当の画面で repository 名を入力し、Visibility は Private を選ぶ。README や template の有無は既存運用に合わせるが、空 repository で始める場合は local `pass` 初期化後に初回 push する。
   - repository 作成後、Code / SSH clone URL 相当の表示で `git@github.com:<owner>/<repo>.git` 形式の URL を確認する。既存 origin がない手動実行では、この URL は `pass-remote register` が CLI の input port で受け取る値である。script 実行時や既存 origin 利用時は CLI/application が repository identity から BWS 登録用 SSH clone URL を導出し、script から argv、pipe、環境変数で中継しない。secret ではないが repository 名が分かるため必要な作業メモだけに残す。
   - push 後、GitHub repository の Code / file list 相当の画面で `.gpg-id` と password-store commit が表示されることを確認する。保存するのは clone URL と repository の所在だけで、password-store 内の secret 値は runbook や作業メモへ書かない。

5. **[CMD]→[外部確認]** GitHub の SSH identity 登録:
   - **[CMD]** `dotfiles gpg export-ssh-public-key` で authentication subkey 由来の SSH 公開鍵を出力する。
   - **[外部確認]** GitHub の Settings / SSH and GPG keys 相当の画面で、出力した SSH 公開鍵を account の SSH keys に登録する。Title には復旧用 GPG authentication subkey 由来であることが分かる名前を付け、Key 欄には `dotfiles gpg export-ssh-public-key` の公開鍵だけを貼り付ける。ここで登録する SSH key は GitHub login 用の security key / passkey ではないため、次の 2FA 登録と混同しない。
   - **[外部確認]** GitHub API や既存 script で SSH key 登録を行う場合、実行直前に `gh auth refresh -h github.com -s admin:public_key` を実行し、GitHub 側で SSH public key を管理できる scope を現在の `gh` 認証へ追加する。scope 追加後に script を実行し、SSH keys 画面で登録結果を確認する。GitHub credential の値そのものは記録せず、argv、環境変数、作業メモに残さない。
   - **[外部確認]** GitHub の Settings / Password and authentication / Two-factor authentication / Security keys 相当の設定カテゴリを開き、Security keys / Passkeys の add / register 操作を選ぶ。primary YubiKey を挿して browser / OS prompt で touch し、登録名に `primary` と分かる名前を付けて保存する。
   - **[外部確認]** 同じ GitHub 2FA 画面で spare YubiKey を追加登録し、登録名に `spare` と分かる名前を付けて保存する。登録後、Password and authentication 画面で primary / spare が別々の security key として表示されていることを確認する。

6. **[外部確認]** Google / Apple など YubiKey を使う各サービスで、primary / spare を別々に登録する。
   - 各サービスの Account security / Sign-in and security / Password and security 相当の設定カテゴリを開き、Passkey / Security key / 2-step verification 相当の画面で add / create / register 操作を選ぶ（FIDO2 / passkey / U2F）。UI 名称はサービスごとに異なるため、sign-in method として物理 security key を追加する項目を選ぶ。
   - primary YubiKey を挿し、ブラウザまたは OS の prompt に従って touch / PIN 入力を行い、登録名または device label を設定できる場合は `primary` と分かる名前で保存する。登録完了画面または sign-in methods 一覧で primary が有効になっていることを確認する。
   - spare YubiKey に差し替え、同じ画面で add / register 操作を繰り返す。登録名または device label を設定できる場合は `spare` と分かる名前で保存し、primary とは別の sign-in method として表示されることを確認する。
   - OTP 要求サービスでは Yubico OTP を primary / spare それぞれに登録する。OATH TOTP を使う場合は同一 TOTP secret / QR code を primary と spare の両方へ登録し、既存 secret を取り出せない場合はサービス側で TOTP を再設定する。
   - 登録後、各サービスの security / recovery 画面で recovery options / backup codes / account recovery contact / trusted phone number / trusted email 相当を確認する。backup code を表示または再生成した場合は、値そのものをこの repository に書かず、サービスごとの推奨に従って別経路の安全な保管先へ保存する。
   - 既存の phone / email fallback が失効済み番号・古いメールアドレス・共有アカウントになっていないか確認し、必要なら更新する。更新後、primary / spare の両方と recovery option が同じ security 画面で有効な回復経路として残っていることを確認する。

7. **[CMD][対話]** Bitwarden Secrets Manager で `password-store-remote` を登録し、`gpg-secret-key-backup` を照合する。
   - `dotfiles secrets gpg-backup register` / `pass-remote register` と `scripts/provision-secret-recovery-source.sh` は、BWS 登録用 access token から見える project `dotfiles-secret-recovery` を解決し、1 件なら使用、0 件なら作成、複数件なら停止する。`pass-remote register` は `password-store-remote` の登録または既存照合を行い、`gpg-backup register` は既存 `gpg-secret-key-backup` envelope の照合だけを行う。script は project 作成のために途中停止しない。
   - この runbook は organization / machine account / service account の作成を前提にしない。Bitwarden UI の名称は変わりうるため、「project `dotfiles-secret-recovery` に `gpg-secret-key-backup` と `password-store-remote` を保存し、その 2 secret を読める復旧用 token を YubiKey に保存する」という保存モデルだけを照合する。
   - BWS 登録用 access token を `dotfiles secrets pass-remote register` と `dotfiles secrets gpg-backup register` の hidden prompt または pipe に渡す。token 値そのものを argv、log、runbook、review artifact に書かない。
   - YubiKey に保存する `bws-access-token` は、復旧時に `dotfiles-secret-recovery` project の必要 secret を読める最小権限の復旧用 token とする。BWS 登録用 token と同一値にしない。
   - `pass-remote register` は `password-store-remote` の secret note に、BWS 登録用 access token の provenance marker を保存する。後続の `yubikey put bws-access-token` / `enroll-primary` / `enroll-spare` / `rotate-bws-token` は、この marker を候補 token の opaque token id と照合し、一致時と marker 不在時の両方で停止する。
  - `password-store-remote` は設定済みまたは作成済み password-store の GitHub SSH clone URL を使い、`dotfiles secrets pass-remote register` で登録する。既存 origin が SSH / HTTPS GitHub URL の場合は CLI/application が repository identity として許容し、BWS 登録値には `git@github.com:<owner>/<repo>.git` へ正規化した値を使う。既存 origin がない手動実行では URL を CLI の input port が受け取る。script から SSH clone URL を argv、pipe、環境変数で中継しない。BWS に同名 secret が 0 件なら作成する。1 件は configured origin から導ける期待値と一致するときだけ既存値を使用し、configured origin が無い場合や不一致の場合は停止する。複数件なら停止する。
   - `gpg-secret-key-backup` は設定済みまたは作成済み password-store `.gpg-id` recipient が単一 primary に解決できる場合、その primary fingerprint を `dotfiles secrets gpg-backup register` が登録対象にする。鍵リングに複数の使用可能な secret primary key があっても、`.gpg-id` が単一 primary を指す場合はその既存設定を使う。password-store が未設定、または `.gpg-id` が存在しない / 空の場合だけ、使用可能な secret primary key の 0 件 / 1 件 / 複数件を一意解決し、0 件または複数件では停止する。BWS に同名 secret が 0 件の場合、現行 CLI は接続中 YubiKey 1 本の recipient しか取得できないため 1 recipient の新規 envelope を作成せず停止する。既存 1 件なら envelope metadata の primary fingerprint、接続中 recipient、primary/spare の 2 recipient 以上が揃う場合だけ使用し、不一致、1 recipient のみ、または複数件なら停止する。BWS 取得成功だけを plaintext 復旧完了として扱わない。
   - `scripts/provision-secret-recovery-source.sh` を使う場合は、password-store `.gpg-id` recipient の GPG secret key 確認または未初期化 store の `pass init`、encryption/authentication/signing subkey の不足追加、GitHub private repository 作成または確認、store の Git remote 設定 / push、authentication subkey 由来 SSH 公開鍵の GitHub 登録、BWS project `dotfiles-secret-recovery` の作成または使用、BWS への `password-store-remote` 登録と `gpg-secret-key-backup` 既存 2 recipient envelope の検証、CLI が対象にした YubiKey への復旧用 `bws-access-token` 保存を実行する。`gpg-secret-key-backup` が未登録または 1 recipient の場合は script も未充足として停止し、登録成功扱いにしない。空の BWS から `gpg-secret-key-backup` を初回作成して初期プロビジョニングを完了させる script ではなく、初回 envelope 作成はこの script / command の範囲外である。script は途中停止型の外部確認 gate を出さない。script は YubiKey の識別子を要求・指定せず、1 本だけ接続されている場合だけ dotfiles CLI が対象にし、複数本なら識別情報を出さず停止して対象 1 本だけの接続を求める。script は利用者入力値を読まず、BWS 登録用 token、password-store remote URL、GPG primary fingerprint、YubiKey 保存用 token などを `dotfiles` CLI へ値付き argv / pipe / stdin で中継しない。現在の repository HEAD の CLI を使う場合は固定フラグ `--repo-head` または固定 toggle `DOTFILES_PROVISION_USE_REPO_HEAD=1` を使い、この指定に secret/credential 値を含めない。

8. **[CMD][対話]** `bws-access-token` を primary / spare YubiKey へ保存する。
   - primary 登録では `dotfiles secrets yubikey enroll-primary` で `bw-email`、`bw-password`、復旧用 `bws-access-token` を保存する。個別保存を使う場合は `dotfiles secrets yubikey put bw-email`、`dotfiles secrets yubikey put bw-password`、`dotfiles secrets yubikey put bws-access-token` をそれぞれ実行する。
   - spare 登録では `dotfiles secrets yubikey enroll-spare` で CLI prompt/input port から bootstrap secret を受け取り、spare に保存する。個別保存で運用する場合も primary / spare の両方に 3 secret が揃っていることを確認する。
   - `scripts/provision-secret-recovery-source.sh` が保存するのは復旧用 `bws-access-token` だけである。`bw-email` / `bw-password` は `enroll-primary` / `enroll-spare` または個別の `put` で保存する。
   - Bitwarden Password Manager 用の `bw-email` / `bw-password` と、Bitwarden Secrets Manager 用の `bws-access-token` を混同しない。Password Manager は `bw-login` の login / unlock、Secrets Manager は `restore-gpg` / `restore-pass` / `verify-yubikey --check bws` の secret 操作に使う。
   - `bws-access-token` 保存前提は、同じ token で `dotfiles-secret-recovery` project の `password-store-remote` とその provenance marker が読めることである。`pass-remote register` が未完了、または marker が欠けた状態では CLI が fail-closed で停止する。

9. **[CMD][対話]** 検証（primary / spare 双方で実施。bw-login は無条件 `bw login`→`unlock` のため再実行前に `bw logout`）:
   ```sh
   dotfiles secrets verify-yubikey --all
   ```

---

## Phase B — 新規マシンでの復旧（spec「到達仕様の復旧フロー」L102-112）

1. **[CMD]** `dotfiles switch`（または `scripts/bootstrap.sh`）で環境を適用し gpg-agent SSH support を有効化する。
2. **[CMD][対話]** `dotfiles secrets verify-yubikey --all`（必要に応じ `--check bws` / `--check bw-login` を個別指定）。
3. **[CMD][対話]** `dotfiles secrets restore-gpg` で BWS の `gpg-secret-key-backup` から GPG secret key を復元する。
4. **[CMD]→[外部確認]** `dotfiles gpg export-ssh-public-key` で SSH 公開鍵を出力し、未登録なら GitHub に登録する。
   - GitHub の Settings / SSH and GPG keys 相当の画面で、既存 SSH keys に同じ authentication subkey 由来の公開鍵が登録済みか確認する。登録済みなら Title と最終利用状況を確認し、重複登録しない。
   - 未登録の場合は New SSH key / Add SSH key 相当を選び、Title には新規マシン復旧用の GPG authentication subkey 由来であることが分かる名前を入力し、Key 欄へ `dotfiles gpg export-ssh-public-key` の出力だけを貼り付ける。private key や GPG secret key は貼り付けない。
   - 保存後、SSH keys 一覧に新しい key が表示されることを確認し、必要なら `ssh -T git@github.com` 相当で account 認証先を確認する。保管するのは登録日、GitHub account、key title、関連 primary fingerprint だけで、秘密鍵素材は記録しない。
5. **[CMD]** `dotfiles secrets restore-pass` で BWS の `password-store-remote` から private `password-store` repository を clone する。
6. **[CMD][対話]** `dotfiles secrets bw-login` — Bitwarden Password Manager に login / unlock。

---

## BWS token rotate 時

BWS access token を rotate する場合は、新 token を有効化してから `dotfiles secrets yubikey rotate-bws-token` で primary と spare の全 YubiKey に反映する。反映後に `dotfiles secrets verify-yubikey --check bws` で BWS 読取、`gpg-secret-key-backup` envelope schema、primary_fingerprint 正規化、接続中 YubiKey recipient matching、unwrap-free recoverability を確認し、全本の確認後に旧 token を失効させる。token 値や断片を runbook、shell history、ログ、永続環境変数に残さない。
