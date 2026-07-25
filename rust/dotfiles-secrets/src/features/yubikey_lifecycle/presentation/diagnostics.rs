//! YubiKey management command の command-scoped diagnostic presentation。
//!
//! composition が既存 port receiver へ decorator を常設し、entrypoint が typed command kind と
//! `debug` だけを [`DiagnosticScopeControl`] へ渡す。decorator は inner operation を一回だけ
//! forwarding し、固定 phase と success/opaque failure 以外を表示しない。

use crate::{
    Result,
    features::{
        cli_interaction::ports::public::{
            BitwardenClientSecretInputPort, BootstrapDocumentInputPort,
        },
        yubikey_lifecycle::{
            domain::{
                piv::SecretStorageSpec,
                storage::{
                    SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
                    SecretStorageSetupInspection, SecretStorageSetupIntent,
                    SecretStorageSetupProbe, SecretStorageStatusInspection,
                    SecretStorageWriteInspection, SecretStorageWriteIntent,
                },
            },
            ports::public::{
                diagnostics::{DiagnosticCommand, DiagnosticRunToken, DiagnosticScopeControl},
                piv_pin_input::PivPinInputPort,
            },
            ports::{DeviceSerialPort, SecretStoragePort},
        },
    },
    foundation::protection::ProtectedSecret,
};
use std::cell::Cell;

type Sink = fn(&str);

thread_local! {
    static SINK: Cell<Option<Sink>> = const { Cell::new(None) };
    static COMMAND: Cell<Option<DiagnosticCommand>> = const { Cell::new(None) };
}

/// support technical observation を presentation phase へ写像する閉じた集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackendPhase {
    SessionOpen,
    VerifyInvocation,
    ManagementKeyAuthentication,
}

/// fixed diagnostic sink と command-local state を束縛する controller。
pub(crate) struct DiagnosticController {
    sink: Sink,
}

impl DiagnosticController {
    pub(crate) fn new(sink: Sink) -> Self {
        Self { sink }
    }
}

impl DiagnosticScopeControl for DiagnosticController {
    fn begin(&self, enabled: bool, command: DiagnosticCommand) -> Box<dyn DiagnosticRunToken + '_> {
        let previous_sink = SINK.with(|current| {
            let previous = current.get();
            current.set(enabled.then_some(self.sink));
            previous
        });
        let previous_command = COMMAND.with(|current| {
            let previous = current.get();
            current.set(enabled.then_some(command));
            previous
        });
        Box::new(RunToken {
            previous_sink,
            previous_command,
            finished: false,
        })
    }
}

struct RunToken {
    previous_sink: Option<Sink>,
    previous_command: Option<DiagnosticCommand>,
    finished: bool,
}

impl DiagnosticRunToken for RunToken {
    fn finish(mut self: Box<Self>, result: &Result<()>) {
        emit(Phase::Provisioning, Observation::from_result(result));
        self.finished = true;
    }
}

impl Drop for RunToken {
    fn drop(&mut self) {
        if !self.finished {
            emit(Phase::Provisioning, Observation::OpaqueFailure);
        }
        COMMAND.with(|current| current.set(self.previous_command));
        SINK.with(|current| current.set(self.previous_sink));
    }
}

#[derive(Clone, Copy)]
enum Phase {
    DeviceProfilePreflight,
    Discovery,
    TargetResolved(u32),
    TtyPinInput,
    TtyNewPinInput,
    TtyTokenInput,
    PivSessionOpen,
    PivVerifyInvocation,
    PivManagementKeyAuthentication,
    StorageSetupInspection,
    StorageInitialization,
    StorageSetupFinalization,
    StorageClear,
    StorageWritePreflightInspection,
    StorageStatusInspection,
    StorageStore,
    LocalVerificationInspection,
    LocalVerificationLoad,
    Provisioning,
}

#[derive(Clone, Copy)]
enum Observation {
    Started,
    Succeeded,
    Accepted,
    OpaqueFailure,
}

impl Observation {
    fn from_result<T>(result: &Result<T>) -> Self {
        if result.is_ok() {
            Self::Succeeded
        } else {
            Self::OpaqueFailure
        }
    }
}

/// support が通知した typed operation を固定 presentation phase として表示する。
pub(crate) fn observe_backend(phase: BackendPhase, succeeded: Option<bool>) {
    let phase = match phase {
        BackendPhase::SessionOpen => Phase::PivSessionOpen,
        BackendPhase::VerifyInvocation => Phase::PivVerifyInvocation,
        BackendPhase::ManagementKeyAuthentication => Phase::PivManagementKeyAuthentication,
    };
    let observation = match succeeded {
        None => Observation::Started,
        Some(true) => Observation::Succeeded,
        Some(false) => Observation::OpaqueFailure,
    };
    emit(phase, observation);
}

fn traced<T>(phase: Phase, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    emit(phase, Observation::Started);
    let result = operation();
    emit(phase, Observation::from_result(&result));
    result
}

fn accepted<T>(phase: Phase, result: &Result<T>) {
    emit(
        phase,
        if result.is_ok() {
            Observation::Accepted
        } else {
            Observation::OpaqueFailure
        },
    );
}

fn emit(phase: Phase, observation: Observation) {
    if COMMAND.with(|command| command.get()).is_none() {
        return;
    }
    let line = match phase {
        Phase::TargetResolved(serial) => {
            format!("dotfiles-piv-debug phase=target-resolved serial={serial}")
        }
        Phase::Provisioning => match observation {
            Observation::Succeeded => "dotfiles-piv-debug phase=provisioning-completed".to_owned(),
            Observation::OpaqueFailure => {
                "dotfiles-piv-debug phase=subsequent-phase-not-reached".to_owned()
            }
            Observation::Started | Observation::Accepted => return,
        },
        phase => {
            let name = match phase {
                Phase::Discovery => "discovery",
                Phase::TtyPinInput => "tty-pin-input",
                Phase::TtyNewPinInput => "tty-new-pin-input",
                Phase::TtyTokenInput => "tty-token-input",
                Phase::PivSessionOpen => "piv-session-open",
                Phase::PivVerifyInvocation => "piv-verify-invocation",
                Phase::PivManagementKeyAuthentication => "piv-management-key-authentication",
                Phase::StorageSetupInspection => "storage-setup-inspection",
                Phase::StorageInitialization => "storage-initialization",
                Phase::StorageSetupFinalization => "storage-setup-finalization",
                Phase::StorageClear => "storage-clear",
                Phase::StorageWritePreflightInspection => "storage-write-preflight-inspection",
                Phase::StorageStatusInspection => "storage-status-inspection",
                Phase::StorageStore => "storage-store",
                Phase::LocalVerificationInspection => "local-verification-inspection",
                Phase::LocalVerificationLoad => "local-verification-load",
                Phase::DeviceProfilePreflight => "device-profile-preflight",
                Phase::TargetResolved(_) | Phase::Provisioning => return,
            };
            let suffix = match observation {
                Observation::Started => "started",
                Observation::Succeeded => "succeeded",
                Observation::Accepted => "accepted",
                Observation::OpaqueFailure => "returned result=opaque-error",
            };
            format!("dotfiles-piv-debug phase={name}-{suffix}")
        }
    };
    SINK.with(|sink| {
        if let Some(sink) = sink.get() {
            sink(&line);
        }
    });
}

pub(crate) struct DiagnosticDevice<'a> {
    inner: &'a mut dyn DeviceSerialPort,
}

pub(crate) fn device(inner: &mut dyn DeviceSerialPort) -> DiagnosticDevice<'_> {
    DiagnosticDevice { inner }
}

impl DeviceSerialPort for DiagnosticDevice<'_> {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        emit(Phase::Discovery, Observation::Started);
        let result = self.inner.resolve_device_serial(requested);
        match result {
            Ok(serial) => {
                emit(Phase::TargetResolved(serial), Observation::Succeeded);
                Ok(serial)
            }
            Err(error) => {
                emit(Phase::Discovery, Observation::OpaqueFailure);
                Err(error)
            }
        }
    }

    fn preflight_device_profile(&mut self, serial: u32) -> Result<()> {
        emit(Phase::DeviceProfilePreflight, Observation::Started);
        let result = self.inner.preflight_device_profile(serial);
        emit(
            Phase::DeviceProfilePreflight,
            if result.is_ok() {
                Observation::Succeeded
            } else {
                Observation::OpaqueFailure
            },
        );
        result
    }
}

pub(crate) struct DiagnosticPin<'a> {
    inner: &'a dyn PivPinInputPort,
}

pub(crate) fn pin(inner: &dyn PivPinInputPort) -> DiagnosticPin<'_> {
    DiagnosticPin { inner }
}

impl PivPinInputPort for DiagnosticPin<'_> {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        emit(Phase::TtyPinInput, Observation::Started);
        let result = self.inner.read_piv_pin_secret();
        accepted(Phase::TtyPinInput, &result);
        result
    }

    fn read_current_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        emit(Phase::TtyPinInput, Observation::Started);
        let result = self.inner.read_current_piv_pin_secret();
        accepted(Phase::TtyPinInput, &result);
        result
    }

    fn read_new_piv_pin_confirmation(&self) -> Result<ProtectedSecret> {
        emit(Phase::TtyNewPinInput, Observation::Started);
        let result = self.inner.read_new_piv_pin_confirmation();
        accepted(Phase::TtyNewPinInput, &result);
        result
    }
}

pub(crate) struct DiagnosticToken<'a> {
    inner: &'a dyn BitwardenClientSecretInputPort,
}

pub(crate) fn token(inner: &dyn BitwardenClientSecretInputPort) -> DiagnosticToken<'_> {
    DiagnosticToken { inner }
}

impl BitwardenClientSecretInputPort for DiagnosticToken<'_> {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        emit(Phase::TtyTokenInput, Observation::Started);
        let result = self.inner.read_bitwarden_client_secret();
        accepted(Phase::TtyTokenInput, &result);
        result
    }
}

pub(crate) struct DiagnosticDocument<'a> {
    inner: &'a mut dyn BootstrapDocumentInputPort,
}

pub(crate) fn document(inner: &mut dyn BootstrapDocumentInputPort) -> DiagnosticDocument<'_> {
    DiagnosticDocument { inner }
}

impl BootstrapDocumentInputPort for DiagnosticDocument<'_> {
    fn read_bootstrap_secret_document_input(
        &mut self,
    ) -> Result<crate::features::yubikey_lifecycle::ports::public::BootstrapSecretDocumentInput>
    {
        emit(Phase::TtyTokenInput, Observation::Started);
        let result = self.inner.read_bootstrap_secret_document_input();
        accepted(Phase::TtyTokenInput, &result);
        result
    }
}

pub(crate) struct DiagnosticStorage<'a> {
    inner: &'a mut dyn SecretStoragePort,
}

pub(crate) fn storage(inner: &mut dyn SecretStoragePort) -> DiagnosticStorage<'_> {
    DiagnosticStorage { inner }
}

impl SecretStoragePort for DiagnosticStorage<'_> {
    fn begin_piv_pin_setup_preflight(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
    ) -> Result<()> {
        self.inner
            .begin_piv_pin_setup_preflight(serial, current_pin)
    }

    fn change_piv_pin(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
        new_pin: &ProtectedSecret,
    ) -> Result<()> {
        self.inner.change_piv_pin(serial, current_pin, new_pin)
    }

    fn begin_piv_management_session(&mut self, serial: u32, pin: ProtectedSecret) -> Result<()> {
        self.inner.begin_piv_management_session(serial, pin)
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
        traced(Phase::StorageSetupInspection, || {
            self.inner.inspect_secret_storage_setup(serial, probe)
        })
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<Vec<u8>> {
        traced(Phase::StorageInitialization, || {
            self.inner.initialize_secret_storage(serial, intent)
        })
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        manifest_bytes: Vec<u8>,
    ) -> Result<()> {
        traced(Phase::StorageSetupFinalization, || {
            self.inner
                .finalize_secret_storage_setup(serial, manifest_bytes)
        })
    }

    fn clear_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageClearIntent,
    ) -> Result<Vec<u8>> {
        traced(Phase::StorageClear, || {
            self.inner.clear_secret_storage(serial, intent)
        })
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        traced(Phase::StorageWritePreflightInspection, || {
            self.inner.inspect_secret_storage_write(serial, storage)
        })
    }

    fn inspect_secret_storage_status(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageStatusInspection> {
        traced(Phase::StorageStatusInspection, || {
            self.inner.inspect_secret_storage_status(serial, storage)
        })
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        traced(Phase::StorageStore, || {
            self.inner.store_secret(serial, intent, secret)
        })
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        traced(Phase::LocalVerificationInspection, || {
            self.inner.inspect_secret_storage_read(serial, storage)
        })
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
    ) -> Result<ProtectedSecret> {
        traced(Phase::LocalVerificationLoad, || {
            self.inner.load_secret(serial, intent)
        })
    }
}
