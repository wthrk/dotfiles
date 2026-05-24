//! enroll / verify 系 command の出力 summary 型。
//!
//! summary DTO は application が生成し、stdout へ JSON 出力する責務を持つ。
//! domain は wire format / 不変条件のみを担い、reporting 型を持たない。

use std::collections::BTreeMap;

use serde::Serialize;

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum CheckStatus {
    /// 確認に成功した状態。
    #[serde(rename = "ok")]
    Ok,
    /// 永続書き込み後の確認に失敗した状態。
    #[serde(rename = "failed")]
    Failed,
    /// 現在の実行範囲では省略した確認項目。
    #[serde(rename = "skipped")]
    Skipped,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EnrollSummary {
    /// 登録対象 YubiKey の serial。
    pub(crate) serial: u32,
    /// 登録した YubiKey の role。
    pub(crate) role: YubikeyRole,
    /// 登録中に完了した確認項目。
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum YubikeyRole {
    /// 正本 secret を最初に登録する primary YubiKey。
    Primary,
    /// primary から再暗号化した secret を持つ spare YubiKey。
    Spare,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VerifySummary {
    /// 検証対象 YubiKey の serial。
    pub(crate) serial: u32,
    /// 実行または省略した確認項目。
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckName {
    /// PIV key と manifest の初期作成。
    Setup,
    /// `bw-email` の保存または復号確認。
    BwEmail,
    /// `bw-password` の保存または復号確認。
    BwPassword,
    /// `bws-access-token` の保存または復号確認。
    BwsAccessToken,
    /// YubiKey local storage 上の 3 secret 復号確認。
    LocalStorage,
    /// Bitwarden Secrets Manager への接続確認。
    Bws,
    /// Bitwarden login secret の妥当性確認。
    BwLogin,
}
