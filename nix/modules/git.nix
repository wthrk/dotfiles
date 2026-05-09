# 管理対象ユーザーに適用する Git の既定設定。
#
# GitHub 認証は gh の credential helper へ委譲し、alias と ignore は全マシンで共有できる値だけを
# 置く。リポジトリ固有の署名鍵や例外設定は、この Home Manager モジュールでは扱わない。
{
  programs.git = {
    enable = true;

    signing.format = "openpgp";

    settings = {
      alias.graph = "log --graph --date-order -C -M --pretty=format:\"<%h> %ad [%an] %Cgreen%d%Creset %s\" --all --date=short";
      init.defaultBranch = "main";
      credential = {
        "https://github.com".helper = [
          ""
          "!gh auth git-credential"
        ];
        "https://gist.github.com".helper = [
          ""
          "!gh auth git-credential"
        ];
      };
    };
  };
}
