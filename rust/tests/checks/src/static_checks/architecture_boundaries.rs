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
    #[serde(default)]
    allow_boundary_layers: Vec<String>,
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
    #[serde(default)]
    root_routes: Vec<RootRoute>,
    entrypoint_starts: Vec<EntrypointStart>,
    allowed_direct_edges: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RootRoute {
    entry_symbol: String,
    bootstrap_symbol: String,
    target_symbol: String,
    entry_source_path: String,
    bootstrap_source_path: String,
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
    let repository_root = repository_root(root.as_path(), MANIFEST)?;
    check_at(shell, repository_root.as_path(), MANIFEST, SOURCE_ROOT)
}

fn repository_root(current: &Path, manifest_relative: &str) -> Result<PathBuf> {
    let mut root = current.to_path_buf();
    loop {
        if root.join(manifest_relative).exists() {
            return Ok(root);
        }
        let Some(parent) = root.parent() else {
            bail!(
                "rule=fixture-path-resolution path={manifest_relative} owner=fixture harness edge=repo-root remediation=run this check from a directory that resolves to the repository root containing rust/dotfiles-secrets/{path}",
                path = manifest_relative
            );
        };
        root = parent.to_path_buf();
    }
}

/// Same checker entry used by CI, with explicit paths for fixture-root tests.
/// The default `check` above remains the production invocation.
pub(super) fn check_at(
    _shell: &Shell,
    root: &Path,
    manifest_relative: &str,
    source_relative: &str,
) -> Result<()> {
    let repository_root = repository_root(root, manifest_relative)?;
    let manifest_path = repository_root.join(manifest_relative);
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("cannot parse {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;

    let source_root = repository_root.join(source_relative);
    ensure!(
        source_root == root.join(source_relative),
        "rule=manifest-source-root path={} owner=manifest edge=source-root remediation=use the canonical dotfiles-secrets src root",
        manifest.source_root
    );
    validate_public_contract_modules(&manifest, &source_root)?;
    validate_bootstrap_route(&manifest, root)?;

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
        let local_roots = local_path_roots(&syntax.items);
        for import in imports {
            inspect_import(
                &relative,
                owner,
                &manifest.public_contracts,
                &import,
                &local_roots,
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
            inspect_external_path_use(&relative, owner, &path, &local_roots, &mut violations);
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
    // `use` trees are handled by `collect_imports`/`inspect_import`, where
    // crate-relative paths and aliases have their lexical import meaning.
    // syn's generic visitor can otherwise expose nested use-tree segments as
    // standalone paths (for example `yubikey_lifecycle`), which are not
    // qualified SDK expressions and must not be classified as external crates.
    fn visit_item_use(&mut self, _item: &'ast syn::ItemUse) {}

    fn visit_path(&mut self, path: &'ast SynPath) {
        // A local absolute path must be treated as one lexical path.  The
        // generic visitor otherwise descends into the same path and exposes
        // every nested feature/module segment as a new root (for example
        // `crate::features::...::yubikey_lifecycle::ports::public`).  That
        // makes a local contract path look like an external SDK crate.
        // `use` paths are checked separately by `collect_imports`.
        let local_absolute = path.leading_colon.is_some()
            || path.segments.first().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "crate" | "self" | "super"
                )
            });
        self.paths.push(
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        );
        // Local absolute paths must be emitted as one complete path so the
        // owner/layer checker can inspect `crate::...` and `self::...` uses.
        // Do not descend into their segments: those segments are not
        // independent external-crate roots.  External paths still recurse so
        // nested expressions remain visible to the qualified SDK check.
        if !local_absolute {
            syn::visit::visit_path(self, path);
        }
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
    local_roots: &[String],
    violations: &mut Vec<String>,
) {
    // `PathUseCollector` walks every expression path, including ordinary
    // local bindings such as `let yubikey = ...; ... yubikey ...`.  A
    // qualified external-crate expression has at least one segment after its
    // crate root (`yubikey::Certificate`, `serde_json::from_str`, ...).  A
    // single segment is therefore a lexical value/type name, not evidence of
    // an SDK use, and must not be sent to the external-owner rule.
    if path.len() < 2 {
        return;
    }
    let Some(first) = external_owner_root(path, local_roots) else {
        return;
    };
    if EXTERNAL_CRATE_ROOTS.contains(&first)
        && !owner
            .allow_external_crates
            .iter()
            .any(|allowed| allowed == first)
    {
        violations.push(message("external-crate-owner", relative, &owner.layer, first, "move the external crate use to an allowed owner layer or register a justified owner rule"));
    }
}

fn local_path_roots(items: &[Item]) -> Vec<String> {
    let mut roots = Vec::new();
    collect_local_path_roots(items, &mut roots);
    roots.sort();
    roots.dedup();
    roots
}

fn collect_local_path_roots(items: &[Item], roots: &mut Vec<String>) {
    for item in items {
        if let Item::Mod(module) = item {
            roots.push(module.ident.to_string());
            if let Some((_, nested)) = &module.content {
                collect_local_path_roots(nested, roots);
            }
        }
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

    for route in &manifest.bootstrap.root_routes {
        let entry = parse_source(&root.join(&route.entry_source_path))?;
        let route_bootstrap = parse_source(&root.join(&route.bootstrap_source_path))?;
        ensure!(
            named_function_call_count(
                &entry,
                &route.entry_symbol,
                &["composition", "bootstrap", route.bootstrap_symbol.as_str()]
            ) == 1,
            "rule=bootstrap-root-route path={} owner=root edge={} remediation=make the root route call the declared bootstrap symbol exactly once",
            route.entry_source_path,
            route.entry_symbol
        );
        ensure!(
            named_function_call_count(
                &route_bootstrap,
                &route.bootstrap_symbol,
                &["entrypoint", route.target_symbol.as_str()]
            ) == 1,
            "rule=bootstrap-entry-route path={} owner=composition edge={} remediation=make the bootstrap route call the declared command facade entrypoint exactly once",
            route.bootstrap_source_path,
            route.target_symbol
        );
    }

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
                collect_use_tree(&item_use.tree, &[], &mut paths);
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
    // An empty prefix is a catch-all fallback and makes a missing owner look
    // registered.  Every source must be covered by an explicit exact path or
    // a non-empty directory prefix; unknown and overlapping ownership fail
    // closed in `owner_for`.
    !prefix.is_empty() && (path == prefix || path.starts_with(prefix))
}

fn collect_imports(items: &[Item], imports: &mut Vec<Vec<String>>) {
    for item in items {
        match item {
            Item::Use(item_use) => collect_use_tree(&item_use.tree, &[], imports),
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_imports(nested, imports);
                }
            }
            _ => {}
        }
    }
}

fn collect_use_tree(tree: &UseTree, prefix: &[String], imports: &mut Vec<Vec<String>>) {
    match tree {
        UseTree::Path(path) => {
            let mut child_prefix = prefix.to_vec();
            child_prefix.push(path.ident.to_string());
            collect_use_tree(path.tree.as_ref(), &child_prefix, imports);
        }
        UseTree::Name(name) => {
            let mut import = prefix.to_vec();
            import.push(name.ident.to_string());
            imports.push(import);
        }
        UseTree::Rename(rename) => {
            let mut import = prefix.to_vec();
            import.push(rename.ident.to_string());
            imports.push(import);
        }
        UseTree::Glob(_) => imports.push(prefix.to_vec()),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(item, prefix, imports);
            }
        }
    }
}

fn inspect_import(
    relative: &str,
    owner: &Owner,
    public_contracts: &[PublicContract],
    path: &[String],
    local_roots: &[String],
    violations: &mut Vec<String>,
) {
    let Some(first_path) = path.first() else {
        return;
    };
    // Relative imports are handled only by the local boundary rules below.
    // They must not fall through to the external-crate owner rule, even when
    // a nested use-tree segment happens to have an SDK-like spelling.
    if is_relative_path_root(first_path) {
        if first_path == "crate" {
            inspect_crate_import(relative, owner, public_contracts, &path[1..], violations);
        } else if first_path == "super" && path.get(1).is_some_and(|segment| segment == "super") {
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
    // A feature port module commonly has a local `mod git`/`mod gpg` and
    // re-exports from it.  Resolve those roots in the current lexical module
    // before comparing names against external crate roots.  This does not
    // weaken SDK detection: a real `use git2::...` is not a local root and is
    // still checked against the owner's external-crate allowlist.
    let Some(first) = external_owner_root(path, local_roots) else {
        return;
    };
    if matches!(first, "std" | "core" | "alloc") {
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

fn is_relative_path_root(root: &str) -> bool {
    matches!(root, "crate" | "self" | "super")
}

/// Return the single lexical root eligible for external-crate ownership.
/// Rust-relative roots and locally declared module roots are handled by the
/// local boundary checker and must never become external-crate edges.
fn external_owner_root<'a>(path: &'a [String], local_roots: &[String]) -> Option<&'a str> {
    let first = path.first()?.as_str();
    if is_relative_path_root(first) || local_roots.iter().any(|root| root == first) {
        return None;
    }
    Some(first)
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
        if owner.feature.as_deref() == Some("command_facade") && layer == "application" {
            // The command facade is the registered cross-feature dispatch
            // boundary; it may start an owner application's internal use case
            // path, while all values/ports remain public-contract imports.
            return;
        }
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
    if owner.layer == "public_contract"
        && matches!(
            layer.as_str(),
            "application" | "adapters" | "support" | "presentation" | "composition" | "entrypoint"
        )
    {
        violations.push(message(
            "public-concrete-reexport",
            relative,
            &owner.layer,
            &format!("{feature}::{layer}"),
            "export only an explicitly registered port or value contract from ports/public",
        ));
        return;
    }
    if layer == "adapters" || layer == "support" || layer == "composition" || layer == "entrypoint"
    {
        let public_contract_implementation = owner.layer == "ports"
            && relative.ends_with("/ports/public.rs")
            && owner.allow_layers.iter().any(|allowed| allowed == layer);
        if owner.layer != "composition"
            && owner.layer != *layer
            && !public_contract_implementation
            && !owner.allow_layers.iter().any(|allowed| allowed == layer)
            && !owner
                .allow_boundary_layers
                .iter()
                .any(|allowed| allowed == layer)
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
    if layer != &owner.layer
        && !owner.allow_layers.iter().any(|allowed| allowed == layer)
        && !owner
            .allow_boundary_layers
            .iter()
            .any(|allowed| allowed == layer)
    {
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
                allow_boundary_layers: vec![],
                allow_external_crates: vec![],
            },
            Owner {
                path_prefix: "features/f/".into(),
                kind: "feature".into(),
                feature: Some("f".into()),
                layer: "domain".into(),
                allow_layers: vec![],
                allow_boundary_layers: vec![],
                allow_external_crates: vec![],
            },
        ];
        assert_eq!(
            owner_for(&owners, "features/f/domain/value.rs").map(|owner| owner.layer.as_str()),
            Some("domain")
        );
    }

    #[test]
    fn empty_prefix_never_claims_an_unowned_source() {
        let owners = vec![Owner {
            path_prefix: String::new(),
            kind: "root".into(),
            feature: None,
            layer: "root".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        }];
        assert!(owner_for(&owners, "features/new/domain/value.rs").is_none());
    }

    #[test]
    fn private_cross_feature_import_is_rejected() {
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("a".into()),
            layer: "application".into(),
            allow_layers: vec!["domain".into()],
            allow_boundary_layers: vec![],
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
            allow_boundary_layers: vec![],
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
    fn public_contract_cannot_reexport_application_runtime() {
        let owner = Owner {
            path_prefix: "features/a/ports/public/".into(),
            kind: "feature".into(),
            feature: Some("a".into()),
            layer: "public_contract".into(),
            allow_layers: vec!["application".into(), "domain".into()],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        inspect_feature_import(
            "features/a/ports/public.rs",
            &owner,
            &[],
            &["a".into(), "application".into(), "run".into()],
            &mut violations,
        );
        assert!(
            violations
                .iter()
                .any(|entry| entry.contains("public-concrete-reexport"))
        );
    }

    #[test]
    fn local_backend_module_is_not_misclassified_as_an_external_crate() {
        let source = syn::parse_file(
            "mod git { pub(crate) struct GitClonePort; }\nuse git::GitClonePort;\n",
        )
        .expect("fixture parses");
        let roots = local_path_roots(&source.items);
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("password_store".into()),
            layer: "ports".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        inspect_external_path_use(
            "features/password_store/ports.rs",
            &owner,
            &["git".into(), "GitClonePort".into()],
            &roots,
            &mut violations,
        );
        assert!(violations.is_empty());

        let mut import_violations = Vec::new();
        inspect_import(
            "features/password_store/ports.rs",
            &owner,
            &[],
            &["git".into(), "GitClonePort".into()],
            &roots,
            &mut import_violations,
        );
        assert!(import_violations.is_empty());
    }

    #[test]
    fn use_tree_aliases_are_not_collected_as_qualified_sdk_paths() {
        let source = syn::parse_file(
            "use crate::{features::{yubikey_lifecycle::ports::public::DeviceSerialPort}};\nfn f() { let _ = crate::features::yubikey_lifecycle::ports::public::DeviceSerialPort; let _ = self::local::Thing; let _ = super::parent::Thing; let _ = yubikey::Certificate::from_bytes; let _ = git2::Repository::open; let _ = gpgme::Context::from_protocol; let _ = serde_json::from_str; }\n",
        )
        .expect("fixture parses");
        let mut collector = PathUseCollector::default();
        collector.visit_file(&source);
        assert!(
            collector
                .paths
                .iter()
                .all(|path| path.first().map(String::as_str) != Some("yubikey_lifecycle"))
        );
        for root in ["yubikey", "git2", "gpgme", "serde_json"] {
            assert!(
                collector
                    .paths
                    .iter()
                    .any(|path| path.first().map(String::as_str) == Some(root)),
                "external root {root} must remain observable"
            );
        }
    }

    #[test]
    fn crate_relative_use_tree_never_enters_external_crate_owner_check() {
        let source = syn::parse_file(
            "use crate::{features::{yubikey_lifecycle::ports::public::DeviceSerialPort, gpg_backup_recovery::ports::public::GpgRecipientPort}};\nuse self::yubikey::LocalType;\nuse super::gpgme::LocalType;\n",
        )
        .expect("fixture parses");
        let mut imports = Vec::new();
        collect_imports(&source.items, &mut imports);
        assert!(imports.iter().any(|path| {
            path.iter().map(String::as_str).eq([
                "crate",
                "features",
                "yubikey_lifecycle",
                "ports",
                "public",
                "DeviceSerialPort",
            ])
        }));
        assert!(imports.iter().any(|path| {
            path.iter().map(String::as_str).eq([
                "crate",
                "features",
                "gpg_backup_recovery",
                "ports",
                "public",
                "GpgRecipientPort",
            ])
        }));
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("password_store".into()),
            layer: "application".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        for import in imports {
            inspect_import(
                "features/password_store/application/relative.rs",
                &owner,
                &[],
                &import,
                &[],
                &mut violations,
            );
        }
        assert!(
            violations
                .iter()
                .all(|entry| !entry.contains("external-crate-owner")),
            "relative roots must not be reported as SDK roots: {violations:?}"
        );
    }

    #[test]
    fn crate_qualified_private_support_call_is_rejected() {
        let source = syn::parse_file(
            "fn f() { crate::features::cli_interaction::support::process_io::secret_input_attempt_count(); }",
        )
        .expect("fixture parses");
        let mut collector = PathUseCollector::default();
        collector.visit_file(&source);
        let owner = Owner {
            path_prefix: "features/yubikey_lifecycle/support/".into(),
            kind: "feature".into(),
            feature: Some("yubikey_lifecycle".into()),
            layer: "support".into(),
            allow_layers: vec!["foundation".into()],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        for path in collector.paths {
            if path.first().map(String::as_str) == Some("crate") {
                inspect_crate_import(
                    "features/yubikey_lifecycle/support/internal_stub_yubikey.rs",
                    &owner,
                    &[],
                    &path[1..],
                    &mut violations,
                );
            }
        }
        assert!(
            violations
                .iter()
                .any(|entry| entry.contains("cross-feature-private-import"))
        );
    }

    #[test]
    fn grouped_crate_use_preserves_the_feature_prefix_for_external_owner_checks() {
        let source = syn::parse_file(
            "use crate::{features::{bws_secrets::ports::public::BwsClientPort, yubikey_lifecycle::ports::public::{DeviceSerialPort, SecretStoragePort}}, foundation::protection::ProtectedSecret};\n",
        )
        .expect("fixture parses");
        let mut imports = Vec::new();
        collect_imports(&source.items, &mut imports);

        for expected in [
            &[
                "crate",
                "features",
                "bws_secrets",
                "ports",
                "public",
                "BwsClientPort",
            ][..],
            &[
                "crate",
                "features",
                "yubikey_lifecycle",
                "ports",
                "public",
                "DeviceSerialPort",
            ][..],
            &[
                "crate",
                "features",
                "yubikey_lifecycle",
                "ports",
                "public",
                "SecretStoragePort",
            ][..],
            &["crate", "foundation", "protection", "ProtectedSecret"][..],
        ] {
            assert!(
                imports
                    .iter()
                    .any(|path| path.iter().map(String::as_str).eq(expected.iter().copied())),
                "expected full grouped use path {:?}, got {imports:?}",
                expected,
            );
        }
        assert!(
            imports
                .iter()
                .all(|path| path.first().map(String::as_str) != Some("yubikey")),
            "grouped crate-relative imports must never become a bare SDK root: {imports:?}",
        );
    }

    #[test]
    fn qualified_sdk_path_remains_rejected_for_an_unapproved_layer() {
        let source = syn::parse_file("fn f() { let _ = git2::Repository::open; }\n")
            .expect("fixture parses");
        let roots = local_path_roots(&source.items);
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("password_store".into()),
            layer: "ports".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        inspect_external_path_use(
            "features/password_store/ports/git.rs",
            &owner,
            &["git2".into(), "Repository".into()],
            &roots,
            &mut violations,
        );
        assert!(
            violations
                .iter()
                .any(|entry| entry.contains("external-crate-owner"))
        );
    }

    #[test]
    fn local_binding_named_like_an_external_crate_is_not_an_sdk_path() {
        let source =
            syn::parse_file("fn f(yubikey: u32) { let _serial = yubikey; let _ = yubikey; }\n")
                .expect("fixture parses");
        let roots = local_path_roots(&source.items);
        let mut collector = PathUseCollector::default();
        collector.visit_file(&source);
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("password_store".into()),
            layer: "application".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        for path in collector.paths {
            inspect_external_path_use(
                "features/password_store/application/value.rs",
                &owner,
                &path,
                &roots,
                &mut violations,
            );
        }
        assert!(
            violations.is_empty(),
            "a local one-segment binding must not be reported as an SDK root: {violations:?}"
        );
    }

    #[test]
    fn external_import_is_not_hidden_by_an_external_root_with_a_local_like_name() {
        let source = syn::parse_file("use yubikey::Certificate;\n").expect("fixture parses");
        let roots = local_path_roots(&source.items);
        assert!(roots.is_empty(), "imports are not local declarations");
        let owner = Owner {
            path_prefix: String::new(),
            kind: "feature".into(),
            feature: Some("password_store".into()),
            layer: "ports".into(),
            allow_layers: vec![],
            allow_boundary_layers: vec![],
            allow_external_crates: vec![],
        };
        let mut violations = Vec::new();
        inspect_import(
            "features/password_store/ports/git.rs",
            &owner,
            &[],
            &["yubikey".into(), "Certificate".into()],
            &roots,
            &mut violations,
        );
        assert!(
            violations
                .iter()
                .any(|entry| entry.contains("external-crate-owner")),
            "a real external import must remain visible to the owner check"
        );
    }

    #[test]
    fn bootstrap_manifest_rejects_a_third_or_old_seam_edge() {
        let bootstrap = Bootstrap {
            root_entry: RootEntry { module_path: "crate".into(), source_path: "rust/dotfiles-secrets/src/lib.rs".into(), symbol: "run".into() },
            bootstrap_module: BootstrapModule { module_path: "crate::composition::bootstrap".into(), source_path: "rust/dotfiles-secrets/src/composition/bootstrap.rs".into(), start_symbol: "start".into() },
            root_routes: vec![],
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
    fn fixture_cases_are_routed_through_the_same_check_entry() {
        // Keep the fixture corpus adjacent to the checker and route the real
        // repository through the injectable entry used by fixture harnesses.
        // Individual fixture files are parsed below so the corpus cannot be
        // silently renamed or removed while CI continues using `check`.
        let shell = Shell::new().expect("shell");
        let root = repository_root(&shell.current_dir(), MANIFEST).expect("repository root");
        check_at(&shell, &root, MANIFEST, SOURCE_ROOT).expect("production boundary check");
        for path in [
            "rust/tests/checks/fixtures/architecture-boundaries/src/features/a/application/private_import.rs",
            "rust/tests/checks/fixtures/architecture-boundaries/src/features/a/support/sdk_violation.rs",
            "rust/tests/checks/fixtures/architecture-boundaries/src/features/a/ports/public.rs",
            "rust/tests/checks/fixtures/architecture-boundaries/src/composition/bootstrap.rs",
        ] {
            let source = std::fs::read_to_string(root.join(path)).expect("fixture source");
            syn::parse_file(&source).expect("fixture Rust source parses");
        }
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
