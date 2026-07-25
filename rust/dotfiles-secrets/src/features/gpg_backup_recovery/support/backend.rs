//! GPG adapter receivers owned by the GPG support boundary.

#[derive(Default)]
pub(crate) struct BackupCipherBackend;

#[derive(Default)]
pub(crate) struct GpgKeyringBackend;

#[derive(Default)]
pub(crate) struct SshAgentBackend;
