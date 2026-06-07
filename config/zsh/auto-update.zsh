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

# 当日 catch-up を既に **成功させたか** を判定する（起動可否の gate）。未成功なら 0（=今日まだ起動すべき）。
#
# catch-up 起動頻度の抑制専用 marker（`last-login-trigger`）。中身は当日（`date +%F`）で、毎シェルではなく
# 1 日 1 回だけ `dotfiles update` を起動するための gate にする。当日分が既に記録されていれば 1 を返して
# 起動を抑止し、未記録（不在 / 別日）なら 0 を返す（=今日まだ起動していない）。
#
# **marker は起動「前」ではなく catch-up が成功した「後」に確定する**（重要・失敗日の再試行）。旧実装は
# background 起動の前に当日付を書いていたため、初回ログインで network 不通や一時的な `nix flake update` 失敗が
# 起きても、同日中の後続シェルは marker を見て再試行しなかった。daemon が動かないマシン（ログイン主体・スリープ
# 運用）ほどこの catch-up が唯一の追随経路なので、失敗日は再試行できる必要がある。よってこの関数は marker を
# **読むだけ**（書かない）にし、marker の確定は background の `dotfiles update home` が成功で終了した時にだけ
# 行う（`_dotfiles_auto_update_run_catchup`）。当日に未成功なら、同日でも次のシェルで再び起動を試みる。
#
# 同日に複数シェルが起動して二重に background を立ち上げても、実適用の重複は `dotfiles update` 側の
# `update.lock` が吸収する（後発は skip）。よって marker を成功時確定に倒しても二重適用は起きない。実適用の
# 重複抑止は別レイヤ（rev ベースの `last-applied-rev`、`dotfiles update` 側）が担う。
_dotfiles_auto_update_should_trigger_today() {
  local state_dir today marker
  state_dir="$(_dotfiles_auto_update_state_dir)"
  today="$(date +%F 2>/dev/null)" || return 1
  [[ -n "$today" ]] || return 1
  marker="$state_dir/last-login-trigger"

  # 当日分（成功記録）が既にあれば抑止する（今日はもう成功した）。未記録 / 別日なら起動側に倒す。
  if [[ -r "$marker" && "$(<"$marker")" == "$today" ]]; then
    return 1
  fi
  return 0
}

# catch-up を background で実行し、**成功した時だけ** 当日 trigger marker を確定する。
#
# `dotfiles update home --defer-rev-marker` が 0 で終了した時のみ `last-login-trigger` を当日付へ原子的に
# 書く。失敗（network 不通・`nix flake update` 失敗・lock 競合 skip 以外の異常）時は marker を書かないので、
# 同日中の後続シェルが再び起動を試みて追随を回復できる（失敗日の再試行）。marker は起動頻度の抑制専用で、
# 実適用の重複抑止（rev ベースの `last-applied-rev`）とは別物。要約は非 tty 適用のため `pending-summary` へ
# 書かれ、`precmd` フックが拾って表示する。
_dotfiles_auto_update_run_catchup() {
  local state_dir today marker tmp
  state_dir="$(_dotfiles_auto_update_state_dir)"

  # **target は home に限定する**（既定 `all` を使わない）。detach した非 tty で `dotfiles update` の既定 `all`
  # を呼ぶと darwin 適用が `sudo darwin-rebuild` を起動し、tty が無いためパスワード入力できず停止する。
  # **`--defer-rev-marker`** で `last-applied-rev` を確定させない（home だけ適用して rev を確定すると、その rev
  # の darwin 適用が daemon / 対話 `dotfiles update` で永久に skip される）。詳細は init 関数のコメント参照。
  if ! dotfiles update home --defer-rev-marker >/dev/null 2>&1; then
    # 失敗した。marker を確定しない（同日の後続シェルが再試行できるよう起動可否を開けておく）。
    return 1
  fi

  # 成功した。当日 trigger marker を確定して、同日中の重複起動を抑止する。
  today="$(date +%F 2>/dev/null)" || return 0
  [[ -n "$today" ]] || return 0
  marker="$state_dir/last-login-trigger"
  # state dir が無ければ作る（ユーザ所有）。失敗しても致命にしない（次回起動側に倒れるだけ）。
  command mkdir -p -- "$state_dir" 2>/dev/null
  # 当日付を原子的に書く（temp→rename）。
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
  # 程度のトリガ marker で、当日まだ catch-up を成功させていなければ detach で `dotfiles update home` を
  # 起動する。`dotfiles update` は remote 解決（`nix flake update dotfiles`）を含み rev ベースで no-op 冪等
  # なので、当日初回に必ず upstream を解決し直し、変化があれば適用、無ければ何もしない。
  #
  # marker は **起動前ではなく成功後** に確定する（`_dotfiles_auto_update_run_catchup`）。初回ログインで
  # network 不通や一時的な `nix flake update` 失敗が起きた日は marker を書かないため、同日の後続シェルが
  # 再試行して追随を回復できる（daemon が動かないマシンほど catch-up が唯一の追随経路）。実適用の重複は
  # `dotfiles update` 側 `update.lock` が吸収するため、同日に複数シェルが起動しても二重適用にならない。
  if _dotfiles_auto_update_should_trigger_today; then
    # ログインをブロックしないよう detach で起動する。多重起動は dotfiles 側 lock が吸収する。適用は非 tty
    # なので要約は `pending-summary` へ書かれ、下の precmd フックが拾って表示する。target/`--defer-rev-marker`
    # の選択理由と marker 成功時確定は `_dotfiles_auto_update_run_catchup` のコメントに集約する。
    { _dotfiles_auto_update_run_catchup } &!
    autoload -Uz add-zsh-hook
    add-zsh-hook precmd _dotfiles_auto_update_precmd
  fi
}

_dotfiles_auto_update_init
