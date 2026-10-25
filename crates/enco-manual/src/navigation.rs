//! A navigation manual: every module and item, private ones included, with source lines.
//!
//! `README.md` lists the modules, each with its summary, so a reader can pick where to look; each
//! module then has a file of its own.
use crate::render::{View, rank};
use rustdoc_types::{Item, ItemEnum, Visibility};
use std::collections::BTreeMap;

pub(crate) fn render(view: &View<'_>) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let Some(root) = view.item(&view.krate.root) else {
        return files;
    };
    let name = root.name.clone().unwrap_or_default();
    let mut modules = Vec::new();
    walk(view, root, String::new(), &mut modules);
    let mut index = format!("# crate `{name}`\n\n");
    view.describe(&mut index, root, 1, true);
    index.push_str("\nModules, each in a file of its own:\n\n");
    for (path, item) in &modules {
        let file = file(path);
        index.push_str(&view.index_line(path, item).replacen(
            &format!("`{path}`"),
            &format!("[`{path}`]({file})"),
            1,
        ));
        index.push('\n');
    }
    files.insert("README.md".to_owned(), index);
    for (path, item) in modules {
        files.insert(file(&path), module(view, &name, &path, item));
    }
    files
}

/// The crate root and every module below it, in source order of nesting.
fn walk<'a>(view: &View<'a>, item: &'a Item, path: String, modules: &mut Vec<(String, &'a Item)>) {
    let label = if path.is_empty() {
        "crate".to_owned()
    } else {
        path.clone()
    };
    modules.push((label, item));
    let ItemEnum::Module(declared) = &item.inner else {
        return;
    };
    let mut children: Vec<_> = declared
        .items
        .iter()
        .filter_map(|id| view.item(id))
        .filter(|child| matches!(child.inner, ItemEnum::Module(_)))
        .collect();
    children.sort_by_key(|child| child.name.clone());
    for child in children {
        let name = child.name.clone().unwrap_or_default();
        let path = if path.is_empty() {
            name
        } else {
            format!("{path}::{name}")
        };
        walk(view, child, path, modules);
    }
}

fn file(path: &str) -> String {
    format!("{}.md", path.replace("::", "/"))
}

fn module(view: &View<'_>, crate_name: &str, path: &str, item: &Item) -> String {
    let mut out = if path == "crate" {
        format!("# crate `{crate_name}`\n\n")
    } else {
        format!("# module `{crate_name}::{path}`\n\n")
    };
    view.describe(&mut out, item, 1, true);
    let ItemEnum::Module(declared) = &item.inner else {
        return out;
    };
    let view = &View {
        here: Some(item.id),
        ..*view
    };
    let mut items: Vec<&Item> = declared
        .items
        .iter()
        .filter_map(|id| view.item(id))
        .filter(|item| !matches!(item.inner, ItemEnum::Module(_) | ItemEnum::Impl(_)))
        .filter(|item| {
            !matches!(item.inner, ItemEnum::Use(_)) || item.visibility == Visibility::Public
        })
        .collect();
    items.sort_by(|a, b| (rank(a), &a.name).cmp(&(rank(b), &b.name)));
    let reexports: Vec<_> = items
        .iter()
        .filter_map(|item| match &item.inner {
            ItemEnum::Use(declared) => Some(format!("`{}`", declared.source)),
            _ => None,
        })
        .collect();
    let items: Vec<_> = items
        .into_iter()
        .filter(|item| !matches!(item.inner, ItemEnum::Use(_)))
        .collect();
    if !items.is_empty() {
        out.push('\n');
    }
    for item in &items {
        let name = item.name.as_deref().unwrap_or_default();
        out.push_str(&view.index_line(name, item));
        out.push('\n');
    }
    if !reexports.is_empty() {
        out.push_str(&format!("\nRe-exports {}.\n", reexports.join(", ")));
    }
    for item in items {
        let name = item.name.as_deref().unwrap_or_default();
        view.section(&mut out, name, item, 2);
    }
    out
}
