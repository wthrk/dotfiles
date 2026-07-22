//! parse 済み CLI command を domain command に変換し、port contract で application を起動する。
//!
//! concrete backend の生成・所有は composition root に限る。この module はそこで生成済みの runtime
//! から既存 port trait を借用して command 選択と use case 起動だけを行い、support concrete を直接
//! 生成・参照しない。

use crate::{
    Result, application,
    composition::SecretsRuntime,
    domain::{
        commands::{
            AddGpgBackupSpareCommand, ClearCommand, EnrollPrimaryCommand, EnrollSpareCommand,
            ProvisionBwsTokenCommand, ProvisionPasswordStoreRemoteCommand, PutCommand,
            RegisterGpgBackupCommand, RestoreGpgCommand, RestorePassCommand, RotateBwsTokenCommand,
            SetupCommand, StatusCommand, VerifyYubikeyCommand,
        },
        gpg_backup::PrimaryFingerprint,
        piv::SecretStorageSpec,
        storage::{
            SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
            SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageStatusInspection, SecretStorageWriteInspection, SecretStorageWriteIntent,
        },
        verification::ExternalCheck,
    },
    ports::{BitwardenClientSecretInputPort, DeviceSerialPort, PivPinInputPort, SecretStoragePort},
    support::protection::ProtectedSecret,
};

/// parse 済み command を domain command へ変換し、composition が所有する port を application へ渡す。
pub(super) async fn dispatch(
    options: super::super::SecretsOptions,
    runtime: &mut SecretsRuntime,
) -> Result<()> {
    match options.command {
        super::super::SecretsCommand::Yubikey(options) => match options.command {
            super::super::YubikeyCommand::Setup(options) => application::run_setup_with::run_setup(
                SetupCommand { serial: options.serial }, &mut runtime.device, &runtime.process_io, &mut runtime.storage,
            ),
            super::super::YubikeyCommand::Clear(options) => application::run_clear_with::run_clear(
                ClearCommand { serial: options.serial, confirmed: options.yes }, &mut runtime.device, &runtime.process_io, &mut runtime.storage,
            ),
            super::super::YubikeyCommand::Put(options) => application::run_put::run_put(
                PutCommand { name: options.name, serial: options.serial, force: options.force },
                &mut runtime.device, &runtime.process_io, if options.stdin { &runtime.streamed_token_input } else { &runtime.hidden_token_input }, &mut runtime.storage,
            ),
            super::super::YubikeyCommand::Status(options) => application::run_status_with::run_status_with(
                StatusCommand { serial: options.serial }, &mut runtime.device, &mut runtime.storage, &runtime.process_io,
            ),
            super::super::YubikeyCommand::EnrollPrimary(options) => {
                let document_input: &mut dyn crate::ports::BootstrapDocumentInputPort = if options.stdin_json { &mut runtime.streamed_bootstrap_document_input } else { &mut runtime.hidden_bootstrap_document_input };
                application::run_enroll_primary::run_enroll_primary(
                    EnrollPrimaryCommand { serial: options.serial }, &mut runtime.device, &runtime.process_io, document_input, &mut runtime.storage, &runtime.report,
                )
            }
            super::super::YubikeyCommand::EnrollSpare(options) => {
                let document_input: Option<&mut dyn crate::ports::BootstrapDocumentInputPort> =
                    options.stdin_json.then_some(
                        &mut runtime.streamed_bootstrap_document_input
                            as &mut dyn crate::ports::BootstrapDocumentInputPort,
                    );
                application::run_enroll_spare::run_enroll_spare(
                    EnrollSpareCommand { primary_serial: options.primary_serial, spare_serial: options.spare_serial },
                    &mut runtime.device, &runtime.process_io, document_input, &mut runtime.storage, &runtime.report,
                )
            }
            super::super::YubikeyCommand::RotateBwsToken(options) => application::run_rotate_bws_token::run_rotate_bws_token(
                RotateBwsTokenCommand { serial: options.serial }, &mut runtime.device, &runtime.process_io, if options.stdin { &runtime.streamed_token_input } else { &runtime.hidden_token_input }, &runtime.process_io, &mut runtime.storage, &runtime.report,
            ),
            super::super::YubikeyCommand::ProvisionBwsToken(options) => run_provision_bws_token(
                runtime, ProvisionBwsTokenCommand { serial: options.serial }, options.debug,
            ),
        },
        super::super::SecretsCommand::VerifyYubikey(options) => application::run_verify_yubikey_with::run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: options.serial,
                checks: options.check.into_iter().map(|check| match check { super::super::VerifyCheck::Bws => ExternalCheck::Bws }).collect(),
                all: options.all,
            },
            &mut runtime.device, &mut runtime.storage, &runtime.report, &runtime.bws_client, &mut runtime.gpg_recipient,
        ).await,
        super::super::SecretsCommand::RestoreGpg(options) => application::run_restore_gpg::run_restore_gpg(
            RestoreGpgCommand { serial: options.serial },
            application::run_restore_gpg::RestoreGpgYubikeyRuntime { device: &mut runtime.device, storage: &mut runtime.storage },
            &runtime.bws_client, &mut runtime.gpg_recipient, &mut runtime.backup_cipher,
            application::run_restore_gpg::RestoreGpgIdentityRuntime { keyring: &mut runtime.gpg_keyring, ssh_agent: &mut runtime.ssh_agent },
            &runtime.report,
        ).await,
        super::super::SecretsCommand::RestorePass(options) => application::run_restore_pass::run_restore_pass(
            RestorePassCommand { serial: options.serial },
            application::run_restore_pass::RestorePassYubikeyRuntime { device: &mut runtime.device, storage: &mut runtime.storage },
            &runtime.bws_client, &mut runtime.gpg_keyring, &mut runtime.password_store, &mut runtime.git_clone, &runtime.report,
        ).await,
        super::super::SecretsCommand::GpgBackup(options) => match options.command {
            super::super::GpgBackupCommand::Register(options) => application::run_register_gpg_backup_primary::run_register_gpg_backup_primary(
                RegisterGpgBackupCommand {
                    primary_fingerprint: options.primary_fingerprint.as_deref().map(PrimaryFingerprint::parse).transpose()?,
                    serial: options.serial,
                },
                application::run_register_gpg_backup_primary::RegisterGpgBackupYubikeyRuntime { device: &mut runtime.device, storage: &mut runtime.storage },
                &mut runtime.gpg_keyring, &mut runtime.backup_cipher, &mut runtime.gpg_recipient, &runtime.process_io, &runtime.bws_client,
            ).await,
            super::super::GpgBackupCommand::AddSpare(options) => application::run_add_gpg_backup_spare::run_add_gpg_backup_spare(
                AddGpgBackupSpareCommand { unwrap_serial: options.unwrap_serial, spare_serial: options.spare_serial, assume_overwrite: options.yes },
                &mut runtime.device, &mut runtime.storage, &runtime.bws_client, &mut runtime.gpg_recipient, &runtime.process_io,
            ).await,
        },
        super::super::SecretsCommand::PassRemote(options) => match options.command {
            super::super::PassRemoteCommand::Register(options) => application::run_provision_password_store_remote::run_provision_password_store_remote(
                ProvisionPasswordStoreRemoteCommand { assume_overwrite: options.yes, serial: options.serial, url: options.url },
                &mut runtime.device, &mut runtime.storage, &runtime.bws_client, &runtime.process_io, &runtime.process_io,
            ).await,
        },
    }
}

/// `--debug` の固定済み非秘匿 phase を presentation として出力する entrypoint decorator。
///
/// error 値、PIN、token、secret、長さ、hash、raw APDU/status は受け取らず復元しない。通常 flow の
/// port 呼び出しを一回だけ forwarding し、成功/失敗は opaque result としてだけ表示する。
fn run_provision_bws_token(
    runtime: &mut SecretsRuntime,
    command: ProvisionBwsTokenCommand,
    debug: bool,
) -> Result<()> {
    let presentation = ProvisionDebugPresentation { enabled: debug };
    let mut device = ProvisionDebugDevice {
        inner: &mut runtime.device,
        presentation: &presentation,
    };
    let pin = ProvisionDebugPin {
        inner: &runtime.process_io,
        presentation: &presentation,
    };
    let token = ProvisionDebugToken {
        inner: &runtime.hidden_token_input,
        presentation: &presentation,
    };
    let mut storage = ProvisionDebugStorage {
        inner: &mut runtime.storage,
        presentation: &presentation,
    };
    let result = application::run_provision_yubikey_bws_token::run_provision_yubikey_bws_token(
        command,
        &mut device,
        &pin,
        &token,
        &mut storage,
    );
    presentation.finish(&result);
    result
}

struct ProvisionDebugPresentation {
    enabled: bool,
}

impl ProvisionDebugPresentation {
    fn started(&self, phase: &str) {
        if self.enabled {
            eprintln!("dotfiles-piv-debug phase={phase}-started");
        }
    }
    fn completed<T>(&self, phase: &str, result: &Result<T>) {
        if self.enabled {
            let suffix = if result.is_ok() {
                "succeeded"
            } else {
                "returned result=opaque-error"
            };
            eprintln!("dotfiles-piv-debug phase={phase}-{suffix}");
        }
    }
    fn trace<T>(&self, phase: &str, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        self.started(phase);
        let result = operation();
        self.completed(phase, &result);
        result
    }
    fn trace_accepted_input<T>(
        &self,
        phase: &str,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.started(phase);
        let result = operation();
        if self.enabled {
            let suffix = if result.is_ok() {
                "accepted"
            } else {
                "returned result=opaque-error"
            };
            eprintln!("dotfiles-piv-debug phase={phase}-{suffix}");
        }
        result
    }
    fn resolved(&self, serial: u32) {
        if self.enabled {
            eprintln!("dotfiles-piv-debug phase=target-resolved serial={serial}");
        }
    }
    fn finish<T>(&self, result: &Result<T>) {
        if self.enabled {
            eprintln!(
                "dotfiles-piv-debug phase={}",
                if result.is_ok() {
                    "provisioning-completed"
                } else {
                    "subsequent-phase-not-reached"
                }
            );
        }
    }
}

struct ProvisionDebugDevice<'a> {
    inner: &'a mut dyn DeviceSerialPort,
    presentation: &'a ProvisionDebugPresentation,
}
impl DeviceSerialPort for ProvisionDebugDevice<'_> {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.presentation.started("discovery");
        let result = self.inner.resolve_device_serial(requested);
        match &result {
            Ok(serial) => self.presentation.resolved(*serial),
            Err(_) => self.presentation.completed("discovery", &result),
        }
        result
    }
}
struct ProvisionDebugPin<'a> {
    inner: &'a dyn PivPinInputPort,
    presentation: &'a ProvisionDebugPresentation,
}
impl PivPinInputPort for ProvisionDebugPin<'_> {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.presentation
            .trace_accepted_input("tty-pin-input", || self.inner.read_piv_pin_secret())
    }
}
struct ProvisionDebugToken<'a> {
    inner: &'a dyn BitwardenClientSecretInputPort,
    presentation: &'a ProvisionDebugPresentation,
}
impl BitwardenClientSecretInputPort for ProvisionDebugToken<'_> {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        self.presentation
            .trace_accepted_input("tty-token-input", || {
                self.inner.read_bitwarden_client_secret()
            })
    }
}
struct ProvisionDebugStorage<'a> {
    inner: &'a mut dyn SecretStoragePort,
    presentation: &'a ProvisionDebugPresentation,
}
impl SecretStoragePort for ProvisionDebugStorage<'_> {
    fn begin_piv_management_session(&mut self, serial: u32, pin: ProtectedSecret) -> Result<()> {
        self.presentation.trace("piv-management-session", || {
            self.inner.begin_piv_management_session(serial, pin)
        })
    }
    fn begin_next_piv_management_session(
        &mut self,
        serial: u32,
        pin: ProtectedSecret,
    ) -> Result<()> {
        self.presentation.trace("piv-management-session", || {
            self.inner.begin_next_piv_management_session(serial, pin)
        })
    }
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.presentation.trace("storage-setup-inspection", || {
            self.inner.inspect_secret_storage_setup(serial, probe)
        })
    }
    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<Vec<u8>> {
        self.presentation.trace("storage-initialization", || {
            self.inner.initialize_secret_storage(serial, intent)
        })
    }
    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        manifest_bytes: Vec<u8>,
    ) -> Result<()> {
        self.presentation.trace("storage-setup-finalization", || {
            self.inner
                .finalize_secret_storage_setup(serial, manifest_bytes)
        })
    }
    fn clear_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageClearIntent,
    ) -> Result<Vec<u8>> {
        self.inner.clear_secret_storage(serial, intent)
    }
    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        self.presentation
            .trace("storage-write-preflight-inspection", || {
                self.inner.inspect_secret_storage_write(serial, storage)
            })
    }
    fn inspect_secret_storage_status(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageStatusInspection> {
        self.presentation.trace("storage-status-inspection", || {
            self.inner.inspect_secret_storage_status(serial, storage)
        })
    }
    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        self.presentation.trace("storage-store", || {
            self.inner.store_secret(serial, intent, secret)
        })
    }
    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        self.presentation
            .trace("local-verification-inspection", || {
                self.inner.inspect_secret_storage_read(serial, storage)
            })
    }
    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
    ) -> Result<ProtectedSecret> {
        self.presentation.trace("local-verification-load", || {
            self.inner.load_secret(serial, intent)
        })
    }
}
