#!/usr/bin/env bash
# ci-ref（または任意の darwinConfigurations 参照）の宣言パッケージ name→version マップを eval して JSON へ書く。
#
# 目的: nightly の nix 版差分を eval ベースにする。nightly が必要とするのは「どの宣言パッケージが old→new で
# 版変化したか」だけで、それは `pname`/`version`（評価時属性）を `nix eval --json` で数秒・ビルド/フェッチ不要で
# 取れる。closure を `nix store diff-closures` のために実体化（フル closure を 2 回ビルド）する必要はない。
#
# 出力: `$2` に `{ "name": "version", ... }` の flat JSON object を書く。これを `dotfiles update-history record`
# の `--nix-old` / `--nix-new` が読み、domain の純粋比較（diff_versions）で版差分を求める。
#
# 評価対象: home-manager の `home.packages`（利用者宣言パッケージ）と nix-darwin の
# `environment.systemPackages`（GUI アプリ等の system 宣言パッケージ）。両 attrset の name→version を統合し、
# 同名は system 側を優先する（重複は実フリートでは基本起きない）。`pname`/`version` が無いパッケージは
# `parseDrvName` でフォールバックし、版が取れなければ空文字（record 側で版不明 = None として扱う）。
#
# 使い方: eval-declared-versions.sh <reference> <out-json>
#   <reference>  例: darwinConfigurations.ci-ref
#   <out-json>   書き出す JSON ファイル path
set -euo pipefail

reference="${1:?usage: eval-declared-versions.sh <reference> <out-json>}"
out="${2:?usage: eval-declared-versions.sh <reference> <out-json>}"

# 参照構成から利用者名を eval で取得する（ci-ref は user=ci 固定だが、参照を引数化したため動的に解決する）。
user="$(nix eval --raw ".#${reference}.config.system.primaryUser")"

# パッケージリスト attrset を name→version object へ畳む `--apply` 式。`pname` 優先、無ければ
# `parseDrvName (p.name)`、version は `p.version`（無ければ空文字）。
apply='ps: builtins.listToAttrs (map (p: { name = p.pname or (builtins.parseDrvName (p.name or "")).name; value = p.version or ""; }) ps)'

home_json="$(nix eval --json \
  ".#${reference}.config.home-manager.users.${user}.home.packages" \
  --apply "$apply")"

system_json="$(nix eval --json \
  ".#${reference}.config.environment.systemPackages" \
  --apply "$apply")"

# home と system を 1 マップへ統合する。同名は system 側（後の `*`）を優先する。
jq -n --argjson home "$home_json" --argjson system "$system_json" \
  '$home * $system' > "$out"
