use std::ffi::{OsStr, OsString};

pub fn display(program: impl AsRef<OsStr>, args: &[OsString]) -> String {
    std::iter::once(program.as_ref().to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .map(|arg| quote(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub fn os_strings<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect()
}
