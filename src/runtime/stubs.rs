use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Shared,
    Client,
    Server,
    Manifest,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Shared, Part::Client, Part::Server, Part::Manifest];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shared => "Shared",
            Self::Client => "Client",
            Self::Server => "Server",
            Self::Manifest => "Manifest",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Shared => "Cfx runtime API (shared)",
            Self::Client => "Cfx runtime API (client)",
            Self::Server => "Cfx runtime API (server)",
            Self::Manifest => "Cfx resource manifest",
        }
    }
}

pub const FILES: &[(Part, &str, &str)] = &[
    (Part::Shared, "cfxlua_std.lua", include_str!("../../data/runtime/shared/cfxlua_std.lua")),
    (Part::Shared, "citizen.lua", include_str!("../../data/runtime/shared/citizen.lua")),
    (Part::Shared, "events.lua", include_str!("../../data/runtime/shared/events.lua")),
    (Part::Shared, "exports.lua", include_str!("../../data/runtime/shared/exports.lua")),
    (Part::Shared, "json.lua", include_str!("../../data/runtime/shared/json.lua")),
    (Part::Shared, "msgpack.lua", include_str!("../../data/runtime/shared/msgpack.lua")),
    (Part::Shared, "promise.lua", include_str!("../../data/runtime/shared/promise.lua")),
    (Part::Shared, "statebags.lua", include_str!("../../data/runtime/shared/statebags.lua")),
    (Part::Shared, "vectors.lua", include_str!("../../data/runtime/shared/vectors.lua")),
    (Part::Client, "client_citizen.lua", include_str!("../../data/runtime/client/client_citizen.lua")),
    (Part::Client, "client_events.lua", include_str!("../../data/runtime/client/client_events.lua")),
    (Part::Client, "client_nui.lua", include_str!("../../data/runtime/client/client_nui.lua")),
    (Part::Client, "client_statebags.lua", include_str!("../../data/runtime/client/client_statebags.lua")),
    (Part::Server, "server_citizen.lua", include_str!("../../data/runtime/server/server_citizen.lua")),
    (Part::Server, "server_events.lua", include_str!("../../data/runtime/server/server_events.lua")),
    (Part::Server, "server_http.lua", include_str!("../../data/runtime/server/server_http.lua")),
    (Part::Server, "server_os_io.lua", include_str!("../../data/runtime/server/server_os_io.lua")),
    (Part::Server, "server_players.lua", include_str!("../../data/runtime/server/server_players.lua")),
    (Part::Manifest, "fxmanifest.lua", include_str!("../../data/runtime/manifest/fxmanifest.lua")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Class,
    Global,
    Local,
    Function,
    Method,
    Field,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Typed {
    pub name: String,
    pub ty: String,
    pub desc: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Annotations {
    pub description: String,
    pub class: Option<String>,
    pub parents: Vec<String>,
    pub fields: Vec<Typed>,
    pub params: Vec<Typed>,
    pub returns: Vec<Typed>,
    pub ty: Option<String>,
    pub overloads: Vec<String>,
    pub deprecated: Option<String>,
    pub see: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Decl {
    pub name: String,
    pub kind: Kind,
    pub owner: Option<String>,
    pub colon: bool,
    pub function: bool,
    pub params: Vec<String>,
    pub doc: Annotations,
    pub part: Part,
    pub file: &'static str,
    pub children: Vec<usize>,
}

impl Decl {
    pub fn is_function(&self) -> bool {
        self.function
    }

    pub fn is_var(&self) -> bool {
        matches!(self.kind, Kind::Global | Kind::Local | Kind::Field) && !self.function
    }

    pub fn label(&self) -> String {
        if self.is_function() { format!("{}({})", self.name, self.params.join(", ")) } else { self.name.clone() }
    }

    pub fn qualified(&self) -> String {
        match &self.owner {
            Some(o) => format!("{o}{}{}", if self.colon { ":" } else { "." }, self.name),
            None => self.name.clone(),
        }
    }

    pub fn typed_params(&self) -> Vec<Typed> {
        self.params
            .iter()
            .map(|p| {
                let doc = self.doc.params.iter().find(|d| d.name.trim_end_matches('?') == p);
                doc.cloned().unwrap_or_else(|| Typed { name: p.clone(), ty: "any".into(), desc: String::new() })
            })
            .collect()
    }

    pub fn var_type(&self) -> Option<&str> {
        self.doc.ty.as_deref().or(self.doc.class.as_deref())
    }
}

#[derive(Clone, Debug)]
pub struct Global {
    pub decl: usize,
    pub table: Option<String>,
    pub extension: bool,
}

pub struct Runtime {
    pub decls: Vec<Decl>,
    pub tops: [Vec<usize>; 4],
    globals: HashMap<String, Global>,
    members: HashMap<String, Vec<usize>>,
    classes: HashMap<String, usize>,
}

pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::parse(FILES))
}

impl Runtime {
    pub fn parse(files: &[(Part, &'static str, &str)]) -> Self {
        let mut rt = Self {
            decls: Vec::new(),
            tops: Default::default(),
            globals: HashMap::new(),
            members: HashMap::new(),
            classes: HashMap::new(),
        };
        for &(part, file, text) in files {
            let src = Source::new(text);
            let tops = FileParser { rt: &mut rt, src: &src, part, file, vars: HashMap::new(), tops: Vec::new() }.run();
            rt.tops[part as usize].extend(tops);
        }
        rt.sort();
        rt
    }

    pub fn decl(&self, i: usize) -> &Decl {
        &self.decls[i]
    }

    pub fn global(&self, name: &str) -> Option<&Global> {
        self.globals.get(name)
    }

    pub fn globals(&self) -> impl Iterator<Item = (&str, &Global)> {
        self.globals.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn members(&self, table: &str) -> &[usize] {
        self.members.get(table).map_or(&[], Vec::as_slice)
    }

    pub fn class(&self, name: &str) -> Option<&Decl> {
        self.classes.get(name).map(|&i| &self.decls[i])
    }

    pub fn is_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    pub fn table_of<'a>(&self, ty: &'a str) -> Option<&'a str> {
        let first = ty.split(',').next().unwrap_or(ty);
        first
            .split('|')
            .map(|t| t.trim().trim_end_matches('?'))
            .find(|t| self.classes.contains_key(*t) || self.members.contains_key(*t))
    }

    fn table_key(&self, decl: usize) -> Option<String> {
        let d = &self.decls[decl];
        if let Some(c) = &d.doc.class {
            return Some(c.clone());
        }
        match d.kind {
            Kind::Global => self.globals.get(&d.name).and_then(|g| g.table.clone()),
            _ => None,
        }
    }

    fn sort(&mut self) {
        let key = |d: &Decl| d.label().to_lowercase();
        for part in 0..self.tops.len() {
            let mut tops = std::mem::take(&mut self.tops[part]);
            tops.sort_by_cached_key(|&i| (self.decls[i].children.is_empty(), key(&self.decls[i])));
            self.tops[part] = tops;
        }
        for i in 0..self.decls.len() {
            let mut children = std::mem::take(&mut self.decls[i].children);
            children.sort_by_cached_key(|&c| key(&self.decls[c]));
            self.decls[i].children = children;
        }
    }
}

struct FileParser<'a, 's> {
    rt: &'a mut Runtime,
    src: &'s Source,
    part: Part,
    file: &'static str,
    vars: HashMap<String, usize>,
    tops: Vec<usize>,
}

impl FileParser<'_, '_> {
    fn run(mut self) -> Vec<usize> {
        let mut i = 0;
        while i < self.src.toks.len() {
            i = self.statement(i).unwrap_or(i + 1);
        }
        self.tops
    }

    fn statement(&mut self, i: usize) -> Option<usize> {
        let src = self.src;
        if src.is_kw(i, "function") {
            let (path, colon, open) = self.path(i + 1);
            let params = self.params(open)?;
            let end = skip_function(src, src.pair[open]? + 1);
            let doc = self.doc_before(i);
            match path.as_slice() {
                [] => {}
                [name] => {
                    let d = self.add(name, Kind::Function, doc);
                    self.function(d, params);
                    self.tops.push(d);
                    self.declare_global(name, d, None);
                }
                [owner @ .., name] => {
                    let parent = self.owner(owner);
                    let d = self.add(name, Kind::Method, doc);
                    self.function(d, params);
                    self.rt.decls[d].colon = colon;
                    self.attach(parent, d);
                }
            }
            return Some(end);
        }
        if src.is_kw(i, "local") {
            let doc = self.doc_before(i);
            if src.is_kw(i + 1, "function") && src.is_name(i + 2) {
                let params = self.params(i + 3)?;
                let end = skip_function(src, src.pair[i + 3]? + 1);
                let d = self.add(src.tok_text(i + 2), Kind::Local, doc);
                self.function(d, params);
                self.tops.push(d);
                return Some(end);
            }
            if !src.is_name(i + 1) {
                return None;
            }
            let name = src.tok_text(i + 1);
            let class = doc.class.clone();
            let d = self.add(
                class.as_deref().unwrap_or(name),
                if class.is_some() { Kind::Class } else { Kind::Local },
                doc,
            );
            self.tops.push(d);
            self.vars.insert(name.to_owned(), d);
            if let Some(c) = class {
                self.rt.classes.entry(c).or_insert(d);
            }
            return Some(if src.is_op(i + 2, "=") { self.value(i + 3, d) } else { i + 2 });
        }
        if !src.is_name(i) || !statement_start(src, i) {
            return None;
        }
        let (path, colon, eq) = self.path(i);
        if colon || !src.is_op(eq, "=") {
            return None;
        }
        let doc = self.doc_before(i);
        match path.as_slice() {
            [name] => {
                if self.vars.contains_key(name) || self.rt.globals.contains_key(name) {
                    return Some(eq + 1);
                }
                let class = doc.class.clone();
                let table = class.clone().or_else(|| src.is_op(eq + 1, "{").then(|| name.clone()));
                let d = self.add(name, Kind::Global, doc);
                self.tops.push(d);
                self.vars.insert(name.clone(), d);
                if let Some(c) = class {
                    self.rt.classes.entry(c).or_insert(d);
                }
                self.declare_global(name, d, table);
                Some(self.value(eq + 1, d))
            }
            [owner @ .., name] => {
                let parent = self.owner(owner);
                if self.rt.decls[parent].children.iter().any(|&c| self.rt.decls[c].name == *name) {
                    return Some(eq + 1);
                }
                let d = self.add(name, Kind::Field, doc);
                self.attach(parent, d);
                Some(self.value(eq + 1, d))
            }
            [] => None,
        }
    }

    fn path(&self, mut i: usize) -> (Vec<String>, bool, usize) {
        let src = self.src;
        let mut names = Vec::new();
        while src.is_name(i) {
            names.push(src.tok_text(i).to_owned());
            i += 1;
            if src.is_op(i, ".") && src.is_name(i + 1) {
                i += 1;
            } else if src.is_op(i, ":") && src.is_name(i + 1) {
                names.push(src.tok_text(i + 1).to_owned());
                return (names, true, i + 2);
            } else {
                break;
            }
        }
        (names, false, i)
    }

    fn params(&self, open: usize) -> Option<Vec<String>> {
        let src = self.src;
        if !src.is_op(open, "(") {
            return None;
        }
        let close = src.pair[open]?;
        Some(
            (open + 1..close)
                .filter(|&k| src.is_name(k) || src.is_op(k, "..."))
                .map(|k| src.tok_text(k).to_owned())
                .collect(),
        )
    }

    fn value(&mut self, i: usize, d: usize) -> usize {
        let src = self.src;
        if src.is_kw(i, "function") {
            return match (self.params(i + 1), src.pair.get(i + 1).copied().flatten()) {
                (Some(params), Some(close)) => {
                    self.function(d, params);
                    skip_function(src, close + 1)
                }
                _ => i + 1,
            };
        }
        let Some(close) = src.is_op(i, "{").then(|| src.pair[i]).flatten() else { return i + 1 };
        let mut k = i + 1;
        while k < close {
            let field =
                src.is_name(k) && src.is_op(k + 1, "=") && [",", ";", "{"].iter().any(|op| src.is_op(k - 1, op));
            if field {
                let doc = self.doc_before(k);
                let f = self.add(src.tok_text(k), Kind::Field, doc);
                self.attach(d, f);
                k = self.value(k + 2, f);
                continue;
            }
            k = match src.toks[k].kind {
                Tok::Op if matches!(src.tok_text(k), "(" | "[" | "{") => src.pair[k].map_or(k + 1, |c| c + 1),
                _ => k + 1,
            };
        }
        close + 1
    }

    fn function(&mut self, d: usize, params: Vec<String>) {
        let decl = &mut self.rt.decls[d];
        decl.function = true;
        decl.params = params;
        decl.kind = match decl.kind {
            Kind::Global => Kind::Function,
            Kind::Field => Kind::Method,
            k => k,
        };
    }

    fn owner(&mut self, path: &[String]) -> usize {
        let root = &path[0];
        let mut node = match self.vars.get(root) {
            Some(&d) => d,
            None => {
                let d = self.add(root, Kind::Global, Annotations::default());
                self.tops.push(d);
                self.vars.insert(root.clone(), d);
                match self.rt.globals.get(root) {
                    // Members of a global declared by another file (`Citizen` in client_citizen.lua).
                    Some(g) => {
                        let class = g.table.clone().filter(|t| self.rt.classes.contains_key(t));
                        self.rt.decls[d].doc.class = class;
                    }
                    None => {
                        self.rt
                            .globals
                            .insert(root.clone(), Global { decl: d, table: Some(root.clone()), extension: true });
                    }
                }
                d
            }
        };
        for name in &path[1..] {
            let found = self.rt.decls[node].children.iter().copied().find(|&c| self.rt.decls[c].name == *name);
            node = match found {
                Some(c) => c,
                None => {
                    let c = self.add(name, Kind::Field, Annotations::default());
                    self.attach(node, c);
                    c
                }
            };
        }
        node
    }

    fn attach(&mut self, parent: usize, child: usize) {
        let table = self.rt.table_key(parent);
        self.rt.decls[child].owner = Some(table.clone().unwrap_or_else(|| self.rt.decls[parent].name.clone()));
        self.rt.decls[parent].children.push(child);
        if let Some(table) = table {
            self.rt.members.entry(table).or_default().push(child);
        }
    }

    fn declare_global(&mut self, name: &str, decl: usize, table: Option<String>) {
        self.rt.globals.entry(name.to_owned()).or_insert(Global { decl, table, extension: false });
    }

    fn add(&mut self, name: &str, kind: Kind, doc: Annotations) -> usize {
        self.rt.decls.push(Decl {
            name: name.to_owned(),
            kind,
            owner: None,
            colon: false,
            function: false,
            params: Vec::new(),
            doc,
            part: self.part,
            file: self.file,
            children: Vec::new(),
        });
        self.rt.decls.len() - 1
    }

    fn doc_before(&self, i: usize) -> Annotations {
        let src = self.src;
        let Some(tok) = src.toks.get(i) else { return Annotations::default() };
        let mut k = src.all.partition_point(|t| t.start < tok.start);
        let mut next = tok.start;
        let mut lines = Vec::new();
        while k > 0 {
            let c = src.all[k - 1];
            let body = c.text(&src.text);
            let gap = &src.text[c.end..next];
            if c.kind != Tok::LineComment
                || !body.starts_with("---")
                || gap.matches('\n').count() != 1
                || !gap.trim().is_empty()
            {
                break;
            }
            lines.push(body);
            next = c.start;
            k -= 1;
        }
        lines.reverse();
        annotations(&lines)
    }
}

fn statement_start(src: &Source, i: usize) -> bool {
    let Some(p) = i.checked_sub(1) else { return true };
    let text = src.tok_text(p);
    match src.toks[p].kind {
        Tok::Keyword => matches!(text, "nil" | "true" | "false" | "end" | "do" | "then" | "else" | "repeat" | "break"),
        Tok::Op => matches!(text, ";" | ")" | "]" | "}"),
        _ => true,
    }
}

fn skip_function(src: &Source, mut i: usize) -> usize {
    let mut depth = 1usize;
    while i < src.toks.len() {
        if src.toks[i].kind == Tok::Keyword {
            match src.tok_text(i) {
                "function" | "if" | "do" | "repeat" => depth += 1,
                "end" | "until" => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    i
}

pub fn annotations(lines: &[&str]) -> Annotations {
    let mut a = Annotations::default();
    let mut description: Vec<&str> = Vec::new();
    for line in lines {
        let body = line.strip_prefix("---").unwrap_or(line);
        let Some(tag) = body.trim_start().strip_prefix('@') else {
            description.push(body.strip_prefix(' ').unwrap_or(body));
            continue;
        };
        let (name, rest) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let rest = rest.trim();
        match name {
            "class" => {
                let (class, parents) = rest.split_once(':').unwrap_or((rest, ""));
                a.class = Some(class.trim().to_owned());
                a.parents = parents.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect();
            }
            "field" => {
                let rest =
                    ["public ", "protected ", "private "].iter().find_map(|v| rest.strip_prefix(v)).unwrap_or(rest);
                if let Some(t) = named(rest) {
                    a.fields.push(t);
                }
            }
            "param" => {
                if let Some(t) = named(rest) {
                    a.params.push(t);
                }
            }
            "vararg" => {
                let (ty, desc) = split_type(rest);
                a.params.push(Typed { name: "...".into(), ty, desc });
            }
            "return" => {
                let (ty, desc) = split_type(rest);
                a.returns.push(Typed { name: String::new(), ty, desc });
            }
            "type" => a.ty = Some(split_type(rest).0),
            "overload" => a.overloads.push(rest.to_owned()),
            "deprecated" => a.deprecated = Some(rest.to_owned()),
            "see" => a.see.push(rest.to_owned()),
            _ => {}
        }
    }
    a.description = description.join("\n").trim_matches('\n').trim_end().to_owned();
    a
}

fn named(rest: &str) -> Option<Typed> {
    let (name, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if name.is_empty() {
        return None;
    }
    let (ty, desc) = split_type(rest.trim());
    Some(Typed { name: name.to_owned(), ty, desc })
}

fn split_type(rest: &str) -> (String, String) {
    if let Some((ty, desc)) = rest.split_once(" @") {
        return (ty.trim().to_owned(), desc.trim().to_owned());
    }
    if let Some(desc) = rest.strip_prefix('@') {
        return (String::new(), desc.trim().to_owned());
    }
    // A type ends at whitespace outside brackets and quotes, unless a `|` continues it.
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut end = rest.len();
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    for (n, &(i, c)) in chars.iter().enumerate() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '(' | '[' | '{' | '<') => depth += 1,
            (None, ')' | ']' | '}' | '>') => depth -= 1,
            (None, c) if c.is_whitespace() && depth <= 0 => {
                let prev = rest[..i].trim_end().chars().next_back();
                let next = chars[n..].iter().map(|&(_, c)| c).find(|c| !c.is_whitespace());
                if prev != Some('|') && prev != Some(',') && next != Some('|') {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    (rest[..end].trim().to_owned(), rest[end..].trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(rt: &'a Runtime, part: Part, label: &str) -> &'a Decl {
        rt.tops[part as usize]
            .iter()
            .map(|&i| rt.decl(i))
            .find(|d| d.label() == label)
            .unwrap_or_else(|| panic!("no {label}"))
    }

    fn children(rt: &Runtime, d: &Decl) -> Vec<String> {
        d.children.iter().map(|&c| rt.decl(c).label()).collect()
    }

    #[test]
    fn annotation_tags() {
        let a = annotations(&[
            "---",
            "--- Creates a thread.",
            "---",
            "--- * item",
            "---   continued",
            "---@overload fun(routine:fun()):boolean",
            "---@param routine fun(a:string, b:string) @The function.",
            "---@param name? string Optional name.",
            "---@vararg any",
            "---@return integer @The id.",
            "---@return string|nil, integer",
            "---@deprecated Use x.",
            "---@generic K, V",
            "---@field public x number @First.",
            "---@class Child : Base, Other",
            "---@type CfxExports|fun(name:string)",
        ]);
        assert_eq!(a.description, "Creates a thread.\n\n* item\n  continued");
        assert_eq!(
            a.params[0],
            Typed { name: "routine".into(), ty: "fun(a:string, b:string)".into(), desc: "The function.".into() }
        );
        assert_eq!(a.params[1], Typed { name: "name?".into(), ty: "string".into(), desc: "Optional name.".into() });
        assert_eq!(a.params[2].name, "...");
        assert_eq!(a.returns[0].ty, "integer");
        assert_eq!(a.returns[1].ty, "string|nil, integer");
        assert_eq!(a.overloads, ["fun(routine:fun()):boolean"]);
        assert_eq!(a.deprecated.as_deref(), Some("Use x."));
        assert_eq!(a.fields[0].name, "x");
        assert_eq!((a.class.as_deref(), a.parents.len()), (Some("Child"), 2));
        assert_eq!(a.ty.as_deref(), Some("CfxExports|fun(name:string)"));
    }

    #[test]
    fn structure_rules() {
        let text = "-- header\n\n---@class Thing\nlocal ThingType = {}\n\n---Does it.\n---@param x number @X.\nfunction ThingType:do_it(x) end\n\
                    function ThingType.static() end\nThingType.size = nil\n\n---@class Lib\nLib = {}\nfunction Lib.f(a, ...) end\nLib = nil\n\
                    function math.round(x) end\nmath.eps = nil\nlocal cfg = { a = 1, b = function(x) end }\nfunction Top() end\n\
                    one = {}\none.only = nil\nAddEventHandler('x', function() local inner = 1 end)\n";
        let rt = Runtime::parse(&[(Part::Shared, "a.lua", text)]);
        let tops: Vec<String> = rt.tops[0].iter().map(|&i| rt.decl(i).label()).collect();
        assert_eq!(tops, ["cfg", "Lib", "math", "one", "Thing", "Top()"]);
        let thing = find(&rt, Part::Shared, "Thing");
        assert_eq!(thing.kind, Kind::Class);
        assert_eq!(children(&rt, thing), ["do_it(x)", "size", "static()"]);
        let do_it = rt.decl(thing.children[0]);
        assert_eq!((do_it.qualified().as_str(), do_it.doc.description.as_str()), ("Thing:do_it", "Does it."));
        assert_eq!(do_it.typed_params()[0].ty, "number");
        assert_eq!(children(&rt, find(&rt, Part::Shared, "Lib")), ["f(a, ...)"]);
        assert_eq!(children(&rt, find(&rt, Part::Shared, "math")), ["eps", "round(x)"]);
        assert_eq!(children(&rt, find(&rt, Part::Shared, "cfg")), ["a", "b(x)"]);
        assert!(rt.global("math").unwrap().extension && !rt.global("Lib").unwrap().extension);
        assert_eq!(rt.global("Lib").unwrap().table.as_deref(), Some("Lib"));
        assert_eq!(rt.members("Thing").len(), 3);
        assert_eq!(rt.table_of("Thing|nil"), Some("Thing"));
        assert!(rt.global("inner").is_none() && rt.global("cfg").is_none());
        let top = find(&rt, Part::Shared, "Top()");
        assert_eq!((top.kind, top.is_function()), (Kind::Function, true));
        assert_eq!(children(&rt, find(&rt, Part::Shared, "one")), ["only"]);
    }

    #[test]
    fn bundled_stubs() {
        let rt = runtime();
        let citizen = find(rt, Part::Shared, "Citizen");
        assert_eq!(citizen.kind, Kind::Global);
        assert!(children(rt, citizen).contains(&"CreateThread(routine)".to_owned()));
        let create =
            rt.decl(rt.members("Citizen").iter().copied().find(|&m| rt.decl(m).name == "CreateThread").unwrap());
        assert_eq!(create.qualified(), "Citizen.CreateThread");
        assert_eq!(create.doc.returns[0].ty, "integer");
        assert!(create.doc.description.starts_with("Creates a new scheduler thread"));
        assert!(
            rt.members("Citizen").iter().any(|&m| rt.decl(m).name == "GetNative" && rt.decl(m).part == Part::Client)
        );
        assert_eq!(rt.global("promise").unwrap().table, None);
        assert_eq!(rt.decl(rt.global("promise").unwrap().decl).doc.ty.as_deref(), Some("PromiseLib"));
        assert_eq!(rt.members("PromiseLib").len(), 4);
        assert_eq!(rt.members("promise").iter().filter(|&&m| rt.decl(m).colon).count(), 3);
        assert_eq!(rt.members("MsgpackPacker").len(), 49);
        assert!(rt.class("CfxPlayer").unwrap().doc.fields.iter().any(|f| f.name == "state" && f.ty == "StateBag"));
        assert!(rt.global("os").unwrap().extension && rt.decl(rt.global("os").unwrap().decl).part == Part::Server);
        let math: Vec<&str> = rt.members("math").iter().map(|&m| rt.decl(m).name.as_str()).collect();
        assert!(math.contains(&"eps") && math.contains(&"feps") && math.contains(&"clamp"), "{math:?}");
        assert_eq!(rt.decl(rt.global("source").unwrap().decl).doc.ty.as_deref(), Some("integer"));
        let parts: Vec<usize> = rt.tops.iter().map(Vec::len).collect();
        assert!(parts.iter().all(|n| *n > 0), "{parts:?}");
        assert_eq!(rt.tops[Part::Manifest as usize].len(), 44);
        // Classes and tables come first in a part.
        let shared: Vec<bool> = rt.tops[0].iter().map(|&i| rt.decl(i).children.is_empty()).collect();
        assert!(shared.windows(2).all(|w| w[0] <= w[1]));
    }
}
