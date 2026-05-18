# 秘密情報復旧基盤の実装方針

この文書は、秘密情報復旧基盤の実装・レビュー・検証で守る設計方針を定義する。`dotfiles secrets` の実装を変更する前に読み、レビュー時はこの文書との差分として指摘を整理する。

対象は `rust/dotfiles-cli/src/secrets/`、関連する CLI 定義、secret recovery docs、secret recovery の integration / runtime tests である。

## アーキテクチャ

`dotfiles secrets` は Hexagonal Architecture（Ports and Adapters）で実装する。内側の domain は保存仕様だけを表し、外側の adapter や support の具体実装へ依存しない。依存方向は常に CLI から application、application から domain port と adapter、adapter から外部 API へ向ける。

| 区分 | 役割 | 置き場所 |
| --- | --- | --- |
| CLI | clap で受けた command を application use case へ渡す。secret lifecycle、YubiKey 操作、wire format は持たない。 | `secrets.rs` |
| Application | use case の順序を所有する。secret を読む前の precondition、PIN/touch、interrupt、summary 出力、複数 device 更新の順序を決める。 | `secrets/application.rs`、`secrets/application/` |
| Domain | `SecretName`、PIV object id、manifest、blob wire format、summary 型、保存規則を表す。terminal、YubiKey crate、memory lock、`ProtectedSecret`、stdin/stdout を知らない。 | `secrets/domain.rs`、`secrets/domain/` |
| Ports | domain/application が必要とする外部操作の最小 contract を定義する。port は raw plaintext を返さず、必要な処理範囲だけ caller 管理の writer / buffer へ書く。 | `secrets/ports.rs` または domain 内の port module |
| Adapters | YubiKey、terminal、stdout、test stub など外部 I/O を port に接続する。外部 crate の都合、mutable copy、raw RSA 結果は adapter 内で閉じる。 | `secrets/adapters.rs`、`secrets/adapters/` |
| Support | memory lock、zeroize、interrupt guard、OAEP/MGF1 など業務語彙を持たない安全部品。domain 名、command 名、secret 名へ依存しない。 | `secrets/support.rs`、`secrets/support/` |
| Tests | domain は保存仕様、application は順序、adapter は外部 I/O 契約、support は保護境界を検証する。 | 対象 module の test、`rust/tests/` |

禁止する依存は具体的に扱う。domain から application / adapter / support への依存、support から domain / application への依存、adapter から application use case への依存、test helper から通常 bytes を secret 所有型へ変換する経路は禁止する。`storage`、`crypto`、`protection` のような機構名をレイヤー名にしない。保存仕様は domain、暗号処理は domain の規則を満たす support 呼び出し、保護メモリは support の責務として分ける。

ファイルが terminal I/O、YubiKey adapter、wire format、暗号処理、use case、test harness を同時に持ち始めたら、機能追加の前に分割する。レビューでは「責務が混在している」とだけ書かず、混在している concern と移動先の層を明記する。

## 型設計

保護が必要な値は、呼び出し側が任意の場所で `lock` を呼ぶ設計にしない。保護済みでなければ業務 flow に渡せない型を用意し、生成時に memory lock と zeroize の順序を型の責務に含める。

入力 buffer、保護済み secret、保存 model はそれぞれ所有者を明確にする。型が自然に持つ操作は free function ではなく method、標準 trait、または `From` / `TryFrom` / `AsRef` / `Deref` / `Write` / `Read` など Rust の一般的な変換・I/O 境界で表す。独自の `from_zeroizing` のような名前は、標準 trait では表せない追加不変条件がある場合だけ使う。

閉じた集合は raw string で扱わない。secret name、check kind、role、mode、state は enum または newtype にし、`Display` / `FromStr` / serde 変換は CLI、JSON、wire format などの I/O 境界に閉じ込める。serde を使う場合も、閉じた enum を `serde_json::Value` 経由で往復させるような迂回を入れない。

`mut` は API が mutable reference を要求する場合、または in-place state が設計上の所有者である場合だけ使う。所有権を消費できる値は消費し、`&mut` で中身を抜く実装にしない。`ManuallyDrop`、`take`、手作業の drop 順序制御は、型設計で保証できない状態を作りやすいため避ける。

repository-authored Rust では `unsafe` を書かない。TTY、file descriptor、signal、memory protection など OS 境界の処理は、安全な標準 API または安全な crate を選ぶ。

## 入力と TTY

入力処理は terminal I/O の adapter と、secret を保持する utility / model を分ける。YubiKey device adapter は利用者への prompt や stdin 読み取りを持たない。device 操作に PIN や secret が必要な場合は、上位層が入力境界で取得した保護済み値を渡す。

stdin から secret を読む経路は、buffer の確保上限、zeroize、memory lock、parse error 時の drop 順序を型で閉じる。`read_to_end` の再確保や lock 外 buffer を避ける必要がある場合は、読み込み可能な保護 buffer 型に `Write` / `Read` 境界または専用 method を持たせ、業務 flow に細かい read loop を露出させない。

TTY prompt、hidden prompt、timeout 付き待機、interrupt 付き待機は、実際の TTY / PTY で検証する。fake boundary の unit test は orchestration の分岐確認には使えるが、terminal mode、raw input、TTY 判定、stdin/stdout/stderr の接続状態は検証できない。TTY 入力を変更した PR では、PTY を使う integration test または手動 TTY 検証結果を残す。

非対話実行は `--serial`、`--stdin`、`--stdin-json` など必要な入力境界を明示させる。TTY で secret を stdout に出す経路は拒否するか、設計文書で定義した明示 option を要求する。

## 既存 crate と API

標準 library、既存 dependency、安全な crate が提供する機能を優先する。特に terminal input、PTY、password prompt、polling、signal handling、memory protection、serde parsing、cryptographic primitive は、手書き実装を追加する前に既存 API を確認する。

既存 crate を使わず自前実装する場合は、理由をコードではなく設計文書または PR 説明に残す。理由は「依存を増やしたくない」だけでは不十分で、必要な安全性、platform behavior、API 制約、検証可能性を具体的に説明する。

serde のカスタム visitor や手書き parser は、wire format の互換性、streaming、zero-copy、secret lifetime など標準 derive で満たせない要件がある場合だけ使う。通常の JSON 入力では derive と明示的な型で受ける。

## コメントとドキュメントコメント

コメントは備忘録や作業履歴にしない。コードの言い換え、関数名の説明、通常の制御フロー、曖昧な安全そうな表現は書かない。

必要なコメントは、永続する不変条件、外部 contract、lifecycle boundary、security property、wire format rule、interaction boundary を説明する。公開 command flow と非自明な private helper は、操作タイミング、必要入力、利用者との境界、失敗時の停止条件をドキュメントコメントで明示する。

既存のドキュメントコメントを削るだけで終わらせない。低価値なコメントを見つけた場合は、削除で足りるか、設計上必要な不変条件を説明するコメントに置き換えるかを判断する。必要な説明を消したままにしない。

コメントの品質確認はフィルタ検索に頼らない。patch に追加・変更されたコメント行を `git diff` で全件読み、各コメントが上記のいずれの不変条件を説明しているか確認する。

## テスト方針

テストは、実装の層と失敗モードに合わせて置く。

| 対象 | 必要な検証 |
| --- | --- |
| Storage model / wire format | roundtrip、未知 version、欠落 field、secret name validation、互換性。 |
| Crypto / protection utility | lock と zeroize の所有境界、drop 順序、エラー時の secret lifetime。 |
| Application flow | fake device / fake input による分岐、停止条件、複数 YubiKey 更新、同一 serial 拒否。 |
| Terminal input | PTY または手動 TTY での prompt、hidden input、timeout、interrupt、TTY 判定。 |
| Device adapter | 実機 read-only 確認、専用領域への限定書き込み、open error の保持。 |
| CLI integration | `dotfiles secrets` 経路が clap から use case へ届くこと、非対話契約が崩れないこと。 |

fake device / fake input の unit test だけで「TTY 入力が動く」と判断しない。TTY 入力を扱う変更では、PTY test を追加するか、実際の terminal での手動検証を PR に記録する。

実機 YubiKey 検証は read-only 確認と専用領域への書き込みに限定し、reset、既存 credential 削除、既存領域上書きを含めない。YubiKey がない環境では実機検証を skipped / blocked として記録し、unit / PTY / fake boundary の結果と混同しない。

検証コマンドは変更内容に対応させる。Markdown だけの変更では `git diff --check`、リンク確認、表示確認を使い、`cargo xtask check` を機械的に再実行しない。code 変更後にすでに同等の検証が成功し、その後 working tree が変わっていない場合は再実行しない。

## レビュー運用

レビュー対応では未解決コメントを範囲でまとめて処理した扱いにしない。各コメントについて、修正したか、設計判断として残したか、後続 issue に分離したかを確認する。

Copilot などの automated review が継続して指摘を出す場合は、個別指摘の修正だけでなく、同じ種類の問題を生む設計を直す。たとえば secret lock 呼び出し漏れの指摘が出た場合は、呼び出し箇所を増やすのではなく、保護済み型でなければ use case に渡せない設計へ寄せる。

PR へ push したあとは automated review が新しい指摘を出す前提で確認し、未解決指摘がなくなるまで確認と対応を繰り返す。コード修正が必要な大きい対応では、作業範囲を分けて sub-agent を使える場合でも、最終的な未解決指摘の確認は親 agent が行う。
