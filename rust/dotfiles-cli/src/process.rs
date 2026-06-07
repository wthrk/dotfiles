//! `dotfiles` が呼ぶ外部コマンドの実行と表示を揃える。
//!
//! `dry_run` では実行せず、実行予定のコマンドだけを shell 風に出力する。実行時は終了状態を見て、
//! 失敗したプログラム名をエラーに含める。

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::Result;
use anyhow::{Context, bail};
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

/// 外部コマンドへ `stdin` を流し込み、stdout を捕捉して返す。非 0 終了は失敗にする。
///
/// `run_capture` と同じく stdout をプログラム的に読むが、加えて秘密情報（認証ヘッダ等）を **argv に乗せず**
/// stdin 経由で渡すための経路である。secret を `-H "Authorization: Bearer <token>"` のように argv に置くと、
/// 同一ホストのプロセス一覧（`ps`）から読めてしまうため、curl の `--config -`（stdin から設定読み取り）に
/// `header = "Authorization: Bearer ..."` を流す用途に使う。`stdin_data` はそのまま子プロセスの stdin へ書き、
/// 書き込み後に EOF を送る。stdin に書く内容も argv に現れないため、ログ・プロセス一覧のどちらにも secret を
/// 残さない。失敗時は終了状態と stderr 全文（前後の空白を `trim` した全体）を文脈に含め、stdout は UTF-8
/// 文字列として返す。
pub(crate) fn run_capture_with_stdin<I>(
    program: impl Into<OsString>,
    args: I,
    stdin_data: &[u8],
) -> Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {}", program.to_string_lossy()))?;

    // 子プロセスの stdin へ secret を含む設定を書き、EOF を送る。stdin は drop で閉じ EOF になる。
    // stdin 取得・書込みの失敗時に early return すると、spawn 済みの子が wait されずぶら下がる（ゾンビ／
    // 孤児プロセス）。`run_capture`/`run` は内部で必ず子を回収するのに対し、ここは手動 spawn のため、
    // エラー経路でも `kill` + `wait` で確実に回収してから Err を返す（回収順序をコードで保証する）。
    let write_result = (|| -> Result<()> {
        let mut stdin = child.stdin.take().context("child stdin was not captured")?;
        stdin
            .write_all(stdin_data)
            .with_context(|| format!("failed to write stdin to {}", program.to_string_lossy()))?;
        // stdin を明示的に drop して EOF を送る（子の読取りを完了させる）。
        drop(stdin);
        Ok(())
    })();
    if let Err(error) = write_result {
        // 子を確実に回収する。kill は既に終了していてもエラーにせず、wait で残骸を刈り取る。
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for {}", program.to_string_lossy()))?;
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
