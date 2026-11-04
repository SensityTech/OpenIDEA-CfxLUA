use crate::lua::lexer::Tok;
use crate::lua::{Seg, Source};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub tok: usize,
    pub in_table: bool,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub name_tok: usize,
    pub name_start: usize,
    pub name_end: usize,
    pub start: usize,
    pub end: usize,
    pub values: Vec<Value>,
    pub table: Option<(usize, Option<usize>)>,
    pub args: usize,
    pub has_keyed_fields: bool,
}

impl Entry {
    pub fn first(&self) -> Option<&Value> {
        self.values.first()
    }
}

pub fn entries(src: &Source) -> Vec<Entry> {
    let chains = src.chains();
    let mut out = Vec::new();
    for chain in &chains {
        // A directive is a plain name called directly: `name 'x'`, `name { }`, `name(…)`.
        let Some(first) = chain.segs.first() else { continue };
        if !matches!(first, Seg::Call { .. } | Seg::StrCall { .. } | Seg::TableCall { .. }) {
            continue;
        }
        if chain.start > 0 && (src.is_kw(chain.start - 1, "function") || src.is_kw(chain.start - 1, "local")) {
            continue;
        }
        let name_t = src.toks[chain.start];
        let mut e = Entry {
            name: chain.head.clone(),
            name_tok: chain.start,
            name_start: name_t.start,
            name_end: name_t.end,
            start: name_t.start,
            end: src.toks.get(chain.end.saturating_sub(1)).map_or(name_t.end, |t| t.end),
            values: Vec::new(),
            table: None,
            args: 0,
            has_keyed_fields: false,
        };
        // Curried calls count as one directive: `data_file 'TYPE' 'path'`, `convar_category 'X' { … }`.
        for seg in &chain.segs {
            match seg {
                Seg::StrCall { tok } => {
                    e.args += 1;
                    push_string(src, *tok, false, &mut e.values);
                    e.end = src.toks[*tok].end;
                }
                Seg::TableCall { open, close } => {
                    e.args += 1;
                    if e.table.is_none() {
                        e.table = Some((*open, *close));
                    }
                    collect_table(src, *open, close.unwrap_or(src.toks.len()), &mut e);
                    e.end = close.map_or(src.text.len(), |c| src.toks[c].end);
                }
                Seg::Call { close, args, .. } => {
                    e.end = close.map_or(src.text.len(), |c| src.toks[c].end);
                    for &(a, b) in args {
                        if a >= b {
                            continue;
                        }
                        e.args += 1;
                        if b == a + 1 {
                            push_string(src, a, false, &mut e.values);
                        } else if src.is_op(a, "{") {
                            let close = src.pair[a];
                            if e.table.is_none() {
                                e.table = Some((a, close));
                            }
                            collect_table(src, a, close.unwrap_or(b), &mut e);
                        }
                    }
                }
                _ => break,
            }
        }
        out.push(e);
    }
    out
}

fn push_string(src: &Source, tok: usize, in_table: bool, out: &mut Vec<Value>) {
    let t = src.toks[tok];
    if t.is_string() {
        out.push(Value { text: t.string_value(&src.text), start: t.start, end: t.end, tok, in_table });
    }
}

fn collect_table(src: &Source, open: usize, close: usize, e: &mut Entry) {
    for (a, b) in src.split_args(open + 1, close) {
        if a >= b {
            continue;
        }
        // `key = v` and `[k] = v` are keyed fields.
        let keyed = (src.is_name(a) && src.is_op(a + 1, "="))
            || (src.is_op(a, "[") && src.pair[a].is_some_and(|c| src.is_op(c + 1, "=")));
        if keyed {
            e.has_keyed_fields = true;
            continue;
        }
        if b == a + 1 {
            push_string(src, a, true, &mut e.values);
        } else if src.is_op(a, "{")
            && let Some(c) = src.pair[a]
        {
            collect_table(src, a, c, e);
        }
    }
}

pub fn at_statement_start(src: &Source, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let p = src.toks[i - 1];
    match p.kind {
        Tok::Str { .. } | Tok::LongStr => true,
        Tok::Op => matches!(p.text(&src.text), "}" | ")" | ";"),
        Tok::Keyword => matches!(p.text(&src.text), "end" | "do" | "then" | "else"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_form() {
        let src = Source::new(
            "fx_version 'cerulean'\ngame \"gta5\"\nclient_script 'client.lua'\nclient_scripts { 'client/*.lua', \"ui/**.lua\" }\n\
             server_scripts({ '@oxmysql/lib/MySQL.lua', 'server/main.lua' })\ndata_file 'DLC_ITYP_REQUEST' 'stream/x.ytyp'\n\
             -- client_script 'ignored.lua'\nconvar_category 'X' { 'desc', { { 'a', '$b', 'CV_BOOL', 'true' } } }\n",
        );
        let e = entries(&src);
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["fx_version", "game", "client_script", "client_scripts", "server_scripts", "data_file", "convar_category"]
        );
        assert_eq!(e[3].values.iter().map(|v| v.text.as_str()).collect::<Vec<_>>(), ["client/*.lua", "ui/**.lua"]);
        assert!(e[3].values[0].in_table);
        assert_eq!(e[4].values.len(), 2);
        assert_eq!(
            e[5].values.iter().map(|v| v.text.as_str()).collect::<Vec<_>>(),
            ["DLC_ITYP_REQUEST", "stream/x.ytyp"]
        );
        assert_eq!(e[6].values.len(), 6, "convar_category: name, description and nested positional fields");
    }
}
