use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::values::{VerifySummary, VerifyYubikeyCommand},
    ports::{self},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// local storage 検証を完了条件の先頭に固定し、未実装の外部確認は report 境界で通知して
/// 明示的に停止することで、verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DevicePinPolicyPort + ports::PinInputPort + ports::StorageVerifyPort + ports::ReportPort,
>(
    command: VerifyYubikeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let requested = command.requested_external_checks()?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(serial, pin.as_ref())?;
    if !requested.is_empty() {
        boundary.write_verify_report(&VerifySummary::external_checks_unavailable(
            serial,
            requested.iter().copied(),
        ))?;
        let requested_names = requested
            .iter()
            .map(|check| check.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.write_verify_report(&VerifySummary::local_storage_verified(serial))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::domain::{material::SecretMaterial, values::ExternalCheck};

    #[derive(Default)]
    struct FakeBoundary {
        requires_pin: bool,
        verify_calls: usize,
    }

    impl ports::DevicePinPolicyPort for FakeBoundary {
        fn device_requires_pin(&mut self, _serial: u32) -> Result<bool> {
            Ok(self.requires_pin)
        }
    }
    impl ports::PinInputPort for FakeBoundary {
        fn read_pin(&self) -> Result<SecretMaterial> {
            Ok(SecretMaterial::from_vec(b"123456".to_vec()))
        }
    }
    impl ports::StorageVerifyPort for FakeBoundary {
        fn verify_local_storage(
            &mut self,
            _serial: u32,
            _pin: Option<&SecretMaterial>,
        ) -> Result<()> {
            self.verify_calls += 1;
            Ok(())
        }
    }
    impl ports::ReportPort for FakeBoundary {
        fn write_enroll_report(
            &self,
            _summary: &crate::secrets::domain::values::EnrollSummary,
        ) -> Result<()> {
            unreachable!()
        }
        fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
            let _ = summary;
            Ok(())
        }
    }

    #[test]
    fn verify_requests_pin_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![],
                all: false,
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(boundary.verify_calls, 1);
    }

    #[test]
    fn verify_stops_when_external_checks_requested() {
        let mut boundary = FakeBoundary::default();
        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        );
        assert!(result.is_err());
    }
}
