//! `---@cast name T`, `+T`, `-T`: a variable read after the annotation has the cast type, until it is
//! assigned or declared again (`LuaCatsCasts`). The cast is looked for backwards from the read, through the
//! statements that precede it in each block around it; a block that precedes the read is not entered.

use super::Ty;
use super::context::{Cx, Psi};
use super::doc::Reader;
use super::expressions::infer_expr;
use super::psi;
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::token::T;
use crate::lua::tree::{Element, N, Node};

struct MayCast(bool);

fn may_cast(src: &Source) -> bool {
    src.model(|src| MayCast(src.text.contains("@cast"))).0
}

/// The documentation comment whose token starts at `at` in the Lua tree.
fn comment_at(src: &Source, at: usize) -> Option<Comment<'_>> {
    let docs = src.docs();
    let inner = docs.tree().node_at(docs.source(), at);
    std::iter::once(inner).chain(inner.ancestors()).find(|node| node.is(N::DocComment)).map(Comment)
}

/// The last cast of `name` a comment holds.
fn cast_in<'s>(comment: Comment<'s>, name: &str) -> Option<Node<'s>> {
    comment.tags(N::DocTagCast).filter(|cast| tags::name(*cast) == Some(name)).last()
}

/// A function or a loop that declares `name`: a cast written outside it is about another variable.
fn declares(scope: Node, name: &str) -> bool {
    matches!(scope.kind(), N::FuncBody | N::ForAStat | N::ForBStat)
        && scope.children_of(N::ParamNameDef).any(|param| param.code() == name)
}

fn assigns_or_declares(stat: Node, name: &str) -> bool {
    let named = |node: Node| node.code() == name;
    match stat.kind() {
        N::LocalDef => stat.child(N::NameList).is_some_and(|names| names.children_of(N::NameDef).any(named)),
        N::LocalFuncDef => psi::function_name(stat) == Some(name),
        N::AssignStat => stat.child(N::VarList).is_some_and(|targets| targets.children_of(N::NameExpr).any(named)),
        _ => false,
    }
}

/// The nearest cast of `name` among what `parent` holds before the byte `before`, then before `parent`
/// itself in what holds it, and so on.
fn cast_before<'s>(src: &'s Source, mut parent: Node<'s>, mut before: usize, name: &str) -> Option<Node<'s>> {
    loop {
        if declares(parent, name) {
            return None;
        }
        let start = |element: &Element| match element {
            Element::Node(node) => node.range().0,
            Element::Token(token) => token.start(),
        };
        let previous: Vec<Element> = parent.elements().take_while(|element| start(element) < before).collect();
        for element in previous.into_iter().rev() {
            let comments: Vec<usize> = match element {
                Element::Token(token) if token.is(T::DocComment) => vec![token.start()],
                Element::Node(stat) if stat.kind().is_statement() => {
                    if assigns_or_declares(stat, name) {
                        return None;
                    }
                    let leading =
                        stat.elements().map_while(Element::token).take_while(|token| token.kind().is_trivia());
                    let comments = leading.filter(|token| token.is(T::DocComment));
                    let mut bound: Vec<usize> = comments.map(|token| token.start()).collect();
                    bound.reverse();
                    bound
                }
                _ => Vec::new(),
            };
            let cast = comments.into_iter().find_map(|at| cast_in(comment_at(src, at)?, name));
            if cast.is_some() {
                return cast;
            }
        }
        before = parent.range().0;
        parent = parent.parent()?;
    }
}

/// The cast that precedes `cast`: an earlier tag of its comment, or one before the comment.
fn cast_before_tag<'s>(src: &'s Source, cast: Node<'s>, name: &str) -> Option<Node<'s>> {
    let comment = Comment(cast.parent()?);
    let earlier = comment.tags(N::DocTagCast).take_while(|tag| *tag != cast);
    if let Some(found) = earlier.filter(|tag| tags::name(*tag) == Some(name)).last() {
        return Some(found);
    }
    let at = comment.0.range().0;
    let holder = src.root().tree().node_at(src.root().source(), at);
    cast_before(src, holder, at, name)
}

fn cast_type(psi: Psi, cast: Node, name: &str, uncast: &dyn Fn() -> Ty) -> Ty {
    let reader = Reader::new(psi.file);
    let (mut replacement, mut added, mut removed) = (None::<Ty>, Vec::new(), Vec::new());
    for item in cast.children_of(N::DocCastItem) {
        // `+?` and `-?` add or remove nil, which a union never keeps.
        let Some(ty) = tags::ty(item).map(|ty| reader.ty(ty)) else { continue };
        if item.has(T::Plus) {
            added.push(ty);
        } else if item.has(T::Minus) {
            removed.push(ty);
        } else {
            replacement = Some(replacement.map_or(ty.clone(), |known| known.union(&ty)));
        }
    }
    let previous =
        || cast_before_tag(psi.src, cast, name).map_or(Ty::Unknown, |cast| cast_type(psi, cast, name, uncast));
    if replacement.is_none() && added.is_empty() && removed.is_empty() {
        return previous();
    }
    let mut result = replacement.unwrap_or_else(|| {
        let before = previous();
        if before.is_invalid() { uncast() } else { before }
    });
    for ty in &added {
        result.add(ty);
    }
    if !removed.is_empty() {
        let mut kept = Ty::Unknown;
        for ty in result.each().iter().filter(|ty| !removed.contains(ty)) {
            kept.add(ty);
        }
        result = kept;
    }
    result
}

/// The type a cast gives to the name `psi` reads; unknown when none applies.
pub fn cast(cx: &Cx, psi: Psi) -> Ty {
    if cx.alone() || !may_cast(psi.src) || psi::assign_stat(psi.node).is_some() {
        return Ty::Unknown;
    }
    let (Some(parent), name) = (psi.node.parent(), psi.node.code()) else { return Ty::Unknown };
    match cast_before(psi.src, parent, psi.node.range().0, name) {
        Some(cast) => cast_type(psi, cast, name, &|| infer_expr(cx, psi)),
        None => Ty::Unknown,
    }
}
