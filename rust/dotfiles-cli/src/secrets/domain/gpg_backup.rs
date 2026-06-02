//! `gpg-secret-key-backup` encrypted envelope の schema・検証・照合を担う domain 層。
//!
//! envelope は UTF-8 JSON で `version` / `metadata` / `recipients` / `ciphertext` を保持する。
//! 固定 version・固定アルゴリズム・fingerprint 形式・nonce/tag byte 長・recipient 件数・
//! base64/hex 妥当性といった保存可能条件と、接続中 YubiKey と recipient の照合規則は、
//! gpgme / YubiKey unwrap / BWS 取得などの外部実装を差し替えても変わらない業務規則である
//! ため、この module に閉じる。鍵リング実装・process I/O・ハードウェア依存は持たず、
//! 純粋な値・検証・照合だけを扱う。secret 平文や復号済み backup はこの層へ載せない。
//!
//! この型群は domain 層のみを実装する増分で追加されており、`restore-gpg` / BWS recipient
//! 更新などの consumer 配線は後続増分で行う。それまでは domain value の検証済み読み取り面が
//! 未使用となるため、module 単位で `dead_code` を許容する（consumer 配線時に解消する）。
#![allow(
    dead_code,
    reason = "restore-gpg / BWS recipient consumers wire a subset of these validated read accessors; \
              the remainder stay as domain read surface for the export / spare-update paths"
)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Result;

/// envelope が固定する schema version。
const ENVELOPE_VERSION: u8 = 1;
/// `metadata.dek_alg` の固定値。
const DEK_ALG: &str = "aes-256-gcm";
/// `metadata.recipient_kek_alg` の固定値。
const RECIPIENT_KEK_ALG: &str = "rsa-oaep-sha256";
/// recipient の固定 PIV slot（正本 BWS schema に合わせ文字列固定）。
const RECIPIENT_PIV_SLOT: &str = "82";
/// `metadata.primary_fingerprint` の lowercase hex 文字数（区切りなし）。
const PRIMARY_FINGERPRINT_HEX_LEN: usize = 40;
/// recipient `public_key_fingerprint` の lowercase hex 文字数（区切りなし）。
const PUBLIC_KEY_FINGERPRINT_HEX_LEN: usize = 64;
/// AES-GCM nonce の byte 長。
const NONCE_LEN: usize = 12;
/// AES-GCM authentication tag の byte 長。
const TAG_LEN: usize = 16;

/// schema 検証に成功した `gpg-secret-key-backup` encrypted envelope の domain 表現。
///
/// 構築は [`GpgBackupEnvelope::from_json`] / [`GpgBackupEnvelope::parse`] を通すことを保証し、
/// 固定 version・固定アルゴリズム・形式・byte 長・件数を満たした値だけがこの型になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpgBackupEnvelope {
    metadata: EnvelopeMetadata,
    recipients: Vec<EnvelopeRecipient>,
    ciphertext: EnvelopeCiphertext,
}

/// envelope `metadata` の検証済み domain 表現。
///
/// `exported_at` は構築時に UTC RFC3339 形式（`YYYY-MM-DDThh:mm:ss[.fff]Z`、UTC を表す
/// `Z` または `+00:00`）として検証済みの文字列だけを保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeMetadata {
    primary_fingerprint: PrimaryFingerprint,
    exported_at: String,
}

/// envelope `ciphertext` の検証済み domain 表現。
///
/// `nonce` は 12 bytes、`tag` は 16 bytes に正規化済みで、`tag` は `body` へ連結しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeCiphertext {
    nonce: Vec<u8>,
    body: Vec<u8>,
    tag: Vec<u8>,
}

/// envelope `recipients` 要素の検証済み domain 表現。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeRecipient {
    yubikey_serial: String,
    public_key_fingerprint: PublicKeyFingerprint,
    wrapped_dek: Vec<u8>,
}

/// primary key fingerprint を lowercase hex 40 文字（区切りなし）に正規化した値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryFingerprint(String);

/// PIV slot 公開鍵 fingerprint を lowercase hex 64 文字（区切りなし）に正規化した値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyFingerprint(String);

/// 接続中 YubiKey を recipient 照合へ渡すための識別子。
///
/// `serial` と `public_key_fingerprint` の両方一致だけを成立条件とし、片方のみ一致は
/// 不正として解決しない recipient matching の入力境界値である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedYubiKey {
    serial: String,
    public_key_fingerprint: PublicKeyFingerprint,
}

/// envelope JSON の wire 表現。検証前の raw 文字列をそのまま受け取る。
///
/// `deny_unknown_fields` により、設計が定める top-level/ネスト field 以外が混入した
/// envelope を deserialize 段階で拒否し、domain 検証失敗として停止させる。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeWire {
    version: u8,
    metadata: MetadataWire,
    recipients: Vec<RecipientWire>,
    ciphertext: CiphertextWire,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWire {
    primary_fingerprint: String,
    exported_at: String,
    dek_alg: String,
    recipient_kek_alg: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CiphertextWire {
    nonce: String,
    body: String,
    tag: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipientWire {
    yubikey_serial: String,
    piv_slot: String,
    public_key_fingerprint: String,
    wrapped_dek: String,
}

impl GpgBackupEnvelope {
    /// UTF-8 JSON bytes を decode し、schema を検証した envelope を構築する。
    ///
    /// JSON 構造の破損・`version` 不一致・固定アルゴリズム不一致・`exported_at` の
    /// UTC RFC3339 形式違反・recipients 0 件・fingerprint 形式違反・nonce/tag byte 長違反・
    /// base64/hex 妥当性違反は domain error として停止する。
    /// 失敗 message に secret 本文や平文は含めない。
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let wire: EnvelopeWire = serde_json::from_slice(bytes)
            .map_err(|_| invalid_data("failed to parse gpg backup envelope JSON"))?;
        Self::from_wire(wire)
    }

    /// UTF-8 JSON 文字列を decode し、schema を検証した envelope を構築する。
    pub fn parse(json: &str) -> Result<Self> {
        Self::from_json(json.as_bytes())
    }

    /// 検証済み envelope を UTF-8 JSON bytes へ serialize する。
    ///
    /// 出力は `version` 固定・固定アルゴリズム・canonical fingerprint・base64 ciphertext を持つ
    /// canonical な envelope である。fingerprint は構築時に canonical 検証済みの保存値を
    /// 無変換でそのまま出力し、書き換えない。
    pub fn to_json(&self) -> Result<Vec<u8>> {
        let wire = EnvelopeWire {
            version: ENVELOPE_VERSION,
            metadata: MetadataWire {
                primary_fingerprint: self.metadata.primary_fingerprint.0.clone(),
                exported_at: self.metadata.exported_at.clone(),
                dek_alg: DEK_ALG.to_owned(),
                recipient_kek_alg: RECIPIENT_KEK_ALG.to_owned(),
            },
            recipients: self
                .recipients
                .iter()
                .map(|recipient| RecipientWire {
                    yubikey_serial: recipient.yubikey_serial.clone(),
                    piv_slot: RECIPIENT_PIV_SLOT.to_owned(),
                    public_key_fingerprint: recipient.public_key_fingerprint.0.clone(),
                    wrapped_dek: base64_encode(&recipient.wrapped_dek),
                })
                .collect(),
            ciphertext: CiphertextWire {
                nonce: base64_encode(&self.ciphertext.nonce),
                body: base64_encode(&self.ciphertext.body),
                tag: base64_encode(&self.ciphertext.tag),
            },
        };
        serde_json::to_vec(&wire)
            .map_err(|_| invalid_data("failed to serialize gpg backup envelope JSON"))
    }

    /// wire 表現を schema 検証して domain envelope へ変換する。
    fn from_wire(wire: EnvelopeWire) -> Result<Self> {
        if wire.version != ENVELOPE_VERSION {
            return Err(invalid_data(format!(
                "unsupported gpg backup envelope version: expected {ENVELOPE_VERSION}, found {}",
                wire.version
            )));
        }

        let metadata = EnvelopeMetadata::from_wire(wire.metadata)?;
        let ciphertext = EnvelopeCiphertext::from_wire(wire.ciphertext)?;

        if wire.recipients.is_empty() {
            return Err(invalid_data(
                "gpg backup envelope must have at least one recipient",
            ));
        }
        let recipients = wire
            .recipients
            .into_iter()
            .map(EnvelopeRecipient::from_wire)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            metadata,
            recipients,
            ciphertext,
        })
    }

    /// envelope `metadata` を借用する。
    pub fn metadata(&self) -> &EnvelopeMetadata {
        &self.metadata
    }

    /// envelope `recipients` を借用する。
    pub fn recipients(&self) -> &[EnvelopeRecipient] {
        &self.recipients
    }

    /// envelope `ciphertext` を借用する。
    pub fn ciphertext(&self) -> &EnvelopeCiphertext {
        &self.ciphertext
    }

    /// 接続中 YubiKey に一致する recipient を解決する。
    ///
    /// `yubikey_serial` と `public_key_fingerprint` の両方が一致する recipient だけを返す。
    /// 片方のみ一致は不正であり解決しない（後続の DEK unwrap へ進ませない停止条件）。
    /// 一致が無い場合は domain failure を返す。
    pub fn resolve_recipient(&self, connected: &ConnectedYubiKey) -> Result<&EnvelopeRecipient> {
        self.recipients
            .iter()
            .find(|recipient| recipient.matches(connected))
            .ok_or_else(|| invalid_data("no gpg backup recipient matches the connected YubiKey"))
    }

    /// 検証済みの構成要素から envelope を直接組み立てる。
    ///
    /// backup export + envelope 化（primary 登録）で使う。`metadata` / `ciphertext` / `recipients`
    /// は構築済み domain 値であり、recipients が空でないことだけをここで再確認する。schema 検証は
    /// 各構成要素の構築時に完了している。
    pub fn assemble(
        metadata: EnvelopeMetadata,
        recipients: Vec<EnvelopeRecipient>,
        ciphertext: EnvelopeCiphertext,
    ) -> Result<Self> {
        if recipients.is_empty() {
            return Err(invalid_data(
                "gpg backup envelope must have at least one recipient",
            ));
        }
        Ok(Self {
            metadata,
            recipients,
            ciphertext,
        })
    }

    /// 既存 envelope へ spare recipient を追加した新しい envelope を返す。
    ///
    /// 設計「recipient 運用 / BWS 更新契約」の spare 追加に対応する。`ciphertext` と `metadata` は
    /// 変更せず、同一 serial の recipient が既にある場合は重複登録を停止条件として拒否する。
    /// caller は同一 DEK を spare recipient 公開鍵で wrap した `wrapped_dek` を渡す責務を負う。
    pub fn with_added_recipient(&self, recipient: EnvelopeRecipient) -> Result<Self> {
        if self
            .recipients
            .iter()
            .any(|existing| existing.yubikey_serial == recipient.yubikey_serial)
        {
            return Err(invalid_data(
                "gpg backup envelope already has a recipient for this YubiKey serial",
            ));
        }
        let mut recipients = self.recipients.clone();
        recipients.push(recipient);
        Ok(Self {
            metadata: self.metadata.clone(),
            recipients,
            ciphertext: self.ciphertext.clone(),
        })
    }
}

impl EnvelopeMetadata {
    /// `metadata` wire を検証する。固定アルゴリズム・`exported_at` の UTC RFC3339 形式、
    /// および `primary_fingerprint` が既に canonical（lowercase hex 40, 区切り・空白なし）で
    /// あることを強制する。保存値は正規化せず、非 canonical なら停止する。
    fn from_wire(wire: MetadataWire) -> Result<Self> {
        if wire.dek_alg != DEK_ALG {
            return Err(invalid_data(format!(
                "unsupported gpg backup dek_alg: expected {DEK_ALG}"
            )));
        }
        if wire.recipient_kek_alg != RECIPIENT_KEK_ALG {
            return Err(invalid_data(format!(
                "unsupported gpg backup recipient_kek_alg: expected {RECIPIENT_KEK_ALG}"
            )));
        }
        validate_rfc3339_utc(&wire.exported_at)?;

        Ok(Self {
            primary_fingerprint: PrimaryFingerprint::from_wire(&wire.primary_fingerprint)?,
            exported_at: wire.exported_at,
        })
    }

    /// 検証済みの primary fingerprint と `exported_at` から metadata を構築する。
    ///
    /// `exported_at` は UTC RFC3339 形式（[`validate_rfc3339_utc`]）を満たす場合だけ受理する。
    /// 固定アルゴリズム（`dek_alg` / `recipient_kek_alg`）は serialize 時に付与するため、
    /// この型は不変な意味（どの primary key の、いつ時点の backup か）だけを保持する。
    pub fn new(
        primary_fingerprint: PrimaryFingerprint,
        exported_at: impl Into<String>,
    ) -> Result<Self> {
        let exported_at = exported_at.into();
        validate_rfc3339_utc(&exported_at)?;
        Ok(Self {
            primary_fingerprint,
            exported_at,
        })
    }

    /// primary key fingerprint を借用する。
    pub fn primary_fingerprint(&self) -> &PrimaryFingerprint {
        &self.primary_fingerprint
    }

    /// 検証済みの `exported_at`（UTC RFC3339 文字列）を借用する。
    ///
    /// 構築時に [`validate_rfc3339_utc`] を通っているため、`YYYY-MM-DDThh:mm:ss[.fff]Z`
    /// 形式で各フィールドが数値範囲内（full-date は暦日として存在する日付）かつ
    /// UTC（`Z` または `+00:00`）であることが保証される。
    pub fn exported_at(&self) -> &str {
        &self.exported_at
    }
}

impl EnvelopeCiphertext {
    /// `ciphertext` wire を検証する。nonce/tag の byte 長と base64 妥当性を強制する。
    fn from_wire(wire: CiphertextWire) -> Result<Self> {
        let nonce = base64_decode(&wire.nonce, "ciphertext.nonce")?;
        if nonce.len() != NONCE_LEN {
            return Err(invalid_data(format!(
                "gpg backup ciphertext.nonce must decode to {NONCE_LEN} bytes"
            )));
        }
        let body = base64_decode(&wire.body, "ciphertext.body")?;
        if body.is_empty() {
            return Err(invalid_data("gpg backup ciphertext.body must not be empty"));
        }
        let tag = base64_decode(&wire.tag, "ciphertext.tag")?;
        if tag.len() != TAG_LEN {
            return Err(invalid_data(format!(
                "gpg backup ciphertext.tag must decode to {TAG_LEN} bytes"
            )));
        }

        Ok(Self { nonce, body, tag })
    }

    /// 暗号化済み構成要素から ciphertext を構築する。
    ///
    /// nonce 12 bytes / tag 16 bytes / 非空 body という保存可能条件を構築時に強制し、
    /// 不正長は domain failure として停止する。`body` は DEK で暗号化済みの backup bytes、
    /// `tag` は `body` へ連結しない detached tag とする。
    pub fn new(nonce: Vec<u8>, body: Vec<u8>, tag: Vec<u8>) -> Result<Self> {
        if nonce.len() != NONCE_LEN {
            return Err(invalid_data(format!(
                "gpg backup ciphertext.nonce must be {NONCE_LEN} bytes"
            )));
        }
        if body.is_empty() {
            return Err(invalid_data("gpg backup ciphertext.body must not be empty"));
        }
        if tag.len() != TAG_LEN {
            return Err(invalid_data(format!(
                "gpg backup ciphertext.tag must be {TAG_LEN} bytes"
            )));
        }
        Ok(Self { nonce, body, tag })
    }

    /// AES-GCM nonce（12 bytes）を借用する。
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    /// 暗号化済み backup body を借用する。`tag` は連結されていない。
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// AES-GCM authentication tag（16 bytes）を借用する。
    pub fn tag(&self) -> &[u8] {
        &self.tag
    }
}

impl EnvelopeRecipient {
    /// `recipients` 要素 wire を検証する。PIV slot 固定値・base64 妥当性に加え、
    /// `public_key_fingerprint` が既に canonical（lowercase hex 64, 区切り・空白なし）で
    /// あることを強制する。保存値は正規化せず、非 canonical なら停止する。
    fn from_wire(wire: RecipientWire) -> Result<Self> {
        if wire.piv_slot != RECIPIENT_PIV_SLOT {
            return Err(invalid_data(format!(
                "gpg backup recipient piv_slot must be the string {RECIPIENT_PIV_SLOT:?}"
            )));
        }
        if wire.yubikey_serial.is_empty()
            || !wire
                .yubikey_serial
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid_data(
                "gpg backup recipient yubikey_serial must be a decimal string",
            ));
        }
        let wrapped_dek = base64_decode(&wire.wrapped_dek, "recipient.wrapped_dek")?;
        if wrapped_dek.is_empty() {
            return Err(invalid_data(
                "gpg backup recipient wrapped_dek must not be empty",
            ));
        }

        Ok(Self {
            yubikey_serial: wire.yubikey_serial,
            public_key_fingerprint: PublicKeyFingerprint::from_wire(&wire.public_key_fingerprint)?,
            wrapped_dek,
        })
    }

    /// 接続中 YubiKey の照合値と wrap 済み DEK から recipient を構築する。
    ///
    /// backup export（primary 登録）と spare 追加で使う。`serial` は 10 進文字列、
    /// `public_key_fingerprint` は既に lowercase hex 64 文字へ正規化済みの domain 値、
    /// `wrapped_dek` は RSA-OAEP-SHA256 で wrap した非空 bytes でなければならない。
    /// 値そのものは構築時にこの module の保存可能条件で再検証する。
    pub fn new(connected: &ConnectedYubiKey, wrapped_dek: Vec<u8>) -> Result<Self> {
        if wrapped_dek.is_empty() {
            return Err(invalid_data(
                "gpg backup recipient wrapped_dek must not be empty",
            ));
        }
        Ok(Self {
            yubikey_serial: connected.serial.clone(),
            public_key_fingerprint: connected.public_key_fingerprint.clone(),
            wrapped_dek,
        })
    }

    /// recipient の `yubikey_serial`（10 進文字列）を借用する。
    pub fn yubikey_serial(&self) -> &str {
        &self.yubikey_serial
    }

    /// recipient PIV slot 公開鍵 fingerprint を借用する。
    pub fn public_key_fingerprint(&self) -> &PublicKeyFingerprint {
        &self.public_key_fingerprint
    }

    /// recipient へ DEK を wrap した bytes を借用する。
    pub fn wrapped_dek(&self) -> &[u8] {
        &self.wrapped_dek
    }

    /// 接続中 YubiKey と serial・fingerprint の両方が一致するかを判定する。
    fn matches(&self, connected: &ConnectedYubiKey) -> bool {
        self.yubikey_serial == connected.serial
            && self.public_key_fingerprint == connected.public_key_fingerprint
    }
}

impl ConnectedYubiKey {
    /// 接続中 YubiKey の serial と公開鍵 fingerprint から照合入力を構築する。
    ///
    /// `serial` は 10 進文字列、`public_key_fingerprint` は大文字小文字・区切り混在を許容し、
    /// lowercase hex 64 文字へ正規化したうえで保持する。
    pub fn new(serial: impl Into<String>, public_key_fingerprint: &str) -> Result<Self> {
        let serial = serial.into();
        if serial.is_empty() || !serial.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_data(
                "connected YubiKey serial must be a decimal string",
            ));
        }
        Ok(Self {
            serial,
            public_key_fingerprint: PublicKeyFingerprint::parse(public_key_fingerprint)?,
        })
    }
}

/// BWS の `gpg-secret-key-backup` secret を read-modify-write で更新するときに、
/// 更新前後の現行値が変化していないことを確認する stale overwrite 防止 guard。
///
/// 設計「recipient 運用 / BWS 更新契約」は、更新識別子（revision / updatedAt / ETag 相当）が
/// 取得できればそれを、取得できなければ最初に読み出した exact UTF-8 secret value bytes の
/// SHA-256 digest を既知値として保持し、更新直前に再取得した現行値の更新識別子（または digest）が
/// 一致する場合だけ上書きすることを要求する。`version` と `metadata.primary_fingerprint` だけを
/// 防止条件に使ってはならない。この判定は SDK 実装を差し替えても変わらない業務規則であるため
/// domain に閉じる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupUpdateGuard {
    /// SDK が提供する更新識別子（revision / updatedAt / ETag 相当）。
    Revision(String),
    /// 更新識別子を取得できない場合の、exact secret value bytes の SHA-256 digest（lowercase hex）。
    ValueDigest(String),
}

impl BackupUpdateGuard {
    /// SDK 更新識別子から guard を作る。空文字列は識別子として扱わず digest fallback を促す。
    pub fn from_revision(revision: impl Into<String>) -> Option<Self> {
        let revision = revision.into();
        if revision.is_empty() {
            None
        } else {
            Some(Self::Revision(revision))
        }
    }

    /// exact secret value bytes の SHA-256 digest から guard を作る（更新識別子 fallback）。
    pub fn from_value_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        Self::ValueDigest(hex)
    }

    /// 更新直前に再取得した現行 guard と一致する場合だけ上書きを許可する。
    ///
    /// 一致しない場合は stale overwrite として停止条件にする。識別子の種類（revision / digest）が
    /// 異なる場合も不一致として扱い、`version` / `primary_fingerprint` だけで判断させない。
    pub fn ensure_matches(&self, current: &Self) -> Result<()> {
        if self == current {
            Ok(())
        } else {
            Err(invalid_data(
                "gpg backup secret changed since it was read; refusing to overwrite (stale update)",
            ))
        }
    }
}

impl PrimaryFingerprint {
    /// 大文字小文字・区切り混在入力を lowercase hex 40 文字へ正規化し、長さと文字種を検証する。
    ///
    /// runtime 由来の入力向け。wire（envelope JSON 由来の保存値）には [`Self::from_wire`] を使う。
    pub fn parse(value: &str) -> Result<Self> {
        Ok(Self(normalize_fingerprint(
            value,
            PRIMARY_FINGERPRINT_HEX_LEN,
            "primary_fingerprint",
        )?))
    }

    /// wire（envelope JSON 由来の保存値）が既に canonical（lowercase hex 40, 区切り・空白なし）
    /// であることを厳格検証して構築する。非 canonical は正規化せず停止する。
    fn from_wire(value: &str) -> Result<Self> {
        Ok(Self(validate_canonical_wire_fingerprint(
            value,
            PRIMARY_FINGERPRINT_HEX_LEN,
            "metadata.primary_fingerprint",
        )?))
    }

    /// 正規化済み lowercase hex 文字列を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PublicKeyFingerprint {
    /// 大文字小文字・区切り混在入力を lowercase hex 64 文字へ正規化し、長さと文字種を検証する。
    ///
    /// runtime 由来の入力向け。wire（envelope JSON 由来の保存値）には [`Self::from_wire`] を使う。
    pub fn parse(value: &str) -> Result<Self> {
        Ok(Self(normalize_fingerprint(
            value,
            PUBLIC_KEY_FINGERPRINT_HEX_LEN,
            "public_key_fingerprint",
        )?))
    }

    /// wire（envelope JSON 由来の保存値）が既に canonical（lowercase hex 64, 区切り・空白なし）
    /// であることを厳格検証して構築する。非 canonical は正規化せず停止する。
    fn from_wire(value: &str) -> Result<Self> {
        Ok(Self(validate_canonical_wire_fingerprint(
            value,
            PUBLIC_KEY_FINGERPRINT_HEX_LEN,
            "recipient.public_key_fingerprint",
        )?))
    }

    /// 正規化済み lowercase hex 文字列を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `metadata.exported_at` が UTC RFC3339 形式であることを schema 検証する。
///
/// 設計「決定事項」は `exported_at` を UTC RFC3339 の必須 metadata と規定する。実時刻取得や
/// timezone DB は不要なため、外部 crate に依存せず純粋関数で形式（`YYYY-MM-DDThh:mm:ss[.fff]`）と
/// 各フィールドの数値範囲（full-date は月別日数と閏年判定を含む暦日妥当性）、および UTC を表す
/// time-offset（`Z` または `+00:00`）だけを検証する。
/// `Z` は RFC3339 に従い大文字小文字を問わず受理し、UTC 以外の offset は拒否する。秒小数部
/// （`.` 以降）は 1 桁以上の数字を許容する。秒は `0..=59` のみ許可し、leap second（秒 `60`）は
/// 受理しない。RFC3339 §5.7 は leap second の表現として秒 `60` を許すが、`exported_at` は本ツールが
/// export 時刻に生成する wall-clock UTC timestamp であり、生成 timestamp に leap second を適用しない
/// 原則のため、秒 `60`（および `>= 61`）は一律停止する（可変な leap-second テーブルを domain 検証へ
/// 持ち込まない確定的厳格化）。形式違反は domain error として停止し、message に入力本文は含めない。
fn validate_rfc3339_utc(value: &str) -> Result<()> {
    let invalid =
        || invalid_data("gpg backup metadata.exported_at must be a UTC RFC3339 timestamp");

    // `date-time = full-date "T" full-time`。`T` は RFC3339 で大文字小文字を問わない。
    let (date, rest) = value.split_once(['T', 't']).ok_or_else(invalid)?;

    // full-date = 4DIGIT "-" 2DIGIT "-" 2DIGIT
    let date_fields: Vec<&str> = date.split('-').collect();
    let [year, month, day] = date_fields.as_slice() else {
        return Err(invalid());
    };
    let year = parse_fixed_width_number(year, 4).ok_or_else(invalid)?;
    let month = parse_fixed_width_number(month, 2).ok_or_else(invalid)?;
    let day = parse_fixed_width_number(day, 2).ok_or_else(invalid)?;
    if !(1..=12).contains(&month) || year < 1 {
        return Err(invalid());
    }
    // 月別日数と 2 月の閏年判定で暦日として存在しない日付（`2026-02-31` /
    // `2026-04-31` / 平年 `2025-02-29` 等）を拒否する。
    if day < 1 || day > days_in_month(year, month) {
        return Err(invalid());
    }

    // full-time = partial-time time-offset。time-offset の UTC 表現だけを受理する。
    let time = if let Some(stripped) = rest.strip_suffix(['Z', 'z']) {
        stripped
    } else if let Some(stripped) = rest.strip_suffix("+00:00") {
        stripped
    } else {
        return Err(invalid());
    };

    // partial-time = 2DIGIT ":" 2DIGIT ":" 2DIGIT [ "." 1*DIGIT ]
    let (hms, fraction) = match time.split_once('.') {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (time, None),
    };
    if let Some(fraction) = fraction
        && (fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let time_fields: Vec<&str> = hms.split(':').collect();
    let [hour, minute, second] = time_fields.as_slice() else {
        return Err(invalid());
    };
    let hour = parse_fixed_width_number(hour, 2).ok_or_else(invalid)?;
    let minute = parse_fixed_width_number(minute, 2).ok_or_else(invalid)?;
    let second = parse_fixed_width_number(second, 2).ok_or_else(invalid)?;
    if hour > 23 || minute > 59 {
        return Err(invalid());
    }
    // 秒は `0..=59` のみ許可する。RFC3339 §5.7 は leap second の表現として秒 `60` を許すが、
    // `exported_at` は本ツールが export 時刻に生成する wall-clock UTC timestamp であり、生成
    // timestamp に leap second を適用しないため、`60`（および `>= 61`）は一律停止する。これにより
    // 通常月・月末を問わず leap second 値の混入（`2026-05-31T23:59:60Z` /
    // `2026-12-31T23:59:60Z` / `2026-05-31T00:00:60Z` 等）を「UTC RFC3339 検証済み」表明から排除する。
    if second > 59 {
        return Err(invalid());
    }

    Ok(())
}

/// 指定した年月の暦日上の日数を返す。
///
/// 2 月は Gregorian の閏年判定（4 で割り切れ、かつ 100 で割り切れない、または 400 で
/// 割り切れる）で 28/29 を返す。`month` は呼び出し側で `1..=12` を保証済みとし、それ以外は
/// 安全側の最小日数（28）を返す。
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let is_leap =
                year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
            if is_leap { 29 } else { 28 }
        }
        _ => 28,
    }
}

/// 固定桁数の 10 進フィールドを数値へ変換する。桁数不一致・非数字・空は `None` を返す。
fn parse_fixed_width_number(field: &str, width: usize) -> Option<u32> {
    if field.len() != width || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value = 0u32;
    for byte in field.bytes() {
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

/// wire（envelope JSON 由来の保存値）の fingerprint が既に canonical であることを厳格検証する。
///
/// BWS から取得した envelope の `metadata.primary_fingerprint` / recipient
/// `public_key_fingerprint` は設計上「lowercase hex、指定長ちょうど、区切り・空白なし」の
/// schema 値である。保存値の正規化（書き換え）は破損を隠すため、この関数は [`normalize_fingerprint`]
/// と異なり大文字小文字変換・区切り除去を一切行わず、uppercase・`:` 等の区切り・空白・長さ
/// 不一致・非 hex を非 canonical として停止させる。`to_json` は保存値をそのまま（無変換で）
/// 出力するため、wire 値はこの関数を通った canonical 値だけを保持する。runtime 由来の照合入力
/// （[`ConnectedYubiKey`]）には適用せず、そちらは [`normalize_fingerprint`] を使う。
fn validate_canonical_wire_fingerprint(
    value: &str,
    expected_len: usize,
    field: &str,
) -> Result<String> {
    let is_canonical = value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !is_canonical {
        return Err(invalid_data(format!(
            "gpg backup {field} must be stored as exactly {expected_len} lowercase hex \
             characters with no separators"
        )));
    }
    Ok(value.to_owned())
}

/// fingerprint 入力を lowercase hex（区切りなし）へ正規化し、長さと文字種を検証する。
///
/// 区切り（空白・コロン）を除去し、hex 桁だけを lowercase 化して保持する。期待文字数と
/// 不一致、または hex 以外の文字が残る場合は domain error として停止する。runtime 由来の照合
/// 入力（[`ConnectedYubiKey`]）向けであり、wire 保存値の検証には
/// [`validate_canonical_wire_fingerprint`] を使う。
fn normalize_fingerprint(value: &str, expected_len: usize, field: &str) -> Result<String> {
    let mut normalized = String::with_capacity(expected_len);
    for ch in value.chars() {
        if ch.is_whitespace() || ch == ':' {
            continue;
        }
        if !ch.is_ascii_hexdigit() {
            return Err(invalid_data(format!(
                "gpg backup {field} must be hex; found a non-hex character"
            )));
        }
        normalized.push(ch.to_ascii_lowercase());
    }
    if normalized.len() != expected_len {
        return Err(invalid_data(format!(
            "gpg backup {field} must normalize to {expected_len} lowercase hex characters"
        )));
    }
    Ok(normalized)
}

/// standard base64（padding 必須）を decode する。
///
/// この domain は外部 base64 crate に依存しないため、保存可能条件の検証に必要な最小 decoder を
/// 純粋関数として持つ。padding・alphabet・長さの妥当性違反は domain error として停止する。
/// padding を含む最終 quantum では canonical 性（RFC 4648 §3.5）も検証し、出力に使われない
/// 余剰 sextet bit が 0 でない非 canonical 入力（`AB==` / `AAB=` 等）も停止させる。
fn base64_decode(input: &str, field: &str) -> Result<Vec<u8>> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(invalid_base64(field));
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunk_count = bytes.len() / 4;
    for (chunk_index, chunk) in bytes.chunks(4).enumerate() {
        let is_last_chunk = chunk_index + 1 == chunk_count;
        let mut accumulator = 0u32;
        let mut chunk_padding = 0usize;
        for (index, &symbol) in chunk.iter().enumerate() {
            if symbol == b'=' {
                // padding（`=`）は入力末尾の 4 文字 chunk の末尾位置にだけ許可する。
                // 末尾以外の chunk に padding が現れる入力（`AA==AAAA` 等、padding 後に
                // さらにデータが続く値）は壊れた envelope として停止する。
                if !is_last_chunk || index < 2 {
                    return Err(invalid_base64(field));
                }
                chunk_padding += 1;
                accumulator <<= 6;
                continue;
            }
            if chunk_padding != 0 {
                return Err(invalid_base64(field));
            }
            let sextet = base64_symbol_value(symbol).ok_or_else(|| invalid_base64(field))?;
            accumulator = (accumulator << 6) | u32::from(sextet);
        }
        let bytes_in_chunk = 3 - chunk_padding;
        // padding を含む最終 quantum では、出力に使われない余剰 sextet bit（RFC 4648 §3.5
        // が 0 を要求する canonical bit）が 0 でなければ拒否する。出力は 24-bit 値の上位
        // `bytes_in_chunk` byte で、捨てられる下位 `(3 - bytes_in_chunk) * 8` bit が
        // すべて 0 であることを確認する（2 padding なら下位 16 bit ＝ 2 番目 sextet の
        // 下位 4 bit、1 padding なら下位 8 bit ＝ 3 番目 sextet の下位 2 bit）。これにより
        // `AB==` / `AAB=` のような非 canonical base64 を schema 検証失敗として停止させる。
        let discarded_bits = (3 - bytes_in_chunk) * 8;
        if discarded_bits != 0 && accumulator & ((1u32 << discarded_bits) - 1) != 0 {
            return Err(invalid_base64(field));
        }
        let chunk_bytes = accumulator.to_be_bytes();
        // accumulator は 24-bit 値で、上位 byte（index 0）は常に 0。
        output.extend_from_slice(&chunk_bytes[1..=bytes_in_chunk]);
    }
    Ok(output)
}

/// standard base64（padding 付き）へ encode する。
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let value =
            (u32::from(buffer[0]) << 16) | (u32::from(buffer[1]) << 8) | u32::from(buffer[2]);
        let symbols = [
            ALPHABET[(value >> 18 & 0x3f) as usize],
            ALPHABET[(value >> 12 & 0x3f) as usize],
            ALPHABET[(value >> 6 & 0x3f) as usize],
            ALPHABET[(value & 0x3f) as usize],
        ];
        match chunk.len() {
            1 => {
                output.push(symbols[0] as char);
                output.push(symbols[1] as char);
                output.push('=');
                output.push('=');
            }
            2 => {
                output.push(symbols[0] as char);
                output.push(symbols[1] as char);
                output.push(symbols[2] as char);
                output.push('=');
            }
            _ => {
                for symbol in symbols {
                    output.push(symbol as char);
                }
            }
        }
    }
    output
}

/// standard base64 alphabet の 1 文字を 6-bit 値へ変換する。
fn base64_symbol_value(symbol: u8) -> Option<u8> {
    match symbol {
        b'A'..=b'Z' => Some(symbol - b'A'),
        b'a'..=b'z' => Some(symbol - b'a' + 26),
        b'0'..=b'9' => Some(symbol - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// base64 妥当性違反の domain error を構築する。secret 本文は含めない。
fn invalid_base64(field: &str) -> anyhow::Error {
    invalid_data(format!("gpg backup {field} is not valid base64"))
}

/// schema 検証失敗を `InvalidData` の domain error へ変換する。
///
/// message は schema 違反の説明だけを含め、secret 値・平文 backup・wrapped DEK を露出しない。
fn invalid_data(message: impl Into<String>) -> anyhow::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    //! `gpg-secret-key-backup` envelope の parse / validate / match 規則の単体テスト。
    //!
    //! 正常系と、設計「停止条件」に対応する異常系（version 不正・recipients 0 件・
    //! fingerprint 長/文字種不正・固定アルゴリズム不一致・nonce/tag 長不正・base64 不正・
    //! `exported_at` の UTC RFC3339 形式不正・片側のみ一致 recipient）を網羅する。
    //! test double は持ち込まず純粋ロジックだけを検証する。

    use super::*;

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";
    const PUBKEY_FP: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// 12-byte nonce fixture。tag/wrapped_dek の base64 と一意に区別できる連番 byte 列を使い、
    /// `String::replace` が `ciphertext.nonce` だけを対象にできるようにする（全 0 だと nonce と
    /// tag の base64 が先頭一致して取り違える）。
    fn nonce_bytes() -> [u8; NONCE_LEN] {
        let mut bytes = [0u8; NONCE_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = 0x10 + index as u8;
        }
        bytes
    }

    /// 16-byte tag fixture。nonce/wrapped_dek の base64 と一意に区別できる連番 byte 列を使う。
    fn tag_bytes() -> [u8; TAG_LEN] {
        let mut bytes = [0u8; TAG_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = 0x80 + index as u8;
        }
        bytes
    }

    /// `Result::Ok` を取り出す。workspace lint で禁止された `unwrap`/`expect` を使わずに、
    /// 失敗時は `panic!` でテストを停止する。
    fn ok<T>(result: Result<T>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected {context} to succeed: {error}"),
        }
    }

    /// 12-byte / 16-byte / 非空 body を base64 化した有効 ciphertext field を返す。
    fn valid_envelope_json() -> String {
        let nonce = base64_encode(&nonce_bytes());
        let body = base64_encode(b"encrypted-backup-bytes");
        let tag = base64_encode(&tag_bytes());
        let wrapped = base64_encode(b"wrapped-dek-bytes");
        format!(
            r#"{{
              "version": 1,
              "metadata": {{
                "primary_fingerprint": "{PRIMARY_FP}",
                "exported_at": "2026-05-31T00:00:00Z",
                "dek_alg": "aes-256-gcm",
                "recipient_kek_alg": "rsa-oaep-sha256"
              }},
              "recipients": [
                {{
                  "yubikey_serial": "12345678",
                  "piv_slot": "82",
                  "public_key_fingerprint": "{PUBKEY_FP}",
                  "wrapped_dek": "{wrapped}"
                }}
              ],
              "ciphertext": {{ "nonce": "{nonce}", "body": "{body}", "tag": "{tag}" }}
            }}"#
        )
    }

    /// 有効 envelope は parse・validate・recipient match を通過する。
    #[test]
    fn valid_envelope_parses_validates_and_matches() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");

        assert_eq!(
            envelope.metadata().primary_fingerprint().as_str(),
            PRIMARY_FP
        );
        assert_eq!(envelope.metadata().exported_at(), "2026-05-31T00:00:00Z");
        assert_eq!(envelope.ciphertext().nonce().len(), NONCE_LEN);
        assert_eq!(envelope.ciphertext().tag().len(), TAG_LEN);
        assert_eq!(envelope.ciphertext().body(), b"encrypted-backup-bytes");
        assert_eq!(envelope.recipients().len(), 1);

        let connected = ok(
            ConnectedYubiKey::new("12345678", PUBKEY_FP),
            "connected yubikey",
        );
        let recipient = ok(envelope.resolve_recipient(&connected), "recipient match");
        assert_eq!(recipient.yubikey_serial(), "12345678");
        assert_eq!(recipient.wrapped_dek(), b"wrapped-dek-bytes");
    }

    /// 検証済み envelope は canonical JSON へ round-trip する。
    #[test]
    fn valid_envelope_round_trips_through_json() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        let json = ok(envelope.to_json(), "serialize");
        let reparsed = ok(GpgBackupEnvelope::from_json(&json), "reparse");

        assert_eq!(envelope, reparsed);
    }

    /// `version` が 1 以外の envelope は停止条件として拒否する。
    #[test]
    fn rejects_unsupported_version() {
        let json = valid_envelope_json().replace("\"version\": 1", "\"version\": 2");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// recipients が 0 件の envelope は停止条件として拒否する。
    #[test]
    fn rejects_empty_recipients() {
        let nonce = base64_encode(&nonce_bytes());
        let body = base64_encode(b"encrypted-backup-bytes");
        let tag = base64_encode(&tag_bytes());
        let json = format!(
            r#"{{
              "version": 1,
              "metadata": {{
                "primary_fingerprint": "{PRIMARY_FP}",
                "exported_at": "2026-05-31T00:00:00Z",
                "dek_alg": "aes-256-gcm",
                "recipient_kek_alg": "rsa-oaep-sha256"
              }},
              "recipients": [],
              "ciphertext": {{ "nonce": "{nonce}", "body": "{body}", "tag": "{tag}" }}
            }}"#
        );

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// fingerprint の文字数が不足する場合は拒否する。
    #[test]
    fn rejects_short_primary_fingerprint() {
        let json = valid_envelope_json().replace(PRIMARY_FP, "0123456789abcdef");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// fingerprint に hex 以外の文字が含まれる場合は拒否する。
    #[test]
    fn rejects_non_hex_public_key_fingerprint() {
        let invalid = format!("{}zz", &PUBKEY_FP[..PUBKEY_FP.len() - 2]);
        let json = valid_envelope_json().replace(PUBKEY_FP, &invalid);

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// `dek_alg` が固定値と異なる場合は拒否する。
    #[test]
    fn rejects_wrong_dek_alg() {
        let json = valid_envelope_json().replace("aes-256-gcm", "aes-128-gcm");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// `recipient_kek_alg` が固定値と異なる場合は拒否する。
    #[test]
    fn rejects_wrong_recipient_kek_alg() {
        let json = valid_envelope_json().replace("rsa-oaep-sha256", "rsa-oaep-sha512");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// nonce の byte 長が 12 でない場合は拒否する。
    #[test]
    fn rejects_wrong_nonce_length() {
        let valid_nonce = base64_encode(&nonce_bytes());
        let short_nonce = base64_encode(&nonce_bytes()[..NONCE_LEN - 1]);
        let json = valid_envelope_json().replace(&valid_nonce, &short_nonce);

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// tag の byte 長が 16 でない場合は拒否する。
    #[test]
    fn rejects_wrong_tag_length() {
        let valid_tag = base64_encode(&tag_bytes());
        let short_tag = base64_encode(&tag_bytes()[..TAG_LEN - 1]);
        let json = valid_envelope_json().replace(&valid_tag, &short_tag);

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// base64 として不正な ciphertext は拒否する。
    #[test]
    fn rejects_invalid_base64_body() {
        let valid_body = base64_encode(b"encrypted-backup-bytes");
        let json = valid_envelope_json().replace(&valid_body, "not*valid*base64");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// padding（`=`）後にさらにデータが続く base64 は壊れた値として拒否する。
    /// `AA==AAAA` / `AB==CDEF` のように非末尾 chunk へ padding が現れる入力を弾く。
    #[test]
    fn rejects_base64_with_padding_before_end() {
        for invalid in ["AA==AAAA", "AB==CDEF", "AAAA====", "====AAAA"] {
            let json = valid_envelope_json().replace(&base64_encode(b"wrapped-dek-bytes"), invalid);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {invalid} to be rejected"
            );
        }
    }

    /// 末尾 chunk の padding（`=`/`==`）を持つ正当な base64 は受理する。
    #[test]
    fn base64_decode_accepts_trailing_padding() {
        // `AAAA` (no padding) / `AAA=` (1 byte tail) / `AA==` (2 bytes tail) を直接検証する。
        assert!(base64_decode("AAAA", "field").is_ok());
        assert!(base64_decode("AAAAAAA=", "field").is_ok());
        assert!(base64_decode("AAAAAA==", "field").is_ok());
        // 非末尾 chunk への padding は拒否する。
        assert!(base64_decode("AA==AAAA", "field").is_err());
    }

    /// padding を含む最終 quantum の余剰 sextet bit が 0 でない非 canonical base64
    /// （RFC 4648 §3.5 違反）を拒否し、canonical な値は受理する。
    #[test]
    fn base64_decode_rejects_non_canonical_padding_bits() {
        // 2 padding（1 byte 出力）: 2 番目 sextet の下位 4 bit が非 0 なら拒否。
        // `A`=0b000000, `B`=0b000001（下位 4 bit に 1 が立つ）。`AA==` は canonical。
        assert!(base64_decode("AB==", "field").is_err());
        assert!(base64_decode("AP==", "field").is_err());
        assert!(base64_decode("AA==", "field").is_ok());
        // 1 padding（2 byte 出力）: 3 番目 sextet の下位 2 bit が非 0 なら拒否。
        // `B`=0b000001（下位 2 bit に 1 が立つ）、`C`=0b000010 も同様。`AAA=` は canonical。
        assert!(base64_decode("AAB=", "field").is_err());
        assert!(base64_decode("AAC=", "field").is_err());
        assert!(base64_decode("AAA=", "field").is_ok());
        // 下位 bit が 0 の sextet を末尾に持つ canonical な padding は受理する。
        // `Q`=0b010000（下位 4 bit 0）→ `AQ==` は 2 padding canonical。
        assert!(base64_decode("AQ==", "field").is_ok());
        // `E`=0b000100（下位 2 bit 0）→ `AAE=` は 1 padding canonical。
        assert!(base64_decode("AAE=", "field").is_ok());
        // padding なしの通常データは canonical 検証の対象外で受理する。
        assert!(base64_decode("AAAA", "field").is_ok());
    }

    /// 未知の top-level field を持つ envelope は拒否する。
    #[test]
    fn rejects_unknown_top_level_field() {
        let json = valid_envelope_json().replace(
            "\"version\": 1,",
            "\"version\": 1,\n              \"extra\": \"x\",",
        );

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// 未知のネスト field（metadata / recipient / ciphertext）を持つ envelope は拒否する。
    #[test]
    fn rejects_unknown_nested_field() {
        let metadata = valid_envelope_json().replace(
            "\"dek_alg\": \"aes-256-gcm\",",
            "\"dek_alg\": \"aes-256-gcm\",\n                \"extra\": \"x\",",
        );
        assert!(
            GpgBackupEnvelope::parse(&metadata).is_err(),
            "unknown metadata field must be rejected"
        );

        let recipient = valid_envelope_json().replace(
            "\"piv_slot\": \"82\",",
            "\"piv_slot\": \"82\",\n                  \"extra\": \"x\",",
        );
        assert!(
            GpgBackupEnvelope::parse(&recipient).is_err(),
            "unknown recipient field must be rejected"
        );

        let ciphertext = valid_envelope_json()
            .replace("\"ciphertext\": {", "\"ciphertext\": { \"extra\": \"x\",");
        assert!(
            GpgBackupEnvelope::parse(&ciphertext).is_err(),
            "unknown ciphertext field must be rejected"
        );
    }

    /// piv_slot が文字列 "82" 以外の recipient は拒否する。
    #[test]
    fn rejects_wrong_piv_slot() {
        let json = valid_envelope_json().replace("\"piv_slot\": \"82\"", "\"piv_slot\": \"83\"");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// piv_slot が数値（正本 schema は文字列固定）の recipient は拒否する。
    #[test]
    fn rejects_numeric_piv_slot() {
        let json = valid_envelope_json().replace("\"piv_slot\": \"82\"", "\"piv_slot\": 82");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// piv_slot は文字列 "82" のときに受理し、`to_json` も文字列で出力する。
    #[test]
    fn accepts_string_piv_slot_and_round_trips() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        let json = ok(envelope.to_json(), "serialize");
        let text = ok(
            String::from_utf8(json).map_err(anyhow::Error::from),
            "utf8 json",
        );

        assert!(
            text.contains("\"piv_slot\":\"82\""),
            "to_json must emit piv_slot as the string \"82\": {text}"
        );
        let reparsed = ok(GpgBackupEnvelope::parse(&text), "reparse");
        assert_eq!(envelope, reparsed);
    }

    /// serial だけ一致し fingerprint が異なる場合は recipient を解決しない。
    #[test]
    fn does_not_match_on_serial_only() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        let other_fp = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        let connected = ok(
            ConnectedYubiKey::new("12345678", other_fp),
            "connected yubikey",
        );

        assert!(envelope.resolve_recipient(&connected).is_err());
    }

    /// fingerprint だけ一致し serial が異なる場合は recipient を解決しない。
    #[test]
    fn does_not_match_on_fingerprint_only() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        let connected = ok(
            ConnectedYubiKey::new("87654321", PUBKEY_FP),
            "connected yubikey",
        );

        assert!(envelope.resolve_recipient(&connected).is_err());
    }

    /// 大文字・コロン区切り混在の fingerprint は lowercase hex（区切りなし）へ正規化される。
    #[test]
    fn normalizes_mixed_case_and_separators() {
        let raw = "01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67";
        let parsed = ok(PrimaryFingerprint::parse(raw), "normalize fingerprint");

        assert_eq!(parsed.as_str(), PRIMARY_FP);
    }

    /// 接続中 YubiKey serial が 10 進でない場合は照合入力構築で拒否する。
    #[test]
    fn rejects_non_decimal_connected_serial() {
        assert!(ConnectedYubiKey::new("12ab", PUBKEY_FP).is_err());
    }

    /// 既定 envelope の `exported_at` 値を差し替えた JSON を返す。
    fn envelope_json_with_exported_at(exported_at: &str) -> String {
        valid_envelope_json().replace("2026-05-31T00:00:00Z", exported_at)
    }

    /// `exported_at` が空文字の envelope は停止条件として拒否する。
    #[test]
    fn rejects_empty_exported_at() {
        let json = envelope_json_with_exported_at("");

        assert!(GpgBackupEnvelope::parse(&json).is_err());
    }

    /// RFC3339 でない（date のみ等）`exported_at` は拒否する。
    #[test]
    fn rejects_non_rfc3339_exported_at() {
        for value in ["2026-05-31", "not-a-date", "2026-05-31 00:00:00Z"] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// UTC でない time-offset を持つ `exported_at` は拒否する。
    #[test]
    fn rejects_non_utc_exported_at() {
        for value in [
            "2026-05-31T00:00:00+09:00",
            "2026-05-31T00:00:00-05:00",
            "2026-05-31T00:00:00",
        ] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// 各フィールドが数値範囲外の `exported_at` は拒否する。
    #[test]
    fn rejects_out_of_range_exported_at() {
        for value in [
            "2026-13-31T00:00:00Z",
            "2026-05-32T00:00:00Z",
            "2026-05-31T24:00:00Z",
            "2026-05-31T00:60:00Z",
            "2026-05-31T00:00:61Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// 暦日として存在しない full-date の `exported_at` は拒否する。
    /// 月別日数（31/30）と 2 月の閏年判定（平年 29 日不在）を検証する。
    #[test]
    fn rejects_nonexistent_calendar_dates() {
        for value in [
            "2026-02-31T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-06-31T00:00:00Z",
            "2026-09-31T00:00:00Z",
            "2026-11-31T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-01-00T00:00:00Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// 閏年・各月末の正当な full-date の `exported_at` を受理する。
    #[test]
    fn accepts_valid_calendar_dates() {
        for value in [
            "2024-02-29T00:00:00Z",
            "2000-02-29T00:00:00Z",
            "2026-01-31T00:00:00Z",
            "2026-04-30T00:00:00Z",
            "2025-02-28T00:00:00Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            let envelope = ok(GpgBackupEnvelope::parse(&json), "parse");
            assert_eq!(envelope.metadata().exported_at(), value);
        }
    }

    /// 桁数不正・小数部欠落の `exported_at` は拒否する。
    #[test]
    fn rejects_malformed_exported_at_fields() {
        for value in [
            "26-05-31T00:00:00Z",
            "2026-5-31T00:00:00Z",
            "2026-05-31T0:00:00Z",
            "2026-05-31T00:00:00.Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// UTC RFC3339 として正当な `exported_at`（`Z` 小文字・`+00:00`・秒小数部）を受理する。
    /// 秒は `0..=59` のみ許可するため leap second（秒 `60`）の許可ケースは含めない。
    #[test]
    fn accepts_valid_utc_rfc3339_exported_at() {
        for value in [
            "2026-05-31T00:00:00Z",
            "2026-05-31t12:34:56z",
            "2026-05-31T00:00:00+00:00",
            "2026-05-31T00:00:00.123Z",
            "2026-05-31T23:59:59Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            let envelope = ok(GpgBackupEnvelope::parse(&json), "parse");
            assert_eq!(envelope.metadata().exported_at(), value);
        }
    }

    /// leap second（秒 `60`）は位置を問わず拒否する。`exported_at` は生成 timestamp として
    /// 秒 `0..=59` のみ許可し、leap second を適用しないため、月末日の `23:59:60`（5/31・12/31）も
    /// 月初の `00:00:60` も、すべて「UTC RFC3339 検証済み」表明から排除する。
    #[test]
    fn rejects_leap_second_at_any_position() {
        for value in [
            "2026-12-31T23:59:60Z",
            "2026-05-31T23:59:60Z",
            "2026-05-31T00:00:60Z",
            "2026-05-30T23:59:60Z",
            "2026-05-31T23:58:60Z",
            "2026-05-31T22:59:60Z",
        ] {
            let json = envelope_json_with_exported_at(value);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected {value} to be rejected"
            );
        }
    }

    /// wire（envelope JSON 由来の保存値）の primary_fingerprint は canonical
    /// （lowercase hex 40, 区切り・空白なし）のみ受理し、非 canonical を正規化せず拒否する。
    #[test]
    fn rejects_non_canonical_wire_primary_fingerprint() {
        // canonical（既定 fixture）は受理する。
        let canonical = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        assert_eq!(
            canonical.metadata().primary_fingerprint().as_str(),
            PRIMARY_FP
        );

        let uppercase = "0123456789ABCDEF0123456789abcdef01234567";
        let colon_separated = "01:23:45:67:89:ab:cd:ef:01:23:45:67:89:ab:cd:ef:01:23:45:67";
        let with_space = "0123456789abcdef0123456789abcdef0123456 ";
        let too_short = "0123456789abcdef0123456789abcdef0123456";
        let too_long = "0123456789abcdef0123456789abcdef012345670";
        let non_hex = "0123456789abcdef0123456789abcdef0123456g";
        for invalid in [
            uppercase,
            colon_separated,
            with_space,
            too_short,
            too_long,
            non_hex,
        ] {
            let json = valid_envelope_json().replace(PRIMARY_FP, invalid);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected wire primary_fingerprint {invalid:?} to be rejected"
            );
        }
    }

    /// wire（envelope JSON 由来の保存値）の recipient public_key_fingerprint は canonical
    /// （lowercase hex 64, 区切り・空白なし）のみ受理し、非 canonical を正規化せず拒否する。
    #[test]
    fn rejects_non_canonical_wire_public_key_fingerprint() {
        let uppercase = "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef";
        let colon_separated = "01:23:45:67:89:ab:cd:ef:01:23:45:67:89:ab:cd:ef:\
             01:23:45:67:89:ab:cd:ef:01:23:45:67:89:ab:cd:ef";
        let with_space = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde ";
        let too_short = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde";
        let too_long = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0";
        let non_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg";
        for invalid in [
            uppercase,
            colon_separated,
            with_space,
            too_short,
            too_long,
            non_hex,
        ] {
            let json = valid_envelope_json().replace(PUBKEY_FP, invalid);
            assert!(
                GpgBackupEnvelope::parse(&json).is_err(),
                "expected wire public_key_fingerprint {invalid:?} to be rejected"
            );
        }
    }

    /// stale-overwrite 防止 guard は同一値で `Ok`、不一致値で `Err` を返す。
    ///
    /// 設計「recipient 運用 / BWS 更新契約」L121-126 の停止条件（更新前 guard と更新直前の現行 guard が
    /// 一致する場合だけ上書き）を domain rule として直接駆動する。Revision 同士・ValueDigest 同士の
    /// 値不一致に加え、種類（Revision / ValueDigest）が異なる場合も不一致として停止することを観測する。
    #[test]
    fn backup_update_guard_matches_only_identical_value() {
        let revision_a = BackupUpdateGuard::Revision("rev-1".to_owned());
        let revision_b = BackupUpdateGuard::Revision("rev-2".to_owned());
        let digest_a = BackupUpdateGuard::from_value_bytes(b"current-value");
        let digest_b = BackupUpdateGuard::from_value_bytes(b"changed-value");

        // 一致: 同一 revision / 同一 digest は上書きを許可する。
        ok(
            revision_a.ensure_matches(&revision_a.clone()),
            "matching revision guard",
        );
        ok(
            digest_a.ensure_matches(&BackupUpdateGuard::from_value_bytes(b"current-value")),
            "matching value-digest guard",
        );

        // 不一致: revision 値違い・digest 値違いは stale-overwrite として停止する。
        assert!(
            revision_a.ensure_matches(&revision_b).is_err(),
            "differing revision must stop the overwrite"
        );
        assert!(
            digest_a.ensure_matches(&digest_b).is_err(),
            "differing value digest must stop the overwrite"
        );

        // 種類違い（Revision vs ValueDigest）も不一致として停止する。
        assert!(
            revision_a.ensure_matches(&digest_a).is_err(),
            "differing guard kind must stop the overwrite"
        );
        assert!(
            digest_a.ensure_matches(&revision_a).is_err(),
            "differing guard kind must stop the overwrite"
        );
    }

    /// canonical な wire fingerprint は `to_json` round-trip で書き換えられず保存値のまま出力される。
    #[test]
    fn wire_fingerprints_round_trip_without_rewrite() {
        let envelope = ok(GpgBackupEnvelope::parse(&valid_envelope_json()), "parse");
        let json = ok(envelope.to_json(), "serialize");
        let text = ok(
            String::from_utf8(json).map_err(anyhow::Error::from),
            "utf8 json",
        );

        assert!(
            text.contains(PRIMARY_FP),
            "to_json must emit primary_fingerprint unchanged: {text}"
        );
        assert!(
            text.contains(PUBKEY_FP),
            "to_json must emit public_key_fingerprint unchanged: {text}"
        );
        let reparsed = ok(GpgBackupEnvelope::parse(&text), "reparse");
        assert_eq!(envelope, reparsed);
    }
}
