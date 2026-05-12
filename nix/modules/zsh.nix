# Home Manager で zsh、補完、プロンプト、プラグインを有効化する。
#
# `.zshrc` はリポジトリ管理のファイルへ置き換える。TAB は通常補完に残し、
# fzf-tab は Ctrl-X TAB に割り当てる前提を `rust/tests/checks/src/zsh.rs` で検証する。
{
  config,
  lib,
  pkgs,
  root,
  ...
}:
{
  home.file."./.zshrc" = {
    force = true;
    source = "${root}/.zshrc";
    target = ".zshrc";
  };

  programs.zsh = {
    enable = true;
    enableCompletion = true;
    autosuggestion.enable = false;
    syntaxHighlighting.enable = false;
    dotDir = config.home.homeDirectory;

    plugins = [
      {
        name = "powerlevel10k";
        src = pkgs.zsh-powerlevel10k;
        file = "share/zsh-powerlevel10k/powerlevel10k.zsh-theme";
      }
      {
        name = "fzf-tab";
        src = pkgs.zsh-fzf-tab;
        file = "share/fzf-tab/fzf-tab.plugin.zsh";
      }
      {
        name = "zsh-autosuggestions";
        src = pkgs.zsh-autosuggestions;
        file = "share/zsh-autosuggestions/zsh-autosuggestions.zsh";
      }
      {
        name = "fast-syntax-highlighting";
        src = pkgs.zsh-fast-syntax-highlighting;
        file = "share/zsh/plugins/fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh";
      }
    ];

    initContent = lib.mkMerge [
      (lib.mkOrder 550 ''
        fpath=(${pkgs.zsh-completions}/share/zsh/site-functions $fpath)
        [[ -f "$HOME/.config/zsh/env.zsh" ]] && source "$HOME/.config/zsh/env.zsh"
        [[ -f "$HOME/.config/zsh/options.zsh" ]] && source "$HOME/.config/zsh/options.zsh"
        [[ -f "$HOME/.config/zsh/history.zsh" ]] && source "$HOME/.config/zsh/history.zsh"
        [[ -f "$HOME/.config/zsh/aliases.zsh" ]] && source "$HOME/.config/zsh/aliases.zsh"
      '')

      ''
        [[ -f "$HOME/.config/zsh/completion.zsh" ]] && source "$HOME/.config/zsh/completion.zsh"
        [[ -f "$HOME/.config/zsh/prompt.zsh" ]] && source "$HOME/.config/zsh/prompt.zsh"
        [[ -f "$HOME/.config/zsh/local.zsh" ]] && source "$HOME/.config/zsh/local.zsh"
      ''
    ];
  };
}
