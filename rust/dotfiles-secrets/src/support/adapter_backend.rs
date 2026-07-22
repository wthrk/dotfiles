//! adapter が port 実装の receiver として使う concrete backend marker。
//!
//! 外部 I/O の technical state は各 support backend が所有する。adapter source はこの
//! receiver に対する port trait implementation だけを定義し、wrapper や状態を持たない。

#[derive(Default)]
pub(crate) struct ProcessIoBackend;

/// hidden TTY から BWS token を読む input backend marker。
#[derive(Default)]
pub(crate) struct HiddenTokenInputBackend;

/// pipe stdin から BWS token を読む input backend marker。
#[derive(Default)]
pub(crate) struct StreamedTokenInputBackend;

/// hidden TTY の bootstrap document input backend marker。
#[derive(Default)]
pub(crate) struct HiddenBootstrapDocumentInputBackend;

/// stdin JSON の bootstrap document input backend marker。
#[derive(Default)]
pub(crate) struct StreamedBootstrapDocumentInputBackend;

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
