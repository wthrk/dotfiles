//! `dotfiles` が呼ぶ外部コマンドの実行と表示を揃える。
//!
//! `dry_run` では実行せず、実行予定のコマンドだけを shell 風に出力する。実行時は終了状態を見て、
//! 失敗したプログラム名をエラーに含める。

use std::ffi::OsString;
use std::process::Command;

use crate::Result;
use anyhow::bail;
use dotfiles_core::command;

/// `dry_run` なら表示のみ、通常時は同じ表示形式で実行して非 0 終了を失敗にする。
pub(crate) fn run<I>(program: impl Into<OsString>, args: I, dry_run: bool) -> Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    println!("$ {}", command::display(&program, &args));
    if dry_run {
        return Ok(());
    }

    let status = Command::new(&program).args(&args).status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {}: {status}", program.to_string_lossy())
    }
}

/// `sudo -H -u <user> <program> ...` の引数列を組み立てる。
pub(crate) fn sudo_as_user_args<I>(user: &str, program: OsString, args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let inherited_path = std::env::var_os("PATH");
    [
        OsString::from("-H"),
        OsString::from("-u"),
        OsString::from(user),
    ]
    .into_iter()
    .chain(
        inherited_path
            .map(|path| {
                vec![OsString::from("env"), {
                    let mut assignment = OsString::from("PATH=");
                    assignment.push(path);
                    assignment
                }]
            })
            .unwrap_or_default(),
    )
    .chain(std::iter::once(program))
    .chain(args)
    .collect()
}

/// 外部コマンドの stdout を捕捉して返す。非 0 終了は失敗にする。
///
/// `run` が stdio を継承するのに対し、本関数は `nix flake archive --json` 出力やリリースノート取得の
/// ように標準出力をプログラム的に読む用途のために `Command::output()` を使う（`environment.rs` の output
/// 取得パターンに揃える）。`dry_run` 経路は持たず常に実行する。失敗時は終了状態と stderr 全文（前後の
/// 空白を `trim` した全体）を文脈に含め、stdout は UTF-8 文字列として返す。stderr は端末へは流さず、失敗時の
/// 診断だけに使う。
pub(crate) fn run_capture<I>(program: impl Into<OsString>, args: I) -> Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new(&program).args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "command failed: {}: {}: {}",
            program.to_string_lossy(),
            output.status,
            stderr.trim()
        );
    }
    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout)
}

/// ユーザー権限実行の sudo 引数が HOME を対象ユーザーへ寄せることを検証する。
#[cfg(test)]
mod tests {
    use super::sudo_as_user_args;
    use std::ffi::OsString;

    /// root daemon から呼ぶ外部コマンドは `sudo -H -u <user> env PATH=...` で包み、呼び出し元 PATH を渡す。
    #[test]
    fn sudo_as_user_args_prefixes_user_context() {
        let args = sudo_as_user_args(
            "alice",
            OsString::from("nix"),
            [OsString::from("flake"), OsString::from("update")],
        );

        assert_eq!(
            &args[..3],
            &[
                OsString::from("-H"),
                OsString::from("-u"),
                OsString::from("alice"),
            ]
        );
        assert!(args.contains(&OsString::from("env")));
        assert!(
            args.iter()
                .any(|arg| arg.to_string_lossy().starts_with("PATH="))
        );
        assert!(args.contains(&OsString::from("nix")));
        assert!(args.ends_with(&[OsString::from("flake"), OsString::from("update")]));
    }
}
