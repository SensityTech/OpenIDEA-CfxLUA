//! The intentions of the JetBrains plugin (`codeInsight/intention`, `editor/surroundWith`): small rewrites
//! offered where the caret stands, or around the statements that are selected.

mod compute;

#[cfg(test)]
mod tests;

use crate::feature::Fix;
use crate::lua::Source;
use crate::lua::tree::{N, Node};
use crate::ty::psi::{self, Literal};

const REGIONS: [(&str, &str, &str); 2] =
    [("Lua Region --region", "region", "endregion"), ("Lua Region --{{{", "{{{", "}}}")];

/// The innermost node `wanted` keeps among those that hold the byte `at`.
fn holding<'a>(src: &'a Source, at: usize, wanted: impl Fn(Node) -> bool) -> Option<Node<'a>> {
    let root = src.root();
    let inner = root.tree().node_at(root.source(), at.min(src.text.len()));
    std::iter::once(inner).chain(inner.ancestors()).find(|node| wanted(*node))
}

/// The same with the caret at `at`: the node it stands in, or else the one it stands right after.
fn at_caret<'a>(src: &'a Source, at: usize, wanted: impl Fn(Node) -> bool + Copy) -> Option<Node<'a>> {
    holding(src, at, wanted).or_else(|| holding(src, at.checked_sub(1)?, wanted))
}

/// What the line of `at` starts with before its first character that is no white space.
fn indent_of(src: &Source, at: usize) -> &str {
    let line = &src.text[src.text[..at].rfind('\n').map_or(0, |start| start + 1)..at];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The rewrites that read nothing but the tree: the parentheses of a call, a boolean, a constant.
pub fn rewrites(src: &Source, at: usize) -> Vec<Fix> {
    let mut found = Vec::new();
    found.extend(call_parentheses(src, at));
    found.extend(inverted_boolean(src, at));
    found.extend(constant(src, at));
    found
}

/// `f "x"` and `f("x")` are the same call (`AppendCallParenIntention`, `RemoveCallParenIntention`).
fn call_parentheses(src: &Source, at: usize) -> Option<Fix> {
    let call = at_caret(src, at, |node| node.is(N::CallExpr))?;
    let callee_end = call.expr()?.code_range().1;
    if let Some(bare) = call.child(N::SingleArg) {
        let (start, end) = bare.code_range();
        return Some(Fix::new("Append call paren").replace(callee_end, start, "(").insert(end, ")"));
    }
    let list = call.child(N::ListArgs)?;
    let mut args = list.exprs();
    let only = args.next().filter(|_| args.next().is_none())?;
    let bare = only.is(N::TableExpr) || psi::is_string(only);
    bare.then(|| Fix::new("Remove call paren").replace(callee_end, list.code_range().1, format!(" {}", only.code())))
}

/// `true` for `false` (`InvertBooleanIntention`).
fn inverted_boolean(src: &Source, at: usize) -> Option<Fix> {
    let literal = at_caret(src, at, |node| node.is(N::LiteralExpr) && psi::literal_kind(node) == Literal::Bool)?;
    let (start, end) = literal.code_range();
    let other = if literal.code() == "true" { "false" } else { "true" };
    Some(Fix::new("Invert boolean value").replace(start, end, other))
}

/// The value of an operation between constants, in its place (`ComputeConstantValueIntention`): offered with
/// the caret on its operator.
fn constant(src: &Source, at: usize) -> Option<Fix> {
    let operation = at_caret(src, at, |node| node.kind().is_expr()).filter(|expr| expr.is(N::BinaryExpr))?;
    let value = compute::value(src, operation)?;
    let (start, end) = operation.code_range();
    let title = format!("Compute constant value of {}", operation.code());
    Some(Fix::new(title).replace(start, end, value))
}

/// Two comments mark a region of the statements that are selected (`LuaSurroundDescriptor`).
pub fn surrounds(src: &Source, start: usize, end: usize) -> Vec<Fix> {
    let Some((first, last)) = selected_statements(src, start, end) else { return Vec::new() };
    let indent = indent_of(src, first);
    let region = |(title, open, close): (&str, &str, &str)| {
        Fix::new(format!("Surround with: {title}"))
            .insert(first, format!("--{open} description\n{indent}"))
            .insert(last, format!("\n{indent}--{close}"))
    };
    REGIONS.into_iter().map(region).collect()
}

/// Where the statements a selection covers start and end: those of one block, from the one that holds its
/// first character to the one that holds its last.
fn selected_statements(src: &Source, start: usize, end: usize) -> Option<(usize, usize)> {
    let text = src.text.as_str();
    let selected = text.get(start..end)?;
    let from = start + (selected.len() - selected.trim_start().len());
    let to = end - (selected.len() - selected.trim_end().len());
    if from >= to {
        return None;
    }
    let statement = |at: usize| holding(src, at, |node| node.kind().is_statement());
    let (mut first, mut last) = (statement(from)?, statement(to - 1)?);
    // The statements of the same block: the deeper one is replaced by the statement around it.
    let depth = |node: Node| node.ancestors().count();
    while depth(first) > depth(last) {
        first = first.ancestors().find(|outer| outer.kind().is_statement())?;
    }
    while depth(last) > depth(first) {
        last = last.ancestors().find(|outer| outer.kind().is_statement())?;
    }
    while first.parent() != last.parent() {
        first = first.ancestors().find(|outer| outer.kind().is_statement())?;
        last = last.ancestors().find(|outer| outer.kind().is_statement())?;
    }
    Some((first.range().0, last.code_range().1))
}
