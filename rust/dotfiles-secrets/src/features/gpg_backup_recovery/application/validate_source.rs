//! source GnuPG backup の非変更 preflight を application 順序として固定する。

use crate::{
    Result,
    features::gpg_backup_recovery::{
        domain::gpg_backup::PrimaryFingerprint, ports::GpgKeyringPort,
    },
};

/// source keyring の E/A/S 構成と exported transferable secret packet を検査する。
///
/// export は memory 内だけで行い、Sequoia parser が protected/unknown secret packet を拒否する。
/// BWS、YubiKey、GitHub、Git、鍵リングへの mutation は行わないため、provisioning script は外部 mutation
/// より先にこの command を実行できる。
pub(crate) fn run_validate_gpg_backup_source(
    primary: PrimaryFingerprint,
    keyring: &mut dyn GpgKeyringPort,
) -> Result<()> {
    keyring.inspect_imported_key(&primary)?.ensure_usable()?;
    let backup = keyring.export_secret_key(&primary)?;
    let parsed = keyring
        .parse_backup_primary_fingerprint(&backup)?
        .ensure_recovery_capabilities()?;
    if parsed.as_str() != primary.as_str() {
        anyhow::bail!("exported gpg backup primary fingerprint does not match the requested key");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::{
        features::gpg_backup_recovery::{
            domain::{
                gpg_backup::PrimaryFingerprint,
                gpg_restore::{
                    ImportedKeyComposition, OpenPgpBackupFacts, OpenPgpRevocation,
                    OpenPgpSubkeyFacts, ResolvedSubkey, SubkeyCapability,
                },
            },
            ports::gpg::MockGpgKeyringPort,
        },
        foundation::protection::ProtectedSecret,
    };

    use super::run_validate_gpg_backup_source;

    const PRIMARY: &str = "0123456789abcdef0123456789abcdef01234567";

    fn primary() -> crate::Result<PrimaryFingerprint> {
        PrimaryFingerprint::parse(PRIMARY)
    }

    fn usable_imported_key() -> ImportedKeyComposition {
        ImportedKeyComposition::new(
            true,
            vec![
                ResolvedSubkey {
                    capability: SubkeyCapability::Encryption,
                    usable: true,
                },
                ResolvedSubkey {
                    capability: SubkeyCapability::Authentication,
                    usable: true,
                },
                ResolvedSubkey {
                    capability: SubkeyCapability::Signing,
                    usable: true,
                },
            ],
        )
    }

    fn valid_packet_facts() -> crate::Result<OpenPgpBackupFacts> {
        let subkey = |signing, authentication, storage_encryption| OpenPgpSubkeyFacts {
            supported: true,
            alive: true,
            revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
            secret: true,
            signing,
            authentication,
            storage_encryption,
            transport_encryption: false,
        };
        Ok(OpenPgpBackupFacts {
            primary_fingerprint: primary()?,
            certificate_revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
            subkeys: vec![
                subkey(true, false, false),
                subkey(false, true, false),
                subkey(false, false, true),
            ],
        })
    }

    #[test]
    fn validate_inspects_exports_and_parses_without_any_mutating_port() -> crate::Result<()> {
        let mut keyring = MockGpgKeyringPort::new();
        let mut sequence = mockall::Sequence::new();
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(usable_imported_key())
            });
        keyring
            .expect_export_secret_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| ProtectedSecret::from_test_bytes(b"transferable-secret-key"));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| valid_packet_facts());

        run_validate_gpg_backup_source(primary()?, &mut keyring)
    }

    /// 現行 application レイヤーの検証順序を deterministic transcript として固定する。
    /// note: この関数自体は sshcontrol まで観測範囲を持たないため、sshcontrol fail-closed の
    /// 旧仕様再現は別レイヤー（ssh_agent_backend support 側）で扱う前提。
    #[test]
    fn validate_source_records_deterministic_success_transcript() -> crate::Result<()> {
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let mut keyring = MockGpgKeyringPort::new();

        keyring
            .expect_inspect_imported_key()
            .returning({
                let events = Arc::clone(&events);
                move |_| {
                    events.lock().unwrap().push("inspect_imported_key");
                    Ok(usable_imported_key())
                }
            });
        keyring
            .expect_export_secret_key()
            .returning({
                let events = Arc::clone(&events);
                move |_| {
                    events.lock().unwrap().push("export_secret_key");
                    ProtectedSecret::from_test_bytes(b"transferable-secret-key")
                }
            });
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning({
                let events = Arc::clone(&events);
                move |_| {
                    events.lock().unwrap().push("parse_backup_primary_fingerprint");
                    valid_packet_facts()
                }
            });

        run_validate_gpg_backup_source(primary()?, &mut keyring)?;
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![
                "inspect_imported_key",
                "export_secret_key",
                "parse_backup_primary_fingerprint"
            ]
        );
        Ok(())
    }

    #[test]
    fn validate_stops_on_protected_packet_parser_failure_without_followup_keyring_call() {
        let mut keyring = MockGpgKeyringPort::new();
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(usable_imported_key()));
        keyring
            .expect_export_secret_key()
            .times(1)
            .returning(|_| ProtectedSecret::from_test_bytes(b"protected-packet"));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "OpenPGP backup contains passphrase-protected secret key material"
                ))
            });

        let error =
            run_validate_gpg_backup_source(primary().expect("fixture fingerprint"), &mut keyring)
                .expect_err("protected packet must fail source preflight");
        assert!(error.to_string().contains("passphrase-protected"));
    }

    #[test]
    fn validate_source_errors_when_exported_fingerprint_mismatch() -> crate::Result<()> {
        let mut keyring = MockGpgKeyringPort::new();
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(usable_imported_key()));
        keyring
            .expect_export_secret_key()
            .times(1)
            .returning(|_| ProtectedSecret::from_test_bytes(b"transferable-secret-key"));
        keyring.expect_parse_backup_primary_fingerprint().times(1).returning({
            move |_| {
                Ok(OpenPgpBackupFacts {
                    primary_fingerprint: PrimaryFingerprint::parse(
                        "0000000000000000000000000000000000000000",
                    )?,
                    certificate_revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
                    subkeys: vec![
                        OpenPgpSubkeyFacts {
                            supported: true,
                            alive: true,
                            revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
                            secret: true,
                            signing: true,
                            authentication: false,
                            storage_encryption: true,
                            transport_encryption: false,
                        },
                        OpenPgpSubkeyFacts {
                            supported: true,
                            alive: true,
                            revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
                            secret: true,
                            signing: false,
                            authentication: true,
                            storage_encryption: true,
                            transport_encryption: false,
                        },
                        OpenPgpSubkeyFacts {
                            supported: true,
                            alive: true,
                            revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
                            secret: true,
                            signing: false,
                            authentication: false,
                            storage_encryption: true,
                            transport_encryption: false,
                        },
                    ],
                })
            }
        });

        let error =
            run_validate_gpg_backup_source(primary()?, &mut keyring).expect_err("fingerprint mismatch must fail");
        assert!(error.to_string().contains("match"));
        Ok(())
    }

    #[test]
    fn validate_source_does_not_call_mutating_paths_on_parse_failure() -> crate::Result<()> {
        let mut keyring = MockGpgKeyringPort::new();
        let mut sequence = mockall::Sequence::new();

        keyring
            .expect_inspect_imported_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(usable_imported_key()));
        keyring
            .expect_export_secret_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| ProtectedSecret::from_test_bytes(b"transferable-secret-key"));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(anyhow::anyhow!("parse failed"))
            });
        keyring.expect_secret_key_exists().times(0);
        keyring.expect_import_secret_key().times(0);
        keyring.expect_delete_secret_key().times(0);

        let error =
            run_validate_gpg_backup_source(primary()?, &mut keyring).expect_err("parse failure must propagate");
        assert!(error.to_string().contains("parse failed"));
        Ok(())
    }
}
