//! The intentions of the JetBrains plugin (`codeInsight/intention`, `editor/surroundWith`): small rewrites
//! offered where the caret stands, or around the statements that are selected.

mod compute;

#[cfg(test)]
mod tests;

use crate::editor::hover::shown;
use crate::feature::Fix;
use crate::lua::Source;
use crate::lua::doc::tags::Comment;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::psi::{self, Literal};
use crate::ty::{FileId, Ty};

const VISIBILITIES: [&str; 3] = ["private", "protected", "public"];
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

/// The type an annotation writes for `node`: what the engine infers, `any` when it knows nothing.
fn written(cx: &Cx, file: FileId, src: &Source, node: Node) -> String {
    let ty = cx.with_value(0, || cx.infer(Psi { file, src, node }));
    shown(cx, &ty)
}

/// The statement, or the table field, a comment above documents `node` through.
fn documented(node: Node) -> Option<Node> {
    std::iter::once(node).chain(node.ancestors()).find(|owner| owner.kind().is_statement() || owner.is(N::TableField))
}

/// The comment above `owner`: not one written behind it on its line.
fn comment_above<'a>(src: &'a Source, owner: Node) -> Option<Comment<'a>> {
    src.comment_of(owner).filter(|comment| comment.0.range().0 <= owner.code_range().0)
}

/// A line added to the documentation of `owner`: after its comment, or above it (`LuaCommentUtil.insertTemplate`).
fn documenting(src: &Source, owner: Node, title: &str, lines: &[String]) -> Option<Fix> {
    let start = owner.code_range().0;
    let indent = indent_of(src, start);
    let fix = Fix::new(title);
    match src.comment_of(owner) {
        Some(comment) if comment.0.range().0 > start => None,
        Some(comment) => {
            let added: String = lines.iter().map(|line| format!("\n{indent}{line}")).collect();
            Some(fix.insert(comment.0.code_range().1, added))
        }
        None => {
            let added: String = lines.iter().map(|line| format!("{line}\n{indent}")).collect();
            Some(fix.insert(start, added))
        }
    }
}

/// The function statement whose header holds the caret (`FunctionIntention`): a closure has no
/// documentation of its own, and inside the body the caret is about something else.
fn function_header<'a>(src: &'a Source, at: usize) -> Option<Node<'a>> {
    let owner = at_caret(src, at, psi::is_function).filter(|owner| !owner.is(N::ClosureExpr))?;
    let (start, end) = owner.child(N::FuncBody)?.code_range();
    (at < start || at > end).then_some(owner)
}

/// The rewrites that ask the engine for a type: the annotations of a parameter, of a local and of a
/// function, the visibility of a method, a field kept from a parameter.
pub fn annotations(cx: &Cx, file: FileId, src: &Source, at: usize) -> Vec<Fix> {
    let mut found = Vec::new();
    let parameter = at_caret(src, at, |node| node.is(N::ParamNameDef));
    if let Some(parameter) = parameter {
        found.extend(parameter_annotation(cx, file, src, parameter));
        found.extend(field_from_parameter(src, parameter));
    }
    if let Some(local) = at_caret(src, at, |node| node.is(N::LocalDef)) {
        found.extend(type_annotation(cx, file, src, local));
    }
    if let Some(function) = function_header(src, at) {
        found.extend(return_annotation(cx, file, src, function));
        found.extend(function_doc(cx, file, src, function));
        found.extend(visibility(src, function));
    }
    found
}

/// `---@param name type` (`CreateParameterAnnotationIntention`).
fn parameter_annotation(cx: &Cx, file: FileId, src: &Source, parameter: Node) -> Option<Fix> {
    let owner = documented(parameter)?;
    let name = parameter.code();
    if comment_above(src, owner).is_some_and(|comment| comment.param(name).is_some()) {
        return None;
    }
    let line = format!("---@param {name} {}", written(cx, file, src, parameter));
    documenting(src, owner, "Create parameter annotation", &[line])
}

/// `---@type type` above a `local` (`CreateTypeAnnotationIntention`).
fn type_annotation(cx: &Cx, file: FileId, src: &Source, local: Node) -> Option<Fix> {
    if comment_above(src, local).is_some_and(|comment| comment.tag(N::DocTagType).is_some()) {
        return None;
    }
    let name = local.child(N::NameList)?.child(N::NameDef)?;
    let line = format!("---@type {}", written(cx, file, src, name));
    documenting(src, local, "Create type annotation", &[line])
}

/// `---@return type` (`CreateFunctionReturnAnnotationIntention`).
fn return_annotation(cx: &Cx, file: FileId, src: &Source, function: Node) -> Option<Fix> {
    if comment_above(src, function).is_some_and(|comment| comment.tag(N::DocTagReturn).is_some()) {
        return None;
    }
    let returned = match cx.infer(Psi { file, src, node: function }) {
        Ty::Function(function) => shown(cx, &cx.ret(&function.main.ret)),
        _ => shown(cx, &Ty::Unknown),
    };
    documenting(src, function, "Create return annotation", &[format!("---@return {returned}")])
}

/// The name of the function and a `@param` for each of its parameters (`CreateFunctionDocIntention`).
fn function_doc(cx: &Cx, file: FileId, src: &Source, function: Node) -> Option<Fix> {
    if src.comment_of(function).is_some() {
        return None;
    }
    let mut lines = vec![format!("---{}", psi::function_name(function)?)];
    for parameter in psi::params(function) {
        lines.push(format!("---@param {} {}", parameter.code(), written(cx, file, src, parameter)));
    }
    documenting(src, function, "Create LuaDoc", &lines)
}

/// `---@private`, `---@protected` or `---@public` on a method that says none (`SetVisibilityIntention`).
fn visibility(src: &Source, method: Node) -> Vec<Fix> {
    let stated = |comment: Comment| comment.tag(N::DocAccessModifier).is_some();
    if !method.is(N::ClassMethodDef) || comment_above(src, method).is_some_and(stated) {
        return Vec::new();
    }
    let set = |word: &str| documenting(src, method, &format!("Set visibility: {word}"), &[format!("---@{word}")]);
    VISIBILITIES.into_iter().filter_map(set).collect()
}

/// `self.name = name` at the start of a method, for one of its parameters
/// (`CreateFieldFromParameterIntention`).
fn field_from_parameter(src: &Source, parameter: Node) -> Option<Fix> {
    let body = parameter.parent().filter(|body| body.is(N::FuncBody))?;
    let method = body.parent().filter(|method| method.is(N::ClassMethodDef) && !psi::is_static(*method))?;
    let closed = body.token(T::RParen)?.end();
    let name = parameter.code();
    let outer = indent_of(src, method.code_range().0);
    let step = if src.text.contains("\n\t") { "\t" } else { "    " };
    let assigned = format!("\n{outer}{step}self.{name} = {name}");
    let fix = Fix::new("Create field for parameter");
    // What follows the parameters on their line moves below the new statement.
    let rest = &src.text[closed..];
    let gap = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let same_line = !rest[gap..].starts_with(['\n', '\r']) && !rest[gap..].is_empty();
    Some(match same_line {
        true => fix.replace(closed, closed + gap, format!("{assigned}\n{outer}")),
        false => fix.insert(closed, assigned),
    })
}
