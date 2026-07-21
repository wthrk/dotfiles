//! adapter が port 実装の receiver として使う concrete backend marker。
//!
//! 外部 I/O の technical state は各 support backend が所有する。adapter source はこの
//! receiver に対する port trait implementation だけを定義し、wrapper や状態を持たない。

#[derive(Default)]
pub(crate) struct ProcessIoBackend;

#[derive(Default)]
pub(crate) struct JsonReportBackend;

#[derive(Default)]
pub(crate) struct BwsClientBackend;

#[derive(Default)]
pub(crate) struct BackupCipherBackend;

#[derive(Default)]
pub(crate) struct GpgKeyringBackend;

#[derive(Default)]
pub(crate) struct SshAgentBackend;

#[derive(Default)]
pub(crate) struct PasswordStoreBackend;

#[derive(Default)]
pub(crate) struct GitCloneBackend;
