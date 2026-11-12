//! The inspections of the JetBrains plugin that read the tree and the scopes of one file
//! (`codeInsight/inspection`): what is declared and never used, what hides a local, what does nothing.

#[cfg(test)]
mod tests;

use crate::editor::target::documented_param;
use crate::feature::{Fix, Problem};
use crate::lua::Source;
use crate::lua::scope::{Decl, DeclKind};
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use openidea_plugin::Severity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inspection {
    /// Its id in the JetBrains plugin: the code of its problems, and the name `---@suppress` takes.
    pub code: &'static str,
    /// The setting that turns it off.
    pub setting: &'static str,
}

pub const UNUSED: Inspection = Inspection { code: "RemoveUnusedLocal", setting: "inspections.unused" };
pub const SHADOWED: Inspection = Inspection { code: "LocalNameShadowed", setting: "inspections.shadowed" };
pub const SIMPLIFY: Inspection = Inspection { code: "SimplifyLocalAssignment", setting: "inspections.simplifyLocal" };
pub const EMPTY_BODY: Inspection = Inspection { code: "EmptyBody", setting: "inspections.emptyBody" };
pub const UNREACHABLE: Inspection = Inspection { code: "UnreachableStatement", setting: "inspections.unreachable" };
pub const ZERO_INDEX: Inspection = Inspection { code: "ArrayIndexZero", setting: "inspections.zeroIndex" };

pub const ALL: [Inspection; 6] = [UNUSED, SHADOWED, SIMPLIFY, EMPTY_BODY, UNREACHABLE, ZERO_INDEX];

const UNDERLINE: &str = "_";

/// What the inspections `wanted` keeps find in `src`, in the order of the text.
pub fn problems(src: &Source, wanted: &dyn Fn(Inspection) -> bool) -> Vec<Problem> {
    let mut out = Vec::new();
    let (unused, shadowed) = (wanted(UNUSED), wanted(SHADOWED));
    if unused || shadowed {
        locals(src, unused, shadowed, &mut out);
    }
    let (simplify, empty, unreachable, zero) =
        (wanted(SIMPLIFY), wanted(EMPTY_BODY), wanted(UNREACHABLE), wanted(ZERO_INDEX));
    if simplify || empty || unreachable || zero {
        for node in src.root().descendants() {
            match node.kind() {
                N::LocalDef if simplify => out.extend(simpler_local(node)),
                N::ForAStat | N::ForBStat | N::DoStat | N::WhileStat if empty => out.extend(empty_body(src, node)),
                N::ReturnStat if unreachable => after_return(src, node, &mut out),
                N::IndexExpr if zero => out.extend(zero_index(node)),
                _ => {}
            }
        }
    }
    out.sort_by_key(|problem| problem.start);
    out
}

/// The bytes a statement leaves with: its lines when nothing else is written on them.
fn removal(src: &Source, statement: Node) -> (usize, usize) {
    let (start, end) = statement.range();
    let text = src.text.as_str();
    let line = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let next = text[end..].find('\n').map_or(text.len(), |at| end + at + 1);
    let alone = text[line..start].trim().is_empty() && text[end..next].trim().is_empty();
    if alone { (line, next) } else { (start, end) }
}

fn remove(title: String, src: &Source, statement: Node) -> Fix {
    let (start, end) = removal(src, statement);
    Fix::new(title).replace(start, end, "")
}

/// Where a `@param` of the comment bound to the statement of a parameter names it.
fn documented_as(src: &Source, param: Node) -> Option<(usize, usize)> {
    let owner = param.ancestors().find(|outer| outer.kind().is_statement() || outer.is(N::TableField))?;
    let reference = src.comment_of(owner)?.param(param.code())?.child(N::DocParamNameRef)?;
    (documented_param(src, reference) == Some(param)).then(|| reference.code_range())
}

fn locals(src: &Source, unused: bool, shadowed: bool, out: &mut Vec<Problem>) {
    let scopes = src.scopes();
    let root = src.root();
    let tokens = root.tree().tokens();
    let uses = scopes.use_counts();
    for (id, decl) in scopes.decls().iter().enumerate() {
        let token = decl.name as usize;
        let name = tokens.text(root.source(), token);
        if decl.kind == DeclKind::Global || name == UNDERLINE {
            continue;
        }
        let at = (tokens.start(token), tokens.end(token));
        if unused && uses[id] == 1 {
            out.push(never_used(src, decl, name, at));
        }
        if let Some(hidden) = scopes.hidden(decl).filter(|_| shadowed) {
            let line = src.lines.line_of(tokens.start(hidden.name as usize)) + 1;
            let message = format!("Local name shadowed, '{name}' was previously defined on line {line}");
            out.push(Problem::new(SHADOWED.code, Severity::Warning, at.0, at.1, message));
        }
    }
}

/// A local, a parameter, a loop variable or a local function nothing reads or assigns (`RemoveUnusedLocal`).
fn never_used(src: &Source, decl: &Decl, name: &str, (start, end): (usize, usize)) -> Problem {
    let root = src.root();
    let node = root.tree().node(root.source(), decl.node);
    let found = |message: String| Problem::new(UNUSED.code, Severity::Hint, start, end, message).unnecessary();
    let rename = Fix::new(format!("Rename to '{UNDERLINE}'")).replace(start, end, UNDERLINE);
    match decl.kind {
        DeclKind::Param => {
            let rename = match documented_as(src, node) {
                Some((from, to)) => rename.replace(from, to, UNDERLINE),
                None => rename,
            };
            found(format!("Unused parameter : '{name}'")).fix(rename)
        }
        DeclKind::LoopVar => found(format!("Unused loop variable : '{name}'")).fix(rename),
        DeclKind::Function => {
            let title = format!("Remove unused local function : '{name}'");
            found(format!("Unused local function : '{name}'")).fix(remove(title, src, node))
        }
        _ => {
            let mut problem = found(format!("Unused local : '{name}'"));
            let Some(statement) = node.ancestor(N::LocalDef) else { return problem };
            let names = statement.child(N::NameList).map_or(0, |list| list.children_of(N::NameDef).count());
            if names == 1 {
                problem = problem.fix(remove(format!("Remove unused local '{name}'"), src, statement));
            }
            // `local a in src` reads the key `a` of src: another name would read another value.
            if statement.has(T::In) { problem } else { problem.fix(rename) }
        }
    }
}

/// `local a = nil` says no more than `local a` (`SimplifyLocalAssignment`).
fn simpler_local(statement: Node) -> Option<Problem> {
    let assign = statement.token(T::Assign)?;
    let values = statement.child(N::ExprList)?;
    let mut written = values.exprs();
    let value = written.next().filter(|value| value.is(N::LiteralExpr) && value.code() == "nil")?;
    if written.next().is_some() {
        return None;
    }
    let (start, end) = value.code_range();
    let from = statement.child(N::NameList).map_or(assign.start(), |names| names.code_range().1);
    let fix = Fix::new("Simplify local assignment").replace(from, values.code_range().1, "");
    let found = Problem::new(SIMPLIFY.code, Severity::Hint, start, end, "Local assignment can be simplified");
    Some(found.unnecessary().fix(fix))
}

/// A loop or a `do` whose block holds nothing but locals (`EmptyBody`).
fn empty_body(src: &Source, statement: Node) -> Option<Problem> {
    let block = statement.child(N::Block).filter(|_| statement.has(T::End))?;
    if block.children().any(|inner| !matches!(inner.kind(), N::LocalDef | N::LocalFuncDef)) {
        return None;
    }
    let (word, keyword) = match statement.kind() {
        N::DoStat => ("do", T::Do),
        N::WhileStat => ("while", T::While),
        _ => ("for", T::For),
    };
    let keyword = statement.token(keyword)?;
    let message = format!("Empty {word} body");
    let fix = remove(format!("Remove empty {word}"), src, statement);
    Some(Problem::new(EMPTY_BODY.code, Severity::Warning, keyword.start(), keyword.end(), message).fix(fix))
}

/// The statements written after a `return` in its block (`UnreachableStatement`).
fn after_return(src: &Source, returned: Node, out: &mut Vec<Problem>) {
    // The statements after a second `return` are reported once, from the first one.
    let mut before = std::iter::successors(returned.prev_sibling(), |node| node.prev_sibling());
    if before.any(|node| node.is(N::ReturnStat)) {
        return;
    }
    let after = std::iter::successors(returned.next_sibling(), |node| node.next_sibling());
    for statement in after.filter(|node| node.kind().is_statement()) {
        let (start, end) = statement.code_range();
        let fix = remove("Remove unreachable statement".to_owned(), src, statement);
        out.push(Problem::new(UNREACHABLE.code, Severity::Warning, start, end, "Unreachable statement").fix(fix));
    }
}

/// `t[0]`: the first element of a Lua array is at 1 (`ArrayIndexZero`).
fn zero_index(index: Node) -> Option<Problem> {
    let zero = index.exprs().find(|inner| inner.is(N::LiteralExpr) && inner.code() == "0")?;
    let (start, end) = zero.code_range();
    let fix = Fix::new("Replace with index 1").replace(start, end, "1");
    Some(Problem::new(ZERO_INDEX.code, Severity::Warning, start, end, "0 index").fix(fix))
}
