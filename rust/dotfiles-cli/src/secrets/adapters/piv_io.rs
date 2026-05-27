//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

use anyhow::{Context, bail};
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use serde_json::json;
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

use std::collections::BTreeMap;

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub {
    // Rust private module の internal stub を検査する test-only bridge。
    //
    // stub 本体は `tests/secrets_internal_stub/` に置き、`secrets-internal-test-stub`
    // feature の internal test でのみ module context へ読み込む。xtask 側の実行経路は
    // `rust/tests/checks/src/static_checks.rs` の `secrets_cli` / `secrets::application`
    // internal test command。production command path と port 契約は変えず、runtime
    // real/stub 分岐も作らない。
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/secrets_internal_stub/piv_io_internal_stub.rs"
    ));
}

use crate::{
    Result,
    secrets::{
        domain::{
            manifest::BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT,
            material::SecretMaterial,
            piv::{PIV_PIN_MAX_LEN, PivApplicationVersion, PivObjectId, SecretStorageSpec},
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
            values::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort, PinInputPort,
            ReportPort, SecretInputPort, SecretOutputPort, SecretStoragePort,
            SpareDeviceSerialPort,
        },
        support::{
            process_io,
            protection::{ProtectedSecret, sealed_blob, secret_consumer, secret_random},
        },
    },
};

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;

/// `ProtectedSecret` backend を domain の secret 境界型へ移す adapter 内部変換。
///
/// `Zeroizing` は protection 内部実装詳細として閉じ、adapter 外へ露出させない。
fn material_from_protected(protected: ProtectedSecret) -> SecretMaterial {
    SecretMaterial::from_backend(protected, ProtectedSecret::len, ProtectedSecret::try_clone)
}

/// domain の secret 境界型から protection backend を借用する adapter 内部変換。
///
/// adapter は backend の存在確認だけを行い、平文 bytes や `Zeroizing` の所有権を外へ出さない。
fn protected_from_material(secret: &SecretMaterial) -> Result<&ProtectedSecret> {
    secret
        .as_backend::<ProtectedSecret>()
        .ok_or_else(|| anyhow::anyhow!("secret material backend is not protected memory"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceCandidate {
    serial: u32,
    label: String,
}

/// 実機 YubiKey backend と通信して device 候補列挙/選択を行う翻訳境界。
///
/// adapter 外へ実機型を漏らさないため、この trait は `SelectedSecretDevice` 構築に必要な
/// 最小 capability だけを提供する。
trait RealDeviceIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<YubikeySecretDevice>;
}

/// 選択済み device に対する PIV 操作を `SecretStoragePort` 契約へ写像する翻訳境界。
///
/// この境界は storage 操作に必要な capability のみを宣言し、use case 手順や
/// domain policy の決定を持ち込まない。
trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn piv_application_version(&self) -> PivApplicationVersion;
    fn pin_retries(&mut self) -> Result<u8>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial>;
}

/// `DeviceSelectionAdapter` が利用する device 発見/オープン境界。
///
/// compile-time seam の有無に関わらず同一 capability を維持し、application から見える
/// production command path を単一路に固定する。
trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

/// `dotfiles secrets` の標準入出力境界を担う runtime adapter。
#[derive(Default)]
struct RealSecretIoAdapter;

impl PinInputPort for RealSecretIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_hidden_line(
            "YubiKey PIN: ",
            PIV_PIN_MAX_LEN,
            "YubiKey PIN is too long",
        )?;
        Ok(material_from_protected(protected))
    }
}

impl SecretInputPort for RealSecretIoAdapter {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        process_io::read_visible_line("bw-email: ", 16 * 1024, "visible secret input is too large")
            .map(material_from_protected)
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        process_io::read_hidden_line(
            "bw-password: ",
            16 * 1024,
            "hidden secret input is too large",
        )
        .map(material_from_protected)
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        process_io::read_hidden_line(
            "bws-access-token: ",
            16 * 1024,
            "hidden secret input is too large",
        )
        .map(material_from_protected)
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        let protected = process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")?;
        Ok(material_from_protected(protected))
    }
}

impl BootstrapSecretDocumentInputPort for RealSecretIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        let protected =
            process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?;
        let fields = protected.decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)?;
        Ok(fields
            .into_iter()
            .map(|(name, secret)| (name, material_from_protected(secret)))
            .collect())
    }
}

impl SecretOutputPort for RealSecretIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        process_io::write_secret_stdout(protected_from_material(secret)?)
    }
}

/// device serial 解決と PIN 要否判定を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct DeviceSelectionAdapter {
    device: SelectedDeviceAdapter,
}

impl DeviceSelectionAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        SelectedDeviceDiscoveryIo::discover_devices(&mut self.device)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }
}

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        if let Some(serial) = requested {
            return Ok(serial);
        }
        let devices = self.discover_devices()?;
        match devices.as_slice() {
            [] => bail!("no YubiKey detected"),
            [device] => Ok(device.serial),
            _ => bail!("multiple YubiKeys detected; pass --serial to select a device"),
        }
    }
}

impl SpareDeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.resolve_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

/// process I/O を secret 入出力 port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct ProcessIoAdapter {
    secret_io: RealSecretIoAdapter,
}

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_streamed_secret()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        self.secret_io.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

/// YubiKey object storage を secret storage port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct StorageAdapter {
    device: SelectedDeviceAdapter,
}

impl StorageAdapter {
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }
}

impl SecretStoragePort for StorageAdapter {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let key_exists = device.key_exists()?;
        let piv_version = device.piv_application_version();
        let pin_retries = device.pin_retries()?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in probe.object_ids() {
            if device.read_object(*object_id)?.is_some() {
                occupied_object_ids.push(*object_id);
            }
        }
        Ok(SecretStorageSetupInspection {
            key_exists,
            piv_version,
            pin_retries,
            manifest_bytes,
            occupied_object_ids,
        })
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        mut intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        device.generate_key()?;
        device.write_object(PivObjectId::MANIFEST, &mut intent.manifest_bytes)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let object_exists = device.read_object(storage.object_id)?.is_some();
        Ok(SecretStorageWriteInspection {
            manifest_bytes,
            object_exists,
        })
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(intent.storage.clone(), secret)?;
        device.write_object(intent.storage.object_id, &mut encoded)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let encoded = device.read_object(storage.object_id)?;
        Ok(SecretStorageReadInspection {
            manifest_bytes,
            encoded,
        })
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        device.open_from_storage(intent.storage.clone(), &intent.encoded)
    }
}

/// JSON report 出力を report port 契約へ翻訳する adapter。
pub(crate) struct JsonReportAdapter {
    route: &'static str,
}

impl Default for JsonReportAdapter {
    fn default() -> Self {
        Self {
            route: selected_device_route_label(),
        }
    }
}

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        write_enroll_report_for_route(self.route, summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        write_verify_report_for_route(self.route, summary)
    }
}

/// enroll 結果を route 監査情報つき JSON report へ翻訳して stdout へ出力する。
///
/// この関数は adapter 翻訳境界として、domain/application 値を CLI 出力契約へ
/// 変換する責務のみを持つ。caller 側は route 判定済みの境界値を渡し、
/// ここで route 判定ロジックを追加しない責務を負う。
fn write_enroll_report_for_route(route: &'static str, summary: &EnrollSummary) -> Result<()> {
    let payload = json!({
        "serial": summary.serial,
        "role": report_role(summary.role),
        "checks": report_checks(&summary.checks),
        "device-adapter-route": route,
    });
    let rendered = serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
    println!("{rendered}");
    Ok(())
}

/// verify 結果を route 監査情報つき JSON report へ翻訳して stdout へ出力する。
///
/// adapter では「report 形式への写像」と「出力」だけを扱い、route 選択は扱わない。
/// caller 側は same-route 監査で確定した route 値を渡し、境界外で別ルートを
/// 生成しないことが責務となる。
fn write_verify_report_for_route(route: &'static str, summary: &VerifySummary) -> Result<()> {
    let payload = json!({
        "serial": summary.serial,
        "checks": report_checks(&summary.checks),
        "device-adapter-route": route,
    });
    let rendered = serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
    println!("{rendered}");
    Ok(())
}

/// domain 側の check map を CLI JSON 配列形式へ翻訳する。
///
/// check 名と状態の表記は外部出力契約なので、domain 値の意味を変えずにここで文字列化する。
fn report_checks(
    checks: &std::collections::BTreeMap<CheckName, CheckStatus>,
) -> Vec<serde_json::Value> {
    checks
        .iter()
        .map(|(name, status)| {
            json!({
                "name": report_check(*name),
                "status": report_check_status(*status),
            })
        })
        .collect()
}

/// domain role 列挙値を JSON wire の安定 key 文字列へ写像する。
fn report_role(value: YubikeyRole) -> &'static str {
    match value {
        YubikeyRole::Primary => "primary",
        YubikeyRole::Spare => "spare",
    }
}

/// domain check 名を互換性維持対象の JSON key 文字列へ翻訳する。
fn report_check(value: CheckName) -> &'static str {
    match value {
        CheckName::Setup => "setup",
        CheckName::BwEmail => "bw-email",
        CheckName::BwPassword => "bw-password",
        CheckName::BwsAccessToken => "bws-access-token",
        CheckName::LocalStorage => "local-storage",
        CheckName::Bws => "bws",
        CheckName::BwLogin => "bw-login",
    }
}

/// domain status 列挙値を report wire status 文字列へ翻訳する。
fn report_check_status(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Ok => "ok",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    }
}

/// internal test 専用 stub の compile-time 注入点。
///
/// production command path と port 契約は固定し、backend 実装だけを compile-time で切り替える。
/// `secrets-internal-test-stub` は internal test 専用であり、xtask 側の実行箇所は
/// `rust/tests/checks/src/static_checks.rs` の internal test command 定義で固定する。
struct SelectedDeviceAdapter;

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn selected_device_route_label() -> &'static str {
    "real"
}

#[cfg(feature = "secrets-internal-test-stub")]
fn selected_device_route_label() -> &'static str {
    "stub"
}

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        eprintln!(
            "{ADAPTER_ROUTE_AUDIT_PREFIX}={}",
            selected_device_route_label()
        );
        Self
    }
}

struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
}

impl SelectedSecretDevice {
    fn new(device: impl SecretDeviceIo + 'static) -> Self {
        Self {
            inner: Box::new(device),
        }
    }
}

impl SecretDeviceIo for SelectedSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        self.inner.key_exists()
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        self.inner.piv_application_version()
    }

    fn pin_retries(&mut self) -> Result<u8> {
        self.inner.pin_retries()
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        self.inner.check_management_auth_preconditions()
    }

    fn generate_key(&mut self) -> Result<()> {
        self.inner.generate_key()
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        self.inner.read_object(object_id)
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.inner.write_object(object_id, value)
    }

    fn requires_pin_input(&self) -> bool {
        self.inner.requires_pin_input()
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        self.inner.verify_pin(pin)
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        self.inner.seal_for_storage(storage, plaintext)
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        self.inner.open_from_storage(storage, encoded)
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        RealDeviceIo::discover_devices(&mut RealDeviceAdapter)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        RealDeviceIo::open_device_by_serial(&mut RealDeviceAdapter, serial)
            .map(SelectedSecretDevice::new)
    }
}

struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

impl YubikeySecretDevice {
    fn open_by_serial(serial: u32) -> Result<Self> {
        Ok(Self {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        })
    }

    fn default_management_key(&self) -> Result<MgmKey> {
        // 現行フェーズは factory-default management key 前提で固定する。
        // repository 固有の非既定 management key の取得・注入は次フェーズの鍵管理作業で扱う。
        MgmKey::get_default(&self.yubikey).context("failed to load default YubiKey management key")
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        let version = self.yubikey.version();
        PivApplicationVersion {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
        }
    }

    fn wrap_content_key(&mut self, key: &ProtectedSecret) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    fn unwrap_content_key(&mut self, wrapped_key: &[u8]) -> Result<ProtectedSecret> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        let decrypted = piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?;
        sealed_blob::unwrap_content_key(&decrypted, 256)
    }
}

struct RealDeviceAdapter;

impl RealDeviceIo for RealDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        let mut context = YubikeyContext::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let yubikey = reader.open()?;
            devices.push(DeviceCandidate {
                serial: yubikey.serial().0,
                label,
            });
        }
        Ok(devices)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<YubikeySecretDevice> {
        YubikeySecretDevice::open_by_serial(serial)
    }
}

impl SecretDeviceIo for YubikeySecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(_) => Ok(true),
            Err(yubikey::Error::NotFound) => {
                match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
                    Ok(_) => Ok(true),
                    Err(yubikey::Error::NotFound) => Ok(false),
                    Err(err) => Err(err.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        self.piv_application_version()
    }

    fn pin_retries(&mut self) -> Result<u8> {
        self.yubikey.get_pin_retries().map_err(anyhow::Error::new)
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Once,
            TouchPolicy::Always,
        )?;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object_id.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        self.yubikey.save_object(object_id.value(), value)?;
        Ok(())
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        secret_consumer::consume(
            protected_from_material(pin)?,
            &mut YubikeyPinVerifier(&mut self.yubikey),
        )?;
        self.pin_verified = true;
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_with_key_wrap(
            sealed_blob::SealWithKeyWrapRequest {
                payload_id: storage.secret_id,
                plaintext: protected_from_material(plaintext)?,
                aad: &storage.additional_data,
            },
            |content_key| self.wrap_content_key(content_key),
        )
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        sealed_blob::open_with_key_unwrap(
            encoded,
            storage.secret_id,
            |wrapped_key| self.unwrap_content_key(wrapped_key),
            &storage.additional_data,
        )
        .map(material_from_protected)
    }
}

struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

impl secret_consumer::SecretConsumer for YubikeyPinVerifier<'_> {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.verify_pin(bytes).map_err(anyhow::Error::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_device_adapter_route_is_compile_time_selected() {
        #[cfg(not(feature = "secrets-internal-test-stub"))]
        assert_eq!(selected_device_route_label(), "real");
        #[cfg(feature = "secrets-internal-test-stub")]
        assert_eq!(selected_device_route_label(), "stub");
    }
}
