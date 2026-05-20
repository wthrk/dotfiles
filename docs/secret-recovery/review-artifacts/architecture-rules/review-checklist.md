# レビュー観点チェックリスト

この文書は、秘密情報復旧基盤のレビューで確認する観点を整理した補助文書である。手順の正本は [implementation-guidelines.md](../../implementation-guidelines.md) と [hexagonal-implementation-rules.md](../../../architecture/hexagonal-implementation-rules.md) とし、この文書で正本の定義や手順本文を再定義しない。

## 計画依頼の観点

1. [implementation-guidelines.md](../../implementation-guidelines.md) に定義された計画依頼向け固定実装単位の実装単位順序とレビュー循環に従っていること。
2. 計画出力に実装結果、検証結果、レビュー結果を混在させていないこと。
3. 役割分離の制約を破っていないこと。

## 構造境界の観点

4. `入口` / `アプリケーション` / `ドメイン` / `ポート` / `アダプター` / `補助` / `テスト` の責務境界が維持されていること。
5. `アダプター` に ユースケース の順序制御が混入していないこと。
6. `アプリケーション` に具体 I/O 実装が混入していないこと。
7. `補助` に業務語彙や機能固有語彙が混入していないこと。
8. `ポート` に要約・報告データや利用者向け文言が混入していないこと。

## 文書整合の観点

9. [implementation-guidelines.md](../../implementation-guidelines.md) の実装単位名と役割名の参照が他文書と一致していること。
10. 機械可読 JSON の wire contract で使う識別子と状態値（例: `ok`、`skipped`）を翻訳していないこと。
11. 削除済み補助文書や旧ファイル名を参照していないこと。

## 進捗最小化の観点

12. [tasks.md](../../tasks.md) が最小進捗のみを示していること。
13. 過去作業への遡及監査要求を含んでいないこと。
