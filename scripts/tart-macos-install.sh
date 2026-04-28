#!/usr/bin/env bash
set -euo pipefail

repo_dir="${DOTFILES_REPO_DIR:-$(pwd -P)}"
scenario="all"
image="ghcr.io/cirruslabs/macos-sequoia-vanilla:latest"
vm_name=""
keep_vm=0
packer_log_dir=""
tart_run_log=""
tart_cli_log=""
use_sudo="${DOTFILES_TART_USE_SUDO:-0}"
ssh_user="${DOTFILES_TART_SSH_USER:-admin}"
ssh_password="${DOTFILES_TART_SSH_PASSWORD:-admin}"
vm_ip=""

usage() {
  cat <<'USAGE'
使い方: scripts/tart-macos-install.sh [options]

Options:
  --scenario all|fresh-bootstrap|second-user-home-manager|darwin-switch-ya
                              実行シナリオ（既定: all）
  --image IMAGE               Tart イメージ（既定: ghcr.io/cirruslabs/macos-sequoia-vanilla:latest）
  --vm-name NAME              利用するローカル VM 名
  --keep-vm                   終了後も VM を残す
  -h, --help                  このヘルプを表示する
USAGE
}

while (($#)); do
  case "$1" in
    --scenario)
      scenario="$2"
      shift 2
      ;;
    --image)
      image="$2"
      shift 2
      ;;
    --vm-name)
      vm_name="$2"
      shift 2
      ;;
    --keep-vm)
      keep_vm=1
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

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "tart 検証は macOS ホストでのみ実行できます" >&2
  exit 1
fi

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "tart 検証は Apple Silicon ホストでのみ実行できます" >&2
  exit 1
fi

if ! command -v tart >/dev/null 2>&1; then
  echo "tart が PATH にありません。nix run .#tart-macos-install で実行してください。" >&2
  exit 1
fi

if [[ ! -f "$repo_dir/flake.nix" || ! -d "$repo_dir/.git" ]]; then
  echo "repo_dir が dotfiles checkout を指していません: $repo_dir" >&2
  echo "repo root で実行するか、DOTFILES_REPO_DIR を指定してください。" >&2
  exit 1
fi

case "$scenario" in
  all|fresh-bootstrap|second-user-home-manager|darwin-switch-ya) ;;
  *)
    echo "不正な scenario: $scenario" >&2
    exit 1
    ;;
esac

timestamp="$(date +%Y%m%d%H%M%S)"
vm_name="${vm_name:-dotfiles-${scenario}-${timestamp}}"

guest_script_dir="$(mktemp -d)"
guest_script="$guest_script_dir/guest.sh"
guest_runner="$guest_script_dir/run-macos-install-scenario.sh"
packer_log_dir="$guest_script_dir/packer-logs"
tart_run_log="/tmp/${vm_name}.log"
tart_cli_log="$guest_script_dir/tart.log"
vm_started=0
vm_created=0

mkdir -p "$packer_log_dir"

print_log_tail() {
  local label="$1"
  local path="$2"
  local lines="${3:-200}"
  if [[ -f "$path" ]]; then
    echo "===== ${label}: ${path} (tail -n ${lines}) =====" >&2
    tail -n "$lines" "$path" >&2 || true
  fi
}

print_failure_diagnostics() {
  local status="$1"
  if [[ "$status" -eq 0 ]]; then
    return 0
  fi

  print_log_tail "tart cli log" "$tart_cli_log" 200
  print_log_tail "tart run log" "$tart_run_log" 200
}

run_with_optional_sudo() {
  local -a cmd=(/usr/bin/arch -arm64 "$@")

  if [[ "$EUID" -eq 0 ]]; then
    "${cmd[@]}"
  elif [[ "$use_sudo" == "1" ]]; then
    sudo --preserve-env=HOME,PATH,USER,LOGNAME,XDG_CONFIG_HOME,XDG_CACHE_HOME "${cmd[@]}"
  else
    "${cmd[@]}"
  fi
}

ensure_sudo_ticket() {
  if [[ "$EUID" -eq 0 ]]; then
    return 0
  fi

  if sudo -n true 2>/dev/null; then
    return 0
  fi

  sudo -v
}

run_tart_logged() {
  local label="$1"
  shift

  {
    echo "===== tart ${label} ====="
    echo "+ tart $*"
    run_with_optional_sudo tart "$@"
  } >>"$tart_cli_log" 2>&1 || {
    echo "tart failed: ${label}" >&2
    print_log_tail "tart cli log" "$tart_cli_log" 200
    return 1
  }
}

run_ssh_logged() {
  local label="$1"
  local remote_cmd="$2"
  local askpass

  askpass="$(mktemp "${TMPDIR:-/tmp}/dotfiles-tart-askpass.XXXXXX")"
  cat >"$askpass" <<'EOF'
#!/bin/sh
printf '%s\n' "${DOTFILES_TART_SSH_PASSWORD:?}"
EOF
  chmod 700 "$askpass"

  {
    echo "===== ssh ${label} ====="
    echo "+ ssh ${ssh_user}@${vm_ip} ${remote_cmd}"
    DOTFILES_TART_SSH_PASSWORD="$ssh_password" \
      DISPLAY=dummy \
      SSH_ASKPASS="$askpass" \
      SSH_ASKPASS_REQUIRE=force \
      ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o PreferredAuthentications=password -o PubkeyAuthentication=no "${ssh_user}@${vm_ip}" -- "${remote_cmd}"
  } >>"$tart_cli_log" 2>&1
  local rc=$?
  rm -f "$askpass"

  if [[ "$rc" -ne 0 ]]; then
    echo "ssh failed: ${label}" >&2
    print_log_tail "tart cli log" "$tart_cli_log" 200
  fi

  return "$rc"
}

tart_image_exists() {
  local image_name="$1"
  local list_output

  if ! list_output="$(tart list 2>&1)"; then
    printf '===== tart list =====\n%s\n' "$list_output" >>"$tart_cli_log"
    echo "tart failed: list" >&2
    print_log_tail "tart cli log" "$tart_cli_log" 200
    return 1
  fi

  printf '===== tart list =====\n%s\n' "$list_output" >>"$tart_cli_log"
  awk '{ print $1 }' <<<"$list_output" | grep -Fxq "$image_name"
}

cleanup() {
  local status=$?
  print_failure_diagnostics "$status"
  if [[ "$vm_started" == "1" ]]; then
    run_tart_logged "stop ${vm_name}" stop "$vm_name" || true
  fi
  if [[ "$keep_vm" != "1" && "$vm_created" == "1" ]]; then
    run_tart_logged "delete ${vm_name}" delete "$vm_name" || true
  fi
  rm -rf "$guest_script_dir"
  exit "$status"
}
trap cleanup EXIT

cp "$repo_dir/scripts/run-macos-install-scenario.sh" "$guest_runner"
chmod +x "$guest_runner"

cat >"$guest_script" <<'GUEST'
#!/usr/bin/env bash
set -euo pipefail

scenario="$1"
export GITHUB_WORKSPACE="/Volumes/My Shared Files/repo"
export RUNNER_TEMP="$HOME/runner-temp"
export NIX_CONFIG='experimental-features = nix-command flakes'
mkdir -p "$RUNNER_TEMP"
log_path="/Volumes/My Shared Files/guest/guest.log"
mkdir -p "$(dirname "$log_path")"
rm -f "$log_path"
exec > >(tee -a "$log_path") 2>&1

cd "$GITHUB_WORKSPACE"
exec bash "/Volumes/My Shared Files/guest/run-macos-install-scenario.sh" "$scenario"
GUEST

chmod +x "$guest_script"

if [[ "$use_sudo" == "1" && "$EUID" -ne 0 ]]; then
  ensure_sudo_ticket "Sequoia の Local Network 回避確認のため"
fi

echo "cloning VM: $image -> $vm_name"
run_tart_logged "clone ${image} -> ${vm_name}" clone "$image" "$vm_name"
vm_created=1

echo "starting VM: $vm_name"
tart run --no-graphics --dir="repo:$repo_dir:ro" --dir="guest:$guest_script_dir" "$vm_name" >"$tart_run_log" 2>&1 &
vm_started=1

echo "waiting for ssh"
for _ in $(seq 1 180); do
  if vm_ip="$(tart ip --wait 1 "$vm_name" 2>/dev/null)"; then
    if run_ssh_logged "ssh ${vm_name} /usr/bin/true" "/usr/bin/true" >/dev/null; then
      break
    fi
  fi
  sleep 2
done

if [[ -z "$vm_ip" ]] || ! run_ssh_logged "ssh ${vm_name} /usr/bin/true" "/usr/bin/true" >/dev/null; then
  echo "ssh の起動待ちに失敗しました" >&2
  exit 1
fi

echo "running scenario: $scenario"
run_ssh_logged \
  "ssh ${vm_name} guest.sh ${scenario}" \
  "/bin/bash '/Volumes/My Shared Files/guest/guest.sh' '${scenario}'"
