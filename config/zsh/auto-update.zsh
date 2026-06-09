# dotfiles 自動アップデートの shell 連携（pending-summary の show-once 表示のみ）。
#
# 単純版ではシェル側の責務を「background 適用（launchd daemon の `dotfiles update`）が残した
# `pending-summary` をログイン時に 1 回だけ表示して原子的に消費する」ことだけに縮小する。catch-up 起動・
# trigger marker・scope cursor・detach 適用（`dotfiles update home` の login catch-up）は撤去した。
# 追随は launchd daemon の `dotfiles update`（rev ベースで冪等）に一本化されており、シェルは適用しない。
#
# このフックは「インタラクティブシェルでのログイン体験を壊さない」ことを最優先にする。重い処理は一切せず、
# 既存 marker の表示だけを行う。dotfiles 不在・state 不在・失敗時は静かに no-op し、ログインを止めない。
#
# 状態ディレクトリ・ファイル名は `rust/dotfiles-cli/src/update.rs` の契約に一致させる
# （state dir = `$XDG_STATE_HOME/dotfiles`、未設定なら `$HOME/.local/state/dotfiles`）。

# インタラクティブシェル限定。非対話（スクリプト・zsh checks の一部経路）では何もしない。
[[ -o interactive ]] || return 0

# 利用者が明示的に無効化したら何もしない。
[[ -n "${DOTFILES_AUTO_UPDATE_DISABLE:-}" ]] && return 0

# state dir は update.rs と同じ規則: $XDG_STATE_HOME/dotfiles、未設定なら $HOME/.local/state/dotfiles。
_dotfiles_auto_update_state_dir() {
  local base="${XDG_STATE_HOME:-$HOME/.local/state}"
  print -r -- "$base/dotfiles"
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
