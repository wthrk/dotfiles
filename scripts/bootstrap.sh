#!/usr/bin/env bash
set -euo pipefail

DOTFILES_DIR="${DOTFILES_DIR:-$HOME/.dotfiles}"
DOTFILES_REPO="${DOTFILES_REPO:-https://github.com/wthrk/dotfiles.git}"
DOTFILES_SWITCH_MODE="${DOTFILES_SWITCH_MODE:-darwin}"
DOTFILES_RUN_SWITCH="${DOTFILES_RUN_SWITCH:-1}"
DOTFILES_FLAKE="${DOTFILES_FLAKE:-ya}"
DOTFILES_SOPS_AGE_KEY_DEST="${DOTFILES_SOPS_AGE_KEY_DEST:-/var/lib/sops-nix/key.txt}"
DOTFILES_DRY_RUN="${DOTFILES_DRY_RUN:-0}"
NIX_EXTRA_ARGS=(--extra-experimental-features "nix-command flakes")
NIX_FLAKE_ARGS=(--no-update-lock-file)
DARWIN_REBUILD_INSTALLER_FLAKE="github:LnL7/nix-darwin/06648f4902343228ce2de79f291dd5a58ee12146"
HOME_MANAGER_INSTALLER_FLAKE="github:nix-community/home-manager/5b56ad02dc643808b8af6d5f3ff179e2ce9593f4"

if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
  # shellcheck disable=SC1091
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi

print_plan() {
  cat <<PLAN
bootstrap 実行計画:
- macOS であることを確認
- Xcode Command Line Tools の導入状態を確認（未導入なら案内）
- dotfiles checkout を clone または既存利用: ${DOTFILES_DIR}
- Nix daemon モードの導入確認
- 必要時のみ sops age key を配置: ${DOTFILES_SOPS_AGE_KEY_DEST}（DOTFILES_SOPS_AGE_KEY_FILE 指定時）
- 実行: nix flake check
- 適用モード: ${DOTFILES_SWITCH_MODE}
- flake 出力: .#${DOTFILES_FLAKE}
- DOTFILES_RUN_SWITCH=${DOTFILES_RUN_SWITCH} を尊重
PLAN
}

if [[ "$DOTFILES_DRY_RUN" == "1" ]]; then
  print_plan
  echo "DOTFILES_DRY_RUN=1 のため、変更を加えず終了します。"
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

if [[ -d "$DOTFILES_DIR/.git" ]]; then
  echo "既存 checkout を使用します: $DOTFILES_DIR"
else
  echo "dotfiles を clone します: $DOTFILES_DIR"
  git clone "$DOTFILES_REPO" "$DOTFILES_DIR"
fi

cd "$DOTFILES_DIR"

if [[ ! -f flake.nix ]]; then
  echo "flake.nix が必要です。Nix 移行後のため init フォールバックはありません（`init.sh` は削除済み）。" >&2
  exit 1
fi

if [[ ! -s flake.lock ]]; then
  echo "flake.lock が必要です。CI と bootstrap では flake input の暗黙更新を許可しません。" >&2
  exit 1
fi

if ! command -v nix >/dev/null 2>&1; then
  echo "Nix をインストールします（daemon mode）..."
  sh <(curl -L https://nixos.org/nix/install) --daemon
  if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
    # shellcheck disable=SC1091
    . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
  fi
fi

if [[ -n "${DOTFILES_SOPS_AGE_KEY_FILE:-}" ]]; then
  echo "sops age key を配置します: $DOTFILES_SOPS_AGE_KEY_DEST"
  sudo mkdir -p "$(dirname "$DOTFILES_SOPS_AGE_KEY_DEST")"
  sudo install -m 0400 "$DOTFILES_SOPS_AGE_KEY_FILE" "$DOTFILES_SOPS_AGE_KEY_DEST"
fi

echo "nix flake check を実行します"
nix "${NIX_EXTRA_ARGS[@]}" flake check "${NIX_FLAKE_ARGS[@]}"

if [[ "$DOTFILES_RUN_SWITCH" == "0" ]]; then
  echo "DOTFILES_RUN_SWITCH=0 のため、flake check 後に終了します"
  exit 0
fi

case "$DOTFILES_SWITCH_MODE" in
  darwin)
    if command -v darwin-rebuild >/dev/null 2>&1; then
      sudo darwin-rebuild switch --flake ".#$DOTFILES_FLAKE"
    else
      nix_bin="$(command -v nix)"
      sudo --preserve-env=NIX_CONFIG "$nix_bin" "${NIX_EXTRA_ARGS[@]}" run "${NIX_FLAKE_ARGS[@]}" "$DARWIN_REBUILD_INSTALLER_FLAKE" -- switch --flake ".#$DOTFILES_FLAKE"
    fi
    ;;
  home-manager)
    if command -v home-manager >/dev/null 2>&1; then
      home-manager switch --flake ".#$DOTFILES_FLAKE"
    else
      nix "${NIX_EXTRA_ARGS[@]}" run "${NIX_FLAKE_ARGS[@]}" "$HOME_MANAGER_INSTALLER_FLAKE" -- switch --flake ".#$DOTFILES_FLAKE"
    fi
    ;;
  *)
    echo "未対応の DOTFILES_SWITCH_MODE: $DOTFILES_SWITCH_MODE" >&2
    exit 1
    ;;
esac
