//! One item as a manual section: heading, source, declaration, docs, then its members.
use crate::{signature, text};
use rustdoc_types::{Crate, Id, Impl, Item, ItemEnum, StructKind, VariantKind, Visibility};

/// What a manual shows of the code it describes.
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub krate: &'a Crate,
    /// Private items too, for a navigation manual.
    pub private: bool,
    /// Line numbers too. A committed manual leaves them out, so moving code does not change it.
    pub lines: bool,
    /// Removed from rustdoc's source paths, which start where rustdoc ran.
    pub prefix: &'a str,
    /// The module being described, whose private items the source declares without `pub`.
    pub here: Option<Id>,
}

/// How an item is called in headings and indexes.
pub(crate) fn kind(item: &Item) -> &'static str {
    match &item.inner {
        ItemEnum::Module(_) => "module",
        ItemEnum::Struct(_) => "struct",
        ItemEnum::Union(_) => "union",
        ItemEnum::Enum(_) => "enum",
        ItemEnum::Trait(_) | ItemEnum::TraitAlias(_) => "trait",
        ItemEnum::Function(_) => "fn",
        ItemEnum::TypeAlias(_) | ItemEnum::AssocType { .. } => "type",
        ItemEnum::Constant { .. } | ItemEnum::AssocConst { .. } => "const",
        ItemEnum::Static(_) => "static",
        ItemEnum::Macro(_) | ItemEnum::ProcMacro(_) => "macro",
        _ => "item",
    }
}

/// Modules first, then types, traits, functions and values, as rustdoc orders them.
pub(crate) fn rank(item: &Item) -> u8 {
    match &item.inner {
        ItemEnum::Module(_) => 0,
        ItemEnum::Macro(_) | ItemEnum::ProcMacro(_) => 1,
        ItemEnum::Struct(_) | ItemEnum::Union(_) | ItemEnum::Enum(_) => 2,
        ItemEnum::Trait(_) | ItemEnum::TraitAlias(_) => 3,
        ItemEnum::Function(_) => 4,
        ItemEnum::TypeAlias(_) => 5,
        _ => 6,
    }
}

impl<'a> View<'a> {
    pub(crate) fn item(&self, id: &Id) -> Option<&'a Item> {
        self.krate.index.get(id)
    }

    pub(crate) fn shows(&self, item: &Item) -> bool {
        self.private || item.visibility == Visibility::Public
    }

    /// One index line: the item's kind, its name and its summary.
    pub(crate) fn index_line(&self, label: &str, item: &Item) -> String {
        let summary = text::summary(item.docs.as_deref(), &item.links);
        let summary = if summary.is_empty() {
            "(no summary)".to_owned()
        } else {
            summary
        };
        format!("- {} `{label}` — {summary}", kind(item))
    }

    pub(crate) fn source(&self, item: &Item) -> Option<String> {
        let span = item.span.as_ref()?;
        let file = span.filename.to_string_lossy();
        let file = file.strip_prefix(self.prefix).unwrap_or(&file);
        Some(if self.lines {
            format!("{file}:{}", span.begin.0)
        } else {
            file.to_owned()
        })
    }

    /// The item's section at heading `level`, followed by its members one level below.
    pub(crate) fn section(&self, out: &mut String, label: &str, item: &Item, level: usize) {
        out.push_str(&format!(
            "\n{} {} `{label}`\n\n",
            "#".repeat(level),
            kind(item)
        ));
        self.describe(out, item, level, true);
        self.members(out, label, item, level + 1);
    }

    /// The source, unless `source` is false, then the declaration and the docs, without a heading.
    pub(crate) fn describe(&self, out: &mut String, item: &Item, level: usize, source: bool) {
        let mut parts = Vec::new();
        if source && let Some(source) = self.source(item) {
            parts.push(format!("`{source}`"));
        }
        if !matches!(item.inner, ItemEnum::Module(_)) {
            parts.push(format!("```rust\n{}\n```", self.declaration(item)));
        }
        let body = text::body(item.docs.as_deref(), &item.links, level);
        if !body.is_empty() {
            parts.push(body);
        }
        out.push_str(&parts.join("\n\n"));
        out.push('\n');
    }

    fn declaration(&self, item: &Item) -> String {
        let name = item.name.as_deref().unwrap_or_default();
        let visibility = signature::visibility(&item.visibility, self.here);
        match &item.inner {
            ItemEnum::Struct(declared) => {
                let (params, clause) = signature::generics(&declared.generics);
                let shape = match &declared.kind {
                    StructKind::Unit => ";".to_owned(),
                    StructKind::Tuple(fields) => format!("({});", self.tuple(fields)),
                    StructKind::Plain {
                        fields,
                        has_stripped_fields,
                    } => format!(
                        " {{{}\n}}",
                        self.fields(fields, *has_stripped_fields, "    ")
                    ),
                };
                format!("{visibility}struct {name}{params}{clause}{shape}")
            }
            ItemEnum::Union(declared) => {
                let (params, clause) = signature::generics(&declared.generics);
                let fields = self.fields(&declared.fields, declared.has_stripped_fields, "    ");
                format!("{visibility}union {name}{params}{clause} {{{fields}\n}}")
            }
            ItemEnum::Enum(declared) => {
                let (params, clause) = signature::generics(&declared.generics);
                let mut variants: Vec<_> = declared
                    .variants
                    .iter()
                    .filter_map(|id| self.item(id))
                    .map(|variant| format!("\n    {},", self.variant(variant)))
                    .collect();
                if declared.has_stripped_variants {
                    variants.push("\n    // some variants omitted".into());
                }
                format!(
                    "{visibility}enum {name}{params}{clause} {{{}\n}}",
                    variants.concat()
                )
            }
            ItemEnum::Trait(declared) => {
                let (params, clause) = signature::generics(&declared.generics);
                let bounds = if declared.bounds.is_empty() {
                    String::new()
                } else {
                    format!(": {}", signature::bounds(&declared.bounds))
                };
                let safety = if declared.is_unsafe { "unsafe " } else { "" };
                format!("{visibility}{safety}trait {name}{params}{bounds}{clause}")
            }
            ItemEnum::Function(function) => {
                signature::function(name, function, &item.visibility, self.here)
            }
            ItemEnum::TypeAlias(alias) => {
                let (params, clause) = signature::generics(&alias.generics);
                format!(
                    "{visibility}type {name}{params} = {}{clause};",
                    signature::ty(&alias.type_)
                )
            }
            ItemEnum::Constant { type_, const_ } => format!(
                "{visibility}const {name}: {} = {};",
                signature::ty(type_),
                const_.expr
            ),
            ItemEnum::Static(declared) => format!(
                "{visibility}static {}{name}: {} = {};",
                if declared.is_mutable { "mut " } else { "" },
                signature::ty(&declared.type_),
                declared.expr
            ),
            ItemEnum::AssocConst { type_, value, .. } => format!(
                "const {name}: {}{};",
                signature::ty(type_),
                value
                    .as_ref()
                    .map_or_else(String::new, |value| format!(" = {value}"))
            ),
            ItemEnum::AssocType {
                generics,
                bounds,
                type_,
                ..
            } => {
                let (params, clause) = signature::generics(generics);
                let bounds = if bounds.is_empty() {
                    String::new()
                } else {
                    format!(": {}", signature::bounds(bounds))
                };
                let value = type_
                    .as_ref()
                    .map_or_else(String::new, |type_| format!(" = {}", signature::ty(type_)));
                format!("type {name}{params}{bounds}{value}{clause};")
            }
            ItemEnum::Macro(source) => source.clone(),
            ItemEnum::Module(_) => format!("{visibility}mod {name}"),
            _ => name.to_owned(),
        }
    }

    fn tuple(&self, fields: &[Option<Id>]) -> String {
        fields
            .iter()
            .map(|field| match field.as_ref().and_then(|id| self.item(id)) {
                Some(field) if self.shows(field) || field.visibility == Visibility::Default => {
                    match &field.inner {
                        ItemEnum::StructField(field_type) => format!(
                            "{}{}",
                            signature::visibility(&field.visibility, self.here),
                            signature::ty(field_type)
                        ),
                        _ => "/* private field */".into(),
                    }
                }
                _ => "/* private field */".into(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Named fields as declaration lines; a variant's fields share the enum's visibility.
    fn fields(&self, fields: &[Id], stripped: bool, indent: &str) -> String {
        let mut lines: Vec<_> = fields
            .iter()
            .filter_map(|id| self.item(id))
            .filter(|field| self.shows(field) || field.visibility == Visibility::Default)
            .filter_map(|field| match &field.inner {
                ItemEnum::StructField(field_type) => Some(format!(
                    "\n{indent}{}{}: {},",
                    signature::visibility(&field.visibility, self.here),
                    field.name.as_deref().unwrap_or_default(),
                    signature::ty(field_type)
                )),
                _ => None,
            })
            .collect();
        if stripped && !self.private {
            lines.push(format!("\n{indent}/* private fields */"));
        }
        lines.concat()
    }

    fn variant(&self, variant: &Item) -> String {
        let name = variant.name.as_deref().unwrap_or_default();
        let ItemEnum::Variant(declared) = &variant.inner else {
            return name.to_owned();
        };
        let shape = match &declared.kind {
            VariantKind::Plain => String::new(),
            VariantKind::Tuple(fields) => format!("({})", self.tuple(fields)),
            VariantKind::Struct {
                fields,
                has_stripped_fields,
            } => format!(
                " {{{}\n    }}",
                self.fields(fields, *has_stripped_fields, "        ")
            ),
        };
        let discriminant = declared
            .discriminant
            .as_ref()
            .map_or_else(String::new, |discriminant| {
                format!(" = {}", discriminant.expr)
            });
        format!("{name}{shape}{discriminant}")
    }

    /// Documented fields and variants as a list, then methods and trait items as sections, then
    /// the traits the type implements on one line.
    fn members(&self, out: &mut String, label: &str, item: &Item, level: usize) {
        let (parts, impls): (Vec<Id>, &[Id]) = match &item.inner {
            ItemEnum::Struct(declared) => (
                match &declared.kind {
                    StructKind::Plain { fields, .. } => fields.clone(),
                    StructKind::Tuple(fields) => fields.iter().flatten().copied().collect(),
                    StructKind::Unit => Vec::new(),
                },
                &declared.impls,
            ),
            ItemEnum::Union(declared) => (declared.fields.clone(), &declared.impls),
            ItemEnum::Enum(declared) => (declared.variants.clone(), &declared.impls),
            ItemEnum::Trait(declared) => {
                for member in declared.items.iter().filter_map(|id| self.item(id)) {
                    self.member(out, label, member, item, level);
                }
                return;
            }
            _ => return,
        };
        let lines = self.documented(&parts, "");
        if !lines.is_empty() {
            out.push_str(&format!("\n{}\n", lines.join("\n")));
        }
        let impls: Vec<&Impl> = impls
            .iter()
            .filter_map(|id| self.item(id))
            .filter_map(|item| match &item.inner {
                ItemEnum::Impl(declared) => Some(declared),
                _ => None,
            })
            .collect();
        for declared in impls.iter().filter(|declared| declared.trait_.is_none()) {
            for id in &declared.items {
                if let Some(member) = self.item(id).filter(|member| self.shows(member)) {
                    self.member(out, label, member, item, level);
                }
            }
        }
        let mut traits: Vec<_> = impls
            .iter()
            .filter(|declared| {
                !declared.is_synthetic && declared.blanket_impl.is_none() && !declared.is_negative
            })
            .filter_map(|declared| declared.trait_.as_ref())
            // The compiler implements this marker for every derived `PartialEq`.
            .filter(|trait_| trait_.path != "StructuralPartialEq")
            .map(|trait_| format!("`{}`", signature::path(trait_)))
            .collect();
        traits.sort();
        traits.dedup();
        if !traits.is_empty() {
            out.push_str(&format!("\nImplements {}.\n", traits.join(", ")));
        }
    }

    /// One list line for each documented field or variant, with a variant's documented fields
    /// nested below it.
    fn documented(&self, parts: &[Id], indent: &str) -> Vec<String> {
        let mut lines = Vec::new();
        for part in parts.iter().filter_map(|id| self.item(id)) {
            if !self.shows(part) && part.visibility != Visibility::Default {
                continue;
            }
            let docs = text::body(part.docs.as_deref(), &part.links, 0);
            let nested = match &part.inner {
                ItemEnum::Variant(variant) => match &variant.kind {
                    rustdoc_types::VariantKind::Struct { fields, .. } => {
                        self.documented(fields, &format!("{indent}  "))
                    }
                    rustdoc_types::VariantKind::Tuple(fields) => {
                        let fields: Vec<_> = fields.iter().flatten().copied().collect();
                        self.documented(&fields, &format!("{indent}  "))
                    }
                    rustdoc_types::VariantKind::Plain => Vec::new(),
                },
                _ => Vec::new(),
            };
            if docs.is_empty() && nested.is_empty() {
                continue;
            }
            let name = part.name.as_deref().unwrap_or_default();
            lines.push(if docs.is_empty() {
                format!("{indent}- `{name}`")
            } else {
                format!(
                    "{indent}- `{name}` — {}",
                    docs.replace('\n', &format!("\n{indent}  "))
                )
            });
            lines.extend(nested);
        }
        lines
    }

    /// A method or trait item as a section; its source is shown when it lies in another file than
    /// its owner's, or with line numbers.
    fn member(&self, out: &mut String, label: &str, member: &Item, owner: &Item, level: usize) {
        let name = member.name.as_deref().unwrap_or_default();
        out.push_str(&format!("\n{} `{label}::{name}`\n\n", "#".repeat(level)));
        let file = |item: &Item| item.span.as_ref().map(|span| span.filename.clone());
        let source = self.lines || file(member) != file(owner);
        self.describe(out, member, level, source);
    }
}
