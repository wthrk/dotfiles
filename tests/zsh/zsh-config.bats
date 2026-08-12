#!/usr/bin/env bats
#
# 構成を適用済みのマシンで zsh をログインシェルとして（`-l`）起動し、その環境を検証する。適用は
# 検証の外側で済ませる。起動を `script(1)` 経由にするのは、制御端末が無いと compinit と zle が
# 初期化されないため。
#
# 起動する zsh は実行環境から決める。実行ユーザーのログインシェルが zsh ならそれ自身を起動し、その
# ユーザーが実際にログインしたときの環境をそのまま観測する。system 層を持たないサブユーザーで走らせた
# ときに `/etc/profiles/per-user/<user>/bin` が無い状態で何が成立するかは、これでしか見られない。
#
# ログインシェルが zsh でない環境（CI runner の `runner` は `/bin/bash`）には観測できるログイン環境が
# 無いので、構成が入れた zsh を同じ形で起動し、設定そのものの検証だけを行う。

setup() {
    bats_load_library bats-support
    bats_load_library bats-assert
}

setup_file() {
    ZSH_CHECK_USER="$(id -un)"
    ZSH_CHECK_SHELL="$(zsh_probe_shell "${ZSH_CHECK_USER}")"
    if [ -z "${ZSH_CHECK_SHELL}" ]; then
        echo "${ZSH_CHECK_USER} が起動できる zsh が無い。構成を適用してから実行する" >&2
        return 1
    fi

    local zshrc_target=""
    if [ -L "${HOME}/.zshrc" ]; then
        zshrc_target="$(readlink "${HOME}/.zshrc")"
    fi
    case "${zshrc_target}" in
        /nix/store/*) ;;
        *)
            echo "${HOME}/.zshrc が Home Manager の生成物ではない。構成を適用してから実行する" >&2
            return 1
            ;;
    esac

    export ZSH_CHECK_USER ZSH_CHECK_SHELL
}

# 起動する zsh を決める。ログインシェルが zsh のユーザーではそれ自身を返し、実際のログイン環境を
# 観測する。そうでなければ構成が入れた zsh を返す。どちらも無ければ空を返し、setup_file が止める。
zsh_probe_shell() {
    local login_shell
    login_shell="$(zsh_login_shell "$1")"
    if [ "${login_shell##*/}" = 'zsh' ] && [ -x "${login_shell}" ]; then
        printf '%s' "${login_shell}"
        return 0
    fi
    if [ -x "${HOME}/.nix-profile/bin/zsh" ]; then
        printf '%s' "${HOME}/.nix-profile/bin/zsh"
    fi
}

# 対象ユーザーのログインシェルを、実行環境のユーザーデータベースから読む。
#
# `$SHELL` は呼び出し元の値なので使えない。ログインシェルはユーザーごとに違うため、実際に
# 記録されているものを読む。macOS には `getent` が無く Linux には `dscl` が無いので、在る方を使う。
zsh_login_shell() {
    if command -v getent >/dev/null 2>&1; then
        getent passwd "$1" | awk -F: '{ print $7 }'
    else
        dscl . -read "/Users/$1" UserShell 2>/dev/null | awk '/^UserShell:/ { print $2 }'
    fi
}

# 渡した zsh コードの出力を制御文字除去済みで返す。終了 status は判定に使わない
# （観測対象が存在しないときに非 0 で終わる probe も渡すため）。
#
# 環境は `env -i` から組む。継承すると `hm-session-vars.sh` の再実行 guard と `XDG_*` が
# 呼び出し元の値で潰れ、生成された設定が何を export するかを見なくなる。
zsh_probe() {
    local raw code_file="${BATS_TEST_TMPDIR}/probe.zsh"
    printf '%s\n' "$1" >"${code_file}"
    # `script(1)` の引数の並びは BSD 版と util-linux 版で違う。実行環境のものに合わせる。
    if script --version 2>/dev/null | grep -q util-linux; then
        raw="$(zsh_probe_env script -qec "'${ZSH_CHECK_SHELL}' -lic 'source \"${code_file}\"'" /dev/null)"
    else
        raw="$(zsh_probe_env script -q /dev/null "${ZSH_CHECK_SHELL}" -lic "source \"${code_file}\"")"
    fi
    strip_terminal_control "${raw}"
}

# ログイン直後の環境に相当する最小の変数だけを渡して起動する。
zsh_probe_env() {
    env -i \
        HOME="${HOME}" \
        USER="${ZSH_CHECK_USER}" \
        LOGNAME="${ZSH_CHECK_USER}" \
        SHELL="${ZSH_CHECK_SHELL}" \
        TERM=xterm-256color \
        LANG=en_US.UTF-8 \
        PATH="$(zsh_probe_path)" \
        "$@"
}

# ログインシェルが起動される時点の PATH。Nix の profile はここに入れず、起動ファイルが自分で
# 足すものだけを観測する。`ZSH_CHECK_INHERITED_PATH` は除外規則の検証だけが使う。
zsh_probe_path() {
    printf '%s' \
        "${ZSH_CHECK_INHERITED_PATH:+${ZSH_CHECK_INHERITED_PATH}:}/usr/bin:/bin:/usr/sbin:/sbin"
}

# `script(1)` が pty 出力へ混ぜる制御文字と前後の空白を落とす。
strip_terminal_control() {
    local text="$1"
    text="${text//$'\r'/}"
    text="${text//$'\x04'$'\b'$'\b'/}"
    text="${text//'^D'$'\b'$'\b'/}"
    text="${text#"${text%%[![:space:]]*}"}"
    text="${text%"${text##*[![:space:]]}"}"
    printf '%s\n' "${text}"
}

@test 'fzf-tab の widget が登録されている' {
    run zsh_probe 'zle -la'
    assert_line 'fzf-tab-complete'
}

@test 'autosuggestions の widget が登録されている' {
    run zsh_probe 'zle -la'
    assert_line 'autosuggest-accept'
}

# store パスではなく定義された関数で見る。store パスは Home Manager の出力が動くたびに変わる。
@test 'fast-syntax-highlighting が読み込まれている' {
    run zsh_probe '(( ${+functions[fast-theme]} && ${+functions[_zsh_highlight]} )) && print loaded'
    assert_output 'loaded'
}

# TAB は全 keymap で通常補完のままにし、fzf-tab は Ctrl-X TAB 側へ割り当てる。
@test 'TAB は emacs keymap で通常補完のまま' {
    run zsh_probe "bindkey -M emacs '^I'"
    assert_output '"^I" expand-or-complete'
}

@test 'TAB は viins keymap で通常補完のまま' {
    run zsh_probe "bindkey -M viins '^I'"
    assert_output '"^I" expand-or-complete'
}

@test 'TAB は vicmd keymap で通常補完のまま' {
    run zsh_probe "bindkey -M vicmd '^I'"
    assert_output '"^I" expand-or-complete'
}

@test 'Ctrl-X TAB が emacs keymap で fzf-tab を起動する' {
    run zsh_probe "bindkey -M emacs '^X^I'"
    assert_output '"^X^I" fzf-tab-complete'
}

# 継承 PATH に shim があるかを実行環境へ委ねると検査が空振りするため、ここで混ぜる。
@test '旧 language-manager の shim は継承 PATH から除外される' {
    local legacy_home="${BATS_TEST_TMPDIR}/legacy"
    local legacy_path="${legacy_home}/.nodebrew/current/bin:${legacy_home}/.bun/bin"
    legacy_path="${legacy_path}:${legacy_home}/.cargo/bin:${legacy_home}/.pyenv/bin"
    legacy_path="${legacy_path}:${legacy_home}/.rbenv/bin"

    ZSH_CHECK_INHERITED_PATH="${legacy_path}" run zsh_probe 'print -l $path'

    # 起動に失敗して出力が空でも不在検査は通るため、先に PATH を組み立てた痕跡を確かめる。
    assert_output --partial '.agent-tools/bin'
    refute_output --partial '.nodebrew/current/bin'
    refute_output --partial '.bun/bin'
    refute_output --partial '.cargo/bin'
    refute_output --partial '.pyenv/bin'
    refute_output --partial '.rbenv/bin'
}

@test 'agent-tools の PATH は残る' {
    run zsh_probe 'print -l $path'
    assert_output --partial '.agent-tools/bin'
}

@test 'rancher-desktop の PATH は残る' {
    run zsh_probe 'print -l $path'
    assert_output --partial '.rd/bin'
}

@test '対話起動が余計なエラーを出さない' {
    run zsh_probe 'exit'
    refute_output --partial 'command not found'
    refute_output --partial 'no such file'
    refute_output --partial 'error'
}
