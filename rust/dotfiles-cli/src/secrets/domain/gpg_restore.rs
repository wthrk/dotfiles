//! `restore-gpg` / `export-ssh-public-key` の鍵リング非依存な domain 値・規則を担う層。
//!
//! ここに置くのは、gpgme / gpg-agent / process I/O などの外部実装を差し替えても変わらない
//! 業務規則だけである。具体的には import 後鍵が満たすべき subkey 構成の利用可能条件、
//! authentication subkey の keygrip 表現、OpenSSH 公開鍵行の妥当性、gpg-agent SSH support の
//! 充足条件（socket 解決 + authentication subkey 識別）である。鍵リング操作・keygrip 計算・
//! socket 検査そのものは port/adapter 側で行い、この層はそれらの結果値の検証・整合判定に限定する。
//! secret key material や平文 backup はこの層へ載せない。

use crate::Result;

/// import 後の鍵が満たすべき subkey capability の閉じた集合。
///
/// 設計「subkey 検証決定」は encryption / authentication / signing の 3 capability を必須とする。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SubkeyCapability {
    Encryption,
    Authentication,
    Signing,
}

impl SubkeyCapability {
    /// import 後鍵が必須とする capability を安定順で列挙する。
    pub fn required() -> [Self; 3] {
        [Self::Encryption, Self::Authentication, Self::Signing]
    }

    /// presentation で使う安定した capability 名を返す。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Encryption => "encryption",
            Self::Authentication => "authentication",
            Self::Signing => "signing",
        }
    }
}

/// import 後鍵から adapter が解決した 1 つの subkey の利用可能性を表す境界値。
///
/// adapter は keyring から各 subkey の capability と利用可能状態（revoked/expired/disabled）を
/// この値へ翻訳して渡し、利用可能性の domain 判定はこの module に閉じる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSubkey {
    /// この subkey が持つ capability。
    pub capability: SubkeyCapability,
    /// `revoked` / `expired` / `disabled` のいずれにも該当しないか。
    pub usable: bool,
}

/// import 後鍵の subkey 構成を集約し、利用可能状態を含めて検証する domain object。
///
/// adapter が解決した primary key の存在・secret material 保持・subkey 群を受け取り、
/// 設計「subkey 検証決定」の 4 条件（primary 1 つ・secret material 保持・3 capability が
/// それぞれ 1 つ以上・利用可能）を業務規則として判定する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedKeyComposition {
    has_secret_material: bool,
    subkeys: Vec<ResolvedSubkey>,
}

impl ImportedKeyComposition {
    /// adapter が解決した subkey 群から構成検証用の domain object を作る。
    pub fn new(has_secret_material: bool, subkeys: Vec<ResolvedSubkey>) -> Self {
        Self {
            has_secret_material,
            subkeys,
        }
    }

    /// import 後鍵が利用可能な subkey 構成を満たすことを検証する。
    ///
    /// secret material 不保持、必須 capability の欠落、または該当 capability の subkey が
    /// すべて利用不能（revoked/expired/disabled）である場合は停止条件として失敗する。
    /// 失敗 message に fingerprint 以外の鍵素材を含めない。
    pub fn ensure_usable(&self) -> Result<()> {
        if !self.has_secret_material {
            anyhow::bail!("imported GPG key does not hold secret key material");
        }
        for capability in SubkeyCapability::required() {
            let mut present = false;
            let mut usable = false;
            for subkey in &self.subkeys {
                if subkey.capability == capability {
                    present = true;
                    usable |= subkey.usable;
                }
            }
            if !present {
                anyhow::bail!(
                    "imported GPG key is missing a {} subkey",
                    capability.as_str()
                );
            }
            if !usable {
                anyhow::bail!(
                    "imported GPG key {} subkey is revoked, expired, or disabled",
                    capability.as_str()
                );
            }
        }
        Ok(())
    }
}

/// authentication subkey の keygrip を表す検証済み値。
///
/// keygrip は GnuPG が鍵素材から導出する 40 文字の uppercase hex 識別子であり、
/// gpg-agent の SSH key list（`sshcontrol` 相当）登録のキーになる。adapter が keyring から
/// 解決した値を受け取り、形式（hex 40 文字）だけを domain rule として固定する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keygrip(String);

impl Keygrip {
    /// keyring 由来の keygrip 文字列を uppercase hex 40 文字へ正規化して検証する。
    ///
    /// 区切り・空白は除去し、hex 以外・長さ不一致は domain failure として停止する。
    pub fn parse(value: &str) -> Result<Self> {
        let mut normalized = String::with_capacity(40);
        for ch in value.chars() {
            if ch.is_whitespace() || ch == ':' {
                continue;
            }
            if !ch.is_ascii_hexdigit() {
                anyhow::bail!("GPG keygrip must be hex; found a non-hex character");
            }
            normalized.push(ch.to_ascii_uppercase());
        }
        if normalized.len() != 40 {
            anyhow::bail!("GPG keygrip must normalize to 40 hex characters");
        }
        Ok(Self(normalized))
    }

    /// 正規化済み keygrip（uppercase hex 40 文字）を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// authentication subkey 由来の OpenSSH 公開鍵 1 行を表す検証済み値。
///
/// 設計「公開鍵出力契約」は「OpenSSH 公開鍵 1 行のみ、機械可読、秘密鍵素材を含めない」を
/// 要求する。adapter が keyring から導出した行を受け取り、1 行であること・既知の OpenSSH
/// 公開鍵 type prefix で始まること・base64 本体が存在することを domain rule として検証する。
/// この値は秘密情報ではないため、stdout 出力境界へそのまま渡してよい。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSshPublicKey(String);

/// OpenSSH 公開鍵行が取りうる type prefix の閉じた集合。
const OPENSSH_KEY_TYPES: [&str; 5] = [
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
];

impl OpenSshPublicKey {
    /// adapter が導出した OpenSSH 公開鍵行を検証して構築する。
    ///
    /// 改行・複数 token を含む行、未知の type prefix、空の base64 本体は domain failure として
    /// 停止する。comment は任意とし、`type base64 [comment]` の最小構造だけを強制する。
    pub fn parse(line: &str) -> Result<Self> {
        if line.contains('\n') || line.contains('\r') {
            anyhow::bail!("OpenSSH public key must be a single line");
        }
        let trimmed = line.trim();
        let mut fields = trimmed.split_whitespace();
        let key_type = fields
            .next()
            .ok_or_else(|| anyhow::anyhow!("OpenSSH public key line is empty"))?;
        if !OPENSSH_KEY_TYPES.contains(&key_type) {
            anyhow::bail!("OpenSSH public key has an unsupported key type");
        }
        let body = fields
            .next()
            .ok_or_else(|| anyhow::anyhow!("OpenSSH public key is missing its base64 body"))?;
        if body.is_empty()
            || !body.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'='
            })
        {
            anyhow::bail!("OpenSSH public key body is not valid base64");
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 検証済み OpenSSH 公開鍵 1 行を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// gpg-agent SSH support が利用可能であることを adapter が報告した観測結果。
///
/// 設計「gpg-agent SSH support 境界」は「SSH agent socket 参照先が解決でき、その socket 経路で
/// authentication subkey が識別可能」を同時に満たす状態を「利用可」とする。adapter は socket 解決
/// 可否と authentication subkey 識別可否を観測してこの値へ翻訳し、業務上の充足判定はこの module で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshAgentReadiness {
    /// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket として解決できたか。
    pub socket_resolved: bool,
    /// その SSH agent 経路で authentication subkey を identity として識別できたか。
    pub authentication_identity_present: bool,
}

impl SshAgentReadiness {
    /// gpg-agent SSH support が `restore-pass` へ引き渡せる前提を満たすことを検証する。
    ///
    /// socket 未解決、または authentication subkey が識別できない場合は停止条件として失敗する。
    pub fn ensure_ready(self) -> Result<()> {
        if !self.socket_resolved {
            anyhow::bail!("gpg-agent SSH agent socket could not be resolved");
        }
        if !self.authentication_identity_present {
            anyhow::bail!(
                "gpg-agent SSH support cannot use the GPG authentication subkey as an identity"
            );
        }
        Ok(())
    }
}

/// `restore-gpg` の完了状態を表す domain summary。
///
/// 設計「鍵リング復元契約」を満たして停止せず復元できたことの意味だけを保持し、表示仕様
/// （JSON key 名・整形）は adapter 側の責務とする。fingerprint 以外の鍵素材はここへ載せない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreGpgSummary {
    /// import / subkey 検証 / keygrip 登録の対象になった primary fingerprint（lowercase hex 40）。
    pub primary_fingerprint: String,
    /// authentication subkey の keygrip（uppercase hex 40）を SSH key list へ登録できたか。
    pub ssh_key_registered: bool,
    /// gpg-agent SSH support が利用可能（socket 解決 + authentication identity 識別）であったか。
    pub ssh_support_ready: bool,
}

#[cfg(test)]
mod tests {
    //! subkey 構成・keygrip・OpenSSH 公開鍵・SSH agent 充足の domain 規則を検証する単体テスト。
    //!
    //! 設計「subkey 検証決定」「公開鍵出力契約」「gpg-agent SSH support 境界」の充足/停止条件を
    //! 純粋ロジックとして網羅し、test double は持ち込まない。

    use super::*;

    fn subkey(capability: SubkeyCapability, usable: bool) -> ResolvedSubkey {
        ResolvedSubkey { capability, usable }
    }

    fn all_usable() -> Vec<ResolvedSubkey> {
        vec![
            subkey(SubkeyCapability::Encryption, true),
            subkey(SubkeyCapability::Authentication, true),
            subkey(SubkeyCapability::Signing, true),
        ]
    }

    #[test]
    fn usable_composition_passes() {
        let composition = ImportedKeyComposition::new(true, all_usable());
        assert!(composition.ensure_usable().is_ok());
    }

    #[test]
    fn missing_secret_material_fails() {
        let composition = ImportedKeyComposition::new(false, all_usable());
        assert!(composition.ensure_usable().is_err());
    }

    #[test]
    fn missing_capability_fails() {
        let composition = ImportedKeyComposition::new(
            true,
            vec![
                subkey(SubkeyCapability::Encryption, true),
                subkey(SubkeyCapability::Signing, true),
            ],
        );
        assert!(composition.ensure_usable().is_err());
    }

    #[test]
    fn unusable_capability_fails() {
        let composition = ImportedKeyComposition::new(
            true,
            vec![
                subkey(SubkeyCapability::Encryption, true),
                subkey(SubkeyCapability::Authentication, false),
                subkey(SubkeyCapability::Signing, true),
            ],
        );
        assert!(composition.ensure_usable().is_err());
    }

    #[test]
    fn capability_present_but_one_usable_passes() {
        // 同一 capability の subkey が複数あり、少なくとも 1 つが利用可能なら成立する。
        let composition = ImportedKeyComposition::new(
            true,
            vec![
                subkey(SubkeyCapability::Encryption, true),
                subkey(SubkeyCapability::Authentication, false),
                subkey(SubkeyCapability::Authentication, true),
                subkey(SubkeyCapability::Signing, true),
            ],
        );
        assert!(composition.ensure_usable().is_ok());
    }

    #[test]
    fn keygrip_normalizes_and_validates() {
        let raw = "aabb:ccdd:eeff:0011:2233:4455:6677:8899:aabb:ccdd";
        let keygrip = Keygrip::parse(raw).expect("valid keygrip");
        assert_eq!(keygrip.as_str(), "AABBCCDDEEFF00112233445566778899AABBCCDD");
        assert_eq!(keygrip.as_str().len(), 40);
    }

    #[test]
    fn keygrip_rejects_wrong_length() {
        assert!(Keygrip::parse("12ab").is_err());
    }

    #[test]
    fn keygrip_rejects_non_hex() {
        assert!(Keygrip::parse("zz3456789abcdef0123456789abcdef0123456789").is_err());
    }

    #[test]
    fn openssh_public_key_accepts_valid_ed25519_line() {
        let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTBODY comment";
        let key = OpenSshPublicKey::parse(line).expect("valid ssh key");
        assert_eq!(key.as_str(), line);
    }

    #[test]
    fn openssh_public_key_rejects_multiline() {
        assert!(OpenSshPublicKey::parse("ssh-ed25519 AAAA\nextra").is_err());
    }

    #[test]
    fn openssh_public_key_rejects_unknown_type() {
        assert!(OpenSshPublicKey::parse("ssh-unknown AAAA").is_err());
    }

    #[test]
    fn openssh_public_key_rejects_missing_body() {
        assert!(OpenSshPublicKey::parse("ssh-ed25519").is_err());
    }

    #[test]
    fn ssh_agent_readiness_requires_both_conditions() {
        assert!(
            SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            }
            .ensure_ready()
            .is_ok()
        );
        assert!(
            SshAgentReadiness {
                socket_resolved: false,
                authentication_identity_present: true,
            }
            .ensure_ready()
            .is_err()
        );
        assert!(
            SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: false,
            }
            .ensure_ready()
            .is_err()
        );
    }
}
