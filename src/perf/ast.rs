use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Expr(usize),
    Stat(usize),
    Block(usize),
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Nil,
    True,
    False,
    Vararg,
    Number,
    Str,
    Hash,
    Name { name: String, decl: Option<usize> },
    Field { obj: usize, name: String, safe: bool },
    Index { obj: usize, key: usize },
    Call { callee: usize, method: Option<String>, args: Vec<usize> },
    Function { params: Vec<usize>, body: usize, ordinal: usize, method: bool },
    Table { fields: Vec<Field> },
    Binary { op: &'static str, lhs: usize, rhs: usize },
    Unary { op: &'static str, operand: usize },
    Paren(usize),
    Error,
}

#[derive(Clone, Debug)]
pub enum FieldKey {
    Positional,
    Named(String),
    Expr(usize),
    Set,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub key: FieldKey,
    pub value: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub first: usize,
    pub end: usize,
    pub start: usize,
    pub stop: usize,
    pub parent: Option<Node>,
}

#[derive(Clone, Debug)]
pub enum StatKind {
    Local { names: Vec<usize>, values: Vec<usize> },
    LocalFunction { decl: usize, func: usize },
    Function { target: usize, method: Option<String>, func: usize },
    Assign { targets: Vec<usize>, values: Vec<usize> },
    Compound { target: usize, op: &'static str, value: usize },
    Expr(usize),
    Do(usize),
    While { cond: usize, body: usize },
    Repeat { body: usize, cond: usize },
    If { conds: Vec<usize>, blocks: Vec<usize>, otherwise: Option<usize> },
    NumFor { var: usize, exprs: Vec<usize>, body: usize },
    GenFor { vars: Vec<usize>, exprs: Vec<usize>, body: usize },
    Return(Vec<usize>),
    Break,
    Goto,
    Label,
    Empty,
}

#[derive(Clone, Debug)]
pub struct Stat {
    pub kind: StatKind,
    pub first: usize,
    pub end: usize,
    pub start: usize,
    pub stop: usize,
    pub parent: usize,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stats: Vec<usize>,
    pub start: usize,
    pub stop: usize,
    pub parent: Option<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Local { stat: usize, index: usize },
    LocalFunction { stat: usize },
    Param { func: usize },
    ForVar { stat: usize },
}

#[derive(Clone, Debug)]
pub struct Decl {
    pub name: String,
    pub tok: usize,
    pub kind: DeclKind,
    pub from: usize,
    pub to: usize,
    pub init: Option<usize>,
    pub assigned: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Named {
    pub path: String,
    pub root: Option<usize>,
    pub func: usize,
}

pub struct Ast {
    pub exprs: Vec<Expr>,
    pub stats: Vec<Stat>,
    pub blocks: Vec<Block>,
    pub decls: Vec<Decl>,
    pub root: usize,
    pub owner: Vec<Option<Node>>,
    pub functions: Vec<usize>,
    by_name: HashMap<String, Vec<usize>>,
}

thread_local! {
    static CACHE: RefCell<Vec<(usize, u64, Rc<Ast>)>> = const { RefCell::new(Vec::new()) };
}

pub fn parse(src: &Source) -> Rc<Ast> {
    let key = (src.text.len(), fnv(&src.text));
    if let Some(ast) = CACHE.with(|c| c.borrow().iter().find(|(l, h, _)| (*l, *h) == key).map(|(_, _, a)| a.clone())) {
        return ast;
    }
    let ast = Rc::new(Ast::new(src));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 3 {
            c.remove(0);
        }
        c.push((key.0, key.1, ast.clone()));
    });
    ast
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

impl Ast {
    pub fn new(src: &Source) -> Self {
        let mut p = Parser::new(src);
        p.open_scope();
        let mut stats = Vec::new();
        while p.pos < p.limit {
            p.statements(&mut stats);
            // A stray closer at the top level: skip it and go on.
            if p.pos < p.limit {
                p.pos += 1;
            }
        }
        p.close_scope(src.toks.len());
        let root = p.ast.blocks.len();
        p.ast.blocks.push(Block { stats, start: 0, stop: src.text.len(), parent: None });
        p.ast.root = root;
        let mut ast = p.ast;
        ast.link();
        ast
    }

    pub fn span(&self, n: Node) -> (usize, usize) {
        match n {
            Node::Expr(e) => (self.exprs[e].start, self.exprs[e].stop),
            Node::Stat(s) => (self.stats[s].start, self.stats[s].stop),
            Node::Block(b) => (self.blocks[b].start, self.blocks[b].stop),
        }
    }

    pub fn text<'s>(&self, src: &'s Source, n: Node) -> &'s str {
        let (a, b) = self.span(n);
        &src.text[a..b]
    }

    pub fn parent(&self, n: Node) -> Option<Node> {
        match n {
            Node::Expr(e) => self.exprs[e].parent,
            Node::Stat(s) => Some(Node::Block(self.stats[s].parent)),
            Node::Block(b) => self.blocks[b].parent,
        }
    }

    pub fn is_ancestor(&self, a: Node, b: Node, strict: bool) -> bool {
        let mut n = if strict { self.parent(b) } else { Some(b) };
        while let Some(x) = n {
            if x == a {
                return true;
            }
            n = self.parent(x);
        }
        false
    }

    pub fn common(&self, a: Node, b: Node) -> Option<Node> {
        let mut n = Some(a);
        while let Some(x) = n {
            if self.is_ancestor(x, b, false) {
                return Some(x);
            }
            n = self.parent(x);
        }
        None
    }

    pub fn unwrap(&self, mut e: usize) -> usize {
        while let ExprKind::Paren(inner) = self.exprs[e].kind {
            e = inner;
        }
        e
    }

    pub fn is_function(&self, n: Node) -> bool {
        matches!(n, Node::Expr(e) if matches!(self.exprs[e].kind, ExprKind::Function { .. }))
    }

    pub fn owner_of(&self, tok: usize) -> Option<Node> {
        self.owner.get(tok).copied().flatten()
    }

    pub fn children(&self, n: Node, mut f: impl FnMut(Node)) {
        let mut ex = |e: usize| f(Node::Expr(e));
        match n {
            Node::Expr(e) => match &self.exprs[e].kind {
                ExprKind::Field { obj, .. } => ex(*obj),
                ExprKind::Index { obj, key } => {
                    ex(*obj);
                    ex(*key);
                }
                ExprKind::Call { callee, args, .. } => {
                    ex(*callee);
                    args.iter().for_each(|a| ex(*a));
                }
                ExprKind::Function { body, .. } => f(Node::Block(*body)),
                ExprKind::Table { fields } => {
                    for field in fields {
                        if let FieldKey::Expr(k) = field.key {
                            ex(k);
                        }
                        if let Some(v) = field.value {
                            ex(v);
                        }
                    }
                }
                ExprKind::Binary { lhs, rhs, .. } => {
                    ex(*lhs);
                    ex(*rhs);
                }
                ExprKind::Unary { operand, .. } => ex(*operand),
                ExprKind::Paren(inner) => ex(*inner),
                _ => {}
            },
            Node::Stat(s) => match &self.stats[s].kind {
                StatKind::Local { values, .. } => values.iter().for_each(|v| ex(*v)),
                StatKind::LocalFunction { func, .. } => ex(*func),
                StatKind::Function { target, func, .. } => {
                    ex(*target);
                    ex(*func);
                }
                StatKind::Assign { targets, values } => {
                    targets.iter().chain(values).for_each(|v| ex(*v));
                }
                StatKind::Compound { target, value, .. } => {
                    ex(*target);
                    ex(*value);
                }
                StatKind::Expr(e) => ex(*e),
                StatKind::Do(b) => f(Node::Block(*b)),
                StatKind::While { cond, body } => {
                    f(Node::Expr(*cond));
                    f(Node::Block(*body));
                }
                StatKind::Repeat { body, cond } => {
                    f(Node::Block(*body));
                    f(Node::Expr(*cond));
                }
                StatKind::If { conds, blocks, otherwise } => {
                    for (c, b) in conds.iter().zip(blocks) {
                        f(Node::Expr(*c));
                        f(Node::Block(*b));
                    }
                    if let Some(b) = otherwise {
                        f(Node::Block(*b));
                    }
                }
                StatKind::NumFor { exprs, body, .. } | StatKind::GenFor { exprs, body, .. } => {
                    exprs.iter().for_each(|e| f(Node::Expr(*e)));
                    f(Node::Block(*body));
                }
                StatKind::Return(es) => es.iter().for_each(|e| ex(*e)),
                StatKind::Break | StatKind::Goto | StatKind::Label | StatKind::Empty => {}
            },
            Node::Block(b) => self.blocks[b].stats.iter().for_each(|s| f(Node::Stat(*s))),
        }
    }

    pub fn walk(&self, root: Node, skip: &dyn Fn(Node) -> bool, visit: &mut dyn FnMut(Node)) {
        visit(root);
        self.children(root, |c| {
            if !self.is_function(c) && !skip(c) {
                self.walk(c, skip, visit);
            }
        });
    }

    pub fn walk_all(&self, root: Node, visit: &mut dyn FnMut(Node)) {
        visit(root);
        self.children(root, |c| self.walk_all(c, visit));
    }

    pub fn calls(&self, root: Node, skip: &dyn Fn(Node) -> bool) -> Vec<usize> {
        let mut out = Vec::new();
        self.post_calls(root, skip, &mut out);
        out
    }

    fn post_calls(&self, n: Node, skip: &dyn Fn(Node) -> bool, out: &mut Vec<usize>) {
        self.children(n, |c| {
            if !self.is_function(c) && !skip(c) {
                self.post_calls(c, skip, out);
            }
        });
        if let Node::Expr(e) = n
            && matches!(self.exprs[e].kind, ExprKind::Call { .. })
        {
            out.push(e);
        }
    }

    pub fn local_at(&self, name: &str, tok: usize) -> Option<usize> {
        self.by_name
            .get(name)?
            .iter()
            .copied()
            .filter(|&d| self.decls[d].from <= tok && tok < self.decls[d].to)
            .max_by_key(|&d| self.decls[d].from)
    }

    pub fn decl_of(&self, e: usize) -> Option<usize> {
        match &self.exprs[e].kind {
            ExprKind::Name { decl, .. } => *decl,
            _ => None,
        }
    }

    pub fn name_of(&self, e: usize) -> Option<&str> {
        match &self.exprs[e].kind {
            ExprKind::Name { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn local_initializer(&self, e: usize) -> Option<usize> {
        let d = self.decl_of(e)?;
        match self.decls[d].kind {
            DeclKind::Local { .. } => self.decls[d].init,
            _ => None,
        }
    }

    pub fn constant_initializer(&self, e: usize) -> Option<usize> {
        let d = self.decl_of(e)?;
        if !self.decls[d].assigned.is_empty() {
            return None;
        }
        self.local_initializer(e)
    }

    pub fn path(&self, e: usize) -> Option<String> {
        match &self.exprs[e].kind {
            ExprKind::Name { name, .. } => Some(name.clone()),
            ExprKind::Field { obj, name, .. } => Some(format!("{}.{name}", self.path(*obj)?)),
            _ => None,
        }
    }

    pub fn root_name(&self, mut e: usize) -> Option<usize> {
        loop {
            match &self.exprs[e].kind {
                ExprKind::Name { .. } => return Some(e),
                ExprKind::Field { obj, .. } | ExprKind::Index { obj, .. } => e = *obj,
                _ => return None,
            }
        }
    }

    pub fn callee_name(&self, call: usize) -> Option<&str> {
        match &self.exprs[call].kind {
            ExprKind::Call { callee, method: None, .. } => self.name_of(*callee),
            _ => None,
        }
    }

    pub fn call_args(&self, call: usize) -> &[usize] {
        match &self.exprs[call].kind {
            ExprKind::Call { args, .. } => args,
            _ => &[],
        }
    }

    pub fn callee_span(&self, src: &Source, call: usize) -> (usize, usize) {
        let ExprKind::Call { callee, method, .. } = &self.exprs[call].kind else { return self.span(Node::Expr(call)) };
        let start = self.exprs[*callee].start;
        let mut stop = self.exprs[*callee].stop;
        if method.is_some() {
            let name_tok = self.exprs[*callee].end + 1;
            if let Some(t) = src.toks.get(name_tok) {
                stop = t.end;
            }
        }
        (start, stop)
    }

    pub fn is_number(&self, e: usize) -> bool {
        matches!(self.exprs[e].kind, ExprKind::Number)
    }

    pub fn number_text(&self, src: &Source, e: usize) -> Option<String> {
        match &self.exprs[e].kind {
            ExprKind::Number => Some(self.text(src, Node::Expr(e)).to_owned()),
            ExprKind::Unary { op: "-", operand } if self.is_number(*operand) => {
                Some(format!("-{}", self.text(src, Node::Expr(*operand))))
            }
            _ => None,
        }
    }

    pub fn ordinal(&self, func: usize) -> Option<usize> {
        match self.exprs.get(func)?.kind {
            ExprKind::Function { ordinal, .. } => Some(ordinal),
            _ => None,
        }
    }

    pub fn named_functions(&self) -> Vec<Named> {
        let mut out = Vec::new();
        let mut add = |target: usize, func: usize, field: Option<&str>| {
            let Some(mut path) = self.path(target) else { return };
            if let Some(f) = field {
                path = format!("{path}.{f}");
            }
            let root = self.root_name(target).and_then(|r| self.decl_of(r));
            out.push(Named { path, root, func });
        };
        for s in &self.stats {
            match &s.kind {
                StatKind::Function { target, method: None, func } => add(*target, *func, None),
                StatKind::Assign { targets, values } => {
                    for (&t, &v) in targets.iter().zip(values) {
                        let v = self.unwrap(v);
                        match &self.exprs[v].kind {
                            ExprKind::Function { .. } => add(t, v, None),
                            ExprKind::Table { fields } => {
                                for field in fields {
                                    if let (FieldKey::Named(name), Some(value)) = (&field.key, field.value)
                                        && matches!(self.exprs[self.unwrap(value)].kind, ExprKind::Function { .. })
                                    {
                                        add(t, self.unwrap(value), Some(name));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn link(&mut self) {
        let mut stack = vec![Node::Block(self.root)];
        let mut kids = Vec::new();
        while let Some(n) = stack.pop() {
            kids.clear();
            self.children(n, |c| kids.push(c));
            for &c in &kids {
                match c {
                    Node::Expr(e) => self.exprs[e].parent = Some(n),
                    Node::Stat(s) => {
                        if let Node::Block(b) = n {
                            self.stats[s].parent = b;
                        }
                    }
                    Node::Block(b) => self.blocks[b].parent = Some(n),
                }
                stack.push(c);
            }
        }
    }
}

const UNARY_PRIORITY: u8 = 12;

fn binary(op: &str) -> Option<(&'static str, u8, u8)> {
    Some(match op {
        "or" => ("or", 1, 1),
        "and" => ("and", 2, 2),
        "<" => ("<", 3, 3),
        ">" => (">", 3, 3),
        "<=" => ("<=", 3, 3),
        ">=" => (">=", 3, 3),
        "~=" => ("~=", 3, 3),
        "==" => ("==", 3, 3),
        "|" => ("|", 4, 4),
        "~" => ("~", 5, 5),
        "&" => ("&", 6, 6),
        "<<" => ("<<", 7, 7),
        ">>" => (">>", 7, 7),
        ".." => ("..", 9, 8),
        "+" => ("+", 10, 10),
        "-" => ("-", 10, 10),
        "*" => ("*", 11, 11),
        "/" => ("/", 11, 11),
        "//" => ("//", 11, 11),
        "%" => ("%", 11, 11),
        "^" => ("^", 14, 13),
        _ => return None,
    })
}

fn unary(op: &str) -> Option<&'static str> {
    Some(match op {
        "not" => "not",
        "-" => "-",
        "#" => "#",
        "~" => "~",
        _ => return None,
    })
}

fn compound(op: &str) -> Option<&'static str> {
    Some(match op {
        "+=" => "+=",
        "-=" => "-=",
        "*=" => "*=",
        "/=" => "/=",
        "%=" => "%=",
        "^=" => "^=",
        "..=" => "..=",
        "//=" => "//=",
        "<<=" => "<<=",
        ">>=" => ">>=",
        "&=" => "&=",
        "|=" => "|=",
        _ => return None,
    })
}

struct Parser<'a> {
    src: &'a Source,
    pos: usize,
    limit: usize,
    ast: Ast,
    scopes: Vec<Vec<usize>>,
    depth: usize,
}

const MAX_DEPTH: usize = 200;

impl<'a> Parser<'a> {
    fn new(src: &'a Source) -> Self {
        let ast = Ast {
            exprs: Vec::new(),
            stats: Vec::new(),
            blocks: Vec::new(),
            decls: Vec::new(),
            root: 0,
            owner: vec![None; src.toks.len()],
            functions: Vec::new(),
            by_name: HashMap::new(),
        };
        Self { src, pos: 0, limit: src.toks.len(), ast, scopes: Vec::new(), depth: 0 }
    }

    fn text(&self, i: usize) -> &'a str {
        if i < self.limit { self.src.tok_text(i) } else { "" }
    }

    fn kind(&self, i: usize) -> Option<Tok> {
        (i < self.limit).then(|| self.src.toks[i].kind)
    }

    fn is_op(&self, op: &str) -> bool {
        self.kind(self.pos) == Some(Tok::Op) && self.text(self.pos) == op
    }

    fn is_kw(&self, kw: &str) -> bool {
        self.kind(self.pos) == Some(Tok::Keyword) && self.text(self.pos) == kw
    }

    fn eat_op(&mut self, op: &str) -> bool {
        let yes = self.is_op(op);
        if yes {
            self.pos += 1;
        }
        yes
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        let yes = self.is_kw(kw);
        if yes {
            self.pos += 1;
        }
        yes
    }

    fn byte_at(&self, i: usize) -> usize {
        if i < self.limit {
            self.src.toks[i].start
        } else if self.limit < self.src.toks.len() {
            self.src.toks[self.limit].start
        } else {
            self.src.text.len()
        }
    }

    fn end_byte(&self, i: usize) -> usize {
        self.src.toks.get(i).map_or(self.src.text.len(), |t| t.end)
    }

    fn span(&self, first: usize, end: usize) -> (usize, usize) {
        if end > first {
            (self.src.toks[first].start, self.src.toks[end - 1].end)
        } else {
            (self.byte_at(first), self.byte_at(first))
        }
    }

    fn own(&mut self, first: usize, end: usize, node: Node) {
        let mut i = first;
        while i < end {
            match self.ast.owner[i] {
                None => {
                    self.ast.owner[i] = Some(node);
                    i += 1;
                }
                Some(Node::Expr(c)) => i = self.ast.exprs[c].end.max(i + 1),
                Some(Node::Stat(c)) => i = self.ast.stats[c].end.max(i + 1),
                Some(Node::Block(_)) => i += 1,
            }
        }
    }

    fn push(&mut self, kind: ExprKind, first: usize) -> usize {
        let end = self.pos.max(first);
        let (start, stop) = self.span(first, end);
        let id = self.ast.exprs.len();
        self.ast.exprs.push(Expr { kind, first, end, start, stop, parent: None });
        self.own(first, end, Node::Expr(id));
        id
    }

    fn push_stat(&mut self, kind: StatKind, first: usize) -> usize {
        let end = self.pos.max(first);
        let (start, stop) = self.span(first, end);
        let id = self.ast.stats.len();
        self.ast.stats.push(Stat { kind, first, end, start, stop, parent: 0 });
        self.own(first, end, Node::Stat(id));
        id
    }

    fn push_block(&mut self, stats: Vec<usize>, start: usize) -> usize {
        let stop = self.byte_at(self.pos).max(start);
        let id = self.ast.blocks.len();
        self.ast.blocks.push(Block { stats, start, stop, parent: None });
        id
    }

    fn open_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn close_scope(&mut self, to: usize) {
        for d in self.scopes.pop().unwrap_or_default() {
            self.ast.decls[d].to = to;
        }
    }

    fn declare(&mut self, tok: usize, kind: DeclKind, from: usize) -> usize {
        let name = self.src.tok_text(tok);
        self.declare_named(name, tok, kind, from)
    }

    fn declare_named(&mut self, name: &str, tok: usize, kind: DeclKind, from: usize) -> usize {
        let name = name.to_owned();
        let id = self.ast.decls.len();
        self.ast.by_name.entry(name.clone()).or_default().push(id);
        self.ast.decls.push(Decl { name, tok, kind, from, to: usize::MAX, init: None, assigned: Vec::new() });
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(id);
        }
        id
    }

    fn resolve(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).copied().find(|&d| self.ast.decls[d].name == name)
    }

    fn at_block_end(&self) -> bool {
        self.pos >= self.limit
            || (self.kind(self.pos) == Some(Tok::Keyword)
                && matches!(self.text(self.pos), "end" | "else" | "elseif" | "until"))
    }

    fn statements(&mut self, out: &mut Vec<usize>) {
        while !self.at_block_end() {
            let before = self.pos;
            if let Some(s) = self.statement() {
                out.push(s);
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
    }

    fn block(&mut self, opener: usize) -> usize {
        self.open_scope();
        let block = self.block_in_scope(opener);
        self.close_scope(self.pos);
        block
    }

    fn block_in_scope(&mut self, opener: usize) -> usize {
        let start = if opener < self.pos { self.end_byte(opener) } else { self.byte_at(self.pos) };
        let mut stats = Vec::new();
        self.statements(&mut stats);
        self.push_block(stats, start)
    }

    fn statement(&mut self) -> Option<usize> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let s = self.statement_here();
        self.depth -= 1;
        s
    }

    fn statement_here(&mut self) -> Option<usize> {
        let first = self.pos;
        let kind = match (self.kind(first)?, self.text(first)) {
            (Tok::Op, ";") => {
                self.pos += 1;
                StatKind::Empty
            }
            (Tok::Op, "::") => {
                self.pos += 1;
                if self.kind(self.pos) == Some(Tok::Name) {
                    self.pos += 1;
                }
                self.eat_op("::");
                StatKind::Label
            }
            (Tok::Keyword, "if") => self.if_stat(),
            (Tok::Keyword, "while") => {
                self.pos += 1;
                let cond = self.expr();
                let opener = self.pos;
                self.eat_kw("do");
                let body = self.block(opener);
                self.eat_kw("end");
                StatKind::While { cond, body }
            }
            (Tok::Keyword, "do") => {
                self.pos += 1;
                let body = self.block(first);
                self.eat_kw("end");
                StatKind::Do(body)
            }
            (Tok::Keyword, "repeat") => {
                self.pos += 1;
                self.open_scope();
                let body = self.block_in_scope(first);
                self.eat_kw("until");
                let cond = self.expr();
                self.close_scope(self.pos);
                StatKind::Repeat { body, cond }
            }
            (Tok::Keyword, "for") => self.for_stat(),
            (Tok::Keyword, "function") => {
                self.pos += 1;
                let (target, method) = self.function_name();
                let func = self.function_body(first, method.is_some());
                if let Some(d) = self.ast.decl_of(target) {
                    let stat = self.ast.stats.len();
                    self.ast.decls[d].assigned.push(stat);
                }
                StatKind::Function { target, method, func }
            }
            (Tok::Keyword, "local") => {
                self.pos += 1;
                if self.eat_kw("function") {
                    return Some(self.local_function(first));
                }
                return Some(self.local(first));
            }
            (Tok::Keyword, "return") => {
                self.pos += 1;
                let values = if self.at_block_end() || self.is_op(";") { Vec::new() } else { self.expr_list() };
                self.eat_op(";");
                StatKind::Return(values)
            }
            (Tok::Keyword, "break") => {
                self.pos += 1;
                StatKind::Break
            }
            (Tok::Keyword, "goto") => {
                self.pos += 1;
                if self.kind(self.pos) == Some(Tok::Name) {
                    self.pos += 1;
                }
                StatKind::Goto
            }
            (Tok::Name, _) | (Tok::Op, "(") => return self.expr_stat(first),
            _ => return None,
        };
        Some(self.push_stat(kind, first))
    }

    fn if_stat(&mut self) -> StatKind {
        let mut conds = Vec::new();
        let mut blocks = Vec::new();
        let mut otherwise = None;
        self.pos += 1;
        loop {
            conds.push(self.expr());
            let opener = self.pos;
            self.eat_kw("then");
            blocks.push(self.block(opener));
            if self.eat_kw("elseif") {
                continue;
            }
            if self.is_kw("else") {
                let opener = self.pos;
                self.pos += 1;
                otherwise = Some(self.block(opener));
            }
            self.eat_kw("end");
            break;
        }
        StatKind::If { conds, blocks, otherwise }
    }

    fn for_stat(&mut self) -> StatKind {
        self.pos += 1;
        let mut names = Vec::new();
        while self.kind(self.pos) == Some(Tok::Name) {
            names.push(self.pos);
            self.pos += 1;
            if !self.eat_op(",") {
                break;
            }
        }
        let numeric = self.is_op("=");
        self.pos += usize::from(numeric || self.is_kw("in"));
        let exprs = self.expr_list();
        let opener = self.pos;
        self.eat_kw("do");
        self.open_scope();
        let vars: Vec<usize> = names.iter().map(|&t| self.declare(t, DeclKind::ForVar { stat: 0 }, opener)).collect();
        let body = self.block(opener);
        self.close_scope(self.pos);
        self.eat_kw("end");
        // The statement is pushed right after its body: that is the loop variables' statement.
        let stat = self.ast.stats.len();
        for &d in &vars {
            self.ast.decls[d].kind = DeclKind::ForVar { stat };
        }
        match (numeric, vars.first()) {
            (true, Some(&var)) => StatKind::NumFor { var, exprs, body },
            _ => StatKind::GenFor { vars, exprs, body },
        }
    }

    fn function_name(&mut self) -> (usize, Option<String>) {
        let first = self.pos;
        if self.kind(self.pos) != Some(Tok::Name) {
            return (self.push(ExprKind::Error, first), None);
        }
        let name = self.text(self.pos).to_owned();
        self.pos += 1;
        let decl = self.resolve(&name);
        let mut target = self.push(ExprKind::Name { name, decl }, first);
        while self.is_op(".") && self.kind(self.pos + 1) == Some(Tok::Name) {
            let name = self.text(self.pos + 1).to_owned();
            self.pos += 2;
            target = self.push(ExprKind::Field { obj: target, name, safe: false }, first);
        }
        let mut method = None;
        if self.is_op(":") && self.kind(self.pos + 1) == Some(Tok::Name) {
            method = Some(self.text(self.pos + 1).to_owned());
            self.pos += 2;
        }
        (target, method)
    }

    fn function_body(&mut self, first: usize, method: bool) -> usize {
        self.open_scope();
        let mut params = Vec::new();
        // The implicit `self` of a method has no token of its own.
        let self_decl = method.then(|| self.declare_named("self", first, DeclKind::Param { func: 0 }, first));
        let mut opener = self.pos;
        if self.is_op("(") {
            let close = self.src.pair[self.pos].filter(|&c| c < self.limit);
            self.pos += 1;
            loop {
                match self.kind(self.pos) {
                    Some(Tok::Name) => {
                        let tok = self.pos;
                        params.push(self.declare(tok, DeclKind::Param { func: 0 }, tok));
                        self.pos += 1;
                    }
                    Some(Tok::Op) if self.text(self.pos) == "..." => self.pos += 1,
                    _ => {}
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            if let Some(c) = close {
                self.pos = c;
            }
            opener = self.pos;
            self.eat_op(")");
        }
        let body = self.block_in_scope(opener);
        self.close_scope(self.pos);
        self.eat_kw("end");
        // The function is pushed right after its body: that is the parameters' function.
        let func = self.ast.exprs.len();
        for &p in params.iter().chain(&self_decl) {
            self.ast.decls[p].kind = DeclKind::Param { func };
        }
        let ordinal = self.ast.functions.len();
        self.ast.functions.push(func);
        self.push(ExprKind::Function { params, body, ordinal, method }, first)
    }

    fn local_function(&mut self, first: usize) -> usize {
        if self.kind(self.pos) != Some(Tok::Name) {
            let func = self.function_body(first, false);
            return self.push_stat(StatKind::Expr(func), first);
        }
        let tok = self.pos;
        self.pos += 1;
        let decl = self.declare(tok, DeclKind::LocalFunction { stat: 0 }, tok);
        let func = self.function_body(first, false);
        let stat = self.ast.stats.len();
        self.ast.decls[decl].kind = DeclKind::LocalFunction { stat };
        self.ast.decls[decl].init = Some(func);
        self.push_stat(StatKind::LocalFunction { decl, func }, first)
    }

    fn local(&mut self, first: usize) -> usize {
        let mut names = Vec::new();
        while self.kind(self.pos) == Some(Tok::Name) {
            names.push(self.pos);
            self.pos += 1;
            // `<const>` / `<close>` attributes.
            if self.is_op("<") && self.kind(self.pos + 1) == Some(Tok::Name) && self.text(self.pos + 2) == ">" {
                self.pos += 3;
            }
            if !self.eat_op(",") {
                break;
            }
        }
        let values = if self.eat_op("=") { self.expr_list() } else { Vec::new() };
        let stat = self.ast.stats.len();
        let from = self.pos;
        let decls: Vec<usize> =
            names.iter().enumerate().map(|(i, &t)| self.declare(t, DeclKind::Local { stat, index: i }, from)).collect();
        for (i, &d) in decls.iter().enumerate() {
            self.ast.decls[d].init = values.get(i).copied();
        }
        self.push_stat(StatKind::Local { names: decls, values }, first)
    }

    fn expr_stat(&mut self, first: usize) -> Option<usize> {
        let e = self.suffixed();
        if self.is_op("=") || self.is_op(",") {
            let mut targets = vec![e];
            while self.eat_op(",") {
                targets.push(self.suffixed());
            }
            self.eat_op("=");
            let values = self.expr_list();
            let stat = self.ast.stats.len();
            for &t in &targets {
                if let Some(d) = self.ast.decl_of(t) {
                    self.ast.decls[d].assigned.push(stat);
                }
            }
            return Some(self.push_stat(StatKind::Assign { targets, values }, first));
        }
        if self.kind(self.pos) == Some(Tok::Op)
            && let Some(op) = compound(self.text(self.pos))
        {
            self.pos += 1;
            let value = self.expr();
            let stat = self.ast.stats.len();
            if let Some(d) = self.ast.decl_of(e) {
                self.ast.decls[d].assigned.push(stat);
            }
            return Some(self.push_stat(StatKind::Compound { target: e, op, value }, first));
        }
        if self.pos == first {
            return None;
        }
        Some(self.push_stat(StatKind::Expr(e), first))
    }

    fn expr_list(&mut self) -> Vec<usize> {
        let mut out = vec![self.expr()];
        while self.eat_op(",") {
            out.push(self.expr());
        }
        out
    }

    fn expr(&mut self) -> usize {
        self.subexpr(0)
    }

    fn subexpr(&mut self, limit: u8) -> usize {
        if self.depth >= MAX_DEPTH {
            return self.push(ExprKind::Error, self.pos);
        }
        self.depth += 1;
        let e = self.subexpr_here(limit);
        self.depth -= 1;
        e
    }

    fn subexpr_here(&mut self, limit: u8) -> usize {
        let first = self.pos;
        let mut chain = 0;
        let mut lhs = match self
            .kind(first)
            .filter(|k| matches!(k, Tok::Op | Tok::Keyword))
            .and_then(|_| unary(self.text(first)))
        {
            Some(op) => {
                self.pos += 1;
                let operand = self.subexpr(UNARY_PRIORITY);
                self.push(ExprKind::Unary { op, operand }, first)
            }
            None => self.simple(),
        };
        while let Some((op, left, right)) = self
            .kind(self.pos)
            .filter(|k| matches!(k, Tok::Op | Tok::Keyword))
            .and_then(|_| binary(self.text(self.pos)))
        {
            if left <= limit || chain >= MAX_DEPTH {
                break;
            }
            chain += 1;
            self.pos += 1;
            let rhs = self.subexpr(right);
            lhs = self.push(ExprKind::Binary { op, lhs, rhs }, first);
        }
        lhs
    }

    fn simple(&mut self) -> usize {
        let first = self.pos;
        let leaf = match (self.kind(first), self.text(first)) {
            (Some(Tok::Number), _) => ExprKind::Number,
            (Some(Tok::Str { .. } | Tok::LongStr), _) => ExprKind::Str,
            (Some(Tok::Hash { .. }), _) => ExprKind::Hash,
            (Some(Tok::Keyword), "nil") => ExprKind::Nil,
            (Some(Tok::Keyword), "true") => ExprKind::True,
            (Some(Tok::Keyword), "false") => ExprKind::False,
            (Some(Tok::Op), "...") => ExprKind::Vararg,
            (Some(Tok::Op), "{") => return self.table(),
            (Some(Tok::Keyword), "function") => {
                self.pos += 1;
                return self.function_body(first, false);
            }
            _ => return self.suffixed(),
        };
        self.pos += 1;
        self.push(leaf, first)
    }

    fn primary(&mut self) -> usize {
        let first = self.pos;
        match self.kind(first) {
            Some(Tok::Name) => {
                let name = self.text(first).to_owned();
                self.pos += 1;
                let decl = self.resolve(&name);
                self.push(ExprKind::Name { name, decl }, first)
            }
            Some(Tok::Op) if self.text(first) == "(" => {
                let close = self.src.pair[first].filter(|&c| c < self.limit);
                self.pos += 1;
                let inner = self.bounded(close, Self::expr);
                self.push(ExprKind::Paren(inner), first)
            }
            _ => self.push(ExprKind::Error, first),
        }
    }

    fn bounded<T>(&mut self, close: Option<usize>, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.limit;
        if let Some(c) = close {
            self.limit = c;
        }
        let out = f(self);
        self.limit = saved;
        match close {
            Some(c) => self.pos = c + 1,
            None => {
                self.eat_op(")");
            }
        }
        out
    }

    fn suffixed(&mut self) -> usize {
        let first = self.pos;
        let mut e = self.primary();
        if matches!(self.ast.exprs[e].kind, ExprKind::Error) {
            return e;
        }
        for _ in 0..MAX_DEPTH {
            let i = self.pos;
            match (self.kind(i), self.text(i)) {
                (Some(Tok::Op), "?") if self.src.is_safe_nav(i) => self.pos += 1,
                (Some(Tok::Op), ".") if self.kind(i + 1) == Some(Tok::Name) => {
                    let name = self.text(i + 1).to_owned();
                    self.pos += 2;
                    let safe = self.src.is_op(i - 1, "?");
                    e = self.push(ExprKind::Field { obj: e, name, safe }, first);
                }
                (Some(Tok::Op), "[") => {
                    let close = self.src.pair[i].filter(|&c| c < self.limit);
                    self.pos += 1;
                    let key = self.bounded_index(close);
                    e = self.push(ExprKind::Index { obj: e, key }, first);
                }
                (Some(Tok::Op), ":") if self.kind(i + 1) == Some(Tok::Name) => {
                    let method = self.text(i + 1).to_owned();
                    self.pos += 2;
                    let args = self.call_args();
                    e = self.push(ExprKind::Call { callee: e, method: Some(method), args }, first);
                }
                (Some(Tok::Op), "(" | "{") | (Some(Tok::Str { .. } | Tok::LongStr), _) => {
                    let args = self.call_args();
                    e = self.push(ExprKind::Call { callee: e, method: None, args }, first);
                }
                _ => return e,
            }
        }
        e
    }

    fn bounded_index(&mut self, close: Option<usize>) -> usize {
        let saved = self.limit;
        if let Some(c) = close {
            self.limit = c;
        }
        let key = self.expr();
        self.limit = saved;
        match close {
            Some(c) => self.pos = c + 1,
            None => {
                self.eat_op("]");
            }
        }
        key
    }

    fn call_args(&mut self) -> Vec<usize> {
        let i = self.pos;
        match (self.kind(i), self.text(i)) {
            (Some(Tok::Op), "(") => {
                let close = self.src.pair[i].filter(|&c| c < self.limit);
                self.pos += 1;
                self.bounded(close, |p| if p.is_op(")") || p.pos >= p.limit { Vec::new() } else { p.expr_list() })
            }
            (Some(Tok::Op), "{") => vec![self.table()],
            (Some(Tok::Str { .. } | Tok::LongStr), _) => {
                self.pos += 1;
                vec![self.push(ExprKind::Str, i)]
            }
            _ => Vec::new(),
        }
    }

    fn table(&mut self) -> usize {
        let first = self.pos;
        let close = self.src.pair[first].filter(|&c| c < self.limit);
        self.pos += 1;
        let saved = self.limit;
        if let Some(c) = close {
            self.limit = c;
        }
        let mut fields = Vec::new();
        while self.pos < self.limit && !self.is_op("}") {
            let start = self.pos;
            let field = self.field();
            if self.pos == start {
                self.pos += 1;
                continue;
            }
            fields.push(Field { key: field.0, value: field.1 });
            let _ = self.eat_op(",") || self.eat_op(";");
        }
        self.limit = saved;
        match close {
            Some(c) => self.pos = c + 1,
            None => {
                self.eat_op("}");
            }
        }
        self.push(ExprKind::Table { fields }, first)
    }

    fn field(&mut self) -> (FieldKey, Option<usize>) {
        let i = self.pos;
        if self.is_op("[") {
            let close = self.src.pair[i].filter(|&c| c < self.limit);
            self.pos += 1;
            let key = self.bounded_index(close);
            if self.eat_op("=") {
                return (FieldKey::Expr(key), Some(self.expr()));
            }
            return (FieldKey::Set, None);
        }
        if self.is_op(".") && self.kind(i + 1) == Some(Tok::Name) {
            self.pos += 2;
            return (FieldKey::Set, None);
        }
        if self.kind(i) == Some(Tok::Name) && self.kind(i + 1) == Some(Tok::Op) && self.text(i + 1) == "=" {
            let name = self.text(i).to_owned();
            self.pos += 2;
            return (FieldKey::Named(name), Some(self.expr()));
        }
        if matches!(self.text(i), "," | ";") && self.kind(i) == Some(Tok::Op) {
            return (FieldKey::Positional, None);
        }
        (FieldKey::Positional, Some(self.expr()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ast(src: &str) -> (Source, Rc<Ast>) {
        let s = Source::new(src);
        let a = parse(&s);
        (s, a)
    }

    #[test]
    fn statements_blocks_and_calls() {
        let (s, a) =
            ast("CreateThread(function()\n  while true do\n    Wait(0)\n    if x then y() else z() end\n  end\nend)");
        let calls = a.calls(Node::Block(a.root), &|_| false);
        // The thread function is a definition: only the outer call.
        assert_eq!(calls.len(), 1);
        let f = a.functions[0];
        let ExprKind::Function { body, .. } = a.exprs[f].kind else { panic!() };
        let calls = a.calls(Node::Block(body), &|_| false);
        let names: Vec<&str> = calls.iter().map(|&c| a.text(&s, Node::Expr(c))).collect();
        assert_eq!(names, ["Wait(0)", "y()", "z()"]);
        let w = a.blocks[body].stats[0];
        assert!(matches!(a.stats[w].kind, StatKind::While { .. }));
        assert_eq!(a.text(&s, Node::Stat(w)).lines().next(), Some("while true do"));
    }

    #[test]
    fn locals_and_scopes() {
        let (s, a) = ast("local a = 1\nlocal function f(b) return a + b end\na = 2\nlocal c = c\nprint(b)");
        let names: Vec<(String, Option<usize>)> = a
            .exprs
            .iter()
            .filter_map(|e| match &e.kind {
                ExprKind::Name { name, decl } => Some((name.clone(), *decl)),
                _ => None,
            })
            .collect();
        let resolved = |n: &str| names.iter().filter(|(x, _)| x == n).map(|(_, d)| d.is_some()).collect::<Vec<_>>();
        assert_eq!(resolved("a"), [true, true]);
        assert_eq!(resolved("b"), [true, false], "the parameter is not visible after the function");
        assert_eq!(resolved("c"), [false], "`local c = c` reads the global");
        let a_decl = a.local_at("a", s.toks.len() - 1).unwrap();
        assert_eq!(a.decls[a_decl].assigned.len(), 1);
        assert!(a.local_at("f", s.toks.len() - 1).is_some());
    }

    #[test]
    fn precedence_and_tables() {
        let (s, a) = ast("local d = #(a - b) < 5 and x ^ 2 ^ 3\nlocal t = { x = 1.0, [k] = 2, 3, .set }");
        let ExprKind::Binary { op, lhs, .. } = &a.exprs[a.stats[0].kind_values()[0]].kind else { panic!() };
        assert_eq!(*op, "and");
        assert_eq!(a.text(&s, Node::Expr(*lhs)), "#(a - b) < 5");
        let ExprKind::Table { fields } = &a.exprs[a.stats[1].kind_values()[0]].kind else { panic!() };
        assert!(matches!(fields[0].key, FieldKey::Named(ref n) if n == "x"));
        assert!(matches!(fields[1].key, FieldKey::Expr(_)));
        assert!(matches!(fields[2].key, FieldKey::Positional));
        assert!(matches!(fields[3].key, FieldKey::Set));
    }

    #[test]
    fn broken_code_still_parses() {
        for text in [
            "while do end",
            "f(a, , b",
            "local x = {1, 2",
            "if x then",
            "end end x = 1",
            "a.",
            "function (",
            "for i = 1",
        ] {
            let (_, a) = ast(text);
            assert!(!a.blocks.is_empty(), "{text}");
        }
        let (s, a) = ast("print(1, function() foo( end, 2)\nnext()");
        let call = |i: usize| matches!(a.exprs[i].kind, ExprKind::Call { .. }) && a.text(&s, Node::Expr(i)) == "next()";
        assert!((0..a.exprs.len()).any(call));
    }

    #[test]
    fn deep_nesting_is_cut() {
        let concat = format!("local s = {}'x'\nlater()", "'a' .. ".repeat(5000));
        let tables = format!("local t = {}1{}\nlater()", "{".repeat(3000), "}".repeat(3000));
        let calls = format!("x = {}1{}\nlater()", "f(".repeat(3000), ")".repeat(3000));
        for text in [concat, tables, calls] {
            let (s, a) = ast(&text);
            let calls = a.calls(Node::Block(a.root), &|_| false);
            assert!(calls.iter().any(|&c| a.text(&s, Node::Expr(c)) == "later()"), "parsing goes on after the cut");
        }
    }

    #[test]
    fn owners_and_parents() {
        let (s, a) = ast("local v = vector3(1.0, -2.0, 3.0)");
        let tok = s.toks.iter().position(|t| t.text(&s.text) == "2.0").unwrap();
        let Some(Node::Expr(n)) = a.owner_of(tok) else { panic!() };
        assert!(a.is_number(n));
        let Some(Node::Expr(neg)) = a.parent(Node::Expr(n)) else { panic!() };
        assert_eq!(a.number_text(&s, neg).as_deref(), Some("-2.0"));
        assert!(
            matches!(a.parent(Node::Expr(neg)), Some(Node::Expr(c)) if matches!(a.exprs[c].kind, ExprKind::Call { .. }))
        );
    }

    impl Stat {
        fn kind_values(&self) -> Vec<usize> {
            match &self.kind {
                StatKind::Local { values, .. } => values.clone(),
                _ => Vec::new(),
            }
        }
    }
}
