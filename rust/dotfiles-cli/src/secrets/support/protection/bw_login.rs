//! Bitwarden Password Manager CLI（`bw`）login / unlock の secret 保護境界 backend 操作。
//!
//! `bw login` / `bw unlock` は master password を要求する外部 command である。secret-handling.md の
//! 外部処理境界に従い、master password の借用とその子プロセスへの受け渡しはこの protection 境界内で
//! 完了させる。master password は `BW_PASSWORD` env として子プロセスにだけ渡し、argv・ログ・親プロセスの
//! 永続環境変数・一時ファイルへ残さない。`BW_SESSION` 値そのものは呼び出し側へ返さない。
//!
//! `bw unlock --raw` は `BW_SESSION` トークン（secret）を、`bw --version` は機械可読でない version 文字列を
//! 子プロセス stdout へ出力する。この dotfiles プロセスの stdout は機械可読 JSON report と同一ストリーム
//! であり、継承すると secret 漏洩・report 破壊を起こす。よってこの境界では `bw` 子プロセスの stdout を
//! 一律 `Stdio::null()` へ破棄し、login / unlock / reachability の成立確認は exit status だけで行う。
//! stderr は `bw` 自身の診断出力で、`bw` は secret を stderr へ出さない（`--raw` の session は stdout 限定）
//! ため、失敗時の診断可視化のために継承する（dotfiles 側から secret を stderr へ書くことはない）。
//!
//! この module は `gpg-backend`（= 非 `secrets-internal-test-stub`）build でだけ compile し、internal
//! test stub build では adapter 側 stub と compile-time で差し替わる。

use std::process::{Command, Stdio};

use anyhow::{Context, bail};

use crate::Result;
use crate::secrets::support::protection::ProtectedSecret;

/// `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` の後
/// `bw unlock --passwordenv BW_PASSWORD --raw` を実行する（spec L178）。
///
/// master password は `password.with_secret_utf8` の借用境界内でだけ `BW_PASSWORD` env value へ複製し、`bw`
/// の子プロセスへ env として渡す。借用 closure を抜けると複製 buffer は zeroize 管理（`Zeroizing`）で破棄
/// される。email は非秘匿だが、YubiKey 由来 email と同じ carrier 型で受け取るため `ProtectedSecret` で渡され、
/// この借用境界内で `&str` へ取り出して `bw login` の位置引数に使う。otp は非秘匿のワンタイムコードで `--code`
/// の値として渡す。login または unlock が非ゼロ終了した場合は停止条件として `Err` を返す。`bw unlock --raw` は
/// `BW_SESSION` 値（secret）を子プロセス stdout へ出力するため、その stdout は `Stdio::null()` で破棄し、成立
/// 確認は exit status だけで行う（`BW_SESSION` を読まず・返さず・親 stdout/ログ/一時ファイルへ一切出さない）。
pub(crate) fn login_and_unlock(
    email: &ProtectedSecret,
    password: &ProtectedSecret,
    otp: &str,
) -> Result<()> {
    email.with_secret_utf8(|email| run_bw_login(email, password, otp))?;
    run_bw_unlock(password)?;
    Ok(())
}

/// `bw` CLI の起動可能性（CLI invocation capability）だけを確認する（spec L155 / L201 の `--check bw-login`）。
///
/// login / unlock を成立させず、`bw --version` の成功で `bw` CLI バイナリが起動可能かだけを確認する。secret は
/// 要求しない。`bw --version` は version 文字列を子プロセス stdout へ出力し、継承すると後段の機械可読 JSON
/// report を破壊するため、その stdout は `Stdio::null()` で破棄し、成立確認は exit status だけで行う。
///
/// 限界: この確認は CLI バイナリの起動可能性に限られ、Bitwarden Password Manager サーバーへの真のサービス
/// 到達性（server URL 設定・ネットワーク疎通）は確認しない。spec L201 が要求する「Bitwarden Password Manager
/// への到達確認」のうちサービス到達性部分は実 `bw` 統合（#16）の責務であり、その差分は
/// `docs/tasks/secret-recovery/review-artifacts/integration/confirmation.md` に既知の制約として記録する。
pub(crate) fn check_reachable() -> Result<()> {
    let status = Command::new("bw")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .context("failed to invoke `bw` CLI for bw-login reachability check")?;
    if !status.success() {
        bail!("`bw` CLI is not available for bw-login reachability check");
    }
    Ok(())
}

/// `bw login` を master password env つきで実行する。
fn run_bw_login(email: &str, password: &ProtectedSecret, otp: &str) -> Result<()> {
    let success = password.with_secret_utf8(|password| {
        let status = Command::new("bw")
            .arg("login")
            .arg(email)
            .arg("--passwordenv")
            .arg("BW_PASSWORD")
            .arg("--method")
            .arg("3")
            .arg("--code")
            .arg(otp)
            .env("BW_PASSWORD", password)
            .stdout(Stdio::null())
            .status()
            .context("failed to invoke `bw login`")?;
        Ok(status.success())
    })?;
    if !success {
        bail!("`bw login` failed");
    }
    Ok(())
}

/// `bw unlock --raw` を master password env つきで実行する。`--raw` が stdout へ出す `BW_SESSION` は
/// `Stdio::null()` で破棄し、成立確認は exit status だけで行う（呼び出し側へ返さず親 stdout へも出さない）。
fn run_bw_unlock(password: &ProtectedSecret) -> Result<()> {
    let success = password.with_secret_utf8(|password| {
        let status = Command::new("bw")
            .arg("unlock")
            .arg("--passwordenv")
            .arg("BW_PASSWORD")
            .arg("--raw")
            .env("BW_PASSWORD", password)
            .stdout(Stdio::null())
            .status()
            .context("failed to invoke `bw unlock`")?;
        Ok(status.success())
    })?;
    if !success {
        bail!("`bw unlock` failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `bw` 子プロセス stdout 破棄（FINDING 1 / 2）を実 process 実行で検証する。
    //!
    //! 実 `bw` CLI は使わず、PATH 先頭へ置いた fake `bw` を起動する。fake `bw` は secret（`BW_SESSION`）/
    //! version 文字列の stdout 出力を模して、一意 sentinel を **自身の stdout** へ出力し、起動証跡を marker
    //! file へ追記する。
    //!
    //! 観測は専用の子プロセス（`current_exe` の再起動）で行う。子プロセスは PATH を fake `bw` 入りに差し替えた
    //! 単独実行環境で対象関数を stdout 継承のまま走らせ、その子プロセス stdout（= dotfiles プロセスが端末 /
    //! JSON report へ出すストリームと同じ）を parent が pipe で捕捉する。対象関数が `Stdio::null()` で `bw` の
    //! stdout を破棄していれば sentinel は子プロセス stdout に現れない。process-global fd 退避や並行 test 間で
    //! 共有する PATH 変更を使わないため、`cargo test` の並行実行でも安定する。

    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::secrets::support::protection::protect_public_bytes;

    /// 子プロセス側で実行する対象シナリオを選ぶ env var。値が未設定なら通常の test harness として動く。
    const SCENARIO_ENV: &str = "DOTFILES_BW_LOGIN_STDOUT_TEST_SCENARIO";

    static UNIQUE: AtomicU64 = AtomicU64::new(0);

    /// 子プロセスとして再起動された `child_scenario_entrypoint` から呼ばれ、対象関数を stdout 継承のまま
    /// 実行して結果を exit code で返す。stdout には対象関数経由の出力だけが乗る。
    fn dispatch_child_scenario_if_requested() -> ! {
        let scenario = std::env::var(SCENARIO_ENV).expect("child scenario env must be set");
        let email = protect_public_bytes(b"user@example.com", 16 * 1024).expect("protect email");
        let password = protect_public_bytes(b"master-password", 16 * 1024).expect("protect pw");
        let result = match scenario.as_str() {
            "reachable" => check_reachable(),
            "login_unlock" => login_and_unlock(&email, &password, "123456"),
            other => panic!("unknown child scenario {other:?}"),
        };
        // 結果は exit code で parent へ返す。stdout には対象関数経由の出力だけが乗る。
        std::process::exit(if result.is_ok() { 0 } else { 7 });
    }

    struct FakeBw {
        dir: PathBuf,
        marker: PathBuf,
        sentinel: String,
    }

    impl FakeBw {
        /// PATH 先頭へ差し込む fake `bw` を temp dir に作る。`exit_code` で成立/失敗を制御する。
        fn new(exit_code: u8) -> FakeBw {
            let nonce = UNIQUE.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "dotfiles-bw-stdout-test-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("create fake bw dir");
            let marker = dir.join("invoked");
            let sentinel = format!("BW-SESSION-SENTINEL-{}-{nonce}", std::process::id());
            let script = dir.join("bw");
            // fake `bw`: 起動証跡を marker へ追記し、sentinel を stdout(fd 1) へ出す。
            // `bw unlock --raw` の `BW_SESSION` / `bw --version` の version 文字列の stdout 出力を模す。
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\n\
                     printf 'invoked %s\\n' \"$1\" >> '{marker}'\n\
                     printf '{sentinel}\\n'\n\
                     exit {exit_code}\n",
                    marker = marker.display(),
                ),
            )
            .expect("write fake bw script");
            let mut perms = std::fs::metadata(&script)
                .expect("stat fake bw")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).expect("chmod fake bw");
            FakeBw {
                dir,
                marker,
                sentinel,
            }
        }

        fn was_invoked(&self) -> bool {
            self.marker.exists()
        }
    }

    impl Drop for FakeBw {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    struct ChildRun {
        success: bool,
        stdout: String,
    }

    /// 同じ test binary を子プロセスとして再起動し、指定 scenario の対象関数を単独実行させる。
    /// PATH は fake `bw` 入りに差し替え、子の stdout を pipe で捕捉して返す。
    fn run_scenario_in_child(scenario: &str, fake: &FakeBw) -> ChildRun {
        let exe = std::env::current_exe().expect("resolve current test executable");
        let mut entries = vec![fake.dir.clone()];
        if let Some(existing) = std::env::var_os("PATH") {
            entries.extend(std::env::split_paths(&existing));
        }
        let joined = std::env::join_paths(entries).expect("join PATH");
        let output = Command::new(exe)
            // この test 名だけを子プロセスで 1 件実行し、その中で scenario hook を起動する。
            .args([
                "--exact",
                "secrets::support::protection::bw_login::tests::child_scenario_entrypoint",
            ])
            .arg("--nocapture")
            .env(SCENARIO_ENV, scenario)
            .env("PATH", joined)
            .output()
            .expect("spawn child test process");
        ChildRun {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        }
    }

    /// 子プロセス専用 entrypoint。scenario env がある場合だけ対象関数を実行して exit する。
    /// scenario env が無い通常実行では即 return し、test harness 側で `ok` 扱いになる。
    #[test]
    fn child_scenario_entrypoint() {
        if std::env::var_os(SCENARIO_ENV).is_some() {
            dispatch_child_scenario_if_requested();
        }
        // 通常の test 実行（parent 側）では scenario env が無いため何もしない。
    }

    fn assert_no_sentinel_in_child_stdout(scenario: &str, exit_code: u8) {
        let fake = FakeBw::new(exit_code);
        let run = run_scenario_in_child(scenario, &fake);
        assert!(
            fake.was_invoked(),
            "fake bw was not invoked by child process"
        );
        assert!(
            !run.stdout.contains(&fake.sentinel),
            "`bw` child stdout leaked into dotfiles stdout (secret exposure / JSON report break): {:?}",
            run.stdout
        );
        // child stdout には dotfiles 側の出力だけが乗る。fake `bw` の sentinel は `Stdio::null()` で破棄される。
        assert!(
            run.success,
            "scenario {scenario} should succeed with exit-0 fake bw"
        );
    }

    #[test]
    fn reachability_check_discards_child_stdout() {
        // FINDING 2: `bw --version` の stdout（version 文字列）が dotfiles stdout（JSON report と同一
        // ストリーム）へ漏れないこと。
        assert_no_sentinel_in_child_stdout("reachable", 0);
    }

    #[test]
    fn login_and_unlock_discards_child_stdout() {
        // FINDING 1: `bw unlock --raw` が stdout へ出す `BW_SESSION`（および `bw login` の stdout）が
        // dotfiles stdout へ漏れないこと。
        assert_no_sentinel_in_child_stdout("login_unlock", 0);
    }

    #[test]
    fn login_failure_is_stop_condition() {
        // login / unlock が非ゼロ終了した場合は停止条件として Err を返す（成立確認は exit status のみ）。
        let fake = FakeBw::new(1);
        let run = run_scenario_in_child("login_unlock", &fake);
        assert!(
            fake.was_invoked(),
            "fake bw was not invoked by child process"
        );
        assert!(
            !run.success,
            "non-zero `bw` exit must be a stop condition (child should exit non-zero)"
        );
        assert!(
            !run.stdout.contains(&fake.sentinel),
            "`bw` child stdout leaked into dotfiles stdout on failure path: {:?}",
            run.stdout
        );
    }
}
