" from http://qiita.com/ryo2851/items/4e3c287d5a0005780034

"新しい行のインデントを現在行と同じにする
set autoindent
"Vi互換をオフ
set nocompatible
"タブの代わりに空白文字を挿入する
set expandtab
"変更中のファイルでも、保存しないで他のファイルを表示
set hidden
"インクリメンタルサーチを行う
set incsearch
"listで表示される文字のフォーマットを指定する
set listchars=eol:$,tab:>\ ,extends:<
"行番号を表示する
set number
"シフト移動幅
set shiftwidth=2
"閉じ括弧が入力されたとき、対応する括弧を表示する
set showmatch
"検索時に大文字を含んでいたら大/小を区別
set smartcase
"新しい行を作ったときに高度な自動インデントを行う
set smartindent
"行頭の余白内で Tab を打ち込むと、'shiftwidth' の数だけインデントする。
set smarttab
"ファイル内の <Tab> が対応する空白の数
set tabstop=4
"カーソルを行頭、行末で止まらないようにする
set whichwrap=b,s,h,l,<,>,[,]
"検索をファイルの先頭へループしない
set nowrapscan
" スクロール
set scrolloff=5
" カーソルライン
set cursorline
" terminal モードから esc で脱出する
tnoremap <silent> jj <C-\><C-n>

set ambiwidth=double

" insertモードから抜ける
"inoremap <silent> jj <ESC>
"inoremap <silent> <C-j> j
"inoremap <silent> kk <ESC>
"inoremap <silent> <C-k> k

" ノーマルモード時だけ ; と : を入れ替える
nnoremap ; :
nnoremap : ;

filetype indent on
set tabstop=2
set shiftwidth=2

set fileencodings=utf-8,ucs-bom,iso-2022-jp-3,iso-2022-jp,eucjp-ms,euc-jisx0213,euc-jp,sjis,cp932
set encoding=utf-8
set fenc=utf-8

" init python3 path
" let g:python3_host_prog = expand('/usr/bin/python3')

filetype plugin indent on

" プラグインがインストールされるディレクトリ
let s:dein_dir = expand('~/.cache/dein')
" dein.vim 本体
let s:dein_repo_dir = s:dein_dir . '/repos/github.com/Shougo/dein.vim'

" dein.vim がなければ github から落としてくる
if &runtimepath !~# '/dein.vim'
  if !isdirectory(s:dein_repo_dir)
    execute '!git clone https://github.com/Shougo/dein.vim' s:dein_repo_dir
  endif
  execute 'set runtimepath^=' . fnamemodify(s:dein_repo_dir, ':p')
endif

" 設定開始
if dein#load_state(s:dein_dir)
  call dein#begin(s:dein_dir)

  " プラグインリストを収めた TOML ファイル
  " 予め TOML ファイルを用意しておく
  let g:rc_dir    = expand("~/.config/nvim/")
  let s:toml      = g:rc_dir . '/dein.toml'
  let s:lazy_toml = g:rc_dir . '/dein_lazy.toml'

  " TOML を読み込み、キャッシュしておく
  call dein#load_toml(s:toml,      {'lazy': 0})
  call dein#load_toml(s:lazy_toml, {'lazy': 1})
  
  let g:opamshare = substitute(system('opam config var share'),'\n$','','''')
  call dein#local(g:opamshare . '/merlin/')

  " 設定終了
  call dein#end()
  call dein#save_state()
endif

" もし、未インストールものものがあったらインストール
if dein#check_install()
  call dein#install()
endif

" for slim
autocmd BufRead,BufNewFile *.slim setfiletype slim

" for css, scss
autocmd FileType css,sass,scss set iskeyword+=-
let g:sass_compile_auto = 0


" for coffee
autocmd BufRead,BufNewFile *.coffee setfiletype coffee
" インデント設
autocmd FileType coffee setlocal sw=2 sts=2 ts=2 et

let g:python3_host_prog=$PYENV_ROOT.'/versions/3.6.5/bin/python'

" for hybrid
set background=dark
colorscheme hybrid

set backupcopy=yes
