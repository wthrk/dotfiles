#!/usr/bin/env bash
# 現在の zsh 設定が作るキーマップを全件出力し、補完やキーバインドの変更確認に使う。
#
# 設定ファイルは変更せず、`bindkey -l` と各 keymap の `bindkey -M` 結果だけを観測する。
set -euo pipefail

out_md="docs/zsh-keymaps-full.md"
out_tsv="docs/zsh-keymaps-full.tsv"
mkdir -p docs

map_list=$(POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true zsh -fc 'source ~/.zshrc >/dev/null 2>&1; bindkey -l')

echo "# Full Zsh Keymap Dump" > "$out_md"
echo >> "$out_md"
echo "Generated: $(date '+%F %T %Z')" >> "$out_md"
echo >> "$out_md"
echo "## Keymaps" >> "$out_md"
while IFS= read -r m; do
  [[ -z "$m" ]] && continue
  echo "- $m" >> "$out_md"
done <<< "$map_list"
echo >> "$out_md"

: > "$out_tsv"
while IFS= read -r m; do
  [[ -z "$m" ]] && continue
  echo "## $m" >> "$out_md"
  echo '```text' >> "$out_md"
  bindings=$(POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true zsh -fc "source ~/.zshrc >/dev/null 2>&1; bindkey -M '$m'")
  printf '%s\n' "$bindings" >> "$out_md"
  echo '```' >> "$out_md"
  echo >> "$out_md"

  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    printf '%s\t%s\n' "$m" "$line" >> "$out_tsv"
  done <<< "$bindings"
done <<< "$map_list"

echo "wrote $out_md"
echo "wrote $out_tsv"
