//! The traits a type implements, as a manual states them.
use crate::signature;
use rustdoc_types::Impl;

/// The traits a type implements, each written as its implementation's declaration.
///
/// A declaration states which types and conditions the implementation covers:
/// `impl Clone for Wrapper<u8>` and `impl<T: Clone> Clone for Wrapper<T>` differ, while the trait's
/// name alone would make both read as an unconditional `Clone`.
pub(crate) fn implements<'a>(out: &mut String, traits: impl IntoIterator<Item = &'a Impl>) {
    let mut declarations: Vec<_> = traits
        .into_iter()
        .filter_map(|declared| Some((declared.trait_.as_ref()?, declared)))
        // The compiler implements this marker for every derived `PartialEq`.
        .filter(|(trait_, _)| trait_.path != "StructuralPartialEq")
        .map(|(trait_, declared)| (signature::path(trait_), implementation(declared)))
        .collect();
    if declarations.is_empty() {
        return;
    }
    declarations.sort();
    let lines: Vec<_> = declarations
        .into_iter()
        .map(|(_, declaration)| declaration)
        .collect();
    out.push_str(&format!(
        "\nImplements:\n\n```rust\n{}\n```\n",
        lines.join("\n")
    ));
}

/// An implementation's declaration on one line, such as `impl<I: Clone> Clone for Message<I>`.
fn implementation(declared: &Impl) -> String {
    let (params, _) = signature::generics(&declared.generics);
    let trait_ = declared.trait_.as_ref().map_or_else(String::new, |trait_| {
        format!("{} for ", signature::path(trait_))
    });
    let predicates = signature::predicates(&declared.generics);
    let clause = if predicates.is_empty() {
        String::new()
    } else {
        format!(" where {}", predicates.join(", "))
    };
    format!(
        "impl{params} {trait_}{}{clause}",
        signature::ty(&declared.for_)
    )
}
