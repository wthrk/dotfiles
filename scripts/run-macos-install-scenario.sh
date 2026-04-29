#!/usr/bin/env bash
set -euo pipefail

scenario="${1:-}"

if [[ -z "$scenario" ]]; then
  echo "使い方: scripts/run-macos-install-scenario.sh <scenario>" >&2
  exit 1
fi

: "${GITHUB_WORKSPACE:=$(pwd)}"
: "${RUNNER_TEMP:=${TMPDIR:-/tmp}/dotfiles-runner-temp}"
: "${NIX_CONFIG:=experimental-features = nix-command flakes}"

cd "$GITHUB_WORKSPACE"
mkdir -p "$RUNNER_TEMP"

runner_info() {
  sw_vers
  uname -a
  id
  xcode-select -p
}

next_uid() {
  dscl . -list /Users UniqueID | awk '{ print $2 }' | sort -n | awk 'BEGIN { uid = 501 } { if ($1 >= uid) uid = $1 + 1 } END { print uid }'
}

ensure_local_user() {
  local user="$1"
  local full_name="$2"
  local password="$3"
  local home="$4"
  local primary_gid="$5"

  if id "$user" >/dev/null 2>&1; then
    return 0
  fi

  local uid
  uid="$(next_uid)"

  sudo dscl . -create "/Users/$user"
  sudo dscl . -create "/Users/$user" UserShell /bin/zsh
  sudo dscl . -create "/Users/$user" RealName "$full_name"
  sudo dscl . -create "/Users/$user" UniqueID "$uid"
  sudo dscl . -create "/Users/$user" PrimaryGroupID "$primary_gid"
  sudo dscl . -create "/Users/$user" NFSHomeDirectory "$home"
  sudo dscl . -passwd "/Users/$user" "$password"
}

fresh_bootstrap() {
  set -euxo pipefail
  test -s flake.lock
  runner_info

  if command -v nix >/dev/null 2>&1; then
    echo "ゼロ状態の導入テストでは Nix 未導入を前提にします。" >&2
    command -v nix >&2
    exit 1
  fi

  bash scripts/bootstrap.sh --dry-run --flake ya

  rm -rf "$RUNNER_TEMP/dotfiles-bootstrap"
  bash scripts/bootstrap.sh \
    --repo "$GITHUB_WORKSPACE" \
    --dir "$RUNNER_TEMP/dotfiles-bootstrap" \
    --flake ya \
    --mode darwin \
    --no-switch

  test -d "$RUNNER_TEMP/dotfiles-bootstrap/.git"
  test -s "$RUNNER_TEMP/dotfiles-bootstrap/flake.lock"
  test "$(git -C "$RUNNER_TEMP/dotfiles-bootstrap" rev-parse HEAD)" = "$(git -C "$GITHUB_WORKSPACE" rev-parse HEAD)"

  test ! -e /etc/bashrc.before-nix-darwin
  test ! -e /etc/zshrc.before-nix-darwin
  test ! -e /opt/homebrew/Library/Taps.before-nix-homebrew
  test ! -e /usr/local/Library/Taps.before-nix-homebrew
  before="$RUNNER_TEMP/no-switch-before.txt"
  after="$RUNNER_TEMP/no-switch-after.txt"
  : >"$before"
  for path in /etc/bashrc /etc/zshrc /opt/homebrew/Library/Taps /usr/local/Library/Taps; do
    if [[ -e "$path" ]]; then
      stat -f '%N %z %m %u %g %p' "$path" >>"$before"
    else
      printf 'missing %s\n' "$path" >>"$before"
    fi
  done

  bash "$RUNNER_TEMP/dotfiles-bootstrap/scripts/bootstrap.sh" \
    --dir "$RUNNER_TEMP/dotfiles-bootstrap" \
    --flake ya \
    --mode darwin \
    --no-switch

  : >"$after"
  for path in /etc/bashrc /etc/zshrc /opt/homebrew/Library/Taps /usr/local/Library/Taps; do
    if [[ -e "$path" ]]; then
      stat -f '%N %z %m %u %g %p' "$path" >>"$after"
    else
      printf 'missing %s\n' "$path" >>"$after"
    fi
  done
  diff -u "$before" "$after"

  rm -rf "$RUNNER_TEMP/dotfiles-missing-lock"
  git clone "$GITHUB_WORKSPACE" "$RUNNER_TEMP/dotfiles-missing-lock"
  rm "$RUNNER_TEMP/dotfiles-missing-lock/flake.lock"
  if bash "$GITHUB_WORKSPACE/scripts/bootstrap.sh" \
    --dir "$RUNNER_TEMP/dotfiles-missing-lock" \
    --flake ya \
    --mode darwin \
    --no-switch; then
    echo "flake.lock がない bootstrap が成功しました" >&2
    exit 1
  fi

  rm -rf "$RUNNER_TEMP/dotfiles-broken-flake"
  git clone "$GITHUB_WORKSPACE" "$RUNNER_TEMP/dotfiles-broken-flake"
  printf '\nthis is invalid nix\n' >>"$RUNNER_TEMP/dotfiles-broken-flake/flake.nix"
  if bash "$GITHUB_WORKSPACE/scripts/bootstrap.sh" \
    --dir "$RUNNER_TEMP/dotfiles-broken-flake" \
    --flake ya \
    --mode darwin \
    --no-switch; then
    echo "壊れた flake の bootstrap が成功しました" >&2
    exit 1
  fi

  key_dir="$RUNNER_TEMP/sops-key-test"
  mkdir -p "$key_dir"
  printf 'old-key\n' >"$key_dir/current.txt"
  printf 'new-key\n' >"$key_dir/new.txt"
  if bash scripts/bootstrap.sh \
    --dir "$RUNNER_TEMP/dotfiles-bootstrap" \
    --flake ya \
    --mode darwin \
    --no-switch \
    --sops-age-key-file "$key_dir/new.txt" \
    --sops-age-key-dest "$key_dir/current.txt"; then
    echo "異なる sops age key の上書きが成功しました" >&2
    exit 1
  fi
  test "$(cat "$key_dir/current.txt")" = "old-key"

  bash scripts/bootstrap.sh --self-test
}

second_user_home_manager() {
  set -euxo pipefail
  test -s flake.lock
  runner_info

  rm -rf "$RUNNER_TEMP/dotfiles-bootstrap"
  bash scripts/bootstrap.sh \
    --repo "$GITHUB_WORKSPACE" \
    --dir "$RUNNER_TEMP/dotfiles-bootstrap" \
    --flake ya \
    --mode darwin \
    --no-switch

  ensure_local_user dotfilesci "Dotfiles CI" "DotfilesCI-Temp-2026!" /Users/dotfilesci 20
  sudo createhomedir -c -u dotfilesci || true
  sudo mkdir -p /Users/dotfilesci
  sudo chown dotfilesci:staff /Users/dotfilesci
  sudo rm -rf /Users/dotfilesci/.dotfiles

  sudo -H -u dotfilesci env \
    HOME=/Users/dotfilesci \
    USER=dotfilesci \
    LOGNAME=dotfilesci \
    SHELL=/bin/zsh \
    NIX_CONFIG="$NIX_CONFIG" \
    bash "$GITHUB_WORKSPACE/scripts/bootstrap.sh" \
      --repo "$GITHUB_WORKSPACE" \
      --dir /Users/dotfilesci/.dotfiles \
      --flake dotfilesci \
      --mode home-manager \
      --run-switch

  sudo -H -u dotfilesci test -d /Users/dotfilesci/.dotfiles/.git
  sudo -H -u dotfilesci test -s /Users/dotfilesci/.dotfiles/flake.lock
  sudo -H -u dotfilesci test -e /Users/dotfilesci/.nix-profile
  sudo -H -u dotfilesci bash -lc '
    set -euo pipefail
    for managed_path in "$HOME/.config/zsh" "$HOME/.config/nvim" "$HOME/.zshrc" "$HOME/.zshenv"; do
      test -L "$managed_path"
      managed_target="$(readlink "$managed_path")"
      [[ "$managed_target" == /nix/store/* ]]
    done
  '
  sudo -H -u dotfilesci env \
    HOME=/Users/dotfilesci \
    USER=dotfilesci \
    LOGNAME=dotfilesci \
    SHELL=/bin/zsh \
    NIX_CONFIG="$NIX_CONFIG" \
    /nix/var/nix/profiles/default/bin/nix --extra-experimental-features "nix-command flakes" eval --no-update-lock-file /Users/dotfilesci/.dotfiles#homeConfigurations.dotfilesci.activationPackage.drvPath
}

darwin_switch_ya() {
  set -euxo pipefail
  test -s flake.lock
  runner_info

  local ya_checkout=/Users/ya/.dotfiles

  ensure_local_user ya "ya" "Ya-Temp-2026!" /Users/ya 80
  sudo createhomedir -c -u ya || true
  sudo mkdir -p /Users/ya
  sudo chown ya:staff /Users/ya
  sudo -H -u ya mkdir -p /Users/ya/Library/LaunchAgents
  sudo -H -u ya rm -f /Users/ya/Library/LaunchAgents/org.nix.colima.plist
  sudo -H -u ya ln -s /nix/store/missing-org.nix.colima.plist /Users/ya/Library/LaunchAgents/org.nix.colima.plist
  if [[ -d /opt/homebrew ]]; then
    sudo chown -R ya:admin /opt/homebrew
  fi

  sudo git config --global --add safe.directory "$GITHUB_WORKSPACE/.git"
  sudo rm -rf "$ya_checkout"
  sudo git -c safe.directory="$GITHUB_WORKSPACE/.git" clone "$GITHUB_WORKSPACE" "$ya_checkout"
  sudo chown -R ya:staff "$ya_checkout"
  sudo git config --global --add safe.directory "$ya_checkout"
  test -d "$ya_checkout/.git"
  test -s "$ya_checkout/flake.nix"
  test -s "$ya_checkout/flake.lock"

  bash scripts/bootstrap.sh \
    --repo "$GITHUB_WORKSPACE" \
    --dir "$ya_checkout" \
    --flake ya \
    --mode darwin \
    --run-switch

  nix_bin="/nix/var/nix/profiles/default/bin/nix"
  ya_path="/etc/profiles/per-user/ya/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin"
  test -x "$nix_bin"
  run_as_ya() {
    (
      cd /Users/ya
      sudo -H -u ya env \
        HOME=/Users/ya \
        USER=ya \
        LOGNAME=ya \
        SHELL=/etc/profiles/per-user/ya/bin/zsh \
        PATH="$ya_path" \
        NIX_CONFIG="$NIX_CONFIG" \
        /bin/bash -lc '
          set -euo pipefail
          exec "$@"
        ' bash "$@"
    )
  }

  run_as_ya "$nix_bin" --extra-experimental-features "nix-command flakes" flake check --no-update-lock-file "$ya_checkout"
  run_as_ya "$nix_bin" --extra-experimental-features "nix-command flakes" eval --no-update-lock-file "$ya_checkout#homeConfigurations.ya.activationPackage.drvPath"
  run_as_ya "$nix_bin" --extra-experimental-features "nix-command flakes" eval --no-update-lock-file "$ya_checkout#darwinConfigurations.ya.system"
  test -d /etc/profiles/per-user/ya
  run_as_ya /bin/bash -c '
    set -euo pipefail
    for managed_path in "$HOME/.config/zsh" "$HOME/.config/nvim" "$HOME/.zshrc" "$HOME/.zshenv"; do
      test -L "$managed_path"
      managed_target="$(readlink "$managed_path")"
      [[ "$managed_target" == /nix/store/* ]]
    done
  '
  run_as_ya /etc/profiles/per-user/ya/bin/zsh -il -c '
    set -euo pipefail
    for tool in git gh jq rg fzf atuin zoxide nvim; do
      tool_path="$(command -v "$tool")"
      real_tool_path="${tool_path:A}"
      case "$real_tool_path" in
        /nix/store/*) ;;
        *)
          echo "$tool resolved outside the Nix store: $tool_path -> $real_tool_path" >&2
          exit 1
          ;;
      esac
    done
  '
  brew="/opt/homebrew/bin/brew"
  run_as_ya "$brew" tap | /usr/bin/grep -Fxq "azure/bicep"
  run_as_ya "$brew" tap | /usr/bin/grep -Fxq "hashicorp/tap"
  if run_as_ya "$brew" list --formula | /usr/bin/grep -Fxq "packer"; then
    echo "packer formula remains after Homebrew cleanup" >&2
    exit 1
  fi
  run_as_ya /bin/bash -c '
    set -euo pipefail
    plist="$HOME/Library/LaunchAgents/org.nix.colima.plist"
    old_plist="$HOME/Library/LaunchAgents/homebrew.mxcl.colima.plist"
    plist_target="$plist"

    test -e "$plist"
    if [[ -L "$plist" ]]; then
      plist_target="$(readlink "$plist")"
      test -n "$plist_target"
      test -e "$plist_target"
    else
      test -f "$plist"
    fi
    test -s "$plist_target"
    test ! -e "$old_plist"
    /usr/bin/grep -Eq "/nix/store/.*/bin/colima" "$plist_target"
  '
  uid="$(id -u ya)"
  if run_as_ya launchctl print "gui/$uid/org.nix.colima" 2>&1 | tee "$RUNNER_TEMP/org.nix.colima.launchd" >/dev/null; then
    /usr/bin/grep -Eq "/nix/store/.*/bin/colima" "$RUNNER_TEMP/org.nix.colima.launchd"
  fi
  if run_as_ya launchctl print "gui/$uid/homebrew.mxcl.colima" >/dev/null 2>&1; then
    echo "homebrew.mxcl.colima is still loaded" >&2
    exit 1
  fi
}

case "$scenario" in
  fresh-bootstrap) fresh_bootstrap ;;
  second-user-home-manager) second_user_home_manager ;;
  darwin-switch-ya) darwin_switch_ya ;;
  all)
    fresh_bootstrap
    second_user_home_manager
    darwin_switch_ya
    ;;
  *)
    echo "不正な scenario: $scenario" >&2
    exit 1
    ;;
esac
