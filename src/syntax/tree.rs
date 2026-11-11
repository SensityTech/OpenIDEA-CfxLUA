use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::lua::token::T;
use crate::lua::tree::{N, Node, Token};

pub type Span = (usize, usize);

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
        let mut lower = Lower { src, funcs: Vec::new(), stack: Vec::new() };
        let mut block = lower.block(src.root());
        block.end = src.toks.len();
        let funcs = lower.funcs;
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

/// Reads the statements and functions off the syntax tree, as spans of `Source::toks`.
struct Lower<'a> {
    src: &'a Source,
    funcs: Vec<Func>,
    stack: Vec<usize>,
}

impl<'a> Lower<'a> {
    fn tok(&self, token: Token) -> usize {
        self.src.tok_of(token.index())
    }

    fn at(&self, node: Node, kind: T) -> Option<usize> {
        node.token(kind).map(|token| self.tok(token))
    }

    fn span(&self, node: Node) -> Span {
        match (node.first_token(), node.last_token()) {
            (Some(first), Some(last)) => (self.tok(first), self.tok(last) + 1),
            _ => {
                let at = self.src.tok_of(node.token_span().0);
                (at, at)
            }
        }
    }

    fn spans(&self, list: Option<Node>) -> Vec<Span> {
        list.into_iter().flat_map(Node::exprs).map(|expr| self.span(expr)).collect()
    }

    /// The functions written in `node`, in source order; a function holds its own.
    fn closures(&mut self, node: Node<'a>) {
        let mut id = node.id();
        while id < node.next_id() {
            let inner = node.tree().node(node.source(), id);
            if inner.is(N::FuncBody) {
                self.func(inner);
                id = inner.next_id();
            } else {
                id += 1;
            }
        }
    }

    fn func(&mut self, body: Node<'a>) -> usize {
        let idx = self.funcs.len();
        let owner = body.parent();
        let kw = owner.and_then(|owner| self.at(owner, T::Function)).unwrap_or(self.span(body).0);
        let at = self.span(body).0;
        self.funcs.push(Func {
            kw,
            params: body.children_of(N::ParamNameDef).map(|param| self.span(param).0).collect(),
            vararg: body.has(T::Ellipsis),
            close: self.at(body, T::RParen),
            body: Block { stmts: Vec::new(), end: at },
            end: self.at(body, T::End),
            next: at,
            parent: self.stack.last().copied(),
        });
        self.stack.push(idx);
        let block = body.child(N::Block).map(|block| self.block(block));
        self.stack.pop();
        let f = &mut self.funcs[idx];
        if let Some(block) = block {
            f.body = block;
        }
        f.next = f.end.map_or(f.body.end, |end| end + 1).max(at);
        idx
    }

    /// A function statement whose parameter list is missing still counts as a function.
    fn func_of(&mut self, stat: Node<'a>) -> usize {
        if let Some(body) = stat.child(N::FuncBody) {
            return self.func(body);
        }
        let at = self.span(stat).1;
        self.funcs.push(Func {
            kw: self.at(stat, T::Function).unwrap_or(at),
            params: Vec::new(),
            vararg: false,
            close: None,
            body: Block { stmts: Vec::new(), end: at },
            end: None,
            next: at,
            parent: self.stack.last().copied(),
        });
        self.funcs.len() - 1
    }

    fn block(&mut self, node: Node<'a>) -> Block {
        let mut stmts = Vec::new();
        for child in node.children() {
            match child.kind() {
                N::EmptyStat | N::ShebangLine => {}
                N::Error => {
                    self.closures(child);
                    let (start, end) = self.span(child);
                    stmts.push(Stmt { start, end: end.max(start + 1), kind: Kind::Error });
                }
                N::ExprStat => self.expr_stat(child, &mut stmts),
                _ => stmts.push(self.stmt(child)),
            }
        }
        Block { stmts, end: self.src.tok_of(node.token_span().1) }
    }

    /// `i++` and `++i` are two statements here: the variable, and the operator CfxLua does not have.
    fn expr_stat(&mut self, node: Node<'a>, stmts: &mut Vec<Stmt>) {
        self.closures(node);
        for child in node.children() {
            let (start, end) = self.span(child);
            if child.is(N::Error) {
                stmts.push(Stmt { start, end: end.max(start + 1), kind: Kind::Error });
            } else if child.kind().is_expr() {
                stmts.push(Stmt { start, end, kind: Kind::Expr((start, end)) });
            }
        }
    }

    fn stmt(&mut self, node: Node<'a>) -> Stmt {
        let (start, mut end) = self.span(node);
        if !node.is(N::ReturnStat) && node.last_token().is_some_and(|token| token.is(T::Semi)) {
            end -= 1;
        }
        let mut blocks = Vec::new();
        let mut func = None;
        match node.kind() {
            N::FuncDef | N::ClassMethodDef | N::LocalFuncDef => func = Some(self.func_of(node)),
            _ => {
                for child in node.children() {
                    if child.is(N::Block) {
                        blocks.push(self.block(child));
                    } else {
                        self.closures(child);
                    }
                }
            }
        }
        let mut body = if blocks.is_empty() || node.is(N::IfStat) { None } else { Some(blocks.remove(0)) };
        let mut body = || body.take().unwrap_or(Block { stmts: Vec::new(), end });
        let kind = match node.kind() {
            N::LocalDef => self.local(node),
            N::LocalFuncDef => match (self.at(node, T::Id), func) {
                (Some(name), Some(func)) => Kind::LocalFunction { name, func },
                _ => Kind::Error,
            },
            N::FuncDef | N::ClassMethodDef => {
                let after = self.at(node, T::Function).map_or(start, |kw| kw + 1);
                let name = node.child(N::ClassMethodName).map(|name| self.span(name));
                let name = name.or_else(|| self.at(node, T::Id).map(|id| (id, id + 1))).unwrap_or((after, after));
                Kind::Function { name: (after, name.1), func: func.unwrap_or(0) }
            }
            N::AssignStat => {
                let unpack = node.has(T::In);
                let values = if unpack { self.spans(Some(node)) } else { self.spans(node.child(N::ExprList)) };
                Kind::Assign { targets: self.spans(node.child(N::VarList)), values, unpack }
            }
            N::CompoundAssignStat => {
                let mut exprs = node.exprs();
                let target = exprs.next().map_or((start, start), |target| self.span(target));
                let op = target.1;
                let value = exprs.next().map_or((op + 1, op + 1), |value| self.span(value));
                Kind::Compound { target, op, value }
            }
            N::IfStat => Kind::If {
                conds: node.exprs().map(|cond| self.span(cond)).collect(),
                has_else: node.has(T::Else),
                blocks,
            },
            N::WhileStat => Kind::While { cond: node.expr().map_or((end, end), |cond| self.span(cond)), body: body() },
            N::DoStat => Kind::Do(body()),
            N::ForAStat => Kind::For {
                vars: node.children_of(N::ParamNameDef).map(|var| self.span(var).0).collect(),
                exprs: node.exprs().map(|expr| self.span(expr)).collect(),
                body: body(),
            },
            N::ForBStat => Kind::For {
                vars: node.children_of(N::ParamNameDef).map(|var| self.span(var).0).collect(),
                exprs: self.spans(node.child(N::ExprList)),
                body: body(),
            },
            N::RepeatStat => {
                let body = body();
                let cond = node.expr().map_or((body.end, body.end), |cond| self.span(cond));
                Kind::Repeat { body, cond }
            }
            N::ReturnStat => Kind::Return { exprs: self.spans(node.child(N::ExprList)) },
            N::BreakStat => Kind::Break,
            N::GotoStat => Kind::Goto,
            N::LabelStat => Kind::Label,
            _ => Kind::Error,
        };
        Stmt { start, end, kind }
    }

    fn local(&self, node: Node<'a>) -> Kind {
        let mut names: Vec<LocalName> = Vec::new();
        for child in node.child(N::NameList).into_iter().flat_map(Node::children) {
            match child.kind() {
                N::NameDef => names.push(LocalName { tok: self.span(child).0, attr: None }),
                N::Attribute => {
                    let closed = self.at(child, T::Lt).zip(self.at(child, T::Gt)).filter(|_| child.has(T::Id));
                    if let Some(name) = names.last_mut() {
                        name.attr = closed;
                    }
                }
                _ => {}
            }
        }
        if names.is_empty() {
            return Kind::Error;
        }
        let unpack = node.has(T::In);
        let values = if unpack { self.spans(Some(node)) } else { self.spans(node.child(N::ExprList)) };
        Kind::Local { names, values, unpack }
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
        Tok::Name => src.is_op(b - 2, "."),
        Tok::Op if last.text(&src.text) == "]" => src.pair[b - 1].is_some_and(|o| src.is_op(o, "[")),
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
        assert!(tree.funcs.len() >= 60);
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
