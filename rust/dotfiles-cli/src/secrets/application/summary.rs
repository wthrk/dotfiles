//! YubiKey 関連 use case が出力する summary DTO。
//!
//! これらは CLI 向け report 契約であり、domain 不変条件ではなく application 層で所有する。

use std::collections::BTreeMap;

use serde::Serialize;

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) enum CheckStatus {
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

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CheckName {
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

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum YubikeyRole {
    /// 正本 secret を最初に登録する primary YubiKey。
    Primary,
    /// primary から再暗号化した secret を持つ spare YubiKey。
    Spare,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct EnrollSummary {
    /// 登録対象 YubiKey の serial。
    pub(super) serial: u32,
    /// 登録した YubiKey の role。
    pub(super) role: YubikeyRole,
    /// 登録中に完了した確認項目。
    pub(super) checks: BTreeMap<CheckName, CheckStatus>,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct VerifySummary {
    /// 検証対象 YubiKey の serial。
    pub(super) serial: u32,
    /// 実行または省略した確認項目。
    pub(super) checks: BTreeMap<CheckName, CheckStatus>,
}
