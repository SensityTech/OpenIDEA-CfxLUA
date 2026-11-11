use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldKind {
    Region,
    Comment,
}

/// What folds, from the line of `start` to the line of `end` (byte offsets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub start: usize,
    pub end: usize,
    pub kind: FoldKind,
}

/// The folds of the JetBrains plugin: blocks, tables, comments and strings of several lines, regions.
pub fn folds(src: &Source) -> Vec<Fold> {
    let root = src.root();
    let tokens = root.tree().tokens();
    let mut out = Vec::new();
    for node in root.descendants() {
        match node.kind() {
            N::Block => {
                let (first, end) = node.token_span();
                if first == 0 || end >= tokens.len() {
                    continue;
                }
                // The line of `else`, `elseif` or `until` stays in view: it opens what follows.
                let closer = tokens.start(end);
                let stop = if tokens.kind(end) == T::End { closer } else { src.text[..closer].trim_end().len() };
                out.push(Fold { start: tokens.start(first - 1), end: stop, kind: FoldKind::Region });
            }
            N::TableExpr => {
                let (start, end) = node.code_range();
                out.push(Fold { start, end: end.saturating_sub(1), kind: FoldKind::Region });
            }
            _ => {}
        }
    }
    let mut regions = Vec::new();
    for i in 0..tokens.len() {
        let (start, end) = (tokens.start(i), tokens.end(i));
        match tokens.kind(i) {
            T::BlockComment | T::DocComment => out.push(Fold { start, end, kind: FoldKind::Comment }),
            T::String => out.push(Fold { start, end, kind: FoldKind::Region }),
            T::Region => regions.push(start),
            T::EndRegion => {
                if let Some(opened) = regions.pop() {
                    out.push(Fold { start: opened, end: start, kind: FoldKind::Region });
                }
            }
            _ => {}
        }
    }
    out.retain(|fold| src.text[fold.start..fold.end].contains('\n'));
    out.sort_by_key(|fold| (fold.start, fold.end));
    out
}
