//! semver 3 要素の比較/整形を提供する共通補助。

/// 3 要素 semver タプルの大小比較を行う。
///
/// caller は `major/minor/patch` の順序で渡す責務を負い、比較規則は Rust のタプル辞書順に一致する。
pub(crate) fn semver_lt(left: (u8, u8, u8), right: (u8, u8, u8)) -> bool {
    left < right
}

/// semver タプルを `major.minor.patch` 文字列へ整形する。
///
/// 表示専用ヘルパーであり、比較や互換性判定は `semver_lt` などの値比較で扱う。
pub(crate) fn format_semver(version: (u8, u8, u8)) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}
