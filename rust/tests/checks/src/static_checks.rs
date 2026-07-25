//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell 構文、Nix flake、workflow、AST 境界などの静的検証を実行する。

use std::path::{Path, PathBuf};
use std::{env, fs, process};

use anyhow::{bail, ensure};
use proc_macro2::TokenStream;
use syn::parse::Parser;
use xshell::{Shell, cmd};

use crate::{Result, command::step};

mod architecture_boundaries;
mod documentation_comments;

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    auto_update_wrapper_uses_update_all_semantics(&shell)?;
    darwin_home_manager_propagates_include_self_package(&shell)?;
    nix_flake_source_candidates_are_tracked()?;
    architecture_boundaries::check(&shell)?;
    documentation_comments::check(&shell)?;
    adapter_boundary_is_structurally_closed(&shell)?;
    internal_stub_observation_is_mechanically_isolated(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)
}

/// adapter/support 境界を Rust AST で fail-closed に検証する。
///
/// 行・brace depth・文字列検索では macro、attribute、複数行宣言、nested module を正しく
/// 扱えない。ここでは全 adapter source を `syn::File` として構文解析し、adapter に許す
/// item を「support-owned backend への forwarding-only port trait impl」と import / doc
/// attribute だけへ限定する。従って free function、inherent impl、state type、const/static/
/// type alias/internal trait、再 export aggregation、adapter 内の SDK/process/device/codec
/// 実装を一律に拒否する。port trait method が support backend への単一委譲でない場合も拒否する。
/// `#[cfg(test)]` inline test は module 自身の private test として明示的に許可する。
///
/// support 側も AST で調べ、`crate::adapters` import、`super::adapters` import、adapter
/// source を読む `#[path]` / `include!` を禁止する。これにより test stub state を support
/// に置く場合でも support -> adapter 依存を作れない。
fn adapter_boundary_is_structurally_closed(shell: &Shell) -> Result<()> {
    step("adapter structural boundary");
    let feature_root = shell
        .current_dir()
        .join("rust/dotfiles-secrets/src/features");
    let files = feature_layer_files(&feature_root, "adapters")?;
    let mut violations = Vec::new();
    for path in files {
        // `adapters.rs` is the feature-private module declaration, not an
        // adapter implementation source.  The manifest checker owns its
        // visibility/import rule; forwarding shape applies to leaf files.
        if path.file_stem().is_some_and(|stem| stem == "adapters") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            anyhow::anyhow!("failed to parse adapter source {}: {error}", path.display())
        })?;
        collect_adapter_item_violations(&syntax.items, false, &path, &mut violations);
    }

    let support_files = feature_layer_files(&feature_root, "support")?;
    for path in support_files {
        let source = fs::read_to_string(&path)?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            anyhow::anyhow!("failed to parse support source {}: {error}", path.display())
        })?;
        collect_support_to_adapter_violations(&syntax.items, &path, &mut violations);
    }

    ensure!(
        violations.is_empty(),
        "adapter boundary must contain forwarding-only port trait implementations and must not be imported by support: {}",
        violations.join(", ")
    );
    Ok(())
}

/// feature-first source layout の指定 layer 配下だけを収集する。
///
/// `features/<feature>/<layer>.rs` は module declaration であり、実装 source
/// は `features/<feature>/<layer>/**/*.rs` に置く。層名を path component として
/// 判定することで、旧 flat `src/adapters` / `src/support` path へ戻ることを防ぐ。
fn feature_layer_files(feature_root: &Path, layer: &str) -> Result<Vec<PathBuf>> {
    let mut all_feature_files = Vec::new();
    collect_rust_files(feature_root, &mut all_feature_files)?;
    Ok(all_feature_files
        .into_iter()
        .filter(|path| {
            path.components()
                .any(|component| component.as_os_str() == layer)
        })
        .collect())
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_adapter_item_violations(
    items: &[syn::Item],
    inside_inline_test: bool,
    path: &Path,
    violations: &mut Vec<String>,
) {
    let owned_backends = forwarding_backend_imported_identifiers(items);
    for item in items {
        match item {
            syn::Item::Use(_) if !inside_inline_test => {}
            // A test-only port implementation is intentionally absent from
            // the production adapter surface.  It must be skipped before
            // the production impl arm; otherwise the guarded arm does not
            // match and the catch-all below reports a false forbidden-item
            // violation.
            syn::Item::Impl(item_impl)
                if !inside_inline_test && is_test_only_cfg(&item_impl.attrs) => {}
            syn::Item::Impl(item_impl)
                if !inside_inline_test && !is_test_only_cfg(&item_impl.attrs) =>
            {
                if item_impl.trait_.is_none() {
                    violations.push(format!("{}: inherent impl is forbidden", path.display()));
                    continue;
                }
                if !is_port_trait_impl(item_impl) {
                    violations.push(format!(
                        "{}: adapter impl must implement an external-I/O Port trait",
                        path.display()
                    ));
                }
                if item_impl
                    .items
                    .iter()
                    .any(|item| !matches!(item, syn::ImplItem::Fn(_)))
                {
                    violations.push(format!(
                        "{}: adapter trait impl may contain methods only",
                        path.display()
                    ));
                }
                if !is_forwarding_only_impl(item_impl, &owned_backends) {
                    violations.push(format!(
                        "{}: adapter port implementation must forward each method directly to an owned backend operation",
                        path.display()
                    ));
                }
            }
            syn::Item::Mod(module) if is_inline_test_module(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_adapter_item_violations(nested, true, path, violations);
                }
            }
            // Inline tests may contain test helpers and local test-only state; the production
            // adapter boundary is not widened by those private test items.
            _ if inside_inline_test => {}
            _ => violations.push(format!(
                "{}: forbidden adapter item `{}`",
                path.display(),
                adapter_item_kind(item)
            )),
        }
    }
}

fn is_port_trait_impl(item_impl: &syn::ItemImpl) -> bool {
    item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
        path.segments
            .last()
            .is_some_and(|segment| segment.ident.to_string().ends_with("Port"))
    })
}

fn is_inline_test_module(module: &syn::ItemMod) -> bool {
    module.attrs.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

/// A `#[cfg(test)]` impl or an explicitly designated internal test-stub
/// feature is not a production adapter implementation. In particular, an
/// empty impl may be required to satisfy a test-only port surface while the
/// real `#[cfg(not(test))]` implementation remains subject to the forwarding
/// gate. Arbitrary features and compound predicates remain checked.
fn is_test_only_cfg(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute.meta.require_list().is_ok_and(|list| {
                // Parse the predicate instead of comparing TokenStream text.
                // `all(test, ...)` is still test-only: every configuration
                // satisfying it has `test` enabled.  Conversely, `any(...)`
                // is exempt only when every branch is explicitly test-only;
                // this keeps `any(test, production_feature)` in the checked
                // production surface.  The same recursive rule covers the
                // compound cfg used by the gpg-agent adapter's test stub.
                cfg_predicate_is_test_only(&list.tokens)
            })
    })
}

fn cfg_predicate_is_test_only(tokens: &TokenStream) -> bool {
    let Ok(predicate) = syn::parse2::<syn::Meta>(tokens.clone()) else {
        return false;
    };
    cfg_meta_is_test_only(&predicate)
}

fn cfg_meta_is_test_only(predicate: &syn::Meta) -> bool {
    match predicate {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::NameValue(name_value) => {
            name_value.path.is_ident("feature")
                && matches!(
                    &name_value.value,
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(value),
                        ..
                    }) if value.value() == "secrets-internal-test-stub"
                )
        }
        syn::Meta::List(list) if list.path.is_ident("all") => {
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .is_ok_and(|predicates| predicates.iter().any(cfg_meta_is_test_only))
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .is_ok_and(|predicates| {
                    !predicates.is_empty() && predicates.iter().all(cfg_meta_is_test_only)
                })
        }
        _ => false,
    }
}

fn adapter_item_kind(item: &syn::Item) -> &'static str {
    match item {
        syn::Item::Const(_) => "const",
        syn::Item::Enum(_) => "enum",
        syn::Item::ExternCrate(_) => "extern crate",
        syn::Item::Fn(_) => "free function",
        syn::Item::ForeignMod(_) => "foreign module",
        syn::Item::Impl(_) => "impl",
        syn::Item::Macro(_) => "macro",
        syn::Item::Mod(_) => "module",
        syn::Item::Static(_) => "static",
        syn::Item::Struct(_) => "struct",
        syn::Item::Trait(_) => "trait",
        syn::Item::TraitAlias(_) => "trait alias",
        syn::Item::Type(_) => "type alias",
        syn::Item::Union(_) => "union",
        syn::Item::Use(_) => "use",
        syn::Item::Verbatim(_) => "verbatim",
        _ => "unknown",
    }
}

/// adapter は support-owned concrete backend または presentation-owned feature I/O backend
/// への forwarding-only trait implementation である。
///
/// method body は backend module/type の `Backend::operation(args...)` を呼ぶ単一
/// expression（async の `.await` を含む）だけを許可する。backend が receiver を要求する場合は
/// call の先頭引数が同じ `self`、それ以外の引数は port method の named parameter と同じ順序・個数・
/// identifier でなければならない。これにより fabricated/default receiver、引数省略・並替え、local
/// conversion を adapter に潜ませない。`self.method()` を含む method call は、receiver が support-owned
/// concrete backend に見えても adapter 自身の helper / state / logic を経由する余地があるため許可しない。
/// local state、conversion、branch、error context、SDK 呼び出しを adapter 側へ再導入しないため、少しでも
/// 複合的な body は fail-closed にする。
fn is_forwarding_only_impl(
    item_impl: &syn::ItemImpl,
    owned_backends: &std::collections::BTreeSet<String>,
) -> bool {
    !item_impl.items.is_empty()
        && item_impl.items.iter().all(|item| match item {
            syn::ImplItem::Fn(method) => method_is_direct_backend_forward(method, owned_backends),
            _ => false,
        })
}

fn method_is_direct_backend_forward(
    method: &syn::ImplItemFn,
    owned_backends: &std::collections::BTreeSet<String>,
) -> bool {
    let [syn::Stmt::Expr(expression, None)] = method.block.stmts.as_slice() else {
        return false;
    };
    let expression = match expression {
        syn::Expr::Await(await_expression) => await_expression.base.as_ref(),
        expression => expression,
    };
    let syn::Expr::Call(call) = expression else {
        return false;
    };
    call_targets_owned_backend(call.func.as_ref(), owned_backends)
        && call_forwards_method_arguments_exactly(method, call)
}

/// backend call が port method の receiver / named parameters を加工せず渡すことを AST で確認する。
///
/// backend が state を持つ場合だけ先頭の `self` forwarding を許し、`Backend::default()`、別 local、
/// `&self`、field access は拒否する。残りは method declaration の typed parameter と同一 identifier
/// だけを同じ順序・個数で許す。support backend が receiver を取らない static operation もあるため、
/// `self` は call に現れる場合だけ先頭で許可する。いずれの場合も adapter が値を生成・省略・変換・
/// 並べ替える余地はない。
fn call_forwards_method_arguments_exactly(method: &syn::ImplItemFn, call: &syn::ExprCall) -> bool {
    let Some(expected) = method_parameter_identifiers(method) else {
        return false;
    };
    let actual = call.args.iter().collect::<Vec<_>>();
    let actual = match actual.first() {
        Some(expression) if expression_is_ident(expression, "self") => &actual[1..],
        _ => actual.as_slice(),
    };
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(expression, identifier)| expression_is_ident(expression, &identifier))
}

fn method_parameter_identifiers(method: &syn::ImplItemFn) -> Option<Vec<String>> {
    method
        .sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                syn::Pat::Ident(identifier) if identifier.subpat.is_none() => {
                    Some(Some(identifier.ident.to_string()))
                }
                _ => Some(None),
            },
        })
        .collect()
}

fn expression_is_ident(expression: &syn::Expr, identifier: &str) -> bool {
    matches!(
        expression,
        syn::Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == identifier
    )
}

fn call_targets_owned_backend(
    expression: &syn::Expr,
    owned_backends: &std::collections::BTreeSet<String>,
) -> bool {
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    let segments = path.path.segments.iter().collect::<Vec<_>>();
    let Some(last) = segments.last() else {
        return false;
    };
    let Some(module) = segments.get(segments.len().saturating_sub(2)) else {
        return false;
    };

    if let Some(support_index) = segments
        .iter()
        .position(|segment| segment.ident == "support")
    {
        return segments.len() >= support_index + 3
            && is_support_backend_module_identifier(
                &segments[support_index + 1].ident.to_string(),
            )
            && last.ident != "support";
    }

    owned_backends.contains(&module.ident.to_string())
        && owned_backends.contains(&segments[0].ident.to_string())
}

/// adapter の `use` から forwarding 先として許可する backend module/type のローカル名を抽出する。
///
/// forwarding source は `use crate::support::bws_backend; bws_backend::operation(...)` の
/// ように support を省略して呼べる。この解決をしないと、正しい forwarding を helper 呼び出しと
/// 誤判定する。一方、function / type の単体 import や support 以外から import した同名 module は
/// 集合に入れない。feature I/O は architecture 上 presentation 所有なので、
/// `crate::features::<feature>::presentation::*` から import した明示許可型だけを同じ集合へ加える。
fn forwarding_backend_imported_identifiers(
    items: &[syn::Item],
) -> std::collections::BTreeSet<String> {
    let mut identifiers = std::collections::BTreeSet::new();
    for item in items {
        if let syn::Item::Use(item_use) = item {
            collect_support_backend_imported_modules(&item_use.tree, false, &mut identifiers);
            collect_presentation_backend_imported_types(&item_use.tree, false, &mut identifiers);
        }
    }
    identifiers
}

fn collect_support_backend_imported_modules(
    tree: &syn::UseTree,
    inside_support: bool,
    identifiers: &mut std::collections::BTreeSet<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let identifier = path.ident.to_string();
            if inside_support && is_support_backend_module_identifier(&identifier) {
                identifiers.insert(identifier.clone());
            }
            let next_inside_support = inside_support || identifier == "support";
            collect_support_backend_imported_modules(
                path.tree.as_ref(),
                next_inside_support,
                identifiers,
            );
        }
        syn::UseTree::Name(name)
            if inside_support && is_support_backend_module_identifier(&name.ident.to_string()) =>
        {
            identifiers.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename)
            if inside_support
                && is_support_backend_module_identifier(&rename.ident.to_string()) =>
        {
            identifiers.insert(rename.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_support_backend_imported_modules(item, inside_support, identifiers);
            }
        }
        syn::UseTree::Glob(_) | syn::UseTree::Name(_) | syn::UseTree::Rename(_) => {}
    }
}

fn collect_presentation_backend_imported_types(
    tree: &syn::UseTree,
    inside_presentation: bool,
    identifiers: &mut std::collections::BTreeSet<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let identifier = path.ident.to_string();
            let next_inside_presentation = inside_presentation || identifier == "presentation";
            collect_presentation_backend_imported_types(
                path.tree.as_ref(),
                next_inside_presentation,
                identifiers,
            );
        }
        syn::UseTree::Name(name)
            if inside_presentation && is_presentation_backend_type(&name.ident.to_string()) =>
        {
            identifiers.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename)
            if inside_presentation && is_presentation_backend_type(&rename.ident.to_string()) =>
        {
            identifiers.insert(rename.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_presentation_backend_imported_types(item, inside_presentation, identifiers);
            }
        }
        syn::UseTree::Glob(_) | syn::UseTree::Name(_) | syn::UseTree::Rename(_) => {}
    }
}

fn is_presentation_backend_type(identifier: &str) -> bool {
    matches!(
        identifier,
        "HiddenBootstrapDocumentInput"
            | "HiddenTokenInput"
            | "JsonReport"
            | "ProcessPresentation"
            | "StreamedBootstrapDocumentInput"
            | "StreamedTokenInput"
            | "TerminalPivPinInput"
    )
}

fn is_support_backend_module_identifier(identifier: &str) -> bool {
    matches!(
        identifier,
        "bws_backend"
            | "clock"
            | "gpg_cipher_backend"
            | "gpg_keyring_backend"
            | "git_clone"
            | "internal_stub_bws"
            | "internal_stub_git"
            | "internal_stub_gpg"
            | "io_backend"
            | "password_store"
            | "primary_bootstrap_document"
            | "ssh_agent_backend"
            | "yubikey_backend"
            | "yubikey_device_serial"
            | "yubikey_storage"
    )
}

fn collect_support_to_adapter_violations(
    items: &[syn::Item],
    path: &Path,
    violations: &mut Vec<String>,
) {
    for item in items {
        match item {
            syn::Item::Use(item_use) if use_tree_imports_adapter(&item_use.tree, false) => {
                violations.push(format!(
                    "{}: support must not import adapters",
                    path.display()
                ))
            }
            syn::Item::Mod(module) => {
                if attribute_or_path_mentions_adapters(&module.attrs) {
                    violations.push(format!(
                        "{}: support must not load adapter source",
                        path.display()
                    ));
                }
                if let Some((_, nested)) = &module.content {
                    collect_support_to_adapter_violations(nested, path, violations);
                }
            }
            syn::Item::Macro(item_macro)
                if item_macro.mac.path.is_ident("include")
                    && item_macro.mac.tokens.to_string().contains("adapter") =>
            {
                violations.push(format!(
                    "{}: support must not include adapter source",
                    path.display()
                ))
            }
            _ => {}
        }
    }
}

/// support から adapter module への reverse import を alias を含めて拒否する。
///
/// `crate::adapter_bw as backend` や `super::adapter_yubikey` を見逃すと、support-owned
/// backend が adapter を経由して state/schema を持つ逆依存を作れる。`support::adapter_backend`
/// のような support 自身の module は許可するため、`support` 配下へ入った後は判定しない。
fn use_tree_imports_adapter(tree: &syn::UseTree, inside_support: bool) -> bool {
    match tree {
        syn::UseTree::Path(path) => {
            let identifier = path.ident.to_string();
            (!inside_support && is_adapter_module_identifier(&identifier))
                || use_tree_imports_adapter(
                    path.tree.as_ref(),
                    inside_support || identifier == "support",
                )
        }
        syn::UseTree::Name(name) => {
            !inside_support && is_adapter_module_identifier(&name.ident.to_string())
        }
        syn::UseTree::Rename(rename) => {
            !inside_support && is_adapter_module_identifier(&rename.ident.to_string())
        }
        syn::UseTree::Glob(_) => false,
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|item| use_tree_imports_adapter(item, inside_support)),
    }
}

fn is_adapter_module_identifier(identifier: &str) -> bool {
    identifier == "adapters" || identifier.starts_with("adapter_")
}

fn attribute_or_path_mentions_adapters(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("path")
            && attribute.meta.require_name_value().is_ok_and(|value| {
                matches!(
                    &value.value,
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(path), .. })
                        if path.value().contains("adapters")
                )
            })
    })
}

/// Rust ワークスペース全体で、警告を失敗扱いにして整形、型検査、lint を回す。
fn rust(shell: &Shell) -> Result<()> {
    step("cargo fmt");
    cmd!(shell, "cargo fmt --all -- --check").run()?;
    step("cargo check");
    cmd!(shell, "env RUSTFLAGS='-D warnings' cargo check --workspace").run()?;
    step("cargo clippy");
    cmd!(shell, "cargo clippy --workspace -- -D warnings").run()?;
    Ok(())
}

/// internal test stub の stdout observation が production command に混入しないことを、
/// feature 名だけでなく target 境界で検証する。
///
/// security-obligations の test-only 観測の4条件に対応する: (1) 専用 binary の
/// compile-time feature selection、(2) normal target の feature 注入拒否、(3) fixture
/// input は feature-gated integration test に閉じること、(4) production entrypoint に
/// observation の環境変数・sentinel 到達経路が無いこと。これは実機・BWS を起動しない。
fn internal_stub_observation_is_mechanically_isolated(shell: &Shell) -> Result<()> {
    step("internal stub observation isolation");
    let manifest = shell.read_file("rust/dotfiles-cli/Cargo.toml")?;
    let production_main = shell.read_file("rust/dotfiles-cli/src/main.rs")?;
    let integration = shell.read_file("rust/dotfiles-cli/tests/secrets_cli.rs")?;

    ensure!(
        manifest.contains("name = \"dotfiles-secrets-internal-test-stub\"")
            && manifest.contains("required-features = [\"secrets-internal-test-stub\"]")
            && manifest.contains("name = \"dotfiles\"\npath = \"src/main.rs\"\nrequired-features = [\"production-cli\"]"),
        "internal stub は required-features を持つ専用 binary target だけで compile すること"
    );
    ensure!(
        production_main.contains("feature = \"production-cli\"")
            && production_main.contains("feature = \"secrets-internal-test-stub\"")
            && production_main
                .contains("only permitted for the dotfiles-secrets-internal-test-stub test binary"),
        "通常 dotfiles target は internal stub feature の注入を compile-time で拒否すること"
    );
    ensure!(
        integration.starts_with("#![cfg(feature = \"secrets-internal-test-stub\")]")
            && integration.contains("feature_stub_cli_binary"),
        "fixture と sentinel 観測は feature-gated integration test と専用 binary に閉じること"
    );
    ensure!(
        !production_main.contains("DOTFILES_SECRETS_")
            && !production_main.contains("__DOTFILES_SECRETS_STUB_OBSERVATION__"),
        "production entrypoint は stub environment / stdout observation に到達してはならない"
    );
    Ok(())
}

/// shell script の構文だけを検証する。実行 fixture は `test` が所有する。
fn shell_scripts(shell: &Shell) -> Result<()> {
    step("shell scripts");
    cmd!(shell, "bash -n scripts/bootstrap.sh").run()?;
    cmd!(shell, "bash -n scripts/provision-secret-recovery-source.sh").run()?;
    Ok(())
}

/// GitHub Actions workflow の構文と式を actionlint で検証する。
fn github_actions(shell: &Shell) -> Result<()> {
    step("GitHub Actions workflows");
    cmd!(shell, "actionlint").run()?;
    nightly_no_update_is_clean_no_op(shell)?;
    nightly_record_secret_gating_is_testable_and_bounded(shell)?;
    nightly_record_rebuilds_in_job(shell)?;
    nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(shell)?;
    nightly_lock_rev_skips_nix_develop(shell)?;
    nightly_artifact_actions_use_supported_node_runtime(shell)?;
    Ok(())
}

/// `mkDarwin` から Home Manager の子モジュールへ `includeSelfPackage` が落ちずに届くことを固定する。
///
/// `darwinModule` 自体で `_module.args.includeSelfPackage` を持っていても、`nix/darwin.nix` が
/// `home-manager.extraSpecialArgs` へ同値を渡し忘れると、`home.nix -> modules/cli.nix` の評価だけが
/// `attribute 'includeSelfPackage' missing` で落ちる。nightly の `darwinConfigurations.ci-ref` eval は
/// まさにこの経路を踏むため、静的検査で配線抜けを止める。
fn darwin_home_manager_propagates_include_self_package(shell: &Shell) -> Result<()> {
    step("darwin home-manager includeSelfPackage propagation");
    let darwin = shell.read_file("nix/darwin.nix")?;
    ensure!(
        darwin.contains("includeSelfPackage ? true,"),
        "nix/darwin.nix は `includeSelfPackage` をモジュール引数で受け取り、mkDarwin 既定値を保持すること"
    );
    let extra_special_args = darwin
        .split("home-manager.extraSpecialArgs =")
        .nth(1)
        .and_then(|section| section.split("home-manager.users.").next())
        .unwrap_or_default();
    ensure!(
        extra_special_args.contains("includeSelfPackage"),
        "nix/darwin.nix は `home-manager.extraSpecialArgs` へ `includeSelfPackage` を渡し、\
         home.nix -> modules/cli.nix の評価で欠落させないこと"
    );
    Ok(())
}

/// nightly-update.yml の「無更新の夜が clean no-op になる」不変条件を hermetic に固定する（finding 3368677388）。
///
/// 全 input が既に最新で nix/brew 差分も空の夜は run_record が更新履歴 TOML を書かず、record job の
/// history-record アップロード対象が 0 件になりうる。このとき record の upload-artifact が
/// `if-no-files-found: error` だと無更新夜が失敗扱いになり、clean no-op（PR 起票せず success）にならない。
/// アップロードを安全側（`warn`/`ignore`）にし、後段 open-pr の history-record download は無更新夜だけ
/// 失敗を許容（`continue-on-error` を record の `has_history != 'true'` でガード）することで、無更新夜が
/// 全体として no-op になりつつ、更新ありの夜の真の download 失敗は握り潰さないことを workflow テキスト上で
/// 固定する（network/nix 非依存・ファイル内容の静的検査のみ）。
fn nightly_no_update_is_clean_no_op(shell: &Shell) -> Result<()> {
    step("nightly-update no-op (history upload not fail-on-empty)");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;

    // history-record の upload-artifact ステップ本体だけを切り出し、その `if-no-files-found` が `error` でない
    // ことを確認する（無更新夜の 0 件アップロードを失敗扱いにしない）。判定対象を当該ステップ（`- name: 履歴
    // TOML を artifact 化` から次の `- name:` の手前まで）にスコープし、後続に別 upload ステップが追加されても
    // その `if-no-files-found:` を拾わないようにする。安全側の `warn`/`ignore` のいずれかを要求する。
    let upload_section = workflow
        .split("- name: 履歴 TOML を artifact 化")
        .nth(1)
        .unwrap_or_default();
    let upload = upload_section.split("- name:").next().unwrap_or_default();
    ensure!(
        !upload.contains("if-no-files-found: error"),
        "record の history-record アップロードは無更新夜（0 件）を失敗扱いにしないため \
         `if-no-files-found: error` を使ってはならない（warn/ignore で clean no-op にする）"
    );
    ensure!(
        upload.contains("if-no-files-found: warn") || upload.contains("if-no-files-found: ignore"),
        "record の history-record アップロードは `if-no-files-found: warn`/`ignore` で 0 件を許容すること"
    );

    // record job は当月 history を書いたか（更新あり）を `has_history` output で後段へ渡すこと。これが無いと
    // open-pr 側で無更新夜と更新夜を区別できず、download 失敗を一律握り潰す回帰へ戻る。
    ensure!(
        workflow.contains("has_history: ${{ steps.record.outputs.has_history }}"),
        "record job は当月 history を書いたかを `has_history` output で公開すること（更新夜の download 失敗を \
         握り潰さないための分岐根拠）"
    );

    // open-pr の history-record download は、無更新夜（record が history を書かない）だけ artifact 不在を許容し、
    // 更新ありの夜（has_history=true）は download の一時失敗を握り潰さず fail-closed にすること。そのため
    // `continue-on-error` を `needs.record.outputs.has_history != 'true'` でガードする（無条件 `true` は禁止）。
    let download = workflow
        .split("name: 履歴 TOML を取得")
        .nth(1)
        .unwrap_or_default();
    let download_step = download.split("- name:").next().unwrap_or_default();
    ensure!(
        download_step
            .contains("continue-on-error: ${{ needs.record.outputs.has_history != 'true' }}"),
        "open-pr の history-record download は無更新夜だけ失敗を許容し更新夜は fail-closed にするため \
         `continue-on-error: ${{ needs.record.outputs.has_history != 'true' }}` でガードすること"
    );
    ensure!(
        !download_step.contains("continue-on-error: true"),
        "open-pr の history-record download は無条件 `continue-on-error: true` を使ってはならない \
         （更新夜の真の download 失敗を握り潰す）"
    );
    Ok(())
}

/// nightly-update.yml の record 要約経路が「default branch ref に限定された secret 注入」になっていることを固定する。
///
/// `OPEN_AI_API_KEY` を workflow_dispatch の任意 ref に戻すと、未審査 ref の Rust/Nix コードへ secret を
/// 渡せる。そこで record job の secret 注入は `schedule` または `workflow_dispatch && github.actor ==
/// github.repository_owner && github.ref == default_branch` に限定し、未審査 ref の dry-run では version-only
/// に倒す。open-pr job 側の既定ブランチ制限と合わせて、secret を使う build/record 経路全体を既定ブランチへ
/// 閉じ込める。
fn nightly_record_secret_gating_is_testable_and_bounded(shell: &Shell) -> Result<()> {
    step("nightly-update record secret gating");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_record_secret_gating_is_testable_and_bounded(&workflow)
}

#[cfg(test)]
fn record_secret_gate_allows(
    event_name: &str,
    actor: &str,
    repository_owner: &str,
    git_ref: &str,
    default_branch: &str,
) -> bool {
    event_name == "schedule"
        || (event_name == "workflow_dispatch"
            && actor == repository_owner
            && git_ref == format!("refs/heads/{default_branch}"))
}

fn assert_nightly_record_secret_gating_is_testable_and_bounded(workflow: &str) -> Result<()> {
    ensure!(
        workflow.contains(
            "OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && secrets.OPEN_AI_API_KEY || '' }}"
        ),
        "record job の OPEN_AI_API_KEY は schedule または repo owner の default branch workflow_dispatch に限定し、\
         未審査 ref の dry-run へ secret を渡さないこと"
    );
    ensure!(
        workflow.contains(
            "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}"
        ),
        "open-pr job の既定ブランチ限定は維持し、PR 起票/status 投稿経路の信頼境界を弱めてはならない"
    );
    Ok(())
}

/// nightly-update.yml の record job が同一 job で dotfiles binary を再ビルドし、job 間で持ち回した binary の
/// 動的ライブラリ参照切れに依存しないことを静的に固定する。
fn nightly_record_rebuilds_in_job(shell: &Shell) -> Result<()> {
    step("nightly-update record rebuilds binary in job");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_record_rebuilds_in_job(&workflow)
}

fn assert_nightly_record_rebuilds_in_job(workflow: &str) -> Result<()> {
    let record_section = workflow
        .split("- name: record（nix/brew 版差分 + 概要）")
        .nth(1)
        .unwrap_or_default();
    let record_step = record_section.split("- name:").next().unwrap_or_default();
    ensure!(
        workflow.contains("- name: record 用 dotfiles バイナリをビルド")
            && workflow.contains("nix develop -c cargo build -p dotfiles-cli"),
        "record job は同一 job の devShell で dotfiles binary を再ビルドすること"
    );
    ensure!(
        record_step.contains("dotfiles_bin=\"$PWD/target/debug/dotfiles\""),
        "record job は同一 job でビルドした target/debug/dotfiles を使うこと"
    );
    ensure!(
        !workflow.contains("chmod +x target/debug/dotfiles"),
        "record job は artifact binary の実行ビット復元に依存してはならない"
    );
    ensure!(
        !workflow.contains("bump 前 eval 版マップと dotfiles binary を取得"),
        "record job の artifact download は binary を前提にしてはならない"
    );
    Ok(())
}

/// nightly-update.yml の bump artifact が `old-flake.lock` と `repo_base_sha` を保持し、record/open-pr へ
/// それぞれ `--lock-old/--lock-new` + `--cursor-old` と `BUMP_BASE_SHA` で受け渡されることを静的に固定する。
fn nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(shell: &Shell) -> Result<()> {
    step("nightly-update bump artifact preserves old lock and base sha wiring");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(&workflow)
}

fn assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(
    workflow: &str,
) -> Result<()> {
    let old_eval_section = workflow
        .split("- name: bump 前の宣言パッケージ版を eval と rev 抽出")
        .nth(1)
        .unwrap_or_default();
    let old_eval_step = old_eval_section.split("- name:").next().unwrap_or_default();
    ensure!(
        old_eval_step.contains("cp flake.lock old-flake.lock"),
        "bump job は flake update 前に `cp flake.lock old-flake.lock` で旧 lock を保存すること"
    );
    ensure!(
        old_eval_step
            .contains("echo \"repo_base_sha=$(git rev-parse HEAD)\" >> \"$GITHUB_OUTPUT\""),
        "bump job は artifact 作成時点の checkout HEAD を `repo_base_sha` output として公開すること"
    );

    let bump_artifact_section = workflow
        .split("- name: bump 済み lock と eval 版マップを artifact 化")
        .nth(1)
        .unwrap_or_default();
    let bump_artifact_step = bump_artifact_section
        .split("- name:")
        .next()
        .unwrap_or_default();
    ensure!(
        bump_artifact_step.contains("name: bump-state"),
        "bump job は record/open-pr 共有用に `bump-state` artifact を publish すること"
    );
    ensure!(
        bump_artifact_step.contains("old-flake.lock"),
        "bump-state artifact は `old-flake.lock` を含み、record job へ旧 lock を渡すこと"
    );
    ensure!(
        bump_artifact_step.contains("flake.lock"),
        "bump-state artifact は bump 後 `flake.lock` も含むこと"
    );

    let record_section = workflow
        .split("- name: record（nix/brew 版差分 + 概要）")
        .nth(1)
        .unwrap_or_default();
    let record_step = record_section.split("- name:").next().unwrap_or_default();
    ensure!(
        record_step.contains("--lock-old old-flake.lock"),
        "record job は bump artifact から展開した `old-flake.lock` を `--lock-old` で渡すこと"
    );
    ensure!(
        record_step.contains("--lock-new flake.lock"),
        "record job は bump 後 `flake.lock` を `--lock-new` で渡すこと"
    );
    ensure!(
        record_step.contains("--cursor-old \"$REPO_BASE_SHA\""),
        "record job は legacy show --rev 互換のため `repo_base_sha` を `--cursor-old` で渡すこと"
    );

    ensure!(
        workflow.contains("repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}"),
        "bump job outputs は `steps.old.outputs.repo_base_sha` を `repo_base_sha` として公開すること"
    );
    ensure!(
        workflow.contains("BUMP_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}"),
        "open-pr job は `needs.bump.outputs.repo_base_sha` を `BUMP_BASE_SHA` へ配線すること"
    );
    ensure!(
        workflow.contains("if [ \"$base_sha\" != \"$BUMP_BASE_SHA\" ]; then"),
        "open-pr job は `BUMP_BASE_SHA` と現在の default branch HEAD を比較して fail-closed にすること"
    );
    Ok(())
}

/// nightly-update.yml の lock-rev 抽出が `nix develop` を不要に挟まず、純粋な lock file parse として直接実行される
/// ことを静的に固定する。
fn nightly_lock_rev_skips_nix_develop(shell: &Shell) -> Result<()> {
    step("nightly-update lock-rev skips nix develop");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_lock_rev_skips_nix_develop(&workflow)
}

fn assert_nightly_lock_rev_skips_nix_develop(workflow: &str) -> Result<()> {
    ensure!(
        workflow
            .contains("\"$DOTFILES_BIN\" update-history lock-rev --lock flake.lock --node nixpkgs"),
        "lock-rev は built dotfiles binary を直接実行すること"
    );
    ensure!(
        workflow.contains("\"$DOTFILES_BIN\" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask"),
        "cask rev 抽出も built dotfiles binary を直接実行すること"
    );
    ensure!(
        !workflow.contains("nix develop -c \"$DOTFILES_BIN\" update-history lock-rev"),
        "lock-rev は `nix develop` を挟まず直接実行し、不要な shell 起動で bump を遅くしてはならない"
    );
    Ok(())
}

/// nightly-update.yml の artifact action が Node 20 廃止 warning の出る古い major に戻らないことを静的に固定する。
fn nightly_artifact_actions_use_supported_node_runtime(shell: &Shell) -> Result<()> {
    step("nightly-update artifact actions avoid node20 deprecation");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_artifact_actions_use_supported_node_runtime(&workflow)
}

fn assert_nightly_artifact_actions_use_supported_node_runtime(workflow: &str) -> Result<()> {
    ensure!(
        workflow.contains("actions/upload-artifact@v7"),
        "nightly-update は Node 20 廃止 warning を避けるため upload-artifact@v7 を使うこと"
    );
    ensure!(
        workflow.contains("actions/download-artifact@v8"),
        "nightly-update は Node 20 廃止 warning を避けるため download-artifact@v8 を使うこと"
    );
    ensure!(
        !workflow.contains("actions/upload-artifact@v4"),
        "nightly-update は upload-artifact@v4 へ戻してはならない"
    );
    ensure!(
        !workflow.contains("actions/download-artifact@v4"),
        "nightly-update は download-artifact@v4 へ戻してはならない"
    );
    Ok(())
}

/// lock file が存在する状態で、Nix flake の評価と Nix ファイルの整形を検証する。
fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
    cmd!(shell, "test -s flake.lock").run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(shell, "nix fmt -- --ci {files...}").run()?;
    }
    step("nix flake check");
    cmd!(shell, "nix flake check --no-update-lock-file --all-systems").run()?;
    Ok(())
}

/// devShell に入っている `nil` で Nix 診断を実行し、モジュール評価の静的な崩れを検出する。
fn nix_diagnostics(shell: &Shell) -> Result<()> {
    let files = nix_files(shell)?;
    if files.is_empty() {
        return Ok(());
    }

    step("nil diagnostics");
    cmd!(shell, "nil diagnostics --deny-warnings {files...}").run()?;
    Ok(())
}

/// Git flake input は tracked file だけを取り込むため、Cargo が読む未追跡 source を
/// package build 前に拒否する。
///
/// `nix build .#dotfiles-cli` は Git source filter を通す。未追跡の Rust module / binary、
/// `Cargo.toml`、Nix source は local `cargo` では見えても derivation では消える。この
/// preflight は source input と実行時 worktree の差を明示的に失敗にし、Nix の長い build
/// が「file not found」で終わるまで問題を隠すことを防ぐ。対象を source 候補に限定するため、
/// `.env` や `target/` のような未追跡のローカル状態は対象外である。
fn nix_flake_source_candidates_are_tracked() -> Result<()> {
    step("Nix flake source candidates are tracked");
    let candidates = untracked_nix_source_candidates(&env::current_dir()?)?;
    ensure_nix_source_candidates_are_tracked(&candidates)
}

/// 指定 worktree の Git source filter から脱落する Cargo / Nix source 候補を返す。
fn untracked_nix_source_candidates(source: &Path) -> Result<Vec<PathBuf>> {
    let worktree = git_worktree_root(source)?;
    let output = process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("git ls-files failed for {}", worktree.display());
        }
        bail!("git ls-files failed for {}: {stderr}", worktree.display());
    }

    let candidates = output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| Ok(PathBuf::from(std::str::from_utf8(entry)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut candidates = candidates
        .into_iter()
        .filter(|path| is_nix_flake_source_candidate(path))
        .collect::<Vec<_>>();
    candidates.sort();
    Ok(candidates)
}

/// Nix package / module evaluation に影響し、Git flake input に含まれる必要があるファイルかを判定する。
fn is_nix_flake_source_candidate(path: &Path) -> bool {
    let is_rust_source =
        path.starts_with("rust") && path.extension().is_some_and(|extension| extension == "rs");
    let is_cargo_manifest = path.file_name().is_some_and(|name| name == "Cargo.toml");
    let is_nix_source = path.extension().is_some_and(|extension| extension == "nix");

    is_rust_source || is_cargo_manifest || is_nix_source
}

/// source 候補が未追跡なら、Nix Git flake input から除外されるため package build 前に止める。
fn ensure_nix_source_candidates_are_tracked(candidates: &[PathBuf]) -> Result<()> {
    ensure!(
        candidates.is_empty(),
        "Nix Git flake source input から除外される未追跡の Cargo / Nix source がある。\n\
         追跡対象にしてから package build を実行すること:\n  {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  "),
    );
    Ok(())
}

/// worktree 内の任意パスから Git 管理ルートを解決する。
fn git_worktree_root(source: &Path) -> Result<PathBuf> {
    let output = process::Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!(
                "git rev-parse --show-toplevel failed for {}",
                source.display()
            );
        }
        bail!(
            "git rev-parse --show-toplevel failed for {}: {stderr}",
            source.display()
        );
    }

    Ok(PathBuf::from(
        String::from_utf8(output.stdout)?.trim_end_matches('\n'),
    ))
}

/// root auto-update wrapper が `dotfiles update` の既定 `all` 経路を保つことを静的に検証する。
fn auto_update_wrapper_uses_update_all_semantics(shell: &Shell) -> Result<()> {
    step("nix-darwin auto-update wrapper");
    let module = shell.read_file("nix/darwin.nix")?;
    assert_auto_update_wrapper_uses_update_all_semantics(&module)
}

/// wrapper 本体だけを見て、`update darwin` 固定への退行と `--user` 欠落を検出する。
fn assert_auto_update_wrapper_uses_update_all_semantics(module: &str) -> Result<()> {
    let wrapper = module
        .split("autoUpdateWrapper = pkgs.writeShellScript")
        .nth(1)
        .unwrap_or_default()
        .split("'';")
        .next()
        .unwrap_or_default();

    ensure!(
        wrapper.contains("${dotfilesBin} update \\"),
        "auto-update wrapper は target を省略して `dotfiles update` の既定 `all` を使うこと"
    );
    ensure!(
        !wrapper.contains("${dotfilesBin} update darwin"),
        "auto-update wrapper は `dotfiles update darwin` に固定してはならない"
    );
    ensure!(
        wrapper.contains("--user ${lib.escapeShellArg user}"),
        "root daemon からの更新では lock 更新と Home Manager を降格するため `--user` を渡すこと"
    );
    ensure!(
        wrapper.contains("--host ${lib.escapeShellArg host}"),
        "nix-darwin 出力名を固定するため `--host` を渡すこと"
    );
    Ok(())
}

/// `target` 配下を除外し、整形と nil 診断の対象になる Nix ファイルだけを列挙する。
fn nix_files(shell: &Shell) -> Result<Vec<String>> {
    Ok(cmd!(
        shell,
        "find . -path ./target -prune -o -name '*.nix' -type f -print"
    )
    .read()?
    .lines()
    .map(|path| path.trim_start_matches("./"))
    .map(ToOwned::to_owned)
    .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        assert_auto_update_wrapper_uses_update_all_semantics,
        assert_nightly_artifact_actions_use_supported_node_runtime,
        assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring,
        assert_nightly_lock_rev_skips_nix_develop, assert_nightly_record_rebuilds_in_job,
        assert_nightly_record_secret_gating_is_testable_and_bounded,
        collect_adapter_item_violations, collect_support_to_adapter_violations,
        ensure_nix_source_candidates_are_tracked, feature_layer_files, record_secret_gate_allows,
        untracked_nix_source_candidates,
    };
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::{env, process};

    /// wrapper が target を省略し、root daemon 用の `--user` / `--host` を渡す形を受け入れる。
    #[test]
    fn auto_update_wrapper_accepts_default_update_target_with_user_and_host() {
        let module = r#"
          autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
            exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update \
              --config-dir ${lib.escapeShellArg configDir} \
              --user ${lib.escapeShellArg user} \
              --host ${lib.escapeShellArg host}
          '';
        "#;

        assert!(assert_auto_update_wrapper_uses_update_all_semantics(module).is_ok());
    }

    /// `update darwin` へ戻すと root daemon の all semantics が崩れるため検出する。
    #[test]
    fn auto_update_wrapper_rejects_darwin_target_regression() {
        let module = r#"
          autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
            exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update darwin \
              --config-dir ${lib.escapeShellArg configDir} \
              --user ${lib.escapeShellArg user} \
              --host ${lib.escapeShellArg host}
          '';
        "#;

        assert!(assert_auto_update_wrapper_uses_update_all_semantics(module).is_err());
    }

    /// record job の OpenAI secret は repo owner の manual dispatch でも default branch ref に限定される。
    #[test]
    fn nightly_record_secret_gating_accepts_owner_default_branch_dispatch_and_keeps_open_pr_gate() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        assert!(assert_nightly_record_secret_gating_is_testable_and_bounded(workflow).is_ok());
    }

    #[test]
    fn record_secret_gate_rejects_owner_non_default_branch_dispatch() {
        assert!(!record_secret_gate_allows(
            "workflow_dispatch",
            "owner",
            "owner",
            "refs/heads/feature",
            "main"
        ));
    }

    #[test]
    fn record_secret_gate_accepts_owner_default_branch_dispatch() {
        assert!(record_secret_gate_allows(
            "workflow_dispatch",
            "owner",
            "owner",
            "refs/heads/main",
            "main"
        ));
    }

    /// record job の OpenAI secret を owner の任意 dispatch へ戻す退行は、未審査 ref へ secret が流れるため拒否する。
    #[test]
    fn nightly_record_secret_gating_rejects_non_default_branch_dispatch_regression() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner)) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        let result = assert_nightly_record_secret_gating_is_testable_and_bounded(workflow);
        assert!(result.is_err());
    }

    #[test]
    fn nightly_record_rebuilds_binary_in_job() {
        let workflow = r#"
          - name: record 用 dotfiles バイナリをビルド
            run: nix develop -c cargo build -p dotfiles-cli
          - name: record（nix/brew 版差分 + 概要）
            run: |
              dotfiles_bin="$PWD/target/debug/dotfiles"
              nix develop -c "$dotfiles_bin" update-history record \
                --out "$out"
        "#;

        assert!(assert_nightly_record_rebuilds_in_job(workflow).is_ok());
    }

    #[test]
    fn nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring() {
        let workflow = r#"
          outputs:
            repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}
          - name: bump 前の宣言パッケージ版を eval と rev 抽出
            run: |
              cp flake.lock old-flake.lock
              echo "repo_base_sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
          - name: bump 済み lock と eval 版マップを artifact 化
            with:
              name: bump-state
              path: |
                flake.lock
                old-flake.lock
                nix-old.json
          - name: record（nix/brew 版差分 + 概要）
            env:
              REPO_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}
            run: |
              nix develop -c "$dotfiles_bin" update-history record \
                --lock-old old-flake.lock \
                --lock-new flake.lock \
                --cursor-old "$REPO_BASE_SHA" \
                --out "$out"
          - name: bump ブランチを作成して commit
            env:
              BUMP_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}
            run: |
              if [ "$base_sha" != "$BUMP_BASE_SHA" ]; then
                exit 1
              fi
        "#;

        assert!(
            assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(workflow).is_ok()
        );
    }

    #[test]
    fn nightly_bump_artifact_rejects_missing_old_lock_and_base_sha_wiring() {
        let workflow = r#"
          outputs:
            repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}
          - name: bump 前の宣言パッケージ版を eval と rev 抽出
            run: |
              echo "repo_base_sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
          - name: bump 済み lock と eval 版マップを artifact 化
            with:
              name: bump-state
              path: |
                flake.lock
                nix-old.json
          - name: record（nix/brew 版差分 + 概要）
            run: |
              nix develop -c "$dotfiles_bin" update-history record \
                --lock-new flake.lock \
                --out "$out"
          - name: bump ブランチを作成して commit
            env:
              BUMP_BASE_SHA: ${{ github.sha }}
            run: |
              if [ "$base_sha" != "$BUMP_BASE_SHA" ]; then
                exit 1
              fi
        "#;

        assert!(
            assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(workflow).is_err()
        );
    }

    #[test]
    fn nightly_lock_rev_runs_without_nix_develop() {
        let workflow = r#"
          nixpkgs_old="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node nixpkgs)"
          cask_rev_old="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask)"
          nixpkgs_new="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node nixpkgs)"
          cask_rev_new="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask)"
        "#;

        assert!(assert_nightly_lock_rev_skips_nix_develop(workflow).is_ok());
    }

    #[test]
    fn nightly_artifact_actions_use_supported_node_runtime() {
        let workflow = r#"
          - uses: actions/upload-artifact@v7
          - uses: actions/download-artifact@v8
        "#;

        assert!(assert_nightly_artifact_actions_use_supported_node_runtime(workflow).is_ok());
    }

    #[test]
    fn adapter_ast_gate_rejects_helpers_state_inherent_impls_and_non_forwarding_port_method()
    -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self) {}
            }
            fn leaked_helper() {}
            #[cfg(feature = "stub")]
            struct StubState;
            impl crate::support::Backend { fn leaked_inherent(&self) {} }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 4);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_allows_port_impl_and_inline_unit_test() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self, value: String) { crate::support::bws_backend::translate(self, value) }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn private_test_helper_is_local() {}
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_allows_cfg_test_only_empty_port_impl_but_checks_production_impl()
    -> TestResult {
        let source = r#"
            use crate::ports::Port;
            #[cfg(not(test))]
            impl Port for crate::support::Backend {
                fn translate(&self) { crate::support::bws_backend::translate(self) }
            }
            #[cfg( test )]
            impl Port for crate::support::Backend {}
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_recognizes_compound_test_only_cfg() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            #[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
            impl Port for crate::support::Backend {}
            #[cfg(any(test, feature = "production"))]
            impl Port for crate::support::Backend {}
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_does_not_skip_cfg_not_test_non_forwarding_impl() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            #[cfg(not(test))]
            impl Port for crate::support::Backend {
                fn translate(&self) { self.translate() }
            }
            #[cfg(test)]
            impl Port for crate::support::Backend {}
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_allows_only_explicit_internal_stub_feature() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            #[cfg(feature = "secrets-internal-test-stub")]
            impl Port for crate::support::Backend {
                fn translate(&self) { self.translate() }
            }
            #[cfg(feature = "other-test-double")]
            impl Port for crate::support::Backend {
                fn translate(&self) { self.translate() }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_allows_presentation_owned_io_backend_type() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            use crate::presentation::io::ProcessPresentation;
            impl Port for ProcessPresentation {
                fn translate(&self, value: String) {
                    ProcessPresentation::translate(self, value)
                }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_non_presentation_backend_with_allowed_type_name() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            use crate::domain::ProcessPresentation;
            impl Port for ProcessPresentation {
                fn translate(&self, value: String) {
                    ProcessPresentation::translate(self, value)
                }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_adapter_receiver_method_call() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self) { self.translate() }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_support_non_backend_direct_call() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self) { crate::support::utility::translate() }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_branching_before_backend_call() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self, branch: bool) {
                    if branch { crate::support::bws_backend::translate() } else { crate::support::bws_backend::translate() }
                }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_adapter_owned_conversion_before_forwarding() -> TestResult {
        let source = r#"
            use crate::ports::Port;
            impl Port for crate::support::Backend {
                fn translate(&self, value: String) {
                    let normalized = value.trim();
                    crate::support::backend::translate(self, normalized)
                }
            }
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("adapter.rs");
        let mut violations = Vec::new();
        collect_adapter_item_violations(&syntax.items, false, path, &mut violations);
        assert_eq!(violations.len(), 1);
        Ok(())
    }

    #[test]
    fn adapter_ast_gate_rejects_fabricated_or_reordered_forwarding_arguments() -> TestResult {
        for source in [
            r#"
                use crate::ports::Port;
                impl Port for crate::support::Backend {
                    fn translate(&self, first: String, second: String) {
                        crate::support::bws_backend::translate(first, "fabricated")
                    }
                }
            "#,
            r#"
                use crate::ports::Port;
                impl Port for crate::support::Backend {
                    fn translate(&self, first: String, second: String) {
                        crate::support::bws_backend::translate(second, first)
                    }
                }
            "#,
            r#"
                use crate::ports::Port;
                impl Port for crate::support::Backend {
                    fn translate(&self, value: String) {
                        crate::support::bws_backend::translate(crate::support::Backend::default(), value)
                    }
                }
            "#,
        ] {
            let syntax = syn::parse_file(source)?;
            let mut violations = Vec::new();
            collect_adapter_item_violations(
                &syntax.items,
                false,
                std::path::Path::new("adapter.rs"),
                &mut violations,
            );
            assert_eq!(violations.len(), 1, "source: {source}");
        }
        Ok(())
    }

    #[test]
    fn support_ast_gate_rejects_adapter_import_and_path_include() -> TestResult {
        let source = r#"
            use crate::adapters::Backend;
            use crate::adapter_bw as real_backend;
            use crate::support::adapter_backend::BwsClientBackend;
            #[path = "../adapters/backend.rs"]
            mod forbidden;
            include!("../adapter_yubikey.rs");
        "#;
        let syntax = syn::parse_file(source)?;
        let path = std::path::Path::new("support.rs");
        let mut violations = Vec::new();
        collect_support_to_adapter_violations(&syntax.items, path, &mut violations);
        assert_eq!(violations.len(), 4);
        Ok(())
    }

    /// 実 repository source も同じ AST rule を満たすことを、Cargo/Nix/full check を起動せず確認する。
    #[test]
    fn adapter_ast_gate_accepts_repository_sources() -> TestResult {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .ok_or_else(|| anyhow::anyhow!("workspace root must be an ancestor of checks crate"))?;
        let feature_root = root.join("rust/dotfiles-secrets/src/features");
        let adapter_files = feature_layer_files(&feature_root, "adapters")?;
        let support_files = feature_layer_files(&feature_root, "support")?;
        let mut violations = Vec::new();

        for path in adapter_files {
            let source = fs::read_to_string(&path)?;
            let syntax = syn::parse_file(&source)?;
            collect_adapter_item_violations(&syntax.items, false, &path, &mut violations);
        }
        for path in support_files {
            let source = fs::read_to_string(&path)?;
            let syntax = syn::parse_file(&source)?;
            collect_support_to_adapter_violations(&syntax.items, &path, &mut violations);
        }

        assert!(violations.is_empty(), "{}", violations.join("\n"));
        Ok(())
    }

    #[test]
    fn nix_flake_source_preflight_rejects_untracked_cargo_and_nix_sources() -> TestResult {
        let repo = unique_temp_dir("dotfiles-nix-source-preflight")?;
        fs::create_dir_all(repo.join("rust/dotfiles-cli/src/bin"))?;
        fs::create_dir_all(repo.join("nix"))?;
        fs::create_dir_all(repo.join("docs"))?;
        fs::write(repo.join("rust/dotfiles-cli/src/lib.rs"), "// tracked\n")?;
        fs::write(repo.join("docs/notes.md"), "local note\n")?;

        git(&repo, ["init"])?;
        git(&repo, ["add", "rust/dotfiles-cli/src/lib.rs"])?;

        fs::write(
            repo.join("rust/dotfiles-cli/src/bin/new.rs"),
            "fn main() {}\n",
        )?;
        fs::write(repo.join("rust/dotfiles-cli/Cargo.toml"), "[package]\n")?;
        fs::write(repo.join("nix/new-module.nix"), "{}\n")?;

        let candidates = untracked_nix_source_candidates(&repo)?;
        assert_eq!(
            candidates,
            vec![
                std::path::PathBuf::from("nix/new-module.nix"),
                std::path::PathBuf::from("rust/dotfiles-cli/Cargo.toml"),
                std::path::PathBuf::from("rust/dotfiles-cli/src/bin/new.rs"),
            ]
        );
        assert!(ensure_nix_source_candidates_are_tracked(&candidates).is_err());

        let _ = fs::remove_dir_all(repo);
        Ok(())
    }

    #[test]
    fn nix_flake_source_preflight_accepts_tracked_sources_and_ignores_local_notes() -> TestResult {
        let repo = unique_temp_dir("dotfiles-nix-source-preflight-tracked")?;
        fs::create_dir_all(repo.join("rust/dotfiles-cli/src"))?;
        fs::create_dir_all(repo.join("nix"))?;
        fs::create_dir_all(repo.join("docs"))?;
        fs::write(repo.join("rust/dotfiles-cli/src/lib.rs"), "// source\n")?;
        fs::write(repo.join("rust/dotfiles-cli/Cargo.toml"), "[package]\n")?;
        fs::write(repo.join("nix/module.nix"), "{}\n")?;
        fs::write(repo.join("docs/local-note.md"), "not a Nix source\n")?;

        git(&repo, ["init"])?;
        git(
            &repo,
            [
                "add",
                "rust/dotfiles-cli/src/lib.rs",
                "rust/dotfiles-cli/Cargo.toml",
                "nix/module.nix",
            ],
        )?;

        let candidates = untracked_nix_source_candidates(&repo)?;
        assert!(candidates.is_empty());
        assert!(ensure_nix_source_candidates_are_tracked(&candidates).is_ok());

        let _ = fs::remove_dir_all(repo);
        Ok(())
    }

    type TestResult = anyhow::Result<()>;

    fn git<const N: usize>(repo: &Path, args: [&str; N]) -> TestResult {
        let status = Command::new("git").current_dir(repo).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("git command failed: {args:?}")).into())
        }
    }

    fn unique_temp_dir(prefix: &str) -> anyhow::Result<std::path::PathBuf> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| anyhow::anyhow!("system time before unix epoch: {error}"))?
            .as_nanos();
        Ok(env::temp_dir().join(format!("{prefix}-{}-{suffix}", process::id())))
    }
}
