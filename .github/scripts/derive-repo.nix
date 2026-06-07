# 宣言パッケージから GitHub `owner/repo` と changelog URL を評価時属性だけで導出する純関数群。
#
# nightly の nix 版差分（eval ベース）で、各パッケージの当該版リリースノート取得元を決めるための値抽出規則を
# 単一正本として固定する。`eval-declared-versions.sh` がこのファイルを import して `home.packages` /
# `environment.systemPackages` の各パッケージへ適用し、checks 側のテスト（`nix eval --expr`）が同じ関数を
# fixture で 4 分岐固定する（script とテストで規則がドリフトしないよう実装を 1 箇所に集約する）。
#
# repo（owner/repo）導出の優先: ①`meta.homepage` が github ならそこから ②無ければ `src`（owner+repo 直接、
# 無ければ url/urls の github URL）から ③無ければ `meta.changelog` の github URL から。github 由来が取れなければ
# 空文字（version-only 行き）。すべて評価時 meta/src 属性の参照のみで、ビルド/フェッチは走らない。
let
  # github URL 文字列から "owner/repo" を取り出す（取れなければ ""）。末尾 .git・クエリ/フラグメントは除く。
  fromUrl =
    url:
    let
      m = builtins.match "https?://github\\.com/([^/]+)/([^/?#]+).*" url;
    in
    if m == null then
      ""
    else
      let
        owner = builtins.elemAt m 0;
        repoRaw = builtins.elemAt m 1;
        repo =
          let
            g = builtins.match "(.+)\\.git" repoRaw;
          in
          if g == null then repoRaw else builtins.elemAt g 0;
      in
      if owner == "" || repo == "" then "" else owner + "/" + repo;

  # 値が文字列ならそれを、そうでなければ "" を返す（安全な文字列化）。
  asStr = v: if builtins.isString v then v else "";

  # src（fetchFromGitHub 等）から owner/repo を取り出す。owner+repo 直接指定を最優先、無ければ url/urls。
  fromSrc =
    p:
    let
      src = p.src or null;
      owner = asStr (src.owner or "");
      repo = asStr (src.repo or "");
      url = asStr (src.url or "");
      urls = src.urls or [ ];
      firstUrl = if builtins.isList urls && urls != [ ] then asStr (builtins.head urls) else "";
    in
    if src == null then
      ""
    else if owner != "" && repo != "" then
      owner + "/" + repo
    else if url != "" && fromUrl url != "" then
      fromUrl url
    else if firstUrl != "" then
      fromUrl firstUrl
    else
      "";

  changelogOf = p: asStr (p.meta.changelog or p.meta.homepage or "");
  homepageOf = p: asStr (p.meta.homepage or "");
  changelogUrlOf = p: asStr (p.meta.changelog or "");

  # repo 導出: homepage(github) → src → changelog(github) の優先。
  repoOf =
    p:
    let
      h = fromUrl (homepageOf p);
      s = if h != "" then h else fromSrc p;
    in
    if s != "" then s else fromUrl (changelogUrlOf p);
in
{
  inherit
    fromUrl
    asStr
    fromSrc
    changelogOf
    repoOf
    ;
}
