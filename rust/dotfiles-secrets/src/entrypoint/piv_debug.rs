//! `provision-bws-token --debug` の CLI presentation を担う。
//!
//! 診断は application の use case と同じ port 呼び出しを透過的に観測するだけである。PIV の
//! protocol step、SDK 型、raw status を port 契約へ追加せず、PIN・token・secret の値、長さ、hash
//! も formatter へ渡さない。

use crate::{
    Result,
    application::run_provision_yubikey_bws_token::{
        ProvisionBwsTokenRuntime, run_provision_yubikey_bws_token,
    },
    domain::{
        commands::ProvisionBwsTokenCommand,
        piv::SecretStorageSpec,
        storage::{
            SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
            SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageStatusInspection, SecretStorageWriteInspection, SecretStorageWriteIntent,
        },
    },
    ports::{BitwardenClientSecretInputPort, DeviceSerialPort, PivPinInputPort, SecretStoragePort},
    support::protection::ProtectedSecret,
};

/// debug command を通常と同じ use case / port lifecycle で起動する。
///
/// ここで追加するのは stderr の allow-list 済み phase だけであり、device discovery、PIN input、
/// PIV session、storage mutation の順序や回数を変えない。失敗は元の error を分類せず伝播する。
pub(super) fn run(
    command: ProvisionBwsTokenCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    secret_input: &dyn BitwardenClientSecretInputPort,
    storage: &mut dyn SecretStoragePort,
) -> Result<()> {
    let output = PivDebugOutput;
    let mut device = DebugDevice {
        inner: device,
        output: &output,
    };
    let pin = DebugPinInput {
        inner: piv_pin,
        output: &output,
    };
    let token = DebugTokenInput {
        inner: secret_input,
        output: &output,
    };
    let mut storage = DebugStorage {
        inner: storage,
        output: &output,
    };
    let result = run_provision_yubikey_bws_token(
        command,
        ProvisionBwsTokenRuntime {
            device: &mut device,
            piv_pin: &pin,
            secret_input: &token,
            storage: &mut storage,
        },
    );
    match result {
        Ok(()) => {
            output.emit("provisioning-completed");
            Ok(())
        }
        Err(error) => {
            output.emit("subsequent-phase-not-reached");
            Err(error)
        }
    }
}

/// fixed `key=value` schema だけを stderr へ出す presentation formatter。
struct PivDebugOutput;

impl PivDebugOutput {
    /// diagnostic phase を値を伴わず出力する。
    fn emit(&self, phase: &str) {
        eprintln!("dotfiles-piv-debug phase={phase}");
    }

    /// 解決済み serial だけを identity として出力する。
    fn target_resolved(&self, serial: u32) {
        eprintln!("dotfiles-piv-debug phase=target-resolved serial={serial}");
    }

    /// 外部 error の意味を推測せず opaque result として出力する。
    fn opaque_failure(&self, phase: &str) {
        eprintln!("dotfiles-piv-debug phase={phase} result=opaque-error");
    }
}

/// device discovery capability を同じ一回の呼び出しで観測する decorator。
struct DebugDevice<'a> {
    inner: &'a mut dyn DeviceSerialPort,
    output: &'a PivDebugOutput,
}

impl DeviceSerialPort for DebugDevice<'_> {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.output.emit("discovery-started");
        match self.inner.resolve_device_serial(requested) {
            Ok(serial) => {
                self.output.target_resolved(serial);
                Ok(serial)
            }
            Err(error) => {
                self.output.opaque_failure("discovery-returned");
                Err(error)
            }
        }
    }
}

/// hidden PIN input capability を秘密値を見ずに観測する decorator。
struct DebugPinInput<'a> {
    inner: &'a dyn PivPinInputPort,
    output: &'a PivDebugOutput,
}

impl PivPinInputPort for DebugPinInput<'_> {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.output.emit("tty-pin-input-started");
        match self.inner.read_piv_pin_secret() {
            Ok(secret) => {
                self.output.emit("tty-pin-input-accepted");
                Ok(secret)
            }
            Err(error) => {
                self.output.emit("tty-pin-input-rejected");
                Err(error)
            }
        }
    }
}

/// BWS token input capability を秘密値を見ずに観測する decorator。
struct DebugTokenInput<'a> {
    inner: &'a dyn BitwardenClientSecretInputPort,
    output: &'a PivDebugOutput,
}

impl BitwardenClientSecretInputPort for DebugTokenInput<'_> {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        self.output.emit("tty-token-input-started");
        match self.inner.read_bitwarden_client_secret() {
            Ok(secret) => {
                self.output.emit("tty-token-input-accepted");
                Ok(secret)
            }
            Err(error) => {
                self.output.emit("tty-token-input-rejected");
                Err(error)
            }
        }
    }
}

/// PIV storage capability を同じ underlying call へ forwarding しながら phase だけを観測する decorator。
struct DebugStorage<'a> {
    inner: &'a mut dyn SecretStoragePort,
    output: &'a PivDebugOutput,
}

impl DebugStorage<'_> {
    /// 一つの high-level storage operation を opaque failure としてだけ観測する。
    fn observe<T>(
        output: &PivDebugOutput,
        phase: &str,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        output.emit(&format!("{phase}-started"));
        match operation() {
            Ok(value) => {
                output.emit(&format!("{phase}-succeeded"));
                Ok(value)
            }
            Err(error) => {
                output.opaque_failure(&format!("{phase}-returned"));
                Err(error)
            }
        }
    }
}

impl SecretStoragePort for DebugStorage<'_> {
    fn begin_piv_management_session(&mut self, serial: u32, pin: ProtectedSecret) -> Result<()> {
        let output = self.output;
        output.emit("piv-management-session-started");
        match self.inner.begin_piv_management_session(serial, pin) {
            Ok(()) => {
                output.emit("piv-management-session-succeeded");
                Ok(())
            }
            Err(error) => {
                output.opaque_failure("piv-management-session-returned");
                Err(error)
            }
        }
    }

    fn begin_next_piv_management_session(
        &mut self,
        serial: u32,
        pin: ProtectedSecret,
    ) -> Result<()> {
        self.inner.begin_next_piv_management_session(serial, pin)
    }

    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        Self::observe(self.output, "storage-setup-inspection", || {
            self.inner.inspect_secret_storage_setup(serial, probe)
        })
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<Vec<u8>> {
        Self::observe(self.output, "storage-initialization", || {
            self.inner.initialize_secret_storage(serial, intent)
        })
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        manifest_bytes: Vec<u8>,
    ) -> Result<()> {
        Self::observe(self.output, "storage-setup-finalization", || {
            self.inner
                .finalize_secret_storage_setup(serial, manifest_bytes)
        })
    }

    fn clear_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageClearIntent,
    ) -> Result<Vec<u8>> {
        Self::observe(self.output, "storage-clear", || {
            self.inner.clear_secret_storage(serial, intent)
        })
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        Self::observe(self.output, "storage-write-preflight-inspection", || {
            self.inner.inspect_secret_storage_write(serial, storage)
        })
    }

    fn inspect_secret_storage_status(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageStatusInspection> {
        Self::observe(self.output, "storage-status-inspection", || {
            self.inner.inspect_secret_storage_status(serial, storage)
        })
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        Self::observe(self.output, "storage-store", || {
            self.inner.store_secret(serial, intent, secret)
        })
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        Self::observe(self.output, "local-verification-inspection", || {
            self.inner.inspect_secret_storage_read(serial, storage)
        })
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
    ) -> Result<ProtectedSecret> {
        Self::observe(self.output, "local-verification-load", || {
            self.inner.load_secret(serial, intent)
        })
    }
}
