//! entrypoint の runtime session と adapter catalog 初期化を扱う composition 境界。
//!
//! support session の開始と adapter 所有関係をここに閉じ、root module と command dispatch へ
//! secret protection の初期化責務を混ぜない。

use crate::{Result, secrets::support::protection::SecretSession};

use super::{adapter_catalog::EntrypointPorts, dispatch};

/// secret 保護 session を開始して adapter catalog を構築し、parse 済み command を dispatch する。
pub(super) async fn run(options: super::super::SecretsOptions) -> Result<()> {
    let _session = SecretSession::start()?;
    let mut ports = EntrypointPorts::production();
    dispatch::dispatch(options, &mut ports).await
}
