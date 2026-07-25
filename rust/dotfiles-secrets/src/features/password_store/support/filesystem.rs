//! adapter が使う OS filesystem の安全な技術 primitive。

use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::Result;

pub(crate) const SMALL_TEXT_FILE_MAX_BYTES: u64 = 64 * 1024;

pub(crate) fn home_child(name: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(name))
}

pub(crate) fn path_exists_including_broken_symlink(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

pub(crate) fn is_regular_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

pub(crate) fn read_regular_text_lines(path: &Path) -> Result<Vec<String>> {
    use std::io::Read;

    let pre = path
        .symlink_metadata()
        .context("failed to stat regular text file")?;
    if !pre.file_type().is_file() {
        anyhow::bail!("refusing to read a non-regular file");
    }
    if pre.len() > SMALL_TEXT_FILE_MAX_BYTES {
        anyhow::bail!("regular text file is unexpectedly large");
    }
    let file = std::fs::File::open(path).context("failed to open regular text file")?;
    let post = file
        .metadata()
        .context("failed to restat regular text file")?;
    if !post.file_type().is_file() || pre.dev() != post.dev() || pre.ino() != post.ino() {
        anyhow::bail!("regular text file changed between stat and open");
    }
    let mut contents = String::new();
    let read = (&file)
        .take(SMALL_TEXT_FILE_MAX_BYTES + 1)
        .read_to_string(&mut contents)
        .context("failed to read regular text file")?;
    if read as u64 > SMALL_TEXT_FILE_MAX_BYTES {
        anyhow::bail!("regular text file is unexpectedly large");
    }
    Ok(contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(str::to_owned)
        .collect())
}

pub(crate) fn first_regular_file_with_extension(
    root: &Path,
    extension: &str,
    excluded_directory: &str,
) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some(excluded_directory) {
                    stack.push(path);
                }
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some(extension)
            {
                return Some(path);
            }
        }
    }
    None
}
