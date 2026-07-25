//! Rust source のドキュメントコメント境界を検査する静的 checker。

use std::{fs, path::Path};

use anyhow::{Context, bail};
use syn::{Expr, Item, Lit, Meta, Visibility, parse_file};
use xshell::Shell;

use crate::Result;

/// feature module の境界コメントを AST で検査する。
/// 対象は dotfiles-secrets の production/test source に限定し、規約正本は
/// `docs/architecture/hexagonal-implementation-rules.md#ドキュメントコメント規則` とする。
pub(crate) fn check(shell: &Shell) -> Result<()> {
    let root = shell.current_dir().join("rust/dotfiles-secrets/src");
    let mut violations = Vec::new();
    for entry in walk(&root)? {
        inspect(&root, &entry, &mut violations)?;
    }
    if violations.is_empty() {
        return Ok(());
    }
    bail!(
        "documentation comment violations:\n{}",
        violations.join("\n")
    );
}

fn walk(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    fn visit(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).context("walking Rust source")? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().is_some_and(|x| x == "rs") {
                files.push(path);
            }
        }
        Ok(())
    }
    visit(root, &mut files)?;
    Ok(files)
}

fn inspect(root: &Path, path: &Path, violations: &mut Vec<String>) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let file = parse_file(&source).with_context(|| format!("parse {}", path.display()))?;
    let relative = path.strip_prefix(root).unwrap_or(path).display();
    let header = source
        .lines()
        .take(8)
        .any(|line| line.trim_start().starts_with("//"));
    if path.components().any(|c| c.as_os_str() == "tests") && !header {
        violations.push(format!("rule=file-header path={relative} line=1"));
    }
    for item in file.items {
        let technical = path.to_string_lossy().contains("support")
            || path.to_string_lossy().contains("adapters");
        let required = matches!(item, Item::Trait(_) | Item::Struct(_) | Item::Enum(_))
            && (path.to_string_lossy().contains("ports/public") || technical);
        if required && !has_doc(&item) {
            violations.push(format!(
                "rule=public-item-doc path={relative} line={}",
                item_line(&source, &item)
            ));
        } else if required && !doc_has_reference(&item) {
            violations.push(format!(
                "rule=doc-reference path={relative} line={}",
                item_line(&source, &item)
            ));
        }
        if let Item::Fn(function) = item {
            let major = path.to_string_lossy().contains("application")
                && function.sig.ident.to_string().starts_with("run_");
            let support = path.to_string_lossy().contains("support")
                && matches!(function.vis, Visibility::Restricted(_));
            if (major || support) && !function.attrs.iter().any(|a| a.path().is_ident("doc")) {
                violations.push(format!(
                    "rule=item-doc path={relative} line={}",
                    source
                        .lines()
                        .position(|line| line.contains(&function.sig.ident.to_string()))
                        .map_or(1, |line| line + 1)
                ));
            } else if support
                && !doc_attribute_text(&function.attrs).contains("docs/")
                && !doc_attribute_text(&function.attrs).contains("https://")
            {
                violations.push(format!(
                    "rule=doc-reference path={relative} line={}",
                    source
                        .lines()
                        .position(|line| line.contains(&function.sig.ident.to_string()))
                        .map_or(1, |line| line + 1)
                ));
            }
        }
    }
    Ok(())
}

fn has_doc(item: &Item) -> bool {
    let attrs = match item {
        Item::Trait(x) => &x.attrs,
        Item::Struct(x) => &x.attrs,
        Item::Enum(x) => &x.attrs,
        _ => return true,
    };
    attrs.iter().any(|a| a.path().is_ident("doc"))
}

fn doc_has_reference(item: &Item) -> bool {
    let attrs = match item {
        Item::Trait(x) => &x.attrs,
        Item::Struct(x) => &x.attrs,
        Item::Enum(x) => &x.attrs,
        _ => return true,
    };
    let text = doc_attribute_text(attrs);
    text.contains("docs/") || text.contains("https://")
}

fn doc_attribute_text(attributes: &[syn::Attribute]) -> String {
    attributes
        .iter()
        .filter_map(|attribute| match &attribute.meta {
            Meta::NameValue(value) if value.path.is_ident("doc") => match &value.value {
                Expr::Lit(expr) => match &expr.lit {
                    Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::doc_has_reference;
    use syn::{Item, parse_file};

    fn first_item(source: &str) -> Item {
        parse_file(source)
            .expect("fixture source must parse")
            .items
            .into_iter()
            .next()
            .unwrap()
    }

    fn item_at(source: &str, index: usize) -> Item {
        parse_file(source)
            .expect("fixture source must parse")
            .items
            .into_iter()
            .nth(index)
            .unwrap()
    }

    #[test]
    fn item_doc_with_repository_reference_is_accepted() {
        assert!(doc_has_reference(&first_item(
            "/// docs/architecture/rules.md\npub struct Fixture;"
        )));
    }

    #[test]
    fn item_doc_with_external_reference_is_accepted() {
        assert!(doc_has_reference(&first_item(
            "/// https://example.invalid/spec\npub struct Fixture;"
        )));
    }

    #[test]
    fn item_without_doc_reference_is_rejected() {
        assert!(!doc_has_reference(&first_item("pub struct Fixture;")));
    }

    #[test]
    fn another_item_reference_does_not_satisfy_this_item() {
        assert!(!doc_has_reference(&item_at(
            "/// docs/architecture/rules.md\npub struct First;\npub struct Second;",
            1
        )));
    }
}

fn item_line(source: &str, item: &Item) -> usize {
    let name = match item {
        Item::Trait(x) => x.ident.to_string(),
        Item::Struct(x) => x.ident.to_string(),
        Item::Enum(x) => x.ident.to_string(),
        _ => return 1,
    };
    source
        .lines()
        .position(|line| line.contains(&name))
        .map_or(1, |line| line + 1)
}
