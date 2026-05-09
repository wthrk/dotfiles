use std::fs;
use std::path::Path;

use crate::Result;
use anyhow::bail;

pub(crate) fn assert_managed_links(home: &str, paths: &[&str]) -> Result<()> {
    for relative in paths {
        let path = Path::new(home).join(relative);
        let meta = fs::symlink_metadata(&path)?;
        if !meta.file_type().is_symlink() {
            bail!("{} is not a symlink", path.display());
        }
        let target = fs::read_link(&path)?;
        if !target.starts_with("/nix/store") {
            bail!(
                "{} does not point into /nix/store: {}",
                path.display(),
                target.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn ensure_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        Ok(())
    } else {
        bail!("{} is missing", path.display())
    }
}

pub(crate) fn ensure_absent_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        bail!("{} exists", path.display())
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_nonempty_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.metadata()?.len() > 0 {
        Ok(())
    } else {
        bail!("{} is empty", path.display())
    }
}
