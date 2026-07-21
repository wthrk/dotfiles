# Secret handling policy

この文書は、secret-recovery で secret を扱う実装の正本である。個別機能の設計書は、この文書の方針を重複定義せず参照する。

## Secret の判断基準

この repository では、認証、復号、署名、復旧、外部サービスアクセス、またはそれらの再生成に使える値を secret として扱う。平文そのものだけでなく、復号直後の値、外部 API から返った復旧用値、復旧に必要な credential、key material、token、passphrase も secret である。

公開鍵、識別子、slot 番号、object ID、project 名、secret 名、serial、固定のコマンド名は、それ単体で上記の能力を与えない限り secret ではない。ただし、ログや診断で secret と同じ構造体に同居する場合は redaction の対象にする。

## 守る対象

実装は、repository が所有している間の secret を守る。具体的には次を守る。

- 平文 secret を public API として返さない。
- 平文 secret を CLI 引数、環境変数、ログ、エラー文脈、stdout/stderr、診断出力、レビュー証跡、一時ファイルに出さない。
- repository 所有の平文 buffer は `ProtectedSecret` または zeroize 対象 buffer に置く。
- secret を必要とする外部処理は、protection 内操作の借用境界で完了させる。

守らない対象は、実行中 host が侵害された状態での process memory 全体、外部 SDK・外部 command・OS・デバイス vendor 実装に所有権を移した後の内部状態である。これらを repository の防御境界として主張しない。

## Core Dump

core dump 無効化は残す。理由は、process crash 時に repository が所有している secret が dump file として永続化される経路を閉じるためである。

core dump 無効化は、secret を読み始める前に実行する。これは永続化経路の削減であり、実行中 memory compromise への防御ではない。core dump 無効化があることを理由に、平文 secret の public API 化、ログ混入、argv/env 露出、エラー文脈混入を許可しない。

## Paging / Memory Lock / Signal Trap

paging 回避、`mlock`、memory lock は強い必須防御として扱わない。これらは platform、権限、resource limit に依存し、repository の監査可能な安全境界にできない。実装が best-effort の補助として memory lock を使ってもよいが、成功を仕様、完了条件、レビュー合格条件にしない。

signal trap による cleanup も強い必須防御として扱わない。通常の所有権、Drop、zeroize を破棄境界にする。SIGINT/SIGTERM handler による特別 cleanup を仕様、完了条件、レビュー合格条件にしない。

## Protection 型

`ProtectedSecret` は repository が所有する平文 secret の保護型であり、domain object が secret を直接保持する場合の carrier でもある。Drop 時に zeroize される buffer を所有し、domain/application は長さなどの opaque 操作だけを使う。平文 bytes、backend 抽出、downcast、所有 plaintext buffer、汎用出力口を取り出して保持しない。

`with_secret` 系操作は `support/protection` 内の実装詳細である。借用 closure の外へ slice、参照、iterator、`Vec<u8>`、`String`、その他の所有 plaintext buffer を返してはならない。所有 plaintext buffer へ変換する public API を作ってはならない。

`ProtectedSecret` の secret 生値アクセスは production API にしない。ただし `#[cfg(test)]` または `#[test]` に閉じた最小アクセス関数は、unit test / application orchestration test が secret 値を観測するために限って許可する。この許可は secret protection の公開解除ではなく test-only の最小観測口であり、`String` 変換公開、production 経路での取り出し、外部処理ごとの protection 内操作を迂回する汎用 plaintext consumer API として解釈してはならない。

`support/protection` は secret 保護の backend 実装境界でもある。外部 SDK、暗号処理、device API が secret を必要とする場合、その外部処理名に対応する専用操作をここへ置ける。これは support を product-neutral utility だけに限定するものではなく、secret の借用、所有 plaintext buffer の作成、外部処理呼び出し、repository 所有 buffer の zeroize を同じ保護境界内で完了させるための配置である。application/domain/ports へ SDK 型や平文 buffer API を漏らしたり、汎用 plaintext consumer API を作ったりしてはならない。

storage backend が暗号化された永続化を内包する場合、暗号化・復号・sealed blob encode/decode は backend 内部機能として `support/protection` に置ける。port は sealed blob 形式や暗号操作ではなく、secret datastore の保存・取得・状態確認 capability を公開する。application/domain は secret の意味、必須性、順序、検証を扱い、sealed blob の内部形式や復号手順を直接扱わない。

この許可は backend 実装依存の技術補助、SDK 呼び出しの安全な補助、暗号化 / 復号 / sealed blob / protection / zeroize / core dump 保護などの技術境界、業務判断を含まない変換に限る。固定 secret key の意味づけ、setup 済み判定、不足項目の決定、必須 secret の決定、一意解決の業務規則、0件/複数件の domain failure 化、取得対象の過不足判定、BWS check の外部検証 plan などを `support/protection` に移してはならない。これらは処理ごとに既存規定上の責務境界を判定し、規定済みの境界に置く。

## 外部処理境界

外部 SDK、外部 command、暗号処理、デバイス API が secret の借用または所有 plaintext buffer の move を要求する場合、その呼び出しは `support/protection` 内の専用操作に閉じる。

実装手順は次の順にする。

1. caller は外部処理ごとの `support/protection` 専用操作へ `ProtectedSecret` を渡す。
2. 専用操作の内部だけで protected value を借用し、必要な外部処理を選ぶ。
3. 専用操作は `with_secret` 系借用境界を開始する。
4. 外部処理が所有 plaintext buffer の move を要求する場合、借用 closure 内で、呼び出し直前にだけその buffer を作る。
5. 外部処理の呼び出しを同じ借用 closure 内で完了する。
6. repository が所有し続ける一時 buffer は zeroize する。
7. 外部処理から返った secret は直ちに `ProtectedSecret` へ移し、以後も `ProtectedSecret` として保持する。

所有 plaintext buffer を作る汎用 public API は作らない。外部処理ごとに、必要な責務だけを持つ protection 内操作を作る。

外部処理へ所有権を移した後の buffer、外部 SDK 内部の複製、外部 command 内部の保持は、その外部処理側の責任範囲である。repository 側の責任範囲は、移す前の secret を protection 境界に閉じること、repository 所有 buffer を zeroize すること、表示を mask / redaction すること、ログへ出さないことである。

## TTY secret input

TTY で受け取るすべての secret（PIV PIN、BWS token、その他の hidden prompt 値）は、受理した各 byte を `*` だけで表示する。backspace は対応する mask だけを消去し、Enter は改行だけを出す。`*` の個数以外に、secret 本文、byte 値、値を識別できる断片を stdout、stderr、log、argv、environment に出してはならない。入力は CR/LF の行終端だけを除き、trim、文字列化、Unicode/encoding 変換なしに保護 buffer から外部処理境界へ渡す。

stdin / stdin JSON など TTY 以外の secret 入力は mask を表示しないが、値を表示しない原則は同じである。PIV PIN は controlling TTY だけから読み、BWS token など stdin 許可された secret と混在させない。physical device が受け取る PIN VERIFY は 1 入力につき 1 回だけとし、同じ command の inspection/store/finalize/local verification は認証済み PIV handle を再利用する。ykman の PIN-protected flow にある「VERIFY を最後の APDU に戻す」second VERIFY も、この契約では実行しない。retry、fallback、PUK、reset を自動実行しない。

## 実装レビュー観点

レビューでは次を確認する。

- secret が `ProtectedSecret` / protection 内操作の境界から漏れていない。
- 平文 secret や所有 plaintext buffer を返す public API がない。
- 外部処理呼び出しは protection 内の専用操作で完了している。
- storage backend 内部の暗号化・復号・sealed blob 操作が application/domain/ports へ漏れていない。
- `support/protection` 内の storage backend 操作が setup 判定、必須 secret 判定、一意解決、0件/複数件 failure 化、外部検証 plan を決めていない。
- 所有 plaintext buffer が必要な場合、作成は借用 closure 内かつ呼び出し直前に限られている。
- repository 所有の一時 buffer は zeroize される。
- secret が CLI 引数、環境変数、ログ、エラー、stdout/stderr、一時ファイル、診断、レビュー証跡へ出ない。
- core dump 無効化は残っている。
- paging 回避、`mlock`、memory lock、signal trap cleanup を強い必須防御として要求していない。
