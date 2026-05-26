//! `secrets-test-stub` feature の same-route 実行系と CLI 統合テストが共有する契約名。

pub const USE_TEST_STUB_ENV: &str = "DOTFILES_SECRETS_USE_TEST_STUB_YUBIKEY";
pub const TEST_STUB_CONTEXT_ENV: &str = "DOTFILES_SECRETS_TEST_STUB_CONTEXT";
pub const TEST_STUB_CONTEXT_VALUE: &str = "integration-test";
pub const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

pub const SEED_BW_EMAIL_ENV: &str = "DOTFILES_TEST_STUB_SEED_BW_EMAIL";
pub const SEED_BW_PASSWORD_ENV: &str = "DOTFILES_TEST_STUB_SEED_BW_PASSWORD";
pub const SEED_BWS_ACCESS_TOKEN_ENV: &str = "DOTFILES_TEST_STUB_SEED_BWS_ACCESS_TOKEN";
pub const CORRUPT_SECRET_ENV: &str = "DOTFILES_TEST_STUB_CORRUPT_SECRET";
pub const READ_PIN_FROM_TTY_ENV: &str = "DOTFILES_TEST_STUB_READ_PIN_FROM_TTY";
pub const STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE";
pub const PRIMARY_STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE_2001";
pub const SPARE_STUB_STATE_ENV: &str = "DOTFILES_TEST_STUB_STATE_2002";
pub const WRITE_EVENT_PREFIX: &str = "DOTFILES_TEST_STUB_WRITE";
pub const PRIMARY_SERIAL: u32 = 2001;
pub const SPARE_SERIAL: u32 = 2002;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwsAccessToken,
}

impl StubState {
    pub fn env_value(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Initialized => "initialized",
            Self::Provisioned => "provisioned",
            Self::WritableBwsAccessToken => "writable-bws-access-token",
        }
    }

    pub fn parse_env_value(value: &str) -> Option<Self> {
        match value {
            "fresh" => Some(Self::Fresh),
            "initialized" => Some(Self::Initialized),
            "provisioned" => Some(Self::Provisioned),
            "writable-bws-access-token" => Some(Self::WritableBwsAccessToken),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubSecret {
    BwEmail,
    BwPassword,
    BwsAccessToken,
}

impl StubSecret {
    pub fn contract_name(self) -> &'static str {
        match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        }
    }
}

pub fn format_write_event(serial: u32, secret_name: &str, value: &str) -> String {
    format!("{WRITE_EVENT_PREFIX} serial={serial} name={secret_name} value={value}")
}
