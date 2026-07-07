//! wall-clock UTC を RFC3339 文字列へ整形する process-generic な時刻 primitive。
//!
//! この module は feature 固有の語彙や業務判断を持たず、`SystemTime` から得た UNIX epoch 秒を
//! `YYYY-MM-DDThh:mm:ssZ`（UTC、秒精度）へ変換する純粋な暦計算だけを担う。adapter が backup envelope の
//! `exported_at` 生成へ利用する技術 primitive であり、application/domain から直接は使わせない。

use std::time::{SystemTime, UNIX_EPOCH};

use crate::Result;

/// 現在時刻を UTC RFC3339（`YYYY-MM-DDThh:mm:ssZ`、秒精度）文字列として返す。
///
/// system clock が UNIX epoch より前を返した場合は失敗する。閏年・月別日数を含む Gregorian 暦で
/// epoch 秒を日付へ展開し、leap second は適用しない（秒は常に `0..=59`）。
pub(crate) fn now_rfc3339_utc() -> Result<String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("system clock is before the UNIX epoch"))?;
    Ok(format_rfc3339_utc(duration.as_secs()))
}

/// UNIX epoch 秒を UTC RFC3339 文字列へ整形する。
fn format_rfc3339_utc(total_seconds: u64) -> String {
    let seconds_of_day = total_seconds % 86_400;
    let mut days = total_seconds / 86_400;

    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    let mut year = 1970u64;
    loop {
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    let mut month = 1u64;
    loop {
        let month_days = u64::from(days_in_month(year, month));
        if days < month_days {
            break;
        }
        days -= month_days;
        month += 1;
    }
    let day = days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Gregorian 閏年判定（4 で割り切れ、かつ 100 で割り切れない、または 400 で割り切れる）。
fn is_leap_year(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// 指定年月の暦日上の日数を返す。
fn days_in_month(year: u64, month: u64) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    //! epoch 秒 → RFC3339 整形の境界値を検証する単体テスト。

    use super::format_rfc3339_utc;

    /// epoch 起点と既知の日時を正しく整形する。
    #[test]
    fn formats_known_timestamps() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        // 2026-05-31T00:00:00Z = 1780185600
        assert_eq!(format_rfc3339_utc(1_780_185_600), "2026-05-31T00:00:00Z");
        // 閏日 2024-02-29T12:34:56Z = 1709210096
        assert_eq!(format_rfc3339_utc(1_709_210_096), "2024-02-29T12:34:56Z");
    }
}
