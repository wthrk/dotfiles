# Homebrew cask の成果物固定（無人 upgrade の明示受容）

この文書は、`greedyCasks` 有効下で無人 upgrade される宣言 cask の成果物固定を明示受容する正本である。
どの cask を宣言しているかは [`nix/modules/homebrew.nix`](../../nix/modules/homebrew.nix) の `casks` が実体であり、
この文書はそれを複製しない。

auto-update 経路は switch 時に `brew upgrade` を実行して installed cask/formula を tap rev の pin へ追従させる。
`homebrew.nix` で `greedyCasks = true` を有効化しているため、既定では upgrade を素通りする `auto_updates true` の
cask も upgrade 対象になり、全 cask が tap pin へ決定論的に収束する（greedy は `version :latest` も対象にするが、
その cask は後述のとおり宣言 cask として定着しない）。
tap rev は cask の「定義」を固定し、ダウンロード成果物の固定性は cask 側の `sha256` 指定に依存する。宣言 cask が
`auto_updates true` のものも含め全て `sha256` で成果物を明示固定している限り、greedy 有効下でも無人 upgrade が
差し替える成果物は tap rev で再現的に固定される。

受容の対象は宣言 cask の一覧そのものではなく、この「全 cask が sha256 固定である」という前提である。

## 前提を守る強制機構

greedy 有効化の前提「全 cask が sha256 固定」は、`dotfiles update-history record` 経路の brew モジュール
（[`rust/dotfiles-cli/src/update_history/brew.rs`](../../rust/dotfiles-cli/src/update_history/brew.rs)）が強制する。
このモジュールは `homebrew.nix` の `casks` 宣言を唯一の対象源として tap rev の cask `.rb` を検査し、
`sha256 :no_check`（未固定成果物）があれば cask 名を添えて fail-closed で停止する。前提を満たさない cask を
`casks` へ足すと nightly の record が止まるため、固定できない cask は `homebrew.nix` の `casks` から外し、
必要なら手動更新へ寄せる。

## nixpkgs ではなく cask で宣言する条件

そもそも cask を選ぶのは、対象が nixpkgs に無いか、nixpkgs にあっても darwin 評価が通らない（例:
`broken = stdenv.hostPlatform.isDarwin`）場合に限る。どちらにも当たらないものは nixpkgs 側で宣言する。

## 無人差し替えの可視化

auto-update が cask を上げた事実は、nightly CI が記録する
[`docs/update-history/*.toml`](../update-history/README.md)（`dotfiles update-history show` で閲覧）に更新
アプリとして現れ、無人差し替えが不可視にならないようにしている。

差分の単位は tap rev 間の `version "..."` 変化であり、`version :latest` の cask は履歴に現れない。ただしこれは
可視化の穴にはならない。Homebrew の cask audit は `version :latest` に `sha256 :no_check` を要求するため、
`:latest` の cask は上記の前提「全 cask が sha256 固定」を満たせず、`casks` に足した時点で record が
fail-closed で止まる。強制の実体は次回 record 実行時の停止なので追加から検出までには時間差があるが、`:latest` の
cask は宣言 cask として定着せず、可視化の保証は宣言 cask 全体に対して成立する。
