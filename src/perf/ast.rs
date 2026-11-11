use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{Element, N, Node as Syntax, Token};
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
        let mut lower = Lower::new(src);
        lower.open_scope();
        let mut stats = Vec::new();
        lower.statements(src.root(), &mut stats);
        lower.close_scope(src.toks.len());
        let root = lower.ast.blocks.len();
        lower.ast.blocks.push(Block { stats, start: 0, stop: src.text.len(), parent: None });
        lower.ast.root = root;
        let mut ast = lower.ast;
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

/// Reads the expressions, statements and blocks off the syntax tree, in the order the source writes
/// them: what a node holds is numbered before the node itself.
struct Lower<'a> {
    src: &'a Source,
    ast: Ast,
    scopes: Vec<Vec<usize>>,
    spine: Vec<Syntax<'a>>,
}

impl<'a> Lower<'a> {
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
        Self { src, ast, scopes: Vec::new(), spine: Vec::new() }
    }

    fn tok(&self, token: Token) -> usize {
        self.src.tok_of(token.index())
    }

    fn at(&self, node: Syntax, kind: T) -> Option<usize> {
        node.token(kind).map(|token| self.tok(token))
    }

    /// The tokens of `node`, without the `;` that ends a statement.
    fn range(&self, node: Syntax) -> (usize, usize) {
        match (node.first_token(), node.last_token()) {
            (Some(first), Some(last)) => {
                let closed = node.kind().is_statement() && !node.is(N::ReturnStat) && last.is(T::Semi);
                let (first, end) = (self.tok(first), self.tok(last) + 1);
                (first, if closed && end > first + 1 { end - 1 } else { end })
            }
            _ => {
                let at = self.src.tok_of(node.token_span().0);
                (at, at)
            }
        }
    }

    fn byte_at(&self, i: usize) -> usize {
        self.src.toks.get(i).map_or(self.src.text.len(), |t| t.start)
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

    fn push(&mut self, kind: ExprKind, (first, end): (usize, usize)) -> usize {
        let (start, stop) = self.span(first, end);
        let id = self.ast.exprs.len();
        self.ast.exprs.push(Expr { kind, first, end, start, stop, parent: None });
        self.own(first, end, Node::Expr(id));
        id
    }

    fn push_stat(&mut self, kind: StatKind, (first, end): (usize, usize)) -> usize {
        let (start, stop) = self.span(first, end);
        let id = self.ast.stats.len();
        self.ast.stats.push(Stat { kind, first, end, start, stop, parent: 0 });
        self.own(first, end, Node::Stat(id));
        id
    }

    fn error(&mut self, at: usize) -> usize {
        self.push(ExprKind::Error, (at, at))
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

    fn assigned(&mut self, target: usize) {
        if let Some(d) = self.ast.decl_of(target) {
            let stat = self.ast.stats.len();
            self.ast.decls[d].assigned.push(stat);
        }
    }

    fn statements(&mut self, parent: Syntax<'a>, out: &mut Vec<usize>) {
        for child in parent.children() {
            if child.kind().is_statement() {
                self.stat(child, out);
            }
        }
    }

    /// The token where the block `node` stops: the keyword that closes it.
    fn stop(&self, node: Syntax) -> usize {
        self.src.tok_of(node.token_span().1)
    }

    fn block_in_scope(&mut self, node: Option<Syntax<'a>>, at: usize) -> usize {
        let mut stats = Vec::new();
        let (start, stop) = match node {
            Some(node) => {
                self.statements(node, &mut stats);
                node.range()
            }
            None => (self.byte_at(at), self.byte_at(at)),
        };
        self.ast.blocks.push(Block { stats, start, stop, parent: None });
        self.ast.blocks.len() - 1
    }

    fn block(&mut self, node: Option<Syntax<'a>>, at: usize) -> usize {
        self.open_scope();
        let block = self.block_in_scope(node, at);
        self.close_scope(node.map_or(at, |node| self.stop(node)));
        block
    }

    fn stat(&mut self, node: Syntax<'a>, out: &mut Vec<usize>) {
        let (first, end) = self.range(node);
        let body = node.child(N::Block);
        let kind = match node.kind() {
            N::EmptyStat => {
                for semi in node.tokens().filter(|token| token.is(T::Semi)) {
                    let at = self.tok(semi);
                    out.push(self.push_stat(StatKind::Empty, (at, at + 1)));
                }
                return;
            }
            N::LabelStat => StatKind::Label,
            N::GotoStat => StatKind::Goto,
            N::BreakStat => StatKind::Break,
            N::IfStat => self.if_stat(node),
            N::WhileStat => {
                let cond = self.expr_or_error(node.expr(), first + 1);
                StatKind::While { cond, body: self.block(body, end) }
            }
            N::DoStat => StatKind::Do(self.block(body, end)),
            N::RepeatStat => {
                self.open_scope();
                let body = self.block_in_scope(body, end);
                let cond = self.expr_or_error(node.expr(), end);
                self.close_scope(end);
                StatKind::Repeat { body, cond }
            }
            N::ForAStat | N::ForBStat => self.for_stat(node, end),
            N::FuncDef | N::ClassMethodDef => {
                let (target, method) = self.function_name(node, first + 1);
                let func = self.function(node, (first, end), method.is_some());
                self.assigned(target);
                StatKind::Function { target, method, func }
            }
            N::LocalFuncDef => match self.at(node, T::Id) {
                Some(tok) => {
                    let decl = self.declare(tok, DeclKind::LocalFunction { stat: 0 }, tok);
                    let func = self.function(node, (first, end), false);
                    self.ast.decls[decl].kind = DeclKind::LocalFunction { stat: self.ast.stats.len() };
                    self.ast.decls[decl].init = Some(func);
                    StatKind::LocalFunction { decl, func }
                }
                None => StatKind::Expr(self.function(node, (first, end), false)),
            },
            N::LocalDef => return self.local(node, (first, end), out),
            N::AssignStat => return self.assign(node, (first, end), out),
            N::CompoundAssignStat => {
                let mut operands = node.exprs();
                let target = self.expr_or_error(operands.next(), first);
                let after = self.ast.exprs[target].end;
                let Some(op) = compound(self.src.tok_text(after)) else { return };
                let value = self.expr_or_error(operands.next(), after + 1);
                self.assigned(target);
                StatKind::Compound { target, op, value }
            }
            N::ExprStat => {
                let Some(expr) = node.expr() else { return };
                let value = self.expr(expr);
                let range = (self.ast.exprs[value].first, self.ast.exprs[value].end);
                out.push(self.push_stat(StatKind::Expr(value), range));
                return self.semi(node, out);
            }
            N::ReturnStat => StatKind::Return(self.expr_list(node.child(N::ExprList), None)),
            _ => return,
        };
        out.push(self.push_stat(kind, (first, end)));
        self.semi(node, out);
    }

    /// The `;` that ends a statement is an empty statement of its own, except after `return`.
    fn semi(&mut self, node: Syntax<'a>, out: &mut Vec<usize>) {
        if node.is(N::ReturnStat) {
            return;
        }
        if let Some(semi) = node.tokens().last().filter(|token| token.is(T::Semi)) {
            let at = self.tok(semi);
            out.push(self.push_stat(StatKind::Empty, (at, at + 1)));
        }
    }

    fn if_stat(&mut self, node: Syntax<'a>) -> StatKind {
        let (mut conds, mut blocks, mut otherwise, mut last) = (Vec::new(), Vec::new(), None, false);
        for element in node.elements() {
            match element {
                Element::Token(token) if token.is(T::Else) => last = true,
                Element::Node(child) if child.kind().is_expr() => conds.push(self.expr(child)),
                Element::Node(child) if child.is(N::Block) => {
                    let at = self.range(child).0;
                    if last {
                        otherwise = Some(self.block(Some(child), at));
                        continue;
                    }
                    if conds.len() == blocks.len() {
                        conds.push(self.error(at));
                    }
                    blocks.push(self.block(Some(child), at));
                }
                _ => {}
            }
        }
        StatKind::If { conds, blocks, otherwise }
    }

    fn for_stat(&mut self, node: Syntax<'a>, end: usize) -> StatKind {
        let names: Vec<usize> = node.children_of(N::ParamNameDef).map(|var| self.range(var).0).collect();
        let numeric = node.is(N::ForAStat) && node.has(T::Assign);
        let body = node.child(N::Block);
        let opener = self.at(node, T::Do).or(body.map(|body| self.range(body).0)).unwrap_or(end);
        let exprs = match node.child(N::ExprList) {
            Some(list) => self.expr_list(Some(list), Some(opener)),
            None => self.expr_list(Some(node), Some(opener)),
        };
        self.open_scope();
        let vars: Vec<usize> = names.iter().map(|&t| self.declare(t, DeclKind::ForVar { stat: 0 }, opener)).collect();
        let stop = body.map_or(end, |body| self.stop(body));
        let body = self.block(body, end);
        self.close_scope(stop);
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

    fn function_name(&mut self, node: Syntax<'a>, at: usize) -> (usize, Option<String>) {
        if let Some(path) = node.child(N::ClassMethodName) {
            let object = self.expr_or_error(path.expr(), at);
            let member = path.token(T::Id).map(|name| name.text().to_owned());
            if path.has(T::Colon) {
                return (object, member);
            }
            let Some(name) = member else { return (object, None) };
            let range = (self.ast.exprs[object].first, self.range(path).1);
            return (self.push(ExprKind::Field { obj: object, name, safe: false }, range), None);
        }
        match node.token(T::Id) {
            Some(name) => {
                let tok = self.tok(name);
                let decl = self.resolve(name.text());
                (self.push(ExprKind::Name { name: name.text().to_owned(), decl }, (tok, tok + 1)), None)
            }
            None => (self.error(at), None),
        }
    }

    fn function(&mut self, owner: Syntax<'a>, (first, end): (usize, usize), method: bool) -> usize {
        self.open_scope();
        // The implicit `self` of a method has no token of its own.
        let self_decl = method.then(|| self.declare_named("self", first, DeclKind::Param { func: 0 }, first));
        let body = owner.child(N::FuncBody);
        let mut params = Vec::new();
        for param in body.into_iter().flat_map(|body| body.children_of(N::ParamNameDef)) {
            let tok = self.range(param).0;
            params.push(self.declare(tok, DeclKind::Param { func: 0 }, tok));
        }
        let block = body.and_then(|body| body.child(N::Block));
        let stop = block.map_or(end, |block| self.stop(block));
        let body = self.block_in_scope(block, end);
        self.close_scope(stop);
        // The function is pushed right after its body: that is the parameters' function.
        let func = self.ast.exprs.len();
        for &p in params.iter().chain(&self_decl) {
            self.ast.decls[p].kind = DeclKind::Param { func };
        }
        let ordinal = self.ast.functions.len();
        self.ast.functions.push(func);
        self.push(ExprKind::Function { params, body, ordinal, method }, (first, end))
    }

    fn local(&mut self, node: Syntax<'a>, (first, end): (usize, usize), out: &mut Vec<usize>) {
        let names: Vec<usize> = node
            .child(N::NameList)
            .into_iter()
            .flat_map(|list| list.children_of(N::NameDef))
            .map(|name| self.range(name).0)
            .collect();
        // `local a, b in src`: the names, then the source as a statement of its own.
        let unpack = self.at(node, T::In);
        let values = match node.child(N::ExprList) {
            Some(list) if unpack.is_none() => self.expr_list(Some(list), Some(end)),
            _ => Vec::new(),
        };
        let stat = self.ast.stats.len();
        let from = unpack.unwrap_or(end);
        let decls: Vec<usize> = names
            .iter()
            .enumerate()
            .map(|(index, &tok)| self.declare(tok, DeclKind::Local { stat, index }, from))
            .collect();
        for (i, &d) in decls.iter().enumerate() {
            self.ast.decls[d].init = values.get(i).copied();
        }
        out.push(self.push_stat(StatKind::Local { names: decls, values }, (first, from)));
        self.unpacked(node, unpack, out);
        self.semi(node, out);
    }

    fn unpacked(&mut self, node: Syntax<'a>, unpack: Option<usize>, out: &mut Vec<usize>) {
        if let (Some(_), Some(source)) = (unpack, node.expr()) {
            let source = self.expr(source);
            let range = (self.ast.exprs[source].first, self.ast.exprs[source].end);
            out.push(self.push_stat(StatKind::Expr(source), range));
        }
    }

    fn assign(&mut self, node: Syntax<'a>, (first, end): (usize, usize), out: &mut Vec<usize>) {
        let targets = self.expr_list(node.child(N::VarList), Some(first));
        let unpack = self.at(node, T::In);
        let kind = match unpack {
            Some(_) if targets.len() == 1 => StatKind::Expr(targets[0]),
            Some(at) => {
                let values = vec![self.error(at)];
                targets.iter().for_each(|&target| self.assigned(target));
                StatKind::Assign { targets, values }
            }
            None => {
                let values = self.expr_list(node.child(N::ExprList), Some(end));
                targets.iter().for_each(|&target| self.assigned(target));
                StatKind::Assign { targets, values }
            }
        };
        out.push(self.push_stat(kind, (first, unpack.unwrap_or(end))));
        self.unpacked(node, unpack, out);
        self.semi(node, out);
    }

    fn expr_or_error(&mut self, node: Option<Syntax<'a>>, at: usize) -> usize {
        match node {
            Some(node) => self.expr(node),
            None => self.error(at),
        }
    }

    /// The expressions of `list`; a list that must hold one holds an error where it is missing.
    fn expr_list(&mut self, list: Option<Syntax<'a>>, required: Option<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        // The comma an expression is still expected after.
        let mut open = None;
        let mut commas = false;
        for element in list.into_iter().flat_map(Syntax::elements) {
            match element {
                Element::Node(expr) if expr.kind().is_expr() => {
                    out.push(self.expr(expr));
                    open = None;
                }
                Element::Token(comma) if comma.is(T::Comma) => {
                    let at = self.tok(comma);
                    if open.is_some() || !commas && out.is_empty() {
                        out.push(self.error(at));
                    }
                    (open, commas) = (Some(at), true);
                }
                _ => {}
            }
        }
        if let Some(comma) = open {
            out.push(self.error(comma + 1));
        }
        if let (true, Some(at)) = (out.is_empty(), required) {
            out.push(self.error(at));
        }
        out
    }

    fn args(&mut self, call: Syntax<'a>) -> Vec<usize> {
        match call.child(N::ListArgs) {
            Some(list) => self.expr_list(Some(list), None),
            None => call.child(N::SingleArg).and_then(Syntax::expr).map(|arg| self.expr(arg)).into_iter().collect(),
        }
    }

    fn table(&mut self, node: Syntax<'a>) -> usize {
        let mut fields = Vec::new();
        for field in node.children_of(N::TableField) {
            let at = self.range(field).0;
            let mut exprs = field.exprs();
            let assign = self.at(field, T::Assign);
            if field.has(T::LBrack) {
                let key = self.expr_or_error(exprs.next(), at + 1);
                match assign {
                    Some(assign) => {
                        let value = self.expr_or_error(exprs.next(), assign + 1);
                        fields.push(Field { key: FieldKey::Expr(key), value: Some(value) });
                    }
                    None => fields.push(Field { key: FieldKey::Set, value: None }),
                }
            } else if field.has(T::Dot) {
                fields.push(Field { key: FieldKey::Set, value: None });
                if let Some(assign) = assign {
                    self.error(assign);
                    let value = self.expr_or_error(exprs.next(), assign + 1);
                    fields.push(Field { key: FieldKey::Positional, value: Some(value) });
                }
            } else if let (Some(assign), Some(name)) = (assign, field.token(T::Id)) {
                let value = self.expr_or_error(exprs.next(), assign + 1);
                fields.push(Field { key: FieldKey::Named(name.text().to_owned()), value: Some(value) });
            } else {
                let value = self.expr_or_error(exprs.next(), at);
                fields.push(Field { key: FieldKey::Positional, value: Some(value) });
            }
        }
        let range = self.range(node);
        self.push(ExprKind::Table { fields }, range)
    }

    fn expr(&mut self, node: Syntax<'a>) -> usize {
        let range = self.range(node);
        let kind = match node.kind() {
            N::BinaryExpr | N::IndexExpr | N::CallExpr => return self.chain(node),
            N::TableExpr => return self.table(node),
            N::ClosureExpr => return self.function(node, range, false),
            N::LiteralExpr => match node.first_token().map(|token| token.kind()) {
                Some(T::Number) => ExprKind::Number,
                Some(T::String) => ExprKind::Str,
                Some(T::HashString) => ExprKind::Hash,
                Some(T::Nil) => ExprKind::Nil,
                Some(T::True) => ExprKind::True,
                Some(T::False) => ExprKind::False,
                Some(T::Ellipsis) => ExprKind::Vararg,
                _ => ExprKind::Error,
            },
            N::NameExpr => {
                let name = node.code();
                ExprKind::Name { name: name.to_owned(), decl: self.resolve(name) }
            }
            N::ParenExpr => ExprKind::Paren(self.expr_or_error(node.expr(), range.0 + 1)),
            N::UnaryExpr => {
                let op = node.child(N::UnaryOp).and_then(|op| unary(op.code()));
                let operand = self.expr_or_error(node.expr(), range.0 + 1);
                match op {
                    Some(op) => ExprKind::Unary { op, operand },
                    None => return operand,
                }
            }
            _ => return self.error(range.0),
        };
        self.push(kind, range)
    }

    /// A chain of operators or suffixes is as deep as it is long on its left side: it is walked from
    /// its first operand outwards instead of recursed into.
    fn chain(&mut self, node: Syntax<'a>) -> usize {
        let binary_chain = node.is(N::BinaryExpr);
        let links = |node: Syntax| {
            if binary_chain { node.is(N::BinaryExpr) } else { matches!(node.kind(), N::IndexExpr | N::CallExpr) }
        };
        let base = self.spine.len();
        let mut first = Some(node);
        while let Some(link) = first.filter(|node| links(*node)) {
            self.spine.push(link);
            first = link.expr();
        }
        let start = self.range(node).0;
        let mut value = self.expr_or_error(first, start);
        while self.spine.len() > base {
            let Some(link) = self.spine.pop() else { break };
            let end = self.range(link).1;
            let kind = match link.kind() {
                N::BinaryExpr => {
                    let op = link.child(N::BinaryOp).and_then(|op| binary(op.code()));
                    let rhs = self.expr_or_error(link.exprs().nth(1), end);
                    match op {
                        Some((op, ..)) => ExprKind::Binary { op, lhs: value, rhs },
                        None => continue,
                    }
                }
                N::IndexExpr if link.has(T::LBrack) => {
                    let key = self.expr_or_error(link.exprs().nth(1), end);
                    ExprKind::Index { obj: value, key }
                }
                N::IndexExpr => {
                    let Some(name) = link.token(T::Id) else { continue };
                    let name = name.text().to_owned();
                    if !link.has(T::Colon) {
                        ExprKind::Field { obj: value, name, safe: link.has(T::Question) }
                    } else {
                        // `obj:name(...)` is one call: the method is not an expression of its own.
                        let call =
                            self.spine.last().copied().filter(|outer| self.spine.len() > base && outer.is(N::CallExpr));
                        let (args, end) = match call {
                            Some(call) => {
                                self.spine.pop();
                                (self.args(call), self.range(call).1)
                            }
                            None => (Vec::new(), end),
                        };
                        value = self.push(ExprKind::Call { callee: value, method: Some(name), args }, (start, end));
                        continue;
                    }
                }
                _ => ExprKind::Call { callee: value, method: None, args: self.args(link) },
            };
            value = self.push(kind, (start, end));
        }
        self.spine.truncate(base);
        value
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
