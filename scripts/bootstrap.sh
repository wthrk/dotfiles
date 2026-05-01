#!/usr/bin/env bash
set -euo pipefail

dotfiles_dir="$HOME/.dotfiles"
dotfiles_repo="https://github.com/wthrk/dotfiles.git"
switch_mode="darwin"
run_switch=1
flake_name="default"
dry_run=0
self_test=0
NIX_EXTRA_ARGS=(--extra-experimental-features "nix-command flakes")
NIX_FLAKE_ARGS=(--no-update-lock-file)
DARWIN_REBUILD_INSTALLER_FLAKE="github:LnL7/nix-darwin/06648f4902343228ce2de79f291dd5a58ee12146"
HOME_MANAGER_INSTALLER_FLAKE="github:nix-community/home-manager/5b56ad02dc643808b8af6d5f3ff179e2ce9593f4"
HOME_MANAGER_BACKUP_EXTENSION="before-home-manager"
prepared_paths=()
rollback_required=0

usage() {
  cat <<USAGE
使い方: scripts/bootstrap.sh [options]

Options:
  --dir PATH                 checkout 先（既定: \$HOME/.dotfiles）
  --repo URL_OR_PATH         clone 元（既定: https://github.com/wthrk/dotfiles.git）
  --mode darwin|home-manager 適用モード（既定: darwin）
  --flake NAME               flake 出力名（既定: default）
  --run-switch               flake check 後に switch する（既定）
  --no-switch                flake check 後に終了する
  --dry-run                  実行計画だけ表示する
  --self-test                内部の安全性テストだけ実行する
  -h, --help                 このヘルプを表示する
USAGE
}

copy_local_worktree() {
  local source_dir="$1"
  local target_dir="$2"

  source_dir="$(cd "$source_dir" && pwd -P)"

  if [[ -e "$target_dir" ]]; then
    if ! rmdir "$target_dir" 2>/dev/null; then
      echo "checkout 先が空ではありません: $target_dir" >&2
      exit 1
    fi
  fi

  mkdir -p "$(dirname "$target_dir")"
  mkdir -p "$target_dir"
  rsync -a \
    --exclude '/.direnv/' \
    --exclude '/target/' \
    "$source_dir"/ "$target_dir"/
}

while (($#)); do
  case "$1" in
    --dir)
      dotfiles_dir="$2"
      shift 2
      ;;
    --repo)
      dotfiles_repo="$2"
      shift 2
      ;;
    --mode)
      switch_mode="$2"
      shift 2
      ;;
    --flake)
      flake_name="$2"
      shift 2
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

print_plan() {
  cat <<PLAN
bootstrap 実行計画:
- macOS であることを確認
- Xcode Command Line Tools の導入状態を確認（未導入なら案内）
- dotfiles checkout を clone または既存利用: ${dotfiles_dir}
- Nix daemon モードの導入確認
- 実行: nix flake check
- 適用モード: ${switch_mode}
- flake 出力: .#${flake_name}
- switch 実行: ${run_switch}
PLAN
}

prepare_nix_darwin_etc() {
  [[ "$switch_mode" == "darwin" ]] || return 0

  for path in /etc/bashrc /etc/zshrc; do
    backup="${path}.before-nix-darwin"
    move_aside "$path" "$backup" "nix-darwin 管理前に退避します"
  done
}

prepare_nix_homebrew() {
  [[ "$switch_mode" == "darwin" ]] || return 0

  local prefix
  for prefix in /opt/homebrew /usr/local; do
    move_aside "$prefix/Library/Taps" "$prefix/Library/Taps.before-nix-homebrew" "nix-homebrew 管理前に退避します"
  done
}

ensure_root_git_safe_directory() {
  local repo_path="$1"
  local root_git_config="/var/root/.gitconfig"

  require_sudo "root の git safe.directory 設定"

  if sudo git config --file "$root_git_config" --get-all safe.directory 2>/dev/null | grep -Fxq "$repo_path"; then
    return 0
  fi

  sudo git config --file "$root_git_config" --add safe.directory "$repo_path"
}

move_aside() {
  local path="$1"
  local backup="$2"
  local message="$3"
  local existing_backup

  [[ -e "$path" ]] || return 0

  if [[ -e "$backup" ]]; then
    existing_backup="${backup}.previous.$(date +%Y%m%d%H%M%S)"
    echo "既存の退避先を保持します: $backup -> $existing_backup"
    sudo mv "$backup" "$existing_backup"
  fi

  echo "$message: $path -> $backup"
  sudo mv "$path" "$backup"
  prepared_paths+=("$path|$backup")
}

restore_prepared_paths() {
  local item path backup failed_path i
  for ((i = ${#prepared_paths[@]} - 1; i >= 0; i--)); do
    item="${prepared_paths[$i]}"
    path="${item%%|*}"
    backup="${item#*|}"
    [[ -e "$backup" ]] || continue

    if [[ -e "$path" ]]; then
      failed_path="${path}.failed-nix-switch.$(date +%Y%m%d%H%M%S)"
      echo "失敗後に生成されたパスを退避します: $path -> $failed_path" >&2
      sudo mv "$path" "$failed_path" || true
    fi

    echo "失敗したため退避を戻します: $backup -> $path" >&2
    sudo mv "$backup" "$path" || true
  done
}

rollback_on_exit() {
  local status=$?
  if [[ "$rollback_required" == "1" && "$status" -ne 0 ]]; then
    restore_prepared_paths
  fi
}

abort_with_status() {
  local status="$1"
  if [[ "${rollback_required:-0}" == "1" ]]; then
    rollback_required=0
    restore_prepared_paths
  fi
  exit "$status"
}

require_sudo() {
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

run_self_test() {
  local tmp path backup generated signal
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  require_sudo "--self-test"

  path="$tmp/path"
  backup="$tmp/path.before-test"
  generated="$tmp/path.failed-nix-switch"
  (
    set -euo pipefail
    prepared_paths=()
    rollback_required=1
    trap rollback_on_exit EXIT
    printf 'original\n' > "$path"
    move_aside "$path" "$backup" "self-test"
    printf 'generated\n' > "$path"
    exit 42
  ) || true

  if [[ "$(cat "$path")" != "original" ]]; then
    echo "self-test failed: rollback did not restore original path" >&2
    return 1
  fi
  if ! ls "$generated".* >/dev/null 2>&1; then
    echo "self-test failed: rollback did not preserve failed generated path" >&2
    return 1
  fi

  for signal in INT TERM HUP; do
    path="$tmp/path-${signal}"
    backup="$tmp/path-${signal}.before-test"
    generated="$tmp/path-${signal}.failed-nix-switch"
    (
      set -euo pipefail
      prepared_paths=()
      rollback_required=1
      trap rollback_on_exit EXIT
      trap 'abort_with_status 130' INT
      trap 'abort_with_status 143' TERM
      trap 'abort_with_status 129' HUP
      printf 'original\n' > "$path"
      move_aside "$path" "$backup" "self-test ${signal}"
      printf 'generated\n' > "$path"
      if [[ "$signal" == "INT" ]]; then
        abort_with_status 130
      fi
      if [[ "$signal" == "HUP" ]]; then
        abort_with_status 129
      fi
      kill "-${signal}" "$(bash -c 'echo $PPID')"
      sleep 1
    ) || true

    if [[ "$(cat "$path")" != "original" ]]; then
      echo "self-test failed: ${signal} did not restore original path" >&2
      return 1
    fi
    if ! ls "$generated".* >/dev/null 2>&1; then
      echo "self-test failed: ${signal} did not preserve failed generated path" >&2
      return 1
    fi
  done

  echo "self-test passed"
}

if [[ "$self_test" == "1" ]]; then
  run_self_test
  exit 0
fi

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

if [[ -d "$dotfiles_dir/.git" ]]; then
  echo "既存 checkout を使用します: $dotfiles_dir"
elif git -C "$dotfiles_repo" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "ローカル作業ツリーをコピーします: $dotfiles_dir"
  copy_local_worktree "$dotfiles_repo" "$dotfiles_dir"
else
  echo "dotfiles を clone します: $dotfiles_dir"
  git clone "$dotfiles_repo" "$dotfiles_dir"
fi

cd "$dotfiles_dir"
dotfiles_dir="$(pwd -P)"

if [[ ! -f flake.nix ]]; then
  echo "flake.nix が必要です。Nix 移行後のため init フォールバックはありません（init.sh は削除済み）。" >&2
  exit 1
fi

if [[ ! -s flake.lock ]]; then
  echo "flake.lock が必要です。CI と bootstrap では flake input の暗黙更新を許可しません。" >&2
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

echo "nix flake check を実行します"
nix "${NIX_EXTRA_ARGS[@]}" flake check "${NIX_FLAKE_ARGS[@]}"

if [[ "$run_switch" == "0" ]]; then
  echo "--no-switch のため、flake check 後に終了します"
  exit 0
fi

case "$switch_mode" in
  darwin)
    require_sudo "nix-darwin switch のための /etc と Homebrew Taps 退避"
    rollback_required=1
    trap rollback_on_exit EXIT
    trap 'abort_with_status 130' INT
    trap 'abort_with_status 143' TERM
    trap 'abort_with_status 129' HUP
    prepare_nix_darwin_etc
    prepare_nix_homebrew
    ensure_root_git_safe_directory "$dotfiles_dir"
    nix_bin="$(command -v nix)"
    sudo -H --preserve-env=NIX_CONFIG "$nix_bin" "${NIX_EXTRA_ARGS[@]}" run "${NIX_FLAKE_ARGS[@]}" "$DARWIN_REBUILD_INSTALLER_FLAKE" -- switch --flake ".#$flake_name"
    rollback_required=0
    trap - EXIT INT TERM HUP
    ;;
  home-manager)
    nix "${NIX_EXTRA_ARGS[@]}" run "${NIX_FLAKE_ARGS[@]}" "$HOME_MANAGER_INSTALLER_FLAKE" -- switch -b "$HOME_MANAGER_BACKUP_EXTENSION" --flake ".#$flake_name"
    ;;
  *)
    echo "未対応の適用モード: $switch_mode" >&2
    exit 1
    ;;
esac
