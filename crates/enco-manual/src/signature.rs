//! Rust declarations written back from rustdoc's types, as the source states them.
use crate::types::{self, Part};
use rustdoc_types::{
    Abi, AssocItemConstraint, AssocItemConstraintKind, Function, FunctionHeader, GenericArg,
    GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind, Generics, Id, Path,
    PreciseCapturingArg, Term, TraitBoundModifier, Type, Visibility, WherePredicate,
};

/// The visibility as the source writes it. rustdoc reports a private item as visible in its own
/// module, `here`, which the source writes as nothing.
pub(crate) fn visibility(visibility: &Visibility, here: Option<Id>) -> String {
    match visibility {
        Visibility::Public => "pub ".into(),
        Visibility::Default => String::new(),
        Visibility::Crate => "pub(crate) ".into(),
        Visibility::Restricted { parent, .. } if Some(*parent) == here => String::new(),
        Visibility::Restricted { path, .. } => {
            format!("pub(in {}) ", path.trim_start_matches("::"))
        }
    }
}

pub(crate) fn ty(ty: &Type) -> String {
    match ty {
        Type::ResolvedPath(path) => self::path(path),
        Type::DynTrait(dyn_trait) => {
            let mut parts: Vec<_> = dyn_trait
                .traits
                .iter()
                .map(|bound| format!("{}{}", hrtb(&bound.generic_params), path(&bound.trait_)))
                .collect();
            parts.extend(dyn_trait.lifetime.clone());
            format!("dyn {}", parts.join(" + "))
        }
        Type::Generic(name) | Type::Primitive(name) => name.clone(),
        Type::FunctionPointer(pointer) => format!(
            "{}{}fn({}){}",
            hrtb(&pointer.generic_params),
            header(&pointer.header),
            pointer
                .sig
                .inputs
                .iter()
                .map(|(_, input)| self::ty(input))
                .collect::<Vec<_>>()
                .join(", "),
            output(pointer.sig.output.as_ref())
        ),
        Type::Tuple(types) if types.len() == 1 => format!("({},)", self::ty(&types[0])),
        Type::Tuple(types) => format!("({})", list(types.iter().map(self::ty))),
        Type::Slice(inner) => format!("[{}]", self::ty(inner)),
        Type::Array { type_, len } => format!("[{}; {len}]", self::ty(type_)),
        Type::Pat { type_, .. } => self::ty(type_),
        Type::ImplTrait(bounds) => format!("impl {}", self::bounds(bounds)),
        Type::Infer => "_".into(),
        Type::RawPointer { is_mutable, type_ } => format!(
            "*{} {}",
            if *is_mutable { "mut" } else { "const" },
            self::ty(type_)
        ),
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => format!(
            "&{}{}{}",
            lifetime
                .as_ref()
                .map_or_else(String::new, |lifetime| format!("{lifetime} ")),
            if *is_mutable { "mut " } else { "" },
            self::ty(type_)
        ),
        Type::QualifiedPath {
            name,
            args,
            self_type,
            trait_,
        } => {
            let args = args.as_deref().map(generic_args).unwrap_or_default();
            match trait_ {
                Some(trait_) => format!(
                    "<{} as {}>::{name}{args}",
                    self::ty(self_type),
                    path(trait_)
                ),
                None => format!("{}::{name}{args}", self::ty(self_type)),
            }
        }
    }
}

pub(crate) fn path(path: &Path) -> String {
    // A path a macro wrote for its own expansion, such as `$crate::clone::Clone` or
    // `_serde::Serialize`, is not one the source wrote; the item's name stands for it.
    let written = match path.path.split("::").next() {
        Some(first) if first == "$crate" || first.starts_with('_') => {
            path.path.rsplit("::").next().unwrap_or(&path.path)
        }
        _ => &path.path,
    };
    format!(
        "{written}{}",
        path.args.as_deref().map(generic_args).unwrap_or_default()
    )
}

fn generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::AngleBracketed { args, constraints } => {
            let parts: Vec<_> = args
                .iter()
                .map(|arg| match arg {
                    GenericArg::Lifetime(lifetime) => lifetime.clone(),
                    GenericArg::Type(arg) => ty(arg),
                    GenericArg::Const(constant) => constant.expr.clone(),
                    GenericArg::Infer => "_".into(),
                })
                .chain(constraints.iter().map(constraint))
                .collect();
            if parts.is_empty() {
                String::new()
            } else {
                format!("<{}>", parts.join(", "))
            }
        }
        GenericArgs::Parenthesized { inputs, output } => {
            format!(
                "({}){}",
                list(inputs.iter().map(ty)),
                self::output(output.as_ref())
            )
        }
        GenericArgs::ReturnTypeNotation => "(..)".into(),
    }
}

fn constraint(constraint: &AssocItemConstraint) -> String {
    let args = constraint
        .args
        .as_deref()
        .map(generic_args)
        .unwrap_or_default();
    match &constraint.binding {
        AssocItemConstraintKind::Equality(term) => {
            format!("{}{args} = {}", constraint.name, self::term(term))
        }
        AssocItemConstraintKind::Constraint(bounds) => {
            format!("{}{args}: {}", constraint.name, self::bounds(bounds))
        }
    }
}

fn term(term: &Term) -> String {
    match term {
        Term::Type(term) => ty(term),
        Term::Constant(constant) => constant.expr.clone(),
    }
}

pub(crate) fn bounds(bounds: &[GenericBound]) -> String {
    bounds
        .iter()
        .map(|bound| match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => format!(
                "{}{}{}",
                hrtb(generic_params),
                match modifier {
                    TraitBoundModifier::None => "",
                    TraitBoundModifier::Maybe => "?",
                    TraitBoundModifier::MaybeConst => "[const] ",
                },
                path(trait_)
            ),
            GenericBound::Outlives(lifetime) => lifetime.clone(),
            GenericBound::Use(args) => format!(
                "use<{}>",
                list(args.iter().map(|arg| match arg {
                    PreciseCapturingArg::Lifetime(name) | PreciseCapturingArg::Param(name) =>
                        name.clone(),
                }))
            ),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn hrtb(params: &[GenericParamDef]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("for<{}> ", list(params.iter().map(param)))
    }
}

fn param(param: &GenericParamDef) -> String {
    match &param.kind {
        GenericParamDefKind::Lifetime { outlives } if outlives.is_empty() => param.name.clone(),
        GenericParamDefKind::Lifetime { outlives } => {
            format!("{}: {}", param.name, outlives.join(" + "))
        }
        GenericParamDefKind::Type {
            bounds, default, ..
        } => {
            let mut text = param.name.clone();
            if !bounds.is_empty() {
                text = format!("{text}: {}", self::bounds(bounds));
            }
            if let Some(default) = default {
                text = format!("{text} = {}", ty(default));
            }
            text
        }
        GenericParamDefKind::Const { type_, default } => match default {
            Some(default) => format!("const {}: {} = {default}", param.name, ty(type_)),
            None => format!("const {}: {}", param.name, ty(type_)),
        },
    }
}

/// The `<...>` after a name and the `where` clause after a declaration, each empty when unused.
pub(crate) fn generics(generics: &Generics) -> (String, String) {
    let params: Vec<_> = generics
        .params
        .iter()
        // `impl Trait` arguments appear as parameters the source never wrote.
        .filter(|param| {
            !matches!(
                param.kind,
                GenericParamDefKind::Type {
                    is_synthetic: true,
                    ..
                }
            )
        })
        .map(param)
        .collect();
    let predicates = predicates(generics);
    (
        if params.is_empty() {
            String::new()
        } else {
            format!("<{}>", params.join(", "))
        },
        if predicates.is_empty() {
            String::new()
        } else {
            format!("\nwhere\n    {}", predicates.join(",\n    "))
        },
    )
}

/// The predicates of a `where` clause.
pub(crate) fn predicates(generics: &Generics) -> Vec<String> {
    generics
        .where_predicates
        .iter()
        .map(|predicate| match predicate {
            WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } => format!(
                "{}{}: {}",
                hrtb(generic_params),
                ty(type_),
                self::bounds(bounds)
            ),
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                format!("{lifetime}: {}", outlives.join(" + "))
            }
            WherePredicate::EqPredicate { lhs, rhs } => format!("{} == {}", ty(lhs), term(rhs)),
        })
        .collect()
}

pub(crate) fn function(
    name: &str,
    function: &Function,
    visibility: &Visibility,
    here: Option<Id>,
) -> String {
    let future = async_trait_output(function);
    let mut generics = function.generics.clone();
    let mut inputs = function.sig.inputs.clone();
    // The source declares none of the lifetimes `#[async_trait]` adds, and writes none of them.
    if future.is_some() {
        generics
            .params
            .retain(|param| !is_async_trait_lifetime(&param.name));
        generics.where_predicates.retain(|predicate| match predicate {
            WherePredicate::BoundPredicate { bounds, .. } => !bounds.iter().any(|bound| {
                matches!(bound, GenericBound::Outlives(lifetime) if is_async_trait_lifetime(lifetime))
            }),
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                !is_async_trait_lifetime(lifetime)
                    && !outlives.iter().any(|lifetime| is_async_trait_lifetime(lifetime))
            }
            WherePredicate::EqPredicate { .. } => true,
        });
        for (_, input) in &mut inputs {
            types::walk(input, &mut |part| match part {
                Part::Lifetime(lifetime) => {
                    if lifetime.as_deref().is_some_and(is_async_trait_lifetime) {
                        *lifetime = None;
                    }
                }
                Part::Path(path) => {
                    if let Some(GenericArgs::AngleBracketed { args, .. }) = path.args.as_deref_mut()
                    {
                        args.retain(|arg| {
                            !matches!(arg, GenericArg::Lifetime(lifetime) if is_async_trait_lifetime(lifetime))
                        });
                    }
                }
            });
        }
    }
    let (params, clause) = self::generics(&generics);
    let inputs = inputs
        .iter()
        .map(|(name, input)| match (name.as_str(), input) {
            ("self", Type::Generic(own)) if own == "Self" => "self".into(),
            (
                "self",
                Type::BorrowedRef {
                    lifetime,
                    is_mutable,
                    type_,
                },
            ) if matches!(type_.as_ref(), Type::Generic(own) if own == "Self") => {
                format!(
                    "&{}{}self",
                    lifetime
                        .as_ref()
                        .map_or_else(String::new, |lifetime| format!("{lifetime} ")),
                    if *is_mutable { "mut " } else { "" }
                )
            }
            (name, input) => format!("{name}: {}", ty(input)),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}{}{}fn {name}{params}({inputs}){}{clause}",
        self::visibility(visibility, here),
        if future.is_some() { "async " } else { "" },
        header(&function.header),
        output(future.or(function.sig.output.as_ref()))
    )
}

/// What an `#[async_trait]` method returns as written, or `None` for any other function.
///
/// The macro turns `async fn` into a method returning
/// `Pin<Box<dyn Future<Output = T> + Send + 'async_trait>>`; this finds `T` so the manual can show
/// the method as written.
fn async_trait_output(function: &Function) -> Option<&Type> {
    if !function
        .generics
        .params
        .iter()
        .any(|param| param.name == "'async_trait")
    {
        return None;
    }
    let Some(Type::ResolvedPath(pin)) = &function.sig.output else {
        return None;
    };
    let GenericArgs::AngleBracketed { args, .. } = pin.args.as_deref()? else {
        return None;
    };
    let [GenericArg::Type(Type::ResolvedPath(boxed))] = args.as_slice() else {
        return None;
    };
    let GenericArgs::AngleBracketed { args, .. } = boxed.args.as_deref()? else {
        return None;
    };
    let [GenericArg::Type(Type::DynTrait(future))] = args.as_slice() else {
        return None;
    };
    future.traits.iter().find_map(|bound| {
        let GenericArgs::AngleBracketed { constraints, .. } = bound.trait_.args.as_deref()? else {
            return None;
        };
        constraints
            .iter()
            .find_map(|constraint| match &constraint.binding {
                AssocItemConstraintKind::Equality(Term::Type(output))
                    if constraint.name == "Output" =>
                {
                    Some(output)
                }
                _ => None,
            })
    })
}

/// Whether `name` is a lifetime `#[async_trait]` adds: `'async_trait`, or one of `'life0`,
/// `'life1` and so on, one per borrowed input.
///
/// A lifetime the author names, such as `'lifetime`, is not one of them.
fn is_async_trait_lifetime(name: &str) -> bool {
    name == "'async_trait"
        || name.strip_prefix("'life").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn header(header: &FunctionHeader) -> String {
    let mut text = String::new();
    if header.is_const {
        text.push_str("const ");
    }
    if header.is_async {
        text.push_str("async ");
    }
    if header.is_unsafe {
        text.push_str("unsafe ");
    }
    let abi = match &header.abi {
        Abi::Rust => return text,
        Abi::C { .. } => "C",
        Abi::Cdecl { .. } => "cdecl",
        Abi::Stdcall { .. } => "stdcall",
        Abi::Fastcall { .. } => "fastcall",
        Abi::Aapcs { .. } => "aapcs",
        Abi::Win64 { .. } => "win64",
        Abi::SysV64 { .. } => "sysv64",
        Abi::System { .. } => "system",
        Abi::Other(abi) => abi,
    };
    text.push_str(&format!("extern \"{abi}\" "));
    text
}

fn output(output: Option<&Type>) -> String {
    output.map_or_else(String::new, |output| format!(" -> {}", ty(output)))
}

fn list(parts: impl Iterator<Item = String>) -> String {
    parts.collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::is_async_trait_lifetime;

    #[test]
    fn only_the_lifetimes_async_trait_names_are_its_own() {
        for generated in ["'async_trait", "'life0", "'life12"] {
            assert!(is_async_trait_lifetime(generated), "{generated}");
        }
        for written in ["'lifetime", "'life", "'a"] {
            assert!(!is_async_trait_lifetime(written), "{written}");
        }
    }
}
