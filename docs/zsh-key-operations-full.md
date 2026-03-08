# Zsh Key Operations (Full)

Generated: 2026-03-08 13:42:50 JST

## Installed Features
### Plugins
- romkatv/powerlevel10k
- zsh-users/zsh-autosuggestions
- zdharma-continuum/fast-syntax-highlighting

### Tools
- zsh: /usr/local/bin/zsh
- fzf: /opt/homebrew/bin/fzf
- atuin: /opt/homebrew/bin/atuin
- zoxide: /opt/homebrew/bin/zoxide
- git: /opt/homebrew/bin/git
- rg: /Users/ya/.nodebrew/node/v22.21.1/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/path/rg
- fd: (missing)
- nvim: /opt/homebrew/bin/nvim

## Expected Feature Operations (including currently disabled)
| Feature | Keymap | Key | Expected Operation | Current Binding | Status |
|---|---|---|---|---|---|
| custom-history | viins | `^P` | `history-beginning-search-backward-end` | `"^P" history-beginning-search-backward-end` | PASS |
| custom-history | viins | `^N` | `history-beginning-search-forward-end` | `"^N" history-beginning-search-forward-end` | PASS |
| custom-history | viins | `\ep` | `history-beginning-search-backward-end` | `"^[p" history-beginning-search-backward-end` | PASS |
| custom-history | viins | `\en` | `history-beginning-search-forward-end` | `"^[n" history-beginning-search-forward-end` | PASS |
| completion-base | emacs | `^I` | `expand-or-complete` | `"^I" expand-or-complete` | PASS |
| completion-base | viins | `^I` | `expand-or-complete` | `"^I" expand-or-complete` | PASS |
| completion-base | vicmd | `^I` | `expand-or-complete` | `"^I" undefined-key` | FAIL |
| fzf | emacs | `^T` | `fzf-file-widget` | `"^T" transpose-chars` | FAIL |
| fzf | viins | `^T` | `fzf-file-widget` | `"^T" self-insert` | FAIL |
| fzf | vicmd | `^T` | `fzf-file-widget` | `"^T" undefined-key` | FAIL |
| fzf | emacs | `\ec` | `fzf-cd-widget` | `"^[c" capitalize-word` | FAIL |
| fzf | viins | `\ec` | `fzf-cd-widget` | `"^[c" undefined-key` | FAIL |
| fzf | vicmd | `\ec` | `fzf-cd-widget` | `"^[c" undefined-key` | FAIL |
| atuin | emacs | `^R` | `atuin-search` | `"^R" atuin-search` | PASS |
| atuin | viins | `^R` | `atuin-search-viins` | `"^R" atuin-search-viins` | PASS |
| atuin | vicmd | `/` | `atuin-search` | `"/" atuin-search` | PASS |
| fzf-tab(expected) | emacs | `^I` | `fzf-tab-complete` | `"^I" expand-or-complete` | FAIL |
| fzf-tab(expected) | viins | `^I` | `fzf-tab-complete` | `"^I" expand-or-complete` | FAIL |
| fzf-tab(expected) | vicmd | `^I` | `fzf-tab-complete` | `"^I" undefined-key` | FAIL |
| zsh-completions(expected) | contextual | Tab on command args | extra completion function available (e.g. `_certbot`) | `_certbot: function` | FAIL |
| autosuggestions | widget-config | accept widgets | forward-char/end-of-line family | `typeset -a ZSH_AUTOSUGGEST_ACCEPT_WIDGETS=( forward-char end-of-line vi-forward-char vi-end-of-line vi-add-eol )` | INFO |
| autosuggestions | widget-config | partial accept widgets | forward-word family | `typeset -a ZSH_AUTOSUGGEST_PARTIAL_ACCEPT_WIDGETS=( forward-word emacs-forward-word vi-forward-word vi-forward-word-end vi-forward-blank-word vi-forward-blank-word-end vi-find-next-char vi-find-next-char-skip )` | INFO |
| zoxide | command | `z <query>` | jump directory | `z: function` | INFO |
| zoxide | command | `zi <query>` | interactive jump | `zi: function` | INFO |
| zoxide | completion | Tab with `z` | completion helper exists | `__zoxide_z_complete: none` | INFO |

## Full Runtime Keymap Inventory (All Key Operations)

### .safe
```text
"^@"-"^I" .self-insert
"^J" .accept-line
"^K"-"^L" .self-insert
"^M" .accept-line
"^N"-"\M-^?" .self-insert
```

### command
```text
"^G" send-break
"^J" accept-line
"^M" accept-line
```

### emacs
```text
"^@" set-mark-command
"^A" beginning-of-line
"^B" backward-char
"^D" delete-char-or-list
"^E" end-of-line
"^F" forward-char
"^G" send-break
"^H" backward-delete-char
"^I" expand-or-complete
"^J" accept-line
"^K" kill-line
"^L" clear-screen
"^M" accept-line
"^N" down-line-or-history
"^O" accept-line-and-down-history
"^P" up-line-or-history
"^Q" push-line
"^R" atuin-search
"^S" history-incremental-search-forward
"^T" transpose-chars
"^U" kill-whole-line
"^V" quoted-insert
"^W" backward-kill-word
"^X^B" vi-match-bracket
"^X^F" vi-find-next-char
"^X^J" vi-join
"^X^K" kill-buffer
"^X^N" infer-next-history
"^X^O" overwrite-mode
"^X^U" undo
"^X^V" vi-cmd-mode
"^X^X" exchange-point-and-mark
"^X*" expand-word
"^X=" what-cursor-position
"^XG" list-expand
"^Xg" list-expand
"^Xr" history-incremental-search-backward
"^Xs" history-incremental-search-forward
"^Xu" undo
"^Y" yank
"^[^D" list-choices
"^[^G" send-break
"^[^H" backward-kill-word
"^[^I" self-insert-unmeta
"^[^J" self-insert-unmeta
"^[^L" clear-screen
"^[^M" self-insert-unmeta
"^[^_" copy-prev-word
"^[ " expand-history
"^[!" expand-history
"^[\"" quote-region
"^[\$" spell-word
"^['" quote-line
"^[-" neg-argument
"^[." insert-last-word
"^[0" digit-argument
"^[1" digit-argument
"^[2" digit-argument
"^[3" digit-argument
"^[4" digit-argument
"^[5" digit-argument
"^[6" digit-argument
"^[7" digit-argument
"^[8" digit-argument
"^[9" digit-argument
"^[<" beginning-of-buffer-or-history
"^[>" end-of-buffer-or-history
"^[?" which-command
"^[A" accept-and-hold
"^[B" backward-word
"^[C" capitalize-word
"^[D" kill-word
"^[F" forward-word
"^[G" get-line
"^[H" run-help
"^[L" down-case-word
"^[N" history-search-forward
"^[OA" up-line-or-history
"^[OB" down-line-or-history
"^[OC" forward-char
"^[OD" backward-char
"^[P" history-search-backward
"^[Q" push-line
"^[S" spell-word
"^[T" transpose-words
"^[U" up-case-word
"^[W" copy-region-as-kill
"^[[200~" bracketed-paste
"^[[A" up-line-or-history
"^[[B" down-line-or-history
"^[[C" forward-char
"^[[D" backward-char
"^[_" insert-last-word
"^[a" accept-and-hold
"^[b" backward-word
"^[c" capitalize-word
"^[d" kill-word
"^[f" forward-word
"^[g" get-line
"^[h" run-help
"^[l" down-case-word
"^[n" history-search-forward
"^[p" history-search-backward
"^[q" push-line
"^[s" spell-word
"^[t" transpose-words
"^[u" up-case-word
"^[w" copy-region-as-kill
"^[x" execute-named-cmd
"^[y" yank-pop
"^[z" execute-last-named-cmd
"^[|" vi-goto-column
"^[^?" backward-kill-word
"^_" undo
" "-"~" self-insert
"^?" backward-delete-char
"\M-^@"-"\M-^?" self-insert
```

### isearch
```text
```

### main
```text
"^A"-"^C" self-insert
"^D" list-choices
"^E"-"^F" self-insert
"^G" list-expand
"^H" vi-backward-delete-char
"^I" expand-or-complete
"^J" accept-line
"^K" self-insert
"^L" clear-screen
"^M" accept-line
"^N" history-beginning-search-forward-end
"^O" self-insert
"^P" history-beginning-search-backward-end
"^Q" vi-quoted-insert
"^R" atuin-search-viins
"^S"-"^T" self-insert
"^U" vi-kill-line
"^V" vi-quoted-insert
"^W" vi-backward-kill-word
"^X^R" _read_comp
"^X?" _complete_debug
"^XC" _correct_filename
"^Xa" _expand_alias
"^Xc" _correct_word
"^Xd" _list_expansions
"^Xe" _expand_word
"^Xh" _complete_help
"^Xm" _most_recent_file
"^Xn" _next_tags
"^Xt" _complete_tag
"^X~" _bash_list-choices
"^Y"-"^Z" self-insert
"^[" vi-cmd-mode
"^[," _history-complete-newer
"^[/" _history-complete-older
"^[OA" up-line-or-history
"^[OB" down-line-or-history
"^[OC" vi-forward-char
"^[OD" vi-backward-char
"^[[200~" bracketed-paste
"^[[A" up-line-or-history
"^[[B" down-line-or-history
"^[[C" vi-forward-char
"^[[D" vi-backward-char
"^[n" history-beginning-search-forward-end
"^[p" history-beginning-search-backward-end
"^[~" _bash_complete-word
"^\\\\"-"~" self-insert
"^?" vi-backward-delete-char
"\M-^@"-"\M-^?" self-insert
```

### vicmd
```text
"^D" list-choices
"^G" list-expand
"^H" vi-backward-char
"^J" accept-line
"^L" clear-screen
"^M" accept-line
"^N" down-history
"^P" up-history
"^R" redo
"^[" beep
"^[OA" up-line-or-history
"^[OB" down-line-or-history
"^[OC" vi-forward-char
"^[OD" vi-backward-char
"^[[200~" bracketed-paste
"^[[A" up-line-or-history
"^[[B" down-line-or-history
"^[[C" vi-forward-char
"^[[D" vi-backward-char
" " vi-forward-char
"\"" vi-set-buffer
"#" pound-insert
"\$" vi-end-of-line
"%" vi-match-bracket
"'" vi-goto-mark-line
"+" vi-down-line-or-history
"," vi-rev-repeat-find
"-" vi-up-line-or-history
"." vi-repeat-change
"/" atuin-search
"0" vi-digit-or-beginning-of-line
"1"-"9" digit-argument
":" execute-named-cmd
";" vi-repeat-find
"<" vi-unindent
"=" list-choices
">" vi-indent
"?" vi-history-search-forward
"A" vi-add-eol
"B" vi-backward-blank-word
"C" vi-change-eol
"D" vi-kill-eol
"E" vi-forward-blank-word-end
"F" vi-find-prev-char
"G" vi-fetch-history
"I" vi-insert-bol
"J" vi-join
"N" vi-rev-repeat-search
"O" vi-open-line-above
"P" vi-put-before
"R" vi-replace
"S" vi-change-whole-line
"T" vi-find-prev-char-skip
"V" visual-line-mode
"W" vi-forward-blank-word
"X" vi-backward-delete-char
"Y" vi-yank-whole-line
"\^" vi-first-non-blank
"\`" vi-goto-mark
"a" vi-add-next
"b" vi-backward-word
"c" vi-change
"d" vi-delete
"e" vi-forward-word-end
"f" vi-find-next-char
"gE" vi-backward-blank-word-end
"gU" vi-up-case
"gUU" "gUgU"
"ga" what-cursor-position
"ge" vi-backward-word-end
"gg" beginning-of-buffer-or-history
"gu" vi-down-case
"guu" "gugu"
"g~" vi-oper-swap-case
"g~~" "g~g~"
"h" vi-backward-char
"i" vi-insert
"j" down-line-or-history
"k" up-line-or-history
"l" vi-forward-char
"m" vi-set-mark
"n" vi-repeat-search
"o" vi-open-line-below
"p" vi-put-after
"r" vi-replace-chars
"s" vi-substitute
"t" vi-find-next-char-skip
"u" undo
"v" visual-mode
"w" vi-forward-word
"x" vi-delete-char
"y" vi-yank
"|" vi-goto-column
"~" vi-swap-case
"^?" vi-backward-char
```

### viins
```text
"^A"-"^C" self-insert
"^D" list-choices
"^E"-"^F" self-insert
"^G" list-expand
"^H" vi-backward-delete-char
"^I" expand-or-complete
"^J" accept-line
"^K" self-insert
"^L" clear-screen
"^M" accept-line
"^N" history-beginning-search-forward-end
"^O" self-insert
"^P" history-beginning-search-backward-end
"^Q" vi-quoted-insert
"^R" atuin-search-viins
"^S"-"^T" self-insert
"^U" vi-kill-line
"^V" vi-quoted-insert
"^W" vi-backward-kill-word
"^X^R" _read_comp
"^X?" _complete_debug
"^XC" _correct_filename
"^Xa" _expand_alias
"^Xc" _correct_word
"^Xd" _list_expansions
"^Xe" _expand_word
"^Xh" _complete_help
"^Xm" _most_recent_file
"^Xn" _next_tags
"^Xt" _complete_tag
"^X~" _bash_list-choices
"^Y"-"^Z" self-insert
"^[" vi-cmd-mode
"^[," _history-complete-newer
"^[/" _history-complete-older
"^[OA" up-line-or-history
"^[OB" down-line-or-history
"^[OC" vi-forward-char
"^[OD" vi-backward-char
"^[[200~" bracketed-paste
"^[[A" up-line-or-history
"^[[B" down-line-or-history
"^[[C" vi-forward-char
"^[[D" vi-backward-char
"^[n" history-beginning-search-forward-end
"^[p" history-beginning-search-backward-end
"^[~" _bash_complete-word
"^\\\\"-"~" self-insert
"^?" vi-backward-delete-char
"\M-^@"-"\M-^?" self-insert
```

### viopp
```text
"^[" vi-cmd-mode
"^[OA" up-line
"^[OB" down-line
"^[[A" up-line
"^[[B" down-line
"aW" select-a-blank-word
"aa" select-a-shell-word
"aw" select-a-word
"iW" select-in-blank-word
"ia" select-in-shell-word
"iw" select-in-word
"j" down-line
"k" up-line
```

### visual
```text
"^[" deactivate-region
"^[OA" up-line
"^[OB" down-line
"^[[A" up-line
"^[[B" down-line
"U" vi-up-case
"aW" select-a-blank-word
"aa" select-a-shell-word
"aw" select-a-word
"iW" select-in-blank-word
"ia" select-in-shell-word
"iw" select-in-word
"j" down-line
"k" up-line
"o" exchange-point-and-mark
"p" put-replace-selection
"u" vi-down-case
"x" vi-delete
"~" vi-oper-swap-case
```
