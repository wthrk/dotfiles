use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use anyhow::{anyhow, bail};
use dotfiles_core::host;

const CONFIG_SUBDIR: &str = ".config/dotfiles";
const CONFIG_FILE: &str = "flake.nix";

pub(crate) fn config_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    Ok(override_dir.unwrap_or(home_dir()?.join(CONFIG_SUBDIR)))
}

pub(crate) fn config_path(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    Ok(config_dir(override_dir)?.join(CONFIG_FILE))
}

pub(crate) fn current_user() -> Result<String> {
    let output = Command::new("id").arg("-un").output()?;
    if !output.status.success() {
        bail!("id -un command failed");
    }
    let user = String::from_utf8(output.stdout)?;
    nonempty("user")(user.trim().to_string())
}

pub(crate) fn current_host() -> Result<String> {
    let output = Command::new("hostname").output()?;
    if !output.status.success() {
        bail!("hostname command failed");
    }
    let host = String::from_utf8(output.stdout)?;
    let host = host::short(host.trim());
    if host.is_empty() {
        bail!("host is required")
    } else {
        Ok(host.to_string())
    }
}

pub(crate) fn default_system() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    format!("{arch}-{os}")
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("HOME is required"))
}

fn nonempty(name: &'static str) -> impl FnOnce(String) -> Result<String> {
    move |value| {
        if value.is_empty() {
            bail!("{name} is empty")
        } else {
            Ok(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use dotfiles_core::host;

    #[test]
    fn uses_short_hostname() {
        assert_eq!(host::short("macbook.local"), "macbook");
        assert_eq!(host::short("macbook"), "macbook");
    }
}
