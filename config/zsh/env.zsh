# `.zshrc` より前に必要な環境変数をここで決める。
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

export EDITOR="nvim"

# Nix と許可済みローカルパスの優先順位を保ったまま、重複だけを落とす。
typeset -U path PATH

# macOS と Nix daemon が前提にする最小限のシステムパス。
path=(/usr/sbin /sbin $path)

# Nix 管理外でも、既存ツールとの連携のために残すユーザー所有パス。
path=("$HOME/.agent-tools/bin" "$HOME/.rd/bin" $path)

# ユーザーごとの bin は使えるようにするが、言語管理ツールの shim は優先しない。
path=(
  "$HOME/.local/bin"
  $path
)

# rbenv、pyenv、nodebrew などの可変 shim が Nix 管理ツールより先に来ることを防ぐ。
#
# `$HOME` 完全一致では落とし切れない。継承した PATH には別ホーム配下の shim
#（CI runner の `/Users/runner/.cargo/bin`、sudo 経由や旧ユーザー名の残骸など）が混ざりうる。
# それらも Nix 管理ツールより先に来れば同じ事故になるため、ホーム位置に依存しない
# 末尾一致で落とす。`*` は path 要素全体に対する glob。
path=(${path:#*/.nodebrew/current/bin})
path=(${path:#*/.bun/bin})
path=(${path:#*/.cargo/bin})
path=(${path:#*/.pyenv/bin})
path=(${path:#*/.rbenv/bin})

export PATH

# GPG / gpg-agent SSH support の前提環境変数。
# pinentry が制御端末を解決できるよう、対話シェルで GPG_TTY を設定する。
export GPG_TTY="$(tty)"

# gpg-agent SSH support 経路を使う場合だけ SSH_AUTH_SOCK を上書きする。
# `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として存在するときだけ上書きし、
# 未存在時は既存の SSH_AUTH_SOCK を保持する（GPG authentication subkey 経由の SSH 経路）。
_gpg_agent_sock="${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh"
[[ -S "$_gpg_agent_sock" ]] && export SSH_AUTH_SOCK="$_gpg_agent_sock"
unset _gpg_agent_sock
