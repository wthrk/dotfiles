//! PIV 制約と object 対応規則を domain で固定し、adapter ごとの差異で保存意味が変わることを防ぐ。

use std::{fmt, str::FromStr};

const STORAGE_BLOB_VERSION: u8 = 1;
const MIN_PIV_METADATA_VERSION: PivApplicationVersion = PivApplicationVersion {
    major: 5,
    minor: 3,
    patch: 0,
};
const FIPS_PIN_NEVER_UNSUPPORTED_FROM: PivApplicationVersion = PivApplicationVersion {
    major: 5,
    minor: 7,
    patch: 1,
};

/// Management GET DEVICE INFORMATION が返す device class と firmware の raw facts。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PivDeviceProfile {
    pub version: PivApplicationVersion,
    pub fips_series: bool,
}

impl PivDeviceProfile {
    /// PIN-free recovery private-key を生成できる device range であることを確認する。
    ///
    /// Yubico は FIPS Series firmware 5.7.1 以降で `PinPolicy::Never` を禁止している。
    /// 根拠は [Yubico PIV PIN/touch policies](https://docs.yubico.com/yesdk/users-manual/application-piv/pin-touch-policies.html)
    /// の PIN policy compatibility 規定であり、SDK の opaque error から推測しない。
    /// 本 repository の recovery unwrap は追加入力なしを必須とするため、この組合せは key
    /// generation 前に fallback せず typed failure にする。これは管理操作の設定済み PIV PIN →
    /// VERIFY → protected management-key authentication を拒否する判定ではなく、管理 caller は
    /// この関数を PIN入力前の preflight として呼んではならない。version 単独、reader 名、serial
    /// から FIPS を推測しない。
    pub fn ensure_pin_free_recovery_supported(self) -> crate::Result<()> {
        if self.fips_series && self.version >= FIPS_PIN_NEVER_UNSUPPORTED_FROM {
            return Err(anyhow::Error::new(PivPinFreeRecoveryUnsupported {
                version: self.version,
            }));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PivPinFreeRecoveryUnsupported {
    version: PivApplicationVersion,
}

impl fmt::Display for PivPinFreeRecoveryUnsupported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "YubiKey FIPS Series firmware {} cannot provision the PIN-free recovery key policy",
            self.version
        )
    }
}

impl std::error::Error for PivPinFreeRecoveryUnsupported {}

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
) -> crate::Result<()> {
    let minimum = PivApplicationVersion::minimum_for_secret_storage();
    if version < minimum {
        anyhow::bail!("YubiKey PIV application version must be at least {minimum}");
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
    /// 列挙順は setup 判定と verification が参照する安定順で、呼び出し側はこの順序を変更前提に依存してはならない。
    pub fn iter() -> impl Iterator<Item = PivObjectId> {
        std::iter::once(PivObjectId::MANIFEST).chain(SecretName::iter().map(SecretName::object_id))
    }
}

/// YubiKey bootstrap に保存できる secret 名を表す閉じた集合。
///
/// 各 variant は固定の PIV object ID と blob secret id を持ち、storage version 1 ではその対応を変更しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretName {
    /// Bitwarden Secrets Manager access token。
    BitwardenClientSecret,
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
        [Self::BitwardenClientSecret].into_iter()
    }

    /// binary blob header に保存する固定 secret id を返す。
    ///
    /// version 1 では各 variant と id の対応は不変で、互換性を壊す変更は version 更新なしに行ってはならない。
    pub fn secret_id(self) -> u8 {
        match self {
            Self::BitwardenClientSecret => 1,
        }
    }

    /// secret ごとに割り当てた PIV data object ID を返す。
    ///
    /// object ID は storage layout の一部であり、既存 device の互換性を保つため固定される。
    pub fn object_id(self) -> PivObjectId {
        match self {
            Self::BitwardenClientSecret => PivObjectId(0x005f_ff19),
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
    pub fn all_for_serial(serial: u32) -> [Self; 1] {
        [SecretName::BitwardenClientSecret.storage_spec(serial)]
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
        error.context(format!("failed to decode {}", self.name))
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BitwardenClientSecret => "bitwarden-client-secret",
        })
    }
}

impl FromStr for SecretName {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "bitwarden-client-secret" => Ok(Self::BitwardenClientSecret),
            _ => Err(format!("unsupported YubiKey secret name: {value}")),
        }
    }
}

/// PIV PIN の repository 値制約を検証する。
///
/// hidden input や prompt 順序は entrypoint、byte 保護は process primitive が担う。この domain
/// rule は Yubico PIV PIN の許容範囲として採用した 6--8 ASCII alphanumeric bytes だけを決める。
pub(crate) fn validate_piv_pin_properties(
    byte_len: usize,
    is_ascii_alphanumeric: bool,
) -> crate::Result<()> {
    if !(6..=8).contains(&byte_len) || !is_ascii_alphanumeric {
        anyhow::bail!("YubiKey PIV PIN must contain 6 to 8 ASCII alphanumeric bytes");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piv_pin_properties_accept_only_six_to_eight_ascii_alphanumeric_bytes() {
        assert!(validate_piv_pin_properties(6, true).is_ok());
        assert!(validate_piv_pin_properties(8, true).is_ok());
        assert!(validate_piv_pin_properties(5, true).is_err());
        assert!(validate_piv_pin_properties(9, true).is_err());
        assert!(validate_piv_pin_properties(6, false).is_err());
    }

    #[test]
    fn secret_storage_setup_preconditions_reject_old_piv_version() {
        let result = validate_secret_storage_setup_preconditions(PivApplicationVersion {
            major: 5,
            minor: 2,
            patch: 9,
        });

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
            objects.get("bitwarden-client-secret").map(String::as_str),
            Some("0x005FFF19")
        );
    }
}
