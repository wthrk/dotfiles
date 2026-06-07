#!/usr/bin/env bash
# ci-ref（または任意の darwinConfigurations 参照）の宣言パッケージ name→{version,repo,changelog} マップを
# eval して JSON へ書く。
#
# 目的: nightly の nix 版差分を eval ベースにする。nightly が必要とするのは「どの宣言パッケージが old→new で
# 版変化したか」と「各パッケージの当該版のリリースノート取得元」だけで、いずれも評価時属性
# （`pname`/`version` と `meta`/`src`）を `nix eval --json` で数秒・ビルド/フェッチ不要で取れる。closure を
# `nix store diff-closures` のために実体化（フル closure を 2 回ビルド）する必要はない。
#
# 出力: `$2` に `{ "name": { "version": "...", "repo": "owner/repo", "changelog": "...", "homepage": "..." }, ... }`
# の JSON object を書く。これを `dotfiles update-history record` の `--nix-old` / `--nix-new` が読み、domain の
# 純粋比較（diff_versions）で版差分を求めると同時に、各パッケージの GitHub owner/repo（repo）・changelog URL・
# homepage URL を delta へ運ぶ。record 側の AI エージェント（GitHub Models tool-use ループ）は、これらを
# 「適切なリリースノートのソースを自分で探して fetch する」ヒント兼 fetch 許可ホスト集合の素材に使う
# （許可ホスト集合は eval 由来のこれらの host だけから組み立て、ノート本文では拡張しない＝SSRF 防御）。
#
# 評価対象: home-manager の `home.packages`（利用者宣言パッケージ）と nix-darwin の
# `environment.systemPackages`（GUI アプリ等の system 宣言パッケージ）。両 attrset の
# name→{version,repo,changelog,homepage} を統合し、同名は system 側を優先する（重複は実フリートでは基本
# 起きない）。
# `pname`/`version` が無いパッケージは `parseDrvName` でフォールバックし、版が取れなければ空文字（record 側で
# 版不明 = None として扱う）。
#
# repo（GitHub owner/repo）の導出優先: ①`meta.homepage` が `github.com/{owner}/{repo}` 形ならそこから、
# ②無ければ `src`（fetchFromGitHub の `src.owner`+`src.repo`、または `src.url`/`src.urls` の github URL）から、
# ③無ければ `meta.changelog` の github URL から。github 由来が取れなければ空文字（version-only 行き）。
# これは評価時 meta/src 属性の参照のみで、ビルド/フェッチは走らない。
#
# changelog: 各パッケージの `meta.changelog`（無ければ `meta.homepage`、いずれも無ければ空文字）。Releases API
# が空振りしたときの changelog raw フォールバック取得元になる。record 側はこの URL（信頼境界外）を host
# allowlist で機械検証してから取得・要約へ回す。
#
# 使い方: eval-declared-versions.sh <reference> <out-json>
#   <reference>  例: darwinConfigurations.ci-ref
#   <out-json>   書き出す JSON ファイル path
set -euo pipefail

reference="${1:?usage: eval-declared-versions.sh <reference> <out-json>}"
out="${2:?usage: eval-declared-versions.sh <reference> <out-json>}"

# owner/repo・changelog 導出の純関数群は derive-repo.nix を単一正本にする（checks のテストと同じ実装を共有し
# 規則のドリフトを防ぐ）。flake の pure eval ではフレーク外の絶対パス import が禁止されるため、ファイル内容を
# 読み込んで `--apply` 式へ inline 注入する（path import を避けつつ実装を 1 箇所に保つ）。
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
derive_repo_nix="$(cat "${script_dir}/derive-repo.nix")"

# 参照構成から利用者名を eval で取得する（ci-ref は user=ci 固定だが、参照を引数化したため動的に解決する）。
user="$(nix eval --raw ".#${reference}.config.system.primaryUser")"

# パッケージリスト attrset を name→{version,repo,changelog,homepage} object へ畳む `--apply` 式。
#
# - name:    `pname` 優先、無ければ `parseDrvName (p.name)`。
# - version: `p.version`（無ければ空文字）。
# - changelog: `p.meta.changelog`（無ければ `p.meta.homepage`、いずれも無ければ空文字）。
# - homepage: `p.meta.homepage`（無ければ空文字）。AI エージェントの fetch 許可ホスト集合のヒントになる。
# - repo:    GitHub owner/repo を ①homepage ②src ③changelog の優先で抽出（無ければ空文字）。
#
# 文字列からの owner/repo 抽出は `builtins.match` の正規表現で行い、url や owner/repo フィールドのみを参照する
# （ビルド/フェッチ非実行）。`p.src.owner`/`p.src.repo` 等は存在しないパッケージがあるため `or` で握りつぶす。
apply="
ps:
let
  derive = (
${derive_repo_nix}
  );
in
builtins.listToAttrs (map (p: {
  name = p.pname or (builtins.parseDrvName (p.name or \"\")).name;
  value = {
    version = p.version or \"\";
    repo = derive.repoOf p;
    changelog = derive.changelogOf p;
    homepage = derive.homepageOf p;
  };
}) ps)
"

home_json="$(nix eval --json \
  ".#${reference}.config.home-manager.users.${user}.home.packages" \
  --apply "$apply")"

system_json="$(nix eval --json \
  ".#${reference}.config.environment.systemPackages" \
  --apply "$apply")"

# home と system を 1 マップへ統合する。同名は system 側（後の `*`）を優先する。
jq -n --argjson home "$home_json" --argjson system "$system_json" \
  '$home * $system' > "$out"
