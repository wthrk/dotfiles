//! ログに出す外部コマンド表記を全クレートで揃える。
//!
//! ここで作る文字列は診断表示専用で、シェルへ再入力するための完全な escaping ではない。
//! 実行は常に `std::process::Command` の引数配列で行う。

use std::ffi::{OsStr, OsString};

/// プログラム名と引数配列を、人間が追える 1 行のログ表記にする。
pub fn display(program: impl AsRef<OsStr>, args: &[OsString]) -> String {
    std::iter::once(program.as_ref().to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .map(|arg| quote(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 空白や記号を含む引数だけを単引用符で囲み、ログの読み間違いを減らす。
pub fn quote(value: &str) -> String {
    let value = value
        .replace('\\', "\\\\")
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@+".contains(ch))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// `Command::args` に渡す値とログ表示用の値を同じ `OsString` 配列から作れるようにする。
pub fn os_strings<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{display, quote};
    use std::ffi::OsString;

    #[test]
    fn quote_escapes_carriage_returns_and_newlines_into_one_line() {
        assert_eq!(quote("line1\r\nline2"), "'line1\\r\\nline2'");
    }

    #[test]
    fn display_escapes_control_line_breaks_in_program_and_arguments() {
        let rendered = display("program\nname", &[OsString::from("argument\rvalue")]);

        assert_eq!(rendered, "'program\\nname' 'argument\\rvalue'");
        assert!(!rendered.contains(['\r', '\n']));
    }
}
