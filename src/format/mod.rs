//! The formatter of the JetBrains plugin (`editor/formatter/`): its blocks and its spacing rules, laid out
//! the way the IntelliJ platform lays them out. Only white space changes, and the `;` no statement needs
//! (`LuaPreFormatProcessor`): the tokens of the file stay what they are.

mod blocks;
mod layout;
mod spacing;
#[cfg(test)]
mod tests;

use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;
use layout::{Gap, Layout, Shape};

/// The code style: the options of `LuaCodeStyleSettings` and those the JetBrains plugin reads in the common
/// settings, with the values IntelliJ gives them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    pub indent: u32,
    pub continuation: u32,
    pub tabs: bool,
    pub tab_size: u32,
    pub space_after_field_sep: bool,
    pub space_around_binary_operator: bool,
    pub space_inside_inline_table: bool,
    pub align_table_field_assign: bool,
    pub space_around_assignment: bool,
    pub space_before_comma: bool,
    pub space_after_comma: bool,
    pub align_parameters: bool,
    pub align_arguments: bool,
    pub keep_simple_blocks: bool,
    pub align_consecutive_assignments: bool,
    pub keep_blank_lines: u32,
    pub remove_semicolons: bool,
}

impl Style {
    /// IntelliJ's defaults for an editor that indents by `tab_size`: a continuation is indented twice.
    pub fn new(tab_size: u32, insert_spaces: bool) -> Self {
        let indent = tab_size.clamp(1, 32);
        Self {
            indent,
            continuation: indent * 2,
            tabs: !insert_spaces,
            tab_size: indent,
            space_after_field_sep: true,
            space_around_binary_operator: true,
            space_inside_inline_table: true,
            align_table_field_assign: false,
            space_around_assignment: true,
            space_before_comma: false,
            space_after_comma: true,
            align_parameters: true,
            align_arguments: false,
            keep_simple_blocks: false,
            align_consecutive_assignments: false,
            keep_blank_lines: 2,
            remove_semicolons: true,
        }
    }

    /// The white space that starts a line indented by `columns` (`IndentInfo.generateNewWhiteSpace`).
    fn indentation(&self, columns: u32) -> String {
        match self.tabs {
            true => {
                let (tabs, spaces) = (columns / self.tab_size, columns % self.tab_size);
                "\t".repeat(tabs as usize) + &" ".repeat(spaces as usize)
            }
            false => " ".repeat(columns as usize),
        }
    }

    /// How many columns a run of spaces and tabs takes.
    fn columns(&self, white: &str) -> u32 {
        white.chars().map(|c| if c == '\t' { self.tab_size } else { 1 }).sum()
    }
}

/// The text that replaces what stands between two byte offsets.
pub type Edit = (usize, usize, String);

/// Whether two tokens written one right after the other would be read as other tokens: two words, a
/// number and a dot, two signs that make a longer one (`- -x` would start a comment).
fn merges(left: &str, right: &str) -> bool {
    let (Some(a), Some(b)) = (left.chars().next_back(), right.chars().next()) else { return false };
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if word(a) && word(b) {
        return true;
    }
    match (a, b) {
        ('.', '.') => true,
        ('.', digit) if digit.is_ascii_digit() => true,
        (digit, '.') if digit.is_ascii_digit() => true,
        ('[', '[' | '=') => true,
        ('-', '-') | ('/', '/' | '*') | ('*', '/') | (':', ':') => true,
        ('<', '<') | ('>', '>') => true,
        ('<' | '>' | '=' | '~' | '+' | '-' | '*' | '/' | '&' | '|' | '^', '=') => true,
        _ => false,
    }
}

/// The `;` to take out, sorted: every one but a field separator of a table, and but the one that keeps a
/// statement that starts with a parenthesis from being read as a call of the statement before it. A file
/// the parser could not read keeps them all.
fn needless_semicolons(src: &Source) -> Vec<u32> {
    let root = src.root();
    let tree = root.tree();
    if tree.has_errors() {
        return Vec::new();
    }
    let tokens = tree.tokens();
    let mut out = Vec::new();
    for index in 0..tokens.len() {
        if tokens.kind(index) != T::Semi || tree.node(root.source(), tree.node_of_token(index)).is(N::TableFieldSep) {
            continue;
        }
        let next =
            (index + 1..tokens.len()).find(|after| !tokens.kind(*after).is_trivia() && tokens.kind(*after) != T::Semi);
        if next.is_none_or(|next| tokens.kind(next) != T::LParen) {
            out.push(index as u32);
        }
    }
    out
}

/// The line breaks of a white space, and the columns after the last one.
fn measured(style: &Style, white: &str) -> (u32, u32) {
    let lf = white.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let last = white.rfind('\n').map_or(white, |at| &white[at + 1..]);
    (lf, style.columns(last.trim_end_matches('\r')))
}

/// A file no layout got to the end of: blocks to align kept moving one another, or the file asks for more
/// work than a layout may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsettled;

/// The edits that format `src`, or of it the white space that touches `range`.
pub fn format(src: &Source, style: &Style, range: Option<(usize, usize)>) -> Result<Vec<Edit>, Unsettled> {
    let (edits, settled) = laid_out(src, style, range);
    if settled {
        return Ok(edits);
    }
    // The `=` to align kept moving one another: the file is formatted without that, or left alone.
    let plain = Style { align_table_field_assign: false, align_consecutive_assignments: false, ..style.clone() };
    let (edits, settled) = laid_out(src, &plain, range);
    if settled { Ok(edits) } else { Err(Unsettled) }
}

/// The same change written as `most` edits at most: neighbours are joined into one edit, which writes again
/// what stands between them. `edits` are in the order of the text, as `format` gives them.
pub fn joined(text: &str, edits: Vec<Edit>, most: usize) -> Vec<Edit> {
    if most == 0 || edits.len() <= most {
        return edits;
    }
    let joined = |group: &[Edit]| {
        let mut written = String::new();
        for (rank, (_, end, with)) in group.iter().enumerate() {
            written.push_str(with);
            if let Some((next, _, _)) = group.get(rank + 1) {
                written.push_str(&text[*end..*next]);
            }
        }
        (group[0].0, group[group.len() - 1].1, written)
    };
    edits.chunks(edits.len().div_ceil(most)).map(joined).collect()
}

/// The edits of one layout, and whether that layout got to the end of the file.
fn laid_out(src: &Source, style: &Style, range: Option<(usize, usize)>) -> (Vec<Edit>, bool) {
    let root = src.root();
    let (tree, text) = (root.tree(), root.source());
    let tokens = tree.tokens();
    // A range is the lines it touches: the indent of its first line is part of it.
    let range = range.map(|(start, end)| {
        let (start, end) = (floor(text, start), floor(text, end.max(start)));
        (text[..start].rfind('\n').map_or(0, |at| at + 1), text[end..].find('\n').map_or(text.len(), |at| end + at))
    });
    let touched = |from: usize, to: usize| range.is_none_or(|(start, end)| from < end && to >= start);
    let mut removed = if style.remove_semicolons { needless_semicolons(src) } else { Vec::new() };
    removed.retain(|index| touched(tokens.start(*index as usize), tokens.end(*index as usize)));
    let mut built = blocks::build(root, style, &removed);
    if built.leaves.is_empty() {
        return (Vec::new(), true);
    }
    let spacings = {
        let rules = spacing::Rules { tree, src: text, blocks: &built, style };
        spacing::of_leaves(&rules)
    };
    let line_break = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut gaps = Vec::with_capacity(built.leaves.len());
    let mut shapes = Vec::with_capacity(built.leaves.len());
    let mut spans = Vec::with_capacity(built.leaves.len());
    let mut before = 0;
    for (rank, (token, _)) in built.leaves.iter().enumerate() {
        let token = *token as usize;
        let (start, end) = (tokens.start(token), tokens.end(token));
        // What stands between two tokens that are kept: white space, and the `;` taken out.
        let white: String = text[before..start].chars().filter(|c| *c != ';').collect();
        let (lf, cols) = measured(style, &white);
        gaps.push(Gap { lf, cols, read_only: !touched(before, start) });
        let piece = &text[start..end];
        let last_line = piece.rfind('\n').map_or(piece, |at| &piece[at + 1..]);
        let kind = tokens.kind(token);
        shapes.push(Shape {
            width: last_line.chars().count() as u32,
            multiline: piece.contains('\n'),
            ends_line: matches!(kind, T::ShortComment | T::DocComment | T::Region | T::EndRegion),
            glued: before == start,
            merges: rank > 0 && merges(&text[spans[rank - 1]..before], piece),
        });
        spans.push(start);
        before = end;
    }
    let mut layout = Layout::new(&mut built, &spacings, &shapes, style, gaps);
    let settled = layout.run();
    let mut edits = Vec::new();
    let mut before = 0;
    for (rank, start) in spans.iter().enumerate() {
        let (gap, token) = (layout.gaps[rank], layout.blocks.leaves[rank].0 as usize);
        let white = match gap.lf {
            0 => " ".repeat(gap.cols as usize),
            lf => line_break.repeat(lf as usize) + &style.indentation(gap.cols),
        };
        if !gap.read_only && white != text[before..*start] {
            edits.push((before, *start, white));
        }
        let end = tokens.end(token);
        if tokens.kind(token) == T::DocComment && shapes[rank].multiline {
            let indent = style.indentation(layout.inner_indent(rank as u32));
            let piece = &text[*start..end];
            for (at, _) in piece.match_indices('\n') {
                let from = *start + at + 1;
                let to = from + piece[at + 1..].len() - piece[at + 1..].trim_start_matches([' ', '\t']).len();
                if touched(from, to) && text[from..to] != indent {
                    edits.push((from, to, indent.clone()));
                }
            }
        }
        before = end;
    }
    // A `;` after the last token goes with the spaces before it.
    for index in removed.iter().map(|index| *index as usize).filter(|index| tokens.start(*index) >= before) {
        let from = text[before..tokens.start(index)].trim_end_matches([' ', '\t']).len() + before;
        edits.push((from.max(edits.last().map_or(0, |edit| edit.1)), tokens.end(index), String::new()));
    }
    (edits, settled)
}

/// The offset `at`, or the start of the character it stands in.
fn floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The text of `src` once its edits are applied.
#[cfg(test)]
pub fn formatted(src: &Source, style: &Style) -> String {
    let mut text = src.text.clone();
    let mut edits = format(src, style, None).expect("a layout that settles");
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for (start, end, with) in edits {
        text.replace_range(start..end, &with);
    }
    text
}
