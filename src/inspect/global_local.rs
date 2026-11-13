//! `GlobalNameCanbeLocal`: the first assignment of a global that nothing outside its file uses.

use crate::feature::{Fix, Problem};
use crate::inspect::GLOBAL_LOCAL;
use crate::lua::Source;
use crate::lua::scope::DeclKind;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::members::module_of;
use openidea_plugin::Severity;
use std::collections::HashMap;

#[cfg(test)]
#[path = "global_local_tests.rs"]
mod tests;

/// `used_elsewhere` tells whether another file or a library reads or declares a global name.
pub fn problems(src: &Source, used_elsewhere: &dyn Fn(&str) -> bool) -> Vec<Problem> {
    if module_of(src).is_some() {
        return Vec::new();
    }
    let root = src.root();
    let first = first_declarations(src);
    let candidates: Vec<(Node, &str)> = src
        .scopes()
        .decls()
        .iter()
        .filter(|decl| decl.kind == DeclKind::Global)
        .map(|decl| root.tree().node(root.source(), decl.node))
        .filter_map(|target| Some((target, target.token(T::Id)?.text())))
        .filter(|(target, name)| first.get(name).is_none_or(|first| *first == target.id()) && !used_elsewhere(name))
        .collect();
    let can_be_local: Vec<u32> = candidates.iter().map(|(target, _)| target.id()).collect();
    candidates.iter().map(|(target, name)| problem(*target, name, &can_be_local)).collect()
}

fn problem(target: Node, name: &str, can_be_local: &[u32]) -> Problem {
    let (start, end) = target.code_range();
    let found =
        Problem::new(GLOBAL_LOCAL.code, Severity::Warning, start, end, format!("Global name \"{name}\" can be local"));
    let Some(statement) = target.parent().and_then(Node::parent).filter(|outer| outer.is(N::AssignStat)) else {
        return found;
    };
    let mut targets = statement.child(N::VarList).into_iter().flat_map(Node::children);
    if !targets.all(|other| can_be_local.contains(&other.id())) {
        return found;
    }
    found.fix(Fix::new("Append \"local\"").insert(statement.code_range().0, "local "))
}

/// The first declaration of each global the JetBrains index holds: what a use nothing in sight explains reads.
fn first_declarations(src: &Source) -> HashMap<&str, u32> {
    let scopes = src.scopes();
    let mut first = HashMap::new();
    for node in src.root().descendants() {
        let declares = match node.kind() {
            N::FuncDef => true,
            N::NameExpr => {
                let statement = node.parent().and_then(Node::parent).filter(|outer| outer.is(N::AssignStat));
                statement.is_some_and(indexed) && scopes.resolve(node).is_none_or(|decl| decl.kind == DeclKind::Global)
            }
            _ => false,
        };
        if declares && let Some(name) = node.token(T::Id) {
            first.entry(name.text()).or_insert(node.id());
        }
    }
    first
}

/// Whether the JetBrains index holds `statement`: a loop around it drops it, a named function or a closure keeps it.
fn indexed(statement: Node) -> bool {
    let mut at = statement;
    loop {
        let Some(mut outer) = at.parent() else { return false };
        if outer.is(N::Block) {
            let Some(above) = outer.parent() else { return false };
            outer = above;
        }
        match outer.kind() {
            N::File => return true,
            N::DoStat | N::IfStat | N::LocalFuncDef => at = outer,
            N::FuncBody => match outer.parent() {
                Some(owner) if matches!(owner.kind(), N::FuncDef | N::ClassMethodDef | N::ClosureExpr) => return true,
                Some(owner) if owner.is(N::LocalFuncDef) => at = owner,
                _ => return false,
            },
            _ => return false,
        }
    }
}
