//! YubiKey object I/O と `SecretStoragePort` 契約を接続する storage adapter。
//!
//! manifest/object 読み書きと暗号化 payload の受け渡しを担当し、use case 分岐は保持しない。

use std::collections::BTreeMap;

use crate::{
    Result,
    secrets::{
        domain::{
            piv::{PivObjectId, SecretStorageSpec},
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
        ports::yubikey::SecretStoragePort,
        support::protection::{ProtectedSecret, SecretSession},
    },
};

use super::{
    SecretDeviceIo, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};

/// YubiKey PIV object I/O を `SecretStoragePort` の inspection/intent 契約へ翻訳する adapter。
///
/// caller は domain が判定した intent を渡す。adapter は device API、object 読み書き、
/// protection 操作の接続だけを担い、storage plan や上書き可否は再判定しない。
///
/// ただし鍵生成を伴う setup では、`initialize_secret_storage` の鍵生成で得た slot 公開鍵を後続の
/// `store_secret` まで保持する必要がある。生成直後は slot の metadata/certificate を device から読み戻せ
/// ないためである。そのため本 adapter は serial ごとの生成公開鍵を `generated_public_keys` に capture し、
/// init→store→finalize の lifecycle をまたいで引き回す唯一の state を持つ。これは封緘鍵の引き回しに限定した
/// connector state であり、storage plan や上書き可否の業務判定を保持するものではない。
#[derive(Default)]
pub(super) struct StorageAdapter {
    device: SelectedDeviceAdapter,
    /// 鍵生成を伴う setup で capture した serial ごとの生成 slot 公開鍵（PKCS1 DER）。
    /// 直後の `store_secret` の seal で消費し、finalize と非生成 init で clear する。
    generated_public_keys: BTreeMap<u32, Vec<u8>>,
}

impl StorageAdapter {
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }

    /// 鍵生成直後の slot 公開鍵を serial へ capture する lifecycle helper。
    ///
    /// 不変条件: 生成（`Some`）した時点でだけ capture し、後続の `store_secret` の seal で消費される。
    /// `generate_key` が `None`（生成なし）を返した serial は capture せず、既存の cache を消す。
    fn remember_generated_public_key(&mut self, serial: u32, public_key: Option<Vec<u8>>) {
        match public_key {
            Some(pkcs1_bytes) => {
                self.generated_public_keys.insert(serial, pkcs1_bytes);
            }
            None => {
                self.generated_public_keys.remove(&serial);
            }
        }
    }

    /// store の seal が同一鍵で封緘するために、capture 済み生成公開鍵を借用する lifecycle helper。
    ///
    /// 不変条件: capture が無い serial では `None` を返し、その場合 store は slot 読み戻しに委ねる。
    fn generated_public_key_for_serial(&self, serial: u32) -> Option<&[u8]> {
        self.generated_public_keys.get(&serial).map(Vec::as_slice)
    }

    /// capture した生成公開鍵を破棄する lifecycle helper。
    ///
    /// 不変条件: finalize 成功時と、鍵生成を伴わない init 経路で呼び、引き回しを次の setup へ持ち越さない。
    fn clear_generated_public_key(&mut self, serial: u32) {
        self.generated_public_keys.remove(&serial);
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
        intent: SecretStorageSetupIntent,
        pin: Option<&ProtectedSecret>,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        if intent.key_generation_required {
            self.clear_generated_public_key(serial);
            if let Some(pin) = pin {
                device.verify_pin(pin)?;
            } else if device.requires_pin_input() {
                anyhow::bail!("PIN is required to generate the YubiKey secret storage key");
            }
            device.check_management_auth_preconditions()?;
            self.remember_generated_public_key(serial, device.generate_key()?);
        } else {
            self.clear_generated_public_key(serial);
        }
        Ok(())
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        mut intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        let result = device.write_object(PivObjectId::MANIFEST, &mut intent.manifest_bytes);
        if result.is_ok() {
            self.clear_generated_public_key(serial);
        }
        result
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
        secret: &ProtectedSecret,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        let generated_public_key = self.generated_public_key_for_serial(serial);
        let mut encoded =
            device.seal_for_storage(intent.storage.clone(), secret, generated_public_key)?;
        device.write_object(intent.storage.object_id, &mut encoded)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        let _session = SecretSession::start()?;
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
        pin: Option<&ProtectedSecret>,
    ) -> Result<ProtectedSecret> {
        let _session = SecretSession::start()?;
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                anyhow::bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        device.open_from_storage(intent.storage.clone(), &intent.encoded)
    }
}

/// production `StorageAdapter` の生成公開鍵 cache helper（serial scope と clear）を検証する inline unit test。
///
/// device I/O を伴わない private helper の不変条件だけを対象とし、capture した生成公開鍵が serial 単位に
/// 分離されること、`clear_generated_public_key`（finalize 成功時と非生成 init で呼ぶ）が当該 serial だけを
/// 破棄し引き回しを次 setup へ持ち越さないことを固定する。
///
/// init の `generate_key` Some を capture し store の seal が同一鍵で封緘する引き回し（Some 経路）の
/// end-to-end 網羅と、PIN 未検証 fail-closed の網羅は、device 肩代わり double を production へ持ち込まず、
/// `secrets-internal-test-stub` feature の internal backend stub + CLI 統合テスト
/// （`tests/secrets_cli.rs` の enroll-primary/enroll-spare が stub の generate_key Some と
/// `sealed_with_generated_key` 観測で封緘鍵を照合し、enroll_spare_fails_closed_when_key_generation_requires_pin が
/// PIN 未検証停止を照合する）に置く。
#[cfg(test)]
mod tests {
    use super::StorageAdapter;

    #[test]
    fn generated_public_key_cache_is_scoped_by_serial() {
        let mut adapter = StorageAdapter::default();

        adapter.remember_generated_public_key(100, Some(vec![1, 2, 3]));
        adapter.remember_generated_public_key(200, Some(vec![4, 5, 6]));

        assert_eq!(
            adapter.generated_public_key_for_serial(100),
            Some(&[1, 2, 3][..])
        );
        assert_eq!(
            adapter.generated_public_key_for_serial(200),
            Some(&[4, 5, 6][..])
        );
    }

    #[test]
    fn clearing_one_serial_keeps_other_cached_keys() {
        let mut adapter = StorageAdapter::default();

        adapter.remember_generated_public_key(100, Some(vec![1, 2, 3]));
        adapter.remember_generated_public_key(200, Some(vec![4, 5, 6]));
        adapter.clear_generated_public_key(100);

        assert_eq!(adapter.generated_public_key_for_serial(100), None);
        assert_eq!(
            adapter.generated_public_key_for_serial(200),
            Some(&[4, 5, 6][..])
        );
    }
}
