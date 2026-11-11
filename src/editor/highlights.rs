use crate::lua::Source;
use crate::lua::scope::DeclKind;
use crate::lua::token::T;
use crate::lua::tree::{N, Node, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Text,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub access: Access,
}

fn at_token(token: Token, access: Access) -> Highlight {
    Highlight { start: token.start(), end: token.end(), access }
}

fn at_node(node: Node, access: Access) -> Highlight {
    let (start, end) = node.code_range();
    Highlight { start, end, access }
}

/// The token under the caret: the one that holds it, or the name or keyword that ends at it.
fn token_at<'a>(root: Node<'a>, at: usize) -> Option<Token<'a>> {
    let tokens = root.tree().tokens();
    let index = tokens.at(at)?;
    let word = |i: usize| matches!(tokens.kind(i), T::Id) || tokens.kind(i).is_keyword();
    let index =
        if !word(index) && index > 0 && tokens.end(index - 1) == at && word(index - 1) { index - 1 } else { index };
    root.all_tokens().nth(index)
}

/// Whether the name is assigned where `node` stands: a target, a function statement or a declaration.
fn written(node: Node) -> bool {
    match node.kind() {
        N::NameDef | N::ParamNameDef | N::LocalFuncDef | N::FuncDef | N::ClassMethodName | N::TableField => true,
        _ => match node.parent() {
            Some(parent) if parent.is(N::VarList) => true,
            Some(parent) if parent.is(N::CompoundAssignStat) => parent.expr() == Some(node),
            _ => false,
        },
    }
}

fn access(node: Node) -> Access {
    if written(node) { Access::Write } else { Access::Read }
}

/// The method `node` stands in when it may read its `self`.
fn method_of(node: Node) -> Option<u32> {
    let method = node.ancestor(N::ClassMethodDef)?;
    method.child(N::ClassMethodName).filter(|name| name.has(T::Colon)).map(|_| method.id())
}

fn names<'a>(src: &'a Source, node: Node<'a>, name: Token<'a>) -> Vec<Highlight> {
    let root = src.root();
    let scopes = src.scopes();
    let own = |node: Node<'a>| node.token(T::Id).map(|id| at_token(id, access(node)));
    let local = |node: Node| scopes.resolve(node).is_some_and(|decl| decl.kind != DeclKind::Global);
    if let Some(decl) = scopes.id_of(node).filter(|_| local(node)) {
        return scopes.uses(decl).filter_map(|id| own(root.tree().node(root.source(), id))).collect();
    }
    // A global is one name in the whole file: every use of it that no local takes.
    let method = (name.text() == "self").then(|| method_of(node));
    root.descendants()
        .filter(|other| matches!(other.kind(), N::NameExpr | N::FuncDef) && !local(*other))
        .filter(|other| other.token(T::Id).is_some_and(|id| id.text() == name.text()))
        .filter(|other| method.is_none_or(|method| method_of(*other) == method))
        .filter_map(own)
        .collect()
}

/// The object a member is read from, as written, without spaces or safe navigation.
fn object(node: Node) -> Option<String> {
    let prefix = node.expr()?;
    Some(prefix.code().chars().filter(|c| !c.is_whitespace() && *c != '?').collect::<String>().replace(':', "."))
}

fn members<'a>(src: &'a Source, node: Node<'a>, name: Token<'a>) -> Vec<Highlight> {
    let Some(target) = object(node) else { return vec![at_token(name, access(node))] };
    src.root()
        .descendants()
        .filter(|other| matches!(other.kind(), N::IndexExpr | N::ClassMethodName))
        .filter(|other| other.token(T::Id).is_some_and(|id| id.text() == name.text()))
        .filter(|other| object(*other).as_deref() == Some(target.as_str()))
        .filter_map(|other| other.token(T::Id).map(|id| at_token(id, access(other))))
        .collect()
}

fn exits(statement: Node) -> Vec<Highlight> {
    let Some(body) = statement.ancestor(N::FuncBody) else { return Vec::new() };
    body.descendants()
        .filter(|node| node.is(N::ReturnStat) && node.ancestor(N::FuncBody) == Some(body))
        .map(|node| at_node(node, Access::Text))
        .collect()
}

fn loop_of(node: Node) -> Option<Node> {
    node.ancestors()
        .take_while(|outer| !outer.is(N::FuncBody))
        .find(|outer| matches!(outer.kind(), N::WhileStat | N::RepeatStat | N::ForAStat | N::ForBStat))
}

/// What the JetBrains plugin highlights with the caret at `at`: the uses of a name, the exits of a
/// function from `return`, the ends of a loop from `break`, an expression from its operator.
pub fn highlights(src: &Source, at: usize) -> Vec<Highlight> {
    let Some(token) = token_at(src.root(), at) else { return Vec::new() };
    let node = token.parent();
    match (token.kind(), node.kind()) {
        (T::Return, N::ReturnStat) => exits(node),
        (T::Break, N::BreakStat) => {
            let Some(outer) = loop_of(node) else { return Vec::new() };
            let end = outer.token(T::End).or(outer.token(T::Until));
            let ends = outer.first_token().into_iter().chain(end).map(|edge| at_token(edge, Access::Text));
            ends.chain([at_token(token, Access::Text)]).collect()
        }
        (_, N::BinaryOp | N::UnaryOp) => node.parent().map(|expr| at_node(expr, Access::Text)).into_iter().collect(),
        (T::Id, N::NameExpr | N::NameDef | N::ParamNameDef | N::LocalFuncDef | N::FuncDef) => names(src, node, token),
        (T::Id, N::IndexExpr | N::ClassMethodName) => members(src, node, token),
        (T::Id, N::TableField) => vec![at_token(token, Access::Write)],
        _ => Vec::new(),
    }
}
