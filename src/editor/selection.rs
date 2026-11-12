//! What Extend Selection grows through, as the JetBrains platform does from the syntax tree: the word at
//! the caret, then each expression, statement and block around it.

use super::target::token_at;
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};

/// What a quoted or a long string holds between its delimiters.
pub fn string_content(text: &str, (start, end): (usize, usize)) -> Option<(usize, usize)> {
    let literal = text.get(start..end)?;
    let open = match literal.as_bytes().first()? {
        b'"' | b'\'' => 1,
        b'[' => 2 + literal[1..].bytes().take_while(|level| *level == b'=').count(),
        _ => return None,
    };
    let closed = match literal.as_bytes()[0] {
        quote @ (b'"' | b'\'') => literal.len() > 1 && literal.as_bytes()[literal.len() - 1] == quote,
        _ => literal.len() >= 2 * open && literal.ends_with(']') && literal[literal.len() - open..].starts_with(']'),
    };
    let inner = (start + open.min(literal.len()), if closed { end - open } else { end });
    (inner.0 <= inner.1).then_some(inner)
}

fn grow(ranges: &mut Vec<(usize, usize)>, (start, end): (usize, usize)) {
    let wider = ranges.last().is_none_or(|(from, to)| start <= *from && *to <= end && (start, end) != (*from, *to));
    if start < end && wider {
        ranges.push((start, end));
    }
}

fn around(ranges: &mut Vec<(usize, usize)>, node: Node) {
    // The file is its whole text, the comments and the lines around its code included.
    for outer in std::iter::once(node).chain(node.ancestors()).take_while(|outer| !outer.is(N::File)) {
        grow(ranges, outer.code_range());
        if outer.kind().is_statement() || outer.is(N::TableField) {
            grow(ranges, outer.range());
        }
    }
}

/// The ranges around the byte `at`, the innermost first.
pub fn ranges(src: &Source, at: usize) -> Vec<(usize, usize)> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let mut ranges = Vec::new();
    // Between a word and a sign the caret is about the word: `'x'|)`.
    let word =
        |kind: T| matches!(kind, T::Id | T::String | T::Number | T::HashString | T::DocComment) || kind.is_keyword();
    let any = |kind: T| !kind.is_trivia() || kind == T::DocComment;
    let Some(index) = token_at(tokens, at, word).or_else(|| token_at(tokens, at, any)) else { return ranges };
    let token = (tokens.start(index), tokens.end(index));
    match tokens.kind(index) {
        // A documentation comment is read as its own tree: a type, its tag, the comment.
        T::DocComment => {
            let docs = src.docs();
            let inner = token_at(docs.tree().tokens(), at, |kind| !kind.is_trivia());
            if let Some(inner) = inner {
                grow(&mut ranges, (docs.tree().tokens().start(inner), docs.tree().tokens().end(inner)));
                let node = docs.tree().node(docs.source(), docs.tree().node_of_token(inner));
                for outer in std::iter::once(node).chain(node.ancestors()).take_while(|outer| !outer.is(N::DocFile)) {
                    grow(&mut ranges, outer.code_range());
                }
            }
        }
        // What the string holds, unless the caret stands on one of its quotes.
        T::String => {
            let held = |(from, to): &(usize, usize)| from < to && *from <= at && at <= *to;
            ranges.extend(string_content(&src.text, token).filter(held));
        }
        _ => {}
    }
    grow(&mut ranges, token);
    around(&mut ranges, tree.node(root.source(), tree.node_of_token(index)));
    grow(&mut ranges, (0, src.text.len()));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each range around the `<caret>` of `text`, as the text it selects.
    fn grown(text: &str) -> Vec<String> {
        let at = text.find("<caret>").expect("a caret");
        let text = text.replacen("<caret>", "", 1);
        let src = Source::new(&text);
        ranges(&src, at).into_iter().map(|(start, end)| text[start..end].to_owned()).collect()
    }

    #[test]
    fn a_selection_grows_from_the_word_to_the_file() {
        assert_eq!(
            grown("local a = 1\nif a then\n    print(a.b<caret>ar, 'x')\nend"),
            [
                "bar",
                "a.bar",
                "(a.bar, 'x')",
                "print(a.bar, 'x')",
                "if a then\n    print(a.bar, 'x')\nend",
                "local a = 1\nif a then\n    print(a.bar, 'x')\nend",
            ]
        );
    }

    #[test]
    fn a_string_gives_what_it_holds_first() {
        assert_eq!(grown("f('he<caret>llo')")[..3], ["hello", "'hello'", "('hello')"]);
        assert_eq!(grown("x = [==[lo<caret>ng]==]")[..2], ["long", "[==[long]==]"]);
        assert_eq!(grown("x = 'open<caret>")[0], "open", "a string that is not closed");
        assert_eq!(grown("f(<caret>'hello')")[0], "'hello'", "on its quote the caret is not in what it holds");
        assert_eq!(grown("f('hello'<caret>)")[0], "'hello'");
        assert_eq!(grown("f('<caret>hello')")[0], "hello");
        assert_eq!(string_content("''", (0, 2)), Some((1, 1)));
        assert_eq!(string_content("[[]]", (0, 4)), Some((2, 2)));
    }

    #[test]
    fn a_statement_is_taken_with_its_comment_and_a_comment_by_its_parts() {
        let text = "---@param door num<caret>ber the door\nlocal function open(door) end";
        assert_eq!(
            grown(text),
            [
                "number",
                "param door number the door",
                "---@param door number the door",
                "---@param door number the door\nlocal function open(door) end",
            ]
        );
        assert_eq!(
            grown("--- Opens.\nfunction ope<caret>n() end\nopen()")[..3],
            ["open", "function open() end", "--- Opens.\nfunction open() end"]
        );
    }

    #[test]
    fn nothing_grows_where_nothing_is_written() {
        assert!(grown("<caret>").is_empty());
        assert_eq!(grown("a = 1\n\n<caret>\nb = 2"), Vec::<String>::new());
    }
}
