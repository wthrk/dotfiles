use anyhow::Result;

use super::{
    material::SecretMaterial,
    piv::PivObjectId,
    wire::{BootstrapSecretWire, ManifestWire, SensitiveBytes},
};

/// bootstrap secret JSON 各 field に許可する最大 byte 長。
pub const BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT: usize = 16 * 1024;

/// manifest が dotfiles secret recovery 用であることを示す app id。
pub(crate) const MANIFEST_APP: &str = "dotfiles.secret-recovery";

/// PIV object に保存する secret storage manifest。
///
/// `version` と `app` の組が storage format の識別子であり、期待値との一致が初期化済み判定の不変条件になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretManifest {
    /// manifest format version。
    pub version: u8,
    /// この repository の secret storage manifest であることを示す app id。
    pub app: String,
}

/// bootstrap enrollment で受け取る secret document の純粋な domain 表現。
///
/// 各 field は常に `SecretMaterial` として保持し、平文文字列化は domain の責務に含めない。
pub struct BootstrapSecretDocument {
    pub bw_email: SecretMaterial,
    pub bw_password: SecretMaterial,
    pub bws_access_token: SecretMaterial,
}

impl SecretManifest {
    /// この repository が認識する manifest sentinel を構築する。
    ///
    /// version と app の組は storage compatibility の基準で、version 1 では固定値を返す。
    pub fn expected() -> Self {
        Self {
            version: 1,
            app: MANIFEST_APP.to_owned(),
        }
    }

    /// manifest が現在の storage format と一致することを確認する。
    ///
    /// `expected()` と一致しない値は互換性違反として失敗する。
    /// 呼び出し側は manifest の存在確認をこの検証の前後どちらで行うかを文脈に応じて選ぶ責務を負う。
    pub fn validate_expected(&self) -> Result<()> {
        if self != &Self::expected() {
            return Err(invalid_data(
                "YubiKey secret manifest does not match dotfiles secret-recovery format",
            )
            .into());
        }

        Ok(())
    }

    /// setup 実行前に storage layout が未初期化状態かを確認する。
    ///
    /// key slot、manifest object、予約済み object 群の関係だけを判定し、device への読み書き自体は含まない。
    /// 既存 manifest が期待形式と一致する場合は「既に初期化済み」として失敗し、呼び出し側は occupied object 一覧を完全に渡す責務を負う。
    pub fn ensure_setup_allowed(
        key_exists: bool,
        manifest_bytes: Option<&[u8]>,
        occupied_object_ids: &[PivObjectId],
    ) -> Result<()> {
        if key_exists {
            if let Some(manifest_bytes) = manifest_bytes {
                let manifest = Self::decode(manifest_bytes)?;
                manifest.validate_expected()?;
                return Err(invalid_data("YubiKey secret storage is already initialized").into());
            }
            return Err(invalid_data("YubiKey PIV slot is already initialized").into());
        }

        if let Some(object_id) = occupied_object_ids.first() {
            return Err(
                invalid_data(format!("YubiKey PIV object {} already exists", object_id)).into(),
            );
        }

        Ok(())
    }

    /// manifest object が存在し、期待する storage format と一致することを確認する。
    ///
    /// `None` は未初期化ではなく「必要な manifest が欠落した異常」として失敗する。
    /// decode 後は `validate_expected` を必ず通し、期待形式以外の manifest を受け入れない。
    pub fn decode_initialized(manifest_bytes: Option<&[u8]>) -> Result<Self> {
        let manifest_bytes =
            manifest_bytes.ok_or_else(|| invalid_data("YubiKey secret manifest is missing"))?;
        let manifest = Self::decode(manifest_bytes)?;
        manifest.validate_expected()?;
        Ok(manifest)
    }

    /// manifest を JSON wire format に encode する。
    ///
    /// 現行 version では `ManifestWire` へ直列化する。
    /// 呼び出し側は返却 byte 列を manifest object 以外へ流用しない責務を負う。
    pub fn encode(&self) -> Result<Vec<u8>> {
        ManifestWire {
            version: self.version,
            app: self.app.clone(),
        }
        .encode_json()
    }

    /// manifest を JSON wire format から decode する。
    ///
    /// JSON 構造が壊れている場合と `app` sentinel が一致しない場合は失敗する。
    /// 呼び出し側はこの結果に対して必要に応じて `validate_expected` を追加で適用する責務を負う。
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let manifest = ManifestWire::decode_json(bytes)?;

        if manifest.app != MANIFEST_APP {
            return Err(invalid_data("failed to parse YubiKey secret manifest").into());
        }

        Ok(Self {
            version: manifest.version,
            app: manifest.app,
        })
    }
}

impl BootstrapSecretDocument {
    /// 対話入力で取得した bootstrap secret 値群を domain document へ復元する。
    ///
    /// 各 field は与えられた byte 列をそのまま `SecretMaterial` へ包み、UTF-8 前提を持ち込まない。
    /// 呼び出し側は field と secret 名の対応を崩さず渡す責務を負う。
    pub fn from_interactive_secrets(
        bw_email: &[u8],
        bw_password: &[u8],
        bws_access_token: &[u8],
    ) -> Result<Self> {
        Ok(Self {
            bw_email: SecretMaterial::from_vec(bw_email.to_vec()),
            bw_password: SecretMaterial::from_vec(bw_password.to_vec()),
            bws_access_token: SecretMaterial::from_vec(bws_access_token.to_vec()),
        })
    }

    /// stdin JSON document を bootstrap secret document として decode する。
    ///
    /// 現行 format では 3 field を必須とし、各 field は `field_limit` byte 以下の JSON string でなければ失敗する。
    /// 呼び出し側は `field_limit` に area policy で定めた上限を渡す責務を負う。
    pub fn decode_json(bytes: &[u8], field_limit: usize) -> Result<Self> {
        let wire = BootstrapSecretWire::decode_json(bytes)?;

        let to_secret = |field: &str, value: SensitiveBytes| -> Result<SecretMaterial> {
            let value = value.into_secret_material();
            if value.len() > field_limit {
                return Err(
                    invalid_data(format!("JSON field `{field}` exceeds maximum length")).into(),
                );
            }
            Ok(value)
        };

        Ok(Self {
            bw_email: to_secret("bw-email", wire.bw_email)?,
            bw_password: to_secret("bw-password", wire.bw_password)?,
            bws_access_token: to_secret("bws-access-token", wire.bws_access_token)?,
        })
    }
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::BootstrapSecretDocument;

    #[test]
    fn decode_json_preserves_whitespace_in_values() {
        let json = br#"{
            "bw-email":"user@example.com",
            "bw-password":"  leading and  internal\tspace  ",
            "bws-access-token":"token value"
        }"#;

        let decoded = BootstrapSecretDocument::decode_json(json, 16 * 1024);
        assert!(decoded.is_ok(), "bootstrap secret JSON should decode");
        let doc = match decoded {
            Ok(doc) => doc,
            Err(error) => panic!("unexpected decode error: {error:?}"),
        };

        doc.bw_password.with_bytes(|bytes| {
            assert_eq!(bytes, b"  leading and  internal\tspace  ");
        });
    }
}
