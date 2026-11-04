//! The text edits of the manifest quick fixes, as `(start, end, new text)`
//! byte edits of the manifest text.

use super::view::{Decl, View};
use crate::lua::Source;
use crate::project::Value;

pub type Edit = (usize, usize, String);

/// Adds `line` after the last statement named one of `after`, else before the first statement, else at the end.
pub fn add_directive(src: &Source, view: &View, line: &str, after: &[&str]) -> Edit {
    let text = &src.text;
    if let Some(anchor) = view.decls.iter().rev().find(|d| d.statement && after.contains(&d.name())) {
        let nl = text[anchor.entry.end..].find('\n').map_or(text.len(), |i| anchor.entry.end + i);
        return (nl, nl, format!("\n{line}"));
    }
    if let Some(first) = src.toks.first() {
        return (first.start, first.start, format!("{line}\n"));
    }
    let sep = if !text.is_empty() && !text.ends_with('\n') { "\n" } else { "" };
    (text.len(), text.len(), format!("{sep}{line}\n"))
}

/// Removes a directive, with its line when nothing else is on it.
pub fn remove_statement(text: &str, d: &Decl) -> Edit {
    let (start, end) = (d.entry.start, d.entry.end);
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let blank = |s: &str| s.chars().all(|c| c == ' ' || c == '\t' || c == '\r');
    if blank(&text[line_start..start]) && blank(&text[end..line_end]) {
        let to = if line_end < text.len() { line_end + 1 } else { line_end };
        return (line_start, to, String::new());
    }
    (start, end, String::new())
}

pub fn replace_statement(d: &Decl, line: &str) -> Edit {
    (d.entry.start, d.entry.end, line.to_owned())
}

/// Replaces a string literal with `value`, in the literal's quotes.
pub fn replace_value(text: &str, v: &Value, value: &str) -> Edit {
    let q = if text[v.start..].starts_with('"') { '"' } else { '\'' };
    (v.start, v.end, format!("{q}{value}{q}"))
}

pub fn rename(d: &Decl, name: &str) -> Edit {
    (d.entry.name_start, d.entry.name_end, name.to_owned())
}

/// Applies byte edits (non-overlapping) to `text`.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_owned();
    for (s, e, t) in sorted {
        out.replace_range(*s..*e, t);
    }
    out
}

/// The single edit turning `old` into `new` (common prefix and suffix kept), `None` when equal.
pub fn minimal(old: &str, new: &str) -> Option<Edit> {
    if old == new {
        return None;
    }
    let mut pre = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
        pre -= 1;
    }
    let max = old.len().min(new.len()) - pre;
    let mut suf = old.bytes().rev().zip(new.bytes().rev()).take(max).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - suf) || !new.is_char_boundary(new.len() - suf) {
        suf -= 1;
    }
    Some((pre, old.len() - suf, new[pre..new.len() - suf].to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::catalog::{FX_VERSION_LINE, RDR3_WARNING, rdr3_warning_line};

    fn add(text: &str, line: &str, after: &[&str]) -> String {
        let src = Source::new(text);
        let view = View::new(&src);
        apply(text, &[add_directive(&src, &view, line, after)])
    }

    #[test]
    fn add_directive_vectors() {
        assert_eq!(add("-- my resource\ngame 'gta5'\n", FX_VERSION_LINE, &[]), "-- my resource\nfx_version 'cerulean'\ngame 'gta5'\n");
        assert_eq!(add("fx_version 'cerulean'\nauthor 'me'", "game 'gta5'", &["fx_version"]), "fx_version 'cerulean'\ngame 'gta5'\nauthor 'me'");
        let rdr3 = format!("game 'rdr3'\n{}", rdr3_warning_line());
        assert_eq!(
            add("fx_version 'cerulean'\nauthor 'me'", &rdr3, &["fx_version"]),
            format!("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning '{RDR3_WARNING}'\nauthor 'me'")
        );
        assert_eq!(
            add("fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\nauthor 'x'\n", &rdr3_warning_line(), &["game", "games"]),
            format!("fx_version 'cerulean'\ngames {{ 'gta5', 'rdr3' }}\nrdr3_warning '{RDR3_WARNING}'\nauthor 'x'\n")
        );
        assert_eq!(add("-- only a comment", FX_VERSION_LINE, &[]), "-- only a comment\nfx_version 'cerulean'\n");
        assert_eq!(add("", FX_VERSION_LINE, &[]), "fx_version 'cerulean'\n");
    }

    #[test]
    fn remove_replace_rename() {
        let text = "fx_version 'cerulean'\n  lua54 'yes'  \ngame 'gta5' lua54 'yes'\n";
        let view = View::new(&Source::new(text));
        let lua54: Vec<&Decl> = view.named("lua54").collect();
        assert_eq!(apply(text, &[remove_statement(text, lua54[0])]), "fx_version 'cerulean'\ngame 'gta5' lua54 'yes'\n");
        assert_eq!(apply(text, &[remove_statement(text, lua54[1])]), "fx_version 'cerulean'\n  lua54 'yes'  \ngame 'gta5' \n");
        let fx = view.first_named("fx_version").unwrap();
        assert_eq!(apply(text, &[replace_statement(fx, "x")]), text.replacen("fx_version 'cerulean'", "x", 1));
        assert_eq!(apply(text, &[rename(fx, "fx")]), text.replacen("fx_version", "fx", 1));
        let text = "game \"gta6\"";
        let view = View::new(&Source::new(text));
        let v = view.decls[0].values().next().unwrap();
        assert_eq!(apply(text, &[replace_value(text, v, "gta5")]), "game \"gta5\"");
    }
}
