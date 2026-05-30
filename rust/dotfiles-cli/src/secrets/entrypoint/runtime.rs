//! entrypoint から application dispatch へ渡す runtime 橋渡し境界。
//!
//! adapter concrete の所有と初期化は親 module（`secrets.rs` の composition root）が担い、
//! entrypoint は parse 済み command の橋渡しだけを行う。

use crate::Result;

use super::dispatch;

/// parse 済み command を application dispatch へ橋渡しする。
pub(super) async fn run(
    options: super::super::SecretsOptions,
    ports: &mut super::super::RuntimePorts,
) -> Result<()> {
    dispatch::dispatch(options, ports).await
}
