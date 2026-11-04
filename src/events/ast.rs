//! A small, error-tolerant Lua parser over the tokens of a [`Source`]: the
//! expressions, statements and functions the state bag and event analyses
//! walk, every name resolved lexically to its declaration (locals, local
//! functions, parameters, loop variables) or left global.
//!
//! Nodes live in arenas indexed by `u32`. The nodes of a function body are
//! allocated while it is parsed, so each function owns a contiguous range of
//! expressions and statements. A bracket bounds what is parsed inside it, so a
//! closure missing its `end` stops at the `)` of the call it is passed to.

use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// No node: a global name, the file as enclosing function.
pub const NONE: u32 = u32::MAX;

const UNARY_PRIORITY: u8 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    /// `== ~= < <= > >=`
    Compare,
    BOr,
    BXor,
    BAnd,
    /// `<< >>`
    Shift,
    Concat,
    /// `+ -`
    Additive,
    /// `* / // %`
    Multiplicative,
    Pow,
}

impl BinOp {
    /// Left and right priorities (Lua 5.4).
    fn priority(self) -> (u8, u8) {
        match self {
            Self::Or => (1, 1),
            Self::And => (2, 2),
            Self::Compare => (3, 3),
            Self::BOr => (4, 4),
            Self::BXor => (5, 5),
            Self::BAnd => (6, 6),
            Self::Shift => (7, 7),
            Self::Concat => (9, 8),
            Self::Additive => (10, 10),
            Self::Multiplicative => (11, 11),
            Self::Pow => (14, 13),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Len,
    Neg,
    BNot,
}

/// The suffix of an index expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `.name` / `?.name` (the name token; `None` while typing `a.`).
    Field(Option<u32>),
    /// `:name` / `?:name`, the callee of a method call.
    Method(Option<u32>),
    /// `[expr]` / `?[expr]`.
    Bracket(Option<u32>),
}

/// A field of a table constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// `name = value`: the name token.
    pub name: Option<u32>,
    /// `[key] = value`: the key expression.
    pub key: Option<u32>,
    pub value: u32,
    pub first: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
    Nil,
    True,
    False,
    Vararg,
    Number,
    /// `'…'`, `"…"`, `[[…]]`.
    Str,
    /// `` `name` ``.
    Hash,
    /// A name and its declaration ([`NONE`]: a global).
    Name(u32),
    Func(u32),
    Table(Vec<Field>),
    Paren(u32),
    Unary(UnOp, u32),
    Binary(BinOp, u32, u32),
    Index(u32, Key),
    /// Callee and arguments; a method call's callee is an [`Key::Method`] index.
    Call(u32, Vec<u32>),
    /// Missing or unreadable code (`()`).
    Error,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    /// First and last tokens (indices in `Source::toks`).
    pub first: u32,
    pub last: u32,
    /// Innermost enclosing function, [`NONE`] at the top level.
    pub func: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StmtKind {
    /// `local a, b = x, y` / `local a, b in src`.
    Local {
        names: Vec<u32>,
        values: Vec<u32>,
        unpack: Option<u32>,
    },
    LocalFunction {
        decl: u32,
        func: u32,
    },
    /// `function a.b:c()`: the name (a name or an index chain).
    Function {
        target: Option<u32>,
        func: u32,
    },
    /// `a, t.b = x, y` / `a, t.b in src`.
    Assign {
        targets: Vec<u32>,
        values: Vec<u32>,
        unpack: Option<u32>,
    },
    /// `x += 1`: the operator token.
    Compound {
        target: u32,
        op: u32,
        value: Option<u32>,
    },
    /// A call (or an incomplete expression).
    Expr(u32),
    Return(Vec<u32>),
    NumFor {
        var: u32,
        exprs: Vec<u32>,
    },
    GenFor {
        vars: Vec<u32>,
        exprs: Vec<u32>,
    },
    /// Blocks and jumps: what they contain is in the arenas.
    Other,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub first: u32,
    pub last: u32,
    pub func: u32,
}

#[derive(Clone, Debug)]
pub struct Func {
    /// The `function` keyword and the `end` (or the last token read).
    pub first: u32,
    pub last: u32,
    /// Named parameters (`self` is implicit and never declared).
    pub params: Vec<u32>,
    pub vararg: bool,
    /// Enclosing function, [`NONE`] at the top level.
    pub parent: u32,
    /// Ids of the expressions and statements of the body: `[start, end)`.
    pub exprs: (u32, u32),
    pub stmts: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    /// A `local` name and its initializer at the same position (no multi-value expansion).
    Local {
        init: Option<u32>,
    },
    LocalFunction(u32),
    /// Function and position.
    Param(u32, u32),
    ForVar,
}

#[derive(Clone, Copy, Debug)]
pub struct Decl {
    /// The name token.
    pub tok: u32,
    pub kind: DeclKind,
}

#[derive(Default)]
pub struct Ast {
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub funcs: Vec<Func>,
    pub decls: Vec<Decl>,
}

impl Ast {
    pub fn parse(src: &Source) -> Self {
        let mut p = Parser {
            src,
            i: 0,
            limit: src.toks.len(),
            ast: Ast::default(),
            visible: HashMap::new(),
            scopes: vec![Vec::new()],
            func: NONE,
        };
        while p.i < p.limit {
            p.block();
            // A stray `end` / `until` closes nothing: skip it.
            if p.i < p.limit {
                p.i += 1;
            }
        }
        p.ast
    }

    pub fn expr(&self, id: u32) -> &Expr {
        &self.exprs[id as usize]
    }

    pub fn kind(&self, id: u32) -> &ExprKind {
        &self.exprs[id as usize].kind
    }

    pub fn func(&self, id: u32) -> &Func {
        &self.funcs[id as usize]
    }

    pub fn decl(&self, id: u32) -> &Decl {
        &self.decls[id as usize]
    }

    /// The expression inside any parentheses.
    pub fn unparen(&self, mut id: u32) -> u32 {
        while let ExprKind::Paren(inner) = self.kind(id) {
            id = *inner;
        }
        id
    }

    /// The text of a name expression.
    pub fn name<'s>(&self, src: &'s Source, id: u32) -> Option<&'s str> {
        matches!(self.kind(id), ExprKind::Name(_)).then(|| src.tok_text(self.expr(id).first as usize))
    }

    /// The declaration of a name expression ([`NONE`] for a global or another expression).
    pub fn decl_of(&self, id: u32) -> u32 {
        match self.kind(id) {
            ExprKind::Name(d) => *d,
            _ => NONE,
        }
    }

    /// A global name: its text.
    pub fn global<'s>(&self, src: &'s Source, id: u32) -> Option<&'s str> {
        (self.decl_of(id) == NONE).then(|| self.name(src, id)).flatten()
    }

    /// `a`, `a.b.c` (dots only); `None` for anything else.
    pub fn path(&self, src: &Source, id: u32) -> Option<String> {
        match self.kind(id) {
            ExprKind::Name(_) => Some(src.tok_text(self.expr(id).first as usize).to_owned()),
            ExprKind::Index(obj, Key::Field(Some(name))) => {
                let mut p = self.path(src, *obj)?;
                p.push('.');
                p.push_str(src.tok_text(*name as usize));
                Some(p)
            }
            _ => None,
        }
    }

    /// Byte span of an expression.
    pub fn span(&self, src: &Source, id: u32) -> (usize, usize) {
        let e = self.expr(id);
        (src.toks[e.first as usize].start, src.toks[e.last as usize].end)
    }

    /// Byte start of an expression.
    pub fn start(&self, src: &Source, id: u32) -> usize {
        src.toks[self.expr(id).first as usize].start
    }

    /// Whether function `inner` is `outer` or nested in it ([`NONE`]: the file holds everything).
    pub fn within(&self, inner: u32, outer: u32) -> bool {
        let mut f = inner;
        loop {
            if f == outer {
                return true;
            }
            if f == NONE {
                return false;
            }
            f = self.func(f).parent;
        }
    }

    /// Literal expressions (`nil`, booleans, numbers, strings, hashes, `...`).
    pub fn is_literal(&self, id: u32) -> bool {
        matches!(
            self.kind(id),
            ExprKind::Nil
                | ExprKind::True
                | ExprKind::False
                | ExprKind::Number
                | ExprKind::Str
                | ExprKind::Hash
                | ExprKind::Vararg
        )
    }

    /// The raw content of a string literal (escapes kept, as written).
    pub fn string(&self, src: &Source, id: u32) -> Option<String> {
        if *self.kind(id) != ExprKind::Str {
            return None;
        }
        Some(raw_string(src, self.expr(id).first as usize))
    }
}

/// The raw content of string token `tok`: the text between its delimiters.
pub fn raw_string(src: &Source, tok: usize) -> String {
    let (s, e) = src.toks[tok].content_range(&src.text);
    src.text[s..e].to_owned()
}

/// The parsed tree of `src`, shared by the features indexing the same text.
pub fn parse(src: &Source) -> Rc<Ast> {
    thread_local! {
        static CACHE: RefCell<Vec<(u64, Rc<Ast>)>> = const { RefCell::new(Vec::new()) };
    }
    let key = fingerprint(&src.text);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, ast)) = cache.iter().find(|(k, _)| *k == key) {
            return ast.clone();
        }
        let ast = Rc::new(Ast::parse(src));
        if cache.len() >= 2 {
            cache.remove(0);
        }
        cache.push((key, ast.clone()));
        ast
    })
}

/// FNV-1a of a text, mixed with its length.
fn fingerprint(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ text.len() as u64;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

const COMPOUND: &[&str] = &["+=", "-=", "*=", "/=", "%=", "^=", "&=", "|=", "<<=", ">>=", "//=", "..="];

struct Parser<'a> {
    src: &'a Source,
    i: usize,
    /// Parsing stops here: the closer of the innermost bracket being read.
    limit: usize,
    ast: Ast,
    /// Declarations visible by name, innermost last.
    visible: HashMap<&'a str, Vec<u32>>,
    /// Names declared by each open scope.
    scopes: Vec<Vec<&'a str>>,
    func: u32,
}

impl<'a> Parser<'a> {
    fn text_at(&self, i: usize) -> &'a str {
        let src: &'a Source = self.src;
        src.toks.get(i).map_or("", |t| &src.text[t.start..t.end])
    }

    fn tok_kind(&self, i: usize) -> Option<Tok> {
        (i < self.limit).then(|| self.src.toks[i].kind)
    }

    fn op_at(&self, i: usize, s: &str) -> bool {
        self.tok_kind(i) == Some(Tok::Op) && self.text_at(i) == s
    }

    fn op(&self, s: &str) -> bool {
        self.op_at(self.i, s)
    }

    fn kw(&self, s: &str) -> bool {
        self.tok_kind(self.i) == Some(Tok::Keyword) && self.text_at(self.i) == s
    }

    fn name_at(&self, i: usize) -> bool {
        self.tok_kind(i) == Some(Tok::Name)
    }

    fn name(&self) -> bool {
        self.name_at(self.i)
    }

    fn eat_op(&mut self, s: &str) -> bool {
        let yes = self.op(s);
        if yes {
            self.i += 1;
        }
        yes
    }

    fn eat_kw(&mut self, s: &str) -> bool {
        let yes = self.kw(s);
        if yes {
            self.i += 1;
        }
        yes
    }

    fn last(&self, first: usize) -> u32 {
        self.i.saturating_sub(1).max(first) as u32
    }

    fn push_expr(&mut self, kind: ExprKind, first: usize) -> u32 {
        let id = self.ast.exprs.len() as u32;
        let last = self.last(first);
        self.ast.exprs.push(Expr { kind, first: first as u32, last, func: self.func });
        id
    }

    fn push_stmt(&mut self, kind: StmtKind, first: usize) {
        let last = self.last(first);
        self.ast.stmts.push(Stmt { kind, first: first as u32, last, func: self.func });
    }

    // ---- scopes

    fn open(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn close(&mut self) {
        for name in self.scopes.pop().unwrap_or_default() {
            if let Some(v) = self.visible.get_mut(name) {
                v.pop();
            }
        }
    }

    fn declare(&mut self, tok: usize, kind: DeclKind) -> u32 {
        let id = self.ast.decls.len() as u32;
        self.ast.decls.push(Decl { tok: tok as u32, kind });
        let name = self.text_at(tok);
        self.visible.entry(name).or_default().push(id);
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name);
        }
        id
    }

    fn lookup(&self, name: &str) -> u32 {
        self.visible.get(name).and_then(|v| v.last()).copied().unwrap_or(NONE)
    }

    /// Runs `f` with the parse bounded by the closer `close` of the bracket just read, then resumes after it.
    fn within<T>(&mut self, close: Option<usize>, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.limit;
        let close = close.filter(|&c| c >= self.i && c < saved);
        if let Some(c) = close {
            self.limit = c;
        }
        let out = f(self);
        self.limit = saved;
        if let Some(c) = close {
            self.i = c + 1;
        }
        out
    }

    // ---- statements

    fn block_end(&self) -> bool {
        self.kw("end") || self.kw("else") || self.kw("elseif") || self.kw("until")
    }

    fn block(&mut self) {
        while self.i < self.limit && !self.block_end() {
            let before = self.i;
            self.statement();
            if self.i == before {
                self.i += 1;
            }
        }
    }

    fn scoped_block(&mut self) {
        self.open();
        self.block();
        self.close();
    }

    fn statement(&mut self) {
        let first = self.i;
        let t = self.src.toks[self.i];
        let kind = match (t.kind, self.text_at(self.i)) {
            (Tok::Op, ";") => {
                self.i += 1;
                return;
            }
            (Tok::Op, "::") => {
                self.i += 1;
                if self.name() {
                    self.i += 1;
                }
                self.eat_op("::");
                StmtKind::Other
            }
            (Tok::Keyword, "break") => {
                self.i += 1;
                StmtKind::Other
            }
            (Tok::Keyword, "goto") => {
                self.i += 1;
                if self.name() {
                    self.i += 1;
                }
                StmtKind::Other
            }
            (Tok::Keyword, "do") => {
                self.i += 1;
                self.scoped_block();
                self.eat_kw("end");
                StmtKind::Other
            }
            (Tok::Keyword, "while") => {
                self.i += 1;
                self.expr();
                self.eat_kw("do");
                self.scoped_block();
                self.eat_kw("end");
                StmtKind::Other
            }
            (Tok::Keyword, "repeat") => {
                self.i += 1;
                // The condition sees the locals of the body.
                self.open();
                self.block();
                if self.eat_kw("until") {
                    self.expr();
                }
                self.close();
                StmtKind::Other
            }
            (Tok::Keyword, "if") => {
                self.i += 1;
                self.expr();
                self.eat_kw("then");
                self.scoped_block();
                while self.eat_kw("elseif") {
                    self.expr();
                    self.eat_kw("then");
                    self.scoped_block();
                }
                if self.eat_kw("else") {
                    self.scoped_block();
                }
                self.eat_kw("end");
                StmtKind::Other
            }
            (Tok::Keyword, "for") => self.for_stmt(),
            (Tok::Keyword, "function") => self.function_stmt(),
            (Tok::Keyword, "local") => self.local_stmt(),
            (Tok::Keyword, "return") => {
                self.i += 1;
                let values = if self.i >= self.limit || self.block_end() || self.op(";") {
                    Vec::new()
                } else {
                    self.expr_list()
                };
                self.eat_op(";");
                StmtKind::Return(values)
            }
            _ => match self.expr_stmt() {
                Some(kind) => kind,
                None => return,
            },
        };
        self.push_stmt(kind, first);
    }

    fn for_stmt(&mut self) -> StmtKind {
        self.i += 1;
        let mut names = Vec::new();
        while self.name() {
            names.push(self.i);
            self.i += 1;
            if !self.eat_op(",") {
                break;
            }
        }
        if names.len() == 1 && self.eat_op("=") {
            let exprs = self.expr_list();
            self.eat_kw("do");
            self.open();
            let var = self.declare(names[0], DeclKind::ForVar);
            self.block();
            self.close();
            self.eat_kw("end");
            return StmtKind::NumFor { var, exprs };
        }
        self.eat_kw("in");
        let exprs = self.expr_list();
        self.eat_kw("do");
        self.open();
        let vars = names.iter().map(|&n| self.declare(n, DeclKind::ForVar)).collect();
        self.block();
        self.close();
        self.eat_kw("end");
        StmtKind::GenFor { vars, exprs }
    }

    fn function_stmt(&mut self) -> StmtKind {
        let first = self.i;
        self.i += 1;
        let mut target = None;
        if self.name() {
            let start = self.i;
            let mut e = self.name_expr();
            loop {
                if self.op(".") && self.name_at(self.i + 1) {
                    self.i += 2;
                    e = self.push_expr(ExprKind::Index(e, Key::Field(Some(self.i as u32 - 1))), start);
                } else if self.op(":") && self.name_at(self.i + 1) {
                    self.i += 2;
                    e = self.push_expr(ExprKind::Index(e, Key::Method(Some(self.i as u32 - 1))), start);
                    break;
                } else {
                    break;
                }
            }
            target = Some(e);
        }
        let func = self.func_body(first);
        StmtKind::Function { target, func }
    }

    fn local_stmt(&mut self) -> StmtKind {
        self.i += 1;
        if self.kw("function") {
            let first = self.i;
            self.i += 1;
            if !self.name() {
                self.func_body(first);
                return StmtKind::Other;
            }
            let tok = self.i;
            self.i += 1;
            let decl = self.declare(tok, DeclKind::LocalFunction(NONE));
            let func = self.func_body(first);
            self.ast.decls[decl as usize].kind = DeclKind::LocalFunction(func);
            return StmtKind::LocalFunction { decl, func };
        }
        let mut names = Vec::new();
        while self.name() {
            names.push(self.i);
            self.i += 1;
            // `<const>` / `<close>`
            if self.op("<") && self.name_at(self.i + 1) && self.op_at(self.i + 2, ">") {
                self.i += 3;
            }
            if !self.eat_op(",") {
                break;
            }
        }
        let (values, unpack) = if self.eat_op("=") {
            (self.expr_list(), None)
        } else if self.eat_kw("in") {
            (Vec::new(), self.expr())
        } else {
            (Vec::new(), None)
        };
        // Declared after the values: `local x = x` reads the outer `x`.
        let mut decls = Vec::with_capacity(names.len());
        for (k, &tok) in names.iter().enumerate() {
            let init = if unpack.is_none() { values.get(k).copied() } else { None };
            decls.push(self.declare(tok, DeclKind::Local { init }));
        }
        StmtKind::Local { names: decls, values, unpack }
    }

    fn expr_stmt(&mut self) -> Option<StmtKind> {
        let e = self.suffixed()?;
        if self.op("=") || self.op(",") {
            let mut targets = vec![e];
            while self.eat_op(",") {
                match self.suffixed() {
                    Some(t) => targets.push(t),
                    None => break,
                }
            }
            if self.eat_op("=") {
                let values = self.expr_list();
                return Some(StmtKind::Assign { targets, values, unpack: None });
            }
            let unpack = if self.eat_kw("in") { self.expr() } else { None };
            return Some(StmtKind::Assign { targets, values: Vec::new(), unpack });
        }
        if self.eat_kw("in") {
            let unpack = self.expr();
            return Some(StmtKind::Assign { targets: vec![e], values: Vec::new(), unpack });
        }
        if self.tok_kind(self.i) == Some(Tok::Op) && COMPOUND.contains(&self.text_at(self.i)) {
            let op = self.i as u32;
            self.i += 1;
            let value = self.expr();
            return Some(StmtKind::Compound { target: e, op, value });
        }
        Some(StmtKind::Expr(e))
    }

    fn func_body(&mut self, first: usize) -> u32 {
        let id = self.ast.funcs.len() as u32;
        self.ast.funcs.push(Func {
            first: first as u32,
            last: first as u32,
            params: Vec::new(),
            vararg: false,
            parent: self.func,
            exprs: (0, 0),
            stmts: (0, 0),
        });
        let outer = self.func;
        self.func = id;
        let (e0, s0) = (self.ast.exprs.len() as u32, self.ast.stmts.len() as u32);
        self.open();
        let mut params = Vec::new();
        let mut vararg = false;
        if self.op("(") {
            let close = self.src.pair[self.i];
            self.i += 1;
            loop {
                if self.name() {
                    let n = params.len() as u32;
                    params.push(self.declare(self.i, DeclKind::Param(id, n)));
                    self.i += 1;
                } else if self.eat_op("...") {
                    vararg = true;
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            match close {
                Some(c) if c >= self.i && c < self.limit => self.i = c + 1,
                _ => {
                    self.eat_op(")");
                }
            }
        }
        self.block();
        let last = if self.kw("end") {
            self.i += 1;
            self.i - 1
        } else {
            self.i.saturating_sub(1).max(first)
        };
        self.close();
        self.func = outer;
        let (e1, s1) = (self.ast.exprs.len() as u32, self.ast.stmts.len() as u32);
        let f = &mut self.ast.funcs[id as usize];
        f.last = last as u32;
        f.params = params;
        f.vararg = vararg;
        f.exprs = (e0, e1);
        f.stmts = (s0, s1);
        id
    }

    // ---- expressions

    fn expr(&mut self) -> Option<u32> {
        self.sub_expr(0)
    }

    fn expr_list(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        if let Some(e) = self.expr() {
            out.push(e);
            while self.eat_op(",") {
                match self.expr() {
                    Some(e) => out.push(e),
                    None => break,
                }
            }
        }
        out
    }

    fn unary_op(&self) -> Option<UnOp> {
        match self.tok_kind(self.i)? {
            Tok::Keyword if self.text_at(self.i) == "not" => Some(UnOp::Not),
            Tok::Op => match self.text_at(self.i) {
                "#" => Some(UnOp::Len),
                "-" => Some(UnOp::Neg),
                "~" => Some(UnOp::BNot),
                _ => None,
            },
            _ => None,
        }
    }

    fn binary_op(&self) -> Option<BinOp> {
        let text = self.text_at(self.i);
        match self.tok_kind(self.i)? {
            Tok::Keyword => match text {
                "or" => Some(BinOp::Or),
                "and" => Some(BinOp::And),
                _ => None,
            },
            Tok::Op => match text {
                "==" | "~=" | "!=" | "<" | "<=" | ">" | ">=" => Some(BinOp::Compare),
                "|" => Some(BinOp::BOr),
                "~" => Some(BinOp::BXor),
                "&" => Some(BinOp::BAnd),
                "<<" | ">>" => Some(BinOp::Shift),
                ".." => Some(BinOp::Concat),
                "+" | "-" => Some(BinOp::Additive),
                "*" | "/" | "//" | "%" => Some(BinOp::Multiplicative),
                "^" => Some(BinOp::Pow),
                _ => None,
            },
            _ => None,
        }
    }

    fn sub_expr(&mut self, limit: u8) -> Option<u32> {
        let first = self.i;
        let mut left = if let Some(op) = self.unary_op() {
            self.i += 1;
            let operand = self.sub_expr(UNARY_PRIORITY)?;
            self.push_expr(ExprKind::Unary(op, operand), first)
        } else {
            self.simple()?
        };
        while let Some(op) = self.binary_op() {
            let (left_priority, right_priority) = op.priority();
            if left_priority <= limit {
                break;
            }
            self.i += 1;
            let Some(right) = self.sub_expr(right_priority) else { break };
            left = self.push_expr(ExprKind::Binary(op, left, right), first);
        }
        Some(left)
    }

    fn simple(&mut self) -> Option<u32> {
        let first = self.i;
        let kind = match (self.tok_kind(self.i)?, self.text_at(self.i)) {
            (Tok::Number, _) => ExprKind::Number,
            (Tok::Str { .. } | Tok::LongStr, _) => ExprKind::Str,
            (Tok::Hash { .. }, _) => ExprKind::Hash,
            (Tok::Keyword, "nil") => ExprKind::Nil,
            (Tok::Keyword, "true") => ExprKind::True,
            (Tok::Keyword, "false") => ExprKind::False,
            (Tok::Op, "...") => ExprKind::Vararg,
            (Tok::Keyword, "function") => {
                self.i += 1;
                let f = self.func_body(first);
                return Some(self.push_expr(ExprKind::Func(f), first));
            }
            (Tok::Op, "{") => return Some(self.table()),
            _ => return self.suffixed(),
        };
        self.i += 1;
        Some(self.push_expr(kind, first))
    }

    fn name_expr(&mut self) -> u32 {
        let decl = self.lookup(self.text_at(self.i));
        let first = self.i;
        self.i += 1;
        self.push_expr(ExprKind::Name(decl), first)
    }

    fn args_start(&self) -> bool {
        match self.tok_kind(self.i) {
            Some(Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. }) => true,
            Some(Tok::Op) => matches!(self.text_at(self.i), "(" | "{"),
            _ => false,
        }
    }

    fn suffixed(&mut self) -> Option<u32> {
        let first = self.i;
        let mut e = if self.name() {
            self.name_expr()
        } else if self.op("(") {
            let close = self.src.pair[self.i];
            self.i += 1;
            let inner = self.within(close, |p| p.expr());
            self.push_expr(inner.map_or(ExprKind::Error, ExprKind::Paren), first)
        } else {
            return None;
        };
        while self.i < self.limit {
            let t = self.src.toks[self.i];
            match (t.kind, self.text_at(self.i)) {
                (Tok::Op, "." | "?.") => {
                    if self.name_at(self.i + 1) {
                        self.i += 2;
                        e = self.push_expr(ExprKind::Index(e, Key::Field(Some(self.i as u32 - 1))), first);
                    } else {
                        self.i += 1;
                        e = self.push_expr(ExprKind::Index(e, Key::Field(None)), first);
                        break;
                    }
                }
                (Tok::Op, ":" | "?:") => {
                    if !self.name_at(self.i + 1) {
                        self.i += 1;
                        e = self.push_expr(ExprKind::Index(e, Key::Method(None)), first);
                        break;
                    }
                    self.i += 2;
                    let callee = self.push_expr(ExprKind::Index(e, Key::Method(Some(self.i as u32 - 1))), first);
                    if !self.args_start() {
                        e = callee;
                        break;
                    }
                    let args = self.args();
                    e = self.push_expr(ExprKind::Call(callee, args), first);
                }
                (Tok::Op, "[" | "?[") => {
                    let close = self.src.pair[self.i];
                    self.i += 1;
                    let key = self.within(close, |p| p.expr());
                    e = self.push_expr(ExprKind::Index(e, Key::Bracket(key)), first);
                }
                // `f?(…)`
                (Tok::Unknown, "?") if self.op_at(self.i + 1, "(") => self.i += 1,
                _ if self.args_start() => {
                    let args = self.args();
                    e = self.push_expr(ExprKind::Call(e, args), first);
                }
                _ => break,
            }
        }
        Some(e)
    }

    fn args(&mut self) -> Vec<u32> {
        if self.op("(") {
            let close = self.src.pair[self.i];
            self.i += 1;
            return self.within(close, |p| p.expr_list());
        }
        if self.op("{") {
            return vec![self.table()];
        }
        let first = self.i;
        let kind = if matches!(self.src.toks[self.i].kind, Tok::Hash { .. }) { ExprKind::Hash } else { ExprKind::Str };
        self.i += 1;
        vec![self.push_expr(kind, first)]
    }

    fn table(&mut self) -> u32 {
        let first = self.i;
        let close = self.src.pair[self.i];
        self.i += 1;
        let fields = self.within(close, |p| p.fields(close.is_some()));
        self.push_expr(ExprKind::Table(fields), first)
    }

    fn fields(&mut self, closed: bool) -> Vec<Field> {
        let mut out = Vec::new();
        while self.i < self.limit {
            if self.eat_op(",") || self.eat_op(";") {
                continue;
            }
            // An unclosed constructor ends where statements resume.
            if !closed
                && self.tok_kind(self.i) == Some(Tok::Keyword)
                && !matches!(self.text_at(self.i), "function" | "nil" | "true" | "false" | "not")
            {
                break;
            }
            let first = self.i as u32;
            let before = self.i;
            if self.op("[") {
                let close = self.src.pair[self.i];
                self.i += 1;
                let key = self.within(close, |p| p.expr());
                self.eat_op("=");
                if let Some(value) = self.expr() {
                    out.push(Field { name: None, key, value, first });
                }
            } else if self.name() && self.op_at(self.i + 1, "=") {
                let name = self.i as u32;
                self.i += 2;
                if let Some(value) = self.expr() {
                    out.push(Field { name: Some(name), key: None, value, first });
                }
            } else if let Some(value) = self.expr() {
                out.push(Field { name: None, key: None, value, first });
            }
            if self.i == before {
                self.i += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Source, Ast) {
        let src = Source::new(text);
        let ast = Ast::parse(&src);
        (src, ast)
    }

    /// The declaration token text and kind of the name expression at the n-th occurrence of `name`.
    fn resolved(src: &Source, ast: &Ast, name: &str, nth: usize) -> Option<DeclKind> {
        let e = ast
            .exprs
            .iter()
            .filter(|e| matches!(e.kind, ExprKind::Name(_)) && src.tok_text(e.first as usize) == name)
            .nth(nth)?;
        match e.kind {
            ExprKind::Name(d) if d != NONE => Some(ast.decl(d).kind),
            _ => None,
        }
    }

    #[test]
    fn lexical_resolution() {
        let (src, ast) = parsed(
            "local x = x\nlocal function f(a, ...) return a, f end\nfor i = 1, 3 do print(i) end\n\
             do local y = 1 end print(y)\nlocal a, a = 1, 2\nprint(a)\nrepeat local z = 1 until z",
        );
        // `local x = x` reads the global
        assert_eq!(resolved(&src, &ast, "x", 0), None);
        assert!(matches!(resolved(&src, &ast, "a", 0), Some(DeclKind::Param(_, 0))));
        assert!(matches!(resolved(&src, &ast, "f", 0), Some(DeclKind::LocalFunction(_))));
        assert_eq!(resolved(&src, &ast, "i", 0), Some(DeclKind::ForVar));
        assert_eq!(resolved(&src, &ast, "y", 0), None, "out of its block");
        // the last `a` of `local a, a = 1, 2`
        let Some(DeclKind::Local { init: Some(init) }) = resolved(&src, &ast, "a", 1) else { panic!() };
        assert_eq!(src.tok_text(ast.expr(init).first as usize), "2");
        assert!(matches!(resolved(&src, &ast, "z", 0), Some(DeclKind::Local { .. })), "until sees the body");
    }

    #[test]
    fn cfx_syntax_and_calls() {
        let (src, ast) = parsed(
            "LocalPlayer?.state?.job = 1\nPlayer(src).state:set('k', v, true)\ncount += 1\n\
             local a, b in src\na, t.b in src\nf'str' g{1} h`hash`\nlocal c <const> = 5",
        );
        let calls: Vec<String> = ast
            .exprs
            .iter()
            .filter_map(|e| match &e.kind {
                ExprKind::Call(callee, args) => {
                    Some(format!("{}/{}", src.tok_text(ast.expr(*callee).last as usize), args.len()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(calls, ["Player/1", "set/3", "f/1", "g/1", "h/1"]);
        let kinds: Vec<&StmtKind> = ast.stmts.iter().map(|s| &s.kind).collect();
        assert!(matches!(kinds[0], StmtKind::Assign { targets, .. } if targets.len() == 1));
        assert!(matches!(kinds[2], StmtKind::Compound { .. }));
        assert!(matches!(kinds[3], StmtKind::Local { unpack: Some(_), .. }));
        assert!(matches!(kinds[4], StmtKind::Assign { targets, unpack: Some(_), .. } if targets.len() == 2));
        assert!(matches!(kinds.last(), Some(StmtKind::Local { values, .. }) if values.len() == 1));
    }

    #[test]
    fn functions_own_their_ranges_and_brackets_bound_broken_code() {
        let (src, ast) = parsed("AddEventHandler('x', function(a)\n  local b = a\n)\nprint(1)");
        let f = ast.func(0);
        assert_eq!(f.params.len(), 1);
        assert_eq!(src.tok_text(f.last as usize), "a", "the closure stops at the call's `)`");
        let print = ast
            .exprs
            .iter()
            .find(|e| matches!(e.kind, ExprKind::Call(..)) && src.tok_text(e.first as usize) == "print");
        assert_eq!(print.map(|e| e.func), Some(NONE), "the next statement is at the top level");
        let (s0, s1) = f.stmts;
        assert_eq!(s1 - s0, 1);
    }

    #[test]
    fn precedence_and_paths() {
        let (src, ast) = parsed("x = a or b and c .. d .. e + f * g ^ h\ny = A.b.c");
        let StmtKind::Assign { values, .. } = &ast.stmts[0].kind else { panic!() };
        assert!(matches!(ast.kind(values[0]), ExprKind::Binary(BinOp::Or, ..)));
        let StmtKind::Assign { values, .. } = &ast.stmts[1].kind else { panic!() };
        assert_eq!(ast.path(&src, values[0]).as_deref(), Some("A.b.c"));
    }
}
