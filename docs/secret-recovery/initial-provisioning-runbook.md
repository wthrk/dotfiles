# 新規マシン展開のための初期プロビジョニング & 復旧 runbook

`secret-recovery` 一式を新規マシンへ展開できる状態にするための、ソース（プロビジョニング元）環境での初期登録と、新規マシンでの復旧の実行手順。各工程を **`dotfiles` コマンドで実現済みか / コマンドが無い手動操作か** で厳密に分ける。

正本（挙動・停止条件・形式・token モデルの定義）:
[secret-recovery-spec.md](secret-recovery-spec.md) /
[bitwarden-personal-vault-design.md](bitwarden-personal-vault-design.md) /
[yubikey-secret-storage-design.md](yubikey-secret-storage-design.md) /
[gnupg-ssh-design.md](gnupg-ssh-design.md)

## 凡例

- **[CMD]**: `dotfiles` / `dotfiles secrets` のコマンド、またはこの repository の provisioning script で実現済み。手順としてはそのコマンドまたは script を実行する。
- **[手動]**: `dotfiles` にコマンドが無い操作。spec/design が「自動化しない（手動または各サービスの公式管理 API）」と定める範囲、もしくは利用者環境固有の前提（GPG 鍵・repo 等）。
- **[対話]**: secret / touch / passphrase / PIV 管理 PIN などの対話入力を伴うため実端末で実行する。PIV 管理操作だけは設定済み PIN を controlling TTY の hidden prompt から受け取る。復旧 read/decrypt 操作では PIV PIN を要求しない。

YubiKey の管理操作（`setup`、`put`、`clear`、enroll、rotate）は、設定済み PIV PIN を controlling TTY の hidden prompt から一時取得し、PIN-protected management key で認証して実行する。復旧 read/decrypt 操作（`status`、`verify-yubikey`、`restore-gpg`、`restore-pass`）は PIN prompt を出さずに完了または失敗しなければならない。PIN は stdin、argv、environment、stdout、stderr、log、一時ファイルへ出さない。serial 未指定時は接続中の単一 YubiKey を共通の device discovery で直接対象にし、接続なし・複数接続では停止する。

### 無対話復旧の利用者操作

新規マシンの復旧では、利用者は **YubiKey を挿して復旧コマンドを実行するだけ** で開始する。`dotfiles secrets verify-yubikey --check bws`（または `--all`）、`dotfiles secrets restore-gpg`、`dotfiles secrets restore-pass` は、YubiKey 保存の `bitwarden-client-secret` を内部で一時利用する。master password、session、PIV PIN、secret を渡す environment variable / argv、YubiKey OTP、または追加の対話入力を要求してはならない。これらの credential、BWS access token、BWS response、復号途中値は stdout、stderr、log、一時ファイル、永続 environment へ出力・保存せず、use 後に破棄する。

YubiKey に保存する bootstrap secret は `bitwarden-client-secret` だけである。`verify-yubikey --all`、`restore-gpg`、`restore-pass` は email、master password、OTP、session を入力・成功条件・確認項目に混在させず、OTP を自動供給する仕組みを実装・仮定しない。Bitwarden Password Manager login は repository の CLI surface 外である。正本は [secret-recovery-spec.md の無対話復旧の利用者契約](secret-recovery-spec.md#無対話復旧の利用者契約) である。

## コマンド有無の対応表（この runbook の基準）

| 工程 | 実現 |
| --- | --- |
| 環境適用（home-manager / nix-darwin） | **[CMD]** `dotfiles switch [home\|darwin\|all]` |
| PIV management key の非既定化と PIN 保護 | **[手動]** operator が [YubiKey Manager PIV Commands: `change-management-key`](https://docs.yubico.com/software/yubikey/tools/ykman/PIV_Commands.html#ykman-piv-access-change-management-key-options) の管理手順に従って、各 `setup` / `clear` / enroll / `put` / rotate の前に実施。`dotfiles` CLI / script は実施しない。 |
| YubiKey PIV 初期化 | **[CMD]** `dotfiles secrets yubikey setup` |
| `gpg-secret-key-backup` 作成（recipient wrap） | **[CMD]** `dotfiles secrets gpg-backup register` |
| `gpg-secret-key-backup` に spare recipient 追加 | **[CMD]** `dotfiles secrets gpg-backup add-spare` |
| `password-store-remote` 保存（clone URL → BWS） | **[CMD]** `dotfiles secrets pass-remote register` |
| YubiKey へ bootstrap secret 登録 | **[CMD]** `dotfiles secrets yubikey enroll-primary` / `enroll-spare` |
| YubiKey / BWS 復旧前提検証 | **[CMD]** `dotfiles secrets verify-yubikey --all` |
| GPG 復元 | **[CMD]** `dotfiles secrets restore-gpg` |
| authentication subkey 由来 SSH 公開鍵の出力 | **[CMD]** `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` |
| `password-store` の SSH clone | **[CMD]** `dotfiles secrets restore-pass` |
| Bitwarden Password Manager email/master-password login | **[手動]**（repository の CLI surface 外） |
| BWS token rotate を全 YubiKey へ反映 | **[CMD]** `dotfiles secrets yubikey rotate-bws-token`（各更新ステップで 1 本だけ接続、または `--serial <serial>` を指定して 1 本ずつ実行） |
| GPG secret key の生成 | **[手動]**（`dotfiles` にコマンドなし） |
| BWS project `dotfiles-secret-recovery` の作成 | **[手動]**（`dotfiles` / script は project を作成しない） |
| private `password-store` repo の作成と既存 store の remote 設定・push | **[CMD][対話]** `scripts/provision-secret-recovery-source.sh`、または **[手動]** |
| GitHub への SSH 公開鍵登録 | **[CMD][対話]** `scripts/provision-secret-recovery-source.sh`、または **[手動]** |
| 各サービスの YubiKey 2FA・FIDO2 登録 | **[手動]**（spec: 物理キー登録は自動化しない） |
| YubiKey への `bitwarden-client-secret` 保存 | **[CMD][対話]** `dotfiles secrets yubikey put bitwarden-client-secret --stdin`、または **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |
| Bitwarden Secrets Manager provisioning | **[CMD]** `dotfiles secrets gpg-backup register` / `add-spare` / `pass-remote register`、または **[CMD][対話]** `scripts/provision-secret-recovery-source.sh` |

---

## Phase A — ソース環境での初期プロビジョニング（一度だけ）

> primary / spare の YubiKey 2 本を同時期に用意する。primary 紛失後に spare を後付けできないため spare を事前登録する（spec「スペア YubiKey 運用」）。

### Phase A 前の必須 gate — PIV management key

primary / spare の各 YubiKey について、**Phase A の `setup`、`clear`、`enroll-primary`、`enroll-spare`、`put`、rotate を始める前**に、operator が公式の Yubico 管理ツールで PIV management key を既定値から変更し、PIN-protected management key として確立・確認する。この手動操作の正本は [YubiKey Manager PIV Commands の `change-management-key`](https://docs.yubico.com/software/yubikey/tools/ykman/PIV_Commands.html#ykman-piv-access-change-management-key-options) である。同資料は management key が key-pair generation などの管理機能を保護し、`--protect` が新しい management key を PIN 保護で YubiKey に保存することを定義する。firmware ごとの algorithm、現在の key / PIN の入力、確認方法、失敗時の扱いはこの一次資料の適用範囲に従う。この runbook は key 値、実機コマンド列、または reset / PUK 回復の手順を推測して再掲しない。

`dotfiles` CLI と provisioning script は factory-default management key の認証、management key の bootstrap、`MgmKey::generate_for`、`MgmKey::set_protected` を実行しない。固定 `yubikey` source で `MgmKey::get_protected` の `NotFound` origin を区別できないため、管理 key が未設定・不明・認証不能、または metadata が健全でない場合は fallback や状態変更をせず停止する。operator が上記 gate を完了・確認してからだけ、repository の `setup` / enroll command に進む。根拠と SDK の適用範囲は [外部 SDK 統合の一次資料](external-sdk-evidence.md#yubikey-piv--yubikey-crate) を参照する。

`yubikey status` の終了コード `42` は予約 storage の観測済み不整合だけを示し、management key の準備完了を示さない。[仕様の終了コード契約](secret-recovery-spec.md#provisioning-script-の終了コード契約) により script はこの場合だけ `clear` へ移行できるが、`clear` も上記 gate の完了後にだけ実行できる。management key が未設定・不明・認証不能、または metadata が健全でない場合、script / CLI は factory-default key、B0 bootstrap、reset、PUK、retry を使わず停止する。

repository の test、agent 作業、通常の検証では物理 YubiKey / PCSC を使用しない。実機の観測・操作はこの runbook を根拠に実行せず、`secrets-internal-test-stub` feature で compile-time に隔離した stub だけを使う。device-specific な確認は別の明示 task と人間の承認済み手順が必要である。

1. **[CMD]** 環境適用: `dotfiles switch home`（必要なら `dotfiles switch all`）。gpg-agent SSH support（`enable-ssh-support`）が有効になる。

2. **[手動]** Bitwarden Password Manager のアカウント（login email / master password）を用意し、account の 2FA として primary と spare の YubiKey を別々に登録する。
   - Bitwarden Web vault に login し、Account settings / Security / Two-step login 相当の設定カテゴリを開く。
   - Two-step login の provider 一覧で Security key / YubiKey / passkey 相当を選び、登録開始操作を行う。provider 名は UI 変更で揺れるため、FIDO2/WebAuthn security key を登録する項目を選ぶ。
   - primary YubiKey を挿し、ブラウザまたは OS の security key prompt で認証を進め、要求されたら YubiKey をタッチする。登録名には `primary` と分かる名前を付け、保存後に provider の登録済み key 一覧へ表示されることを確認する。
   - spare YubiKey に差し替え、同じ provider で追加登録を行う。登録名には `spare` と分かる名前を付け、primary とは別の登録済み key として表示されることを確認する。同じ物理キーを 2 回登録した状態で済ませない。
   - Recovery code / account recovery code の表示または再生成操作を行い、値そのものをこの repository に書かず、Bitwarden 外の安全な保管先へ保存する。保存後は recovery code の保管場所と更新日だけを作業メモに残し、code 本体は残さない。
   - いったん Web vault から logout し、login email / master password と primary YubiKey で再 login できることを確認する。spare も別ブラウザ session または再 logout 後の login で確認し、両方の key が実際の 2FA prompt で使える状態にする。
   - login email / master password と 2FA 登録状態を service 側で再確認する。これらを YubiKey へ保存しない。`verify-yubikey --all` は BWS recovery prerequisite だけを確認し、Bitwarden Password Manager login・OTP・spare security key 登録を確認しない。

3. **[手動]** GPG secret key を用意する。要件: 1 つの primary key と **encryption / authentication / signing** capability の subkey をそれぞれ持ち、いずれも revoked/expired/disabled でないこと（gnupg-ssh-design L32/L80-81）。
   - 実端末で GnuPG の key generation / edit-key 相当の操作を行い、primary key を作成した後、encryption / authentication / signing の用途ごとに subkey を追加する。UI やコマンドの表記は GnuPG の版で揺れるため、capability 表示で `E` / `A` / `S` 相当がそれぞれ別 subkey に付いていることを確認する。
   - `gpg --list-secret-keys --with-subkey-fingerprint --keyid-format long` 相当で対象 key を表示し、primary fingerprint が lowercase hex 40 文字として控えられること、各 subkey が revoked / expired / disabled になっていないことを確認する。
   - 鍵バックアップ操作と revocation certificate 作成操作を実行し、secret key backup と revocation certificate を repository 外の安全な保管先へ保存する。保管先名、作成日、primary fingerprint だけを作業メモに残し、secret key material や passphrase はこの repository に書かない。

4. **[手動]** GitHub に private な `password-store` repository を用意し（clone URL は `git@github.com:<owner>/<repo>.git` 形式）、既存 password-store の `.gpg-id` recipient がローカル GPG secret key で復号可能なことを確認してから remote を設定して push する。
   - GitHub の Repositories / New repository 相当の画面で repository 名を入力し、Visibility は Private を選ぶ。README や template の有無は既存運用に合わせるが、空 repository で始める場合は local `pass` 初期化後に初回 push する。
   - repository 作成後、Code / SSH clone URL 相当の表示で `git@github.com:<owner>/<repo>.git` 形式の URL を控える。この URL は `pass-remote register --url` に渡す値であり、secret ではないが repository 名が分かるため必要な作業メモだけに残す。
   - ソース環境の実端末で既存 `~/.password-store/.gpg-id` を確認し、対象 recipient の GPG secret key がローカル keyring に存在することを確認する。未初期化 store をここで新規生成する場合は script ではなく手動で `pass init` 相当を行い、`.gpg-id` が対象 key を指す状態を先に作る。
   - 既存 local `password-store` を Git repository として初期化または確認し、GitHub の SSH remote に push する。push 後、GitHub repository の Code / file list 相当の画面で `.gpg-id` と password-store commit が表示されることを確認する。保存するのは clone URL と repository の所在だけで、password-store 内の secret 値は runbook や作業メモへ書かない。

5. **[CMD]→[手動]** GitHub の SSH identity 登録:
   - **[CMD]** `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` で authentication subkey 由来の SSH 公開鍵を出力する。
   - **[手動]** GitHub の Settings / SSH and GPG keys 相当の画面で、出力した SSH 公開鍵を account の SSH keys に登録する。Title には復旧用 GPG authentication subkey 由来であることが分かる名前を付け、Key 欄には `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` の公開鍵だけを貼り付ける。ここで登録する SSH key は GitHub login 用の security key / passkey ではないため、次の 2FA 登録と混同しない。
   - **[手動]** GitHub API や既存 script で SSH key 登録を行う場合、実行直前に `gh auth refresh -h github.com -s admin:public_key` を実行し、GitHub 側で SSH public key を管理できる scope を現在の `gh` token に追加する。scope 追加後に script を実行し、SSH keys 画面で登録結果を確認する。token 値そのものは記録せず、発行済み token は安全な保管先から script の環境変数または prompt にだけ渡す。
   - **[手動]** GitHub の Settings / Password and authentication / Two-factor authentication / Security keys 相当の設定カテゴリを開き、Security keys / Passkeys の add / register 操作を選ぶ。primary YubiKey を挿して browser / OS prompt で touch し、登録名に `primary` と分かる名前を付けて保存する。
   - **[手動]** 同じ GitHub 2FA 画面で spare YubiKey を追加登録し、登録名に `spare` と分かる名前を付けて保存する。登録後、Password and authentication 画面で primary / spare が別々の security key として表示されていることを確認する。

6. **[手動]** Google / Apple など YubiKey を使う各サービスで、primary / spare を別々に登録する。
   - 各サービスの Account security / Sign-in and security / Password and security 相当の設定カテゴリを開き、Passkey / Security key / 2-step verification 相当の画面で add / create / register 操作を選ぶ（FIDO2 / passkey / U2F）。UI 名称はサービスごとに異なるため、sign-in method として物理 security key を追加する項目を選ぶ。
   - primary YubiKey を挿し、ブラウザまたは OS の prompt に従って touch / PIN 入力を行い、登録名または device label を設定できる場合は `primary` と分かる名前で保存する。登録完了画面または sign-in methods 一覧で primary が有効になっていることを確認する。
   - spare YubiKey に差し替え、同じ画面で add / register 操作を繰り返す。登録名または device label を設定できる場合は `spare` と分かる名前で保存し、primary とは別の sign-in method として表示されることを確認する。
   - OTP 要求サービスでは Yubico OTP を primary / spare それぞれに登録する。OATH TOTP を使う場合は同一 TOTP secret / QR code を primary と spare の両方へ登録し、既存 secret を取り出せない場合はサービス側で TOTP を再設定する。
   - 登録後、各サービスの security / recovery 画面で recovery options / backup codes / account recovery contact / trusted phone number / trusted email 相当を確認する。backup code を表示または再生成した場合は、値そのものをこの repository に書かず、サービスごとの推奨に従って別経路の安全な保管先へ保存する。
   - 既存の phone / email fallback が失効済み番号・古いメールアドレス・共有アカウントになっていないか確認し、必要なら更新する。更新後、primary / spare の両方と recovery option が同じ security 画面で有効な回復経路として残っていることを確認する。

7. **[CMD][対話]** `bitwarden-client-secret` を primary / spare YubiKey へ保存する。secret は所定の stdin / prompt から、PIV PIN は controlling TTY の hidden prompt から受け取る。PIN を stdin payload、argv、environment、出力、log へ渡さない。
   - primary 登録では `dotfiles secrets yubikey enroll-primary` で復旧用 `bitwarden-client-secret` だけを保存する。個別保存を使う場合は `dotfiles secrets yubikey put bitwarden-client-secret --stdin` を実行する。
   - spare 登録では `dotfiles secrets yubikey enroll-spare` で primary から復旧用 `bitwarden-client-secret` を読み出して spare に保存する。primary / spare の両方にこの 1 secret が揃っていることを確認する。
   - `scripts/provision-secret-recovery-source.sh` が保存するのは復旧用 `bitwarden-client-secret` だけである。
   - Bitwarden Password Manager login は repository の CLI surface 外であり、Secrets Manager は `restore-gpg` / `restore-pass` / `verify-yubikey --check bws` の secret 操作に使う。

8. **[手動]→[CMD][対話]** Bitwarden Secrets Manager に復旧用 secret を登録する。
   - **[手動]** Bitwarden Secrets Manager で、YubiKey storage の `bitwarden-client-secret` から BWS project `dotfiles-secret-recovery` が 1 件だけ見える状態にする。`dotfiles secrets gpg-backup register` / `add-spare` / `pass-remote register` と `scripts/provision-secret-recovery-source.sh` は project を作成しない。project が存在しない、または同名 project が複数見える場合は BWS provisioning 前 gate 不成立として停止し、secret 登録へ進まない。
   - この runbook は organization / machine account / service account の作成を前提にしない。Bitwarden UI の名称は変わりうるため、手動操作では「BWS project `dotfiles-secret-recovery` を作り、project に `gpg-secret-key-backup` と `password-store-remote` を保存し、その 2 secret を読める復旧用 token を YubiKey storage の `bitwarden-client-secret` に保存する」という保存モデルだけを照合する。
   - BWS access token の hidden prompt / stdin secret input は `dotfiles secrets yubikey put bitwarden-client-secret`、`enroll-primary`、`rotate-bws-token` など YubiKey storage へ保存・更新する経路だけで行う。`dotfiles secrets pass-remote register` と `dotfiles secrets gpg-backup register` / `add-spare` は token を prompt / stdin で受け取らず、BWS command 実行前に対象 YubiKey storage へ保存済みの `bitwarden-client-secret` から取得する。
   - `password-store-remote` は既存 password-store の GitHub SSH clone URL を使い、`dotfiles secrets pass-remote register --serial <serial> --url git@github.com:<owner>/<repo>.git` で登録または明示確認後に更新する。serial 未指定時は単一接続だけを自動解決し、複数接続では停止する。
   - `gpg-secret-key-backup` は既存 password-store recipient と一致する primary fingerprint を使い、`dotfiles secrets gpg-backup register --primary-fingerprint <fingerprint> --serial <serial>` で YubiKey recipient 付き encrypted envelope と接続中 YubiKey recipient を登録する。BWS 取得成功だけを plaintext 復旧完了として扱わない。
   - `scripts/provision-secret-recovery-source.sh` を使う場合は、既存 password-store `.gpg-id` recipient の GPG secret key 確認、encryption/authentication/signing subkey の不足追加、authentication subkey 由来 SSH 公開鍵の GitHub 登録、GitHub private repository 作成、既存 store の Git remote 設定 / push、BWS project `dotfiles-secret-recovery` の事前存在確認 gate、指定 YubiKey の `bitwarden-client-secret` 保存確認、BWS への `password-store-remote` / `gpg-secret-key-backup` 登録をこの順序で実行する。状態遷移に使う終了コードは [spec の provisioning script の終了コード契約](secret-recovery-spec.md) に従う。正常な manifest の任意の bootstrap secret subset は正常状態であり、script は `clear` も `setup` も再実行せず保存済み secret を維持する。manifest/object 不整合を `clear` した場合も clear 自身が空の version 2 manifest を確定するので、script は `setup` を追加実行しない。`status` は無PIN列挙であり slot key/metadata を判定しない。script は project を作成せず、BWS 書込み command へ token を pipe しない。環境変数、argv、shell history に token を載せない。script は未初期化 password-store から `.gpg-id` を生成しない。

9. **[CMD]** 無対話復旧前提の検証（primary / spare 双方で実施。BWS credential と recovery object だけを確認し、OTP / session を要求しない）:
   ```sh
   dotfiles secrets verify-yubikey --serial <serial> --all
   ```

---

## Phase B — 新規マシンでの復旧（spec「到達仕様の復旧フロー」L102-112）

1. **[CMD]** `dotfiles switch`（または `scripts/bootstrap.sh`）で環境を適用し gpg-agent SSH support を有効化する。
2. **[CMD]** `dotfiles secrets verify-yubikey --all`（必要に応じ `--check bws` を個別指定）。YubiKey を挿して実行するだけで、OTP / master password / session / PIV PIN の入力を要求しない。
3. **[CMD]** `dotfiles secrets restore-gpg` で BWS の `gpg-secret-key-backup` から GPG secret key を復元する。
4. **[CMD]→[手動]** `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` で SSH 公開鍵を出力し、未登録なら GitHub に登録する。
   - GitHub の Settings / SSH and GPG keys 相当の画面で、既存 SSH keys に同じ authentication subkey 由来の公開鍵が登録済みか確認する。登録済みなら Title と最終利用状況を確認し、重複登録しない。
   - 未登録の場合は New SSH key / Add SSH key 相当を選び、Title には新規マシン復旧用の GPG authentication subkey 由来であることが分かる名前を入力し、Key 欄へ `dotfiles gpg export-ssh-public-key --primary-fingerprint <40-hex-fingerprint>` の出力だけを貼り付ける。private key や GPG secret key は貼り付けない。
   - 保存後、SSH keys 一覧に新しい key が表示されることを確認し、必要なら `ssh -T git@github.com` 相当で account 認証先を確認する。保管するのは登録日、GitHub account、key title、関連 primary fingerprint だけで、秘密鍵素材は記録しない。
5. **[CMD]** `dotfiles secrets restore-pass` で BWS の `password-store-remote` から private `password-store` repository を clone する。
6. Bitwarden Password Manager の login は本復旧手順の対象外であり、repository の CLI surface には含めない。

---

## BWS token rotate 時

Bitwarden client-secret を rotate する場合は、新 token を有効化してから `dotfiles secrets yubikey rotate-bws-token` で primary と spare の全 YubiKey に反映する。serial 未指定の対話実行では、新 token を一度だけ入力し、各更新ステップで対象 YubiKey だけを 1 本接続して進める。同一実行内の継続 prompt で次へ進む場合も、次の更新前に対象 YubiKey だけを接続する。複数本を接続したまま進める場合は同一実行で継続せず、`--serial <serial>` で対象を明示して 1 本ずつ実行する。serial 未指定で複数本接続されている場合は一覧表示や選択へ進まず停止する。反映後に `dotfiles secrets verify-yubikey --check bws` で BWS 読取、`gpg-secret-key-backup` envelope schema、primary_fingerprint 正規化、接続中 YubiKey recipient matching、unwrap-free recoverability を確認し、全本の確認後に旧 token を失効させる。token 値や断片を runbook、shell history、ログ、永続環境変数に残さない。
