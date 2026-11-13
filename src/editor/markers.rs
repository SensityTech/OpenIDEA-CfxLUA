//! The gutter markers of the JetBrains plugin (`LuaLineMarkerProvider`): a method that overrides another or
//! is overridden, a class that has subclasses, a recursive call, a call that is returned.

use super::definition::place;
use super::target::{self, Target};
use crate::index::{ClassDef, Found, MemberKind};
use crate::lua::Source;
use crate::lua::scope::DeclKind;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Elem, Psi};
use crate::ty::{FileId, psi};
use openidea_plugin::Location;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

pub const OVERRIDES: &str = "gutter.overrides";
pub const CLASSES: &str = "gutter.classes";
pub const RECURSIVE_CALLS: &str = "gutter.recursiveCalls";
pub const TAIL_CALLS: &str = "gutter.tailCalls";

const OVERRIDING: &str = "overriding";
const OVERRIDDEN: &str = "overridden";
const CLASS: &str = "icons/markers/class.svg";
const RECURSIVE: &str = "icons/markers/recursive.svg";
const TAIL_CALL: &str = "icons/markers/tailCall.svg";

/// How many names a tooltip lists.
const LISTED: usize = 5;

#[derive(Clone, Copy, Debug)]
pub struct Wanted {
    pub overrides: bool,
    pub classes: bool,
    pub recursive_calls: bool,
    pub tail_calls: bool,
    /// Whether the project has been read to the end: who inherits from a class is not known before.
    pub project_read: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    /// A byte of the line the marker stands on.
    pub at: usize,
    pub icon: &'static str,
    pub tooltip: String,
    pub target: Option<Location>,
}

/// The markers of a file, one per line: the first one found there, in the order they are looked for.
struct Markers<'a> {
    src: &'a Source,
    by_line: BTreeMap<usize, Marker>,
}

impl Markers<'_> {
    fn put(&mut self, at: usize, icon: &'static str, tooltip: String, target: Option<Location>) {
        self.by_line.entry(self.src.lines.line_of(at)).or_insert(Marker { at, icon, tooltip, target });
    }
}

fn listed(names: &[Rc<str>]) -> String {
    let mut shown: Vec<&str> = names.iter().take(LISTED).map(|name| &**name).collect();
    if names.len() > LISTED {
        shown.push("…");
    }
    shown.join(", ")
}

/// The classes that name each class as a parent, among those the file sees.
pub(crate) fn children<'a>(cx: &Cx<'a>) -> HashMap<Rc<str>, Vec<(FileId, &'a ClassDef)>> {
    let mut children: HashMap<Rc<str>, Vec<(FileId, &ClassDef)>> = HashMap::new();
    for (file, class) in cx.index.all_classes().filter(|(file, _)| cx.files.visible(*file)) {
        for parent in class.super_name.iter().chain(&class.extra_supers) {
            children.entry(parent.clone()).or_default().push((file, class));
        }
    }
    children
}

/// The classes that inherit from `class`, the nearest first (`LuaClassInheritorsSearch`).
pub(crate) fn inheritors<'a>(
    children: &HashMap<Rc<str>, Vec<(FileId, &'a ClassDef)>>,
    class: &str,
) -> Vec<(FileId, &'a ClassDef)> {
    let mut seen: HashSet<&str> = HashSet::from([class]);
    let mut found: Vec<(FileId, &ClassDef)> = Vec::new();
    let mut next = 0;
    let mut parent: &str = class;
    loop {
        for (file, child) in children.get(parent).into_iter().flatten() {
            if seen.insert(&child.name) {
                found.push((*file, child));
            }
        }
        let Some((_, child)) = found.get(next) else { return found };
        parent = &child.name;
        next += 1;
    }
}

/// The method `name` a class declares itself, under its name or under the table it annotates.
pub(crate) fn own_method<'a>(cx: &Cx<'a>, class: &str, name: &str) -> Option<Found<'a>> {
    let others = cx.index.other_names(class, cx.options.global_name_as_type);
    let mut kept_under = std::iter::once(Rc::from(class)).chain(others);
    kept_under.find_map(|kept| {
        cx.index
            .own_named(&kept, name)
            .find(|found| found.member.kind == MemberKind::Method && cx.files.visible(found.file))
    })
}

fn declared(found: Found) -> Target {
    Target::Elem(Elem { file: found.file, node: found.member.node, doc: false })
}

/// The class a method statement declares its method on, when the project declares that class.
pub(crate) fn class_of(cx: &Cx, method: Psi) -> Option<Rc<str>> {
    let owner = psi::method_owner(method.node)?;
    let ty = cx.with_value(0, || cx.infer(method.at(owner)));
    let named = ty.top_classes().into_iter().filter_map(|class| class.class_name());
    named.map(Rc::from).find(|name: &Rc<str>| cx.index.class(name).is_some())
}

fn inheritance(cx: &Cx, file: FileId, src: &Source, wanted: Wanted, markers: &mut Markers) {
    let methods: Vec<Node> = match wanted.overrides {
        true => src.root().descendants().filter(|node| node.is(N::ClassMethodDef)).collect(),
        false => Vec::new(),
    };
    let classes = cx.index.file(file).map_or(&[][..], |declared| declared.classes.as_slice());
    if methods.is_empty() && (classes.is_empty() || !wanted.classes) {
        return;
    }
    let children = children(cx);
    for method in methods {
        let (Some(name), Some(class)) = (psi::function_name(method), class_of(cx, Psi { file, src, node: method }))
        else {
            continue;
        };
        let Some(at) = method.child(N::ClassMethodName).and_then(|named| named.token(T::Id)) else { continue };
        let parents = cx.index.supers(&class, None);
        let above = parents.iter().find_map(|parent| Some((parent, own_method(cx, parent, name)?)));
        let below: Vec<(Rc<str>, Found)> = inheritors(&children, &class)
            .into_iter()
            .filter_map(|(_, child)| Some((child.name.clone(), own_method(cx, &child.name, name)?)))
            .collect();
        let overridden = (!below.is_empty()).then(|| {
            let names: Vec<Rc<str>> = below.iter().map(|(child, _)| child.clone()).collect();
            format!("Is overridden in {}", listed(&names))
        });
        match (above, overridden) {
            (Some((parent, found)), overridden) => {
                let mut tooltip = format!("Overrides function in {parent}");
                tooltip.extend(overridden.map(|also| format!("\n{also}")));
                markers.put(at.start(), OVERRIDING, tooltip, place(cx, &declared(found)));
            }
            (None, Some(tooltip)) => markers.put(at.start(), OVERRIDDEN, tooltip, place(cx, &declared(below[0].1))),
            (None, None) => {}
        }
    }
    if !wanted.classes {
        return;
    }
    let docs = src.docs();
    for class in classes.iter().filter(|class| (class.node as usize) < docs.tree().len()) {
        let tag = docs.tree().node(docs.source(), class.node);
        let Some(at) = tag.token(T::Id).filter(|_| tag.is(N::DocTagClass)) else { continue };
        let below = inheritors(&children, &class.name);
        match below.first() {
            Some((child_file, child)) => {
                let names: Vec<Rc<str>> = below.iter().map(|(_, child)| child.name.clone()).collect();
                let tooltip = format!("Class {}\nIs subclassed by {}", class.name, listed(&names));
                let to = Target::Elem(Elem { file: *child_file, node: child.node, doc: true });
                markers.put(at.start(), OVERRIDDEN, tooltip, place(cx, &to));
            }
            None => markers.put(at.start(), CLASS, format!("Class {}", class.name), None),
        }
    }
}

/// Whether a call names the function it is written in: a local function by its scope, a global one by its
/// name, a method by what the engine resolves its name to.
fn is_recursive(cx: &Cx, file: FileId, src: &Source, call: Node) -> bool {
    let Some(callee) = psi::prefix(call) else { return false };
    let Some(name) = callee.token(T::Id).map(|id| id.text()) else { return false };
    let around = || call.ancestors().filter(|outer| psi::is_function(*outer));
    match callee.kind() {
        N::NameExpr => match src.scopes().resolve(callee) {
            Some(decl) if decl.kind != DeclKind::Global => around().any(|outer| outer.id() == decl.node),
            _ => around().any(|outer| outer.is(N::FuncDef) && psi::function_name(outer) == Some(name)),
        },
        N::IndexExpr => {
            let Some(method) =
                around().find(|outer| outer.is(N::ClassMethodDef) && psi::function_name(*outer) == Some(name))
            else {
                return false;
            };
            let here = Elem { file, node: method.id(), doc: false };
            matches!(target::named(cx, Psi { file, src, node: callee }), Some(Target::Elem(elem)) if elem == here)
        }
        _ => false,
    }
}

fn calls(cx: &Cx, file: FileId, src: &Source, wanted: Wanted, markers: &mut Markers) {
    // A line shows one marker: a recursive call says more than a call that is returned.
    if wanted.recursive_calls {
        for call in src.root().descendants().filter(|node| node.is(N::CallExpr)) {
            if is_recursive(cx, file, src, call) {
                markers.put(call.code_range().0, RECURSIVE, "Recursive call".to_owned(), None);
            }
        }
    }
    if wanted.tail_calls {
        for returned in src.root().descendants().filter(|node| node.is(N::ReturnStat)) {
            let mut values = returned.child(N::ExprList).into_iter().flat_map(Node::exprs);
            if values.any(|value| value.is(N::CallExpr)) {
                markers.put(returned.code_range().0, TAIL_CALL, "Tail call".to_owned(), None);
            }
        }
    }
}

/// The markers of `src`, the file `file`, in the order of its lines.
pub fn markers(cx: &Cx, file: FileId, src: &Source, wanted: Wanted) -> Vec<Marker> {
    let mut found = Markers { src, by_line: BTreeMap::new() };
    if wanted.project_read && (wanted.overrides || wanted.classes) {
        inheritance(cx, file, src, wanted, &mut found);
    }
    if wanted.recursive_calls || wanted.tail_calls {
        calls(cx, file, src, wanted, &mut found);
    }
    found.by_line.into_values().collect()
}
