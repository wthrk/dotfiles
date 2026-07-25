//! 実行を伴うテスト検証。
//!
//! `static_checks` は source と構文だけを検査し、ここでは実行主体を一度ずつ所有する。
//! この分離により、static required check が test binary、feature stub、shell fixture を
//! 暗黙に起動することを防ぐ。

use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// workspace test、internal-stub CLI integration、provision shell fixture を各一回実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    workspace_tests(&shell)?;
    internal_stub_cli_integration(&shell)?;
    provision_shell_test(&shell)
}

fn workspace_tests(shell: &Shell) -> Result<()> {
    step("cargo test workspace");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets"
    )
    .run()?;
    Ok(())
}

fn internal_stub_cli_integration(shell: &Shell) -> Result<()> {
    step("cargo test secrets internal stub CLI integration");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test -p dotfiles-cli --no-default-features --features secrets-internal-test-stub --test secrets_cli"
    )
    .run()?;
    Ok(())
}

fn provision_shell_test(shell: &Shell) -> Result<()> {
    step("provision shell test");
    cmd!(
        shell,
        "bash scripts/provision-secret-recovery-source_test.sh"
    )
    .run()?;
    Ok(())
}
