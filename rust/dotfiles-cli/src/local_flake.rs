//! ユーザー所有 flake の Nix ソースを組み立てる。
//!
//! 生成内容は `inputs.dotfiles.url` と `dotfiles.lib.mkHome` / `dotfiles.lib.mkDarwin` だけに依存する。
//! concrete なユーザー名、ホスト名、システム名は、このリポジトリではなく生成ファイルへ閉じ込める。

/// `#<user>` で Home Manager、`#<host>` で nix-darwin を参照できる flake を描画する。
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

/// CLI 由来の値を Nix の二重引用符文字列へ安全に埋め込む。
fn escape_nix_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${")
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

        // 生成 flake は指定された dotfiles 参照を使う必要がある。
        assert!(flake.contains(r#"inputs.dotfiles.url = "github:wthrk/dotfiles";"#));
        // `dotfiles switch home` は `#<user>` を参照するため、
        // Home Manager の出力名はユーザー名にする。
        assert!(flake.contains(r#"homeConfigurations."alice""#));
        // `dotfiles switch darwin` は `#<host>` を参照するため、
        // Darwin の出力名はホスト名にする。
        assert!(flake.contains(r#"darwinConfigurations."macbook""#));
        // 具体的なユーザー名、ホスト名、システム名はリポジトリ側の flake ではなく
        // 生成された flake だけに入る。
        assert!(flake.contains(r#"user = "alice";"#));
        assert!(flake.contains(r#"host = "macbook";"#));
        assert!(flake.contains(r#"system = "aarch64-darwin";"#));
        // 生成された flake は公開された生成関数を使う必要がある。
        assert!(flake.contains("dotfiles.lib.mkHome"));
        assert!(flake.contains("dotfiles.lib.mkDarwin"));
    }

    #[test]
    fn escapes_nix_strings() {
        let flake = render(
            "path:/tmp/a\"b/${bad}",
            "a\\b",
            "h\"ost/${host}",
            "x86_64-linux",
        );

        // 動的な参照、ユーザー名、ホスト名は Nix 文字列に埋め込むため、
        // 引用符、バックスラッシュ、補間開始で生成 flake を壊してはいけない。
        assert!(flake.contains(r#"inputs.dotfiles.url = "path:/tmp/a\"b/\${bad}";"#));
        assert!(flake.contains(r#"homeConfigurations."a\\b""#));
        assert!(flake.contains(r#"host = "h\"ost/\${host}";"#));
    }
}
