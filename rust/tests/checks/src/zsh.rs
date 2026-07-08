//! Home Manager が生成した zsh 設定を、実ホームを触らずに起動して検証する。
//!
//! activation package の home-files を一時ホームへ写し、リンク先や HOME を置換したうえで
//! 対話 zsh を起動する。これにより現在のユーザー環境を汚さずに補完、キーバインド、PATH を見る。

use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process};

use anyhow::bail;
use xshell::{Shell, cmd};

use crate::{Result, command::current_user};

/// 一時ホームを作り、ショートカットとキー操作の両方を検証する。
pub fn check() -> Result<()> {
    let shell = Shell::new()?;
    let home = TestHome::new(&shell)?;
    shortcuts(&shell, &home)?;
    key_operations(&shell, &home)
}

/// zsh モジュールが利用者に約束する補完、ウィジェット、PATH の不変条件を検証する。
fn shortcuts(shell: &Shell, home: &TestHome) -> Result<()> {
    let fzf_tab_widget = zsh_output(shell, home, "zle -la | rg '^fzf-tab-complete$' | head -n 1")?;
    let autosuggest_widget = zsh_output(
        shell,
        home,
        "zle -la | rg '^autosuggest-accept$' | head -n 1",
    )?;
    let syntax = zsh_output(shell, home, syntax_probe())?;
    let path_dump = zsh_output(shell, home, "print -l $path")?;

    // fzf-tab が読み込まれている必要がある。読み込まれていない場合、
    // TAB 補完が黙って素の zsh 挙動に戻る。
    expect_match(
        "T001 fzf-tab widget exists",
        &fzf_tab_widget,
        "fzf-tab-complete",
    )?;
    // キー割り当てがウィジェットを参照するため、autosuggestions が読み込まれている必要がある。
    expect_match(
        "T002 autosuggest widget exists",
        &autosuggest_widget,
        "autosuggest-accept",
    )?;
    // syntax highlighting はプラグインパスではなく関数の存在で見る。
    // Home Manager の store パスが変わっても検査が壊れないようにする。
    expect_nonempty("T003 fast-syntax-highlighting loaded", &syntax)?;
    // TAB は全キーマップで通常補完のままにする。
    // fzf-tab は意図的に Ctrl-X TAB に割り当てる。
    expect_eq(
        "T010 TAB emacs",
        &zsh_output(shell, home, "bindkey -M emacs '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T011 TAB viins",
        &zsh_output(shell, home, "bindkey -M viins '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T012 TAB vicmd",
        &zsh_output(shell, home, "bindkey -M vicmd '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T013 Ctrl-X TAB emacs",
        &zsh_output(shell, home, "bindkey -M emacs '^X^I'")?,
        "\"^X^I\" fzf-tab-complete",
    )?;
    // 言語環境は Nix/Home Manager 管理に寄せるため、旧 language-manager の
    // PATH エントリは意図的に除外する。
    expect_absent(
        "T020 legacy language-manager PATH entries are absent",
        &path_dump,
        &[
            ".nodebrew/current/bin",
            ".bun/bin",
            ".cargo/bin",
            ".pyenv/bin",
            ".rbenv/bin",
        ],
    )?;
    // これらのユーザーローカルなツールパスは意図的に残す。
    expect_contains(
        "T021 agent-tools path is allowed",
        &path_dump,
        ".agent-tools/bin",
    )?;
    expect_contains(
        "T022 rancher-desktop path is allowed",
        &path_dump,
        ".rd/bin",
    )?;
    Ok(())
}

/// 既存の手動確認表と同じラベルで、キー操作の退行を検出する。
fn key_operations(shell: &Shell, home: &TestHome) -> Result<()> {
    // 過去のキーマップ検証と同じ観点で、emacs/insert/command mode の TAB 挙動を
    // 固定する。
    expect_eq(
        "KEY:emacs:^I",
        &zsh_output(shell, home, "bindkey -M emacs '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:viins:^I",
        &zsh_output(shell, home, "bindkey -M viins '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:vicmd:^I",
        &zsh_output(shell, home, "bindkey -M vicmd '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:emacs:^X^I",
        &zsh_output(shell, home, "bindkey -M emacs '^X^I'")?,
        "\"^X^I\" fzf-tab-complete",
    )?;
    // ウィジェット検証はキー割り当てが読み込み済みプラグイン関数を指すことを確認する。
    expect_match(
        "WIDGET:fzf-tab-complete",
        &zsh_output(shell, home, "zle -la | rg '^fzf-tab-complete$' | head -n 1")?,
        "fzf-tab-complete",
    )?;
    expect_match(
        "WIDGET:autosuggest-accept",
        &zsh_output(
            shell,
            home,
            "zle -la | rg '^autosuggest-accept$' | head -n 1",
        )?,
        "autosuggest-accept",
    )?;
    expect_nonempty(
        "FUNC:syntax-highlighting",
        &zsh_output(shell, home, syntax_probe())?,
    )?;
    let path_dump = zsh_output(shell, home, "print -l $path")?;
    // PATH 検証では旧 language-manager shim を排除しつつ、
    // 意図的なユーザーローカルなツールパスは残す。
    expect_absent(
        "PATH:legacy-managers-absent",
        &path_dump,
        &[
            ".nodebrew/current/bin",
            ".bun/bin",
            ".cargo/bin",
            ".pyenv/bin",
            ".rbenv/bin",
        ],
    )?;
    expect_contains("PATH:agent-tools-allowed", &path_dump, ".agent-tools/bin")?;
    expect_contains("PATH:rancher-desktop-allowed", &path_dump, ".rd/bin")?;

    let startup = zsh_output(shell, home, "exit")?;
    // 起動時に典型的なシェルエラーを出してはいけない。
    // プロンプトが正常に読み込めることは zsh モジュールの利用者向け契約。
    if startup.contains("command not found")
        || startup.contains("no such file")
        || startup.contains("error")
    {
        bail!("FAIL STARTUP:clean\n{startup}");
    }
    println!("PASS STARTUP:clean");
    Ok(())
}

/// store パスに依存せず、fast-syntax-highlighting が読み込まれた事実だけを見る probe を返す。
fn syntax_probe() -> &'static str {
    "functions | rg '(^|[[:space:]])_zsh_highlight|(^|[[:space:]])_fast_highlight|(^|[[:space:]])fast-theme|(^|[[:space:]])FAST_HIGHLIGHT' || :"
}

/// `script(1)` 経由で zsh を対話起動し、TTY がないと再現しない補完初期化も通す。
fn zsh_output(shell: &Shell, home: &TestHome, script: &str) -> Result<String> {
    let home_path = home.path().display().to_string();
    let user = home.user();
    let raw = cmd!(
        shell,
        "env HOME={home_path} USER={user} LOGNAME={user} POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true script -q /dev/null zsh -ic {script}"
    )
    .ignore_status()
    .read()?;
    Ok(strip_script_control(&raw))
}

/// Home Manager の生成物を使って、実ホームとは別の `$HOME` を構築する。
struct TestHome {
    path: PathBuf,
    user: String,
    config_dir: PathBuf,
    source_snapshot: PathBuf,
}

impl TestHome {
    /// activation package をビルドし、生成時のホームパスを一時ホームへ差し替えて起動可能にする。
    fn new(shell: &Shell) -> Result<Self> {
        let user = current_user()?;
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let config_dir =
            env::temp_dir().join(format!("dotfiles-zsh-config-{}-{suffix}", process::id()));
        let config_dir_path = config_dir.display().to_string();
        let source_root = env::current_dir()?.canonicalize()?;
        let source_snapshot =
            env::temp_dir().join(format!("dotfiles-zsh-source-{}-{suffix}", process::id()));
        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_dir_all(&source_snapshot);
        fs::create_dir_all(&config_dir)?;
        materialize_tracked_snapshot(&source_root, &source_snapshot)?;
        let source = source_snapshot.canonicalize()?.display().to_string();

        cmd!(
            shell,
            "env DOTFILES_CONFIG_DIR={config_dir_path} cargo run --package dotfiles-cli -- init --user {user} --host {user} --system aarch64-darwin --source {source} --skip-self-package"
        )
        .run()?;
        assert_self_package_excluded(shell, &config_dir_path, &user)?;

        let activation_package = cmd!(
            shell,
            "nix build --no-link --print-out-paths {config_dir_path}#homeConfigurations.{user}.activationPackage"
        )
        .read()?
        .trim()
        .to_string();
        let generated_home = cmd!(
            shell,
            "nix eval --raw --no-update-lock-file {config_dir_path}#homeConfigurations.{user}.config.home.homeDirectory"
        )
        .read()?
        .trim()
        .to_string();
        let home_files = PathBuf::from(activation_package).join("home-files");
        let zshrc = home_files.join(".zshrc");
        // 対話シェル検証を走らせる前に、アクティベーションパッケージが zshrc を
        // 含むことを確認する。
        if !zshrc.is_file() {
            bail!("{} is missing", zshrc.display());
        }

        let path = env::temp_dir().join(format!("dotfiles-zsh-home-{}-{suffix}", process::id()));
        fs::create_dir_all(&path)?;
        fs::create_dir_all(path.join(".config"))?;
        fs::create_dir_all(path.join(".local/state"))?;
        fs::create_dir_all(path.join(".cache"))?;
        unix_fs::symlink(home_files.join(".config/zsh"), path.join(".config/zsh"))?;
        unix_fs::symlink(home_files.join(".config/git"), path.join(".config/git"))?;
        unix_fs::symlink(
            home_files.join(".config/direnv"),
            path.join(".config/direnv"),
        )?;
        unix_fs::symlink(home_files.join(".zsh"), path.join(".zsh"))?;
        copy_with_home_rewrite(&zshrc, &path.join(".zshrc"), &path, &generated_home)?;
        copy_with_home_rewrite(
            &home_files.join(".zshenv"),
            &path.join(".zshenv"),
            &path,
            &generated_home,
        )?;

        Ok(Self {
            path,
            user,
            config_dir,
            source_snapshot,
        })
    }

    /// zsh 起動時に `HOME` として渡す一時ディレクトリを返す。
    fn path(&self) -> &PathBuf {
        &self.path
    }

    /// `USER` と `LOGNAME` に渡す名前を返し、prompt 初期化が実ユーザー名に依存しないようにする。
    fn user(&self) -> &str {
        &self.user
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_dir_all(&self.config_dir);
        let _ = fs::remove_dir_all(&self.source_snapshot);
    }
}

/// Home Manager が埋め込んだ元ホームパスを一時ホームに置換してからファイルを配置する。
fn copy_with_home_rewrite(
    source: &Path,
    dest: &Path,
    home: &Path,
    generated_home: &str,
) -> Result<()> {
    let text = fs::read_to_string(source)?;
    let rewritten = text.replace(generated_home, &home.display().to_string());
    fs::write(dest, rewritten)?;
    Ok(())
}

/// mutable な worktree をそのまま path input に渡すと `target/` 配下の一時生成物の出入りで
/// `nix flake lock` が壊れるため、`git ls-files` で列挙した tracked file だけを
/// zsh 検証用 snapshot として materialize する。
fn materialize_tracked_snapshot(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let source_root = git_worktree_root(source)?;
    for relative in tracked_files(source)? {
        let source_path = source_root.join(&relative);
        let dest_path = dest.join(&relative);
        let file_type = match fs::symlink_metadata(&source_path) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if file_type.is_symlink() {
            let target = fs::read_link(&source_path)?;
            unix_fs::symlink(target, &dest_path)?;
        } else {
            fs::copy(&source_path, &dest_path)?;
        }
    }
    Ok(())
}

/// `git ls-files -z` を使い、未追跡ファイルを含まない snapshot 対象だけを返す。
fn tracked_files(source: &Path) -> Result<Vec<PathBuf>> {
    let source_root = git_worktree_root(source)?;
    let output = process::Command::new("git")
        .arg("-C")
        .arg(&source_root)
        .args(["ls-files", "--full-name", "-z"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("git ls-files failed for {}", source_root.display());
        }
        bail!("git ls-files failed for {}: {stderr}", source_root.display());
    }

    output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            tracked_relative_path(Path::new(std::str::from_utf8(entry)?)).map(PathBuf::from)
        })
        .collect()
}

/// worktree 内の任意パスから git 管理ルートを解決する。
fn git_worktree_root(source: &Path) -> Result<PathBuf> {
    let output = process::Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!(
                "git rev-parse --show-toplevel failed for {}",
                source.display()
            );
        }
        bail!(
            "git rev-parse --show-toplevel failed for {}: {stderr}",
            source.display()
        );
    }

    Ok(PathBuf::from(
        String::from_utf8(output.stdout)?.trim_end_matches('\n'),
    ))
}

/// `git ls-files` 出力が worktree 相対パスに閉じていることを確認する。
fn tracked_relative_path(path: &Path) -> Result<&Path> {
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!("tracked path escapes worktree: {}", path.display());
    }
    Ok(path)
}

/// zsh 検証用 local flake では self package を外し、不要な dotfiles CLI release build を避ける。
fn assert_self_package_excluded(shell: &Shell, config_dir_path: &str, user: &str) -> Result<()> {
    let packages = cmd!(
        shell,
        "nix eval --raw --no-update-lock-file {config_dir_path}#homeConfigurations.{user}.config.home.packages --apply 'ps: builtins.concatStringsSep \",\" (builtins.map (p: p.name or \"\") ps)'"
    )
    .read()?;
    if packages
        .split(',')
        .any(|name| name.starts_with("dotfiles-cli-"))
    {
        bail!("FAIL PATH:self-package-excluded\n  actual: {packages}");
    }
    println!("PASS PATH:self-package-excluded");
    Ok(())
}

/// `script(1)` が付ける制御文字を落とし、bindkey 出力を安定して比較できる形にする。
fn strip_script_control(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            line.trim_start_matches("^D\u{8}\u{8}")
                .trim_start_matches("\u{4}\u{8}\u{8}")
                .trim_end_matches('\r')
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// 期待する bindkey 結果のように、余分な文字を許容しない値を検証する。
fn expect_eq(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        println!("PASS {label}");
        Ok(())
    } else {
        bail!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}")
    }
}

/// `zle -la` のような複数行出力から、期待するウィジェット名が単独行で存在することを検証する。
fn expect_match(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual.lines().any(|line| line.trim() == expected) {
        println!("PASS {label}");
        Ok(())
    } else {
        bail!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}")
    }
}

/// 関数一覧の probe が何かを見つけたことだけを要求し、store パスや関数本文には依存しない。
fn expect_nonempty(label: &str, actual: &str) -> Result<()> {
    if actual.trim().is_empty() {
        bail!("FAIL {label}\n  actual: <empty>")
    } else {
        println!("PASS {label}");
        Ok(())
    }
}

/// PATH など順序付き出力に、許可対象の断片が残っていることを検証する。
fn expect_contains(label: &str, actual: &str, needle: &str) -> Result<()> {
    if actual.contains(needle) {
        println!("PASS {label}");
        Ok(())
    } else {
        bail!("FAIL {label}\n  expected substring: {needle}\n  actual: {actual}")
    }
}

/// PATH など順序付き出力に、旧 language manager の断片が混入していないことを検証する。
fn expect_absent(label: &str, actual: &str, needles: &[&str]) -> Result<()> {
    if let Some(needle) = needles.iter().find(|needle| actual.contains(**needle)) {
        bail!("FAIL {label}\n  unexpected substring: {needle}\n  actual: {actual}")
    } else {
        println!("PASS {label}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{check, materialize_tracked_snapshot, tracked_relative_path};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::{env, process};

    type TestResult = anyhow::Result<()>;

    #[test]
    fn tracked_snapshot_excludes_untracked_env_file() -> TestResult {
        let repo = unique_temp_dir("dotfiles-zsh-tracked-source");
        let snapshot = unique_temp_dir("dotfiles-zsh-tracked-dest");
        fs::create_dir_all(repo.join("tracked"))?;
        fs::write(repo.join("tracked/config.txt"), "tracked\n")?;
        fs::write(repo.join(".env"), "SECRET=should-not-copy\n")?;

        git(&repo, ["init"])?;
        git(&repo, ["config", "user.name", "dotfiles-checks"])?;
        git(
            &repo,
            ["config", "user.email", "dotfiles-checks@example.com"],
        )?;
        git(&repo, ["add", "tracked/config.txt"])?;
        git(&repo, ["commit", "-m", "track config"])?;

        materialize_tracked_snapshot(&repo, &snapshot)?;

        assert!(snapshot.join("tracked/config.txt").is_file());
        assert!(!snapshot.join(".env").exists());

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(snapshot);
        Ok(())
    }

    #[test]
    fn tracked_snapshot_skips_deleted_tracked_file() -> TestResult {
        let repo = unique_temp_dir("dotfiles-zsh-deleted-source");
        let snapshot = unique_temp_dir("dotfiles-zsh-deleted-dest");
        fs::create_dir_all(repo.join("tracked"))?;
        fs::write(repo.join("tracked/keep.txt"), "keep\n")?;
        fs::write(repo.join("tracked/deleted.txt"), "delete me\n")?;

        git(&repo, ["init"])?;
        git(&repo, ["config", "user.name", "dotfiles-checks"])?;
        git(
            &repo,
            ["config", "user.email", "dotfiles-checks@example.com"],
        )?;
        git(&repo, ["add", "tracked/keep.txt", "tracked/deleted.txt"])?;
        git(&repo, ["commit", "-m", "track files"])?;

        fs::remove_file(repo.join("tracked/deleted.txt"))?;
        materialize_tracked_snapshot(&repo, &snapshot)?;

        assert!(snapshot.join("tracked/keep.txt").is_file());
        assert!(!snapshot.join("tracked/deleted.txt").exists());

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(snapshot);
        Ok(())
    }

    #[test]
    fn tracked_snapshot_resolves_repo_root_from_subdirectory() -> TestResult {
        let repo = unique_temp_dir("dotfiles-zsh-subdir-source");
        let snapshot = unique_temp_dir("dotfiles-zsh-subdir-dest");
        fs::create_dir_all(repo.join("nested/work"))?;
        fs::create_dir_all(repo.join("tracked"))?;
        fs::write(repo.join("tracked/root.txt"), "root\n")?;

        git(&repo, ["init"])?;
        git(&repo, ["config", "user.name", "dotfiles-checks"])?;
        git(
            &repo,
            ["config", "user.email", "dotfiles-checks@example.com"],
        )?;
        git(&repo, ["add", "tracked/root.txt"])?;
        git(&repo, ["commit", "-m", "track root file"])?;

        materialize_tracked_snapshot(&repo.join("nested/work"), &snapshot)?;

        assert!(snapshot.join("tracked/root.txt").is_file());

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(snapshot);
        Ok(())
    }

    #[test]
    fn tracked_relative_path_rejects_parent_escape() {
        assert!(tracked_relative_path(Path::new("../.env")).is_err());
        assert!(tracked_relative_path(Path::new("/tmp/secret")).is_err());
        assert!(tracked_relative_path(Path::new("tracked/config.txt")).is_ok());
    }

    #[test]
    #[ignore = "requires nix/cargo and runs the real zsh check path"]
    fn zsh_check_runs_via_test_home_new_with_tracked_snapshot() -> TestResult {
        let previous = env::current_dir()?;
        env::set_current_dir(repo_root())?;
        let result = check();
        env::set_current_dir(previous)?;
        result
    }

    fn git<const N: usize>(repo: &Path, args: [&str; N]) -> TestResult {
        let status = Command::new("git").current_dir(repo).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io_error(format!("git command failed: {:?}", args)).into())
        }
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{}-{suffix}", process::id()))
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("dotfiles-checks manifest dir should be nested under repo root")
    }

    fn io_error(message: String) -> std::io::Error {
        std::io::Error::other(message)
    }
}
