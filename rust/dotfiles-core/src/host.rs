//! flake 出力名に使うホスト名の正規化。

/// `foo.local` と `foo` が別の `darwinConfigurations` を指さないよう、最初の DNS ラベルだけを返す。
pub fn short(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}
