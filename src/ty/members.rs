//! The members of a type: what the index declares for its classes, and the fields a type carries itself
//! (`LuaShortNamesManager`, `ClassMemberChain`, `LuaCatsIndexSignatures`, `TyAliasSubstitutor`).

use super::context::{Cx, Psi};
use super::expressions::assigned_name;
use super::{ANONYMOUS, Class, Detail, DocField, G, GLOBAL, Ty};
use crate::index::{Found, MemberKind, declare};
use crate::lua::Source;
use std::cell::RefCell;
use std::rc::Rc;

/// A member of a class: one a file declares, or a field of a table written in a comment.
#[derive(Clone, Debug)]
pub enum Member<'a> {
    Declared(Found<'a>),
    Written(DocField),
}

impl Member<'_> {
    pub fn name(&self) -> &str {
        match self {
            Member::Declared(found) => &found.member.name,
            Member::Written(field) => &field.name,
        }
    }

    /// Whether the member is declared as a function: `function a:b()` or `function b()`.
    pub fn is_function(&self) -> bool {
        matches!(self, Member::Declared(found) if matches!(found.member.kind, MemberKind::Method | MemberKind::Function))
    }
}

struct Module(Option<Rc<str>>);

/// The name `---@module Name` gives to a file.
pub fn module_of(src: &Source) -> Option<Rc<str>> {
    src.model(|src| Module(declare::module_name(src))).0.clone()
}

/// The name the members of a class are looked up under: the class a generic parameter is bound to stands
/// for the parameter.
fn lookup_name(class: &Ty) -> Option<&str> {
    match class {
        Ty::Class(class) => match &class.detail {
            Detail::Param(base) => base.as_deref(),
            _ => Some(&class.name),
        },
        other => other.class_name(),
    }
}

fn written(class: &Ty) -> Option<&[DocField]> {
    match class {
        Ty::Class(class) => match &class.detail {
            Detail::Table(fields) => Some(fields),
            _ => None,
        },
        _ => None,
    }
}

/// The type of a member.
pub fn type_of(cx: &Cx, member: &Member) -> Ty {
    match member {
        Member::Written(field) => field.ty.clone(),
        Member::Declared(found) => match &found.member.ty {
            Some(ty) => ty.clone(),
            None => {
                let inferred = cx.at(found.file, found.member.node, |psi| cx.with_value(0, || cx.infer(psi)));
                inferred.unwrap_or(Ty::Unknown)
            }
        },
    }
}

/// The type a declaration found for a global name gives to it.
pub fn declared_type(cx: &Cx, found: Found) -> Ty {
    if found.member.kind != MemberKind::Global {
        return type_of(cx, &Member::Declared(found));
    }
    cx.at(found.file, found.member.node, |def| assigned_name(cx, def)).unwrap_or(Ty::Unknown)
}

/// The declarations of the global `name` the project knows, as seen from the file of `from`: members of
/// its module when it has one, of `_G` otherwise.
pub fn globals<'a>(cx: &Cx<'a>, from: Psi, name: &str) -> Vec<Found<'a>> {
    if cx.alone() {
        return Vec::new();
    }
    let module = module_of(from.src);
    cx.index.named(module.as_deref().unwrap_or(G), name, cx.options.global_name_as_type)
}

/// The members `name` of one class, those of the classes it inherits from included.
pub fn named<'a>(cx: &Cx<'a>, class: &Ty, name: &str) -> Vec<Member<'a>> {
    if let Some(fields) = written(class) {
        return fields.iter().filter(|field| &*field.name == name).cloned().map(Member::Written).collect();
    }
    let Some(lookup) = lookup_name(class).filter(|_| !cx.alone()) else { return Vec::new() };
    let found = cx.index.named(lookup, name, cx.options.global_name_as_type);
    found.into_iter().map(Member::Declared).collect()
}

/// The classes a class inherits from, the nearest first.
pub fn supers(cx: &Cx, class: &Ty) -> Vec<Rc<str>> {
    let (Some(name), false) = (class.class_name(), cx.alone()) else { return Vec::new() };
    let base = match class {
        Ty::Class(class) => match &class.detail {
            Detail::Param(base) => base.as_deref(),
            _ => None,
        },
        _ => None,
    };
    cx.index.supers(name, base)
}

fn accepts(declared: &Ty, key: &Ty) -> bool {
    let fits = |accepted: &Ty| {
        accepted == key
            || *accepted == Ty::STRING && matches!(key, Ty::StringLiteral(_))
            || matches!(accepted, Ty::StringLiteral(_)) && *key == Ty::STRING
    };
    key.is_invalid() || declared.each().iter().any(fits)
}

/// What reading `owner` with a key of type `key` gives through an index signature, `---@field [K] V`, of
/// its classes or of those they inherit from. An unknown key fits every signature.
pub fn indexed(cx: &Cx, owner: &Ty, key: Option<&Ty>) -> Ty {
    let mut result = Ty::Unknown;
    if cx.alone() {
        return result;
    }
    for class in owner.top_classes().into_iter().filter(|class| !class.is_anonymous()) {
        let Some(name) = class.class_name() else { continue };
        for class in std::iter::once(Rc::from(name)).chain(supers(cx, class)) {
            let Some(def) = cx.index.class(&class) else { continue };
            for (declared, value) in &def.signatures {
                if key.is_none_or(|key| accepts(declared, key)) {
                    result.add(value);
                }
            }
        }
    }
    result
}

/// The members of one class and of those it inherits from, as they hide one another (`ClassMemberChain`):
/// each level holds one member per name, the first level is the class itself.
fn chain<'a>(cx: &Cx<'a>, class: &str, base: Option<&str>, open: &mut Vec<Rc<str>>) -> Vec<Vec<Found<'a>>> {
    let index = cx.index;
    if open.iter().any(|name| **name == *class) {
        return Vec::new();
    }
    open.push(class.into());
    let inherited = index.parent(class, base).map_or(Vec::new(), |parent| chain(cx, &parent, None, open));
    let find = |levels: &[Vec<Found<'a>>], name: &str| {
        levels.iter().flatten().find(|found| &*found.member.name == name).copied()
    };
    let mut own: Vec<Found<'a>> = Vec::new();
    let add = |own: &mut Vec<Found<'a>>, found: Found<'a>| {
        let worth = found.member.kind.worth();
        let hidden = find(&inherited, &found.member.name).is_some_and(|parent| {
            let parent = parent.member.kind.worth();
            !(worth > parent || worth == parent && worth > 0)
        });
        if hidden {
            return;
        }
        match own.iter_mut().find(|known| known.member.name == found.member.name) {
            Some(known) if worth > known.member.kind.worth() => *known = found,
            Some(_) => {}
            None => own.push(found),
        }
    };
    for found in index.own(class) {
        add(&mut own, found);
    }
    for other in index.other_names(class, cx.options.global_name_as_type) {
        for found in index.own(&other) {
            add(&mut own, found);
        }
    }
    let extras = index.class(class).map(|def| def.extra_supers.clone()).unwrap_or_default();
    for extra in extras {
        for level in chain(cx, &extra, None, open) {
            for found in level {
                let name = &found.member.name;
                if find(std::slice::from_ref(&own), name).is_none() && find(&inherited, name).is_none() {
                    add(&mut own, found);
                }
            }
        }
    }
    open.pop();
    std::iter::once(own).chain(inherited).collect()
}

fn levels<'a>(cx: &Cx<'a>, class: &Ty) -> Vec<Vec<Found<'a>>> {
    let (Some(name), false) = (class.class_name(), cx.alone()) else { return Vec::new() };
    match class {
        Ty::Class(class) => match &class.detail {
            Detail::Param(Some(base)) => chain(cx, base, None, &mut Vec::new()),
            Detail::Param(None) => Vec::new(),
            _ => chain(cx, name, None, &mut Vec::new()),
        },
        _ => chain(cx, name, None, &mut Vec::new()),
    }
}

/// The members of a class as a completion lists them: its own, then those it inherits; a name comes once,
/// from the nearest class that declares it.
pub fn visible<'a>(cx: &Cx<'a>, class: &Ty) -> Vec<Member<'a>> {
    if let Some(fields) = written(class) {
        return fields.iter().cloned().map(Member::Written).collect();
    }
    let mut listed: Vec<Member<'a>> = Vec::new();
    for found in levels(cx, class).into_iter().flatten() {
        if !listed.iter().any(|member| member.name() == &*found.member.name) {
            listed.push(Member::Declared(found));
        }
    }
    listed
}

/// The member `name` of a class: the one of the nearest class that declares it.
pub fn find<'a>(cx: &Cx<'a>, class: &Ty, name: &str) -> Option<Member<'a>> {
    if let Some(fields) = written(class) {
        return fields.iter().find(|field| &*field.name == name).cloned().map(Member::Written);
    }
    let found = levels(cx, class).into_iter().flatten().find(|found| &*found.member.name == name);
    found.map(Member::Declared)
}

fn aliased(cx: &Cx, ty: &Ty, seen: &RefCell<Vec<Rc<str>>>) -> Ty {
    ty.substitute(&mut |class: &Class| {
        if seen.borrow().contains(&class.name) {
            return None;
        }
        seen.borrow_mut().push(class.name.clone());
        if class.flags & (ANONYMOUS | GLOBAL) != 0 || !matches!(class.detail, Detail::Named) {
            return None;
        }
        cx.index.alias(&class.name).map(|alias| aliased(cx, &alias.ty, seen))
    })
}

/// The type with each alias, `---@alias Name T`, replaced by what it stands for.
pub fn resolve_aliases(cx: &Cx, ty: &Ty) -> Ty {
    if cx.alone() { ty.clone() } else { aliased(cx, ty, &RefCell::default()) }
}
