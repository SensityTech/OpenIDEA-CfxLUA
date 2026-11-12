//! Whether a value of a type can stand where another type is asked for (`ITy.subTypeOf`), and where a
//! LuaCATS type cannot be checked from what is written (`LuaCatsTypeChecks`).

use super::context::Cx;
use super::expressions::table_of;
use super::members::{resolve_aliases, supers};
use super::{ANONYMOUS_TABLE, Signature, Ty};
use crate::index::EnumKind;
use crate::lua::tree::N;

/// What every type answers first: anything fits `any` unless `strict`, fits a union by one of its members,
/// and fits itself.
fn same(cx: &Cx, ty: &Ty, other: &Ty, strict: bool) -> bool {
    match other {
        Ty::Unknown => !strict,
        Ty::Union(members) => members.iter().any(|member| fits(cx, ty, member, strict)),
        _ => ty == other,
    }
}

/// Whether the table a class was made for is written without any field: an empty list fits any array.
fn is_empty_table(cx: &Cx, name: &str) -> bool {
    let Some((file, offset)) = table_of(name) else { return false };
    let empty = cx.read(file, |src| {
        let inner = src.root().tree().node_at(src.root().source(), offset);
        let table = std::iter::once(inner).chain(inner.ancestors()).find(|node| node.is(N::TableExpr));
        table.is_some_and(|table| table.children_of(N::TableField).next().is_none())
    });
    empty.unwrap_or(false)
}

/// A function type takes the place of another one when the parameters of that one fit its own, in order.
fn signature_fits(cx: &Cx, wanted: &Signature, given: &Signature, strict: bool) -> bool {
    let fits_at = |(rank, param): (usize, &super::Param)| {
        given.params.get(rank).is_some_and(|own| fits(cx, &param.ty, &own.ty, strict))
    };
    wanted.params.iter().enumerate().all(fits_at)
}

/// Whether a value of type `ty` can stand where `other` is asked for. `nil` fits anything, as it does in
/// the JetBrains plugin unless its strict nil option is set.
pub fn fits(cx: &Cx, ty: &Ty, other: &Ty, strict: bool) -> bool {
    match ty {
        Ty::Unknown => !strict,
        Ty::Void => false,
        Ty::Nil => true,
        Ty::Primitive(_) | Ty::StringLiteral(_) => same(cx, ty, other, strict),
        Ty::Array(element) => {
            let array = matches!(other, Ty::Array(wanted) if fits(cx, element, wanted, strict));
            same(cx, ty, other, strict) || array || *other == Ty::TABLE
        }
        Ty::Class(class) => {
            if *other == Ty::TABLE || same(cx, ty, other, strict) {
                return true;
            }
            if class.flags & ANONYMOUS_TABLE != 0 {
                return matches!(other, Ty::Array(_)) && is_empty_table(cx, &class.name);
            }
            supers(cx, ty).iter().any(|parent| Ty::named(parent) == *other)
        }
        Ty::Function(function) => {
            if same(cx, ty, other, strict) || *other == Ty::FUNCTION {
                return true;
            }
            let Ty::Function(wanted) = other else { return false };
            let taken = |given: &Signature| wanted.signatures().any(|wanted| signature_fits(cx, wanted, given, strict));
            function.signatures().any(taken)
        }
        Ty::Generic(generic) => {
            if same(cx, ty, other, strict) {
                return true;
            }
            match other {
                Ty::Generic(wanted) => {
                    fits(cx, &generic.base, &wanted.base, strict)
                        && generic.params.len() == wanted.params.len()
                        && generic.params.iter().zip(&wanted.params).all(|(own, asked)| fits(cx, own, asked, strict))
                }
                _ => fits(cx, &generic.base, other, strict),
            }
        }
        Ty::Tuple(list) => match other {
            Ty::Tuple(wanted) => {
                list.len() == wanted.len()
                    && list.iter().zip(wanted.iter()).all(|(own, asked)| fits(cx, own, asked, strict))
            }
            _ => false,
        },
        Ty::Union(members) => {
            same(cx, ty, other, strict) || members.iter().any(|member| fits(cx, member, other, strict))
        }
    }
}

/// Whether a value of type `actual` may be given where `expected` is declared although it does not fit as
/// written: `expected` is an alias of a type that takes it, names string values while a string is given (its
/// value is rarely known), or is an enum (its values are the fields of a table).
pub fn is_uncheckable(cx: &Cx, expected: &Ty, actual: &Ty) -> bool {
    let resolved = resolve_aliases(cx, expected);
    if resolved != *expected && fits(cx, actual, &resolved, false) {
        return true;
    }
    let is_enum = |ty: &Ty| match ty {
        Ty::Class(class) if !ty.is_anonymous() && !ty.is_global() => {
            cx.index.class(&class.name).is_some_and(|declared| declared.enum_kind != EnumKind::No)
        }
        _ => false,
    };
    if resolved.each().iter().any(is_enum) {
        return true;
    }
    if !resolved.each().iter().any(|member| matches!(member, Ty::StringLiteral(_))) {
        return false;
    }
    actual.each().iter().all(|member| *member == Ty::STRING || matches!(member, Ty::StringLiteral(_)))
}
