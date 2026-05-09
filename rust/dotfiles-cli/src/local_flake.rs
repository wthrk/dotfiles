pub(crate) fn render(source: &str, user: &str, host: &str, system: &str) -> String {
    format!(
        r#"{{
  inputs.dotfiles.url = "{source}";

  outputs = {{ dotfiles, ... }}: {{
    homeConfigurations."{user_attr}" =
      dotfiles.lib.mkHome {{
        user = "{user_value}";
        system = "{system_value}";
      }};

    darwinConfigurations."{host_attr}" =
      dotfiles.lib.mkDarwin {{
        user = "{user_value}";
        host = "{host_value}";
        system = "{system_value}";
      }};
  }};
}}
"#,
        source = escape_nix_string(source),
        user_attr = escape_nix_string(user),
        user_value = escape_nix_string(user),
        host_attr = escape_nix_string(host),
        host_value = escape_nix_string(host),
        system_value = escape_nix_string(system)
    )
}

fn escape_nix_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn renders_local_config_flake() {
        let flake = render(
            "github:wthrk/dotfiles",
            "alice",
            "macbook",
            "aarch64-darwin",
        );

        assert!(flake.contains(r#"inputs.dotfiles.url = "github:wthrk/dotfiles";"#));
        assert!(flake.contains(r#"homeConfigurations."alice""#));
        assert!(flake.contains(r#"darwinConfigurations."macbook""#));
        assert!(flake.contains(r#"user = "alice";"#));
        assert!(flake.contains(r#"host = "macbook";"#));
        assert!(flake.contains(r#"system = "aarch64-darwin";"#));
        assert!(flake.contains("dotfiles.lib.mkHome"));
        assert!(flake.contains("dotfiles.lib.mkDarwin"));
    }

    #[test]
    fn escapes_nix_strings() {
        let flake = render("path:/tmp/a\"b", "a\\b", "h\"ost", "x86_64-linux");

        assert!(flake.contains(r#"inputs.dotfiles.url = "path:/tmp/a\"b";"#));
        assert!(flake.contains(r#"homeConfigurations."a\\b""#));
        assert!(flake.contains(r#"host = "h\"ost";"#));
    }
}
