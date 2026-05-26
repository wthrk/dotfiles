//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    secrets_structure(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)
}

/// Rust ワークスペース全体で、警告を失敗扱いにして整形、型検査、lint、テストを回す。
fn rust(shell: &Shell) -> Result<()> {
    step("cargo fmt");
    cmd!(shell, "cargo fmt --all -- --check").run()?;
    step("cargo check");
    cmd!(shell, "env RUSTFLAGS='-D warnings' cargo check --workspace").run()?;
    step("cargo clippy");
    cmd!(
        shell,
        "cargo clippy --workspace --all-targets -- -D warnings"
    )
    .run()?;
    step("cargo clippy secrets CLI stub crate");
    cmd!(
        shell,
        "cargo clippy -p dotfiles-cli-secrets-test-stub --test secrets_cli -- -D warnings"
    )
    .run()?;
    step("cargo test");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets"
    )
    .run()?;
    step("cargo test secrets CLI stub crate");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test -p dotfiles-cli-secrets-test-stub --test secrets_cli"
    )
    .run()?;
    Ok(())
}

/// secret-recovery 実装で要求される構造制約を、軽量な静的検査で常時検証する。
fn secrets_structure(shell: &Shell) -> Result<()> {
    step("secrets structure checks");
    cmd!(
        shell,
        "rg -n 'mod structure_tests' rust/dotfiles-cli/src/secrets.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'enum SecretInputSource|enum EnrollmentInputSource|EnrollmentBytes' rust/dotfiles-cli/src/secrets/ports.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'prompt_yes_no|stdin_is_terminal|stdout_is_terminal|read_enrollment_json_bytes|require_serial|require_option|ask_continue_rotation' rust/dotfiles-cli/src/secrets/ports.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'enum\\s+.*InputSource|EnrollmentSecretSet|EnrollmentBytes|serde_json|from_slice|from_str|read_enrollment_json_bytes|prompt_yes_no|stdin_is_terminal|stdout_is_terminal|require_serial|require_option|ask_continue_rotation' rust/dotfiles-cli/src/secrets/ports.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'application::' rust/dotfiles-cli/src/secrets/adapters/piv_io.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'pub\\(super\\) use' rust/dotfiles-cli/src/secrets/adapters.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'TestStub|StubDevice|Fake|Mock|Dummy|test[_-]?double|dotfiles-stub|cfg\\(feature\\s*=\\s*\"dotfiles-stub\"\\)' rust/dotfiles-cli/src/secrets && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'prompt|stdin|stdout|tty|terminal|console_io|read_line|read_hidden|ask_' rust/dotfiles-cli/src/secrets/support && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'YubiKey secret' rust/dotfiles-cli/src/secrets/support/aead.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "test -e rust/dotfiles-cli/src/secrets/application/use_case.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "test -d rust/dotfiles-cli/src/secrets/application/use_case && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n '#\\[path\\s*=|use_case_shared|mod\\s+use_case\\b' rust/dotfiles-cli/src/secrets/application.rs rust/dotfiles-cli/src/secrets/application/*.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "ls rust/dotfiles-cli/src/secrets/application/run_*.rs >/dev/null"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'VerifyCheck|EnrollPrimaryOptions|RotateBwsTokenOptions' rust/dotfiles-cli/src/secrets/application/*.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'CheckName|CheckStatus|EnrollSummary|VerifySummary|YubikeyRole|trait SecretDevice|std::io::Write' rust/dotfiles-cli/src/secrets/domain && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'application::|adapters::|support::|println!|std::io::stdin|std::io::stdout|serde_json::|aes_gcm|yubikey::|piv::' rust/dotfiles-cli/src/secrets/application/*.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'application::use_case|CheckName|CheckStatus|EnrollSummary|VerifySummary|YubikeyRole' rust/dotfiles-cli/src/secrets/adapters/yubikey.rs && exit 1 || true"
    )
    .run()?;
    cmd!(
        shell,
        "rg -n 'application::|adapters::|ports::' rust/dotfiles-cli/src/secrets/domain/wire.rs && exit 1 || true"
    )
    .run()?;
    Ok(())
}

/// bootstrap 用 shell script の構文を検証する。
fn shell_scripts(shell: &Shell) -> Result<()> {
    step("shell scripts");
    cmd!(shell, "bash -n scripts/bootstrap.sh").run()?;
    Ok(())
}

/// GitHub Actions workflow の構文と式を actionlint で検証する。
fn github_actions(shell: &Shell) -> Result<()> {
    step("GitHub Actions workflows");
    cmd!(shell, "actionlint").run()?;
    Ok(())
}

/// lock file が存在する状態で、Nix flake の評価と Nix ファイルの整形を検証する。
fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
    cmd!(shell, "test -s flake.lock").run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(shell, "nix fmt -- --ci {files...}").run()?;
    }
    step("nix flake check");
    cmd!(shell, "nix flake check --no-update-lock-file --all-systems").run()?;
    Ok(())
}

/// devShell に入っている `nil` で Nix 診断を実行し、モジュール評価の静的な崩れを検出する。
fn nix_diagnostics(shell: &Shell) -> Result<()> {
    let files = nix_files(shell)?;
    if files.is_empty() {
        return Ok(());
    }

    step("nil diagnostics");
    cmd!(shell, "nil diagnostics --deny-warnings {files...}").run()?;
    Ok(())
}

/// `target` 配下を除外し、整形と nil 診断の対象になる Nix ファイルだけを列挙する。
fn nix_files(shell: &Shell) -> Result<Vec<String>> {
    Ok(cmd!(
        shell,
        "find . -path ./target -prune -o -name '*.nix' -type f -print"
    )
    .read()?
    .lines()
    .map(|path| path.trim_start_matches("./"))
    .map(ToOwned::to_owned)
    .collect())
}
