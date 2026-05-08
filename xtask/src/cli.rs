use std::env;

use crate::{Result, apply, check};

pub fn dispatch() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("apply") => apply::run(args.collect()),
        Some("check") => check::run(args.collect()),
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
  cargo xtask apply
  cargo xtask apply all
  cargo xtask apply home-manager
  cargo xtask check
  cargo xtask check static
  cargo xtask check zsh
  cargo xtask check runtime <fresh-bootstrap|second-user-home-manager|darwin-switch-ya|all>
  cargo xtask check all"#
    );
}
