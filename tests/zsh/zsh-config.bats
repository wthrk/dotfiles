#!/usr/bin/env bats
#
# Home Manager が生成した zsh 設定を、実ホームを触らずに起動して検証する。
#
# 検証対象は `flake.nix` の `homeConfigurations.ci-ref` を build した activation package が置く
# `.zshrc` / `.zshenv` / `.config/zsh` である。`setup_file` で activation package を 1 回だけ build し、
# 生成時に埋め込まれたホームパスを一時ホームへ置換したうえで、各テストがその一時ホームで対話 zsh を
# 起動する。観測するのは補完ウィジェット、キーバインド、PATH、起動時出力だけであり、利用者の実
# `$HOME` と実 `~/.config/dotfiles` には書き込まない。
#
# zsh の起動は `script(1)` 経由にする。制御端末が無いと compinit と zle が初期化されず、`zle -la` や
# `bindkey` で観測したい状態がそもそも作られないため、TTY を用意しない検証は退行を検出できない。

# 検証に使う CI 参照 Home Manager 構成。実利用者名・実ホストに依存しない固定構成である。
CI_REFERENCE='homeConfigurations.ci-ref'

# assertion library は各テストのシェルで読み込む（`setup_file` の関数定義はテストへ引き継がれない）。
setup() {
    bats_load_library bats-support
    bats_load_library bats-assert
}

# activation package の build と一時ホーム構築はファイル単位で 1 回だけ行う。
#
# ここを各テストで繰り返すと、build と Home Manager の評価を検証項目の数だけ払うことになる。
# テストへ引き継げるのは export した変数だけなので、後続が使う値は export する。
setup_file() {
    ZSH_CHECK_REPO="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"

    # `--no-update-lock-file` を付ける。検証が repository の `flake.lock` を書き換えないようにするため、
    # lock 不足は暗黙更新ではなく失敗として扱う。
    ZSH_CHECK_USER="$(nix eval --raw --no-update-lock-file \
        "${ZSH_CHECK_REPO}#${CI_REFERENCE}.config.home.username")"
    local generated_home
    generated_home="$(nix eval --raw --no-update-lock-file \
        "${ZSH_CHECK_REPO}#${CI_REFERENCE}.config.home.homeDirectory")"
    local activation_package
    activation_package="$(nix build --no-link --print-out-paths --no-update-lock-file \
        "${ZSH_CHECK_REPO}#${CI_REFERENCE}.activationPackage")"

    local home_files="${activation_package}/home-files"
    if [ ! -f "${home_files}/.zshrc" ]; then
        echo "activation package に ${home_files}/.zshrc が無い" >&2
        return 1
    fi

    # `home.packages` の実体。activation package の `home-path` は Home Manager が
    # `home.packages` から作る profile へのリンクであり、その `bin` が利用者環境で
    # `$HOME/.nix-profile/bin` として PATH に載る（`nix/darwin.nix` の PATH 構成と同じ位置づけ）。
    local home_path="${activation_package}/home-path"
    if [ ! -d "${home_path}/bin" ]; then
        echo "activation package に ${home_path}/bin が無い" >&2
        return 1
    fi

    # 一時ホームは bats が run 終了後に破棄する `BATS_FILE_TMPDIR` 配下に置く。
    ZSH_CHECK_HOME="${BATS_FILE_TMPDIR}/home"
    mkdir -p \
        "${ZSH_CHECK_HOME}/.config" \
        "${ZSH_CHECK_HOME}/.local/share" \
        "${ZSH_CHECK_HOME}/.local/state" \
        "${ZSH_CHECK_HOME}/.cache"
    # zsh 起動が読む設定だけを link する。`.config` 自体を store への link にすると、起動時に
    # `$HOME/.config` 配下へ書くツールが read-only failure を起こし、起動時出力の検証と混ざる。
    ln -s "${home_files}/.config/zsh" "${ZSH_CHECK_HOME}/.config/zsh"
    ln -s "${home_files}/.config/git" "${ZSH_CHECK_HOME}/.config/git"
    ln -s "${home_files}/.config/direnv" "${ZSH_CHECK_HOME}/.config/direnv"
    ln -s "${home_files}/.zsh" "${ZSH_CHECK_HOME}/.zsh"
    # 利用者環境で `home.packages` が置かれる `$HOME/.nix-profile` を一時ホームにも作る。
    ln -s "${home_path}" "${ZSH_CHECK_HOME}/.nix-profile"
    rewrite_generated_home "${home_files}/.zshrc" "${ZSH_CHECK_HOME}/.zshrc" "${generated_home}"
    rewrite_generated_home "${home_files}/.zshenv" "${ZSH_CHECK_HOME}/.zshenv" "${generated_home}"

    ZSH_CHECK_PACKAGE_BIN="${ZSH_CHECK_HOME}/.nix-profile/bin"

    export ZSH_CHECK_REPO ZSH_CHECK_USER ZSH_CHECK_HOME ZSH_CHECK_PACKAGE_BIN
}

# Home Manager が埋め込んだ生成時ホームパスを一時ホームへ置換してから配置する。
#
# 置換しないと `.zshrc` が実在しない `/Users/<ci 参照利用者>` を読みに行き、検証対象の設定が
# 読み込まれないまま起動が成功してしまう。
rewrite_generated_home() {
    local source="$1" destination="$2" generated_home="$3" content
    content="$(<"${source}")"
    printf '%s\n' "${content//${generated_home}/${ZSH_CHECK_HOME}}" >"${destination}"
}

# 一時ホームで対話 zsh を起動し、渡した zsh コードの出力を制御文字除去済みで返す。
#
# 終了 status は判定に使わない。`zle -la` の絞り込みなど、観測対象が存在しないときに非 0 で終わる
# コードも probe として渡すため、判定は出力側で行う。呼び出し側は `run zsh_probe ...` で使う。
#
# `home.packages` の bin を PATH の先頭へ載せる。利用者環境では載っており、`config/zsh` の
# 外部ツール連携は `(( $+commands[...] ))` で guard されている。載せないと atuin / zoxide の
# init、fzf の key-bindings、eza / nvim の alias 定義といった guard 済み分岐が丸ごと無言で
# skip され、その退行を検出できないまま検査が通る。実環境と同じ解決結果にして guard の
# 内側まで実行させ、被覆を失わせないために載せる。
#
# ホーム位置を決める変数も一時ホーム配下へ固定する。`config/zsh/env.zsh` の `XDG_CACHE_HOME`
# / `XDG_DATA_HOME` は `${VAR:-...}` 既定値なので、固定しないと呼び出し元シェルが export した
# 実ホームの値が勝ち、`config/zsh/completion.zsh` の compdump、plugin のキャッシュ、上で PATH に
# 載せた外部ツールの data ディレクトリを利用者の実ホームへ書く。`ZDOTDIR` は zsh が起動ファイルを
# 読む位置そのもの、`XDG_STATE_HOME` / `XDG_CONFIG_HOME` も継承値が実ホームを指しうるため、
# まとめて一時ホームへ閉じ込める。
zsh_probe() {
    local raw
    raw="$(env \
        PATH="${ZSH_CHECK_PACKAGE_BIN}:${PATH}" \
        HOME="${ZSH_CHECK_HOME}" \
        ZDOTDIR="${ZSH_CHECK_HOME}" \
        XDG_CONFIG_HOME="${ZSH_CHECK_HOME}/.config" \
        XDG_CACHE_HOME="${ZSH_CHECK_HOME}/.cache" \
        XDG_DATA_HOME="${ZSH_CHECK_HOME}/.local/share" \
        XDG_STATE_HOME="${ZSH_CHECK_HOME}/.local/state" \
        USER="${ZSH_CHECK_USER}" \
        LOGNAME="${ZSH_CHECK_USER}" \
        POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true \
        script -q /dev/null zsh -ic "$1")"
    strip_terminal_control "${raw}"
}

# `script(1)` が pty 出力へ混ぜる制御文字と前後の空白を落とし、出力を安定比較できる形にする。
strip_terminal_control() {
    local text="$1"
    text="${text//$'\r'/}"
    text="${text//$'\x04'$'\b'$'\b'/}"
    text="${text//'^D'$'\b'$'\b'/}"
    text="${text#"${text%%[![:space:]]*}"}"
    text="${text%"${text##*[![:space:]]}"}"
    printf '%s\n' "${text}"
}

# fzf-tab が読み込まれていないと、TAB 補完が黙って素の zsh 挙動へ戻る。
@test 'fzf-tab の widget が登録されている' {
    run zsh_probe 'zle -la'
    assert_line 'fzf-tab-complete'
}

# キー割り当てが参照する widget なので、autosuggestions の読み込み自体を固定する。
@test 'autosuggestions の widget が登録されている' {
    run zsh_probe 'zle -la'
    assert_line 'autosuggest-accept'
}

# syntax highlighting はプラグインの store パスではなく、定義された関数の有無で見る。
# store パスを見ると Home Manager の出力が動くたびに検査が壊れ、挙動と無関係な失敗になる。
@test 'fast-syntax-highlighting が読み込まれている' {
    run zsh_probe '(( ${+functions[fast-theme]} && ${+functions[_zsh_highlight]} )) && print loaded'
    assert_output 'loaded'
}

# TAB は全 keymap で通常補完のままにする。fzf-tab は意図的に Ctrl-X TAB 側へ割り当てる。
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

# 言語環境は Nix / Home Manager 管理へ寄せるため、旧 language-manager の shim は PATH から落とす。
#
# 除外は「継承した PATH に混ざった shim も落とす」ことを目的にしている（別ホーム配下の
# `/Users/runner/.cargo/bin` などが Nix 管理ツールより先に来ると同じ事故になる）。継承 PATH に
# たまたま shim があるかどうかへ結果を委ねると検査が空振りするため、ここで明示的に混ぜる。
@test '旧 language-manager の shim は継承 PATH から除外される' {
    local legacy_home="${BATS_TEST_TMPDIR}/legacy"
    local legacy_path="${legacy_home}/.nodebrew/current/bin:${legacy_home}/.bun/bin"
    legacy_path="${legacy_path}:${legacy_home}/.cargo/bin:${legacy_home}/.pyenv/bin"
    legacy_path="${legacy_path}:${legacy_home}/.rbenv/bin"

    PATH="${legacy_path}:${PATH}" run zsh_probe 'print -l $path'

    # 起動に失敗して出力が空でも不在検査は通ってしまうため、先に PATH を組み立てた痕跡を確かめる。
    assert_output --partial '.agent-tools/bin'
    refute_output --partial '.nodebrew/current/bin'
    refute_output --partial '.bun/bin'
    refute_output --partial '.cargo/bin'
    refute_output --partial '.pyenv/bin'
    refute_output --partial '.rbenv/bin'
}

# これらの利用者ローカルなツールパスは意図的に残す。除外規則の巻き添えで消えていないことを見る。
@test 'agent-tools の PATH は残る' {
    run zsh_probe 'print -l $path'
    assert_output --partial '.agent-tools/bin'
}

@test 'rancher-desktop の PATH は残る' {
    run zsh_probe 'print -l $path'
    assert_output --partial '.rd/bin'
}

# 起動時に典型的なシェルエラーを出さないことは zsh モジュールが利用者へ約束している契約である。
@test '対話起動が余計なエラーを出さない' {
    run zsh_probe 'exit'
    refute_output --partial 'command not found'
    refute_output --partial 'no such file'
    refute_output --partial 'error'
}

# 検証構成が dotfiles CLI 自身を home.packages に含めると、この検証だけのために CLI の release build を
# 払うことになる。zsh 検証を Rust ビルドから切り離した構成を維持するための不変条件として固定する。
@test '検証用 Home Manager 構成は dotfiles CLI 自身を含めない' {
    run nix eval --raw --no-update-lock-file \
        "${ZSH_CHECK_REPO}#${CI_REFERENCE}.config.home.packages" \
        --apply 'ps: builtins.concatStringsSep "," (builtins.map (p: p.name or "") ps)'
    assert_success
    refute_output --partial 'dotfiles-cli-'
}
