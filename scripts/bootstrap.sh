#!/usr/bin/env bash
# Nix や `dotfiles` CLI がまだ無いマシンで、最初のローカル flake を作る。
#
# `nix run <source> -- init` を呼べる状態まで必要なら Nix を導入し、生成後の適用は
# `nix run <source> -- switch ...` に委譲する。
set -euo pipefail

default_dotfiles_source="github:wthrk/dotfiles"
if [[ -n "${DOTFILES_BOOTSTRAP_SOURCE_REF:-}" ]]; then
  default_dotfiles_source="github:wthrk/dotfiles/${DOTFILES_BOOTSTRAP_SOURCE_REF}"
fi
dotfiles_source="${DOTFILES_BOOTSTRAP_SOURCE:-$default_dotfiles_source}"
switch_mode="darwin"
run_switch=1
user=""
host=""
system=""
force=0
NIX_FLAKE_ARGS=(--no-update-lock-file)

usage() {
  # CLI 自体を取得する前にも使うスクリプトなので、help は clap に依存させない。
  cat <<USAGE
使い方: scripts/bootstrap.sh [options]

Options:
  --source FLAKE             dotfiles flake（既定: github:wthrk/dotfiles）
  --user USER                生成する flake に書くユーザー名
  --host HOST                生成する flake に書くホスト名
  --system SYSTEM            生成 flake に書く system（例: aarch64-darwin）
  --mode darwin|home-manager|all
                             適用モード（既定: darwin）
  --force                    既存 ~/.config/dotfiles/flake.nix を上書きする
  --no-switch                init 後に終了する
  -h, --help                 このヘルプを表示する
USAGE
}

while (($#)); do
  case "$1" in
    --source)
      dotfiles_source="$2"
      shift 2
      ;;
    --user)
      user="$2"
      shift 2
      ;;
    --host)
      host="$2"
      shift 2
      ;;
    --system)
      system="$2"
      shift 2
      ;;
    --mode)
      switch_mode="$2"
      shift 2
      ;;
    --force)
      force=1
      shift
      ;;
    --no-switch)
      run_switch=0
      shift
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

if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
  # shellcheck disable=SC1091
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi

export NIX_CONFIG="${NIX_CONFIG:-experimental-features = nix-command flakes}"

require_sudo() {
  # Nix インストールや Darwin switch の前に sudo 権限を確認する。失敗時の再試行やパスワード制御は
  # sudo 側の責務にし、このスクリプトでは隠れたリトライを入れない。
  local purpose="$1"

  if sudo -n true 2>/dev/null; then
    return 0
  fi

  if [[ -t 0 ]]; then
    echo "sudo 認証が必要です: $purpose" >&2
    sudo -v
    return
  fi

  if { : </dev/tty; } 2>/dev/null; then
    echo "sudo 認証が必要です: $purpose" >/dev/tty
    sudo -v
    return
  fi

  echo "sudo 認証が必要ですが、この実行環境ではパスワード入力できません: $purpose" >&2
  echo "対話端末で sudo -v を実行してから再実行してください。" >&2
  return 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "この bootstrap は macOS 専用です。" >&2
  exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools が必要です。インストーラを起動します..."
  xcode-select --install || true
  echo "Command Line Tools の導入完了後に再実行してください。"
  exit 1
fi

if ! command -v nix >/dev/null 2>&1; then
  require_sudo "Nix daemon mode インストール"
  echo "Nix をインストールします（daemon mode）..."
  NIX_INSTALLER_NO_CHANNEL_ADD=1 sh <(curl -fsSL --retry 5 --retry-delay 2 https://nixos.org/nix/install) --daemon
  if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
    # shellcheck disable=SC1091
    . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
  fi
  for _ in $(seq 1 30); do
    if [[ -S /nix/var/nix/daemon-socket/socket ]]; then
      break
    fi
    sleep 1
  done
  export NIX_REMOTE=daemon
  hash -r
fi

init_args=(init --source "$dotfiles_source")
if [[ -n "$user" ]]; then
  init_args+=(--user "$user")
fi
if [[ -n "$host" ]]; then
  init_args+=(--host "$host")
fi
if [[ -n "$system" ]]; then
  init_args+=(--system "$system")
fi
if [[ "$force" == "1" ]]; then
  init_args+=(--force)
fi

echo "dotfiles init を実行します"
nix run "${NIX_FLAKE_ARGS[@]}" "$dotfiles_source" -- "${init_args[@]}"

if [[ "$run_switch" == "0" ]]; then
  echo "--no-switch のため、init 後に終了します"
  exit 0
fi

case "$switch_mode" in
  darwin)
    switch_target="darwin"
    ;;
  home-manager)
    switch_target="home"
    ;;
  all)
    switch_target="all"
    ;;
  *)
    echo "未対応の適用モード: $switch_mode" >&2
    exit 1
    ;;
esac

if [[ "$switch_target" == "darwin" || "$switch_target" == "all" ]]; then
  require_sudo "nix-darwin switch"
fi

echo "dotfiles switch ${switch_target} を実行します"
switch_env=()
if [[ -n "$user" ]]; then
  switch_env+=(DOTFILES_USER="$user")
fi
if [[ -n "$host" ]]; then
  switch_env+=(DOTFILES_HOST="$host")
fi
switch_env+=(DOTFILES_SOURCE="$dotfiles_source")

env "${switch_env[@]}" nix run "${NIX_FLAKE_ARGS[@]}" "$dotfiles_source" -- switch "$switch_target"
