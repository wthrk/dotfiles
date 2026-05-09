use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process};

use anyhow::bail;
use xshell::{Shell, cmd};

use crate::{Result, command::current_user};

pub fn check() -> Result<()> {
    let shell = Shell::new()?;
    let home = TestHome::new(&shell)?;
    shortcuts(&shell, &home)?;
    key_operations(&shell, &home)
}

fn shortcuts(shell: &Shell, home: &TestHome) -> Result<()> {
    let fzf_tab_widget = zsh_output(shell, home, "zle -la | rg '^fzf-tab-complete$' | head -n 1")?;
    let autosuggest_widget = zsh_output(
        shell,
        home,
        "zle -la | rg '^autosuggest-accept$' | head -n 1",
    )?;
    let syntax = zsh_output(shell, home, syntax_probe())?;
    let path_dump = zsh_output(shell, home, "print -l $path")?;

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

fn key_operations(shell: &Shell, home: &TestHome) -> Result<()> {
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
    if startup.contains("command not found")
        || startup.contains("no such file")
        || startup.contains("error")
    {
        bail!("FAIL STARTUP:clean\n{startup}");
    }
    println!("PASS STARTUP:clean");
    Ok(())
}

fn syntax_probe() -> &'static str {
    "functions | rg '(^|[[:space:]])_zsh_highlight|(^|[[:space:]])_fast_highlight|(^|[[:space:]])fast-theme|(^|[[:space:]])FAST_HIGHLIGHT' || :"
}

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

struct TestHome {
    path: PathBuf,
    user: String,
    config_dir: PathBuf,
}

impl TestHome {
    fn new(shell: &Shell) -> Result<Self> {
        let user = current_user()?;
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let config_dir =
            env::temp_dir().join(format!("dotfiles-zsh-config-{}-{suffix}", process::id()));
        let config_dir_path = config_dir.display().to_string();
        let source = env::current_dir()?.canonicalize()?.display().to_string();
        let _ = fs::remove_dir_all(&config_dir);
        fs::create_dir_all(&config_dir)?;

        cmd!(
            shell,
            "env DOTFILES_CONFIG_DIR={config_dir_path} cargo run --package dotfiles-cli -- init --user {user} --host {user} --system aarch64-darwin --source {source}"
        )
        .run()?;

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
        })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }

    fn user(&self) -> &str {
        &self.user
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_dir_all(&self.config_dir);
    }
}

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
        bail!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}")
    }
}

fn expect_match(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual.lines().any(|line| line.trim() == expected) {
        println!("PASS {label}");
        Ok(())
    } else {
        bail!("FAIL {label}\n  expected: {expected}\n  actual:   {actual}")
    }
}

fn expect_nonempty(label: &str, actual: &str) -> Result<()> {
    if actual.trim().is_empty() {
        bail!("FAIL {label}\n  actual: <empty>")
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
        bail!("FAIL {label}\n  expected substring: {needle}\n  actual: {actual}")
    }
}

fn expect_absent(label: &str, actual: &str, needles: &[&str]) -> Result<()> {
    if let Some(needle) = needles.iter().find(|needle| actual.contains(**needle)) {
        bail!("FAIL {label}\n  unexpected substring: {needle}\n  actual: {actual}")
    } else {
        println!("PASS {label}");
        Ok(())
    }
}
