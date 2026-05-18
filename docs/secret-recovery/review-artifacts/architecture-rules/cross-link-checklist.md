# Cross Link Checklist

この文書は、対象 6 文書と review artifact 間の参照整合を確認するための雛形である。

| 参照元 | 参照先 | 要求 | 状態 | 証跡 |
| --- | --- | --- | --- | --- |
| `implementation-guidelines.md` | `hexagonal-implementation-rules.md` | 一般構造の正本参照 | completed | [implementation-guidelines.md](/Users/ya/works/dotfiles/docs/secret-recovery/implementation-guidelines.md:11) |
| `implementation-guidelines.md` | `tasks.md` | 進捗正本参照 | completed | [implementation-guidelines.md](/Users/ya/works/dotfiles/docs/secret-recovery/implementation-guidelines.md:11) |
| `implementation-guidelines.md` | `yubikey-secret-storage-design.md` | 機能仕様参照 | completed | [implementation-guidelines.md](/Users/ya/works/dotfiles/docs/secret-recovery/implementation-guidelines.md:11) |
| `implementation-guidelines.md` | `AGENTS.md` / `AGENTS_ja.md` | planning procedure の入口参照が一致すること | completed | [AGENTS.md](/Users/ya/works/dotfiles/AGENTS.md:5), [AGENTS_ja.md](/Users/ya/works/dotfiles/AGENTS_ja.md:5) |
| `README.md` / `tasks.md` / `yubikey-secret-storage-design.md` | `implementation-guidelines.md` | planning request の入口が `## 3. planning request 実行手順` で一致すること | completed | [README.md](/Users/ya/works/dotfiles/docs/secret-recovery/README.md:18), [tasks.md](/Users/ya/works/dotfiles/docs/secret-recovery/tasks.md:52), [yubikey-secret-storage-design.md](/Users/ya/works/dotfiles/docs/secret-recovery/yubikey-secret-storage-design.md:360) |
| `yubikey-secret-storage-design.md` | `hexagonal-implementation-rules.md` | Architecture Governance 節 | completed | [yubikey-secret-storage-design.md](/Users/ya/works/dotfiles/docs/secret-recovery/yubikey-secret-storage-design.md:358) |
| `yubikey-secret-storage-design.md` | `implementation-guidelines.md` | Architecture Governance 節 | completed | [yubikey-secret-storage-design.md](/Users/ya/works/dotfiles/docs/secret-recovery/yubikey-secret-storage-design.md:358) |
| `AGENTS.md` / `AGENTS_ja.md` | `implementation-guidelines.md` | secret-recovery 例外規則 | completed | [AGENTS.md](/Users/ya/works/dotfiles/AGENTS.md:14), [AGENTS_ja.md](/Users/ya/works/dotfiles/AGENTS_ja.md:14) |
| 対象 6 文書と artifact 6 件 | すべてのローカル Markdown file link | 参照先ファイルが存在すること | completed | 32 件のローカル link を確認し broken 0 |
