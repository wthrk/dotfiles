#!/usr/bin/env bats
#
# 構成を適用済みのマシンで zsh を起動し、その環境を検証する。適用は検証の外側で済ませる。
# 起動を `script(1)` 経由にするのは、制御端末が無いと compinit と zle が初期化されないため。

setup() {
    bats_load_library bats-support
    bats_load_library bats-assert
}

setup_file() {
    ZSH_CHECK_USER="$(id -un)"

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

    export ZSH_CHECK_USER
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
        raw="$(zsh_probe_env script -qec "zsh -ic 'source ${code_file}'" /dev/null)"
    else
        raw="$(zsh_probe_env script -q /dev/null zsh -ic "source ${code_file}")"
    fi
    strip_terminal_control "${raw}"
}

# 実機のログイン環境に相当する最小の変数だけを渡して起動する。
zsh_probe_env() {
    env -i \
        HOME="${HOME}" \
        USER="${ZSH_CHECK_USER}" \
        LOGNAME="${ZSH_CHECK_USER}" \
        SHELL="${HOME}/.nix-profile/bin/zsh" \
        TERM=xterm-256color \
        LANG=en_US.UTF-8 \
        PATH="$(zsh_probe_path)" \
        "$@"
}

# 起動前から PATH にあった要素。`ZSH_CHECK_INHERITED_PATH` は除外規則の検証だけが使う。
zsh_probe_path() {
    printf '%s' \
        "${ZSH_CHECK_INHERITED_PATH:+${ZSH_CHECK_INHERITED_PATH}:}${HOME}/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
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
