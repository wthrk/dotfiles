//! Crate-root composition boundary for the command facade.
//!
//! This is the sole owner of production concrete construction. It creates the entrypoint-owned
//! invocation DTO and directly starts the entrypoint; it never selects a command or implements a
//! product workflow.

use crate::{
    features::{
        bws_secrets::support::backend::BwsClientBackend,
        gpg_backup_recovery::support::backend::{
            BackupCipherBackend, GpgKeyringBackend, SshAgentBackend,
        },
        password_store::support::backend::{GitCloneBackend, PasswordStoreBackend},
    },
    features::{
        cli_interaction::{
            presentation::io,
            support::{clock, process_io},
        },
        command_facade::entrypoint::{self, SecretsInvocation},
        yubikey_lifecycle::{
            presentation::diagnostics,
            support::{process_diagnostic, yubikey_backend, yubikey_storage},
        },
    },
};

fn write_process_diagnostic_line(line: &str) {
    process_io::write_stderr_line(line);
}

fn observe_backend_operation(
    operation: process_diagnostic::Operation,
    observation: process_diagnostic::Observation,
) {
    let phase = match operation {
        process_diagnostic::Operation::SessionOpen => diagnostics::BackendPhase::SessionOpen,
        process_diagnostic::Operation::VerifyInvocation => {
            diagnostics::BackendPhase::VerifyInvocation
        }
        process_diagnostic::Operation::ManagementKeyAuthentication => {
            diagnostics::BackendPhase::ManagementKeyAuthentication
        }
    };
    let succeeded = match observation {
        process_diagnostic::Observation::Started => None,
        process_diagnostic::Observation::Succeeded => Some(true),
        process_diagnostic::Observation::Failed => Some(false),
    };
    diagnostics::observe_backend(phase, succeeded);
}

/// Root-owned concrete catalog. It is never exposed beyond this composition module.
struct SecretsRuntime {
    device: yubikey_backend::YubikeyDeviceBackend,
    process_io: io::ProcessPresentation,
    clock: clock::SystemClock,
    piv_pin: crate::features::yubikey_lifecycle::presentation::piv_pin_input::TerminalPivPinInput,
    hidden_token_input: io::HiddenTokenInput,
    streamed_token_input: io::StreamedTokenInput,
    hidden_bootstrap_document_input: io::HiddenBootstrapDocumentInput,
    streamed_bootstrap_document_input: io::StreamedBootstrapDocumentInput,
    storage: yubikey_storage::YubikeyStorageBackend,
    report: io::JsonReport,
    bws_client: BwsClientBackend,
    gpg_recipient: yubikey_backend::YubikeyRecipientBackend,
    backup_cipher: BackupCipherBackend,
    gpg_keyring: GpgKeyringBackend,
    ssh_agent: SshAgentBackend,
    password_store: PasswordStoreBackend,
    git_clone: GitCloneBackend,
    gpg_agent_socket: crate::features::gpg_backup_recovery::support::backend::GpgAgentSocketBackend,
}

impl SecretsRuntime {
    fn production() -> Self {
        Self {
            device: yubikey_backend::YubikeyDeviceBackend::default(),
            process_io: io::ProcessPresentation::new(process_io::stdin_is_terminal, process_io::read_control_line, process_io::read_visible_plain_line, process_io::read_stdin_plain_line, process_io::write_stdout_line),
            clock: clock::SystemClock,
            piv_pin: crate::features::yubikey_lifecycle::presentation::piv_pin_input::TerminalPivPinInput::new(process_io::read_hidden_tty_line),
            hidden_token_input: io::HiddenTokenInput::new(process_io::read_hidden_tty_line),
            streamed_token_input: io::StreamedTokenInput::new(process_io::read_stdin_line),
            hidden_bootstrap_document_input: io::HiddenBootstrapDocumentInput::new(process_io::read_hidden_line),
            streamed_bootstrap_document_input: io::StreamedBootstrapDocumentInput::new(process_io::read_stdin_all),
            storage: yubikey_storage::YubikeyStorageBackend::default(),
            report: io::JsonReport::new(process_io::write_stdout_line),
            bws_client: BwsClientBackend,
            gpg_recipient: yubikey_backend::YubikeyRecipientBackend,
            backup_cipher: BackupCipherBackend,
            gpg_keyring: GpgKeyringBackend,
            ssh_agent: SshAgentBackend,
            password_store: PasswordStoreBackend,
            git_clone: GitCloneBackend,
            gpg_agent_socket: crate::features::gpg_backup_recovery::support::backend::GpgAgentSocketBackend,
        }
    }
}

/// Construct the entrypoint invocation from root-owned concrete receivers and start it directly.
pub(crate) async fn start(options: crate::SecretsOptions) -> crate::Result<()> {
    let _session = crate::foundation::protection::SecretSession::start()?;
    let mut runtime = SecretsRuntime::production();
    let controller = diagnostics::DiagnosticController::new(write_process_diagnostic_line);
    let _backend_observation = process_diagnostic::Scope::new(observe_backend_operation);
    let mut device = diagnostics::device(&mut runtime.device);
    let pin = diagnostics::pin(&runtime.piv_pin);
    let hidden_token = diagnostics::token(&runtime.hidden_token_input);
    let streamed_token = diagnostics::token(&runtime.streamed_token_input);
    let mut hidden_document = diagnostics::document(&mut runtime.hidden_bootstrap_document_input);
    let mut streamed_document =
        diagnostics::document(&mut runtime.streamed_bootstrap_document_input);
    let mut storage = diagnostics::storage(&mut runtime.storage);
    entrypoint::start(
        options,
        SecretsInvocation {
            device: &mut device,
            piv_pin: &pin,
            hidden_token_input: &hidden_token,
            streamed_token_input: &streamed_token,
            hidden_bootstrap_document_input: &mut hidden_document,
            streamed_bootstrap_document_input: &mut streamed_document,
            storage: &mut storage,
            report: &runtime.report,
            bws_client: &runtime.bws_client,
            gpg_recipient: &mut runtime.gpg_recipient,
            backup_cipher: &mut runtime.backup_cipher,
            gpg_keyring: &mut runtime.gpg_keyring,
            ssh_agent: &mut runtime.ssh_agent,
            password_store: &mut runtime.password_store,
            git_clone: &mut runtime.git_clone,
            gpg_agent_socket: &mut runtime.gpg_agent_socket,
            status_output: &runtime.process_io,
            rotation_continuation: &runtime.process_io,
            clock: &runtime.clock,
            backup_update_confirmation: &runtime.process_io,
            password_store_remote_input: &runtime.process_io,
            diagnostics: &controller,
        },
    )
    .await
}

/// Construct the `dotfiles gpg` receiver set and start its entrypoint directly.
pub(crate) fn start_gpg(options: crate::GpgOptions) -> crate::Result<()> {
    let mut keyring = GpgKeyringBackend;
    let output = io::ProcessPresentation::new(
        process_io::stdin_is_terminal,
        process_io::read_control_line,
        process_io::read_visible_plain_line,
        process_io::read_stdin_plain_line,
        process_io::write_stdout_line,
    );
    entrypoint::start_gpg(
        options,
        entrypoint::GpgInvocation {
            keyring: &mut keyring,
            output: &output,
        },
    )
}
