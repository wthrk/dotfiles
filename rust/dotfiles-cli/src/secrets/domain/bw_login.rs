//! `bw-login` use case の結果意味を表す domain 値。
//!
//! Bitwarden Password Manager への login / unlock の到達結果だけを保持し、`bw` CLI 実行手段や
//! JSON 表示形式、`BW_SESSION` の取り回しは持たない。session token などの secret 値はこの層へ
//! 載せず、login / unlock が成立したかという業務上の意味だけを表す。

/// `bw-login` use case（spec L176-178）の結果要約。
///
/// 設計は YubiKey 由来の `bw-email` / `bw-password` と OTP を使って `bw login` の後 `bw unlock` を
/// 実行する。この summary は login と unlock の成立だけを意味として保持し、`BW_SESSION` 値そのもの
/// （secret）や表示形式は持たない。`BW_SESSION` を出力するか否かの presentation 仕様は adapter 側で
/// 決め、ここでは「unlock 済み session を確立したか」という意味だけを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwLoginSummary {
    /// `bw login` が成立したか。
    pub logged_in: bool,
    /// `bw unlock` まで成立し、unlock 済み session を確立したか。
    pub unlocked: bool,
}

impl BwLoginSummary {
    /// login / unlock の双方が成立した通常系 summary を構築する。
    pub fn established() -> Self {
        Self {
            logged_in: true,
            unlocked: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 成立 summary は login / unlock の双方を成立として保持する。
    #[test]
    fn established_summary_marks_login_and_unlock() {
        let summary = BwLoginSummary::established();

        assert!(summary.logged_in);
        assert!(summary.unlocked);
    }
}
