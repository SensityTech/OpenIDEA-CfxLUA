//! What a file gives OpenIDEA's document indexer: the names whose uses Find Usages gathers across a project.
//! A key is read from the text of the file alone, the host indexing a file again only when that file changes.
//! Every anchor is a reference: Go to Declaration stays with the type engine.

use crate::editor::target::{is_generic, token_at};
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::scope::DeclKind;
use crate::lua::token::{T, lex};
use crate::lua::tree::{N, Node, Tree};
use crate::ty::members::module_of;
use crate::ty::{SELF, Ty, psi, resolve};
use std::rc::Rc;

/// The host refuses a key longer than this.
const KEY_LIMIT: usize = 1024;
const GLOBALS: &str = "_G";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    /// A class or an alias named in a comment.
    Class,
    /// A global variable or function.
    Global,
    /// A member of a table or of a class the file itself names.
    Member,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub family: Family,
    pub key: String,
    pub start: usize,
    pub end: usize,
}

struct Reading<'a> {
    src: &'a Source,
    module: Option<Rc<str>>,
    library: &'a dyn Fn(&str) -> bool,
    out: Vec<Anchor>,
}

fn class_key(name: &str) -> String {
    format!("lua.class#{name}")
}

fn member_key(owner: &str, name: &str) -> String {
    format!("lua.member:{owner}#{name}")
}

fn push(out: &mut Vec<Anchor>, family: Family, key: String, (start, end): (usize, usize)) {
    if key.len() <= KEY_LIMIT && start < end {
        out.push(Anchor { family, key, start, end });
    }
}

/// `---@field name T` declares a member of the class of its comment, or of the class it names.
fn doc_field(tag: Node, out: &mut Vec<Anchor>) {
    let Some(name) = tag.token(T::Id) else { return };
    let named = tag.child(N::DocClassNameRef).map(Node::code);
    let comment = tag.ancestor(N::DocComment).map(Comment);
    let class = named.or_else(|| comment?.tag(N::DocTagClass).and_then(tags::name));
    if let Some(class) = class {
        push(out, Family::Member, member_key(class, name.text()), (name.start(), name.end()));
    }
}

/// What the comments of a file name: the classes and the aliases they declare and refer to, the fields they
/// declare.
fn comments(docs: Node, out: &mut Vec<Anchor>) {
    for node in docs.descendants() {
        match node.kind() {
            N::DocClassNameRef => {
                let named = node.token(T::Id).filter(|id| Ty::builtin(id.text()).is_none() && !is_generic(node));
                if let Some(id) = named {
                    push(out, Family::Class, class_key(id.text()), (id.start(), id.end()));
                }
            }
            N::DocTagClass | N::DocTagAlias => {
                if let Some(id) = node.token(T::Id) {
                    push(out, Family::Class, class_key(id.text()), (id.start(), id.end()));
                }
            }
            N::DocTagField => doc_field(node, out),
            _ => {}
        }
    }
}

fn in_order(mut out: Vec<Anchor>) -> Vec<Anchor> {
    out.sort_by_key(|anchor| (anchor.start, anchor.end));
    out.dedup_by_key(|anchor| (anchor.start, anchor.end));
    out
}

impl Reading<'_> {
    fn push(&mut self, family: Family, key: String, range: (usize, usize)) {
        push(&mut self.out, family, key, range);
    }

    fn is_local(&self, name: Node) -> bool {
        self.src.scopes().resolve(name).is_some_and(|decl| decl.kind != DeclKind::Global)
    }

    /// Whether a global name is one of the project: `self`, `_G` and what a library declares are not.
    fn of_project(&self, name: &str) -> bool {
        name != SELF && name != GLOBALS && !(self.library)(name)
    }

    /// The name the members of a global are kept under: its own, after the module a `---@module` file declares.
    fn global_path(&self, name: &str) -> String {
        match &self.module {
            Some(module) => format!("{module}.{name}"),
            None => name.to_owned(),
        }
    }

    fn global_key(&self, name: &str) -> String {
        match &self.module {
            Some(module) => member_key(module, name),
            None => format!("lua.global#{name}"),
        }
    }

    /// The class a local is declared as: `---@class Name` over its `local` statement.
    fn class_of_local(&self, name: Node) -> Option<String> {
        let decl = self.src.scopes().resolve(name)?;
        let root = self.src.root();
        let statement = root.tree().node(root.source(), decl.node).ancestor(N::LocalDef)?;
        self.class_over(statement)
    }

    fn class_over(&self, statement: Node) -> Option<String> {
        let class = self.src.comment_of(statement)?.tag(N::DocTagClass)?;
        tags::name(class).map(str::to_owned)
    }

    fn is_globals_table(&self, expr: Node) -> bool {
        expr.is(N::NameExpr) && expr.code() == GLOBALS && !self.is_local(expr)
    }

    /// The name the members of what an expression stands for are kept under, when the file alone tells it: a
    /// global, a path of names from one, a local declared as a class, `self` in a method of any of these.
    fn owner(&self, expr: Node) -> Option<String> {
        match expr.kind() {
            N::NameExpr if self.is_local(expr) => self.class_of_local(expr),
            N::NameExpr => {
                // `function self.a:b()` names its own object: there is no method around to read it from.
                let named = expr.ancestor(N::ClassMethodName).is_some();
                match resolve::self_owner(expr).filter(|_| !named) {
                    Some(object) => self.owner(object),
                    None => self.of_project(expr.code()).then(|| self.global_path(expr.code())),
                }
            }
            N::IndexExpr => {
                let prefix = psi::prefix(expr)?;
                let name = psi::member_name(self.src, expr)?;
                if self.is_globals_table(prefix) {
                    return self.of_project(&name).then(|| self.global_path(&name));
                }
                Some(format!("{}.{name}", self.owner(prefix)?))
            }
            _ => None,
        }
    }

    /// The name the fields of a table constructor are kept under: what the table is assigned to, or the field
    /// of such a table it is the value of.
    fn table_owner(&self, table: Node) -> Option<String> {
        let parent = table.parent()?;
        match parent.kind() {
            N::TableField if psi::field_value(parent) == Some(table) => {
                let name = psi::field_name(self.src, parent)?;
                let outer = parent.parent().filter(|outer| outer.is(N::TableExpr))?;
                Some(format!("{}.{name}", self.table_owner(outer)?))
            }
            N::ExprList => {
                let (statement, rank) = (parent.parent()?, psi::rank(table));
                match statement.kind() {
                    N::AssignStat => self.owner(statement.child(N::VarList)?.exprs().nth(rank)?),
                    N::LocalDef if rank == 0 => self.class_over(statement),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Where the name of an index or of a table field is written: its identifier, or the inside of the string
    /// that spells it as it is.
    fn name_range(&self, node: Node, name: &str) -> Option<(usize, usize)> {
        if let Some(id) = node.token(T::Id) {
            return Some((id.start(), id.end()));
        }
        let key = if node.is(N::TableField) { node.expr() } else { psi::key_expr(node) }?;
        let token = key.first_token().filter(|_| psi::is_string(key))?;
        let (start, end) = self.src.toks.get(self.src.tok_of(token.index()))?.content_range(&self.src.text);
        (self.src.text.get(start..end) == Some(name)).then_some((start, end))
    }

    fn global(&mut self, node: Node) {
        let Some(id) = node.token(T::Id).filter(|id| self.of_project(id.text())) else { return };
        self.push(Family::Global, self.global_key(id.text()), (id.start(), id.end()));
    }

    fn member(&mut self, node: Node) {
        let (Some(prefix), Some(name)) = (psi::prefix(node), psi::member_name(self.src, node)) else { return };
        let Some(range) = self.name_range(node, &name) else { return };
        if self.is_globals_table(prefix) {
            if self.of_project(&name) {
                self.push(Family::Global, self.global_key(&name), range);
            }
        } else if let Some(owner) = self.owner(prefix) {
            self.push(Family::Member, member_key(&owner, &name), range);
        }
    }

    fn method(&mut self, node: Node) {
        let name = node.child(N::ClassMethodName).and_then(|named| named.token(T::Id));
        let (Some(object), Some(name)) = (psi::method_owner(node), name) else { return };
        if let Some(owner) = self.owner(object) {
            self.push(Family::Member, member_key(&owner, name.text()), (name.start(), name.end()));
        }
    }

    fn field(&mut self, node: Node) {
        let Some(name) = psi::field_name(self.src, node) else { return };
        let table = node.parent().filter(|table| table.is(N::TableExpr));
        let (Some(range), Some(table)) = (self.name_range(node, &name), table) else { return };
        if let Some(owner) = self.table_owner(table) {
            self.push(Family::Member, member_key(&owner, &name), range);
        }
    }

    fn visit(&mut self, node: Node) {
        match node.kind() {
            N::NameExpr | N::FuncDef if !self.is_local(node) => self.global(node),
            N::IndexExpr => self.member(node),
            N::ClassMethodDef => self.method(node),
            N::TableField => self.field(node),
            _ => {}
        }
    }

    fn code(&mut self) {
        for node in self.src.root().descendants() {
            self.visit(node);
        }
    }
}

/// The anchors of a file, in the order they are written. `library` tells the names no project declares:
/// natives, the runtime, the standard library.
pub fn anchors(src: &Source, library: &dyn Fn(&str) -> bool) -> Vec<Anchor> {
    let mut reading = Reading { src, module: module_of(src), library, out: Vec::new() };
    reading.code();
    comments(src.docs(), &mut reading.out);
    in_order(reading.out)
}

/// The global or the member named at `at`, as `anchors` gives it, read from what stands around `at` only.
pub fn shared_at(src: &Source, at: usize, library: &dyn Fn(&str) -> bool) -> Option<Anchor> {
    let root = src.root();
    let tree = root.tree();
    let index = token_at(tree.tokens(), at, |kind| matches!(kind, T::Id | T::String))?;
    let named = tree.node(root.source(), tree.node_of_token(index));
    let mut reading = Reading { src, module: module_of(src), library, out: Vec::new() };
    // The name of a method stands two nodes under its statement, a string key two under its index.
    for node in std::iter::once(named).chain(named.ancestors()).take(3) {
        reading.visit(node);
    }
    reading.out.into_iter().find(|anchor| anchor.start <= at && at <= anchor.end)
}

/// The anchors the comments of a text give, without reading its code: what a project too big to give its
/// globals asks of each of its files.
pub fn of_comments(text: &str) -> Vec<Anchor> {
    let mut out = Vec::new();
    if text.contains('@') {
        comments(Tree::docs(text, &lex(text)).root(text), &mut out);
    }
    in_order(out)
}
