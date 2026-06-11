//! `restore-pass` の Git / SSH agent / filesystem 実装に依存しない domain 値・規則を担う層。
//!
//! ここに置くのは、git2 / SSH agent / filesystem などの外部実装を差し替えても変わらない
//! 業務規則だけである。具体的には `password-store-remote` が満たすべき clone URL 形式
//! （`git@github.com:<owner>/<repo>.git`）の妥当性、clone 後 store が `pass` から読める状態の
//! 充足条件（`.gpg-id` の存在・非空・各 recipient が非空 token であること。store entry の復号可否は
//! keyring 照合で確定し、これが可読性の最終判定となる）、`.gpg-id` recipient（`GpgRecipientId`）の妥当性、`restore-pass`
//! の完了状態の意味である。clone そのもの・
//! `~/.password-store` の存在確認・store の filesystem 走査は port/adapter 側で行い、この層は
//! それらの結果値の検証・整合判定に限定する。secret 値はこの層へ載せない。

use crate::Result;

/// `~/.password-store` を指す store path の業務名。
///
/// `secret-recovery-spec.md` は個人 vault から `password-store-remote` を取得し、GPG authentication subkey
/// 経由の SSH agent 認証で clone することを定義する。この実装では clone 先を home 直下の
/// `.password-store` に固定する。実際の `$HOME` 解決と存在確認は filesystem adapter の責務であり、
/// この定数は復旧対象 store path の実装ローカルな同一性だけを表す。
pub const PASSWORD_STORE_DIR_NAME: &str = ".password-store";

/// `pass` が store を読めることの判定に使う store 識別ファイル名。
///
/// `pass` の store は root に GPG recipient を記す `.gpg-id` を持つ。clone した directory が
/// この識別ファイルを持つことだけでは「store として読める」最小条件にならない。`.gpg-id` が
/// 空・不正、または手元に秘密鍵を持たない別 GPG 鍵宛てだけの場合は `pass` が復号できないため、
/// 識別ファイルの存在・非空 recipient に加えて、store entry の実復号可否（entry が無い空 store では
/// recipient のいずれか 1 つに対応する秘密鍵の保持）まで検証する。`pass` CLI への無条件シェルアウトには依存しない。
pub const PASSWORD_STORE_GPG_ID: &str = ".gpg-id";

/// `password-store-remote` が満たすべき GitHub SSH clone URL を表す検証済み値。
///
/// `secret-recovery-spec.md` と `bitwarden-personal-vault-design.md` は `password-store-remote` を
/// private password-store repository の GitHub SSH clone URL として扱う。
/// adapter が Bitwarden vault から取得した secret value 文字列を [`PasswordStoreRemote::parse`] で検証して
/// 構築し、domain は復旧で使う canonical な `git@github.com:<owner>/<repo>.git` 形式だけを受け入れる。
/// URL は秘密情報ではないため domain 値として保持してよいが、GitHub SSH clone URL としての
/// `<owner>` / `<repo>` 以外のスキーム・ホスト・余剰要素は停止条件として拒否する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordStoreRemote(String);

/// SSH clone URL の固定 prefix（`git@github.com:`）。GitHub 以外のホストは復旧対象に含めない。
const GITHUB_SSH_PREFIX: &str = "git@github.com:";
/// SSH clone URL の固定 suffix（`.git`）。
const GIT_SUFFIX: &str = ".git";

impl PasswordStoreRemote {
    /// Bitwarden vault 由来の `password-store-remote` 文字列を GitHub SSH clone URL として検証して構築する。
    ///
    /// `secret-recovery-spec.md` と `bitwarden-personal-vault-design.md` が `password-store-remote` として扱う
    /// GitHub SSH clone URL を、復旧用の canonical な `git@github.com:<owner>/<repo>.git` 形式として受け入れる。
    /// trim による値の黙示変更を避けるため、前後・内部のいずれであれ空白や制御文字を含む値は停止条件で拒否する。
    /// prefix/suffix 不一致、`<owner>`/`<repo>` の欠落・空・余剰 path segment も domain failure として停止する。
    /// `<owner>` / `<repo>` は GitHub repository identity として扱える ASCII の閉じた文字集合だけを許可し、
    /// path traversal や追加 segment（`/` を 1 つだけ含む）を作らせない。
    pub fn parse(value: &str) -> Result<Self> {
        // trim せず、空白・制御文字を含む値は前後・内部いずれでも拒否する。空白は ASCII に
        // 限らず、U+2000 などの非 ASCII 空白も `char::is_whitespace` で一律に拒否する。
        if value.chars().any(|ch| ch.is_whitespace()) {
            anyhow::bail!("password-store-remote must not contain whitespace");
        }
        if value.chars().any(|ch| ch.is_control()) {
            anyhow::bail!("password-store-remote must not contain control characters");
        }
        let Some(without_prefix) = value.strip_prefix(GITHUB_SSH_PREFIX) else {
            anyhow::bail!("password-store-remote must be a git@github.com SSH clone URL");
        };
        let Some(owner_repo) = without_prefix.strip_suffix(GIT_SUFFIX) else {
            anyhow::bail!("password-store-remote must end with .git");
        };
        let mut segments = owner_repo.split('/');
        let owner = segments
            .next()
            .filter(|owner| is_valid_owner(owner))
            .ok_or_else(|| anyhow::anyhow!("password-store-remote owner is invalid"))?;
        let repo = segments
            .next()
            .filter(|repo| is_valid_repository(repo))
            .ok_or_else(|| anyhow::anyhow!("password-store-remote repository is invalid"))?;
        if segments.next().is_some() {
            anyhow::bail!("password-store-remote must be exactly owner/repository");
        }
        // owner / repo 以外の文字混入を弾いたうえで canonical な形へ正規化する。
        Ok(Self(format!(
            "{GITHUB_SSH_PREFIX}{owner}/{repo}{GIT_SUFFIX}"
        )))
    }

    /// 既存 password-store の GitHub origin remote を repository identity として受け入れ、
    /// Bitwarden vault 登録用の SSH clone URL へ正規化する。
    ///
    /// 利用者の既存 `password-store` が HTTPS origin を使っている場合でも、origin は repository の同一性確認に
    /// だけ使う。復旧時の clone は GPG authentication subkey 経由の SSH に固定するため、Bitwarden vault へ保存する値は常に
    /// `git@github.com:<owner>/<repo>.git` 形式へ canonicalize する。CLI/shell はこの SSH URL を argv/stdin/env
    /// で中継せず、application が観測済み origin から domain rule として導出する。
    pub fn from_configured_origin(value: &str) -> Result<Self> {
        if let Ok(remote) = Self::parse(value) {
            return Ok(remote);
        }
        if value.chars().any(|ch| ch.is_whitespace()) {
            anyhow::bail!("password-store origin remote must not contain whitespace");
        }
        if value.chars().any(|ch| ch.is_control()) {
            anyhow::bail!("password-store origin remote must not contain control characters");
        }
        let Some(without_prefix) = value.strip_prefix("https://github.com/") else {
            anyhow::bail!("password-store origin remote must be a GitHub SSH or HTTPS clone URL");
        };
        let owner_repo = without_prefix
            .strip_suffix(GIT_SUFFIX)
            .unwrap_or(without_prefix);
        let mut segments = owner_repo.split('/');
        let owner = segments
            .next()
            .filter(|owner| is_valid_owner(owner))
            .ok_or_else(|| anyhow::anyhow!("password-store origin owner is invalid"))?;
        let repo = segments
            .next()
            .filter(|repo| is_valid_repository(repo))
            .ok_or_else(|| anyhow::anyhow!("password-store origin repository is invalid"))?;
        if segments.next().is_some() {
            anyhow::bail!("password-store origin remote must be exactly owner/repository");
        }
        Ok(Self(format!(
            "{GITHUB_SSH_PREFIX}{owner}/{repo}{GIT_SUFFIX}"
        )))
    }

    /// 検証済み clone URL を adapter（git2）へ渡すために借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// GitHub `<owner>` 識別子の妥当性を判定する。
///
/// GitHub repository identity の owner として、先頭末尾は英数字、中間は英数字とハイフン、
/// 全体は 1〜39 文字（先頭 1 + 中間
/// 0〜37 + 末尾 1、1 文字 owner も許可）に限定し、先頭/末尾ハイフン、`_`、`.`、39 文字超は拒否する。
fn is_valid_owner(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes {
        // 1 文字 owner は英数字 1 文字のみ。
        [only] => only.is_ascii_alphanumeric(),
        // 2 文字以上は先頭末尾が英数字、中間は英数字とハイフン、全体 39 文字以内。
        [first, middle @ .., last] if value.len() <= 39 => {
            first.is_ascii_alphanumeric()
                && last.is_ascii_alphanumeric()
                && middle
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        }
        _ => false,
    }
}

/// GitHub `<repo>` 識別子の妥当性を判定する。
///
/// GitHub repository identity の repository name として、空、`.`/`..` のみ、`/` や制御文字・空白は
/// parse 段階で拒否済みだが、ここでも
/// 英数・`-`・`_`・`.` 以外の混入と空・`.`/`..` を拒否する。
fn is_valid_repository(value: &str) -> bool {
    if value.is_empty() || value == "." || value == ".." {
        return false;
    }
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// `.gpg-id` に記された GPG recipient（key id / fingerprint / email・user-id）を表す検証済み値。
///
/// この型が保証するのは「keyring 照合へ渡せる非空の recipient token であること」だけである。`pass init`
/// は long key id・fingerprint だけでなく email / user-id も recipient として受け付けるため、`.gpg-id` は
/// hex とは限らない。recipient の解決は gpgme（`get_secret_key`）が担い、hex key id・fingerprint・
/// user-id/email のいずれも同じ API で解決できる。store が実際に読めるかの最終判定は store entry の復号
/// （`can_decrypt_store_entry`）で行うため、この層は recipient を hex へ制約しない。別鍵宛て・短縮・曖昧な
/// id は keyring 照合で単に「秘密鍵なし」と評価され、上流で安全に扱われる。値そのものは秘密情報ではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpgRecipientId(String);

impl GpgRecipientId {
    /// `.gpg-id` の 1 行を GPG recipient token として検証して構築する。
    ///
    /// 前後空白を除いた本体が非空であれば、`pass` が受け付ける recipient（hex key id・fingerprint・
    /// email・user-id）として trim 済みの文字列をそのまま保持する（hex 以外がありうるため大文字化しない）。
    /// 空・空白のみ、または ASCII 制御文字を含む行だけを domain failure として停止する。recipient の鍵解決は
    /// gpgme に委ね、可読性の最終判定は store entry の復号で行う。
    pub fn parse(line: &str) -> Result<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            anyhow::bail!("password-store .gpg-id recipient is empty");
        }
        if trimmed.chars().any(|ch| ch.is_ascii_control()) {
            anyhow::bail!("password-store .gpg-id recipient contains control characters");
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// trim 済み recipient token を keyring 照合（gpgme `get_secret_key`）へ渡すために借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// clone 後 store が `pass` から読める構成であることを adapter が観測した結果。
///
/// `secret-recovery-spec.md` が定義する SSH agent 認証 clone の後、この実装は clone 済み store を
/// `pass` 互換に読める構成へ到達したか確認する。adapter は clone 先 directory を走査し、store 識別ファイル
/// （`.gpg-id`）の有無・recipient 行・復号確認に使うサンプル entry path を観測してこの値へ翻訳する。
/// recipient が非空 token であることと、サンプル entry を手元の復元済み秘密鍵で復号できることの業務判定は
/// この module（と keyring 照合）で行い、`pass` CLI への無条件シェルアウトはしない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordStoreReadiness {
    /// clone 先 store root に `.gpg-id` が存在したか。
    pub gpg_id_present: bool,
    /// `.gpg-id` の各行から読み取った recipient 候補（空行・コメントは除外、未 trim の生文字列）。
    /// adapter は filesystem 走査だけを担い、形式妥当性は domain で判定する。
    pub gpg_id_recipients: Vec<String>,
    /// 復号可否確認に使う store 内サンプル entry（`*.gpg`）の path。1 件も無ければ `None`。
    pub sample_entry: Option<std::path::PathBuf>,
}

impl PasswordStoreReadiness {
    /// `.gpg-id` の recipient 行を検証済み [`GpgRecipientId`] の集合へ変換する。
    ///
    /// 識別ファイルが存在しない、recipient が 1 件も無い、いずれかの行が空・制御文字を含む場合は、
    /// clone が成功しても `pass` store として不完全・不正であるとして停止条件で失敗する。返す recipient は
    /// keyring 照合（復元済み秘密鍵を持つか・復号できるか）へ渡す対象であり、ここでは非空 token であることだけを確定する。
    pub fn parse_recipients(&self) -> Result<Vec<GpgRecipientId>> {
        if !self.gpg_id_present {
            anyhow::bail!(
                "cloned password-store is missing its {PASSWORD_STORE_GPG_ID}; pass cannot read the store"
            );
        }
        if self.gpg_id_recipients.is_empty() {
            anyhow::bail!(
                "cloned password-store {PASSWORD_STORE_GPG_ID} is empty; pass cannot determine its recipients"
            );
        }
        self.gpg_id_recipients
            .iter()
            .map(|line| GpgRecipientId::parse(line))
            .collect()
    }

    /// 復号確認に使う store 内サンプル entry path を借用する（存在しなければ `None`）。
    pub fn sample_entry(&self) -> Option<&std::path::Path> {
        self.sample_entry.as_deref()
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
    //! 正本が定義する `password-store-remote` と、実装ローカルの clone URL 形式・store 可読性の
    //! 充足/停止条件を純粋ロジックとして網羅し、test double は持ち込まない。

    use super::*;

    #[test]
    fn parses_valid_github_ssh_clone_url() -> Result<()> {
        let remote = PasswordStoreRemote::parse("git@github.com:owner/password-store.git")?;
        assert_eq!(remote.as_str(), "git@github.com:owner/password-store.git");
        Ok(())
    }

    /// HTTPS origin は既存 repository identity として受け入れ、復旧用の SSH clone URL へ正規化する。
    #[test]
    fn configured_https_origin_normalizes_to_ssh_clone_url() -> Result<()> {
        let remote =
            PasswordStoreRemote::from_configured_origin("https://github.com/owner/repo.git")?;
        assert_eq!(remote.as_str(), "git@github.com:owner/repo.git");
        Ok(())
    }

    #[test]
    fn parses_single_character_owner() -> Result<()> {
        // owner は 1 文字英数字も許可する（owner 規則の先頭 1 文字 + 中間 0 + 末尾省略）。
        let remote = PasswordStoreRemote::parse("git@github.com:o/r.git")?;
        assert_eq!(remote.as_str(), "git@github.com:o/r.git");
        Ok(())
    }

    #[test]
    fn parses_max_length_owner() -> Result<()> {
        // 39 文字 owner は許可する（先頭 1 + 中間 37 + 末尾 1）。
        let owner = "a".repeat(39);
        let remote = PasswordStoreRemote::parse(&format!("git@github.com:{owner}/repo.git"))?;
        assert_eq!(remote.as_str(), format!("git@github.com:{owner}/repo.git"));
        Ok(())
    }

    #[test]
    fn rejects_surrounding_whitespace() {
        // canonical URL rule は前後空白を許可しない。trim せず停止する。
        assert!(PasswordStoreRemote::parse("  git@github.com:o/r.git\t").is_err());
        assert!(PasswordStoreRemote::parse("git@github.com:o/r.git ").is_err());
        assert!(PasswordStoreRemote::parse(" git@github.com:o/r.git").is_err());
    }

    #[test]
    fn rejects_internal_whitespace() {
        // 内部の空白も拒否する（canonical URL rule と owner/repository 規則の空白禁止）。
        assert!(PasswordStoreRemote::parse("git@github.com:o w/r.git").is_err());
        assert!(PasswordStoreRemote::parse("git@github.com:o/r\t.git").is_err());
    }

    #[test]
    fn rejects_non_ascii_whitespace() {
        // ASCII 以外の空白（U+2000 EN QUAD など）も `char::is_whitespace` で一律に拒否する。
        assert!(PasswordStoreRemote::parse("git@github.com:owner/re\u{2000}po.git").is_err());
    }

    #[test]
    fn rejects_control_characters() {
        // 制御文字を含む値は拒否する。
        assert!(PasswordStoreRemote::parse("git@github.com:o/r.git\u{0007}").is_err());
    }

    #[test]
    fn rejects_owner_with_underscore() {
        // owner に `_` は許可しない（owner 規則）。
        assert!(PasswordStoreRemote::parse("git@github.com:bad_owner/repo.git").is_err());
    }

    #[test]
    fn rejects_owner_with_dot() {
        // owner に `.` は許可しない。
        assert!(PasswordStoreRemote::parse("git@github.com:bad.owner/repo.git").is_err());
    }

    #[test]
    fn rejects_owner_with_leading_or_trailing_hyphen() {
        // owner の先頭/末尾ハイフンは許可しない。
        assert!(PasswordStoreRemote::parse("git@github.com:-owner/repo.git").is_err());
        assert!(PasswordStoreRemote::parse("git@github.com:owner-/repo.git").is_err());
    }

    #[test]
    fn rejects_owner_over_max_length() {
        // 40 文字 owner は許可しない（全体 1〜39 文字）。
        let owner = "a".repeat(40);
        assert!(PasswordStoreRemote::parse(&format!("git@github.com:{owner}/repo.git")).is_err());
    }

    #[test]
    fn accepts_repository_with_dot_underscore_hyphen() -> Result<()> {
        // repo は `[A-Za-z0-9._-]+` を許可する（owner と異なり `.`/`_` を含める）。
        let remote = PasswordStoreRemote::parse("git@github.com:owner/my.password_store-1.git")?;
        assert_eq!(
            remote.as_str(),
            "git@github.com:owner/my.password_store-1.git"
        );
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

    fn readiness(
        gpg_id_present: bool,
        recipients: &[&str],
        sample: Option<&str>,
    ) -> PasswordStoreReadiness {
        PasswordStoreReadiness {
            gpg_id_present,
            gpg_id_recipients: recipients.iter().map(|line| (*line).to_owned()).collect(),
            sample_entry: sample.map(std::path::PathBuf::from),
        }
    }

    #[test]
    fn parses_recipients_for_readable_store() -> Result<()> {
        // recipient 行は trim 済みの token としてそのまま受理する（hex の大文字化はしない）。
        let recipients = readiness(
            true,
            &[
                "0123456789abcdef",
                "0x0123456789abcdef0123456789abcdef01234567",
                "alice@example.com",
            ],
            None,
        )
        .parse_recipients()?;
        assert_eq!(recipients.len(), 3);
        assert_eq!(recipients[0].as_str(), "0123456789abcdef");
        assert_eq!(
            recipients[1].as_str(),
            "0x0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(recipients[2].as_str(), "alice@example.com");
        Ok(())
    }

    #[test]
    fn parse_recipients_requires_gpg_id_file() {
        // `.gpg-id` 不在は停止条件。
        assert!(readiness(false, &[], None).parse_recipients().is_err());
    }

    #[test]
    fn parse_recipients_rejects_empty_gpg_id() {
        // 識別ファイルはあるが recipient が 1 件も無い（空 `.gpg-id`）は停止条件。
        assert!(readiness(true, &[], None).parse_recipients().is_err());
    }

    #[test]
    fn parse_recipients_rejects_empty_recipient_line() {
        // recipient 行が空・空白のみだけの `.gpg-id` は `GpgRecipientId::parse` 経由で拒否する。
        assert!(readiness(true, &[""], None).parse_recipients().is_err());
        assert!(readiness(true, &["   "], None).parse_recipients().is_err());
    }

    #[test]
    fn gpg_recipient_id_accepts_long_id_and_fingerprint() -> Result<()> {
        // trim 済み token をそのまま保持する（hex でも大文字化しない）。
        assert_eq!(
            GpgRecipientId::parse("  0123456789abcdef \n")?.as_str(),
            "0123456789abcdef"
        );
        assert_eq!(
            GpgRecipientId::parse("0123456789ABCDEF0123456789ABCDEF01234567")?.as_str(),
            "0123456789ABCDEF0123456789ABCDEF01234567"
        );
        // email / user-id も `pass` が受け付ける recipient なので受理する。
        assert_eq!(
            GpgRecipientId::parse("alice@example.com")?.as_str(),
            "alice@example.com"
        );
        // short id・`0x` 付き非 hex も opaque token として受理する（解決は gpgme が担う）。
        assert_eq!(GpgRecipientId::parse("DEADBEEF")?.as_str(), "DEADBEEF");
        Ok(())
    }

    #[test]
    fn gpg_recipient_id_rejects_empty_and_control() {
        // 空・空白のみ・制御文字を含む行だけを停止条件として拒否する。
        assert!(GpgRecipientId::parse("").is_err());
        assert!(GpgRecipientId::parse("   ").is_err());
        assert!(GpgRecipientId::parse("alice\u{0007}@example.com").is_err());
    }
}
