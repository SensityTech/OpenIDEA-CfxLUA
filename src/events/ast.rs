use crate::lua::Source;
use crate::lua::scope::Scopes;
use crate::lua::token::T;
use crate::lua::tree::{N, Node, Token};
use std::rc::Rc;

pub const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Compare,
    BOr,
    BXor,
    BAnd,
    Shift,
    Concat,
    Additive,
    Multiplicative,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Len,
    Neg,
    BNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Field(Option<u32>),
    Method(Option<u32>),
    Bracket(Option<u32>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: Option<u32>,
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
    Str,
    Hash,
    Name(u32),
    Func(u32),
    Table(Vec<Field>),
    Paren(u32),
    Unary(UnOp, u32),
    Binary(BinOp, u32, u32),
    Index(u32, Key),
    Call(u32, Vec<u32>),
    Error,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub first: u32,
    pub last: u32,
    pub func: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StmtKind {
    Local { names: Vec<u32>, values: Vec<u32>, unpack: Option<u32> },
    LocalFunction { decl: u32, func: u32 },
    Function { target: Option<u32>, func: u32 },
    Assign { targets: Vec<u32>, values: Vec<u32>, unpack: Option<u32> },
    Compound { target: u32, op: u32, value: Option<u32> },
    Expr(u32),
    Return(Vec<u32>),
    NumFor { var: u32, exprs: Vec<u32> },
    GenFor { vars: Vec<u32>, exprs: Vec<u32> },
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
    pub first: u32,
    pub last: u32,
    pub params: Vec<u32>,
    pub vararg: bool,
    pub parent: u32,
    pub exprs: (u32, u32),
    pub stmts: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Local { init: Option<u32> },
    LocalFunction(u32),
    Param(u32, u32),
    ForVar,
}

#[derive(Clone, Copy, Debug)]
pub struct Decl {
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
        let scopes = src.scopes();
        let mut lower = Lower {
            src,
            scopes,
            ast: Ast::default(),
            decls: vec![NONE; scopes.decls().len()],
            spine: Vec::new(),
            func: NONE,
        };
        lower.block(src.root());
        lower.ast
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

    pub fn unparen(&self, mut id: u32) -> u32 {
        while let ExprKind::Paren(inner) = self.kind(id) {
            id = *inner;
        }
        id
    }

    pub fn name<'s>(&self, src: &'s Source, id: u32) -> Option<&'s str> {
        matches!(self.kind(id), ExprKind::Name(_)).then(|| src.tok_text(self.expr(id).first as usize))
    }

    pub fn decl_of(&self, id: u32) -> u32 {
        match self.kind(id) {
            ExprKind::Name(d) => *d,
            _ => NONE,
        }
    }

    pub fn global<'s>(&self, src: &'s Source, id: u32) -> Option<&'s str> {
        (self.decl_of(id) == NONE).then(|| self.name(src, id)).flatten()
    }

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

    pub fn span(&self, src: &Source, id: u32) -> (usize, usize) {
        let e = self.expr(id);
        (src.toks[e.first as usize].start, src.toks[e.last as usize].end)
    }

    pub fn start(&self, src: &Source, id: u32) -> usize {
        src.toks[self.expr(id).first as usize].start
    }

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

    pub fn string(&self, src: &Source, id: u32) -> Option<String> {
        if *self.kind(id) != ExprKind::Str {
            return None;
        }
        Some(raw_string(src, self.expr(id).first as usize))
    }
}

pub fn raw_string(src: &Source, tok: usize) -> String {
    let (s, e) = src.toks[tok].content_range(&src.text);
    src.text[s..e].to_owned()
}

pub fn parse(src: &Source) -> Rc<Ast> {
    src.model(Ast::parse)
}

/// Reads the expressions, statements and functions off the syntax tree, in the order the source
/// writes them: what a node needs is numbered before the node itself.
struct Lower<'a> {
    src: &'a Source,
    scopes: &'a Scopes,
    ast: Ast,
    decls: Vec<u32>,
    spine: Vec<Node<'a>>,
    func: u32,
}

fn binary_op(kind: T) -> Option<BinOp> {
    Some(match kind {
        T::Or => BinOp::Or,
        T::And => BinOp::And,
        T::Eq | T::Ne | T::Lt | T::Le | T::Gt | T::Ge => BinOp::Compare,
        T::BitOr => BinOp::BOr,
        T::BitTilde => BinOp::BXor,
        T::BitAnd => BinOp::BAnd,
        T::BitLtLt | T::BitRtRt => BinOp::Shift,
        T::Concat => BinOp::Concat,
        T::Plus | T::Minus => BinOp::Additive,
        T::Mult | T::Div | T::DoubleDiv | T::Mod => BinOp::Multiplicative,
        T::Exp => BinOp::Pow,
        _ => return None,
    })
}

fn unary_op(kind: T) -> Option<UnOp> {
    Some(match kind {
        T::Not => UnOp::Not,
        T::GetN => UnOp::Len,
        T::Minus => UnOp::Neg,
        T::BitTilde => UnOp::BNot,
        _ => return None,
    })
}

impl<'a> Lower<'a> {
    fn tok(&self, token: Token) -> u32 {
        self.src.tok_of(token.index()) as u32
    }

    fn at(&self, node: Node, kind: T) -> Option<u32> {
        node.token(kind).map(|token| self.tok(token))
    }

    fn span(&self, node: Node) -> (u32, u32) {
        match (node.first_token(), node.last_token()) {
            (Some(first), Some(last)) => (self.tok(first), self.tok(last)),
            _ => {
                let at = self.src.tok_of(node.token_span().0) as u32;
                (at, at)
            }
        }
    }

    fn push_expr(&mut self, kind: ExprKind, node: Node) -> u32 {
        let (first, last) = self.span(node);
        self.ast.exprs.push(Expr { kind, first, last, func: self.func });
        self.ast.exprs.len() as u32 - 1
    }

    fn push_stmt(&mut self, kind: StmtKind, node: Node) {
        let (first, mut last) = self.span(node);
        if !node.is(N::ReturnStat) && last > first && node.last_token().is_some_and(|token| token.is(T::Semi)) {
            last -= 1;
        }
        self.ast.stmts.push(Stmt { kind, first, last, func: self.func });
    }

    fn declare(&mut self, node: Node, kind: DeclKind) -> u32 {
        let tok = self.at(node, T::Id).unwrap_or(self.span(node).0);
        self.ast.decls.push(Decl { tok, kind });
        let id = self.ast.decls.len() as u32 - 1;
        if let Some(decl) = self.scopes.id_of(node) {
            self.decls[decl as usize] = id;
        }
        id
    }

    fn lookup(&self, node: Node) -> u32 {
        match self.scopes.id_of(node) {
            Some(decl) if self.scopes.decl(decl).kind != crate::lua::scope::DeclKind::Global => {
                self.decls[decl as usize]
            }
            _ => NONE,
        }
    }

    fn block(&mut self, node: Node<'a>) {
        for child in node.children() {
            if child.kind().is_statement() {
                self.stmt(child);
            }
        }
    }

    fn inner(&mut self, node: Node<'a>) {
        for child in node.children() {
            if child.is(N::Block) {
                self.block(child);
            } else if child.kind().is_expr() {
                self.expr(child);
            }
        }
    }

    fn stmt(&mut self, node: Node<'a>) {
        let kind = match node.kind() {
            N::EmptyStat => return,
            N::LocalDef => {
                let unpack = node.has(T::In);
                let (values, unpack) = if unpack {
                    (Vec::new(), node.expr().and_then(|source| self.expr(source)))
                } else {
                    (self.exprs(node.child(N::ExprList)), None)
                };
                // Declared after the values: `local x = x` reads the outer `x`.
                let mut names = Vec::new();
                for (k, name) in
                    node.child(N::NameList).into_iter().flat_map(|list| list.children_of(N::NameDef)).enumerate()
                {
                    let init = if unpack.is_none() { values.get(k).copied() } else { None };
                    names.push(self.declare(name, DeclKind::Local { init }));
                }
                StmtKind::Local { names, values, unpack }
            }
            N::LocalFuncDef => {
                let first = self.at(node, T::Function).unwrap_or(self.span(node).0);
                if node.has(T::Id) {
                    let decl = self.declare(node, DeclKind::LocalFunction(NONE));
                    let func = self.func(node, first);
                    self.ast.decls[decl as usize].kind = DeclKind::LocalFunction(func);
                    StmtKind::LocalFunction { decl, func }
                } else {
                    self.func(node, first);
                    StmtKind::Other
                }
            }
            N::FuncDef => {
                let target = node.token(T::Id).map(|name| {
                    let tok = self.tok(name);
                    self.ast.exprs.push(Expr {
                        kind: ExprKind::Name(self.lookup(node)),
                        first: tok,
                        last: tok,
                        func: self.func,
                    });
                    self.ast.exprs.len() as u32 - 1
                });
                let func = self.func(node, self.span(node).0);
                StmtKind::Function { target, func }
            }
            N::ClassMethodDef => {
                let target = node.child(N::ClassMethodName).and_then(|name| {
                    let object = self.expr(name.expr()?)?;
                    let member = self.at(name, T::Id);
                    let key = if name.has(T::Colon) { Key::Method(member) } else { Key::Field(member) };
                    Some(self.push_expr(ExprKind::Index(object, key), name))
                });
                let func = self.func(node, self.span(node).0);
                StmtKind::Function { target, func }
            }
            N::AssignStat => {
                let targets = self.exprs(node.child(N::VarList));
                if node.has(T::In) {
                    let unpack = node.expr().and_then(|source| self.expr(source));
                    StmtKind::Assign { targets, values: Vec::new(), unpack }
                } else {
                    StmtKind::Assign { targets, values: self.exprs(node.child(N::ExprList)), unpack: None }
                }
            }
            N::CompoundAssignStat => {
                let mut operands = node.exprs();
                let Some(target) = operands.next().and_then(|target| self.expr(target)) else { return };
                let op = self.ast.exprs[target as usize].last + 1;
                let value = operands.next().and_then(|value| self.expr(value));
                StmtKind::Compound { target, op, value }
            }
            N::ExprStat => {
                let Some(expr) = node.expr() else { return };
                let Some(value) = self.expr(expr) else { return };
                self.push_stmt(StmtKind::Expr(value), expr);
                return;
            }
            N::ReturnStat => StmtKind::Return(self.exprs(node.child(N::ExprList))),
            N::ForAStat | N::ForBStat => {
                let exprs = match node.child(N::ExprList) {
                    Some(list) => self.exprs(Some(list)),
                    None => self.exprs(Some(node)),
                };
                let vars: Vec<u32> =
                    node.children_of(N::ParamNameDef).map(|var| self.declare(var, DeclKind::ForVar)).collect();
                if let Some(body) = node.child(N::Block) {
                    self.block(body);
                }
                match vars.as_slice() {
                    [var] if node.is(N::ForAStat) => StmtKind::NumFor { var: *var, exprs },
                    _ => StmtKind::GenFor { vars, exprs },
                }
            }
            _ => {
                self.inner(node);
                StmtKind::Other
            }
        };
        self.push_stmt(kind, node);
    }

    fn func(&mut self, owner: Node<'a>, first: u32) -> u32 {
        let id = self.ast.funcs.len() as u32;
        self.ast.funcs.push(Func {
            first,
            last: first,
            params: Vec::new(),
            vararg: false,
            parent: self.func,
            exprs: (0, 0),
            stmts: (0, 0),
        });
        let outer = std::mem::replace(&mut self.func, id);
        let (e0, s0) = (self.ast.exprs.len() as u32, self.ast.stmts.len() as u32);
        let (mut params, mut vararg, mut last) = (Vec::new(), false, self.span(owner).1.max(first));
        if let Some(body) = owner.child(N::FuncBody) {
            for (n, param) in body.children_of(N::ParamNameDef).enumerate() {
                params.push(self.declare(param, DeclKind::Param(id, n as u32)));
            }
            vararg = body.has(T::Ellipsis);
            if let Some(block) = body.child(N::Block) {
                self.block(block);
            }
            last = self.at(body, T::End).unwrap_or(self.span(body).1).max(first);
        }
        self.func = outer;
        let (e1, s1) = (self.ast.exprs.len() as u32, self.ast.stmts.len() as u32);
        let f = &mut self.ast.funcs[id as usize];
        f.last = last;
        f.params = params;
        f.vararg = vararg;
        f.exprs = (e0, e1);
        f.stmts = (s0, s1);
        id
    }

    /// The expressions of a list, up to the first one that is missing.
    fn exprs(&mut self, list: Option<Node<'a>>) -> Vec<u32> {
        let mut out = Vec::new();
        for expr in list.into_iter().flat_map(Node::exprs) {
            match self.expr(expr) {
                Some(value) => out.push(value),
                None => break,
            }
        }
        out
    }

    fn args(&mut self, call: Node<'a>) -> Vec<u32> {
        if let Some(list) = call.child(N::ListArgs) {
            return self.exprs(Some(list));
        }
        call.child(N::SingleArg).and_then(Node::expr).and_then(|arg| self.expr(arg)).into_iter().collect()
    }

    fn fields(&mut self, table: Node<'a>) -> Vec<Field> {
        let mut out = Vec::new();
        for field in table.children_of(N::TableField) {
            let first = self.span(field).0;
            let mut exprs = field.exprs();
            if field.has(T::LBrack) {
                let key = exprs.next().and_then(|key| self.expr(key));
                if let Some(value) = exprs.next().and_then(|value| self.expr(value)) {
                    out.push(Field { name: None, key, value, first });
                }
            } else if field.has(T::Assign) {
                let name = self.at(field, T::Id);
                if let Some(value) = exprs.next().and_then(|value| self.expr(value)) {
                    out.push(Field { name, key: None, value, first: name.unwrap_or(first) });
                }
            } else if let Some(value) = exprs.next().and_then(|value| self.expr(value)) {
                out.push(Field { name: None, key: None, value, first });
            }
        }
        out
    }

    fn expr(&mut self, node: Node<'a>) -> Option<u32> {
        let kind = match node.kind() {
            N::BinaryExpr | N::IndexExpr | N::CallExpr => return self.chain(node),
            N::LiteralExpr => match node.first_token()?.kind() {
                T::Number => ExprKind::Number,
                T::String => ExprKind::Str,
                T::HashString => ExprKind::Hash,
                T::Nil => ExprKind::Nil,
                T::True => ExprKind::True,
                T::False => ExprKind::False,
                T::Ellipsis => ExprKind::Vararg,
                _ => return None,
            },
            N::NameExpr => ExprKind::Name(self.lookup(node)),
            N::ParenExpr => node.expr().and_then(|inner| self.expr(inner)).map_or(ExprKind::Error, ExprKind::Paren),
            N::UnaryExpr => {
                let op = unary_op(node.child(N::UnaryOp)?.first_token()?.kind())?;
                ExprKind::Unary(op, self.expr(node.expr()?)?)
            }
            N::TableExpr => ExprKind::Table(self.fields(node)),
            N::ClosureExpr => {
                let first = self.at(node, T::Function).unwrap_or(self.span(node).0);
                ExprKind::Func(self.func(node, first))
            }
            _ => return None,
        };
        Some(self.push_expr(kind, node))
    }

    /// A chain of operators or suffixes is as deep as it is long on its left side: it is walked from
    /// its first operand outwards instead of recursed into.
    fn chain(&mut self, node: Node<'a>) -> Option<u32> {
        let binary = node.is(N::BinaryExpr);
        let links = |node: Node| {
            if binary { node.is(N::BinaryExpr) } else { matches!(node.kind(), N::IndexExpr | N::CallExpr) }
        };
        let base = self.spine.len();
        let mut first = node;
        while links(first) {
            self.spine.push(first);
            match first.expr() {
                Some(left) => first = left,
                None => {
                    self.spine.truncate(base);
                    return None;
                }
            }
        }
        let Some(mut value) = self.expr(first) else {
            self.spine.truncate(base);
            return None;
        };
        while self.spine.len() > base {
            let Some(link) = self.spine.pop() else { break };
            let kind = match link.kind() {
                N::BinaryExpr => {
                    let op = link.child(N::BinaryOp).and_then(Node::first_token).and_then(|op| binary_op(op.kind()));
                    let right = link.exprs().nth(1).and_then(|right| self.expr(right));
                    match (op, right) {
                        (Some(op), Some(right)) => ExprKind::Binary(op, value, right),
                        _ => break,
                    }
                }
                N::IndexExpr if link.has(T::LBrack) => {
                    ExprKind::Index(value, Key::Bracket(link.exprs().nth(1).and_then(|key| self.expr(key))))
                }
                N::IndexExpr => {
                    let name = self.at(link, T::Id);
                    ExprKind::Index(value, if link.has(T::Colon) { Key::Method(name) } else { Key::Field(name) })
                }
                _ => ExprKind::Call(value, self.args(link)),
            };
            value = self.push_expr(kind, link);
        }
        self.spine.truncate(base);
        Some(value)
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
