# dotfiles 自動アップデートの shell 連携（pending-summary 表示と catch-up 起動）。
#
# このフックは「インタラクティブシェルでのログイン体験を壊さない」ことを最優先にする。重い処理は
# 一切せず、判定（軽量なファイル比較）と表示だけを行い、実適用は detach した `dotfiles update` へ委ねる。
# dotfiles 不在・state 不在・失敗時は静かに no-op し、ログインを止めない。
#
# 振る舞い:
#   1. ログイン時、background 適用（launchd daemon 等）が残した `pending-summary` を 1 回だけ表示し、
#      原子的 rename で消費する（複数端末は rename の原子性で最初の 1 端末のみが消費に成功）。
#   2. 1 日 1 回程度、detach で `dotfiles update home` を起動する（ログインをブロックしない）。多重起動は
#      `dotfiles update` 側の lock が吸収する。detach 後はその端末の `precmd` で `pending-summary` を
#      1 回拾って表示し、消費したら監視を終了する。
#
# catch-up 起動可否を stale local pin で決めない（重要）:
#   旧実装は「ローカル flake.lock の dotfiles pin != last-applied-rev」でのみ起動していたが、これは定常状態
#   （local pin == last-applied-rev だが upstream は nightly bump で進んでいる）で永久に起動せず、daemon が
#   走らなかったマシン（ログイン主体・スリープ運用等）が upstream へ追随できなかった。ローカル pin は
#   `nix flake update dotfiles` を実行するまで前回適用値のまま動かないため、ローカル pin 比較では upstream の
#   進行を検知できない。よって catch-up 起動は **stale local pin 比較で決めず**、`dotfiles update`（remote 解決
#   を含み rev ベースで no-op 冪等）を 1 日 1 回程度のトリガ marker（`last-login-trigger`、当日未トリガなら
#   起動）で起動する。これにより upstream 変化を確実に検知する。トリガ marker は「起動頻度の抑制」専用で、
#   実適用の重複抑止（rev ベースの `last-applied-rev`）とは別物（rev ベース dedup は `dotfiles update` 側が
#   維持する）。毎シェルで `dotfiles update` を叩かないこと。
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

# 当日 catch-up を既に起動済みかを判定し、未起動なら trigger marker を当日付で更新して 0 を返す。
#
# catch-up 起動頻度の抑制専用 marker（`last-login-trigger`）。中身は当日（`date +%F`）で、毎シェルではなく
# 1 日 1 回だけ `dotfiles update` を起動するための gate にする。当日分が既に記録されていれば 1 を返して
# 起動を抑止し、未記録（不在 / 別日）なら marker を当日付へ原子的に更新して 0 を返す（=今日初回起動）。
# 実適用の重複抑止は別レイヤ（rev ベースの `last-applied-rev`、`dotfiles update` 側）が担うため、この marker は
# upstream 検知のための「毎日 1 回は remote 解決を試す」起動頻度制御だけに使う。
_dotfiles_auto_update_should_trigger_today() {
  local state_dir today marker tmp
  state_dir="$(_dotfiles_auto_update_state_dir)"
  today="$(date +%F 2>/dev/null)" || return 1
  [[ -n "$today" ]] || return 1
  marker="$state_dir/last-login-trigger"

  # 当日分が既に記録されていれば抑止する（今日はもう起動した）。
  if [[ -r "$marker" && "$(<"$marker")" == "$today" ]]; then
    return 1
  fi

  # state dir が無ければ作る（ユーザ所有）。失敗しても起動判断は続行する（marker 不在時は起動側に倒す）。
  command mkdir -p -- "$state_dir" 2>/dev/null
  # 当日付を原子的に書く（temp→rename）。書込み失敗でも今日初回として起動側に倒す。
  tmp="$marker.$$.tmp"
  if print -r -- "$today" > "$tmp" 2>/dev/null; then
    command mv -f -- "$tmp" "$marker" 2>/dev/null
  fi
  return 0
}

# pending-summary を 1 回だけ表示して原子的に消費する。
#
# 表示前に `pending-summary` を `pending-summary.consuming.$$`（$$=シェル PID）へ rename して所有権を取る。
# rename はディレクトリ内で原子的なので、複数端末が同時に起動しても rename に成功した 1 端末だけが内容を
# 表示でき、二重表示を防ぐ。所有した端末は内容を表示してから `pending-summary.shown` へ**追記**で退避し
# （連続適用の複数 rev ブロックを上書きで失わない）、`consuming` 一時ファイルを削除する。rename に失敗
# （既に他端末が消費 / そもそも存在しない）したら何もしない。`.shown` は表示済みの記録として残す。
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

  # catch-up 起動可否は **stale local pin で決めない**（定常状態で upstream を検知できないため）。1 日 1 回
  # 程度のトリガ marker で、当日未トリガなら detach で `dotfiles update home` を起動する。`dotfiles update` は
  # remote 解決（`nix flake update dotfiles`）を含み rev ベースで no-op 冪等なので、当日初回に必ず upstream を
  # 解決し直し、変化があれば適用、無ければ何もしない。
  if _dotfiles_auto_update_should_trigger_today; then
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
