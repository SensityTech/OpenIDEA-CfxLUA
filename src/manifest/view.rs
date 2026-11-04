//! A manifest's directives as the checks and completion read them, on top of
//! [`crate::project::manifest::entries`]: which argument each value belongs
//! to (`data_file 'TYPE' 'path'`), whether the directive is written in table
//! form (FXServer records `client_scripts { … }` as `client_script`, but
//! `client_scripts 'x'` as `client_scripts`), and whether it is a statement.

use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::project::manifest::entries;
use crate::project::{Entry, Value};

/// One directive call of a manifest.
#[derive(Clone, Debug)]
pub struct Decl {
    pub entry: Entry,
    /// For each value of `entry.values`: its argument (0: the value, 1: the chained extra value, 2: a later one).
    pub arg: Vec<u8>,
    /// The first argument is a table (`name { … }`, `name({ … })`).
    pub table_form: bool,
    /// A whole statement (not nested in an expression or a table).
    pub statement: bool,
}

impl Decl {
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// Values with their argument index, first and extra arguments only.
    pub fn args(&self) -> impl Iterator<Item = (usize, &Value)> {
        self.entry.values.iter().zip(&self.arg).filter(|(_, a)| **a < 2).map(|(v, a)| (*a as usize, v))
    }

    /// The values of the first argument.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.args().filter(|(a, _)| *a == 0).map(|(_, v)| v)
    }

    /// The name FXServer records: the `s` of a table form removed (`games { … }` → `game`).
    pub fn metadata_key(&self) -> &str {
        let name = self.name();
        if self.table_form { name.strip_suffix('s').unwrap_or(name) } else { name }
    }
}

/// The directives of a manifest, in order.
pub struct View {
    pub decls: Vec<Decl>,
}

impl View {
    pub fn new(src: &Source) -> Self {
        let depths = depths(src);
        let decls = entries(src)
            .into_iter()
            .map(|entry| {
                let args = arg_ranges(src, entry.name_tok);
                let arg = entry
                    .values
                    .iter()
                    .map(|v| args.iter().position(|&(a, b)| (a..b).contains(&v.tok)).map_or(2, |i| i.min(2) as u8))
                    .collect();
                let table_form = args.first().is_some_and(|&(a, b)| is_table_arg(src, a, b));
                let statement = depths.get(entry.name_tok) == Some(&0) && starts_statement(src, entry.name_tok);
                Decl { entry, arg, table_form, statement }
            })
            .collect();
        Self { decls }
    }

    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Decl> + 'a {
        self.decls.iter().filter(move |d| d.name() == name)
    }

    pub fn first_named(&self, name: &str) -> Option<&Decl> {
        self.decls.iter().find(|d| d.name() == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.first_named(name).is_some()
    }

    /// The directive whose value is the string token `tok`, with the value's argument index.
    pub fn value_at(&self, tok: usize) -> Option<(&Decl, usize, &Value)> {
        self.decls.iter().find_map(|d| d.args().find(|(_, v)| v.tok == tok).map(|(a, v)| (d, a, v)))
    }
}

/// Token ranges `[start, end)` of the call arguments after the name at `name_tok` (`'x'`, `{ … }`, `( … )`).
fn arg_ranges(src: &Source, name_tok: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = name_tok + 1;
    while let Some(t) = src.toks.get(i) {
        let end = if t.is_string() {
            i + 1
        } else if src.is_op(i, "{") || src.is_op(i, "(") {
            match src.pair[i] {
                Some(close) => close + 1,
                None => {
                    out.push((i, src.toks.len()));
                    break;
                }
            }
        } else {
            break;
        };
        out.push((i, end));
        i = end;
    }
    out
}

/// Whether the argument `[a, b)` is a table, or a parenthesised list starting with one.
fn is_table_arg(src: &Source, a: usize, b: usize) -> bool {
    if src.is_op(a, "{") {
        return true;
    }
    if !src.is_op(a, "(") {
        return false;
    }
    let close = src.pair[a].unwrap_or(b);
    src.split_args(a + 1, close)
        .first()
        .is_some_and(|&(s, e)| src.is_op(s, "{") && src.pair[s].is_some_and(|c| c + 1 == e))
}

/// Bracket depth before each code token.
fn depths(src: &Source) -> Vec<u32> {
    let mut depth = 0u32;
    src.toks
        .iter()
        .map(|t| {
            if t.kind != Tok::Op {
                return depth;
            }
            match t.text(&src.text) {
                "(" | "[" | "{" | "?[" => {
                    depth += 1;
                    depth - 1
                }
                ")" | "]" | "}" => {
                    depth = depth.saturating_sub(1);
                    depth
                }
                _ => depth,
            }
        })
        .collect()
}

/// Whether a statement may start at token `i`: the previous token ends a statement or opens a block.
pub fn starts_statement(src: &Source, i: usize) -> bool {
    let Some(p) = i.checked_sub(1).and_then(|p| src.toks.get(p)) else { return true };
    let text = p.text(&src.text);
    match p.kind {
        Tok::Name | Tok::Number | Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. } => true,
        Tok::Op => matches!(text, ")" | "]" | "}" | ";" | "..."),
        Tok::Keyword => matches!(text, "end" | "do" | "then" | "else" | "repeat" | "break" | "true" | "false" | "nil"),
        _ => false,
    }
}

/// Whether byte `at` is outside every bracket pair (at top level, or directly in a block).
pub fn at_depth_zero(src: &Source, at: usize) -> bool {
    let mut depth = 0i32;
    for t in src.toks.iter().take_while(|t| t.start < at) {
        if t.kind == Tok::Op {
            match t.text(&src.text) {
                "(" | "[" | "{" | "?[" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                _ => {}
            }
        }
    }
    depth <= 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(text: &str) -> View {
        View::new(&Source::new(text))
    }

    #[test]
    fn arguments_table_forms_and_statements() {
        let v = view(
            "games { 'gta5' }\ngames 'rdr3'\ndata_file 'HANDLING_FILE' 'data/x.meta'\nserver_scripts({ 'a.lua' }, 'b.lua')\n\
             client_scripts('c.lua', 'd.lua')\nif x then server_script 'e.lua' end\nlocal t = { f = foo 'x' }\nCreateThread(function() lua54 'yes' end)\n",
        );
        let names: Vec<&str> = v.decls.iter().map(Decl::name).collect();
        assert_eq!(
            names,
            [
                "games",
                "games",
                "data_file",
                "server_scripts",
                "client_scripts",
                "server_script",
                "foo",
                "CreateThread",
                "lua54"
            ]
        );
        assert_eq!(v.decls[0].metadata_key(), "game");
        assert_eq!(v.decls[1].metadata_key(), "games");
        let data: Vec<(usize, &str)> = v.decls[2].args().map(|(a, v)| (a, v.text.as_str())).collect();
        assert_eq!(data, [(0, "HANDLING_FILE"), (1, "data/x.meta")]);
        assert!(v.decls[3].table_form);
        assert_eq!(v.decls[3].values().count(), 2, "every string of a parenthesised list is a value");
        assert!(!v.decls[4].table_form);
        let statements: Vec<bool> = v.decls.iter().map(|d| d.statement).collect();
        assert_eq!(statements, [true, true, true, true, true, true, false, true, false]);
        let tok = v.decls[2].entry.values[1].tok;
        assert_eq!(v.value_at(tok).map(|(d, a, _)| (d.name(), a)), Some(("data_file", 1)));
    }
}
