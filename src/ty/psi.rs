//! What the engine reads of the syntax tree (`PsiExtension.kt`, `LuaPsiImplUtil.kt`).

use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Literal {
    String,
    Bool,
    Number,
    Nil,
    Varargs,
    Hash,
    Unknown,
}

pub fn literal_kind(node: Node) -> Literal {
    match node.first_token().map(|token| token.kind()) {
        Some(T::String) => Literal::String,
        Some(T::True | T::False) => Literal::Bool,
        Some(T::Nil) => Literal::Nil,
        Some(T::Number) => Literal::Number,
        Some(T::Ellipsis) => Literal::Varargs,
        Some(T::HashString) => Literal::Hash,
        _ => Literal::Unknown,
    }
}

pub fn is_string(node: Node) -> bool {
    node.is(N::LiteralExpr) && literal_kind(node) == Literal::String
}

/// The value of a string literal.
pub fn string_value(src: &Source, node: Node) -> Option<String> {
    let token = node.first_token().filter(|_| is_string(node))?;
    src.string_at(src.tok_of(token.index()))
}

/// The expression an index or a call applies to.
pub fn prefix(node: Node) -> Option<Node> {
    node.expr()
}

pub fn is_brack(node: Node) -> bool {
    node.has(T::LBrack)
}

pub fn is_colon(node: Node) -> bool {
    node.has(T::Colon)
}

/// The expression between the brackets of `t[key]`.
pub fn key_expr(node: Node) -> Option<Node> {
    node.exprs().nth(1).filter(|_| is_brack(node))
}

/// The name an index reads: `t.name`, `t:name` or `t['name']`.
pub fn member_name<'a>(src: &'a Source, node: Node<'a>) -> Option<Cow<'a, str>> {
    if let Some(id) = node.token(T::Id) {
        return Some(Cow::Borrowed(id.text()));
    }
    key_expr(node).and_then(|key| string_value(src, key)).map(Cow::Owned)
}

/// The arguments of a call.
pub fn args(call: Node) -> Vec<Node> {
    match call.children().find(|child| matches!(child.kind(), N::ListArgs | N::SingleArg)) {
        Some(list) => list.exprs().collect(),
        None => Vec::new(),
    }
}

/// `a:b()`.
pub fn is_colon_call(call: Node) -> bool {
    prefix(call).is_some_and(|callee| callee.is(N::IndexExpr) && is_colon(callee))
}

/// The first argument of a call when it is a string literal.
pub fn first_string_arg(call: Node) -> Option<Node> {
    args(call).first().copied().filter(|arg| is_string(*arg))
}

/// The receiver of a call: what stands before the name of the function.
pub fn receiver(call: Node) -> Option<Node> {
    prefix(call).filter(|callee| callee.is(N::IndexExpr)).and_then(prefix)
}

/// The name of a function statement, the last one of `function a.b:c()`.
pub fn function_name(node: Node<'_>) -> Option<&str> {
    let named = if node.is(N::ClassMethodDef) { node.child(N::ClassMethodName)? } else { node };
    named.token(T::Id).map(|id| id.text())
}

/// The expression a method is declared on: `a.b` of `function a.b:c()`.
pub fn method_owner(method: Node) -> Option<Node> {
    method.child(N::ClassMethodName).and_then(Node::expr)
}

/// A method declared with a dot takes no `self`.
pub fn is_static(method: Node) -> bool {
    method.child(N::ClassMethodName).is_some_and(|name| name.has(T::Dot))
}

pub fn params(owner: Node) -> impl Iterator<Item = Node> {
    owner.child(N::FuncBody).into_iter().flat_map(|body| body.children_of(N::ParamNameDef))
}

pub fn has_varargs(owner: Node) -> bool {
    owner.child(N::FuncBody).is_some_and(|body| body.has(T::Ellipsis))
}

pub fn is_function(node: Node) -> bool {
    matches!(node.kind(), N::FuncDef | N::ClassMethodDef | N::LocalFuncDef | N::ClosureExpr)
}

/// The function a node is written in.
pub fn enclosing_function(node: Node) -> Option<Node> {
    node.ancestors().find(|ancestor| is_function(*ancestor))
}

/// The statement that assigns to `target`, one of its targets.
pub fn assign_stat(target: Node) -> Option<Node> {
    let list = target.parent().filter(|list| list.is(N::VarList))?;
    list.parent().filter(|stat| stat.is(N::AssignStat))
}

/// The rank of `node` among the expressions, or the names, of its list.
pub fn rank(node: Node) -> usize {
    let same = |sibling: &Node| if node.kind().is_expr() { sibling.kind().is_expr() } else { sibling.is(node.kind()) };
    node.parent().map_or(0, |list| list.children().filter(same).take_while(|sibling| *sibling != node).count())
}

/// The source of a CfxLua `in` unpacking: `local a, b in source`.
pub fn in_source(stat: Node) -> Option<Node> {
    stat.expr().filter(|_| stat.has(T::In))
}

/// The name of a table field: `name = v`, `.name` or `['name'] = v`.
pub fn field_name<'a>(src: &'a Source, field: Node<'a>) -> Option<Cow<'a, str>> {
    if let Some(id) = field.token(T::Id) {
        return Some(Cow::Borrowed(id.text()));
    }
    let key = field.expr().filter(|_| field.has(T::LBrack))?;
    string_value(src, key).map(Cow::Owned)
}

/// A CfxLua set field, `{ .name }` or `{ [key] }`: it stores true.
pub fn is_set_field(field: Node) -> bool {
    !field.has(T::Assign) && (field.has(T::Dot) || field.has(T::LBrack))
}

pub fn field_value(field: Node) -> Option<Node> {
    if is_set_field(field) { None } else { field.exprs().last() }
}

/// The value written at the rank `index` of a list, the last one when the list is shorter.
pub fn value_at(list: Node, index: i32) -> Option<Node> {
    usize::try_from(index).ok().and_then(|index| list.exprs().nth(index)).or_else(|| list.exprs().last())
}
