use std::any::Any;

/// Domain 境界で扱う秘密値の唯一許可表現。
///
/// domain は保護メモリ実装を知らず、長さと fallible duplicate だけを使って
/// business rule を適用する。具体的な secret buffer 操作は、この opaque value を
/// 生成・消費する adapter/support 境界に閉じ込める。
pub struct SecretMaterial {
    backend: Box<dyn SecretMaterialBackend>,
}

trait SecretMaterialBackend: Any {
    fn len(&self) -> usize;
    fn try_clone(&self) -> crate::Result<Box<dyn SecretMaterialBackend>>;
    fn as_any(&self) -> &dyn Any;
}

struct TypedSecretMaterialBackend<T> {
    value: T,
    len: fn(&T) -> usize,
    try_clone: fn(&T) -> crate::Result<T>,
}

impl SecretMaterial {
    /// 保護実装を domain opaque value として受け取る。
    ///
    /// caller は raw bytes ではなく、secret storage に適した保護済み所有値だけを渡す。
    pub(in crate::secrets) fn from_backend<T: 'static>(
        value: T,
        len: fn(&T) -> usize,
        try_clone: fn(&T) -> crate::Result<T>,
    ) -> Self {
        Self {
            backend: Box::new(TypedSecretMaterialBackend {
                value,
                len,
                try_clone,
            }),
        }
    }

    /// 保持中 secret の byte 長を返す。
    pub fn len(&self) -> usize {
        self.backend.len()
    }

    /// 同じ保護実装に委譲して、独立した fallible duplicate を作る。
    pub fn try_clone(from: &Self) -> crate::Result<Self> {
        Ok(Self {
            backend: from.backend.try_clone()?,
        })
    }

    /// adapter/support 境界で具体保護実装へ戻す。
    ///
    /// raw bytes は返さず、具体型への参照だけを返す。具体型側の可視性により
    /// secret buffer access は protection 内部に閉じたまま維持される。
    pub(in crate::secrets) fn as_backend<T: 'static>(&self) -> Option<&T> {
        self.backend
            .as_any()
            .downcast_ref::<TypedSecretMaterialBackend<T>>()
            .map(|backend| &backend.value)
    }
}

impl<T: 'static> SecretMaterialBackend for TypedSecretMaterialBackend<T> {
    fn len(&self) -> usize {
        (self.len)(&self.value)
    }

    fn try_clone(&self) -> crate::Result<Box<dyn SecretMaterialBackend>> {
        Ok(Box::new(Self {
            value: (self.try_clone)(&self.value)?,
            len: self.len,
            try_clone: self.try_clone,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
