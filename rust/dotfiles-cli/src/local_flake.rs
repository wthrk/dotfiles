//! ユーザー所有 flake の Nix ソースを組み立てる。
//!
//! 生成内容は `inputs.dotfiles.url` と `dotfiles.lib.mkHome` / `dotfiles.lib.mkDarwin` だけに依存する。
//! concrete なユーザー名、ホスト名、システム名は、このリポジトリではなく生成ファイルへ閉じ込める。

use crate::environment::ConfigScope;

/// 生成 flake が dotfiles repo を参照する input 名。
///
/// `dotfiles update` が「dotfiles repo の committed lock に追従」する際、
/// この同じ input 名を `nix flake update <INPUT_NAME>` へ渡す必要がある。
/// 名前がここと `update` 側でずれると、update が dotfiles input を更新できず
/// 推移的 nixpkgs を repo の lock に追従させられない。
pub(crate) const INPUT_NAME: &str = "dotfiles";

/// `#<user>` で Home Manager、`#<host>` で nix-darwin を参照できる flake を描画する。
///
/// `scope` が [`ConfigScope::Home`] のときは `darwinConfigurations` を出力しない。system 層を
/// 別ユーザーが持つマシンでは、その出力があるだけで `darwin-rebuild` の適用先になりうる。
pub(crate) fn render(
    source: &str,
    user: &str,
    host: &str,
    system: &str,
    include_self_package: bool,
    scope: ConfigScope,
) -> String {
    let include_self_package = if include_self_package {
        String::new()
    } else {
        r#"
        includeSelfPackage = false;"#
            .to_string()
    };
    let darwin_configurations = match scope {
        ConfigScope::Home => String::new(),
        ConfigScope::Full => format!(
            r#"
    darwinConfigurations."{host_attr}" =
      {input}.lib.mkDarwin {{
        user = "{user_value}";
        host = "{host_value}";
        system = "{system_value}";
{include_self_package}
      }};
"#,
            input = INPUT_NAME,
            host_attr = escape_nix_string(host),
            user_value = escape_nix_string(user),
            host_value = escape_nix_string(host),
            system_value = escape_nix_string(system),
            include_self_package = include_self_package.clone()
        ),
    };
    format!(
        r#"{{
  inputs.{input}.url = "{source}";

  outputs = {{ {input}, ... }}: {{
    homeConfigurations."{user_attr}" =
      {input}.lib.mkHome {{
        user = "{user_value}";
        system = "{system_value}";
{home_include_self_package}
      }};
{darwin_configurations}  }};
}}
"#,
        input = INPUT_NAME,
        source = escape_nix_string(source),
        user_attr = escape_nix_string(user),
        user_value = escape_nix_string(user),
        system_value = escape_nix_string(system),
        home_include_self_package = include_self_package,
        darwin_configurations = darwin_configurations
    )
}

/// CLI 由来の値を Nix の二重引用符文字列へ安全に埋め込む。
fn escape_nix_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${")
}

/// 生成 flake の出力名、文字列エスケープ、scope ごとの出力範囲を検証する。
#[cfg(test)]
mod tests {
    use super::{ConfigScope, INPUT_NAME, render};

    #[test]
    fn input_name_is_dotfiles() {
        // `dotfiles update` は `nix flake update <INPUT_NAME>` で dotfiles input を
        // 更新する。生成 flake の input 名がこの値からずれると update が追従に失敗する。
        assert_eq!(INPUT_NAME, "dotfiles");
    }

    #[test]
    fn renders_local_config_flake() {
        let flake = render(
            "github:wthrk/dotfiles",
            "alice",
            "macbook",
            "aarch64-darwin",
            true,
            ConfigScope::Full,
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
            true,
            ConfigScope::Full,
        );

        // 動的な参照、ユーザー名、ホスト名は Nix 文字列に埋め込むため、
        // 引用符、バックスラッシュ、補間開始で生成 flake を壊してはいけない。
        assert!(flake.contains(r#"inputs.dotfiles.url = "path:/tmp/a\"b/\${bad}";"#));
        assert!(flake.contains(r#"homeConfigurations."a\\b""#));
        assert!(flake.contains(r#"host = "h\"ost/\${host}";"#));
    }

    #[test]
    fn renders_home_without_self_package_when_requested() {
        let flake = render(
            "github:wthrk/dotfiles",
            "alice",
            "macbook",
            "aarch64-darwin",
            false,
            ConfigScope::Full,
        );

        assert_eq!(flake.matches("includeSelfPackage = false;").count(), 2);
    }

    /// system 層を別ユーザーが持つマシンでは、生成 flake に nix-darwin の適用先を作らない。
    #[test]
    fn home_scope_renders_no_darwin_configuration() {
        let flake = render(
            "github:wthrk/dotfiles",
            "bob",
            "macbook",
            "aarch64-darwin",
            true,
            ConfigScope::Home,
        );

        assert!(flake.contains(r#"homeConfigurations."bob""#));
        assert!(!flake.contains("darwinConfigurations"));
        assert!(!flake.contains("mkDarwin"));
    }
}
