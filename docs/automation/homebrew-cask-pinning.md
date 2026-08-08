# Homebrew cask の固定状況（無人 upgrade の明示受容）

この文書は、`greedyCasks` 有効下で無人 upgrade される宣言 cask の成果物固定状況を明示受容する正本である。
宣言そのものは [`nix/modules/homebrew.nix`](../../nix/modules/homebrew.nix) が持つ。

auto-update 経路は switch 時に `brew upgrade` を実行して installed cask/formula を tap rev の pin へ追従させる。
`homebrew.nix` で `greedyCasks = true` を有効化しているため、`auto_updates true` / `version :latest` の cask も
upgrade 対象になり、全 cask が tap pin へ決定論的に収束する（`dotfiles update-history` の差分にも現れる）。
tap rev は cask の「定義」を固定し、ダウンロード成果物の固定性は cask 側の `sha256` 指定に依存する。現在の
宣言 cask は `auto_updates true` のものも含め全て `sha256` で成果物を明示固定しているため、greedy 有効下でも無人
upgrade が差し替える成果物は tap rev で再現的に固定される。

現在の宣言 cask の固定状況を明示受容する。

| cask | tap | sha256 固定 | auto_updates | 無人 upgrade 対象 | 成果物の固定 |
|---|---|---|---|---|---|
| `azookey` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `font-cica` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `kicad` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `yubico-authenticator` | homebrew/cask | あり | なし | 対象 | tap rev で固定（再現的） |
| `bitwarden` | homebrew/cask | あり | **true** | 対象（greedy） | tap rev で固定（再現的） |
| `codex-app` | homebrew/cask | あり（arm） | **true** | 対象（greedy） | tap rev で固定（再現的） |
| `ghostty` | homebrew/cask | あり | **true** | 対象（greedy） | tap rev で固定（再現的） |

## cask を追加するときの確認手順

greedy 有効化の前提は「全 cask が sha256 固定」である。`sha256 :no_check`（未固定成果物）の cask を足すと、greedy
有効下では未固定成果物が無人差し替えされうるため、`dotfiles update-history record` 経路の brew モジュール
（[`rust/dotfiles-cli/src/update_history/brew.rs`](../../rust/dotfiles-cli/src/update_history/brew.rs)）が tap
rev の cask `.rb` を検査し、`sha256 :no_check` があれば fail-closed で停止する（cask 名を添えて中断）。cask を
追加する際は、対象 cask の `.rb` が `sha256 "<hash>"` で固定されている（`sha256 :no_check` でない）ことを確認し、
上の表へ 1 行追加する。固定できない cask は `homebrew.nix` の `casks` から外し、必要なら手動更新へ寄せる。

そもそも cask を選ぶのは、対象が nixpkgs に無いか、nixpkgs にあっても darwin 評価が通らない場合に限る。
現在の宣言では `font-cica` が前者、`kicad` が後者（nixpkgs の `pkgs/by-name/ki/kicad` が
`broken = stdenv.hostPlatform.isDarwin`）にあたる。

auto-update が cask を上げた事実は、nightly CI が記録する
[`docs/update-history/*.toml`](../update-history/README.md)（`dotfiles update-history show` で閲覧）に更新
アプリとして現れ、無人差し替えが不可視にならないようにしている。
