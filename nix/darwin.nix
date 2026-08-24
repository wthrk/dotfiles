# nix-darwin で管理するホスト設定の最上位モジュール。
#
# 引数 `user` は `system.primaryUser`、Home Manager の対象ユーザー、nix-homebrew の所有者に使う。
# 引数 `host` は `networking.hostName` に使う。`includeSelfPackage` は Home Manager 側へそのまま渡し、
# `home.packages` へ repo 自身の `dotfiles` CLI を含めるかを制御する。`root` は設定ファイルのリンク元、
# `inputs` と `homebrewTaps` は Home Manager / nix-homebrew 連携と pin 済み tap の生成に使う。
# 評価結果は Nix 設定、macOS defaults、Homebrew、launchd daemon、Home Manager をまとめた
# `darwinConfigurations.<host>` 用の構成になる。
{
  homebrewTaps,
  inputs,
  lib,
  root,
  user,
  host,
  includeSelfPackage ? true,
  pkgs,
  config,
  ...
}:
let
  # auto-update daemon は nightly bump 後に各マシンを repo pin へ無人収束させる薄い launchd service である。
  # launchd timer は 30 分ごとに起動し、root daemon から `dotfiles update` を呼ぶ。対象ユーザーは
  # 指定せず、このマシンでローカル flake を持つ全ユーザーの解決を CLI に委ねる。CLI 側が各ユーザーの
  # lock 更新と Home Manager をそのユーザー権限へ降格し、system 層は所有者の flake からだけ適用する。
  # マシンの自動更新はこの 1 個の daemon であり、スケジュールもログも 1 箇所に残る。
  autoUpdateLabel = "org.dotfiles.auto-update";

  homeDir = "/Users/${user}";

  # 退役した CapsLock→Ctrl 実装（login のたびに `hidutil` で HID mapping を掛け直していた user agent）。
  retiredKeyboardRemapLabel = "org.dotfiles.keyboard-remap";
  retiredKeyboardRemapPlist = "${homeDir}/Library/LaunchAgents/${retiredKeyboardRemapLabel}.plist";

  autoUpdateSystem = pkgs.stdenv.hostPlatform.system;
  # x86_64-darwin 等、flake の `packageSystems` に含まれない system では `packages.${system}` が存在しない。その
  # system で評価が落ちないよう、package が無ければ daemon を定義しない（degrade）。動的キー（`system` は変数）の
  # 存在判定は `builtins.hasAttr` で行う。
  hasAutoUpdatePackage = builtins.hasAttr autoUpdateSystem inputs.self.packages;

  # 絶対 store パスで CLI を指す（PATH 非依存。launchd の最小環境で確実に解決するため）。
  dotfilesBin = "${inputs.self.packages.${autoUpdateSystem}.default}/bin/dotfiles";

  # launchd の最小環境で、CLI が絶対パスを持たないコマンドを解決するための PATH。CLI 自身が引くのは
  # `sudo`（対象ユーザーへの降格）と `dscl`（ローカル flake を持つユーザーの列挙）で、`nix` / coreutils は
  # そこから起動する子プロセスが使う。`darwin-rebuild` / `home-manager` は `flake.nix` の `wrapProgram` が
  # `DOTFILES_DARWIN_REBUILD` / `DOTFILES_HOME_MANAGER` へ焼き込んだ絶対パスで解決するため、ここには要らない。
  autoUpdatePath = lib.concatStringsSep ":" [
    (lib.makeBinPath [
      config.nix.package
      pkgs.coreutils
    ])
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];

  # launchd timer が呼ぶ薄い wrapper。target と対象ユーザーは指定しない。root で `--user` 省略の
  # `dotfiles update` は、このマシンでローカル flake を持つ全ユーザーを対象にし、各ユーザーの
  # 適用範囲もそのユーザーの scope から決まる。ここへ利用者名や config-dir を埋め込むと、daemon が
  # 1 人だけを見る状態に戻る。`--host` はマシン単位の値で、`dotfiles init --host` で短縮 hostname と
  # 異なる出力名を使った環境でも `#<host>` を正しく参照する。
  #
  # `exec` は job を更新処理そのものへ置き換える。job の寿命が更新処理の寿命と一致することが、
  # launchd が同じ label の次の発火を落とす条件であり、`launchctl bootout` と `kickstart -k` が
  # 進行中の更新へ届く条件でもある。
  autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
    set -euo pipefail

    export PATH=${lib.escapeShellArg autoUpdatePath}

    exec ${dotfilesBin} update --host ${lib.escapeShellArg host}
  '';

  # plist が指す program のパス。`/etc` 配下の固定文字列で、世代が変わっても同じ値になる。
  #
  # nix-darwin の activation は daemon の plist に差分があると `launchctl unload` してから plist を
  # 置き直して load し直す。unload の対象は、その activation を起こした無人実行そのものである。
  # plist が世代ごとに変わる値を持つと、更新のたびにこの分岐へ入り、daemon は plist の設置と再 load、
  # `/run/current-system` の張り替えを残したまま自分ごと消える。2026-08-20 と 2026-08-23 の無人実行は
  # これで停止し、`/run/current-system` は 2026-08-19 の世代を指したまま、daemon は system domain から
  # 居なくなった。
  #
  # そのため plist には世代ごとに変わる値を載せない。CLI と PATH の store path は wrapper 本体の側へ
  # 集め、plist からはこの固定パスだけを指す。wrapper の実体は `environment.etc` が同じ activation の
  # `/etc` 節で置き直すので、次の発火から新しい世代の CLI が起動する。
  autoUpdateWrapperEtcTarget = "dotfiles/auto-update";
  autoUpdateProgram = "/etc/${autoUpdateWrapperEtcTarget}";

  # 稼働中の daemon が読む plist の設置先。上流の launchd 節と、その前に差分を解消する `/etc` 節が
  # 同じ場所を置き直す。
  autoUpdateInstalledPlist = "/Library/LaunchDaemons/${autoUpdateLabel}.plist";

in
{
  imports = [
    ./modules/macos.nix
    ./modules/homebrew.nix
    ./modules/macos-defaults.nix
  ];

  # nix-darwin 側の launchd 宣言はこの daemon だけに保ち、`launchd.user.agents` は空にする。user agent が
  # 0 件だと上流の `userLaunchd`（`modules/system/launchd.nix`）が旧世代の user agent を撤去するループごと
  # 生成されないため、一度足した user agent は後で宣言を消しても適用済みマシンに残る。
  #
  # x86_64-darwin 等 `packageSystems` 外の system では `packages.${system}` が無いため daemon を定義しない。条件は
  # モジュールの **構造**（宣言する option 集合）ではなく option の **値** に `mkIf` で寄せる。`mkIf false` は遅延
  # 評価のため、package が無い system では内側の `autoUpdateWrapper`（=`dotfilesBin` の force）も評価されない。
  launchd.daemons.${autoUpdateLabel} = lib.mkIf hasAutoUpdatePackage {
    serviceConfig = {
      Label = autoUpdateLabel;
      ProgramArguments = [ autoUpdateProgram ];
      # nightly bump（CI の自動マージ）が入った pin へ、時刻を待たずに収束させるための発火間隔。
      # 適用済みの層へ適用をやり直させないのは `dotfiles update` 自身の責務で、この間隔はそれを前提に
      # 決める。`StartInterval` ではなく毎時 00 分と 30 分の 2 件にするのは、スリープ中に飛ばした発火を
      # launchd が起床時にまとめて 1 回起こすためである（`StartInterval` はスリープ中の発火を落とした
      # まま次の間隔まで待つ）。走っている間の発火は落ちるので、1 回の実行が次の発火に重なっても
      # 二重には起動しない。
      StartCalendarInterval = [
        { Minute = 0; }
        { Minute = 30; }
      ];
      # ブート直後の無人適用を避け、定期発火のみで動かす。
      RunAtLoad = false;
      StandardOutPath = "/var/log/${autoUpdateLabel}.out.log";
      StandardErrorPath = "/var/log/${autoUpdateLabel}.err.log";
    };
  };

  # daemon が起動する wrapper の実体。plist が持てない世代ごとの値をここへ置く。`mkIf` を
  # `environment.etc` の値へ寄せるのは `launchd.daemons` と同じ理由で、package が無い system では
  # 内側の `autoUpdateWrapper` も評価されないようにするためである。
  environment.etc = lib.mkIf hasAutoUpdatePackage {
    ${autoUpdateWrapperEtcTarget}.source = autoUpdateWrapper;
  };

  nix.settings = {
    experimental-features = [
      "nix-command"
      "flakes"
    ];
    trusted-users = [
      "root"
      user
    ];
  };

  nixpkgs.config.allowUnfree = true;

  networking.hostName = lib.mkDefault host;

  programs.zsh.enable = true;
  # 既定を止めるだけにせず置き直すのは、home 層を持たない別の利用者と root の補完がここでしか
  # 初期化されないため。
  programs.zsh.enableGlobalCompInit = false;
  programs.zsh.interactiveShellInit = "autoload -Uz compinit && compinit -i";

  # 旧 CapsLock→Ctrl 実装の退役。宣言を消すだけでは適用済みマシンから消えないため、activation で撤去する。
  # nix-darwin の `launchd.user.agents` で足した user agent は上流の退役ループが走らないので残り、`hidutil` の
  # `UserKeyMapping` は `system.keyboard.enableKeyMapping` を落としてもクリアされない。旧 plist が残っている
  # マシンでだけ走らせ、撤去済みのマシンで `UserKeyMapping` を触り続けないようにする。旧 agent が残る実マシンが
  # 無くなったらこの宣言ごと削除してよい。
  system.activationScripts.postActivation.text = lib.mkAfter ''
    uid="$(id -u ${lib.escapeShellArg user})"

    if [ -e ${lib.escapeShellArg retiredKeyboardRemapPlist} ]; then
      launchctl asuser "$uid" sudo --user=${lib.escapeShellArg user} -- launchctl bootout "gui/$uid/${retiredKeyboardRemapLabel}" >/dev/null 2>&1 || true
      sudo --user=${lib.escapeShellArg user} -- rm -f ${lib.escapeShellArg retiredKeyboardRemapPlist}
      hidutil property --set '{"UserKeyMapping":[]}' > /dev/null
    fi
  '';

  # 旧 system 層 GUI アプリの退役。宣言を `environment.systemPackages` から外すだけでは、既に
  # `/Applications/Nix Apps` へ実体コピーされた bundle が適用済みマシンに残る。上流が無条件に足す
  # App Management 検査（`modules/system/applications.nix`）はそのディレクトリを走査し、bundle が
  # 1 個でもあると非 Aqua セッションの auto-update daemon で activation を中断する。検査は同期
  # （`activationScripts.applications`）より先に合流するため、さらに前の preActivation で退避する。
  # 退避後は同じ適用内で上流の同期が空のディレクトリを作り直す。bundle が残っているマシンでだけ
  # 走らせ、退役済みのマシンで毎回やり直さないようにする。bundle が残る実マシンが無くなったら
  # この宣言ごと削除してよい。
  #
  # `rm -rf "$nixApps"` で直接消さず、退避してから消す。検査が走査するのは `/Applications/Nix Apps`
  # だけなので、bundle 内部の unlink をその走査対象の外へ先に移し、そのうえで実体を回収する。
  #
  # 退避先の親は root だけが書ける `/private/var/root` に置く。退避も削除も、失敗したら事実を stderr に
  # 残して activation は続ける。
  system.activationScripts.preActivation.text = lib.mkAfter ''
    nixApps='/Applications/Nix Apps'
    retired='/private/var/root/nix-apps-retired'

    for appBundle in "$nixApps"/*.app; do
      if [ -d "$appBundle" ]; then
        if mv "$nixApps" "$retired"; then
          rm -rf "$retired" || echo "Nix Apps: $retired の削除に失敗しました" >&2
        else
          echo "Nix Apps: $nixApps の退避に失敗しました" >&2
        fi

        break
      fi
    done
  '';

  # auto-update daemon の plist は、上流の `/etc` 節の直後、launchd 節より前に置き直す。
  #
  # launchd 節は設置済み plist と世代の plist に差があると `launchctl unload` してから置き直して
  # load し直す。適用を起こすのがこの daemon のとき、unload の対象は activation を走らせている当の
  # job になる。差分をここで先に消しておけば launchd 節は何もせず、daemon は自分を降ろさずに
  # `/run/current-system` の張り替えまで到達する。plist の内容が変わる適用は、`autoUpdateProgram` を
  # 入れたこの世代を含めてこの経路を通る。
  #
  # この位置を選ぶ理由は 3 つの前後関係にある。plist が指す `autoUpdateProgram` の実体は直前の `/etc`
  # 節が置くので、設置した plist の program は同じ適用の中で既に存在する。activation を中断する
  # fail-closed な検査（`activationScripts.checks`。上の Nix Apps の節が扱う App Management 検査も
  # ここへ合流する）はすべてこれより前で、中断した適用が新しい plist だけを残すことはない。そして
  # launchd 節より前なので `diff` が偽になり、daemon が自分を降ろさない条件を保つ。
  #
  # 上流の `load -w` も併せて飛ぶため、label が system domain に居ないときだけここで load する。
  # 既に降ろされているマシンはこの経路で戻る。既に居る job は、launchd が plist を読み直すまで
  # メモリ上の設定で動き続ける。上流の boot daemon（`modules/services/activate-system`）が splice
  # するのは `checks` / `etc` / `keyboard` の 3 節なので、この断片は再起動でも走る。
  #
  # `packageSystems` 外の system では daemon を宣言しないので、世代側の plist が無ければ何もしない。
  system.activationScripts.etc.text = lib.mkAfter ''
    autoUpdatePlist=${lib.escapeShellArg autoUpdateInstalledPlist}
    autoUpdateGenerationPlist="$systemConfig/Library/LaunchDaemons/${autoUpdateLabel}.plist"

    if [ -f "$autoUpdateGenerationPlist" ]; then
      if ! diff "$autoUpdateGenerationPlist" "$autoUpdatePlist" > /dev/null 2>&1; then
        cp -f "$autoUpdateGenerationPlist" "$autoUpdatePlist"
      fi

      if ! launchctl print ${lib.escapeShellArg "system/${autoUpdateLabel}"} > /dev/null 2>&1; then
        launchctl load -w "$autoUpdatePlist"
      fi
    fi
  '';

  # sudo の PAM 設定を nix-darwin で管理し、Touch ID 認証を通常端末と tmux/screen の両方で使えるようにする。
  security.pam.services.sudo_local = {
    touchIdAuth = true;
    reattach = true;
  };

  users.users.${user} = {
    home = "/Users/${user}";
    shell = pkgs.zsh;
  };

  system.primaryUser = user;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "before-home-manager";
  home-manager.extraSpecialArgs = {
    inherit
      inputs
      root
      user
      includeSelfPackage
      ;
  };
  # `dotfiles switch home` は standalone の `homeConfigurations`（`nix/home.nix`）だけを適用し、Home Manager の
  # generation は darwin 経由の適用と共有される。darwin 層でしか宣言されていない home 生成物は、home 層だけを
  # 適用した時点で旧世代のリンクとして撤去される。利用者の生成物は `nix/home.nix` の import 側で宣言する。
  home-manager.users.${user} = import ./home.nix;

  nix-homebrew = {
    enable = true;
    inherit user;
    autoMigrate = true;
    mutableTaps = false;
    taps = builtins.listToAttrs (
      map (tap: {
        name = tap.nixHomebrewName;
        value = tap.source;
      }) homebrewTaps
    );
  };

  system.stateVersion = 6;
}
