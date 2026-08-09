# 無人更新の自動化

このディレクトリは、利用者の操作を介さずに依存 pin と installed パッケージを前進させる経路（nightly の
`flake.lock` bump と、switch 時の Homebrew 無人 upgrade）の運用規約・安全ゲート・明示受容をまとめる入口である。

利用者向けの `dotfiles update` / `dotfiles switch` の使い方は repository root の
[`README.md`](../../README.md) を参照する。

## 配下の項目

- [nightly-lock-bump.md](nightly-lock-bump.md): nightly の `flake.lock` 全 input bump、auto-merge を
  fail-closed に保つゲート。
- [homebrew-cask-pinning.md](homebrew-cask-pinning.md): `greedyCasks` による無人 cask upgrade の明示受容と、
  前提（全 cask が sha256 固定）を守る強制機構。
