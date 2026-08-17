//! HOME を持たない環境で、明示された設定ディレクトリがそのまま解決に使われることを実行結果で検証する。
//!
//! auto-update daemon は launchd の system domain で起動し、その環境は PATH だけを持つ。`--config-dir`
//! を明示してもプロセス環境の `$HOME` を読む実装だと、この経路の全ユーザー更新が設定ディレクトリを
//! 触る前に `HOME is required` で落ちる。

use std::process::Command;

/// 設定ディレクトリとして渡す、存在しないパス。
///
/// テストは作らず、`flake.nix` が無いことを固定する。解決先がこのパスになったかどうかを、失敗内容に
/// このパスが現れるかで読む。
const MISSING_CONFIG_DIR: &str = "/dotfiles-config-dir-without-home";

/// `$HOME` を持たない実行でも、明示された設定ディレクトリで解決が進む。
///
/// 失敗内容に指定したディレクトリが現れることを、解決に使われた証拠として扱う。`$HOME` を読んで
/// いれば、設定ディレクトリの確認より前に環境不足で落ちてこの文字列が現れない。
#[test]
fn explicit_config_dir_is_used_without_home() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .args([
            "switch",
            "--dry-run",
            "--user",
            "tester",
            "--host",
            "tester-host",
            "--config-dir",
            MISSING_CONFIG_DIR,
        ])
        .env_remove("HOME")
        .output()?;

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains(MISSING_CONFIG_DIR), "{stderr}");
    Ok(())
}
