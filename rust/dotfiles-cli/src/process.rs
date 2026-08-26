//! `dotfiles` が呼ぶ外部コマンドの実行と表示を揃える。
//!
//! `dry_run` では実行せず、実行予定のコマンドだけを shell 風に出力する。実行時は終了状態を見て、
//! 失敗したプログラム名をエラーに含める。
//!
//! 捕捉実行（`run_capture`）も含め、実行する外部コマンドと標準入出力はオミットせずに端末へ
//! 表示し、利用者に進行状況と結果、警告・診断情報を伝える。捕捉した値を機械可読な親 stdout
//! 契約へ混ぜないため、捕捉実行の表示先は stderr に固定する。
//!
//! 無人実行の呼び出し側は期限を渡せる。期限を過ぎたコマンドは打ち切って失敗にし、停止した 1 件が
//! 後続の処理を無期限に止めないようにする。
//!
//! `sudo` による昇格・降格も argv の組み立てなので [`Invocation`] としてここに置く。use case 側で
//! 組み立てると、同じ権限規則が use case ごとに写る。

use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
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

/// 外部コマンド実行の起動プログラムと引数列。
///
/// 権限の切り替えは `sudo` の前置として argv に現れるため、起動側の層ではなくここで組み立てる。
/// 昇格（root 権限を要する処理）と降格（利用者所有の対象を触る処理）は対になる規則であり、片方だけを
/// 呼び出し側へ写すと同じ規則が複数の use case へ散る。
///
/// euid と降格対象は引数で受け取り、構築を副作用なしにする。caller responsibility: 実行時 euid の解決と
/// 降格対象の決定は呼び出し側で済ませ、この型には結果だけを渡すこと。
pub(crate) struct Invocation {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
}

impl Invocation {
    /// 利用者所有の対象を触るコマンドを、降格対象の有無で組み立てる。
    ///
    /// 降格対象があれば `sudo -H -u <user>` を前置し、無ければそのまま起動する。root のまま利用者所有の
    /// ファイルへ書くと所有者が root へ変わるため、降格の要否は呼び出し側の判断をそのまま渡す。
    pub(crate) fn downgraded<I>(program: OsString, args: I, downgrade_to: Option<&str>) -> Self
    where
        I: IntoIterator<Item = OsString>,
    {
        match downgrade_to {
            Some(user) => Self {
                program: OsString::from("sudo"),
                args: sudo_as_user_args(user, program, args),
            },
            None => Self {
                program,
                args: args.into_iter().collect(),
            },
        }
    }

    /// root 権限を要するコマンドを、実行時 euid で `sudo` 前置の有無を切り替えて組み立てる。
    pub(crate) fn escalated<I>(program: OsString, args: I, is_root: bool) -> Self
    where
        I: IntoIterator<Item = OsString>,
    {
        if is_root {
            Self {
                program,
                args: args.into_iter().collect(),
            }
        } else {
            Self {
                program: OsString::from("sudo"),
                args: std::iter::once(program).chain(args).collect(),
            }
        }
    }

    /// 組み立てた起動を実行する。`dry_run` と `deadline` の扱いは [`run`] と同じ。
    pub(crate) fn run(self, dry_run: bool, deadline: Option<Instant>) -> Result<()> {
        run(self.program, self.args, dry_run, deadline)
    }

    /// 組み立てた起動を実行し、標準出力を捕捉する。`deadline` の扱いは [`run`] と同じ。
    ///
    /// 実行するコマンドを stderr へ表示したうえで実行し、標準出力を捕捉して返す。
    /// 標準出力・標準エラー出力とも stderr へ流し、利用者に進行状況や警告・エラーを伝える。
    pub(crate) fn run_capture(self, deadline: Option<Instant>) -> Result<String> {
        capture(self.program, self.args, deadline)
    }
}

/// 外部コマンドを起動して標準出力を捕捉する。非 0 終了は失敗にする。
///
/// 実行するコマンドと子プロセス出力を stderr へ表示し、進捗や警告をオミットしない。
/// stdout は pipe で受け、待機と並行に読み切って stderr へ転送する。これにより、呼び出し元の
/// 機械可読 stdout 契約を壊さない。stdin は与えない。
///
/// 期限を超えた実行は pipe を読み切らずに戻る。`sudo` 越しに起動した孫は pipe の write end を持った
/// まま残るため、読み切ってから戻ると打ち切りの効き目が孫の寿命まで延びる。
fn capture(program: OsString, args: Vec<OsString>, deadline: Option<Instant>) -> Result<String> {
    eprintln!("$ {}", command::display(&program, &args));
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = read_to_end_and_forward_in_background(child.stdout.take());
    let status = match deadline {
        None => child.wait()?,
        Some(deadline) => wait_until(&mut child, deadline, &program)?,
    };
    let stdout = joined(stdout, &program)?;
    if !status.success() {
        bail!("command failed: {}: {status}", program.to_string_lossy());
    }
    Ok(String::from_utf8(stdout)?)
}

/// 子プロセスの stdout pipe を stderr へ転送しながら、別スレッドで EOF まで捕捉する。
///
/// 読み出しを待機から分けるのは、読まないまま待つと pipe buffer が満ちた時点で子プロセスが書き込みで
/// 止まり、待機側が期限にも終了にも到達しないためである。転送先は chunk ごとに flush し、長時間の
/// 評価中にも子プロセス出力を利用者へ遅延させない。親 stdout は機械可読な呼び出し元が所有する。
fn read_to_end_and_forward_in_background(
    pipe: Option<impl Read + Send + 'static>,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut captured = Vec::new();
        if let Some(mut pipe) = pipe {
            let stderr = std::io::stderr();
            read_to_end_and_forward(&mut pipe, &mut captured, |chunk| {
                // pipe の次の読み取り中に lock を持ち続けない。期限後も孫プロセスが write end を
                // 保持し得るため、ここで lock を保持すると呼び出し元の診断出力まで停止する。
                let mut terminal = stderr.lock();
                terminal.write_all(chunk)?;
                terminal.flush()
            })?;
        }
        Ok(captured)
    })
}

/// stdout の各 chunk を stderr へ転送し、呼び出し元が後から読むため同じ内容を捕捉する。
///
/// `forward` は各読み取りの完了後にだけ呼ぶ。呼び出し元は stderr lock のような共有出力資源を
/// `forward` の呼び出し中だけ保持し、次の `read` を待つ間は解放しなければならない。
fn read_to_end_and_forward(
    reader: &mut impl Read,
    captured: &mut Vec<u8>,
    mut forward: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        forward(&buffer[..count])?;
        captured.extend_from_slice(&buffer[..count]);
    }
}

/// [`read_to_end_and_forward_in_background`] が読み切った内容を受け取る。
fn joined(handle: JoinHandle<std::io::Result<Vec<u8>>>, program: &OsString) -> Result<Vec<u8>> {
    match handle.join() {
        Ok(read) => Ok(read?),
        Err(_) => bail!(
            "failed to read the output of: {}",
            program.to_string_lossy()
        ),
    }
}

/// 外部コマンドの stdout を期限なしで捕捉して返す。非 0 終了は失敗にする。
///
/// `run` が標準入出力を継承するのに対し、本関数は `nix flake archive --json` 出力やリリースノート取得の
/// ように標準出力をプログラム的に読む用途に使う。コマンド表記と子プロセス出力は stderr へ流して
/// 進行や警告を伝え、呼び出し元の stdout を機械可読な値専用に保つ。`dry_run` 経路は持たず常に実行する。
pub(crate) fn run_capture<I>(program: impl Into<OsString>, args: I) -> Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    capture(program.into(), args.into_iter().collect(), None)
}

/// ユーザー権限実行の sudo 引数が HOME を対象ユーザーへ寄せること、期限付き実行が停止した
/// コマンドを打ち切ること、捕捉実行が標準出力を正しく返し非 0 終了を失敗にすること、および
/// 捕捉する起動の打ち切りが孫プロセスの寿命に引きずられないことを検証する。
#[cfg(test)]
mod tests {
    use super::{capture, read_to_end_and_forward, run, sudo_as_user_args};
    use std::ffi::OsString;
    use std::io::{Cursor, Write};
    use std::time::{Duration, Instant};

    /// 捕捉実行は標準出力を正しく返し、非 0 終了は失敗として報告する。
    #[test]
    fn capture_captures_stdout_and_reports_failure() -> anyhow::Result<()> {
        let out = capture(
            OsString::from("echo"),
            vec![OsString::from("hello dotfiles")],
            None,
        )?;
        assert_eq!(out.trim(), "hello dotfiles");

        let err = capture(
            OsString::from("sh"),
            vec![OsString::from("-c"), OsString::from("exit 42")],
            None,
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
        assert!(err.contains("command failed"), "{err}");
        Ok(())
    }

    /// 捕捉実行の stdout は stderr へ即時転送しつつ、呼び出し元にも同じ値を返せる。
    #[test]
    fn captured_stdout_is_forwarded_and_retained() -> std::io::Result<()> {
        let mut source = Cursor::new(b"nix evaluation output\n".to_vec());
        let mut stderr = Vec::new();
        let mut captured = Vec::new();

        read_to_end_and_forward(&mut source, &mut captured, |chunk| {
            stderr.write_all(chunk)?;
            stderr.flush()
        })?;

        assert_eq!(stderr, b"nix evaluation output\n");
        assert_eq!(captured, stderr);
        Ok(())
    }

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

    /// 捕捉する起動の打ち切りは、pipe の write end を持つ孫が生きていても期限で戻る。
    ///
    /// 直の子を kill しても、その子が起こした `sleep` は捕捉用 pipe を持ったまま残る。打ち切りで
    /// pipe を読み切ってから戻ると、期限ではなくこの孫の終了まで待つことになる。
    #[test]
    fn capture_with_deadline_returns_while_an_orphan_holds_the_pipe() {
        let started = Instant::now();
        let err = capture(
            OsString::from("sh"),
            vec![
                OsString::from("-c"),
                OsString::from("sleep 10 & exec sleep 30"),
            ],
            Some(Instant::now() + Duration::from_millis(100)),
        )
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

        assert!(err.contains("timed out"), "{err}");
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
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
