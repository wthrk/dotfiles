//! Password-store feature が所有する application input values。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestorePassCommand {
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProvisionPasswordStoreRemoteCommand {
    pub(crate) assume_overwrite: bool,
    pub(crate) serial: Option<u32>,
    pub(crate) url: Option<String>,
}
