//! `secrets-internal-test-stub` feature 専用の adapter backend stub 群。
//!
//! この module は test 時だけ compile され、production build には含めない。
//! integration test はこの module を import せず、feature 有効でビルドされた同じ `dotfiles`
//! binary を実行し、production command path は変更しないまま
//! `DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH` の state file を介して backend 挙動を検証する。
//! real/stub 切替は runtime 分岐ではなく compile-time feature selection。

#[cfg(feature = "secrets-internal-test-stub")]
pub(super) mod bw;
#[cfg(feature = "secrets-internal-test-stub")]
pub(super) mod state;
#[cfg(feature = "secrets-internal-test-stub")]
pub(super) mod yubikey;
