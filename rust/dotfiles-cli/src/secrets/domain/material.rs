use crate::secrets::support::protection::ProtectedSecret;

/// Domain 境界で扱う秘密値の唯一許可表現。
pub type SecretMaterial = ProtectedSecret;
