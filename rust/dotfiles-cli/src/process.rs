//! `dotfiles` が呼ぶ外部コマンドの実行と表示を揃える。
//!
//! `dry_run` では実行せず、実行予定のコマンドだけを shell 風に出力する。実行時は終了状態を見て、
//! 失敗したプログラム名をエラーに含める。
//!
//! 無人実行の呼び出し側は期限を渡せる。期限を過ぎたコマンドは打ち切って失敗にし、停止した 1 件が
//! 後続の処理を無期限に止めないようにする。

use std::ffi::OsString;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use crate::Result;
use anyhow::bail;
use dotfiles_core::command;

/// 期限付き実行で子プロセスの終了を確認する間隔。
///
/// 期限は時間単位なので取りこぼしの粒度は問題にならない。短くしているのは、期限より先に終わる
/// 通常のコマンドで待ち時間を足さないためである。
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// `dry_run` なら表示のみ、通常時は同じ表示形式で実行して非 0 終了を失敗にする。
///
/// `deadline` を渡すと、その時刻までに終わらないコマンドを打ち切って失敗にする。利用者が対話的に
/// 起動する経路は自分で中断できるので `None` を渡し、途中のビルドを勝手に殺さない。期限を渡すのは
/// 無人で複数対象を順に処理する経路だけである。
pub(crate) fn run<I>(
    program: impl Into<OsString>,
    args: I,
    dry_run: bool,
    deadline: Option<Instant>,
) -> Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    println!("$ {}", command::display(&program, &args));
    if dry_run {
        return Ok(());
    }

    let mut child = Command::new(&program).args(&args).spawn()?;
    let status = match deadline {
        None => child.wait()?,
        Some(deadline) => wait_until(&mut child, deadline, &program)?,
    };
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {}: {status}", program.to_string_lossy())
    }
}

/// 期限まで終了を待ち、超過したら子プロセスを kill して失敗として返す。
///
/// 送るシグナルは起動した子プロセス自身にだけ届く。`sudo` 越しに起動した `nix` のような孫プロセスは
/// 残るため、これは資源の回収手段ではなく、呼び出し側の走査を先へ進めるための打ち切りである。
fn wait_until(child: &mut Child, deadline: Instant, program: &OsString) -> Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            child.kill()?;
            child.wait()?;
            bail!("command timed out: {}", program.to_string_lossy())
        };
        std::thread::sleep(remaining.min(WAIT_POLL_INTERVAL));
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

/// ユーザー権限実行の sudo 引数が HOME を対象ユーザーへ寄せること、および期限付き実行が停止した
/// コマンドを打ち切ることを検証する。
#[cfg(test)]
mod tests {
    use super::{run, sudo_as_user_args};
    use std::ffi::OsString;
    use std::time::{Duration, Instant};

    /// 期限を過ぎても終わらないコマンドは、待ち続けずに失敗として返る。
    #[test]
    fn run_with_deadline_stops_a_command_that_does_not_finish() {
        let started = Instant::now();
        let err = run(
            "sleep",
            [OsString::from("30")],
            false,
            Some(Instant::now() + Duration::from_millis(100)),
        )
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(30), "{err}");
    }

    /// 期限を渡さない実行は従来どおり終了状態だけを見る。
    #[test]
    fn run_without_deadline_reports_command_failure() {
        let err = run("sleep", [OsString::from("--dotfiles-invalid")], false, None)
            .err()
            .map(|err| err.to_string())
            .unwrap_or_default();

        assert!(err.contains("command failed"), "{err}");
    }

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
