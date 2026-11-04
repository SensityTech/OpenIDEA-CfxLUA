//! Adding a pattern to `escrow_ignore`, keeping the manifest's layout: into
//! the existing table (one line per pattern when it is written that way,
//! trailing comments kept), around a single string, or as a new directive.

use crate::lua::Source;
use crate::manifest::edits::{apply, minimal};
use crate::manifest::view::{Decl, View};
use openidea_plugin::TextEdit;

/// The manifest `text` with `pattern` in its `escrow_ignore` (unchanged when already listed).
pub fn add_pattern(text: &str, pattern: &str) -> String {
    let src = Source::new(text);
    let view = View::new(&src);
    let ignores: Vec<&Decl> = view.named("escrow_ignore").collect();
    if ignores.iter().flat_map(|d| d.values()).any(|v| v.text == pattern) {
        return text.to_owned();
    }
    let quote = ignores
        .iter()
        .flat_map(|d| d.values())
        .next()
        .or_else(|| view.decls.iter().flat_map(|d| d.values()).next())
        .map_or('\'', |v| if text[v.start..].starts_with('"') { '"' } else { '\'' });
    let quoted = format!("{quote}{pattern}{quote}");
    let table = ignores.iter().find_map(|d| match d.entry.table {
        Some((open, Some(close))) => Some((open, close)),
        _ => None,
    });
    if let Some((open, close)) = table {
        let (table_start, table_end) = (src.toks[open].start, src.toks[close].end);
        let inner_end = src.toks[close].start;
        let fields: Vec<(usize, usize)> = src.split_args(open + 1, close).into_iter().filter(|(a, b)| a < b).collect();
        let Some(&(first_tok, last_tok)) = fields.last() else {
            let inner = &text[src.toks[open].end..inner_end];
            let new = if inner.contains('\n') { format!("{{\n    {quoted},\n}}") } else { format!("{{ {quoted} }}") };
            return apply(text, &[(table_start, table_end, new)]);
        };
        let last_end = src.toks[last_tok - 1].end;
        let tail = &text[last_end..inner_end];
        let trimmed = tail.trim_start();
        let has_sep = trimmed.starts_with([',', ';']);
        let after_sep = if has_sep { last_end + (tail.len() - trimmed.len()) + 1 } else { last_end };
        let line_end = text[after_sep..].find('\n').map(|i| after_sep + i).filter(|&l| l < inner_end);
        let multi_line = text[table_start..table_end].contains('\n');
        let Some(line_end) = line_end.filter(|_| multi_line) else {
            return apply(text, &[(last_end, last_end, format!(", {quoted}"))]);
        };
        let field_start = src.toks[first_tok].start;
        let line_start = text[..field_start].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        // Before a `\r\n` line end, the new line ends the same way.
        let (at, nl) = if text[..line_end].ends_with('\r') { (line_end - 1, "\r\n") } else { (line_end, "\n") };
        let mut edits = vec![(at, at, format!("{nl}{indent}{quoted}{}", if has_sep { "," } else { "" }))];
        if !has_sep {
            edits.push((last_end, last_end, ",".to_owned()));
        }
        return apply(text, &edits);
    }
    if let Some(v) = ignores.iter().flat_map(|d| d.values()).next() {
        return apply(text, &[(v.start, v.end, format!("{{ {}, {quoted} }}", &text[v.start..v.end]))]);
    }
    let sep = if text.is_empty() || text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{text}{sep}escrow_ignore {{\n    {quoted},\n}}\n")
}

/// The manifest `text` with every pattern added.
pub fn add_patterns(text: &str, patterns: &[String]) -> String {
    patterns.iter().fold(text.to_owned(), |t, p| add_pattern(&t, p))
}

/// The edit adding `patterns` to the manifest `text`, as a byte edit (`None` when they are all listed).
pub fn pattern_edit(text: &str, patterns: &[String]) -> Option<(usize, usize, String)> {
    minimal(text, &add_patterns(text, patterns))
}

/// The same edit in IDE coordinates, for a manifest that is not the document of the request.
pub fn pattern_text_edit(text: &str, patterns: &[String]) -> Option<TextEdit> {
    let (start, end, new) = pattern_edit(text, patterns)?;
    let lines = crate::text::LineIndex::new(text);
    Some(TextEdit { range: lines.range(text, start, end), text: new })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(text: &str) -> String {
        add_pattern(text, "b.lua")
    }

    #[test]
    fn layouts() {
        assert_eq!(add("escrow_ignore {\n  'a.lua', -- open\n}\n"), "escrow_ignore {\n  'a.lua', -- open\n  'b.lua',\n}\n");
        assert_eq!(add("escrow_ignore {\n    'a.lua'\n}\n"), "escrow_ignore {\n    'a.lua',\n    'b.lua'\n}\n");
        assert_eq!(add("escrow_ignore { \"a.lua\" }\n"), "escrow_ignore { \"a.lua\", \"b.lua\" }\n");
        assert_eq!(add("escrow_ignore({ 'a.lua' })\n"), "escrow_ignore({ 'a.lua', 'b.lua' })\n");
        assert_eq!(add("escrow_ignore 'a.lua'\n"), "escrow_ignore { 'a.lua', 'b.lua' }\n");
        assert_eq!(add("escrow_ignore {}\n"), "escrow_ignore { 'b.lua' }\n");
        assert_eq!(add("escrow_ignore {\n}\n"), "escrow_ignore {\n    'b.lua',\n}\n");
        assert_eq!(add("fx_version 'cerulean'\n"), "fx_version 'cerulean'\n\nescrow_ignore {\n    'b.lua',\n}\n");
        assert_eq!(add("fx_version \"cerulean\""), "fx_version \"cerulean\"\n\nescrow_ignore {\n    \"b.lua\",\n}\n");
        assert_eq!(add(""), "escrow_ignore {\n    'b.lua',\n}\n");
        assert_eq!(add("escrow_ignore { 'b.lua' }\n"), "escrow_ignore { 'b.lua' }\n");
        assert_eq!(add("escrow_ignore {\r\n    'a.lua',\r\n}\r\n"), "escrow_ignore {\r\n    'a.lua',\r\n    'b.lua',\r\n}\r\n");
    }

    #[test]
    fn minimal_edits() {
        let text = "escrow_ignore {\n    'config.lua', -- edited by buyers\n}\n";
        let (s, e, t) = pattern_edit(text, &["data/items.lua".to_owned()]).unwrap();
        assert_eq!((s, e), (s, s), "an insertion");
        assert_eq!(apply(text, &[(s, e, t)]), "escrow_ignore {\n    'config.lua', -- edited by buyers\n    'data/items.lua',\n}\n");
        assert!(pattern_edit(text, &["config.lua".to_owned()]).is_none());
        let edit = pattern_text_edit("a 'x'\nescrow_ignore {}\n", &["b".to_owned()]).unwrap();
        assert_eq!((edit.range.start.line, edit.range.start.character, edit.text.as_str()), (1, 15, " 'b' "));
    }
}
