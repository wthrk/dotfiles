# dotfiles 自動アップデートの shell 連携（pending-summary の show-once 表示のみ）。
#
# シェル側の責務は「background 適用（launchd daemon の `dotfiles update`）が残した `pending-summary` を
# ログイン時に 1 回だけ表示して原子的に消費する」ことだけである。repo pin への追随は launchd daemon の
# `dotfiles update`（rev ベースで冪等）に一本化されており、シェルは適用しない。
#
# このフックは「インタラクティブシェルでのログイン体験を壊さない」ことを最優先にする。重い処理は一切せず、
# 既存 marker の表示だけを行う。dotfiles 不在・state 不在・失敗時は静かに no-op し、ログインを止めない。
#
# 状態ディレクトリ・ファイル名は `rust/dotfiles-cli/src/update.rs` の契約に一致させる
# （state dir = `$HOME/.local/state/dotfiles` 固定）。launchd daemon は clean env で動き利用者の interactive
# `XDG_STATE_HOME` を見られないため、CLI 側は XDG 非依存に HOME 基準で固定する。shell hook も同じ HOME 基準を
# 読み、daemon・手動 CLI・shell の state dir を一致させて show-once（pending-summary 消費）を成立させる。

# インタラクティブシェル限定。非対話（スクリプト・zsh checks の一部経路）では何もしない。
[[ -o interactive ]] || return 0

# 利用者が明示的に無効化したら何もしない。
[[ -n "${DOTFILES_AUTO_UPDATE_DISABLE:-}" ]] && return 0

# state dir は update.rs と同じ規則: $HOME/.local/state/dotfiles 固定（XDG_STATE_HOME 非依存）。daemon の clean
# env で見えない XDG を参照しないことで daemon・手動 CLI・shell の state dir を一致させる。
_dotfiles_auto_update_state_dir() {
  print -r -- "$HOME/.local/state/dotfiles"
}

# pending-summary を 1 回だけ表示して原子的に消費する。
#
# 表示前に `pending-summary` を `pending-summary.consuming.$$`（$$=シェル PID）へ rename して所有権を取る。
# rename はディレクトリ内で原子的なので、複数端末が同時に起動しても rename に成功した 1 端末だけが内容を
# 表示でき、二重表示を防ぐ。所有した端末は内容を表示してから `pending-summary.shown` へ **追記** で退避し
# （連続適用の複数 rev ブロックを上書きで失わない）、`consuming` 一時ファイルを削除する。rename に失敗
# （既に他端末が消費 / そもそも存在しない）したら何もしない。
_dotfiles_auto_update_consume_pending() {
  local state_dir pending shown
  state_dir="$(_dotfiles_auto_update_state_dir)"
  pending="$state_dir/pending-summary"
  [[ -f "$pending" ]] || return 1

  shown="$state_dir/pending-summary.shown"
  if command mv -f -- "$pending" "$pending.consuming.$$" 2>/dev/null; then
    command cat -- "$pending.consuming.$$"
    command cat -- "$pending.consuming.$$" >> "$shown" 2>/dev/null
    command rm -f -- "$pending.consuming.$$" 2>/dev/null
    return 0
  fi
  return 1
}

# ログイン時のエントリ。pending-summary を 1 回表示して消費するだけ（catch-up 起動はしない）。
_dotfiles_auto_update_init() {
  _dotfiles_auto_update_consume_pending
}

_dotfiles_auto_update_init
