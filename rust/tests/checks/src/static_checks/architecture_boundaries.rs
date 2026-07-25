//! Feature-first architecture boundary enforcement.
//!
//! The checker deliberately parses every owned Rust source with `syn`.  A
//! missing manifest entry, an unreadable source, or an AST parse failure is a
//! failure: there is no text-search fallback and no warning-only mode.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail, ensure};
use serde::Deserialize;
use syn::{Expr, Item, Path as SynPath, UseTree, visit::Visit};
use xshell::Shell;

use crate::Result;

const MANIFEST: &str = "rust/dotfiles-secrets/architecture-boundaries.v1.json";
const SOURCE_ROOT: &str = "rust/dotfiles-secrets/src";

#[derive(Debug, Deserialize)]
struct Manifest {
    schema: String,
    #[serde(rename = "crate")]
    crate_name: String,
    source_root: String,
    checker: String,
    ci_required_check: String,
    owners: Vec<Owner>,
    public_contracts: Vec<PublicContract>,
    exclusions: Vec<Exclusion>,
    bootstrap: Bootstrap,
}

#[derive(Debug, Deserialize)]
struct Owner {
    path_prefix: String,
    kind: String,
    feature: Option<String>,
    layer: String,
    allow_layers: Vec<String>,
    allow_external_crates: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PublicContract {
    stable_identifier: String,
    version: String,
    owner_feature: String,
    module_path: String,
    registered_consumers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Exclusion {
    path_prefix: String,
    reason: String,
    owner: String,
    expiry: String,
}

/// Manifest-owned, exact root-route contract.  This deliberately has no
/// wildcard callers: `pub(crate)` cannot express root-only access.
#[derive(Debug, Deserialize)]
struct Bootstrap {
    root_entry: RootEntry,
    bootstrap_module: BootstrapModule,
    entrypoint_starts: Vec<EntrypointStart>,
    allowed_direct_edges: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RootEntry {
    module_path: String,
    source_path: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct BootstrapModule {
    module_path: String,
    source_path: String,
    start_symbol: String,
}

#[derive(Debug, Deserialize)]
struct EntrypointStart {
    feature: String,
    module_path: String,
    start_symbol: String,
    allowed_direct_callers: Vec<String>,
    allowed_importers: Vec<String>,
    allowed_reexporters: Vec<String>,
    invocation_boundary_owner: String,
    concrete_public_export: bool,
}

/// Run the manifest-backed AST check used by the required static-check route.
pub(super) fn check(shell: &Shell) -> Result<()> {
    crate::command::step("feature architecture boundary");
    let root = shell.current_dir();
    let manifest_path = root.join(MANIFEST);
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("cannot parse {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;

    let source_root = root.join(&manifest.source_root);
    ensure!(
        source_root == root.join(SOURCE_ROOT),
        "rule=manifest-source-root path={} owner=manifest edge=source-root remediation=use the canonical dotfiles-secrets src root",
        manifest.source_root
    );
    validate_public_contract_modules(&manifest, &source_root)?;
    validate_bootstrap_route(&manifest, &root)?;

    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files)?;
    let mut violations = Vec::new();
    for source in files {
        let relative = source
            .strip_prefix(&source_root)
            .context("source escaped manifest root")?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(exclusion) = manifest
            .exclusions
            .iter()
            .find(|entry| path_matches(&relative, &entry.path_prefix))
        {
            if exclusion.reason.is_empty()
                || exclusion.owner.is_empty()
                || exclusion.expiry.is_empty()
            {
                violations.push(message(
                    "exclusion-metadata",
                    &relative,
                    "excluded",
                    "manifest",
                    "give every exclusion a reason, owner, and expiry",
                ));
            }
            continue;
        }
        let Some(owner) = owner_for(&manifest.owners, &relative) else {
            violations.push(message(
                "unknown-source",
                &relative,
                "unknown",
                "source->owner",
                "add exactly one owner entry or a justified exclusion",
            ));
            continue;
        };
        let source_text = match fs::read_to_string(&source) {
            Ok(value) => value,
            Err(error) => {
                violations.push(message(
                    "source-read",
                    &relative,
                    &owner.layer,
                    "source->ast",
                    &format!("make the source readable: {error}"),
                ));
                continue;
            }
        };
        let syntax = match syn::parse_file(&source_text) {
            Ok(value) => value,
            Err(error) => {
                violations.push(message(
                    "parse-failure",
                    &relative,
                    &owner.layer,
                    "source->ast",
                    &format!("fix Rust syntax: {error}"),
                ));
                continue;
            }
        };
        let mut imports = Vec::new();
        collect_imports(&syntax.items, &mut imports);
        for import in imports {
            inspect_import(
                &relative,
                owner,
                &manifest.public_contracts,
                &import,
                &mut violations,
            );
        }
        let mut paths = PathUseCollector::default();
        paths.visit_file(&syntax);
        for path in paths.paths {
            if path.first().is_some_and(|segment| segment == "crate") {
                inspect_crate_import(
                    &relative,
                    owner,
                    &manifest.public_contracts,
                    &path[1..],
                    &mut violations,
                );
            }
            inspect_external_path_use(&relative, owner, &path, &mut violations);
        }
    }
    if !violations.is_empty() {
        bail!(violations.join("\n"));
    }
    Ok(())
}

/// `use` だけでなく fully-qualified path で現れる external crate use を収集する。
/// AST が path を解決できない時に allow へ倒さず、workspace が明示的に許可する crate root だけを
/// owner layer と照合する。local binding と区別できない未知 root はこの rule の対象外であり、
/// ownership/import rule が別途 fail-closed に扱う。
#[derive(Default)]
struct PathUseCollector {
    paths: Vec<Vec<String>>,
}
impl<'ast> Visit<'ast> for PathUseCollector {
    fn visit_path(&mut self, path: &'ast SynPath) {
        self.paths.push(
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        );
        syn::visit::visit_path(self, path);
    }
}

const EXTERNAL_CRATE_ROOTS: &[&str] = &[
    "aes_gcm",
    "anyhow",
    "bincode",
    "bitwarden",
    "clap",
    "crossterm",
    "filedescriptor",
    "git2",
    "gpgme",
    "mockall",
    "pcsc",
    "rand",
    "rand_chacha",
    "rand_core",
    "region",
    "rlimit",
    "rsa",
    "scopeguard",
    "sequoia_openpgp",
    "serde",
    "serde_json",
    "sha2",
    "tokio",
    "uuid",
    "yubikey",
    "zeroize",
];

fn inspect_external_path_use(
    relative: &str,
    owner: &Owner,
    path: &[String],
    violations: &mut Vec<String>,
) {
    let Some(first) = path.first() else {
        return;
    };
    if EXTERNAL_CRATE_ROOTS.contains(&first.as_str())
        && !owner
            .allow_external_crates
            .iter()
            .any(|allowed| allowed == first)
    {
        violations.push(message("external-crate-owner", relative, &owner.layer, first, "move the external crate use to an allowed owner layer or register a justified owner rule"));
    }
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    ensure!(
        manifest.schema == "architecture-boundaries/v1",
        "rule=manifest-schema path={MANIFEST} owner=manifest edge=schema remediation=use architecture-boundaries/v1"
    );
    ensure!(
        manifest.crate_name == "dotfiles-secrets",
        "rule=manifest-crate path={MANIFEST} owner=manifest edge=crate remediation=declare dotfiles-secrets"
    );
    ensure!(
        !manifest.checker.is_empty() && !manifest.ci_required_check.is_empty(),
        "rule=manifest-consumer path={MANIFEST} owner=manifest edge=checker->ci remediation=declare the developer command and required CI consumer"
    );
    ensure!(
        !manifest.owners.is_empty(),
        "rule=manifest-owner path={MANIFEST} owner=manifest edge=source->owner remediation=register source owners"
    );
    ensure!(
        !manifest.public_contracts.is_empty(),
        "rule=public-contract-registry path={MANIFEST} owner=manifest edge=contract->consumer remediation=register every cross-feature public contract"
    );
    for contract in &manifest.public_contracts {
        ensure!(
            !contract.stable_identifier.is_empty()
                && !contract.version.is_empty()
                && !contract.owner_feature.is_empty()
                && !contract.module_path.is_empty()
                && !contract.registered_consumers.is_empty(),
            "rule=public-contract-registry path={MANIFEST} owner=manifest edge=contract->consumer remediation=register identifier, version, owner, module path, and consumers"
        );
    }
    validate_bootstrap_manifest(&manifest.bootstrap)?;
    Ok(())
}

fn validate_bootstrap_manifest(bootstrap: &Bootstrap) -> Result<()> {
    ensure!(
        bootstrap.root_entry.module_path == "crate"
            && bootstrap.root_entry.source_path == "rust/dotfiles-secrets/src/lib.rs"
            && bootstrap.root_entry.symbol == "run",
        "rule=bootstrap-root-entry path={MANIFEST} owner=manifest edge=root-entry remediation=declare crate::run in dotfiles-secrets/src/lib.rs"
    );
    ensure!(
        bootstrap.bootstrap_module.module_path == "crate::composition::bootstrap"
            && bootstrap.bootstrap_module.source_path
                == "rust/dotfiles-secrets/src/composition/bootstrap.rs"
            && bootstrap.bootstrap_module.start_symbol == "start",
        "rule=bootstrap-module path={MANIFEST} owner=manifest edge=bootstrap-module remediation=declare crate::composition::bootstrap::start"
    );
    ensure!(
        bootstrap.allowed_direct_edges
            == [
                "crate::run->crate::composition::bootstrap::start",
                "crate::composition::bootstrap::start->crate::features::command_facade::entrypoint::start",
            ],
        "rule=bootstrap-direct-edges path={MANIFEST} owner=manifest edge=root-route remediation=declare the exact two-edge root route"
    );
    ensure!(
        bootstrap.entrypoint_starts.len() == 1,
        "rule=bootstrap-entrypoint-count path={MANIFEST} owner=manifest edge=entrypoint remediation=declare exactly one root-started entrypoint"
    );
    let entrypoint = &bootstrap.entrypoint_starts[0];
    ensure!(
        entrypoint.feature == "command_facade"
            && entrypoint.module_path == "crate::features::command_facade::entrypoint"
            && entrypoint.start_symbol == "start"
            && entrypoint.allowed_direct_callers == ["crate::composition::bootstrap"]
            && entrypoint.allowed_importers == ["crate::composition::bootstrap"]
            && entrypoint.allowed_reexporters.is_empty()
            && entrypoint.invocation_boundary_owner == "entrypoint"
            && !entrypoint.concrete_public_export,
        "rule=bootstrap-entrypoint-contract path={MANIFEST} owner=manifest edge=entrypoint remediation=declare command_facade entrypoint start with exact root-only allowlists"
    );
    Ok(())
}

/// Check the two executable root edges and reject every other direct import,
/// call, or re-export of `command_facade::entrypoint::start`.  The route uses
/// AST nodes, not text matches, so comments and string literals cannot make a
/// forbidden edge appear valid.
fn validate_bootstrap_route(manifest: &Manifest, root: &Path) -> Result<()> {
    let root_entry = root.join(&manifest.bootstrap.root_entry.source_path);
    let bootstrap = root.join(&manifest.bootstrap.bootstrap_module.source_path);
    let root_syntax = parse_source(&root_entry)?;
    let bootstrap_syntax = parse_source(&bootstrap)?;

    ensure!(
        named_function_call_count(&root_syntax, "run", &["composition", "bootstrap", "start"]) == 1,
        "rule=bootstrap-root-call path={} owner=root edge=crate::run->crate::composition::bootstrap::start remediation=make run call bootstrap::start exactly once",
        manifest.bootstrap.root_entry.source_path
    );
    ensure!(
        named_function_call_count(&bootstrap_syntax, "start", &["entrypoint", "start"]) == 1,
        "rule=bootstrap-entrypoint-call path={} owner=composition edge=bootstrap.start->command_facade.entrypoint.start remediation=make bootstrap::start call entrypoint::start exactly once",
        manifest.bootstrap.bootstrap_module.source_path
    );
    for (function, calls) in named_function_calls(&bootstrap_syntax) {
        if function != "start"
            && calls
                .iter()
                .any(|call| ends_with_path(call, &["entrypoint", "start"]))
        {
            bail!(
                "rule=bootstrap-wrapper path={} owner=composition edge={function}->entrypoint::start remediation=keep the entrypoint call directly inside bootstrap::start",
                manifest.bootstrap.bootstrap_module.source_path
            );
        }
    }

    let source_root = root.join(&manifest.source_root);
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files)?;
    let bootstrap_relative = manifest
        .bootstrap
        .bootstrap_module
        .source_path
        .strip_prefix("rust/dotfiles-secrets/src/")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "rule=bootstrap-module-source-path path={MANIFEST} owner=manifest edge=bootstrap-module remediation=declare the canonical source path below rust/dotfiles-secrets/src/"
            )
        })?;
    for path in files {
        let relative = path
            .strip_prefix(&source_root)
            .context("source escaped manifest root")?
            .to_string_lossy()
            .replace('\\', "/");
        let syntax = parse_source(&path)?;
        let calls = call_paths(&syntax);
        let imports = imports_with_visibility(&syntax.items);
        let is_bootstrap = relative == bootstrap_relative;
        for call in calls {
            if ends_with_path(&call, &["entrypoint", "start"]) && !is_bootstrap {
                bail!(
                    "rule=bootstrap-unknown-caller path={relative} owner=source edge={} remediation=only crate::composition::bootstrap may call command_facade entrypoint start",
                    call.join("::")
                );
            }
        }
        for (path, is_public) in imports {
            if path_contains(&path, &["features", "command_facade", "entrypoint"])
                && (!is_bootstrap || is_public)
            {
                bail!(
                    "rule=bootstrap-import-or-reexport path={relative} owner=source edge={} remediation=only private root-bootstrap import of entrypoint start is allowed",
                    path.join("::")
                );
            }
        }
    }
    Ok(())
}

fn parse_source(path: &Path) -> Result<syn::File> {
    let source =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    syn::parse_file(&source).with_context(|| format!("cannot parse {}", path.display()))
}

fn named_function_call_count(file: &syn::File, name: &str, target_suffix: &[&str]) -> usize {
    named_function_calls(file)
        .into_iter()
        .filter(|(function, _)| function == name)
        .map(|(_, calls)| {
            calls
                .into_iter()
                .filter(|path| ends_with_path(path, target_suffix))
                .count()
        })
        .sum()
}

fn named_function_calls(file: &syn::File) -> Vec<(String, Vec<Vec<String>>)> {
    file.items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) => Some((
                function.sig.ident.to_string(),
                call_paths_in_block(&function.block),
            )),
            _ => None,
        })
        .collect()
}

fn call_paths(file: &syn::File) -> Vec<Vec<String>> {
    let mut collector = CallCollector::default();
    collector.visit_file(file);
    collector.calls
}

fn call_paths_in_block(block: &syn::Block) -> Vec<Vec<String>> {
    let mut collector = CallCollector::default();
    collector.visit_block(block);
    collector.calls
}

#[derive(Default)]
struct CallCollector {
    calls: Vec<Vec<String>>,
}
impl<'ast> Visit<'ast> for CallCollector {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = call.func.as_ref() {
            self.calls.push(
                path.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect(),
            );
        }
        syn::visit::visit_expr_call(self, call);
    }
}

fn ends_with_path(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn path_contains(path: &[String], needle: &[&str]) -> bool {
    path.windows(needle.len())
        .any(|window| window.iter().map(String::as_str).eq(needle.iter().copied()))
}

fn imports_with_visibility(items: &[Item]) -> Vec<(Vec<String>, bool)> {
    let mut imports = Vec::new();
    for item in items {
        match item {
            Item::Use(item_use) => {
                let mut paths = Vec::new();
                collect_use_tree(&item_use.tree, Vec::new(), &mut paths);
                imports.extend(
                    paths
                        .into_iter()
                        .map(|path| (path, !matches!(item_use.vis, syn::Visibility::Inherited))),
                );
            }
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    imports.extend(imports_with_visibility(nested));
                }
            }
            _ => {}
        }
    }
    imports
}

fn validate_public_contract_modules(manifest: &Manifest, source_root: &Path) -> Result<()> {
    for contract in &manifest.public_contracts {
        ensure!(
            contract.module_path == format!("features/{}/ports/public", contract.owner_feature),
            "rule=public-contract-module path={} owner={} edge=contract->module remediation=use the owner feature ports/public module",
            contract.module_path,
            contract.owner_feature,
        );
        let module = source_root.join(format!("{}.rs", contract.module_path));
        ensure!(
            module.is_file(),
            "rule=public-contract-module path={} owner={} edge=contract->module remediation=create the registered ports/public module",
            contract.module_path,
            contract.owner_feature,
        );
    }
    Ok(())
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("cannot enumerate {}", directory.display()))?
    {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn owner_for<'a>(owners: &'a [Owner], relative: &str) -> Option<&'a Owner> {
    let matches = owners
        .iter()
        .filter(|owner| path_matches(relative, &owner.path_prefix))
        .collect::<Vec<_>>();
    let longest = matches.iter().map(|owner| owner.path_prefix.len()).max()?;
    let most_specific = matches
        .into_iter()
        .filter(|owner| owner.path_prefix.len() == longest)
        .collect::<Vec<_>>();
    (most_specific.len() == 1).then_some(most_specific[0])
}

fn path_matches(path: &str, prefix: &str) -> bool {
    prefix.is_empty() || path == prefix || path.starts_with(prefix)
}

fn collect_imports(items: &[Item], imports: &mut Vec<Vec<String>>) {
    for item in items {
        match item {
            Item::Use(item_use) => collect_use_tree(&item_use.tree, Vec::new(), imports),
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_imports(nested, imports);
                }
            }
            _ => {}
        }
    }
}

fn collect_use_tree(tree: &UseTree, mut prefix: Vec<String>, imports: &mut Vec<Vec<String>>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use_tree(path.tree.as_ref(), prefix, imports);
        }
        UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            imports.push(prefix);
        }
        UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            imports.push(prefix);
        }
        UseTree::Glob(_) => imports.push(prefix),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(item, prefix.clone(), imports);
            }
        }
    }
}

fn inspect_import(
    relative: &str,
    owner: &Owner,
    public_contracts: &[PublicContract],
    path: &[String],
    violations: &mut Vec<String>,
) {
    let Some(first) = path.first() else {
        return;
    };
    if first == "crate" {
        inspect_crate_import(relative, owner, public_contracts, &path[1..], violations);
        return;
    }
    if matches!(first.as_str(), "self" | "super") {
        if first == "super" && path.get(1).is_some_and(|segment| segment == "super") {
            violations.push(message(
                "relative-layer-escape",
                relative,
                &owner.layer,
                "super::super",
                "import the owned layer through its explicit feature path",
            ));
        }
        return;
    }
    if matches!(first.as_str(), "std" | "core" | "alloc") {
        return;
    }
    let allowed = owner
        .allow_external_crates
        .iter()
        .any(|allowed| allowed == first);
    if !allowed {
        violations.push(message("external-crate-owner", relative, &owner.layer, first, "move the external crate use to an allowed owner layer or register a justified owner rule"));
    }
}

fn inspect_crate_import(
    relative: &str,
    owner: &Owner,
    public_contracts: &[PublicContract],
    path: &[String],
    violations: &mut Vec<String>,
) {
    let Some(first) = path.first() else {
        return;
    };
    if first == "Result" || first == "secrets_internal_test_stub_contract" {
        return;
    }
    if first == "features" || first == "composition" {
        if first == "features" {
            inspect_feature_import(relative, owner, public_contracts, &path[1..], violations);
        } else if owner.kind != "composition" && owner.kind != "root" {
            violations.push(message(
                "composition-import",
                relative,
                &owner.layer,
                "crate::composition",
                "only root composition may import composition wiring",
            ));
        }
        return;
    }
    if first == "foundation" || first == "shared" {
        if owner.kind.as_str() != first
            && !owner.allow_layers.iter().any(|allowed| allowed == first)
        {
            violations.push(message(
                "horizontal-layer-import",
                relative,
                &owner.layer,
                first,
                "declare the dependency in the owner layer policy or move the responsibility",
            ));
        }
        return;
    }
    if matches!(
        first.as_str(),
        "application" | "domain" | "ports" | "adapters" | "presentation" | "support" | "entrypoint"
    ) {
        violations.push(message("root-layer-escape", relative, &owner.layer, &format!("crate::{first}"), "use crate::features::<feature>::<layer> explicitly; root compatibility imports are forbidden"));
    }
}

fn inspect_feature_import(
    relative: &str,
    owner: &Owner,
    public_contracts: &[PublicContract],
    path: &[String],
    violations: &mut Vec<String>,
) {
    if path.len() < 2 {
        violations.push(message(
            "feature-path",
            relative,
            &owner.layer,
            "crate::features",
            "import a concrete feature public contract or owned layer",
        ));
        return;
    }
    let feature = &path[0];
    let layer = &path[1];
    if owner.feature.as_deref() != Some(feature.as_str()) {
        if owner.kind == "composition"
            && feature == "command_facade"
            && matches!(layer.as_str(), "composition" | "entrypoint")
        {
            return;
        }
        if owner.layer == "composition"
            && matches!(layer.as_str(), "adapters" | "presentation" | "support")
        {
            return;
        }
        if layer != "ports" || path.get(2).map(String::as_str) != Some("public") {
            violations.push(message(
                "cross-feature-private-import",
                relative,
                &owner.layer,
                &format!("{feature}::{layer}"),
                "depend only on the target feature's registered ports::public contract",
            ));
        } else if let Some(consumer) = owner.feature.as_deref() {
            let registered = public_contracts.iter().any(|contract| {
                contract.owner_feature == *feature
                    && contract
                        .registered_consumers
                        .iter()
                        .any(|entry| entry == consumer)
            });
            if !registered {
                violations.push(message(
                    "unregistered-public-contract-consumer",
                    relative,
                    &owner.layer,
                    &format!("{feature}::ports::public"),
                    "register this consumer in the target feature public-contract manifest entry",
                ));
            }
        }
        return;
    }
    if owner.layer == "ports" && path.get(2).map(String::as_str) == Some("public") {
        return;
    }
    if layer == "adapters" || layer == "support" || layer == "composition" || layer == "entrypoint"
    {
        let public_contract_implementation = owner.layer == "ports"
            && relative.ends_with("/ports/public.rs")
            && owner.allow_layers.iter().any(|allowed| allowed == layer);
        if owner.layer != "composition" && owner.layer != *layer && !public_contract_implementation
        {
            violations.push(message(
                "horizontal-private-import",
                relative,
                &owner.layer,
                layer,
                "move the dependency behind a port or into composition",
            ));
        }
        return;
    }
    if layer != &owner.layer && !owner.allow_layers.iter().any(|allowed| allowed == layer) {
        violations.push(message(
            "horizontal-layer-import",
            relative,
            &owner.layer,
            layer,
            "move responsibility to an allowed layer or introduce a port contract",
        ));
    }
}

fn message(rule: &str, path: &str, owner: &str, edge: &str, remediation: &str) -> String {
    format!("rule={rule} path={path} owner={owner} edge={edge} remediation={remediation}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_selects_one_owner() {
        let owners = vec![
            Owner {
                path_prefix: String::new(),
                kind: "root".into(),
                feature: None,
                layer: "root".into(),
                allow_layers: vec![],
                allow_external_crates: vec![],
            },
            Owner {
                path_prefix: "features/f/".into(),
                kind: "feature".into(),
                feature: Some("f".into()),
                layer: "domain".into(),
                allow_layers: vec![],
                allow_external_crates: vec![],
            },
        ];
        assert_eq!(
            owner_for(&owners, "features/f/domain/value.rs").map(|owner| owner.layer.as_str()),
            Some("domain")
        );
    }

    #[test]
    fn private_cross_feature_import_is_rejected() {
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("a".into()),
            layer: "application".into(),
            allow_layers: vec!["domain".into()],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        inspect_feature_import(
            "features/a/application/x.rs",
            &owner,
            &[],
            &["b".into(), "domain".into()],
            &mut violations,
        );
        assert!(violations[0].contains("cross-feature-private-import"));
    }

    #[test]
    fn unregistered_public_contract_consumer_is_rejected() {
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("a".into()),
            layer: "application".into(),
            allow_layers: vec!["ports".into()],
            allow_external_crates: vec![],
        };
        let contracts = vec![PublicContract {
            stable_identifier: "b/capability".into(),
            version: "v1".into(),
            owner_feature: "b".into(),
            module_path: "features/b/ports/public".into(),
            registered_consumers: vec!["c".into()],
        }];
        let mut violations = Vec::new();
        inspect_feature_import(
            "features/a/application/x.rs",
            &owner,
            &contracts,
            &["b".into(), "ports".into(), "public".into()],
            &mut violations,
        );
        assert!(
            violations
                .iter()
                .any(|entry| entry.contains("unregistered-public-contract-consumer"))
        );
    }

    #[test]
    fn bootstrap_manifest_rejects_a_third_or_old_seam_edge() {
        let bootstrap = Bootstrap {
            root_entry: RootEntry { module_path: "crate".into(), source_path: "rust/dotfiles-secrets/src/lib.rs".into(), symbol: "run".into() },
            bootstrap_module: BootstrapModule { module_path: "crate::composition::bootstrap".into(), source_path: "rust/dotfiles-secrets/src/composition/bootstrap.rs".into(), start_symbol: "start".into() },
            entrypoint_starts: vec![EntrypointStart {
                feature: "command_facade".into(), module_path: "crate::features::command_facade::entrypoint".into(), start_symbol: "start".into(),
                allowed_direct_callers: vec!["crate::composition::bootstrap".into()], allowed_importers: vec!["crate::composition::bootstrap".into()], allowed_reexporters: vec![], invocation_boundary_owner: "entrypoint".into(), concrete_public_export: false,
            }],
            allowed_direct_edges: vec![
                "crate::run->crate::composition::bootstrap::start".into(),
                "crate::composition::bootstrap::start->crate::features::command_facade::entrypoint::start".into(),
                "crate::composition::bootstrap::start->compose_for_root".into(),
            ],
        };
        assert!(validate_bootstrap_manifest(&bootstrap).is_err());
    }

    #[test]
    fn command_facade_entrypoint_path_is_detected_even_when_not_a_direct_symbol_import() {
        assert!(path_contains(
            &[
                "crate".into(),
                "features".into(),
                "command_facade".into(),
                "entrypoint".into(),
                "SecretsInvocation".into()
            ],
            &["features", "command_facade", "entrypoint"],
        ));
    }

    #[test]
    fn suffix_match_does_not_accept_a_wrapper_named_start() {
        assert!(ends_with_path(
            &["entrypoint".into(), "start".into()],
            &["entrypoint", "start"],
        ));
        assert!(!ends_with_path(
            &["wrapper".into(), "start".into()],
            &["entrypoint", "start"],
        ));
    }
}
