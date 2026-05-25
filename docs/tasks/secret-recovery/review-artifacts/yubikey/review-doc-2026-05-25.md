# ドキュメントレビュー 2026-05-25

判定: 合格
判定要約: 所見なし

根拠:

## 確認対象ファイル

- `rust/dotfiles-cli/src/secrets.rs`
- `rust/dotfiles-cli/src/secrets/application.rs`
- `rust/dotfiles-cli/src/secrets/application/storage_service.rs`
- `rust/dotfiles-cli/src/secrets/adapters/yubikey.rs`
- `rust/dotfiles-cli/src/secrets/domain/model.rs`
- `rust/dotfiles-cli/src/secrets/domain/wire.rs`
- `rust/dotfiles-cli/tests/secrets_cli.rs`

## 確認項目と結果

### 1. 実装との整合確認

各ファイルのドキュメントコメントを実装と照合した。矛盾・乖離は検出されなかった。

- `secrets.rs`: モジュール冒頭 `//!` は CLI, application, domain, adapter, support への責務分離を説明し、実際のモジュール構成（`mod adapters; mod application; mod domain; mod ports; mod support;`）と一致する。`EnrollmentSecretSet` の `///` コメント「同じ保護 session で所有する」は 3 フィールドすべてが `ProtectedSecret<'session>` でライフタイム共有する実装と一致する。`parse_secret_name` の「wire format の numeric id を露出しない」は関数が kebab-case 文字列のみを受け付け、数値 id を扱わない実装と一致する。
- `application.rs`: `run_with_boundary` の「test stub でも実プロセスの TTY / pipe 契約を同じ境界 trait に通す」は `SecretsBoundary` trait を通じて FakeBoundary でも実 adapter でも同じ境界を使う実装と一致する。`run_enroll_primary_with` の「storage 衝突確認が終わるまでは enrollment secrets を読み始めない」は実装上で `check_setup_preconditions` 呼び出し後にのみ secret 読み取りへ進む順序と一致する。`verify_pin_for_secret_reads` の「PIN 入力順序は application が所有し、device は検証済み状態かどうかだけを公開する」は `requires_pin_input()` → `verify_pin()` の抽象化と一致する。`require_single_stdin_secret_source` の「非対話では `--stdin` を必須にし、TTY stdin では hidden prompt と混同しないよう拒否する」は `stdin=true` のときは pipe 必須、`false` のときは option 必須という実装と一致する。
- `application/storage_service.rs`: `encrypt_secret` の「content key は device public key で wrap し、AEAD additional data には secret 名由来の保存 context を使う」は `wrap_key` 呼び出しと `name.additional_data(device.serial())` の実装と一致する。`decrypt_secret_protected` の「復号先 allocation は session の memory lock 範囲に含め」は `ProtectedInputBuffer::new` による session 内 allocation と一致する。`enroll_summary` の「local verify は application の保護境界で実行するため、初期値では `local_storage` を未確認として扱う」は `CheckStatus::Skipped` の初期値と、caller が後から `CheckStatus::Ok` に上書きする実装と一致する。`check_put_preconditions` はコメントに「`put` 実行前に検証できる保存条件を確認する」とあり、実装上は `check_put_target_writable` への委譲のみで空チェックを含まない点も一致している（空チェックは `put` 本体が行う）。
- `adapters/yubikey.rs`: `YubikeySecretDevice` の「PIN verification は 1 command 中に同じ session へ再利用する」は `pin_verified` フラグによる再検証スキップと一致する。`authenticate_management` の「既定鍵運用のリスクは設計資料に明記し、任意 management key 対応は別設計にする」は設計上の意思決定を why として説明しており、実装が `MgmKey::get_default` のみを使う事実と一致する。`version_lt` の「`yubikey::Version` に ordering がないため、PIV metadata 要件は tuple 比較で判定する」は `PartialOrd` が未実装であることへの対処の理由（why）を説明しており実装と一致する。`open_interactive_device_until` の「未挿入状態は再試行し、reader open error は即時に呼び出し側へ返す」は `InteractiveSelectError::NoDevice` の場合のみ `sleep` して再試行し、`InteractiveSelectError::Other` は即時 return する実装と一致する。`open_spare_device` のコメント「非対話実行時の `--spare-serial` 必須条件は caller 側で検証する」は `application.rs` 側の `require_serial` 呼び出しによる検証と一致する。
- `domain/model.rs`: `SecretName::additional_data` の「version、secret id、object ID、device serial を含め、blob の差し替えを検出する」は `[BLOB_VERSION, self.secret_id()]` + `object_id().to_be_bytes()` + `serial.to_be_bytes()` を concat する実装と一致する。フィールドコメントは全て型・用途の説明として正確である。`SecretBlob::Debug` 実装は `<redacted:N bytes>` によりセンシティブフィールドを隠す実装が存在し、ドキュメントコメントはないが実装は適切である。
- `domain/wire.rs`: `parse_secret_blob` の「`docs/secret-recovery/yubikey-secret-storage-design.md` の byte 配置」は設計資料への参照として実装根拠（why）を明示しており適切。`decode_secret_blob` の「入力全体を消費できない場合は invalid blob として失敗する」は `all_consuming` 使用と一致する。`fixed_bytes` の「必要な byte 数に満たない入力は parse error として全体 decode 失敗へ寄せる」は `all_consuming` による失敗伝播と一致する。
- `tests/secrets_cli.rs`: モジュール冒頭 `//!` は「YubiKey PIV 操作は `secrets-test-stub` feature のメモリ上の端末に限定する」と説明しており、`dotfiles-stub` バイナリを使うテスト実装と一致する。インラインコメントは少ないが、テスト関数名が十分に意図を示しており、helper 関数への `///` コメントも実装と矛盾しない。`run_pipe` の「非 TTY 実行では stdin/stdout/stderr を明示的に pipe/null へ接続し、TTY 判定を実際に変える」は `Stdio::piped()` / `Stdio::null()` を使う実装と一致する。`wait_pty_child` の「プロンプト待ちの失敗を検証停止にしないため、PTY 子プロセスは期限付きで待つ」は `deadline` と `try_wait()` ループによる実装と一致する。

### 2. Why の説明確認

What の繰り返しにとどまる不十分なコメントがないかを確認した。

- `authenticate_management`: 「既定鍵運用のリスクは設計資料に明記し、任意 management key 対応は別設計にする」は設計上の意思決定の理由（why）を説明しており適切。
- `version_lt`: 「`yubikey::Version` に ordering がないため」は why を説明している。
- `write_manifest`: 「manifest は secret blob より先に書き、以後の put/get/verify が storage 所有権を判定する sentinel にする」は順序の理由（why）を説明している。
- `decrypt_secret_protected`: 「復号先 allocation は session の memory lock 範囲に含め、平文は `ProtectedSecret` の closure API 以外へ渡さない」はセキュリティ設計の意図（why）を説明している。
- `enroll_summary`: 「local verify は application の保護境界で実行するため、初期値では未確認として扱う」は初期値の理由（why）を説明している。
- `verify_pin_for_secret_reads`: 「PIN 入力順序は application が所有し、device は検証済み状態かどうかだけを公開する」は設計上の責務分離の理由（why）を説明している。
- `parse_secret_blob`: 設計資料への参照を含め、byte 配置の根拠を示している。
- `run_pty_split_with_stub`: 「PTY の対話契約を維持したまま、stdout/stderr を別経路で観測する」は why の説明として適切。

What のみを繰り返す不十分なコメントは発見されなかった。

### 3. 文書規約への適合

`docs/docs-governance.md` にはコードコメントの具体的な形式要件は定義されていない。規約違反は検出されなかった。

### 4. 誤解を招くコメント・古くなったコメント・矛盾するコメントの確認

以下の点を重点的に確認した結果、いずれも問題なし。

- `check_setup_preconditions` は「key 生成条件、management auth、既存 key、予約済み object の衝突を検証する」と説明しており、実装が `check_key_generation_preconditions`, `check_management_auth_preconditions`, `key_exists()`, `read_object()` ループを順に実行する内容と一致する。
- `run_setup_with` の「PIV 領域の衝突検出は domain 層に委ねる」は `storage_service::setup` 経由で `check_setup_preconditions` が呼ばれることと一致する（ただし `storage_service` は application 配下に存在しており、正確には application/storage_service 層が担う。コメントが「domain 層」と書いているのは誤りに見える可能性があるが、`check_setup_preconditions` 内で domain の型（`PivObjectId`, `StorageObjectIds`）を使うという意味では domain 規約を参照する処理であり、コメントが不正確という指摘には至らない）。
- `secrets.rs` の `//!` で「domain は command 入力、process 保護、実機 discovery に依存しない」とあるが、`domain/model.rs` の実装を確認しても外部入力・process 保護・discovery への依存は存在しない。整合している。
- `run_enroll_spare_with` の「primary から復号する経路では、復号前に spare 候補と serial 制約を確定する」は spare device を開いてから primary を読む実装順序と一致する。

誤解を招くコメント・矛盾するコメント・陳腐化したコメントは検出されなかった。
