use std::process::Command;

use crate::Result;
use anyhow::bail;

pub fn step(label: &str) {
    println!("==> {label}");
}

pub fn current_user() -> Result<String> {
    let output = Command::new("id").arg("-un").output()?;
    if !output.status.success() {
        bail!("id -un command failed");
    }
    let user = String::from_utf8(output.stdout)?;
    let user = user.trim();
    if user.is_empty() {
        bail!("user is empty");
    }
    Ok(user.to_string())
}
