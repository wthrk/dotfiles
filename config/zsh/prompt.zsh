# Powerlevel10k を読み込み、リポジトリ管理の表示設定に接続する。
#
# 表示の詳細は生成済みの p10k.zsh に閉じ込め、通常の zsh 設定と混ぜない。
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -t 1 ]] && [[ -r "$HOME/.config/zsh/p10k.zsh" ]]; then
  source "$HOME/.config/zsh/p10k.zsh"
fi
