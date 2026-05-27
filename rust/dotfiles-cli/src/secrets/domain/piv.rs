use std::{fmt, str::FromStr};

const STORAGE_BLOB_VERSION: u8 = 1;

/// YubiKey PIV PIN の最小 byte 長。
pub const PIV_PIN_MIN_LEN: usize = 6;
/// YubiKey PIV PIN の最大 byte 長。
pub const PIV_PIN_MAX_LEN: usize = 8;
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
    /// エラー表示で使う安定 secret 名。
    pub label: String,
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

    /// PIV data object ID に対応する secret 名を返す。
    ///
    /// object ID と secret 名の逆引きも storage layout の一部なので、adapter は
    /// 独自に `object_id` 対応を再実装せずこの domain 規則を使う。
    #[cfg(feature = "secrets-test-stub")]
    pub fn from_object_id(object_id: PivObjectId) -> Option<Self> {
        Self::iter().find(|name| name.object_id() == object_id)
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

    /// 対話入力時に可視入力を許可する secret かどうかを返す。
    ///
    /// 可視入力可否は secret の意味に依存する domain rule で、端末 I/O 実装の詳細は含まない。
    pub fn uses_visible_input(self) -> bool {
        matches!(self, Self::BwEmail)
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
            label: self.to_string(),
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
