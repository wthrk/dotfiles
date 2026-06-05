# dotfiles 自動アップデートの shell 連携（pending-summary 表示と catch-up 起動）。
#
# このフックは「インタラクティブシェルでのログイン体験を壊さない」ことを最優先にする。重い処理は
# 一切せず、判定（軽量なファイル比較）と表示だけを行い、実適用は detach した `dotfiles update` へ委ねる。
# dotfiles 不在・state 不在・失敗時は静かに no-op し、ログインを止めない。
#
# 振る舞い:
#   1. ログイン時、background 適用（launchd daemon 等）が残した `pending-summary` を 1 回だけ表示し、
#      原子的 rename で消費する（複数端末は rename の原子性で最初の 1 端末のみが消費に成功）。
#   2. 現在の repo pin（ローカル flake の `flake.lock` の dotfiles input rev）が `last-applied-rev` と
#      異なれば、detach で `dotfiles update` を起動する（ログインをブロックしない）。多重起動は
#      `dotfiles update` 側の lock が吸収する。detach 後はその端末の `precmd` で `pending-summary` を
#      1 回拾って表示し、消費したら監視を終了する。
#
# 状態ディレクトリ・ファイル名・pin の所在は `rust/dotfiles-cli/src/update.rs` の契約に一致させる。

# インタラクティブシェル限定。非対話（スクリプト・zsh checks の一部経路）では何もしない。
[[ -o interactive ]] || return 0

# 利用者が明示的に無効化したら何もしない。
[[ -n "${DOTFILES_AUTO_UPDATE_DISABLE:-}" ]] && return 0

# state dir は update.rs と同じ規則: $XDG_STATE_HOME/dotfiles、未設定なら $HOME/.local/state/dotfiles。
_dotfiles_auto_update_state_dir() {
  local base="${XDG_STATE_HOME:-$HOME/.local/state}"
  print -r -- "$base/dotfiles"
}

# config dir は update.rs と同じ規則: $DOTFILES_CONFIG_DIR、未設定なら $HOME/.config/dotfiles。
_dotfiles_auto_update_config_dir() {
  print -r -- "${DOTFILES_CONFIG_DIR:-$HOME/.config/dotfiles}"
}

# pending-summary を 1 回だけ表示して原子的に消費する。
#
# 表示前に `pending-summary` を `pending-summary.shown.<pid>.<epoch>` へ rename する。rename はディレクトリ内で
# 原子的なので、複数端末が同時に起動しても rename に成功した 1 端末だけが内容を表示でき、二重表示を防ぐ。
# rename に失敗（既に他端末が消費 / そもそも存在しない）したら何もしない。表示済みファイルは記録として残す。
_dotfiles_auto_update_consume_pending() {
  local state_dir pending shown
  state_dir="$(_dotfiles_auto_update_state_dir)"
  pending="$state_dir/pending-summary"
  [[ -f "$pending" ]] || return 1

  # 表示済みマーカーへ原子的に rename して所有権を取る。失敗した端末は消費を諦める。
  shown="$state_dir/pending-summary.shown"
  # 複数 rev ぶんが連続適用されている場合に上書きで失わないよう、消費したブロックは追記で退避する。
  if command mv -f -- "$pending" "$pending.consuming.$$" 2>/dev/null; then
    command cat -- "$pending.consuming.$$"
    command cat -- "$pending.consuming.$$" >> "$shown" 2>/dev/null
    command rm -f -- "$pending.consuming.$$" 2>/dev/null
    return 0
  fi
  return 1
}

# 現在の repo pin を `flake.lock` の dotfiles input locked rev から読む。
#
# `jq` があれば `nodes.dotfiles.locked.rev` で厳密抽出する（JSON 構造を正しく辿るため誤検知しない）。`jq` が
# 無い環境では awk で素朴に拾うが、**dotfiles ノードのブロック範囲に限定**する。素朴な awk が `"dotfiles":` を
# 一度見ると EOF まで in_node を維持すると、dotfiles ノード内に rev が無い／取り損ねた場合に後続ノードの rev を
# 誤って拾い、現在 pin でない値で catch-up を不要起動してしまう。これを防ぐため、`"dotfiles":` 行で in_node を
# 開始し、ネスト深さ（`{`/`}` の数）が dotfiles ノード開始時の深さへ戻った時点で in_node を閉じる。確実性より
# 「軽量・失敗で no-op」を優先し、取れなければ空文字を返して catch-up を起動しない（誤起動より未起動に倒す）。
# 実際の rev 判定と適用は `dotfiles update` 側（堅牢な JSON パーサ + lock）が単一の正本として行う。
_dotfiles_auto_update_repo_pin() {
  local lock
  lock="$(_dotfiles_auto_update_config_dir)/flake.lock"
  [[ -r "$lock" ]] || return 1

  # jq があれば構造を正しく辿って厳密抽出する（誤検知なし）。null/欠落は空文字に倒す。
  if (( $+commands[jq] )); then
    local rev
    rev="$(jq -r '.nodes.dotfiles.locked.rev // empty' "$lock" 2>/dev/null)"
    print -r -- "$rev"
    return 0
  fi

  # jq 不在時の fallback。dotfiles ノードのブロック範囲（中括弧のネスト深さ）に限定して rev を拾う。
  awk '
    !in_node && /"dotfiles"[[:space:]]*:/ {
      in_node = 1
      start_depth = depth
    }
    {
      # 行内の中括弧でネスト深さを更新する。in_node 開始時の深さへ戻ったらノードを抜ける。
      n_open = gsub(/{/, "{"); n_close = gsub(/}/, "}")
      if (in_node && match($0, /"rev"[[:space:]]*:[[:space:]]*"[0-9a-fA-F]+"/)) {
        s = substr($0, RSTART, RLENGTH)
        gsub(/.*"rev"[[:space:]]*:[[:space:]]*"/, "", s)
        gsub(/".*/, "", s)
        print s
        exit
      }
      depth += n_open - n_close
      if (in_node && depth <= start_depth) {
        # dotfiles ノードを rev 未取得のまま抜けた。後続ノードの rev を拾わないよう探索を打ち切る。
        exit
      }
    }
  ' "$lock" 2>/dev/null
}

# detach 起動した適用が完了して `pending-summary` を書いたら、その端末で 1 回拾って表示し消費する。
#
# 適用は detach（background）なので即座には pending が現れない。`precmd` ごとに pending を確認し、消費に
# 成功したらこのフック自身を `precmd` から外して監視を終了する（二重表示防止は consume の原子的 rename と整合）。
_dotfiles_auto_update_precmd() {
  if _dotfiles_auto_update_consume_pending; then
    add-zsh-hook -d precmd _dotfiles_auto_update_precmd
  fi
}

# ログイン時のエントリ。表示（show-once）と catch-up 判定を行う。重い処理はしない。
_dotfiles_auto_update_init() {
  # `dotfiles` が PATH に無ければ catch-up は起動できない。表示だけ行って戻る。
  if ! (( $+commands[dotfiles] )); then
    _dotfiles_auto_update_consume_pending
    return 0
  fi

  # 既に background 適用が残した要約があれば、まず 1 回表示して消費する。
  _dotfiles_auto_update_consume_pending

  # 現在 pin と last-applied-rev を比較し、相違時のみ detach で適用を起動する。
  local pin applied state_dir
  pin="$(_dotfiles_auto_update_repo_pin)" || return 0
  [[ -n "$pin" ]] || return 0
  state_dir="$(_dotfiles_auto_update_state_dir)"
  applied=""
  [[ -r "$state_dir/last-applied-rev" ]] && applied="$(<"$state_dir/last-applied-rev")"
  applied="${applied//[[:space:]]/}"

  if [[ "$pin" != "$applied" ]]; then
    # ログインをブロックしないよう detach で起動する。多重起動は dotfiles 側 lock が吸収する。
    # 適用は非 tty なので要約は `pending-summary` へ書かれ、下の precmd フックが拾って表示する。
    #
    # **target は home に限定する**（既定 `all` を使わない）。detach した非 tty プロセスで `dotfiles update`
    # の既定 `all` を呼ぶと darwin 適用が `sudo darwin-rebuild` を起動し、tty が無いためパスワード入力できず
    # 停止する。すると `last-applied-rev` 更新・`pending-summary` 書込みへ到達せず、表示も catch-up も完了
    # しない。シェル catch-up は user 権限で完結する home 適用だけを行い、darwin 適用は root daemon
    # （auto-update.nix の launchd daemon）に委ねる。home 側で lock 更新と switch home は進むため、ユーザは
    # home 更新を即時反映でき、要約表示（pending-summary/precmd）も完了する。
    { dotfiles update home >/dev/null 2>&1 } &!
    autoload -Uz add-zsh-hook
    add-zsh-hook precmd _dotfiles_auto_update_precmd
  fi
}

_dotfiles_auto_update_init
