//! What turns an inspection off where it would speak: `---@suppress Id, Other` above a statement or a table
//! field (`LuaSuppressManager`), and `---@meta` in a file that only declares an API
//! (`LuaCatsMetaInspectionSuppressor`).

use crate::feature::Problem;
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;

const TAG: &str = "@suppress";
const META: &str = "@meta";

/// The inspections that only make sense for code that runs: a declaration file leaves its parameters unused,
/// its functions empty and its fields to placeholder values.
const OF_RUNNING_CODE: [&str; 6] = [
    "RemoveUnusedLocal",
    "GlobalNameCanbeLocal",
    "UndeclaredVariable",
    "ReturnType",
    "AssignType",
    "MatchFunctionSignature",
];

/// Whether a line of the text is a `---@meta` comment.
pub fn is_meta(text: &str) -> bool {
    let marks = |line: &str| {
        let line = line.trim_start_matches([' ', '\t']);
        let Some(tag) = line.strip_prefix("---") else { return false };
        let Some(rest) = tag.trim_start_matches('-').trim_start_matches([' ', '\t']).strip_prefix(META) else {
            return false;
        };
        !rest.starts_with(|next: char| next.is_alphanumeric() || next == '_')
    };
    text.contains(META) && text.lines().any(marks)
}

/// Whether the statement, or the table field, that most closely holds the byte `at` suppresses `code`.
fn suppressed(src: &Source, at: usize, code: &str) -> bool {
    let root = src.root();
    let inner = root.tree().node_at(root.source(), at);
    let owner = std::iter::once(inner)
        .chain(inner.ancestors())
        .find(|node| node.kind().is_statement() || node.is(N::TableField));
    let tag = owner.and_then(|owner| src.comment_of(owner)).and_then(|comment| comment.tag(N::DocTagSuppress));
    tag.is_some_and(|tag| tag.tokens().any(|id| id.is(T::Id) && id.text() == code))
}

/// The problems no comment suppresses.
pub fn unsuppressed(src: &Source, mut problems: Vec<Problem>) -> Vec<Problem> {
    if problems.is_empty() {
        return problems;
    }
    if is_meta(&src.text) {
        problems.retain(|problem| !OF_RUNNING_CODE.contains(&problem.code));
    }
    if src.text.contains(TAG) {
        problems.retain(|problem| !suppressed(src, problem.start, problem.code));
    }
    problems
}
