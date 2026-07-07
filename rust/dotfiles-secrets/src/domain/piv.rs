//! PIV 制約と object 対応規則を domain で固定し、adapter ごとの差異で保存意味が変わることを防ぐ。

use std::{fmt, str::FromStr};

const STORAGE_BLOB_VERSION: u8 = 1;
const MIN_PIV_METADATA_VERSION: PivApplicationVersion = PivApplicationVersion {
    major: 5,
    minor: 3,
    patch: 0,
};

/// YubiKey PIV PIN の最小 byte 長。
pub const PIV_PIN_MIN_LEN: usize = 6;
/// YubiKey PIV PIN の最大 byte 長。
pub const PIV_PIN_MAX_LEN: usize = 8;

/// YubiKey PIV PIN の長さ制約を検証する。
///
/// PIN 値そのものは受け取らず長さだけを使うことで、secret buffer の中身を
/// domain 層へ露出させずに PIV PIN policy を domain rule として固定する。
pub fn validate_piv_pin_len(len: usize) -> crate::Result<()> {
    if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&len) {
        anyhow::bail!("YubiKey PIN must be 6 to 8 bytes");
    }
    Ok(())
}

/// YubiKey PIV application version を SDK 非依存に表す値オブジェクト。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PivApplicationVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl PivApplicationVersion {
    /// secret storage setup に必要な最小 PIV application version を返す。
    pub fn minimum_for_secret_storage() -> Self {
        MIN_PIV_METADATA_VERSION
    }
}

impl fmt::Display for PivApplicationVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// secret storage setup 前に PIV application の状態が業務上許容されるかを確認する。
pub fn validate_secret_storage_setup_preconditions(
    version: PivApplicationVersion,
    pin_retries: u8,
) -> crate::Result<()> {
    let minimum = PivApplicationVersion::minimum_for_secret_storage();
    if version < minimum {
        anyhow::bail!("YubiKey PIV application version must be at least {minimum}");
    }
    if pin_retries == 0 {
        anyhow::bail!("YubiKey PIN retries are exhausted");
    }
    Ok(())
}

/// PIV data object ID を表す値オブジェクト。
///
/// 値は常に 32-bit object ID として保持し、表示・AEAD additional data・PIV API 引数への変換規則を一元化する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PivObjectId(u32);

impl PivObjectId {
    /// manifest を保存する PIV data object ID。
    pub const MANIFEST: Self = Self(0x005f_ff16);

    /// PIV data object API に渡す raw object ID を返す。
    ///
    /// 返値は YubiKey PIV object 指定以外の意味を持たず、呼び出し側は別用途へ流用しないこと。
    pub fn value(self) -> u32 {
        self.0
    }

    /// PIV object ID を AEAD additional data 用の big-endian bytes へ変換する。
    fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

impl fmt::Display for PivObjectId {
    /// PIV object ID を 8 桁 hex 表記で表示する。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:08X}", self.0)
    }
}

/// YubiKey secret storage が予約する PIV data object ID 集合を表す名前空間。
///
/// manifest object と各 secret object の固定配置だけを定義し、列挙順は storage layout の安定順を守る。
pub struct StorageObjectIds;

impl StorageObjectIds {
    /// manifest と全 secret blob の object ID を列挙する。
    ///
    /// 列挙順は `ensure_setup_allowed` と verification が参照する安定順で、呼び出し側はこの順序を変更前提に依存してはならない。
    pub fn iter() -> impl Iterator<Item = PivObjectId> {
        std::iter::once(PivObjectId::MANIFEST).chain(SecretName::iter().map(SecretName::object_id))
    }
}

/// YubiKey bootstrap に保存できる secret 名を表す閉じた集合。
///
/// 各 variant は固定の PIV object ID と blob secret id を持ち、storage version 1 ではその対応を変更しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretName {
    /// Bitwarden login email。
    BwEmail,
    /// Bitwarden login password。
    BwPassword,
    /// Bitwarden Secrets Manager access token。
    BwsAccessToken,
}

/// secret blob と PIV object の対応規則を固定した storage domain object。
///
/// adapter はこの値を外部 I/O の指定として受け取り、secret id / AAD / plaintext 制約の組立を行わない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretStorageSpec {
    /// 保存対象 secret の domain 名。
    pub name: SecretName,
    /// PIV data object ID。
    pub object_id: PivObjectId,
    /// encrypted blob header に保存する secret id。
    pub secret_id: u8,
    /// AEAD additional data。
    pub additional_data: Vec<u8>,
    /// 保存する plaintext secret の最小 byte 長。
    pub minimum_plaintext_len: usize,
}

impl SecretName {
    /// summary と verify で使う secret 名を安定順で列挙する。
    ///
    /// 列挙順は report と object 配置の期待順であり、version を上げずに並びを変えてはならない。
    pub fn iter() -> impl Iterator<Item = Self> {
        [Self::BwEmail, Self::BwPassword, Self::BwsAccessToken].into_iter()
    }

    /// binary blob header に保存する固定 secret id を返す。
    ///
    /// version 1 では各 variant と id の対応は不変で、互換性を壊す変更は version 更新なしに行ってはならない。
    pub fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    /// secret ごとに割り当てた PIV data object ID を返す。
    ///
    /// object ID は storage layout の一部であり、既存 device の互換性を保つため固定される。
    pub fn object_id(self) -> PivObjectId {
        match self {
            Self::BwEmail => PivObjectId(0x005f_ff17),
            Self::BwPassword => PivObjectId(0x005f_ff18),
            Self::BwsAccessToken => PivObjectId(0x005f_ff19),
        }
    }

    /// 既存 secret object への書き込みが許可されるかを確認する。
    ///
    /// `force` なしで既存値を置き換えない方針は secret storage の domain policy であり、
    /// application は object の存在有無を渡してこの規則を適用する。
    pub fn ensure_write_allowed(self, object_exists: bool, force: bool) -> crate::Result<()> {
        if object_exists && !force {
            anyhow::bail!("{self} already exists; pass --force to replace it");
        }
        Ok(())
    }

    /// secret 名ごとの入力取得 capability を選択する。
    ///
    /// どの secret がどの入力 capability に対応するかは domain rule であり、
    /// adapter はこの分岐を再実装せず、選ばれた capability の I/O 翻訳だけを担う。
    pub fn read_interactive_secret_with<T>(
        self,
        read_bw_email: impl FnOnce() -> crate::Result<T>,
        read_bw_password: impl FnOnce() -> crate::Result<T>,
        read_bws_access_token: impl FnOnce() -> crate::Result<T>,
    ) -> crate::Result<T> {
        match self {
            Self::BwEmail => read_bw_email(),
            Self::BwPassword => read_bw_password(),
            Self::BwsAccessToken => read_bws_access_token(),
        }
    }

    /// AEAD additional data に使う保存 context bytes を構築する。
    ///
    /// version、secret id、object ID、device serial を束ね、blob の差し替え検知に使う。
    /// 呼び出し側は同じ blob を復号する際に同一 serial と secret 名を渡す責務を負う。
    pub fn additional_data(self, serial: u32) -> Vec<u8> {
        [
            &[STORAGE_BLOB_VERSION, self.secret_id()][..],
            &self.object_id().to_be_bytes(),
            &serial.to_be_bytes(),
        ]
        .concat()
    }

    /// 指定 device serial 上でこの secret を保存・復号するための規則を構築する。
    pub fn storage_spec(self, serial: u32) -> SecretStorageSpec {
        SecretStorageSpec {
            name: self,
            object_id: self.object_id(),
            secret_id: self.secret_id(),
            additional_data: self.additional_data(serial),
            minimum_plaintext_len: 1,
        }
    }
}

impl SecretStorageSpec {
    /// 指定 serial における全 secret storage spec を安定順で返す。
    ///
    /// 保存対象集合と各 object/spec の対応は storage domain rule であり、use case は
    /// 個別の `SecretName` から対応関係を再構築せず、この集合を順序制御へ適用する。
    pub fn all_for_serial(serial: u32) -> [Self; 3] {
        [
            SecretName::BwEmail.storage_spec(serial),
            SecretName::BwPassword.storage_spec(serial),
            SecretName::BwsAccessToken.storage_spec(serial),
        ]
    }

    /// 保存対象 plaintext がこの spec の値制約を満たすことを確認する。
    pub fn ensure_plaintext_len(&self, len: usize) -> crate::Result<()> {
        if len < self.minimum_plaintext_len {
            anyhow::bail!("{} must not be empty", self.name);
        }
        Ok(())
    }

    /// この spec に対応する保存済み secret が欠落した domain error を返す。
    pub fn missing_error(&self) -> anyhow::Error {
        anyhow::anyhow!("{} is not stored on this YubiKey", self.name)
    }

    /// この spec に対応する encrypted blob の復号失敗を domain error へ変換する。
    pub fn decode_error(&self, error: anyhow::Error) -> anyhow::Error {
        anyhow::anyhow!("failed to decode {}: {error}", self.name)
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        })
    }
}

impl FromStr for SecretName {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "bw-email" => Ok(Self::BwEmail),
            "bw-password" => Ok(Self::BwPassword),
            "bws-access-token" => Ok(Self::BwsAccessToken),
            _ => Err(format!("unsupported YubiKey secret name: {value}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_storage_setup_preconditions_accept_minimum_version_with_pin_retries() {
        let result = validate_secret_storage_setup_preconditions(
            PivApplicationVersion::minimum_for_secret_storage(),
            1,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn secret_storage_setup_preconditions_reject_old_piv_version() {
        let result = validate_secret_storage_setup_preconditions(
            PivApplicationVersion {
                major: 5,
                minor: 2,
                patch: 9,
            },
            1,
        );

        assert!(result.is_err());
    }

    #[test]
    fn secret_storage_setup_preconditions_reject_exhausted_pin_retries() {
        let result = validate_secret_storage_setup_preconditions(
            PivApplicationVersion::minimum_for_secret_storage(),
            0,
        );

        assert!(result.is_err());
    }

    #[test]
    fn secret_name_rejects_unknown_name() {
        let parsed = "github-token".parse::<SecretName>();

        assert!(parsed.is_err());
    }

    #[test]
    fn secret_names_match_design_object_mapping() {
        let objects = SecretName::iter()
            .map(|name| (name.to_string(), name.object_id().to_string()))
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            objects.get("bw-email").map(String::as_str),
            Some("0x005FFF17")
        );
        assert_eq!(
            objects.get("bw-password").map(String::as_str),
            Some("0x005FFF18")
        );
        assert_eq!(
            objects.get("bws-access-token").map(String::as_str),
            Some("0x005FFF19")
        );
    }
}
