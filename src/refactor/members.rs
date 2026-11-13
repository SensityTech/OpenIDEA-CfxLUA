//! A member renamed where the type engine, not the syntax, tells what it is: its uses through a value
//! (`shop:open()`), and the methods that override it or that it overrides (`ReferencesSearch`,
//! `RenameLuaMethodProcessor`).

use crate::editor::markers::{children, class_of, inheritors, own_method};
use crate::editor::target::{self, Target, token_at};
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::refactor::edges::{is_local, literal_name};
use crate::ty::context::{Cx, Elem};
use crate::ty::{FileId, psi};

/// A member of the project, and what a rename of it reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// What the name stands for, then the methods that override it and those it overrides.
    pub declarations: Vec<Elem>,
}

/// Where the name of a member is written at `at`: after a `.` or a `:`, as the name of a method statement or
/// of a field of a table, as the string that stands for one of them, or as the name of a `---@field`.
pub fn name_at(src: &Source, at: usize) -> Option<(usize, usize)> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let index = token_at(tokens, at, |kind| matches!(kind, T::Id | T::String | T::DocComment))?;
    let node = tree.node(root.source(), tree.node_of_token(index));
    match tokens.kind(index) {
        T::Id => {
            let member = matches!(node.kind(), N::IndexExpr | N::ClassMethodName | N::TableField);
            member.then(|| (tokens.start(index), tokens.end(index)))
        }
        T::String => {
            let parent = node.parent()?;
            let keys = match parent.kind() {
                N::IndexExpr => psi::key_expr(parent) == Some(node),
                N::TableField => parent.has(T::LBrack) && parent.expr() == Some(node),
                _ => false,
            };
            literal_name(src, tokens.start(index)).filter(|_| keys && psi::is_string(node))
        }
        _ => {
            let docs = src.docs();
            let (tree, tokens) = (docs.tree(), docs.tree().tokens());
            let index = token_at(tokens, at, |kind| kind == T::Id)?;
            let field = tree.node(docs.source(), tree.node_of_token(index)).is(N::DocTagField);
            field.then(|| (tokens.start(index), tokens.end(index)))
        }
    }
}

/// Whether the member named at `at` is reached from a name a library declares: `string.format`,
/// `Citizen.Wait`, what a native returns.
pub fn from_library(src: &Source, at: usize, library: &dyn Fn(&str) -> bool) -> bool {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let Some(index) = token_at(tokens, at, |kind| matches!(kind, T::Id | T::String)) else { return false };
    let named = tree.node(root.source(), tree.node_of_token(index));
    let mut reached = std::iter::once(named).chain(named.ancestors()).find(|node| node.is(N::IndexExpr));
    while let Some(expr) = reached.filter(|expr| !expr.is(N::NameExpr)) {
        reached = psi::prefix(expr);
    }
    reached.is_some_and(|name| !is_local(src, name) && library(name.code()))
}

/// The member whose name is written at `at` in `src`, the file `file`, and what a rename of it reaches.
/// Nothing for what a library declares, nor for a name nothing declares.
pub fn at(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Member> {
    let (start, end) = name_at(src, at)?;
    let name = src.text.get(start..end)?;
    let named = target::at(cx, file, src, start)?;
    let Target::Elem(declared) = named.target else { return None };
    if cx.files.is_library(declared.file) || !declares(cx, declared) {
        return None;
    }
    Some(Member { name: name.to_owned(), declarations: related(cx, declared, name) })
}

/// Whether what a member resolved to declares it: an index that nothing explains resolves to itself.
fn declares(cx: &Cx, found: Elem) -> bool {
    let read = |node: Node| !node.is(N::IndexExpr) || is_target(node);
    found.doc || cx.at(found.file, found.node, |psi| read(psi.node)).unwrap_or(false)
}

/// Whether an expression is written on the left of an assignment.
fn is_target(expr: Node) -> bool {
    let list = expr.parent().filter(|list| list.is(N::VarList));
    list.and_then(Node::parent).is_some_and(|statement| statement.is(N::AssignStat))
}

/// A declaration, then the methods of that name of the classes that inherit from its class and of those its
/// class inherits from: only a method statement has them, a field of the same name does not follow.
fn related(cx: &Cx, declared: Elem, name: &str) -> Vec<Elem> {
    let mut all = vec![declared];
    let class = match declared.doc {
        true => None,
        false => cx.at(declared.file, declared.node, |method| {
            method.node.is(N::ClassMethodDef).then(|| class_of(cx, method)).flatten()
        }),
    };
    let Some(class) = class.flatten() else { return all };
    let children = children(cx);
    let below: Vec<_> = inheritors(&children, &class).into_iter().map(|(_, child)| child.name.clone()).collect();
    for kin in below.iter().chain(&cx.index.supers(&class, None)) {
        let Some(found) = own_method(cx, kin, name) else { continue };
        let method = Elem { file: found.file, node: found.member.node, doc: false };
        if !all.contains(&method) {
            all.push(method);
        }
    }
    all
}

/// Where `src`, the file `file`, writes the name of `member` as that member: what the engine resolves to one
/// of its declarations, those written in this file included. Sorted.
pub fn uses(cx: &Cx, file: FileId, src: &Source, member: &Member) -> Vec<(usize, usize)> {
    let resolved = |at: usize| match target::at(cx, file, src, at).map(|named| named.target) {
        Some(Target::Elem(elem)) => member.declarations.contains(&elem),
        _ => false,
    };
    let mut found = Vec::new();
    let root = src.root();
    let tokens = root.tree().tokens();
    for index in 0..tokens.len() {
        let start = tokens.start(index);
        let spelled = |(from, to): &(usize, usize)| src.text.get(*from..*to) == Some(member.name.as_str());
        let written = match tokens.kind(index) {
            T::Id => Some((start, tokens.end(index))).filter(spelled),
            T::String => literal_name(src, start).filter(spelled),
            _ => None,
        };
        let named = written.and_then(|(from, _)| name_at(src, from));
        found.extend(named.filter(|(from, _)| resolved(*from)));
    }
    if src.text.contains('@') {
        let docs = src.docs();
        let tokens = docs.tree().tokens();
        for index in (0..tokens.len()).filter(|index| tokens.kind(*index) == T::Id) {
            let (from, to) = (tokens.start(index), tokens.end(index));
            if tokens.text(docs.source(), index) == member.name && name_at(src, from).is_some() && resolved(from) {
                found.push((from, to));
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(test)]
#[path = "members_tests.rs"]
mod tests;
