//! A library's public interface, by the paths its callers write.
//!
//! An item reachable at several paths is described once, at its preferred path: the shortest, and
//! among equally short ones a declaration before a re-export. Its other paths point there.
use crate::render::{View, kind, rank};
use rustdoc_types::{Id, Item, ItemEnum, Visibility};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One way to reach an item: the path a caller writes, and whether it passes through a `use`.
struct Member<'a> {
    name: String,
    item: &'a Item,
    reexported: bool,
}

/// A public module at its preferred path, with what it offers.
struct Module<'a> {
    path: String,
    item: &'a Item,
    members: Vec<Member<'a>>,
    /// Re-exports from other crates, as their source paths.
    external: Vec<String>,
}

/// How preferable a path is; smaller is better.
type Preference = (usize, bool, String);

pub(crate) fn render(view: &View<'_>) -> String {
    let Some(root) = view.item(&view.krate.root) else {
        return String::new();
    };
    let name = root.name.clone().unwrap_or_default();
    let modules = modules(view, root);
    let mut preferred: HashMap<Id, Preference> = HashMap::new();
    for module in &modules {
        for member in &module.members {
            let path = join(&module.path, &member.name);
            let candidate = (path.matches("::").count(), member.reexported, path);
            let best = preferred
                .entry(member.item.id)
                .or_insert_with(|| candidate.clone());
            if candidate < *best {
                *best = candidate;
            }
        }
    }
    let mut out =
        String::from("<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->\n");
    for module in &modules {
        if module.path.is_empty() {
            out.push_str(&format!("# crate `{name}`\n\n"));
        } else {
            out.push_str(&format!("\n# module `{name}::{}`\n\n", module.path));
        }
        view.describe(&mut out, module.item, 1, true);
        let mut sections = Vec::new();
        if !module.members.is_empty() {
            out.push('\n');
        }
        for member in &module.members {
            let path = join(&module.path, &member.name);
            match preferred.get(&member.item.id) {
                Some((_, _, best)) if *best != path => out.push_str(&format!(
                    "- {} `{path}` — same as `{best}`\n",
                    kind(member.item)
                )),
                _ => {
                    out.push_str(&view.index_line(&path, member.item));
                    out.push('\n');
                    if !matches!(member.item.inner, ItemEnum::Module(_)) {
                        sections.push((path, member.item));
                    }
                }
            }
        }
        if !module.external.is_empty() {
            out.push_str(&format!("\nRe-exports {}.\n", module.external.join(", ")));
        }
        for (path, item) in sections {
            view.section(&mut out, &path, item, 2);
        }
    }
    out
}

/// Every public module at its preferred path, level by level from the root.
fn modules<'a>(view: &View<'a>, root: &'a Item) -> Vec<Module<'a>> {
    let mut modules = vec![module(view, String::new(), root)];
    let mut placed = BTreeSet::from([root.id]);
    let mut level = 0;
    while level < modules.len() {
        let mut next: BTreeMap<Id, (Preference, &'a Item)> = BTreeMap::new();
        for module in &modules[level..] {
            for member in &module.members {
                if !matches!(member.item.inner, ItemEnum::Module(_))
                    || placed.contains(&member.item.id)
                {
                    continue;
                }
                let path = join(&module.path, &member.name);
                let candidate = (path.matches("::").count(), member.reexported, path);
                let best = next
                    .entry(member.item.id)
                    .or_insert_with(|| (candidate.clone(), member.item));
                if candidate < best.0 {
                    *best = (candidate, member.item);
                }
            }
        }
        level = modules.len();
        let mut found: Vec<_> = next.into_values().collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        for ((_, _, path), item) in found {
            placed.insert(item.id);
            modules.push(module(view, path, item));
        }
    }
    modules
}

fn module<'a>(view: &View<'a>, path: String, item: &'a Item) -> Module<'a> {
    let mut module = Module {
        path,
        item,
        members: Vec::new(),
        external: Vec::new(),
    };
    if let ItemEnum::Module(declared) = &item.inner {
        collect(
            view,
            &declared.items,
            &mut module,
            false,
            &mut BTreeSet::new(),
        );
    }
    module
        .members
        .sort_by(|a, b| (rank(a.item), &a.name).cmp(&(rank(b.item), &b.name)));
    module.external.sort();
    module
}

/// The public members of one module, following glob re-exports into the modules they name.
fn collect<'a>(
    view: &View<'a>,
    ids: &[Id],
    module: &mut Module<'a>,
    reexported: bool,
    globbed: &mut BTreeSet<Id>,
) {
    for item in ids.iter().filter_map(|id| view.item(id)) {
        match &item.inner {
            ItemEnum::Use(declared) if item.visibility == Visibility::Public => {
                match declared.id.and_then(|id| view.item(&id)) {
                    Some(imported) if declared.is_glob => {
                        if let ItemEnum::Module(inner) = &imported.inner
                            && globbed.insert(imported.id)
                        {
                            collect(view, &inner.items, module, true, globbed);
                        }
                    }
                    Some(imported) => module.members.push(Member {
                        name: declared.name.clone(),
                        item: imported,
                        reexported: true,
                    }),
                    None => module.external.push(format!("`{}`", declared.source)),
                }
            }
            ItemEnum::Use(_) | ItemEnum::Impl(_) => {}
            ItemEnum::Module(declared) if declared.is_stripped => {}
            _ if view.shows(item) => module.members.push(Member {
                name: item.name.clone().unwrap_or_default(),
                item,
                reexported,
            }),
            _ => {}
        }
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}::{name}")
    }
}
