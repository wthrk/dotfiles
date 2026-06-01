//! `restore-pass` の Git / SSH agent / filesystem 実装に依存しない domain 値・規則を担う層。
//!
//! ここに置くのは、git2 / SSH agent / filesystem などの外部実装を差し替えても変わらない
//! 業務規則だけである。具体的には `password-store-remote` が満たすべき clone URL 形式
//! （`git@github.com:<owner>/<repo>.git`）の妥当性、clone 後 store が `pass` から読める状態の
//! 充足条件（`.gpg-id` の存在）、`restore-pass` の完了状態の意味である。clone そのもの・
//! `~/.password-store` の存在確認・store の filesystem 走査は port/adapter 側で行い、この層は
//! それらの結果値の検証・整合判定に限定する。secret 値はこの層へ載せない。

use crate::Result;

/// `~/.password-store` を指す store path の業務名。
///
/// 設計（spec L174）では clone 先を `~/.password-store` に固定する。実際の `$HOME` 解決と
/// 存在確認は filesystem adapter の責務であり、この定数は復旧対象の store path の業務上の
/// 同一性（home 直下の `.password-store`）だけを表す。
pub const PASSWORD_STORE_DIR_NAME: &str = ".password-store";

/// `pass` が store を読めることの判定に使う store 識別ファイル名。
///
/// `pass` の store は root に GPG recipient を記す `.gpg-id` を持つ。clone した directory が
/// この識別ファイルを持つことを「store として読める」最小条件とし、`pass` CLI への無条件
/// シェルアウトに依存しない。
pub const PASSWORD_STORE_GPG_ID: &str = ".gpg-id";

/// `password-store-remote` が満たすべき GitHub SSH clone URL を表す検証済み値。
///
/// 設計（spec L56 / L213、停止条件）は値を `git@github.com:<owner>/<repo>.git` 形式に限定する。
/// adapter が BWS から取得した secret value 文字列を [`PasswordStoreRemote::parse`] で検証して
/// 構築し、形式に合致した値だけがこの型になる。URL は秘密情報ではないため domain 値として保持
/// してよいが、`<owner>` / `<repo>` 以外のスキーム・ホスト・余剰要素は停止条件として拒否する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordStoreRemote(String);

/// SSH clone URL の固定 prefix（`git@github.com:`）。GitHub 以外のホストは復旧対象に含めない。
const GITHUB_SSH_PREFIX: &str = "git@github.com:";
/// SSH clone URL の固定 suffix（`.git`）。
const GIT_SUFFIX: &str = ".git";

impl PasswordStoreRemote {
    /// BWS 由来の `password-store-remote` 文字列を GitHub SSH clone URL として検証して構築する。
    ///
    /// `git@github.com:<owner>/<repo>.git` の固定形式だけを許可する。前後空白は除去し、改行を含む
    /// 値、prefix/suffix 不一致、`<owner>`/`<repo>` の欠落・空・余剰 path segment は domain failure
    /// として停止する。`<owner>` と `<repo>` は GitHub の識別子に許される文字（英数・`-`・`_`・`.`）
    /// だけを許可し、path traversal や追加 segment（`/` を 1 つだけ含む）を作らせない。
    pub fn parse(value: &str) -> Result<Self> {
        if value.contains('\n') || value.contains('\r') {
            anyhow::bail!("password-store-remote must be a single line");
        }
        let trimmed = value.trim();
        let Some(without_prefix) = trimmed.strip_prefix(GITHUB_SSH_PREFIX) else {
            anyhow::bail!("password-store-remote must be a git@github.com SSH clone URL");
        };
        let Some(owner_repo) = without_prefix.strip_suffix(GIT_SUFFIX) else {
            anyhow::bail!("password-store-remote must end with .git");
        };
        let mut segments = owner_repo.split('/');
        let owner = segments
            .next()
            .filter(|owner| is_valid_path_component(owner))
            .ok_or_else(|| anyhow::anyhow!("password-store-remote owner is invalid"))?;
        let repo = segments
            .next()
            .filter(|repo| is_valid_path_component(repo))
            .ok_or_else(|| anyhow::anyhow!("password-store-remote repository is invalid"))?;
        if segments.next().is_some() {
            anyhow::bail!("password-store-remote must be exactly owner/repository");
        }
        // owner / repo 以外の文字混入を弾いたうえで canonical な形へ正規化する。
        Ok(Self(format!(
            "{GITHUB_SSH_PREFIX}{owner}/{repo}{GIT_SUFFIX}"
        )))
    }

    /// 検証済み clone URL を adapter（git2）へ渡すために借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// GitHub の owner / repository 識別子に許す文字だけで構成されるかを判定する。
///
/// 空、`.`/`..` のみ、`/` や path traversal 文字を許さず、英数・`-`・`_`・`.` だけを許可する。
fn is_valid_path_component(value: &str) -> bool {
    if value.is_empty() || value == "." || value == ".." {
        return false;
    }
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// clone 後 store が `pass` から読める構成であることを adapter が観測した結果。
///
/// 設計（spec L174）は clone 後に「`pass` が store を読めること」の確認を要求する。adapter は
/// clone 先 directory を走査して store 識別ファイル（`.gpg-id`）の有無を観測してこの値へ翻訳し、
/// 業務上の充足判定はこの module で行う。`pass` CLI への無条件シェルアウトはしない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordStoreReadiness {
    /// clone 先 store root に `.gpg-id` が存在したか。
    pub gpg_id_present: bool,
}

impl PasswordStoreReadiness {
    /// clone した store が `pass` から読める構成を満たすことを検証する。
    ///
    /// store 識別ファイルが存在しない場合は、clone は成功しても `pass` store として不完全である
    /// として停止条件で失敗する。
    pub fn ensure_readable(self) -> Result<()> {
        if !self.gpg_id_present {
            anyhow::bail!(
                "cloned password-store is missing its {PASSWORD_STORE_GPG_ID}; pass cannot read the store"
            );
        }
        Ok(())
    }
}

/// `restore-pass` の完了状態を表す domain summary。
///
/// clone 先 store path と、store が `pass` から読める状態へ到達したことの意味だけを保持し、
/// 表示仕様（JSON key 名・整形）は adapter 側の責務とする。secret 値はここへ載せない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePassSummary {
    /// clone した store の path（`~/.password-store`）。
    pub store_path: String,
    /// clone 後 store が `pass` から読めること（`.gpg-id` 存在）を確認できたか。
    pub store_readable: bool,
}

#[cfg(test)]
mod tests {
    //! `password-store-remote` URL 妥当性・store 可読性・summary の domain 規則を検証する単体テスト。
    //!
    //! 設計（spec L56 / L174 / L213）の clone URL 形式と store 可読性の充足/停止条件を純粋ロジック
    //! として網羅し、test double は持ち込まない。

    use super::*;

    #[test]
    fn parses_valid_github_ssh_clone_url() -> Result<()> {
        let remote = PasswordStoreRemote::parse("git@github.com:owner/password-store.git")?;
        assert_eq!(remote.as_str(), "git@github.com:owner/password-store.git");
        Ok(())
    }

    #[test]
    fn trims_surrounding_whitespace() -> Result<()> {
        let remote = PasswordStoreRemote::parse("  git@github.com:o/r.git\t")?;
        assert_eq!(remote.as_str(), "git@github.com:o/r.git");
        Ok(())
    }

    #[test]
    fn rejects_https_url() {
        assert!(PasswordStoreRemote::parse("https://github.com/owner/repo.git").is_err());
    }

    #[test]
    fn rejects_non_github_host() {
        assert!(PasswordStoreRemote::parse("git@gitlab.com:owner/repo.git").is_err());
    }

    #[test]
    fn rejects_missing_git_suffix() {
        assert!(PasswordStoreRemote::parse("git@github.com:owner/repo").is_err());
    }

    #[test]
    fn rejects_missing_repository() {
        assert!(PasswordStoreRemote::parse("git@github.com:owner.git").is_err());
    }

    #[test]
    fn rejects_extra_path_segments() {
        assert!(PasswordStoreRemote::parse("git@github.com:owner/group/repo.git").is_err());
    }

    #[test]
    fn rejects_path_traversal_components() {
        assert!(PasswordStoreRemote::parse("git@github.com:../etc/repo.git").is_err());
        assert!(PasswordStoreRemote::parse("git@github.com:owner/..git").is_err());
    }

    #[test]
    fn rejects_multiline_value() {
        assert!(PasswordStoreRemote::parse("git@github.com:owner/repo.git\nextra").is_err());
    }

    #[test]
    fn readable_store_requires_gpg_id() {
        assert!(
            PasswordStoreReadiness {
                gpg_id_present: true
            }
            .ensure_readable()
            .is_ok()
        );
        assert!(
            PasswordStoreReadiness {
                gpg_id_present: false
            }
            .ensure_readable()
            .is_err()
        );
    }
}
