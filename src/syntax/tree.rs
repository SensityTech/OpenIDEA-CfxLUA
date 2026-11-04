use crate::lua::Source;
use crate::lua::lexer::Tok;

pub type Span = (usize, usize);

const MAX_DEPTH: usize = 120;

pub const COMPOUND_OPS: &[&str] = &["+=", "-=", "*=", "/=", "<<=", ">>=", "&=", "|=", "^=", "//=", "%=", "..="];

pub struct Tree {
    pub block: Block,
    pub funcs: Vec<Func>,
    owner: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalName {
    pub tok: usize,
    pub attr: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Local { names: Vec<LocalName>, values: Vec<Span>, unpack: bool },
    LocalFunction { name: usize, func: usize },
    Function { name: Span, func: usize },
    Assign { targets: Vec<Span>, values: Vec<Span>, unpack: bool },
    Compound { target: Span, op: usize, value: Span },
    Expr(Span),
    If { conds: Vec<Span>, blocks: Vec<Block>, has_else: bool },
    While { cond: Span, body: Block },
    Do(Block),
    For { vars: Vec<usize>, exprs: Vec<Span>, body: Block },
    Repeat { body: Block, cond: Span },
    Return { exprs: Vec<Span> },
    Break,
    Goto,
    Label,
    Error,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub kw: usize,
    pub params: Vec<usize>,
    pub vararg: bool,
    pub close: Option<usize>,
    pub body: Block,
    pub end: Option<usize>,
    pub next: usize,
    pub parent: Option<usize>,
}

impl Tree {
    pub fn new(src: &Source) -> Self {
        let mut p = Parser { src, n: src.toks.len(), funcs: Vec::new(), stack: Vec::new(), depth: 0 };
        let mut stmts = Vec::new();
        let mut i = 0;
        loop {
            let b = p.block(i, &[]);
            stmts.extend(b.stmts);
            if b.end >= p.n {
                break;
            }
            // A stray `end` / `else` / `until` at the top level.
            stmts.push(Stmt { start: b.end, end: b.end + 1, kind: Kind::Error });
            i = b.end + 1;
        }
        let block = Block { stmts, end: p.n };
        let funcs = p.funcs;
        let mut owner = vec![0u32; src.toks.len()];
        for (k, f) in funcs.iter().enumerate() {
            for o in &mut owner[f.kw.min(src.toks.len())..f.next.min(src.toks.len())] {
                *o = k as u32 + 1;
            }
        }
        Self { block, funcs, owner }
    }

    pub fn owner(&self, tok: usize) -> Option<usize> {
        self.owner.get(tok).and_then(|&o| o.checked_sub(1)).map(|o| o as usize)
    }

    pub fn func_at(&self, kw: usize) -> Option<usize> {
        self.funcs.binary_search_by_key(&kw, |f| f.kw).ok()
    }

    pub fn statements(&self) -> Vec<&Stmt> {
        let mut out = Vec::new();
        collect(&self.block, &mut out);
        for f in &self.funcs {
            collect(&f.body, &mut out);
        }
        out
    }
}

fn collect<'a>(b: &'a Block, out: &mut Vec<&'a Stmt>) {
    for s in &b.stmts {
        out.push(s);
        for inner in s.blocks() {
            collect(inner, out);
        }
    }
}

impl Stmt {
    pub fn blocks(&self) -> Vec<&Block> {
        match &self.kind {
            Kind::If { blocks, .. } => blocks.iter().collect(),
            Kind::While { body, .. } | Kind::Do(body) | Kind::For { body, .. } | Kind::Repeat { body, .. } => {
                vec![body]
            }
            _ => Vec::new(),
        }
    }
}

struct Parser<'a> {
    src: &'a Source,
    n: usize,
    funcs: Vec<Func>,
    stack: Vec<usize>,
    depth: usize,
}

const END: &[&str] = &["end"];
const IF_TERMS: &[&str] = &["end", "else", "elseif"];
const UNTIL: &[&str] = &["until"];
const BLOCK_ENDS: &[&str] = &["end", "else", "elseif", "until"];

impl Parser<'_> {
    fn text(&self, i: usize) -> &str {
        self.src.tok_text(i)
    }

    fn kind(&self, i: usize) -> Option<Tok> {
        self.src.toks.get(i).map(|t| t.kind)
    }

    fn kw(&self, i: usize, kw: &str) -> bool {
        self.src.is_kw(i, kw)
    }

    fn op(&self, i: usize, op: &str) -> bool {
        self.src.is_op(i, op)
    }

    fn name(&self, i: usize) -> bool {
        self.src.is_name(i)
    }

    fn block(&mut self, mut i: usize, terms: &[&str]) -> Block {
        if self.depth >= MAX_DEPTH {
            return Block { stmts: Vec::new(), end: self.skip_flat(i, terms) };
        }
        self.depth += 1;
        let mut stmts = Vec::new();
        while i < self.n {
            if self.kind(i) == Some(Tok::Keyword) && BLOCK_ENDS.contains(&self.text(i)) {
                if terms.contains(&self.text(i)) || terms.is_empty() {
                    break;
                }
                stmts.push(Stmt { start: i, end: i + 1, kind: Kind::Error });
                i += 1;
                continue;
            }
            if self.op(i, ";") {
                i += 1;
                continue;
            }
            let s = self.stmt(i);
            i = s.end.max(i + 1);
            stmts.push(s);
        }
        self.depth -= 1;
        Block { stmts, end: i.min(self.n) }
    }

    fn skip_flat(&self, mut i: usize, terms: &[&str]) -> usize {
        let mut depth = 0usize;
        while i < self.n {
            if self.kind(i) == Some(Tok::Keyword) {
                match self.text(i) {
                    "function" | "if" | "do" | "repeat" => depth += 1,
                    t @ ("end" | "until") => {
                        if depth == 0 && terms.contains(&t) {
                            return i;
                        }
                        depth = depth.saturating_sub(1);
                    }
                    t @ ("else" | "elseif") if depth == 0 && terms.contains(&t) => return i,
                    _ => {}
                }
            }
            i += 1;
        }
        self.n
    }

    fn after_end(&self, at: usize) -> usize {
        if self.kw(at, "end") { at + 1 } else { at }
    }

    fn stmt(&mut self, i: usize) -> Stmt {
        if self.kind(i) == Some(Tok::Keyword) {
            match self.text(i) {
                "local" => return self.local(i),
                "function" => return self.function_stmt(i),
                "if" => return self.if_stmt(i),
                "while" => {
                    let c = self.expr(i + 1);
                    let from = if self.kw(c, "do") { c + 1 } else { c };
                    let body = self.block(from, END);
                    let end = self.after_end(body.end);
                    return Stmt { start: i, end, kind: Kind::While { cond: (i + 1, c), body } };
                }
                "do" => {
                    let body = self.block(i + 1, END);
                    let end = self.after_end(body.end);
                    return Stmt { start: i, end, kind: Kind::Do(body) };
                }
                "for" => return self.for_stmt(i),
                "repeat" => {
                    let body = self.block(i + 1, UNTIL);
                    let (cond, end) = if self.kw(body.end, "until") {
                        let c = self.expr(body.end + 1);
                        ((body.end + 1, c), c.max(body.end + 1))
                    } else {
                        ((body.end, body.end), body.end)
                    };
                    return Stmt { start: i, end, kind: Kind::Repeat { body, cond } };
                }
                "return" => {
                    let at_end = i + 1 >= self.n
                        || self.op(i + 1, ";")
                        || (self.kind(i + 1) == Some(Tok::Keyword) && BLOCK_ENDS.contains(&self.text(i + 1)));
                    let (exprs, mut end) = if at_end { (Vec::new(), i + 1) } else { self.expr_list(i + 1) };
                    end = end.max(i + 1);
                    if self.op(end, ";") {
                        end += 1;
                    }
                    return Stmt { start: i, end, kind: Kind::Return { exprs } };
                }
                "break" => return Stmt { start: i, end: i + 1, kind: Kind::Break },
                "goto" => {
                    let end = if self.name(i + 1) { i + 2 } else { i + 1 };
                    return Stmt { start: i, end, kind: Kind::Goto };
                }
                _ => {}
            }
        }
        if self.op(i, "::") {
            let end = if self.name(i + 1) && self.op(i + 2, "::") { i + 3 } else { i + 1 };
            return Stmt { start: i, end, kind: Kind::Label };
        }
        self.expr_stmt(i)
    }

    fn local(&mut self, i: usize) -> Stmt {
        if self.kw(i + 1, "function") {
            if self.name(i + 2) {
                let func = self.func(i + 1, i + 3);
                return Stmt { start: i, end: self.funcs[func].next, kind: Kind::LocalFunction { name: i + 2, func } };
            }
            let func = self.func(i + 1, i + 2);
            return Stmt { start: i, end: self.funcs[func].next, kind: Kind::Error };
        }
        let mut names = Vec::new();
        let mut j = i + 1;
        while self.name(j) {
            let tok = j;
            j += 1;
            let attr = if self.op(j, "<") && self.name(j + 1) && self.op(j + 2, ">") {
                j += 3;
                Some((j - 3, j - 1))
            } else {
                None
            };
            names.push(LocalName { tok, attr });
            if self.op(j, ",") && self.name(j + 1) {
                j += 1;
            } else {
                break;
            }
        }
        if names.is_empty() {
            return Stmt { start: i, end: i + 1, kind: Kind::Error };
        }
        if self.op(j, "=") {
            let (values, end) = self.expr_list(j + 1);
            return Stmt { start: i, end: end.max(j + 1), kind: Kind::Local { names, values, unpack: false } };
        }
        if self.kw(j, "in") {
            let e = self.expr(j + 1);
            let values = if e > j + 1 { vec![(j + 1, e)] } else { Vec::new() };
            return Stmt { start: i, end: e.max(j + 1), kind: Kind::Local { names, values, unpack: true } };
        }
        Stmt { start: i, end: j, kind: Kind::Local { names, values: Vec::new(), unpack: false } }
    }

    fn function_stmt(&mut self, i: usize) -> Stmt {
        let mut j = i + 1;
        if self.name(j) {
            j += 1;
            while self.op(j, ".") && self.name(j + 1) {
                j += 2;
            }
            if self.op(j, ":") && self.name(j + 1) {
                j += 2;
            }
        }
        let func = self.func(i, j);
        Stmt { start: i, end: self.funcs[func].next, kind: Kind::Function { name: (i + 1, j), func } }
    }

    fn if_stmt(&mut self, i: usize) -> Stmt {
        let mut conds = Vec::new();
        let mut blocks = Vec::new();
        let mut has_else = false;
        let mut at = i;
        let end;
        loop {
            let c = self.expr(at + 1);
            conds.push((at + 1, c));
            let from = if self.kw(c, "then") { c + 1 } else { c };
            let b = self.block(from, IF_TERMS);
            let stop = b.end;
            blocks.push(b);
            if self.kw(stop, "elseif") {
                at = stop;
                continue;
            }
            if self.kw(stop, "else") {
                has_else = true;
                let b = self.block(stop + 1, END);
                end = self.after_end(b.end);
                blocks.push(b);
            } else {
                end = self.after_end(stop);
            }
            break;
        }
        Stmt { start: i, end, kind: Kind::If { conds, blocks, has_else } }
    }

    fn for_stmt(&mut self, i: usize) -> Stmt {
        let mut vars = Vec::new();
        let mut j = i + 1;
        let exprs;
        if self.name(j) && self.op(j + 1, "=") {
            vars.push(j);
            let (e, end) = self.expr_list(j + 2);
            exprs = e;
            j = end;
        } else {
            while self.name(j) {
                vars.push(j);
                j += 1;
                if self.op(j, ",") {
                    j += 1;
                } else {
                    break;
                }
            }
            if self.kw(j, "in") {
                let (e, end) = self.expr_list(j + 1);
                exprs = e;
                j = end;
            } else {
                exprs = Vec::new();
            }
        }
        let from = if self.kw(j, "do") { j + 1 } else { j };
        let body = self.block(from, END);
        let end = self.after_end(body.end);
        Stmt { start: i, end, kind: Kind::For { vars, exprs, body } }
    }

    fn expr_stmt(&mut self, i: usize) -> Stmt {
        let e = self.suffixed(i);
        if e == i {
            // Nothing a statement starts with: a table or a parenthesis still holds closures.
            let end = if self.op(i, "{") || self.op(i, "[") { self.group(i) } else { i + 1 };
            return Stmt { start: i, end, kind: Kind::Error };
        }
        if self.op(e, ",") || self.op(e, "=") || self.kw(e, "in") {
            let mut targets = vec![(i, e)];
            let mut j = e;
            while self.op(j, ",") {
                let t = self.suffixed(j + 1);
                if t == j + 1 {
                    break;
                }
                targets.push((j + 1, t));
                j = t;
            }
            if self.op(j, "=") {
                let (values, end) = self.expr_list(j + 1);
                return Stmt { start: i, end: end.max(j + 1), kind: Kind::Assign { targets, values, unpack: false } };
            }
            if self.kw(j, "in") {
                let v = self.expr(j + 1);
                let values = if v > j + 1 { vec![(j + 1, v)] } else { Vec::new() };
                return Stmt { start: i, end: v.max(j + 1), kind: Kind::Assign { targets, values, unpack: true } };
            }
            return Stmt { start: i, end: e, kind: Kind::Expr((i, e)) };
        }
        if self.kind(e) == Some(Tok::Op) && COMPOUND_OPS.contains(&self.text(e)) {
            let v = self.expr(e + 1);
            return Stmt {
                start: i,
                end: v.max(e + 1),
                kind: Kind::Compound { target: (i, e), op: e, value: (e + 1, v) },
            };
        }
        Stmt { start: i, end: e, kind: Kind::Expr((i, e)) }
    }

    fn func(&mut self, kw: usize, at: usize) -> usize {
        let idx = self.funcs.len();
        self.funcs.push(Func {
            kw,
            params: Vec::new(),
            vararg: false,
            close: None,
            body: Block { stmts: Vec::new(), end: at },
            end: None,
            next: at,
            parent: self.stack.last().copied(),
        });
        let mut j = at;
        let (mut params, mut vararg, mut close) = (Vec::new(), false, None);
        if self.op(j, "(") {
            close = self.src.pair[j];
            let to = close.unwrap_or(self.n);
            for k in j + 1..to {
                if self.name(k) {
                    params.push(k);
                } else if self.op(k, "...") {
                    vararg = true;
                } else if !self.op(k, ",") {
                    break;
                }
            }
            j = close.map_or(to, |c| c + 1);
        }
        self.stack.push(idx);
        let body = self.block(j, END);
        self.stack.pop();
        let (end, next) = if self.kw(body.end, "end") { (Some(body.end), body.end + 1) } else { (None, body.end) };
        let f = &mut self.funcs[idx];
        f.params = params;
        f.vararg = vararg;
        f.close = close;
        f.body = body;
        f.end = end;
        f.next = next.max(at);
        idx
    }

    fn suffixed(&mut self, i: usize) -> usize {
        let mut j = if self.name(i) {
            i + 1
        } else if self.op(i, "(") {
            self.group(i)
        } else {
            return i;
        };
        while let Some(t) = self.src.toks.get(j) {
            match (t.kind, t.text(&self.src.text)) {
                (Tok::Op, "." | "?." | ":" | "?:") if self.name(j + 1) => j += 2,
                (Tok::Op, "[" | "?[" | "(" | "{") => j = self.group(j),
                (Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. }, _) => j += 1,
                // `f?(x)`, `f?{…}`, `f?'s'`: the lexer leaves a lone `?` unknown.
                (Tok::Unknown, "?")
                    if self.op(j + 1, "(")
                        || self.op(j + 1, "{")
                        || self
                            .kind(j + 1)
                            .is_some_and(|k| matches!(k, Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. })) =>
                {
                    j += 1
                }
                _ => break,
            }
        }
        j
    }

    fn group(&mut self, open: usize) -> usize {
        let close = self.src.pair[open];
        let to = close.unwrap_or(self.n);
        let mut k = open + 1;
        while k < to {
            if self.kw(k, "function") {
                let f = self.func(k, k + 1);
                k = self.funcs[f].next.max(k + 1);
                continue;
            }
            k += 1;
        }
        close.map_or(self.n, |c| (c + 1).max(k))
    }

    fn expr(&mut self, i: usize) -> usize {
        let mut i = i;
        loop {
            while self.unary(i) {
                i += 1;
            }
            let e = self.simple(i);
            if e == i {
                return i;
            }
            match self.binop(e) {
                Some(width) => i = e + width,
                None => return e,
            }
        }
    }

    fn expr_list(&mut self, i: usize) -> (Vec<Span>, usize) {
        let mut out = Vec::new();
        let e = self.expr(i);
        if e == i {
            return (out, i);
        }
        out.push((i, e));
        let mut j = e;
        while self.op(j, ",") {
            let e = self.expr(j + 1);
            if e > j + 1 {
                out.push((j + 1, e));
            }
            j = e.max(j + 1);
        }
        (out, j)
    }

    fn simple(&mut self, i: usize) -> usize {
        let Some(t) = self.src.toks.get(i) else { return i };
        match (t.kind, t.text(&self.src.text)) {
            (Tok::Number | Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. }, _) => i + 1,
            (Tok::Keyword, "nil" | "true" | "false") => i + 1,
            (Tok::Keyword, "function") => {
                let f = self.func(i, i + 1);
                self.funcs[f].next.max(i + 1)
            }
            (Tok::Op, "...") => i + 1,
            (Tok::Op, "{") => self.group(i),
            (Tok::Name, _) | (Tok::Op, "(") => self.suffixed(i),
            _ => i,
        }
    }

    fn unary(&self, i: usize) -> bool {
        self.src.toks.get(i).is_some_and(|t| match t.kind {
            Tok::Op => matches!(t.text(&self.src.text), "-" | "#" | "~" | "!"),
            Tok::Keyword => t.text(&self.src.text) == "not",
            _ => false,
        })
    }

    fn binop(&self, i: usize) -> Option<usize> {
        let t = self.src.toks.get(i)?;
        let text = t.text(&self.src.text);
        match t.kind {
            Tok::Keyword if matches!(text, "and" | "or") => Some(1),
            Tok::Op => match text {
                "&" | "|" => {
                    let doubled =
                        self.src.toks.get(i + 1).is_some_and(|n| n.start == t.end && n.text(&self.src.text) == text);
                    Some(if doubled { 2 } else { 1 })
                }
                "+" | "-" | "*" | "/" | "//" | "%" | "^" | ".." | "==" | "~=" | "!=" | "<" | ">" | "<=" | ">="
                | "~" | "<<" | ">>" => Some(1),
                _ => None,
            },
            _ => None,
        }
    }
}

pub fn is_var(src: &Source, (a, b): Span) -> bool {
    if b <= a || !src.is_name(a) {
        return false;
    }
    if b == a + 1 {
        return true;
    }
    let last = src.toks[b - 1];
    match last.kind {
        Tok::Name => src.is_op(b - 2, ".") || src.is_op(b - 2, "?."),
        Tok::Op if last.text(&src.text) == "]" => {
            src.pair[b - 1].is_some_and(|o| src.is_op(o, "[") || src.is_op(o, "?["))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Source, Tree) {
        let src = Source::new(text);
        let tree = Tree::new(&src);
        (src, tree)
    }

    fn kinds(tree: &Tree) -> Vec<&'static str> {
        tree.block
            .stmts
            .iter()
            .map(|s| match s.kind {
                Kind::Local { unpack: true, .. } => "local-in",
                Kind::Local { .. } => "local",
                Kind::LocalFunction { .. } => "local-function",
                Kind::Function { .. } => "function",
                Kind::Assign { unpack: true, .. } => "assign-in",
                Kind::Assign { .. } => "assign",
                Kind::Compound { .. } => "compound",
                Kind::Expr(_) => "expr",
                Kind::If { .. } => "if",
                Kind::While { .. } => "while",
                Kind::Do(_) => "do",
                Kind::For { .. } => "for",
                Kind::Repeat { .. } => "repeat",
                Kind::Return { .. } => "return",
                Kind::Break => "break",
                Kind::Goto => "goto",
                Kind::Label => "label",
                Kind::Error => "error",
            })
            .collect()
    }

    #[test]
    fn statements_of_cfxlua() {
        let (_, tree) = parse(
            "local a <const>, b = 1, 2\nlocal c in src\nt.count += a * 2 + 1\nx.a, x.b in src\nfunction M:m(x) end\n\
             local function f(...) return ... end\nif a then b() elseif c then else end\nfor i = 1, 3 do end\n\
             for k, v in pairs(t) do end\nwhile x do break end\nrepeat local z = 1 until z\ndo goto done end ::done::\ni++",
        );
        assert_eq!(
            kinds(&tree),
            [
                "local",
                "local-in",
                "compound",
                "assign-in",
                "function",
                "local-function",
                "if",
                "for",
                "for",
                "while",
                "repeat",
                "do",
                "label",
                "expr",
                "error",
            ]
        );
        assert_eq!(tree.funcs.len(), 2);
        assert!(tree.funcs[1].vararg);
    }

    #[test]
    fn closures_in_arguments_and_tables() {
        let (src, tree) = parse(
            "RegisterNUICallback('x', function(data, cb)\n  if data then return cb(1) end\nend)\n\
             local M = { open = function() end, [1] = function(a) end }",
        );
        assert_eq!(kinds(&tree), ["expr", "local"]);
        assert_eq!(tree.funcs.len(), 3);
        let cb = &tree.funcs[0];
        assert_eq!(cb.params.iter().map(|&p| src.tok_text(p)).collect::<Vec<_>>(), ["data", "cb"]);
        assert!(matches!(cb.body.stmts[0].kind, Kind::If { has_else: false, .. }));
        let at = src.toks.iter().position(|t| t.text(&src.text) == "return").unwrap();
        assert_eq!(tree.owner(at), Some(0));
        assert_eq!(tree.owner(0), None);
    }

    #[test]
    fn tolerates_broken_code() {
        let (_, tree) = parse("end\nfunction f(\n  else x = 1\n");
        assert_eq!(tree.funcs.len(), 1);
        let (_, tree) = parse("local x = function() if a then");
        assert_eq!(tree.funcs.len(), 1);
        let deep = "if x then ".repeat(500) + &"end ".repeat(500);
        let (_, tree) = parse(&deep);
        assert_eq!(kinds(&tree), ["if"]);
        let deep = "f(function() ".repeat(400);
        let (_, tree) = parse(&deep);
        assert!(tree.funcs.len() >= MAX_DEPTH / 2);
    }

    #[test]
    fn variables() {
        let src = Source::new("i t.n t[1] f() t:m() a.b.c");
        assert!(is_var(&src, (0, 1)));
        assert!(is_var(&src, (1, 4)));
        assert!(is_var(&src, (4, 8)));
        assert!(!is_var(&src, (8, 11)));
        assert!(!is_var(&src, (11, 16)));
        assert!(is_var(&src, (16, 21)));
    }
}
