//! `dotfiles` CLI が省略値として使う環境情報を集める。
//!
//! ユーザー名、ホスト名、システム名、設定ディレクトリは `init` と `switch` の両方で使う。
//! 取得方法をここに閉じ込め、生成される flake の出力名と CLI が参照する出力名を一致させる。
//!
//! system 層をこのマシンで誰が持っているかもここで解決する。判定材料は nix-darwin が既に
//! 作っている成果物 `/etc/profiles/per-user/` だけにし、所有者名の写しを別ファイルへ書き出さない。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use anyhow::{anyhow, bail};
use dotfiles_core::host;

const CONFIG_SUBDIR: &str = ".config/dotfiles";
const CONFIG_FILE: &str = "flake.nix";

/// nix-darwin の `home-manager.useUserPackages` が管理対象ユーザーごとに作るディレクトリ。
const SYSTEM_PROFILES_DIR: &str = "/etc/profiles/per-user";

/// `dscl -readall` がレコードの区切りに使う行。
const RECORD_SEPARATOR: &str = "-";
/// ログイン名を持つ `dscl` 属性。別名を持つレコードでは先頭値が短い名前になる。
const RECORD_NAME_ATTRIBUTE: &str = "RecordName";
/// ホームディレクトリを持つ `dscl` 属性。
const HOME_DIRECTORY_ATTRIBUTE: &str = "NFSHomeDirectory";

/// 明示された設定ディレクトリを優先し、省略時は `$HOME/.config/dotfiles` を返す。
///
/// 既定値の解決は明示指定が無いときにだけ行う。無人経路（launchd system domain の auto-update
/// daemon）の環境は `$HOME` を持たないため、明示指定があるのに読むと設定ディレクトリを触る前に
/// 環境不足で落ちる。遅延評価する `map_or_else` を使い、既定値の解決を分岐の内側へ閉じる。
pub(crate) fn config_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    override_dir.map_or_else(default_config_dir, Ok)
}

/// 設定ディレクトリが明示されなかったときだけ使う `$HOME/.config/dotfiles`。
fn default_config_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(CONFIG_SUBDIR))
}

/// `dotfiles init` が書き込む `flake.nix` のパスを返す。
pub(crate) fn config_path(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    Ok(config_dir(override_dir)?.join(CONFIG_FILE))
}

/// `homeConfigurations.<user>` として使うログイン名を読む。
pub(crate) fn current_user() -> Result<String> {
    let output = Command::new("id").arg("-un").output()?;
    if !output.status.success() {
        bail!("id -un command failed");
    }
    let user = String::from_utf8(output.stdout)?;
    nonempty("user")(user.trim().to_string())
}

/// `darwinConfigurations.<host>` として使う短いホスト名を読む。
pub(crate) fn current_host() -> Result<String> {
    let output = Command::new("hostname").output()?;
    if !output.status.success() {
        bail!("hostname command failed");
    }
    let host = String::from_utf8(output.stdout)?;
    let host = host::short(host.trim());
    if host.is_empty() {
        bail!("host is required")
    } else {
        Ok(host.to_string())
    }
}

/// ローカル flake から適用してよい層の範囲。
///
/// nix-darwin の system 層はマシンに 1 つで、`system.primaryUser`、`users.users`、nix-homebrew の
/// 所有者を 1 人のユーザーへ結び付ける。別のユーザーがその層を適用すると結び付きがそのユーザーへ
/// 移り、元の所有者の宣言が消える。scope はその適用を止めるために使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigScope {
    /// home 層と system 層の両方を、そのユーザーの flake が持つ。
    Full,
    /// home 層だけを持つ。system 層はこのマシンの別ユーザーが持っている。
    Home,
}

/// このマシンの system 層が誰を管理しているかから、対象ユーザーの scope を決める。
///
/// `/etc/profiles/per-user/` のエントリは system 層の管理対象ユーザーと 1 対 1 で対応する。
/// ディレクトリ自体が無ければ system 層は未適用なので、そのユーザーが 1 人目として持つ。
pub(crate) fn config_scope(user: &str) -> Result<ConfigScope> {
    Ok(scope_from(
        system_profile_users(Path::new(SYSTEM_PROFILES_DIR))?.as_deref(),
        user,
    ))
}

/// このマシンでローカル flake を持つユーザーと、その設定ディレクトリ。
pub(crate) struct LocalFlakeAccount {
    pub(crate) user: String,
    pub(crate) config_dir: PathBuf,
}

/// ローカル flake を持つユーザーを、macOS のユーザーレコードから列挙する。
///
/// auto-update daemon が root から全ユーザーを更新するときの対象集合であり、ホームの位置を
/// `/Users/<name>` と仮定せずディレクトリサービスから引く。結果はユーザー名の昇順で返す。
///
/// 列挙は macOS のディレクトリサービス（`dscl`）に依存するため macOS 専用である。他 OS では
/// `dscl` の起動失敗を素の OS エラーとして見せず、対象を 1 人に絞る指定が要ることを示して止める。
pub(crate) fn local_flake_accounts() -> Result<Vec<LocalFlakeAccount>> {
    if std::env::consts::OS != "macos" {
        bail!(
            "全ユーザー走査は macOS のユーザーレコードに依存するため macOS 以外では使えない（`--user` で対象を 1 人に指定する）"
        );
    }
    let output = Command::new("dscl")
        .args([".", "-readall", "/Users", "RecordName", "NFSHomeDirectory"])
        .output()?;
    if !output.status.success() {
        bail!("dscl -readall /Users RecordName NFSHomeDirectory command failed");
    }
    let listing = String::from_utf8(output.stdout)?;
    let mut accounts = parse_user_homes(&listing)
        .into_iter()
        .map(|(user, home)| LocalFlakeAccount {
            user,
            config_dir: home.join(CONFIG_SUBDIR),
        })
        .filter(|account| account.config_dir.join(CONFIG_FILE).is_file())
        .collect::<Vec<_>>();
    accounts.sort_by(|left, right| left.user.cmp(&right.user));
    Ok(accounts)
}

/// 現在の OS と CPU から、ローカル flake に記録する既定の Nix system 文字列を作る。
pub(crate) fn default_system() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    format!("{arch}-{os}")
}

/// system 層が管理するユーザー名の一覧。ディレクトリが無ければ system 層は未適用で `None`。
fn system_profile_users(dir: &Path) -> Result<Option<Vec<OsString>>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut users = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        users.push(entry?.file_name());
    }
    Ok(Some(users))
}

/// system 層の管理対象ユーザー一覧から scope を決める。
fn scope_from(managed: Option<&[OsString]>, user: &str) -> ConfigScope {
    match managed {
        None => ConfigScope::Full,
        Some(users) if users.iter().any(|managed| managed == user) => ConfigScope::Full,
        Some(_) => ConfigScope::Home,
    }
}

/// `dscl . -readall /Users RecordName NFSHomeDirectory` の出力からユーザー名とホームを取り出す。
///
/// `dscl` はレコードを `-` だけの行で区切り、属性を次の 2 形式のどちらかで書く。値が空白を含むか
/// どうかで形式が切り替わるため、両方を読む必要がある。
///
/// - どの値も空白を含まない: `NFSHomeDirectory: /Users/alice`（値は空白区切りで 1 行に並ぶ）
/// - 空白を含む値がある: `NFSHomeDirectory:` の次行から、値 1 件ごとに空白 1 個を前置した行
///
/// `-list` は後者の形式を持たず値を空白で連結するだけなので、空白を含むホームと複数のホームを
/// 区別できない。区別できないとホームが途中で切れ、そのアカウントが黙って対象から外れる。
///
/// `root` のように複数のホームを持つレコードがあるため、値は先頭 1 件だけを使う。ユーザー名または
/// ホームを持たないレコードは対象から外す。
fn parse_user_homes(listing: &str) -> Vec<(String, PathBuf)> {
    let mut accounts = Vec::new();
    let mut record = UserRecord::default();
    // 直前の `<属性>:` 行。次に続く空白前置行がその属性の値になる。
    let mut pending_attribute = None;
    for line in listing.lines() {
        if line == RECORD_SEPARATOR {
            accounts.extend(std::mem::take(&mut record).into_account());
            pending_attribute = None;
        } else if let Some(value) = line.strip_prefix(' ') {
            // 先頭値だけを採るため、`take` で 2 件目以降を無視する。
            if let Some(attribute) = pending_attribute.take() {
                record.set(attribute, value);
            }
        } else if let Some((attribute, values)) = line.split_once(": ") {
            pending_attribute = None;
            if let Some(value) = values.split_whitespace().next() {
                record.set(attribute, value);
            }
        } else if let Some(attribute) = line.strip_suffix(':') {
            pending_attribute = Some(attribute);
        }
    }
    accounts.extend(record.into_account());
    accounts
}

/// `dscl` の 1 レコードから、必要な 2 属性の先頭値だけを取り出す途中状態。
#[derive(Default)]
struct UserRecord {
    user: Option<String>,
    home: Option<PathBuf>,
}

impl UserRecord {
    /// 関心のある属性の先頭値だけを保持する。同じ属性の 2 件目以降は捨てる。
    fn set(&mut self, attribute: &str, value: &str) {
        match attribute {
            RECORD_NAME_ATTRIBUTE => {
                self.user.get_or_insert_with(|| value.to_string());
            }
            HOME_DIRECTORY_ATTRIBUTE => {
                self.home.get_or_insert_with(|| PathBuf::from(value));
            }
            _ => {}
        }
    }

    /// ユーザー名とホームが揃ったレコードだけを列挙対象にする。
    fn into_account(self) -> Option<(String, PathBuf)> {
        Some((self.user?, self.home?))
    }
}

/// 設定ファイルの配置先を決めるため `$HOME` を必須値として読む。
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("HOME is required"))
}

/// 環境由来の値が空文字なら、後続の flake 出力名として使う前に失敗させる。
fn nonempty(name: &'static str) -> impl FnOnce(String) -> Result<String> {
    move |value| {
        if value.is_empty() {
            bail!("{name} is empty")
        } else {
            Ok(value)
        }
    }
}

/// ホスト名の正規化と、system 層の所有者判定に使う入力の解釈を検証する。
#[cfg(test)]
mod tests {
    use super::{ConfigScope, parse_user_homes, scope_from};
    use dotfiles_core::host;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn uses_short_hostname() {
        // CLI と生成 flake の出力名は同じ短いホスト名を使う必要がある。
        // ここがずれると `dotfiles switch darwin` が別の属性を参照する。
        assert_eq!(host::short("macbook.local"), "macbook");
        assert_eq!(host::short("macbook"), "macbook");
    }

    /// system 層が未適用のマシンでは、最初に導入するユーザーが system 層まで持つ。
    #[test]
    fn missing_system_profiles_dir_means_full_scope() {
        assert_eq!(scope_from(None, "alice"), ConfigScope::Full);
    }

    /// 自分のエントリがあるなら、そのユーザーが system 層の所有者である。
    #[test]
    fn own_entry_means_full_scope() {
        let managed = [OsString::from("alice")];
        assert_eq!(scope_from(Some(&managed), "alice"), ConfigScope::Full);
    }

    /// 別ユーザーが所有するマシンでは home 層だけを持つ。
    #[test]
    fn other_users_entry_means_home_scope() {
        let managed = [OsString::from("alice")];
        assert_eq!(scope_from(Some(&managed), "bob"), ConfigScope::Home);
    }

    /// `root` のようにホームを複数持つレコードは先頭のホームだけを採り、ホームを持たない
    /// レコードは対象から外す。空白を含むホームは `dscl` の空白前置行から全体を採る。
    ///
    /// 入力は実機の `dscl . -readall /Users RecordName NFSHomeDirectory` が返す 2 形式を写したもの。
    /// 空白を含む値は 1 行に並べず、`<属性>:` の次行から 1 件ずつ空白前置で返る。
    #[test]
    fn parses_user_homes_from_directory_service_records() {
        let listing = concat!(
            "NFSHomeDirectory: /Users/alice\n",
            "RecordName: alice\n",
            "-\n",
            "NFSHomeDirectory:\n",
            " /Users/space y\n",
            "RecordName: spacey\n",
            "-\n",
            "NFSHomeDirectory: /var/root /private/var/root\n",
            "RecordName:\n",
            " root\n",
            " BUILTIN\\Local System\n",
            "-\n",
            "RecordName: nohome\n",
        );

        assert_eq!(
            parse_user_homes(listing),
            vec![
                ("alice".to_string(), PathBuf::from("/Users/alice")),
                ("spacey".to_string(), PathBuf::from("/Users/space y")),
                ("root".to_string(), PathBuf::from("/var/root")),
            ]
        );
    }
}
