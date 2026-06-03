# #16 Bitwarden Password Manager CLI ログイン

- 作業種別: `機能実装`
- 作業目的: `bw-login` 経路を、CLI、application、外部 command 境界の役割分担に従って実装する。
- 構造完了条件:
  - `bw` CLI 呼び出しは adapter / port 境界へ閉じる。
  - YubiKey 由来 secret の取得順序は application が持つ。
  - `domain` は `bw` CLI や process 実行詳細に依存しない。
  - `verify-yubikey --check bw-login` を真の Bitwarden Password Manager サービス到達確認（server URL 設定・ネットワーク疎通の検証）へ拡張し、#17 で記録された「CLI 起動可能性確認に限定」された既知制約を解消する。具体的には、`secret-recovery-spec.md` の `### dotfiles secrets verify-yubikey` 節および `## 停止条件` 節の `--check bw-login` 到達確認項に記された CLI 起動可能性確認への限定記述、port 契約 doc（`ports/bw.rs` の `BwLoginPort`）/ `support/protection/bw_login.rs` の限界記述、および `review-artifacts/integration/confirmation.md` の既知制約記録を、サービス到達確認へ広げた挙動と一致させる。
- 既存実装の流用方針: `現行の構成・アーキテクチャを固定の前提とし、既存フロー・既存コードを優先的に流用する。新規追加経路を現行の層境界へ収める範囲で実装し、現行コード構造の大幅な作り替えは前提にしない。`
- 境界維持の観点（新規実装が持ち込んではならない結合）:
  - process 実行の境界漏れ
  - secret 入出力境界の混在
  - use case 順序と外部 command 詳細の結合
- レビュー合格条件: `外部 command 依存が現行の層境界内に収まり、新規実装がアーキテクチャ規約違反を持ち込まないこと。あわせて、構造完了条件に挙げた verify-yubikey --check bw-login の真のサービス到達確認への拡張が実装され、#17 で記録された CLI 起動可能性確認の既知制約（spec・port 契約 doc・integration confirmation）が解消されていること。`
- 完了条件充足記録（PR #43 / #16）: `verify-yubikey --check bw-login` は `BwLoginPort::login_and_unlock` を経由して実際に `bw login` / `bw unlock` を実行する到達確認として実装済み（`bw --version` 等の CLI 起動可能性確認ではない）。これに伴い (1) `secret-recovery-spec.md` の `verify-yubikey` 節・`停止条件` 節の限定記述を実到達確認へ是正、(2) #43 の port 契約 doc（`ports/bw.rs` の `BwLoginPort`）・`support/protection/bw_login.rs` は元から限定記述を持たない実 login/unlock 設計、(3) `review-artifacts/integration/confirmation.md` の #17 既知制約は本 #16 実装で解消（同記録に後日注記）。よって #17 で deferred とされた既知制約は解消済み。
