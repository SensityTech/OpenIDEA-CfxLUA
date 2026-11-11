use crate::feature::Problem;
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use openidea_plugin::Severity;

pub const CODE: &str = "CfxSyntaxError";

/// The checks that name an operator CfxLua does not have: on their line the parser's own errors
/// would only repeat them.
const NAMED: &[&str] = &["CfxUnsupportedOperator", "CfxIncrement", "CfxDecrementComment"];

/// Where to show an error that holds no token: on the token the missing piece should follow.
fn anchor(node: Node) -> (usize, usize) {
    let tokens = node.tree().tokens();
    let at = node.token_span().0;
    let before = (0..at).rev().find(|i| !tokens.kind(*i).is_trivia());
    let found = before.or_else(|| (at..tokens.len()).find(|i| !tokens.kind(*i).is_trivia()));
    found.map_or((0, 0), |i| (tokens.start(i), tokens.end(i)))
}

fn range(node: Node) -> (usize, usize) {
    match node.first_token() {
        Some(_) => node.code_range(),
        None => anchor(node),
    }
}

fn has_error(node: Node) -> bool {
    node.descendants().any(|inner| inner.is(N::Error))
}

/// What the parser could not read, and the two errors the JetBrains plugin's annotator adds: a
/// statement that is a bare expression, and a method name without its arguments.
pub fn errors(src: &Source, found: &[Problem]) -> Vec<Problem> {
    let root = src.root();
    let text = root.source();
    let line = |at: usize| src.lines.line_of(at);
    let named: Vec<usize> = found.iter().filter(|p| NAMED.contains(&p.code)).map(|p| line(p.start)).collect();
    let mut out: Vec<Problem> = Vec::new();
    let mut push = |(start, end): (usize, usize), message: &str| {
        let repeated = out.last().is_some_and(|last| last.start == start && last.message == message);
        if !repeated && !named.contains(&line(start)) {
            out.push(Problem::new(CODE, Severity::Error, start, end, message));
        }
    };
    for (node, message) in root.tree().errors(text) {
        push(range(node), message);
    }
    for node in root.descendants() {
        match node.kind() {
            N::ExprStat if !node.expr().is_some_and(|expr| expr.is(N::CallExpr)) && !has_error(node) => {
                push(node.code_range(), "syntax error");
            }
            N::IndexExpr if node.has(T::Colon) && node.has(T::Id) => {
                let called = node.parent().is_some_and(|parent| parent.is(N::CallExpr) && parent.expr() == Some(node));
                if !called {
                    push(node.code_range(), "Arguments expected");
                }
            }
            _ => {}
        }
    }
    out
}
