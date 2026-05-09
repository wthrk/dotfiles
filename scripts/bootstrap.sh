#!/usr/bin/env bash
# Nix や `dotfiles` CLI がまだ無いマシンで、最初のローカル flake を作る。
#
# `nix run <source> -- init` を呼べる状態まで必要なら Nix を導入し、生成後の適用は
# `nix run <source> -- switch ...` に委譲する。古い checkout 前提の引数は互換用に受けるだけにする。
set -euo pipefail

dotfiles_source="github:wthrk/dotfiles"
switch_mode="darwin"
run_switch=1
user=""
host=""
system=""
force=0
dry_run=0
self_test=0
deprecated_dir=""
NIX_FLAKE_ARGS=(--no-update-lock-file)

usage() {
  # CLI 自体を取得する前にも使うスクリプトなので、help は clap に依存させない。
  cat <<USAGE
使い方: scripts/bootstrap.sh [options]

Options:
  --source FLAKE             dotfiles flake（既定: github:wthrk/dotfiles）
  --repo URL_OR_PATH         --source の互換 alias
  --user USER                生成する flake に書くユーザー名
  --host HOST                生成する flake に書くホスト名
  --system SYSTEM            生成 flake に書く system（例: aarch64-darwin）
  --mode darwin|home-manager|all
                             適用モード（既定: darwin）
  --force                    既存 ~/.config/dotfiles/flake.nix を上書きする
  --run-switch               init 後に適用する（既定）
  --no-switch                init 後に終了する
  --dry-run                  実行計画だけ表示する
  --self-test                bootstrap 自体の軽い検証だけ実行する
  --dir PATH                 互換用。checkout は作らず無視する
  --flake NAME               互換用。--user 未指定時の USER として扱う
  -h, --help                 このヘルプを表示する
USAGE
}

legacy_repo_to_source() {
  # 旧 bootstrap の `--repo` は clone URL を受けていた。既知の dotfiles URL だけを flake 参照へ直し、
  # それ以外は利用者が渡した参照をそのまま `nix run` に渡す。
  case "$1" in
    https://github.com/wthrk/dotfiles.git|git@github.com:wthrk/dotfiles.git)
      printf '%s\n' "github:wthrk/dotfiles"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

while (($#)); do
  case "$1" in
    --source)
      dotfiles_source="$2"
      shift 2
      ;;
    --repo)
      dotfiles_source="$(legacy_repo_to_source "$2")"
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
    --flake)
      user="${user:-$2}"
      shift 2
      ;;
    --force)
      force=1
      shift
      ;;
    --run-switch)
      run_switch=1
      shift
      ;;
    --no-switch)
      run_switch=0
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --self-test)
      self_test=1
      shift
      ;;
    --dir)
      deprecated_dir="$2"
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

if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
  # shellcheck disable=SC1091
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi

export NIX_CONFIG="${NIX_CONFIG:-experimental-features = nix-command flakes}"

if [[ "$self_test" == "1" ]]; then
  bash -n "$0"
  echo "self-test passed"
  exit 0
fi

print_plan() {
  # 予行実行では、インストール、init、switch の実行予定だけを表示し、状態変更は行わない。
  cat <<PLAN
bootstrap 実行計画:
- macOS であることを確認
- Xcode Command Line Tools の導入状態を確認（未導入なら案内）
- Nix daemon モードの導入確認
- 実行: dotfiles init
- dotfiles source: ${dotfiles_source}
- local config: \$HOME/.config/dotfiles/flake.nix
- 適用モード: ${switch_mode}
- switch 実行: ${run_switch}
PLAN
  if [[ -n "$deprecated_dir" ]]; then
    echo "- --dir は互換用に受け取りますが checkout は作りません: ${deprecated_dir}"
  fi
}

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

if [[ "$dry_run" == "1" ]]; then
  print_plan
  echo "--dry-run のため、変更を加えず終了します。"
  exit 0
fi

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
