use super::bridge::At;
use super::web::NUI_EVENT_PREFIX;
use crate::lua::lexer::Tok;
use crate::lua::{Call, Chain, Seg, Source};
use crate::syntax::scope::{DeclKind, Scopes};
use crate::syntax::tree::{Kind, Span, Tree};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

pub const CALLBACK_FUNCTIONS: &[&str] =
    &["RegisterNUICallback", "RegisterNuiCallback", "RegisterRawNUICallback", "RegisterRawNuiCallback"];
pub const TYPE_FUNCTIONS: &[&str] = &["RegisterNuiCallbackType"];
pub const MESSAGE_FUNCTIONS: &[(&str, usize)] = &[("SendNUIMessage", 0), ("SendNuiMessage", 0), ("SendDuiMessage", 1)];
const CONSTRUCTORS: &[&str] = &["new", "create", "New", "Create"];
const MAX_ARGS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LuaKind {
    Callback,
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FnKind {
    Global,
    Field,
    Method,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedFn {
    pub path: String,
    pub kind: FnKind,
}

impl NamedFn {
    pub fn class(&self) -> Option<&str> {
        (self.kind == FnKind::Method).then(|| self.path.rsplit_once(':').map_or("", |(c, _)| c))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LuaName {
    pub name: String,
    pub literal: usize,
    pub at: At,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LuaWrapper {
    pub kind: LuaKind,
    pub function: NamedFn,
    pub param: usize,
    pub field: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub callbacks: Vec<LuaName>,
    pub messages: Vec<(String, LuaName)>,
    pub wrappers: Vec<LuaWrapper>,
    pub dynamic: bool,
    pub call_names: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Str(LuaName),
    Const(String),
    Param(usize),
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LuaCall {
    pub callee: String,
    pub colon: bool,
    pub at: At,
    pub args: Vec<Arg>,
    pub function: Option<NamedFn>,
    pub receiver_class: Option<String>,
}

impl LuaCall {
    pub fn receiver(&self) -> Option<&str> {
        self.colon.then(|| self.callee.rsplit_once(':').map_or("", |(r, _)| r))
    }
}

pub fn simple_name(path: &str) -> &str {
    path.rsplit([':', '.']).next().unwrap_or(path)
}

pub fn name_hash(name: &str) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

pub struct Analysis<'a> {
    pub src: &'a Source,
    pub tree: Tree,
    pub scopes: Scopes,
    pub chains: Vec<Chain>,
    pub calls: Vec<Call>,
    args: Vec<Vec<Span>>,
    named: HashMap<usize, NamedFn>,
}

pub fn split(src: &Source, tree: &Tree, from: usize, to: usize) -> Vec<Span> {
    split_at(src, tree, from, to, &[","])
}

fn split_at(src: &Source, tree: &Tree, from: usize, to: usize, seps: &[&str]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = from;
    let mut i = from;
    while i < to {
        if src.is_kw(i, "function")
            && let Some(f) = tree.func_at(i)
        {
            i = tree.funcs[f].next.max(i + 1);
            continue;
        }
        if seps.iter().any(|s| src.is_op(i, s)) {
            if i > start {
                out.push((start, i));
            }
            start = i + 1;
        } else if let Some(c) = src.pair[i].filter(|&c| c > i) {
            i = c;
        }
        i += 1;
    }
    if to > start {
        out.push((start, to));
    }
    out
}

impl<'a> Analysis<'a> {
    pub fn new(src: &'a Source) -> Self {
        let tree = Tree::new(src);
        let scopes = Scopes::new(src, &tree);
        let chains = src.chains();
        let calls = src.calls(&chains);
        // `Call::args` splits at the commas of the functions passed as arguments: split again past them.
        let args = calls
            .iter()
            .map(|c| {
                if src.is_op(c.open, "(") {
                    split(src, &tree, c.open + 1, c.close.unwrap_or(src.toks.len()))
                } else {
                    c.args.iter().copied().filter(|(a, b)| b > a).collect()
                }
            })
            .collect();
        let mut a = Self { src, tree, scopes, chains, calls, args, named: HashMap::new() };
        a.named = a.named_functions();
        a
    }

    pub fn plain_callee(&self, call: &Call) -> Option<&str> {
        (call.seg == 0).then(|| self.chains[call.chain].head.as_str())
    }

    pub fn args(&self, i: usize) -> &[Span] {
        &self.args[i]
    }

    fn named_functions(&self) -> HashMap<usize, NamedFn> {
        let src = self.src;
        let mut out = HashMap::new();
        let top = |f: usize| self.tree.funcs[f].parent.is_none();
        for s in self.tree.statements() {
            match &s.kind {
                Kind::Function { name: (a, b), func } if b > a => {
                    let path: String = (*a..*b).map(|i| src.tok_text(i)).collect();
                    let kind = if path.contains(':') {
                        FnKind::Method
                    } else if path.contains('.') {
                        FnKind::Field
                    } else {
                        FnKind::Global
                    };
                    out.insert(*func, NamedFn { path, kind });
                }
                Kind::LocalFunction { name, func } if top(*func) => {
                    out.insert(*func, NamedFn { path: src.tok_text(*name).to_owned(), kind: FnKind::Local });
                }
                Kind::Local { names, values, unpack: false } => {
                    for (n, v) in names.iter().zip(values) {
                        let name = src.tok_text(n.tok);
                        if let Some(f) = self.closure(*v) {
                            if top(f) {
                                out.insert(f, NamedFn { path: name.to_owned(), kind: FnKind::Local });
                            }
                        } else {
                            self.table_functions(name, *v, &mut out);
                        }
                    }
                }
                Kind::Assign { targets, values, unpack: false } => {
                    for (t, v) in targets.iter().zip(values) {
                        let Some(path) = path_of(src, *t) else { continue };
                        if let Some(f) = self.closure(*v) {
                            let kind = if t.1 > t.0 + 1 {
                                FnKind::Field
                            } else if self.scopes.decl_of(t.0).is_some() {
                                if !top(f) {
                                    continue;
                                }
                                FnKind::Local
                            } else {
                                FnKind::Global
                            };
                            out.insert(f, NamedFn { path, kind });
                        } else {
                            self.table_functions(&path, *v, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn table_functions(&self, path: &str, (a, b): Span, out: &mut HashMap<usize, NamedFn>) {
        if !self.src.is_op(a, "{") || self.src.pair[a] != Some(b - 1) {
            return;
        }
        for (fa, fb) in table_fields(self.src, &self.tree, a) {
            if self.src.is_name(fa)
                && self.src.is_op(fa + 1, "=")
                && let Some(f) = self.closure((fa + 2, fb))
            {
                out.insert(f, NamedFn { path: format!("{path}.{}", self.src.tok_text(fa)), kind: FnKind::Field });
            }
        }
    }

    pub fn closure(&self, s: Span) -> Option<usize> {
        let (a, b) = unparen(self.src, s);
        let f = self.tree.func_at(a)?;
        (self.src.is_kw(a, "function") && self.tree.funcs[f].next == b).then_some(f)
    }

    pub fn enclosing_named(&self, tok: usize) -> Option<&NamedFn> {
        let mut f = self.tree.owner(tok);
        while let Some(i) = f {
            if let Some(n) = self.named.get(&i) {
                return Some(n);
            }
            f = self.tree.funcs[i].parent;
        }
        None
    }

    fn enclosing_named_index(&self, tok: usize) -> Option<usize> {
        let mut f = self.tree.owner(tok);
        while let Some(i) = f {
            if self.named.contains_key(&i) {
                return Some(i);
            }
            f = self.tree.funcs[i].parent;
        }
        None
    }

    pub fn param_of(&self, s: Span, at: usize) -> Option<usize> {
        let (a, b) = unparen(self.src, s);
        if self.src.is_name(a)
            && self.src.tok_text(a) == "tostring"
            && self.src.is_op(a + 1, "(")
            && self.src.pair[a + 1] == Some(b - 1)
        {
            return self.param_of((a + 2, b - 1), at);
        }
        if b != a + 1 || !self.src.is_name(a) {
            return None;
        }
        let d = self.scopes.decl(a)?;
        let named = self.enclosing_named_index(at)?;
        (d.kind == DeclKind::Param && d.func == Some(named) && !d.assigned).then_some(d.index)
    }

    fn constant(&self, s: Span) -> Option<String> {
        let (a, b) = unparen(self.src, s);
        if b != a + 1 || !self.src.is_name(a) {
            return None;
        }
        let d = self.scopes.decl(a)?;
        let (ia, ib) = d.init.filter(|_| d.kind == DeclKind::Local && !d.assigned)?;
        (ib == ia + 1).then(|| string_value(self.src, ia).map(|(v, _)| v)).flatten()
    }

    pub fn message_table(&self, s: Span, depth: usize) -> Option<usize> {
        let src = self.src;
        let (a, b) = unparen(src, s);
        if depth > 3 || b <= a {
            return None;
        }
        if src.is_op(a, "{") && src.pair[a] == Some(b - 1) {
            return Some(a);
        }
        if is_json_encode(src, a) && src.pair[a + 3] == Some(b - 1) {
            let arg = split(src, &self.tree, a + 4, b - 1).into_iter().next()?;
            return self.message_table(arg, depth + 1);
        }
        if b == a + 1 && src.is_name(a) {
            let d = self.scopes.decl(a).filter(|d| d.kind == DeclKind::Local)?;
            return self.message_table(d.init?, depth + 1);
        }
        None
    }

    fn arg(&self, s: Span, at: usize) -> Arg {
        if s.1 == s.0 + 1
            && let Some(name) = lua_name(self.src, s.0)
        {
            return Arg::Str(name);
        }
        if let Some(p) = self.param_of(s, at) {
            return Arg::Param(p);
        }
        self.constant(s).map_or(Arg::Other, Arg::Const)
    }

    fn receiver_class(&self, chain: &Chain, seg: usize) -> Option<String> {
        let src = self.src;
        if seg != 1 || !matches!(chain.segs.first(), Some(Seg::Method { .. })) {
            return None;
        }
        let d = self.scopes.decl(chain.start).filter(|d| d.kind == DeclKind::Local)?;
        let (a, b) = unparen(src, d.init?);
        let global = |i: usize| src.is_name(i) && self.scopes.decl(i).is_none();
        if src.tok_text(a) == "setmetatable" && src.is_op(a + 1, "(") && src.pair[a + 1] == Some(b - 1) {
            let (ma, mb) = *split(src, &self.tree, a + 2, b - 1).get(1)?;
            let (ma, mb) = unparen(src, (ma, mb));
            return (mb == ma + 1 && global(ma)).then(|| src.tok_text(ma).to_owned());
        }
        let ctor = global(a)
            && (src.is_op(a + 1, ".") || src.is_op(a + 1, ":"))
            && CONSTRUCTORS.contains(&src.tok_text(a + 2))
            && src.is_op(a + 3, "(")
            && src.pair[a + 3] == Some(b - 1);
        ctor.then(|| src.tok_text(a).to_owned())
    }
}

pub fn unparen(src: &Source, (mut a, mut b): Span) -> Span {
    while b > a + 1 && src.is_op(a, "(") && src.pair[a] == Some(b - 1) {
        a += 1;
        b -= 1;
    }
    (a, b)
}

fn is_json_encode(src: &Source, a: usize) -> bool {
    src.is_name(a)
        && src.tok_text(a) == "json"
        && src.is_op(a + 1, ".")
        && src.tok_text(a + 2) == "encode"
        && src.is_op(a + 3, "(")
}

fn path_of(src: &Source, (a, b): Span) -> Option<String> {
    if !src.is_name(a) {
        return None;
    }
    let mut path = src.tok_text(a).to_owned();
    let mut i = a + 1;
    while i < b {
        if !(src.is_op(i, ".") && src.is_name(i + 1)) {
            return None;
        }
        path.push('.');
        path.push_str(src.tok_text(i + 1));
        i += 2;
    }
    Some(path)
}

pub fn table_fields(src: &Source, tree: &Tree, open: usize) -> Vec<Span> {
    match src.pair[open] {
        Some(close) => split_at(src, tree, open + 1, close, &[",", ";"]),
        None => Vec::new(),
    }
}

pub fn field_key(src: &Source, (a, b): Span) -> Option<(String, Span)> {
    if src.is_name(a) && src.is_op(a + 1, "=") {
        return Some((src.tok_text(a).to_owned(), (a + 2, b)));
    }
    if src.is_op(a, "[") && src.pair[a] == Some(a + 2) && src.is_op(a + 3, "=") {
        return Some((string_value(src, a + 1)?.0, (a + 4, b)));
    }
    None
}

pub fn string_value(src: &Source, tok: usize) -> Option<(String, bool)> {
    let t = src.toks.get(tok).filter(|t| t.is_string())?;
    let (s, e) = t.content_range(&src.text);
    let raw = &src.text[s..e];
    if t.kind == Tok::LongStr || !raw.contains('\\') {
        return Some((raw.to_owned(), true));
    }
    let b = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&c) = b.get(i) else { break };
        i += 1;
        match c {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'\n' | b'\r' => {
                // A line break counts once (`\r\n`, `\n\r`).
                if b.get(i).is_some_and(|&n| (n == b'\n' || n == b'\r') && n != c) {
                    i += 1;
                }
                out.push(b'\n');
            }
            b'z' => {
                while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
            }
            b'x' => match raw.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 2;
                }
                None => out.push(b'x'),
            },
            b'u' if b.get(i) == Some(&b'{') => {
                let close = raw[i..].find('}').map(|c| i + c);
                match close.and_then(|c| u32::from_str_radix(&raw[i + 1..c], 16).ok().zip(Some(c))) {
                    Some((v, c)) => {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(char::from_u32(v).unwrap_or('\u{fffd}').encode_utf8(&mut buf).as_bytes());
                        i = c + 1;
                    }
                    None => out.push(b'u'),
                }
            }
            b'0'..=b'9' => {
                let mut v = u32::from(c - b'0');
                let mut n = 1;
                while n < 3 && b.get(i).is_some_and(u8::is_ascii_digit) {
                    v = v * 10 + u32::from(b[i] - b'0');
                    i += 1;
                    n += 1;
                }
                out.push(u8::try_from(v).unwrap_or(b'?'));
            }
            other => out.push(other),
        }
    }
    Some((String::from_utf8_lossy(&out).into_owned(), false))
}

fn lua_name(src: &Source, tok: usize) -> Option<LuaName> {
    lua_name_skip(src, tok, 0)
}

fn lua_name_skip(src: &Source, tok: usize, skip: usize) -> Option<LuaName> {
    let (value, exact) = string_value(src, tok)?;
    let name = value.get(skip..)?.to_owned();
    let t = src.toks[tok];
    let start = t.content_range(&src.text).0 + skip;
    let len = name.len();
    Some(LuaName { at: At::new(src, start, len, exact, t.end), name, literal: t.start })
}

impl Facts {
    pub fn read(src: &Source) -> Self {
        let t = &src.text;
        if ["NUI", "Nui", "DuiMessage", NUI_EVENT_PREFIX].iter().any(|n| t.contains(n)) {
            return Self::of(&Analysis::new(src));
        }
        let mut call_names: Vec<u64> = src
            .calls(&src.chains())
            .iter()
            .filter(|c| c.args.iter().any(|(a, b)| b > a))
            .map(|c| name_hash(src.tok_text(c.name_tok)))
            .collect();
        call_names.sort_unstable();
        call_names.dedup();
        Self { call_names, ..Self::default() }
    }

    pub fn of(an: &Analysis) -> Self {
        let src = an.src;
        let mut f = Facts::default();
        for (i, call) in an.calls.iter().enumerate() {
            if !an.args(i).is_empty() {
                f.call_names.push(name_hash(src.tok_text(call.name_tok)));
            }
            let Some(callee) = an.plain_callee(call) else { continue };
            let first = an.args(i).first().copied();
            if CALLBACK_FUNCTIONS.contains(&callee) || TYPE_FUNCTIONS.contains(&callee) {
                let Some(arg) = first else { continue };
                if arg.1 == arg.0 + 1 && src.toks[arg.0].is_string() {
                    if let Some(name) = lua_name(src, arg.0).filter(|n| !n.name.is_empty()) {
                        f.callbacks.push(name);
                    }
                } else if let Some(param) = an.param_of(arg, call.name_tok)
                    && let Some(function) = an.enclosing_named(call.name_tok)
                {
                    f.wrappers.push(LuaWrapper {
                        kind: LuaKind::Callback,
                        function: function.clone(),
                        param,
                        field: None,
                    });
                } else {
                    f.dynamic = true;
                }
            } else if callee == "AddEventHandler" {
                if let Some((a, b)) = first
                    && b == a + 1
                    && let Some(name) = lua_name_skip(src, a, NUI_EVENT_PREFIX.len())
                    && string_value(src, a).is_some_and(|(v, _)| v.starts_with(NUI_EVENT_PREFIX))
                    && !name.name.is_empty()
                {
                    f.callbacks.push(name);
                }
            } else if let Some(&(_, index)) = MESSAGE_FUNCTIONS.iter().find(|(n, _)| *n == callee)
                && let Some(arg) = an.args(i).get(index).copied()
                && let Some(table) = an.message_table(arg, 0)
            {
                for field in table_fields(src, &an.tree, table) {
                    let Some((key, (va, vb))) = field_key(src, field) else { continue };
                    if vb == va + 1 && src.toks[va].is_string() {
                        if let Some(name) = lua_name(src, va).filter(|n| !n.name.is_empty()) {
                            f.messages.push((key, name));
                        }
                    } else if vb == va + 1
                        && src.is_name(va)
                        && let Some(param) = an.param_of((va, vb), call.name_tok)
                        && let Some(function) = an.enclosing_named(call.name_tok)
                    {
                        f.wrappers.push(LuaWrapper {
                            kind: LuaKind::Message,
                            function: function.clone(),
                            param,
                            field: Some(key),
                        });
                    }
                }
            }
        }
        f.call_names.sort_unstable();
        f.call_names.dedup();
        f
    }
}

fn callee_path(chain: &Chain, seg: usize) -> Option<(String, bool)> {
    let segs = &chain.segs[..seg.min(chain.segs.len())];
    let mut path = chain.head.clone();
    for (k, s) in segs.iter().enumerate() {
        match s {
            Seg::Field { name, .. } => {
                path.push('.');
                path.push_str(name);
            }
            Seg::Method { name, .. } if k + 1 == segs.len() => return Some((format!("{path}:{name}"), true)),
            _ => {
                return match segs.last() {
                    Some(Seg::Method { name, .. }) => Some((format!("?:{name}"), true)),
                    _ => None,
                };
            }
        }
    }
    Some((path, false))
}

pub fn calls(an: &Analysis) -> Vec<LuaCall> {
    let src = an.src;
    let mut out = Vec::new();
    for (i, call) in an.calls.iter().enumerate() {
        let chain = &an.chains[call.chain];
        let Some((callee, colon)) = callee_path(chain, call.seg) else { continue };
        let args: Vec<Arg> = an.args(i).iter().take(MAX_ARGS).map(|&s| an.arg(s, call.name_tok)).collect();
        if args.is_empty() {
            continue;
        }
        let t = src.toks[call.name_tok];
        out.push(LuaCall {
            callee,
            colon,
            at: At::new(src, t.start, t.end - t.start, false, t.end),
            args,
            function: an.enclosing_named(call.name_tok).cloned(),
            receiver_class: if colon { an.receiver_class(chain, call.seg) } else { None },
        });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Message,
    Callback,
    Wrapper { path: String, colon: bool, arg: usize },
}

pub fn slot(an: &Analysis, k: usize, keys: &[String]) -> Option<Slot> {
    let src = an.src;
    // `{ action = '|' }` / `{ ['action'] = '|' }` in a message table.
    if k >= 3 && src.is_op(k - 1, "=") {
        let key = if src.is_name(k - 2) {
            Some((src.tok_text(k - 2).to_owned(), k - 2))
        } else if src.is_op(k - 2, "]") && src.pair[k - 2].is_some_and(|o| o + 2 == k - 2) {
            src.pair[k - 2].and_then(|o| string_value(src, o + 1).map(|(v, _)| (v, o)))
        } else {
            None
        };
        if let Some((key, field_start)) = key
            && let Some(p) = field_start.checked_sub(1)
            && (src.is_op(p, "{") || src.is_op(p, ",") || src.is_op(p, ";"))
        {
            let open = enclosing_brace(src, p)?;
            return (keys.contains(&key) && is_message(an, open)).then_some(Slot::Message);
        }
    }
    for (i, call) in an.calls.iter().enumerate() {
        let Some(index) = an.args(i).iter().position(|&s| s == (k, k + 1)) else { continue };
        if index == 0
            && let Some(callee) = an.plain_callee(call)
            && (CALLBACK_FUNCTIONS.contains(&callee) || TYPE_FUNCTIONS.contains(&callee))
        {
            return Some(Slot::Callback);
        }
        let (path, colon) = callee_path(&an.chains[call.chain], call.seg)?;
        return Some(Slot::Wrapper { path, colon, arg: index });
    }
    None
}

fn enclosing_brace(src: &Source, p: usize) -> Option<usize> {
    if src.is_op(p, "{") {
        return Some(p);
    }
    let mut i = p;
    while i > 0 {
        i -= 1;
        let t = src.toks[i];
        if t.kind == Tok::Op {
            match t.text(&src.text) {
                ")" | "]" | "}" => i = src.pair[i]?,
                "{" => return Some(i),
                "(" | "[" => return None,
                _ => {}
            }
        }
    }
    None
}

fn is_message(an: &Analysis, open: usize) -> bool {
    let src = an.src;
    let Some(close) = src.pair[open] else { return false };
    let table = (open, close + 1);
    let message_call = |i: usize, arg: Span| {
        an.plain_callee(&an.calls[i])
            .and_then(|c| MESSAGE_FUNCTIONS.iter().find(|(n, _)| *n == c))
            .is_some_and(|&(_, index)| an.args(i).get(index) == Some(&arg))
    };
    for (i, call) in an.calls.iter().enumerate() {
        if an.args(i).first() != Some(&table) {
            continue;
        }
        if message_call(i, table) {
            return true;
        }
        // `json.encode({ … })` passed at the message index.
        let chain = &an.chains[call.chain];
        if call.seg == 1 && chain.path(1) == "json.encode" {
            let encoded = (chain.start, call.close.map_or(src.toks.len(), |c| c + 1));
            return (0..an.calls.len()).any(|outer| message_call(outer, encoded));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(text: &str) -> (Source, Facts) {
        let src = Source::new(text);
        let f = Facts::of(&Analysis::new(&src));
        (src, f)
    }

    #[test]
    fn callbacks_messages_and_dynamic_registrations() {
        let (src, f) = facts(
            "RegisterNUICallback('buyItem', function(data, cb) cb(true) end)\nRegisterNuiCallbackType [[close]]\n\
             AddEventHandler('__cfx_nui:refund', function() end)\nAddEventHandler('__cfx_nui:', f)\nRegisterNUICallback('', f)\n\
             SendNUIMessage({ action = 'open', ['type'] = \"ui\", other = 1 })\nSendDuiMessage(dui, json.encode({ action = 'dui' }))\n\
             local msg = { action = 'held' }\nSendNuiMessage(msg)",
        );
        let names: Vec<&str> = f.callbacks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["buyItem", "close", "refund"]);
        let refund = &f.callbacks[2];
        assert_eq!(&src.text[refund.at.start..refund.at.start + refund.at.len], "refund");
        assert!(refund.at.exact);
        assert_eq!(&src.text[refund.literal..refund.literal + 1], "'");
        let msgs: Vec<String> = f.messages.iter().map(|(k, m)| format!("{k}={}", m.name)).collect();
        assert_eq!(msgs, ["action=open", "type=ui", "action=dui", "action=held"]);
        assert!(!f.dynamic);
        let (_, f) = facts(
            "local actions = { 'a' }\nfor _, name in ipairs(actions) do\n  RegisterNUICallback(name, function(d, cb) cb(1) end)\nend",
        );
        assert!(f.dynamic && f.callbacks.is_empty() && f.wrappers.is_empty());
    }

    #[test]
    fn escaped_names_are_decoded_but_not_exact() {
        let (src, f) = facts("RegisterNUICallback('buy\\x49tem\\65', f)");
        assert_eq!(f.callbacks[0].name, "buyItemA");
        assert!(!f.callbacks[0].at.exact);
        assert_eq!(string_value(&Source::new("'a\\z   b\\u{48}\\\nc'"), 0).map(|v| v.0).as_deref(), Some("abH\nc"));
        let at = &f.callbacks[0].at;
        assert_eq!(&src.text[at.start..at.end], "buy\\x49tem\\65'", "not exact: the rest of the literal");
    }

    #[test]
    fn wrappers_of_parameters() {
        let (_, f) = facts(
            "local function SendReactMessage(action, data)\n  SendNUIMessage({ action = action, data = data })\nend\n\
             function RegisterUiCallback(name, handler)\n  RegisterNUICallback(name, function(data, cb) handler(data, cb) end)\nend\n\
             function M.reg(name) RegisterNUICallback(tostring(name), f) end\n\
             function M:send(kind) SendNUIMessage({ type = kind }) end\n\
             function Reassigned(name) name = name or 'x' RegisterNUICallback(name, f) end",
        );
        let w: Vec<String> = f
            .wrappers
            .iter()
            .map(|w| {
                format!(
                    "{:?} {} {:?}#{}@{}",
                    w.kind,
                    w.function.path,
                    w.function.kind,
                    w.param,
                    w.field.as_deref().unwrap_or("-")
                )
            })
            .collect();
        assert_eq!(
            w,
            [
                "Message SendReactMessage Local#0@action",
                "Message SendReactMessage Local#1@data",
                "Callback RegisterUiCallback Global#0@-",
                "Callback M.reg Field#0@-",
                "Message M:send Method#0@type",
            ]
        );
        assert!(f.dynamic, "a reassigned parameter is computed");
    }

    #[test]
    fn calls_of_wrappers_with_their_arguments() {
        let src = Source::new(
            "local NAME = 'fromConst'\nfunction ShowPanel(action) SendUi(action, {}) end\nShowPanel('lit')\nShowPanel(NAME)\n\
             local z = Zone.new()\nz:open('x')\nfunction Zone:show() self:open('y') end\nlocal p = setmetatable({}, Panel)\np:open('z')",
        );
        let an = Analysis::new(&src);
        let calls = calls(&an);
        let find = |callee: &str| calls.iter().filter(|c| c.callee == callee).collect::<Vec<_>>();
        let send = find("SendUi");
        assert_eq!(send[0].args[0], Arg::Param(0));
        assert_eq!(send[0].function.as_ref().map(|f| f.path.as_str()), Some("ShowPanel"));
        let show = find("ShowPanel");
        assert!(matches!(&show[0].args[0], Arg::Str(n) if n.name == "lit"));
        assert_eq!(show[1].args[0], Arg::Const("fromConst".into()));
        assert_eq!(find("z:open")[0].receiver_class.as_deref(), Some("Zone"));
        let own = find("self:open");
        assert_eq!(own[0].function.as_ref().and_then(|f| f.class()), Some("Zone"));
        assert_eq!(find("p:open")[0].receiver_class.as_deref(), Some("Panel"));
        assert_eq!(simple_name("a.b:c"), "c");
    }

    #[test]
    fn completion_slots() {
        let keys = ["action".to_owned(), "type".to_owned()];
        let slot_of = |text: &str| {
            let src = Source::new(text);
            let an = Analysis::new(&src);
            let k = src.toks.iter().position(|t| t.text(&src.text) == "'x'").unwrap();
            slot(&an, k, &keys)
        };
        assert_eq!(slot_of("SendNUIMessage({ action = 'x' })"), Some(Slot::Message));
        assert_eq!(slot_of("SendNuiMessage(json.encode({ action = 'x' }))"), Some(Slot::Message));
        assert_eq!(slot_of("SendDuiMessage(dui, json.encode({ action = 'x' }))"), Some(Slot::Message));
        assert_eq!(slot_of("SendDuiMessage(dui, { action = 'x' })"), None);
        assert_eq!(slot_of("SendNUIMessage({ other = 'x' })"), None);
        assert_eq!(slot_of("RegisterNUICallback('x', f)"), Some(Slot::Callback));
        assert_eq!(slot_of("Ui.show(a, 'x')"), Some(Slot::Wrapper { path: "Ui.show".into(), colon: false, arg: 1 }));
        assert_eq!(slot_of("local t = { action = 'x' }"), None);
    }
}
