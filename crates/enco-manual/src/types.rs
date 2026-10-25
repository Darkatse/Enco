//! Every type inside a type, for edits and questions that concern all of them.
//!
//! A signature can borrow, name an item or carry a lifetime at any depth, as in
//! `Option<&'a [Item<'a>]>`, so a rule about one of these holds only when it reaches all of them.
use rustdoc_types::{
    AssocItemConstraintKind, GenericArg, GenericArgs, GenericBound, Path, Term, Type,
};

/// A place in a type that names an item or a lifetime.
pub(crate) enum Part<'a> {
    /// A path to an item, with the generic arguments written after it.
    Path(&'a mut Path),
    /// The lifetime of a reference or a trait object, when one is written.
    Lifetime(&'a mut Option<String>),
}

/// Visit every part of `ty`, each part before the types inside it.
pub(crate) fn walk(ty: &mut Type, visit: &mut impl FnMut(Part<'_>)) {
    match ty {
        Type::ResolvedPath(path) => walk_path(path, visit),
        Type::DynTrait(dyn_trait) => {
            for bound in &mut dyn_trait.traits {
                walk_path(&mut bound.trait_, visit);
            }
            visit(Part::Lifetime(&mut dyn_trait.lifetime));
        }
        Type::BorrowedRef {
            lifetime, type_, ..
        } => {
            visit(Part::Lifetime(lifetime));
            walk(type_, visit);
        }
        Type::Slice(inner)
        | Type::Array { type_: inner, .. }
        | Type::Pat { type_: inner, .. }
        | Type::RawPointer { type_: inner, .. } => walk(inner, visit),
        Type::Tuple(types) => {
            for ty in types {
                walk(ty, visit);
            }
        }
        Type::FunctionPointer(pointer) => {
            for (_, input) in &mut pointer.sig.inputs {
                walk(input, visit);
            }
            if let Some(output) = &mut pointer.sig.output {
                walk(output, visit);
            }
        }
        Type::ImplTrait(bounds) => walk_bounds(bounds, visit),
        Type::QualifiedPath {
            self_type,
            trait_,
            args,
            ..
        } => {
            walk(self_type, visit);
            if let Some(trait_) = trait_ {
                walk_path(trait_, visit);
            }
            if let Some(args) = args {
                walk_args(args, visit);
            }
        }
        Type::Generic(_) | Type::Primitive(_) | Type::Infer => {}
    }
}

fn walk_path(path: &mut Path, visit: &mut impl FnMut(Part<'_>)) {
    visit(Part::Path(path));
    if let Some(args) = path.args.as_deref_mut() {
        walk_args(args, visit);
    }
}

fn walk_args(args: &mut GenericArgs, visit: &mut impl FnMut(Part<'_>)) {
    match args {
        GenericArgs::AngleBracketed { args, constraints } => {
            for arg in args {
                if let GenericArg::Type(ty) = arg {
                    walk(ty, visit);
                }
            }
            for constraint in constraints {
                if let Some(args) = constraint.args.as_deref_mut() {
                    walk_args(args, visit);
                }
                match &mut constraint.binding {
                    AssocItemConstraintKind::Equality(Term::Type(ty)) => walk(ty, visit),
                    AssocItemConstraintKind::Equality(Term::Constant(_)) => {}
                    AssocItemConstraintKind::Constraint(bounds) => walk_bounds(bounds, visit),
                }
            }
        }
        GenericArgs::Parenthesized { inputs, output } => {
            for input in inputs {
                walk(input, visit);
            }
            if let Some(output) = output {
                walk(output, visit);
            }
        }
        GenericArgs::ReturnTypeNotation => {}
    }
}

fn walk_bounds(bounds: &mut [GenericBound], visit: &mut impl FnMut(Part<'_>)) {
    for bound in bounds {
        if let GenericBound::TraitBound { trait_, .. } = bound {
            walk_path(trait_, visit);
        }
    }
}
