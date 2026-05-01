use std::env;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use xshell::{cmd, Shell};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => check(args.collect()),
        Some("-h" | "--help") | None => {
            print_help();
            Ok(())
        }
        Some(cmd) => Err(format!("unknown command: {cmd}").into()),
    }
}

fn print_help() {
    println!(
        r#"Usage:
  cargo xtask check static
  cargo xtask check zsh
  cargo xtask check runtime <fresh-bootstrap|second-user-home-manager|darwin-switch-ya|all>
  cargo xtask check all"#
    );
}

fn check(args: Vec<String>) -> Result<()> {
    match args.as_slice() {
        [target] if target == "static" => check_static(),
        [target] if target == "zsh" => check_zsh(),
        [target] if target == "all" => {
            check_static()?;
            check_zsh()
        }
        [target, scenario] if target == "runtime" => check_runtime(scenario),
        [] => Err("missing check target".into()),
        _ => Err(format!("unsupported check arguments: {}", args.join(" ")).into()),
    }
}

fn check_static() -> Result<()> {
    let sh = Shell::new()?;
    step("cargo fmt");
    cmd!(sh, "cargo fmt --all -- --check").run()?;
    step("cargo check");
    cmd!(sh, "cargo check --workspace").run()?;
    step("flake.lock exists");
    cmd!(sh, "test -s flake.lock").run()?;
    step("nix flake check");
    cmd!(
        sh,
        "nix --extra-experimental-features 'nix-command flakes' flake check --no-update-lock-file"
    )
    .run()?;
    let nix_files = cmd!(sh, "git ls-files '*.nix'").read()?;
    if !nix_files.trim().is_empty() {
        let files = nix_files.lines().collect::<Vec<_>>();
        step("nix fmt");
        cmd!(
            sh,
            "nix --extra-experimental-features 'nix-command flakes' fmt -- --check {files...}"
        )
        .run()?;
    }
    step("runner Home Manager output eval");
    cmd!(
        sh,
        "nix --extra-experimental-features 'nix-command flakes' eval --no-update-lock-file .#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;
    Ok(())
}

fn check_zsh() -> Result<()> {
    let sh = Shell::new()?;
    let home = ZshTestHome::new(&sh)?;
    run_zsh_shortcuts(&sh, &home)?;
    run_zsh_key_operations(&sh, &home)
}

fn check_runtime(scenario: &str) -> Result<()> {
    match scenario {
        "fresh-bootstrap" | "second-user-home-manager" | "darwin-switch-ya" | "all" => {}
        _ => return Err(format!("unsupported runtime scenario: {scenario}").into()),
    }

    let sh = Shell::new()?;
    if env::var_os("GITHUB_ACTIONS").is_some() {
        step("runtime scenario");
        cmd!(sh, "bash scripts/run-macos-install-scenario.sh {scenario}").run()?;
    } else {
        step("runtime scenario via tart");
        cmd!(
            sh,
            "nix run --impure .#tart-macos-install -- --scenario {scenario}"
        )
        .run()?;
    }
    Ok(())
}

fn run_zsh_shortcuts(sh: &Shell, home: &ZshTestHome) -> Result<()> {
    let fzf_tab_widget = zsh_output(sh, home, "zle -la | rg '^fzf-tab-complete$' | head -n 1")?;
    let autosuggest_widget =
        zsh_output(sh, home, "zle -la | rg '^autosuggest-accept$' | head -n 1")?;
    let syntax = zsh_output(sh, home, syntax_probe())?;
    let path_dump = zsh_output(sh, home, "print -l $path")?;

    expect_match(
        "T001 fzf-tab widget exists",
        &fzf_tab_widget,
        "fzf-tab-complete",
    )?;
    expect_match(
        "T002 autosuggest widget exists",
        &autosuggest_widget,
        "autosuggest-accept",
    )?;
    expect_nonempty("T003 fast-syntax-highlighting loaded", &syntax)?;
    expect_eq(
        "T010 TAB emacs",
        &zsh_output(sh, home, "bindkey -M emacs '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T011 TAB viins",
        &zsh_output(sh, home, "bindkey -M viins '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T012 TAB vicmd",
        &zsh_output(sh, home, "bindkey -M vicmd '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "T013 Ctrl-X TAB emacs",
        &zsh_output(sh, home, "bindkey -M emacs '^X^I'")?,
        "\"^X^I\" fzf-tab-complete",
    )?;
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

fn run_zsh_key_operations(sh: &Shell, home: &ZshTestHome) -> Result<()> {
    expect_eq(
        "KEY:emacs:^I",
        &zsh_output(sh, home, "bindkey -M emacs '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:viins:^I",
        &zsh_output(sh, home, "bindkey -M viins '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:vicmd:^I",
        &zsh_output(sh, home, "bindkey -M vicmd '^I'")?,
        "\"^I\" expand-or-complete",
    )?;
    expect_eq(
        "KEY:emacs:^X^I",
        &zsh_output(sh, home, "bindkey -M emacs '^X^I'")?,
        "\"^X^I\" fzf-tab-complete",
    )?;
    expect_match(
        "WIDGET:fzf-tab-complete",
        &zsh_output(sh, home, "zle -la | rg '^fzf-tab-complete$' | head -n 1")?,
        "fzf-tab-complete",
    )?;
    expect_match(
        "WIDGET:autosuggest-accept",
        &zsh_output(sh, home, "zle -la | rg '^autosuggest-accept$' | head -n 1")?,
        "autosuggest-accept",
    )?;
    expect_nonempty(
        "FUNC:syntax-highlighting",
        &zsh_output(sh, home, syntax_probe())?,
    )?;
    let path_dump = zsh_output(sh, home, "print -l $path")?;
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

    let startup = zsh_output(sh, home, "exit")?;
    if startup.contains("command not found")
        || startup.contains("no such file")
        || startup.contains("error")
    {
        return Err(format!("FAIL STARTUP:clean\n{startup}").into());
    }
    println!("PASS STARTUP:clean");
    Ok(())
}

fn syntax_probe() -> &'static str {
    "functions | rg '(^|[[:space:]])_zsh_highlight|(^|[[:space:]])_fast_highlight|(^|[[:space:]])fast-theme|(^|[[:space:]])FAST_HIGHLIGHT' || :"
}

fn zsh_output(sh: &Shell, home: &ZshTestHome, script: &str) -> Result<String> {
    let home_path = home.path().display().to_string();
    let raw = cmd!(
        sh,
        "env HOME={home_path} USER=ya LOGNAME=ya POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true script -q /dev/null zsh -ic {script}"
    )
    .ignore_status()
    .read()?;
    Ok(strip_script_control(&raw))
}

struct ZshTestHome {
    path: PathBuf,
}

impl ZshTestHome {
    fn new(sh: &Shell) -> Result<Self> {
        let activation_package = cmd!(
            sh,
            "nix build --no-update-lock-file --no-link --print-out-paths .#homeConfigurations.default.activationPackage"
        )
        .read()?
        .trim()
        .to_string();
        let home_files = PathBuf::from(activation_package).join("home-files");
        let zshrc = home_files.join(".zshrc");
        if !zshrc.is_file() {
            return Err(format!("{} is missing", zshrc.display()).into());
        }

        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            env::temp_dir().join(format!("dotfiles-zsh-home-{}-{suffix}", std::process::id()));
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
        copy_with_home_rewrite(&zshrc, &path.join(".zshrc"), &path)?;
        copy_with_home_rewrite(&home_files.join(".zshenv"), &path.join(".zshenv"), &path)?;

        Ok(Self { path })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for ZshTestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn copy_with_home_rewrite(source: &Path, dest: &Path, home: &Path) -> Result<()> {
    let text = fs::read_to_string(source)?;
    let rewritten = text.replace("/Users/ya", &home.display().to_string());
    fs::write(dest, rewritten)?;
    Ok(())
}

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

fn expect_eq(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        println!("PASS {label}");
        Ok(())
    } else {
        Err(format!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}").into())
    }
}

fn expect_match(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual.lines().any(|line| line.trim() == expected) {
        println!("PASS {label}");
        Ok(())
    } else {
        Err(format!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}").into())
    }
}

fn expect_nonempty(label: &str, actual: &str) -> Result<()> {
    if actual.trim().is_empty() {
        Err(format!("FAIL {label}\n  actual: <empty>").into())
    } else {
        println!("PASS {label}");
        Ok(())
    }
}

fn expect_contains(label: &str, actual: &str, needle: &str) -> Result<()> {
    if actual.contains(needle) {
        println!("PASS {label}");
        Ok(())
    } else {
        Err(format!("FAIL {label}\n  expected substring: {needle}\n  actual: {actual}").into())
    }
}

fn expect_absent(label: &str, actual: &str, needles: &[&str]) -> Result<()> {
    if let Some(needle) = needles.iter().find(|needle| actual.contains(**needle)) {
        Err(format!("FAIL {label}\n  unexpected substring: {needle}\n  actual: {actual}").into())
    } else {
        println!("PASS {label}");
        Ok(())
    }
}

fn step(label: &str) {
    println!("==> {label}");
}
