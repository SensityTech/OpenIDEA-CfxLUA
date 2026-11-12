//! The members of a type: what the index declares for its classes, the fields a table constructor writes,
//! and the fields a type carries itself (`LuaShortNamesManager`, `ClassMemberChain`,
//! `LuaCatsIndexSignatures`, `TyAliasSubstitutor`).

use super::context::{Cx, Psi};
use super::expressions::{assigned_name, global_type, table_of};
use super::{ANONYMOUS, Class, Detail, DocField, FileId, G, GLOBAL, Ty, psi};
use crate::index::{Found, MemberKind, declare};
use crate::lua::Source;
use crate::lua::doc::tags::Visibility;
use crate::lua::tree::{N, Node};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A member of a class.
#[derive(Clone, Debug)]
pub enum Member<'a> {
    /// One a file declares: the index keeps it.
    Declared(Found<'a>),
    /// A field a table constructor writes: it is read in its file.
    Field(TableField),
    /// A field of a table written in a comment: the type carries it.
    Written(DocField),
}

#[derive(Clone, Debug)]
pub struct TableField {
    pub file: FileId,
    /// The `TableField` of the Lua tree.
    pub node: u32,
    pub name: Rc<str>,
}

/// What a field of a table constructor weighs among the declarations of a name: less than a function,
/// more than an assignment.
const TABLE_FIELD: u32 = 800;

impl Member<'_> {
    pub fn name(&self) -> &str {
        match self {
            Member::Declared(found) => &found.member.name,
            Member::Field(field) => &field.name,
            Member::Written(field) => &field.name,
        }
    }

    fn shared_name(&self) -> Rc<str> {
        match self {
            Member::Declared(found) => found.member.name.clone(),
            Member::Field(field) => field.name.clone(),
            Member::Written(field) => field.name.clone(),
        }
    }

    fn worth(&self) -> u32 {
        match self {
            Member::Declared(found) => found.member.kind.worth(),
            Member::Field(_) => TABLE_FIELD,
            Member::Written(_) => MemberKind::DocField.worth(),
        }
    }

    pub fn visibility(&self) -> Visibility {
        match self {
            Member::Declared(found) => found.member.visibility,
            Member::Field(_) | Member::Written(_) => Visibility::Public,
        }
    }

    /// Its type when no file has to be read to tell it: the one its file kept, or the one it is written
    /// with.
    pub fn kept_type(&self) -> Option<Ty> {
        match self {
            Member::Declared(found) => found.member.ty.clone(),
            Member::Field(_) => None,
            Member::Written(field) => Some(field.ty.clone()),
        }
    }

    /// Whether the member is declared as a function: `function a:b()` or `function b()`.
    pub fn is_function(&self) -> bool {
        matches!(self, Member::Declared(found) if matches!(found.member.kind, MemberKind::Method | MemberKind::Function))
    }
}

/// The named fields the table constructor of the class `class` writes, read in its file: nothing when the
/// class is no table, or when its file cannot be read.
fn table_fields(cx: &Cx, class: &str) -> Vec<TableField> {
    let Some((file, offset)) = table_of(class) else { return Vec::new() };
    let fields = cx.read(file, |src| {
        let inner = src.root().tree().node_at(src.root().source(), offset);
        let written = |node: &Node| node.is(N::TableExpr) && node.code_range().0 == offset;
        let table = std::iter::once(inner).chain(inner.ancestors()).find(written)?;
        let named = |field: Node| {
            Some(TableField { file, node: field.id(), name: psi::field_name(src, field)?.as_ref().into() })
        };
        Some(table.children_of(N::TableField).filter_map(named).collect())
    });
    fields.flatten().unwrap_or_default()
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

/// The type of a member: the one its file kept when it declared it, or else what reading the file gives.
pub fn type_of(cx: &Cx, member: &Member) -> Ty {
    match member {
        Member::Written(field) => field.ty.clone(),
        Member::Field(field) => {
            let inferred = cx.at(field.file, field.node, |psi| cx.with_value(0, || cx.infer(psi)));
            inferred.unwrap_or(Ty::Unknown)
        }
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
    let Some(known) = &found.member.ty else {
        return cx.at(found.file, found.member.node, |def| assigned_name(cx, def)).unwrap_or(Ty::Unknown);
    };
    let mut ty = known.clone();
    if !matches!(ty, Ty::Primitive(_)) {
        ty.add(&global_type(cx, &found.member.name, false));
    }
    ty
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
    let mut found = Vec::new();
    for kept_under in cx.index.names(lookup, cx.options.global_name_as_type) {
        let fields = table_fields(cx, &kept_under).into_iter().filter(|field| &*field.name == name);
        found.extend(fields.map(Member::Field));
        found.extend(cx.index.own_named(&kept_under, name).map(Member::Declared));
    }
    found
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

/// The members one class of a chain declares, one per name.
struct Level<'a> {
    class: Rc<str>,
    members: Vec<Member<'a>>,
}

fn find_in<'l, 'a>(levels: &'l [Level<'a>], name: &str) -> Option<&'l Member<'a>> {
    levels.iter().flat_map(|level| &level.members).find(|member| member.name() == name)
}

/// The members one class declares, one per name: the one that weighs most, the first of those.
#[derive(Default)]
struct Own<'a> {
    members: Vec<Member<'a>>,
    places: HashMap<Rc<str>, usize>,
}

impl<'a> Own<'a> {
    fn add(&mut self, inherited: &[Level<'a>], member: Member<'a>) {
        let worth = member.worth();
        let hidden = find_in(inherited, member.name()).is_some_and(|parent| {
            let parent = parent.worth();
            !(worth > parent || worth == parent && worth > 0)
        });
        if hidden {
            return;
        }
        match self.places.get(member.name()) {
            Some(&at) if worth > self.members[at].worth() => self.members[at] = member,
            Some(_) => {}
            None => {
                self.places.insert(member.shared_name(), self.members.len());
                self.members.push(member);
            }
        }
    }
}

/// The members of one class and of those it inherits from, as they hide one another (`ClassMemberChain`):
/// the first level is the class itself, the next ones its parents. Only the names `wanted` are read.
fn chain<'a>(cx: &Cx<'a>, class: &str, open: &mut Vec<Rc<str>>, wanted: &dyn Fn(&str) -> bool) -> Vec<Level<'a>> {
    let index = cx.index;
    if open.iter().any(|name| **name == *class) {
        return Vec::new();
    }
    open.push(class.into());
    let inherited = index.parent(class, None).map_or(Vec::new(), |parent| chain(cx, &parent, open, wanted));
    let mut own = Own::default();
    let others = index.other_names(class, cx.options.global_name_as_type);
    for kept_under in std::iter::once(Rc::from(class)).chain(others) {
        for field in table_fields(cx, &kept_under).into_iter().filter(|field| wanted(&field.name)) {
            own.add(&inherited, Member::Field(field));
        }
        for found in index.own(&kept_under).filter(|found| wanted(&found.member.name)) {
            own.add(&inherited, Member::Declared(found));
        }
    }
    let extras = index.class(class).map(|def| def.extra_supers.clone()).unwrap_or_default();
    for extra in extras {
        for member in chain(cx, &extra, open, wanted).into_iter().flat_map(|level| level.members) {
            if !own.places.contains_key(member.name()) && find_in(&inherited, member.name()).is_none() {
                own.add(&inherited, member);
            }
        }
    }
    open.pop();
    std::iter::once(Level { class: class.into(), members: own.members }).chain(inherited).collect()
}

fn levels<'a>(cx: &Cx<'a>, class: &Ty, wanted: &dyn Fn(&str) -> bool) -> Vec<Level<'a>> {
    match lookup_name(class) {
        Some(name) if !cx.alone() => chain(cx, name, &mut Vec::new(), wanted),
        _ => Vec::new(),
    }
}

/// A member a completion lists, and the class that declares it.
pub struct Listed<'a> {
    pub member: Member<'a>,
    pub class: Ty,
}

/// The members of a class as a completion lists them: its own, then those it inherits; a name comes once,
/// from the nearest class that declares it.
pub fn visible<'a>(cx: &Cx<'a>, class: &Ty) -> Vec<Listed<'a>> {
    visible_named(cx, class, &|_| true)
}

/// The members of a class whose name is `wanted`, as a completion lists them: a class with thousands of
/// members, as the globals are, is not listed whole.
pub fn visible_named<'a>(cx: &Cx<'a>, class: &Ty, wanted: &dyn Fn(&str) -> bool) -> Vec<Listed<'a>> {
    if let Some(fields) = written(class) {
        let listed = |field: &DocField| Listed { member: Member::Written(field.clone()), class: class.clone() };
        return fields.iter().filter(|field| wanted(&field.name)).map(listed).collect();
    }
    let mut listed: Vec<Listed<'a>> = Vec::new();
    let mut names: HashSet<Rc<str>> = HashSet::new();
    for level in levels(cx, class, wanted) {
        let declaring = if class.class_name() == Some(&*level.class) { class.clone() } else { Ty::named(&level.class) };
        for member in level.members {
            if names.insert(member.shared_name()) {
                listed.push(Listed { member, class: declaring.clone() });
            }
        }
    }
    listed
}

/// The member `name` of a class: the one of the nearest class that declares it.
pub fn find<'a>(cx: &Cx<'a>, class: &Ty, name: &str) -> Option<Member<'a>> {
    if let Some(fields) = written(class) {
        return fields.iter().find(|field| &*field.name == name).cloned().map(Member::Written);
    }
    find_in(&levels(cx, class, &|member| member == name), name).cloned()
}

/// Whether a value of the class `class` can stand where `other` is asked for: it is that class, or
/// inherits from it. Nothing is known to fit an unknown type when `strict`.
pub fn is_subtype(cx: &Cx, class: &Ty, other: &Ty, strict: bool) -> bool {
    match other {
        Ty::Unknown => !strict,
        Ty::Union(members) => members.iter().any(|member| is_subtype(cx, class, member, strict)),
        _ if class == other => true,
        _ if !matches!(class, Ty::Class(_)) => false,
        _ => *other == Ty::TABLE || supers(cx, class).iter().any(|parent| Ty::named(parent) == *other),
    }
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
