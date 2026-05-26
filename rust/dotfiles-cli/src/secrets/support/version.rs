//! semver 3 要素の比較/整形を提供する共通補助。

pub(crate) fn semver_lt(left: (u8, u8, u8), right: (u8, u8, u8)) -> bool {
    left < right
}

pub(crate) fn format_semver(version: (u8, u8, u8)) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}
