#!/usr/bin/env bash
set -euo pipefail

fail=0
warn=0
skip=0
pass=0
verify_migration_phase="post-migration"
verify_flake="ya"
NIX_EXTRA_ARGS=(--extra-experimental-features "nix-command flakes")
NIX_FLAKE_ARGS=(--no-update-lock-file)
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$REPO_DIR"

if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
  # shellcheck disable=SC1091
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi

current_user="$(id -un)"
export PATH="/etc/profiles/per-user/${current_user}/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:$PATH"

usage() {
  cat <<USAGE
使い方: scripts/verify-nix-migration.sh [options]

Options:
  --phase pre-switch|post-migration  検証フェーズ（既定: post-migration）
  --flake NAME                       flake 出力名（既定: ya）
  -h, --help                         このヘルプを表示する
USAGE
}

while (($#)); do
  case "$1" in
    --phase)
      verify_migration_phase="$2"
      shift 2
      ;;
    --flake)
      verify_flake="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "未対応の引数: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

say() { printf '\n## %s\n' "$1"; }
mark_pass() { printf 'PASS %s\n' "$1"; pass=$((pass+1)); }
mark_fail() { printf 'FAIL %s\n' "$1"; fail=$((fail+1)); }
mark_warn() { printf 'WARN %s\n' "$1"; warn=$((warn+1)); }
mark_skip() { printf 'SKIP %s\n' "$1"; skip=$((skip+1)); }

phase_noncompliant() {
  local msg="$1"
  if [[ "$verify_migration_phase" == "pre-switch" ]]; then
    mark_warn "${msg}（pre-switch モード）"
  else
    mark_fail "$msg"
  fi
}

run_or_warn() {
  local label="$1"
  shift
  printf '$ %s\n' "$*"
  if "$@"; then
    mark_pass "$label"
  else
    mark_warn "$label"
  fi
}

run_or_fail() {
  local label="$1"
  shift
  printf '$ %s\n' "$*"
  if "$@"; then
    mark_pass "$label"
  else
    mark_fail "$label"
  fi
}

check_runtime_auth_not_symlink_if_present() {
  local label="$1"
  local path="$2"
  if [[ -e "$path" || -L "$path" ]]; then
    if [[ -L "$path" ]]; then
      mark_fail "${label}（${path} が symlink）"
    else
      mark_pass "$label"
    fi
  else
    mark_skip "${label}（${path} が存在しないため SKIP）"
  fi
}

is_nix_owned_path() {
  local p="$1"
  [[ "$p" == /nix/store/* || "$p" == /nix/var/nix/profiles/* || "$p" == /etc/profiles/per-user/* ]]
}

say "検証モード"
if [[ "$verify_migration_phase" == "pre-switch" || "$verify_migration_phase" == "post-migration" ]]; then
  mark_pass "phase=$verify_migration_phase"
else
  phase_noncompliant "phase の値が不正: $verify_migration_phase"
fi

say "Flake lock"
if [[ -s flake.lock ]]; then
  mark_pass "flake.lock が存在する"
else
  mark_fail "flake.lock が存在しないか空です"
fi

say "Nix 利用可否"
if command -v nix >/dev/null 2>&1; then
  printf '$ nix --version\n'
  nix --version && mark_pass "nix が利用可能" || mark_warn "nix のバージョン取得に失敗"
else
  mark_warn "nix コマンドが見つかりません"
fi

say "Flake 評価"
if command -v nix >/dev/null 2>&1; then
  run_or_fail "nix flake check" nix "${NIX_EXTRA_ARGS[@]}" flake check "${NIX_FLAKE_ARGS[@]}"
  run_or_fail "home activation drvPath 評価（${verify_flake}）" nix "${NIX_EXTRA_ARGS[@]}" eval "${NIX_FLAKE_ARGS[@]}" ".#homeConfigurations.${verify_flake}.activationPackage.drvPath"
  run_or_fail "darwin config 評価（${verify_flake}）" nix "${NIX_EXTRA_ARGS[@]}" eval "${NIX_FLAKE_ARGS[@]}" ".#darwinConfigurations.${verify_flake}.system"
else
  phase_noncompliant "nix 未導入のため flake 検証不可"
fi

say "zsh 検証"
run_or_warn "zsh 起動" zsh -i -c 'echo ok'
run_or_warn "zsh path 取得" zsh -i -c 'print -l $path >/dev/null'
run_or_warn "zsh 主要コマンド解決" zsh -i -c 'command -v git gh jq rg fzf atuin zoxide nvim >/dev/null'

if zsh -i -c '(( $+widgets[fzf-tab-complete] ))' >/dev/null 2>&1; then
  mark_pass "widget fzf-tab-complete"
else
  mark_skip "widget fzf-tab-complete（HM switch 前は未読込のため SKIP 可）"
fi

if zsh -i -c '(( $+widgets[autosuggest-accept] ))' >/dev/null 2>&1; then
  mark_pass "widget autosuggest-accept"
else
  mark_skip "widget autosuggest-accept（HM switch 前は未読込のため SKIP 可）"
fi

if zsh -i -c 'typeset -f _zsh_highlight >/dev/null || typeset -f _fast_highlight >/dev/null' >/dev/null 2>&1; then
  mark_pass "syntax highlighting 関数"
else
  mark_skip "syntax highlighting 関数（HM switch 前は未読込のため SKIP 可）"
fi

say "Neovim/Mason 検証"
if command -v nvim >/dev/null 2>&1; then
  run_or_warn "Mason コマンド存在" nvim --headless '+lua print(vim.fn.exists(":Mason"))' '+qa'
  run_or_warn "stylua exepath 確認" nvim --headless '+lua print(vim.fn.exepath("stylua"))' '+qa'
  run_or_warn "markdownlint exepath 確認" nvim --headless '+lua print(vim.fn.exepath("markdownlint"))' '+qa'
  run_or_warn "mason PATH skip 設定" rg -n 'PATH\s*=\s*"skip"' config/nvim/lua/omy/configs
  run_or_warn "ts_ls 設定" rg -n 'ts_ls' config/nvim/lua
  mark_skip "checkhealth は既定で実行しません（local parser/build cache の影響を分離）"
else
  mark_skip "nvim 未導入のため Neovim 検証を SKIP"
fi

say "ランタイム認証/状態ファイルの symlink 境界"
check_runtime_auth_not_symlink_if_present "docker config が symlink ではない" "$HOME/.docker/config.json"
check_runtime_auth_not_symlink_if_present "kube config が symlink ではない" "$HOME/.kube/config"
check_runtime_auth_not_symlink_if_present "gh hosts が symlink ではない" "$HOME/.config/gh/hosts.yml"
check_runtime_auth_not_symlink_if_present "gcloud credentials.db が symlink ではない" "$HOME/.config/gcloud/credentials.db"
check_runtime_auth_not_symlink_if_present "gcloud access_tokens.db が symlink ではない" "$HOME/.config/gcloud/access_tokens.db"
check_runtime_auth_not_symlink_if_present "gcloud default_configs.db が symlink ではない" "$HOME/.config/gcloud/default_configs.db"
check_runtime_auth_not_symlink_if_present "gcloud active_config が symlink ではない" "$HOME/.config/gcloud/active_config"

say "Docker/Compose 検証"
run_or_warn "docker binary 確認" command -v docker
run_or_warn "docker version" docker --version
run_or_warn "docker compose version" docker compose version

compose_link="$HOME/.docker/cli-plugins/docker-compose"
if [[ -L "$compose_link" ]]; then
  compose_real="$(realpath "$compose_link" 2>/dev/null || true)"
  if [[ -z "$compose_real" || ! -x "$compose_real" ]]; then
    phase_noncompliant "compose plugin symlink が dangling または非実行可能（${compose_link} -> $(readlink "$compose_link" 2>/dev/null || true)）"
  elif is_nix_owned_path "$compose_real"; then
    mark_pass "compose plugin 実体は Nix 所有（${compose_real}）"
  elif [[ "$compose_real" == "$HOME/.rd/bin/"* || "$compose_real" == /opt/homebrew/* || "$compose_real" == /usr/local/* ]]; then
    phase_noncompliant "compose plugin 実体が非 Nix provider（${compose_real}）"
  else
    phase_noncompliant "compose plugin 実体が Nix 所有として判定できない（${compose_real}）"
  fi
elif [[ -e "$compose_link" ]]; then
  if [[ -x "$compose_link" ]]; then
    phase_noncompliant "compose plugin が symlink ではない（${compose_link}）"
  else
    phase_noncompliant "compose plugin が存在するが実行不可（${compose_link}）"
  fi
else
  phase_noncompliant "compose plugin link が存在しない（${compose_link}）"
fi

# Validate credsStore/credHelpers policy when docker config exists.
docker_cfg="$HOME/.docker/config.json"
if [[ -e "$docker_cfg" ]]; then
  if command -v jq >/dev/null 2>&1; then
    helpers="$(jq -r '.credsStore? // empty, (.credHelpers? // {} | to_entries[] | .value)' "$docker_cfg" 2>/dev/null | awk 'NF' | sort -u || true)"
    if [[ -z "$helpers" ]]; then
      mark_pass "docker credsStore/credHelpers は未設定"
    else
      mark_pass "docker credsStore/credHelpers を解析"
      if printf '%s\n' "$helpers" | rg -qx 'desktop'; then
        phase_noncompliant "docker creds helper 'desktop' は許可されない"
      fi
      while IFS= read -r helper; do
        [[ -z "$helper" ]] && continue
        if command -v "docker-credential-${helper}" >/dev/null 2>&1; then
          mark_pass "docker credential helper 解決: docker-credential-${helper}"
        else
          phase_noncompliant "docker credential helper が見つからない: docker-credential-${helper}"
        fi
      done <<< "$helpers"
    fi
  else
    phase_noncompliant "docker credsStore/credHelpers 検証には jq が必要"
  fi
else
  mark_skip "docker config 不在のため credsStore/credHelpers 検証を SKIP"
fi

say "言語ツールチェーン"
run_or_warn "node/npm/corepack/bun 確認" zsh -i -c 'command -v node npm corepack bun >/dev/null'
run_or_warn "python ツールチェーン確認" zsh -i -c 'command -v python pip uv pyright ruff black >/dev/null'
run_or_warn "rust ツールチェーン確認" zsh -i -c 'command -v rustc cargo cargo-audit cargo-deny cargo-make >/dev/null'
run_or_warn "ruby ツールチェーン確認" zsh -i -c 'command -v ruby bundle >/dev/null'
run_or_warn "go ツールチェーン確認" zsh -i -c 'command -v go golangci-lint dlv >/dev/null'
run_or_warn "php ツールチェーン確認" zsh -i -c 'command -v php composer >/dev/null'
run_or_warn "ocaml ツールチェーン確認" zsh -i -c 'command -v ocaml dune opam utop >/dev/null'

say "ポリシースクリプト"
run_or_warn "zsh shortcuts policy 検証" bash scripts/test-zsh-shortcuts.sh
run_or_warn "zsh key ops policy 検証" bash scripts/test-zsh-key-operations-full.sh

printf '\n集計: PASS=%d WARN=%d SKIP=%d FAIL=%d\n' "$pass" "$warn" "$skip" "$fail"
if (( fail > 0 )); then
  exit 1
fi
