//! Lexical scoping over a [`Tree`]: which declaration (local, local function,
//! parameter, loop variable) each name token refers to. `do`, `then`,
//! `else`, loop bodies, `repeat … until` and functions open scopes; a local is
//! visible after its statement, a local function inside its own body.

use super::tree::{Block, Func, Kind, Span, Stmt, Tree};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Local,
    LocalFunction,
    Param,
    ForVar,
}

#[derive(Clone, Debug)]
pub struct Decl {
    pub tok: usize,
    pub kind: DeclKind,
    /// `<const>` / `<close>`: the `<` and `>` tokens.
    pub attr: Option<(usize, usize)>,
    /// The value of `local x = v` at the same position.
    pub init: Option<Span>,
    /// Parameters: their function and position.
    pub func: Option<usize>,
    pub index: usize,
    /// Assigned after its declaration (`x = …`, `x += …`, `x.f = …`, `function x()`).
    pub assigned: bool,
}

pub struct Scopes {
    pub decls: Vec<Decl>,
    /// Per code token: declaration index + 1 (0 for a global, a field, a keyword…).
    refs: Vec<u32>,
}

impl Scopes {
    pub fn new(src: &Source, tree: &Tree) -> Self {
        let mut w = Walker {
            src,
            tree,
            visible: HashMap::new(),
            scopes: Vec::new(),
            out: Scopes { decls: Vec::new(), refs: vec![0; src.toks.len()] },
        };
        w.block(&tree.block);
        w.out
    }

    /// The declaration code token `tok` names (a reference or the declaration itself).
    pub fn decl_of(&self, tok: usize) -> Option<usize> {
        self.refs.get(tok).and_then(|&r| r.checked_sub(1)).map(|r| r as usize)
    }

    pub fn decl(&self, tok: usize) -> Option<&Decl> {
        self.decl_of(tok).map(|d| &self.decls[d])
    }

    /// The tokens of `[from, to)` referring to declaration `decl` (the declaration excluded).
    pub fn uses(&self, decl: usize, from: usize, to: usize) -> impl Iterator<Item = usize> + '_ {
        let target = decl as u32 + 1;
        let skip = self.decls[decl].tok;
        (from..to.min(self.refs.len())).filter(move |&t| self.refs[t] == target && t != skip)
    }

    /// The text of the attribute of `decl` (`const`, `close`), if any.
    pub fn attr<'a>(&self, src: &'a Source, decl: &Decl) -> Option<&'a str> {
        decl.attr.map(|(lt, _)| src.tok_text(lt + 1))
    }
}

struct Walker<'a> {
    src: &'a Source,
    tree: &'a Tree,
    visible: HashMap<&'a str, Vec<usize>>,
    scopes: Vec<Vec<&'a str>>,
    out: Scopes,
}

impl<'a> Walker<'a> {
    fn push(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop(&mut self) {
        for name in self.scopes.pop().unwrap_or_default() {
            if let Some(v) = self.visible.get_mut(name) {
                v.pop();
            }
        }
    }

    fn declare(&mut self, decl: Decl) {
        let src = self.src;
        let name = src.tok_text(decl.tok);
        let d = self.out.decls.len();
        if let Some(r) = self.out.refs.get_mut(decl.tok) {
            *r = d as u32 + 1;
        }
        self.out.decls.push(decl);
        self.visible.entry(name).or_default().push(d);
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name);
        }
    }

    fn local(&mut self, tok: usize, kind: DeclKind) -> Decl {
        Decl { tok, kind, attr: None, init: None, func: None, index: 0, assigned: false }
    }

    fn reference(&mut self, tok: usize) -> Option<usize> {
        let d = *self.visible.get(self.src.tok_text(tok))?.last()?;
        if let Some(r) = self.out.refs.get_mut(tok) {
            *r = d as u32 + 1;
        }
        Some(d)
    }

    fn block(&mut self, b: &Block) {
        self.push();
        for s in &b.stmts {
            self.stmt(s);
        }
        self.pop();
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            Kind::Local { names, values, unpack } => {
                self.exprs(values);
                for (k, n) in names.iter().enumerate() {
                    let mut d = self.local(n.tok, DeclKind::Local);
                    d.attr = n.attr;
                    d.init = if *unpack { None } else { values.get(k).copied() };
                    self.declare(d);
                }
            }
            Kind::LocalFunction { name, func } => {
                let d = self.local(*name, DeclKind::LocalFunction);
                self.declare(d);
                self.func(*func);
            }
            Kind::Function { name, func } => {
                if name.1 > name.0 {
                    self.target(*name);
                }
                self.func(*func);
            }
            Kind::Assign { targets, values, .. } => {
                self.exprs(values);
                for t in targets {
                    self.target(*t);
                }
            }
            Kind::Compound { target, value, .. } => {
                self.expr(*value);
                self.target(*target);
            }
            Kind::Expr(span) => self.expr(*span),
            Kind::If { conds, blocks, .. } => {
                for (k, b) in blocks.iter().enumerate() {
                    if let Some(c) = conds.get(k) {
                        self.expr(*c);
                    }
                    self.block(b);
                }
            }
            Kind::While { cond, body } => {
                self.expr(*cond);
                self.block(body);
            }
            Kind::Do(body) => self.block(body),
            Kind::For { vars, exprs, body } => {
                self.exprs(exprs);
                self.push();
                for &v in vars {
                    let d = self.local(v, DeclKind::ForVar);
                    self.declare(d);
                }
                self.block(body);
                self.pop();
            }
            Kind::Repeat { body, cond } => {
                // `until` sees the locals of the body.
                self.push();
                for st in &body.stmts {
                    self.stmt(st);
                }
                self.expr(*cond);
                self.pop();
            }
            Kind::Return { exprs } => self.exprs(exprs),
            Kind::Break | Kind::Goto | Kind::Label | Kind::Error => {
                self.expr((s.start, s.end));
            }
        }
    }

    fn func(&mut self, f: usize) {
        let tree = self.tree;
        let func: &Func = &tree.funcs[f];
        self.push();
        for (index, &p) in func.params.iter().enumerate() {
            let mut d = self.local(p, DeclKind::Param);
            d.func = Some(f);
            d.index = index;
            self.declare(d);
        }
        for s in &func.body.stmts {
            self.stmt(s);
        }
        self.pop();
    }

    /// An assignment target: its first name is written, the rest of it is read.
    fn target(&mut self, (a, b): Span) {
        if self.src.is_name(a) {
            if let Some(d) = self.reference(a) {
                self.out.decls[d].assigned = true;
            }
            self.expr((a + 1, b));
        } else {
            self.expr((a, b));
        }
    }

    fn exprs(&mut self, spans: &[Span]) {
        for s in spans {
            self.expr(*s);
        }
    }

    fn expr(&mut self, (a, b): Span) {
        let src = self.src;
        let mut i = a;
        while i < b.min(src.toks.len()) {
            let t = src.toks[i];
            if t.is_kw(&src.text, "function")
                && let Some(f) = self.tree.func_at(i)
            {
                self.func(f);
                i = self.tree.funcs[f].next.max(i + 1);
                continue;
            }
            if t.kind == Tok::Name && is_reference(src, i) {
                self.reference(i);
            }
            i += 1;
        }
    }
}

/// Whether name token `i` reads a variable (not a field, a method, a label or a table key).
fn is_reference(src: &Source, i: usize) -> bool {
    if i > 0 {
        let p = src.toks[i - 1];
        if p.kind == Tok::Op && matches!(p.text(&src.text), "." | ":" | "?." | "?:" | "::") {
            return false;
        }
        if p.is_kw(&src.text, "goto") {
            return false;
        }
    }
    // `{ key = v }` (assignment targets are walked apart, so `=` here is a table key's).
    !(src.is_op(i + 1, "=") && i > 0 && (src.is_op(i - 1, "{") || src.is_op(i - 1, ",") || src.is_op(i - 1, ";")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(text: &str) -> (Source, Scopes) {
        let src = Source::new(text);
        let tree = Tree::new(&src);
        let scopes = Scopes::new(&src, &tree);
        (src, scopes)
    }

    /// Index of the `n`-th token named `name`.
    fn nth(src: &Source, name: &str, n: usize) -> usize {
        src.toks.iter().enumerate().filter(|(_, t)| t.text(&src.text) == name).nth(n).unwrap().0
    }

    /// The declaration token the `n`-th token named `name` refers to.
    fn decl_tok(src: &Source, scopes: &Scopes, name: &str, n: usize) -> Option<usize> {
        scopes.decl(nth(src, name, n)).map(|d| d.tok)
    }

    #[test]
    fn locals_params_and_shadowing() {
        let (src, s) = resolve(
            "local x = 1\nlocal function f(x, y)\n  local x = x\n  return x, y\nend\nprint(x)\nlocal t = { x = x }\nt.x = 2",
        );
        let x = |n| nth(&src, "x", n);
        // `local x = x` in f: the value is the parameter, the later read the new local.
        assert_eq!(decl_tok(&src, &s, "x", 3), Some(x(1)));
        assert_eq!(decl_tok(&src, &s, "x", 4), Some(x(2)));
        assert_eq!(decl_tok(&src, &s, "x", 5), Some(x(0)), "print(x) after f");
        assert_eq!(decl_tok(&src, &s, "x", 6), None, "a table key");
        assert_eq!(decl_tok(&src, &s, "x", 7), Some(x(0)));
        assert_eq!(decl_tok(&src, &s, "x", 8), None, "a field");
        let t = s.decls.iter().find(|d| src.tok_text(d.tok) == "t").unwrap();
        assert!(t.assigned, "t.x = 2 writes t");
        assert_eq!(s.decls.iter().find(|d| src.tok_text(d.tok) == "y").unwrap().kind, DeclKind::Param);
    }

    #[test]
    fn attributes_loops_and_repeat() {
        let (src, s) = resolve(
            "local c <const> = 1\nc += 1\nfor i = 1, 2 do print(i) end\nprint(i)\nrepeat local r = 1 until r\nlocal k <close> in src",
        );
        let c = s.decl(nth(&src, "c", 1)).unwrap();
        assert_eq!(s.attr(&src, c), Some("const"));
        assert!(c.assigned);
        assert_eq!(decl_tok(&src, &s, "i", 1), Some(nth(&src, "i", 0)));
        assert_eq!(decl_tok(&src, &s, "i", 2), None);
        assert!(decl_tok(&src, &s, "r", 1).is_some(), "until sees the body");
        let k = s.decls.iter().find(|d| src.tok_text(d.tok) == "k").unwrap();
        assert_eq!(s.attr(&src, k), Some("close"));
        assert!(k.init.is_none());
    }
}
