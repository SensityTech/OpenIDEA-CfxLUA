//! What one file declares: the stubs of the JetBrains plugin (`stubs/`), read from the tree. The classes a
//! member belongs to are those the file alone can tell.

use super::{AliasDef, ClassDef, Declared, EnumKind, Member, MemberKind};
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment, Visibility};
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::doc::Reader;
use crate::ty::expressions::table_type_name;
use crate::ty::{FileId, G, Ty, psi, resolve};
use std::rc::Rc;

/// The name `---@module Name` gives to a file, or else the name its first `module("name")` statement is
/// called with.
pub fn module_name(src: &Source) -> Option<Rc<str>> {
    if let Some(name) = src.docs().children_of(N::DocComment).find_map(|comment| Comment(comment).module_name()) {
        return Some(name.into());
    }
    for stat in src.root().children() {
        let call = stat.expr().filter(|call| stat.is(N::ExprStat) && call.is(N::CallExpr));
        let Some(call) = call.filter(|call| psi::prefix(*call).is_some_and(|callee| callee.code() == "module")) else {
            continue;
        };
        if let Some(name) = psi::first_string_arg(call).and_then(|arg| psi::string_value(src, arg)) {
            return Some(name.into());
        }
    }
    None
}

/// The table a class annotates: the first value of the statement its comment documents.
fn annotated_table<'a>(src: &'a Source, comment: Comment<'a>) -> Option<Node<'a>> {
    let at = comment.0.range().1;
    let after = src.root().tree().node_at(src.root().source(), at.saturating_sub(1));
    let owner = std::iter::once(after)
        .chain(after.ancestors())
        .find(|node| matches!(node.kind(), N::AssignStat | N::LocalDef) && src.comment_of(*node) == Some(comment))?;
    owner.child(N::ExprList).and_then(Node::expr).filter(|value| value.is(N::TableExpr))
}

struct Declaring<'a, 'c> {
    cx: &'a Cx<'c>,
    file: FileId,
    src: &'a Source,
    module: Rc<str>,
    out: Declared,
}

impl Declaring<'_, '_> {
    fn psi<'s>(&'s self, node: Node<'s>) -> Psi<'s> {
        Psi { file: self.file, src: self.src, node }
    }

    fn deprecated(&self, node: Node) -> bool {
        self.src.comment_of(node).is_some_and(Comment::is_deprecated)
    }

    fn visibility(&self, node: Node) -> Visibility {
        self.src.comment_of(node).map_or(Visibility::Public, |comment| tags::visibility(comment.0))
    }

    /// The type of what a node writes, read from the file alone.
    fn alone(&self, node: Node) -> Ty {
        self.cx.in_file_alone(|| self.cx.with_value(0, || self.cx.infer(self.psi(node))))
    }

    /// The type of a value that says it by the way it is written: a literal, a function, a table.
    fn written(&self, value: Option<Node>) -> Option<Ty> {
        let value = value.filter(|value| matches!(value.kind(), N::LiteralExpr | N::ClosureExpr | N::TableExpr))?;
        Some(self.alone(value))
    }

    /// The type of a target of an assignment, when nothing but the statement is needed to know it: its
    /// annotation, or a value that says its type. `class` tells whether a class the comment declares counts.
    fn assigned(&self, stat: Node, target: Node, class: bool) -> Option<Ty> {
        let reader = Reader::new(self.file);
        let comment = self.src.comment_of(stat);
        let annotated =
            comment.and_then(|comment| if class { reader.declared(comment) } else { reader.typed(comment) });
        let value = stat.child(N::ExprList).and_then(|values| values.exprs().nth(psi::rank(target)));
        match annotated {
            // A global keeps the table it is given next to the class that annotates it.
            Some(annotated)
                if class && (annotated.is_invalid() || value.is_some_and(|value| value.is(N::TableExpr))) =>
            {
                Some(annotated.union(&self.written(value)?))
            }
            Some(annotated) => Some(annotated),
            None => self.written(value),
        }
    }

    fn member(
        &mut self,
        owners: Vec<Rc<str>>,
        name: &str,
        kind: MemberKind,
        (node, documented): (Node, Node),
        ty: Option<Ty>,
    ) {
        self.out.members.push(Member {
            owners,
            name: name.into(),
            kind,
            node: node.id(),
            ty,
            is_static: kind == MemberKind::Method && psi::is_static(node),
            deprecated: self.deprecated(documented),
            visibility: self.visibility(documented),
        });
    }

    /// The names of the classes the file alone gives to an expression.
    fn classes(&self, expr: Node) -> Vec<Rc<str>> {
        let ty = self.alone(expr);
        let mut names: Vec<Rc<str>> = Vec::new();
        for class in ty.each().iter().filter_map(Ty::class_name) {
            if !names.iter().any(|name| **name == *class) {
                names.push(class.into());
            }
        }
        names
    }

    fn method(&mut self, node: Node) {
        let (Some(on), Some(name)) = (psi::method_owner(node), psi::function_name(node)) else { return };
        if node.child(N::FuncBody).is_none() {
            return;
        }
        let mut owners = self.classes(on);
        if owners.is_empty() {
            owners.push(on.code().into());
        }
        let ty = self.alone(node);
        self.member(owners, name, MemberKind::Method, (node, node), Some(ty));
    }

    fn assignment(&mut self, stat: Node) {
        for target in stat.child(N::VarList).into_iter().flat_map(Node::children) {
            match target.kind() {
                N::NameExpr if !resolve::is_local(self.psi(target)) => {
                    let module = self.module.clone();
                    let ty = self.assigned(stat, target, true);
                    self.member(vec![module], target.code(), MemberKind::Global, (target, stat), ty);
                }
                N::IndexExpr => {
                    let (Some(on), Some(name)) = (psi::prefix(target), psi::member_name(self.src, target)) else {
                        continue;
                    };
                    let owners = self.classes(on);
                    if !owners.is_empty() {
                        let ty = self.assigned(stat, target, false);
                        self.member(owners, &name, MemberKind::Field, (target, stat), ty);
                    }
                }
                _ => {}
            }
        }
    }

    fn class(&mut self, comment: Comment, tag: Node) {
        let Some(name) = tags::name(tag) else { return };
        let mut parents = tag.children_of(N::DocClassNameRef).map(|parent| Rc::from(parent.code()));
        let reader = Reader::new(self.file);
        let signatures = comment.tags(N::DocTagField).filter_map(|field| {
            let key = tags::key_ty(field)?;
            Some((reader.ty(key), tags::ty(field).map_or(Ty::Unknown, |value| reader.ty(value))))
        });
        let table = annotated_table(self.src, comment);
        let keyed = tag.child(N::DocClassAttr).is_some_and(|attr| attr.tokens().any(|id| id.text() == "key"));
        self.out.classes.push(ClassDef {
            name: name.into(),
            super_name: parents.next(),
            extra_supers: parents.collect(),
            alias: table.map(|table| table_type_name(self.file, table.code_range().0).into()),
            deprecated: comment.is_deprecated(),
            enum_kind: match (tag.has(T::DocTagEnum), keyed) {
                (false, _) => EnumKind::No,
                (true, false) => EnumKind::Values,
                (true, true) => EnumKind::Keys,
            },
            signatures: signatures.collect(),
            node: tag.id(),
        });
    }

    /// `---@field name T` declares a member of the class of its comment, or of the class it names.
    fn field(&mut self, comment: Comment, tag: Node) {
        let Some(name) = tags::name(tag) else { return };
        let named = tag.child(N::DocClassNameRef).map(Node::code);
        let class = named.or_else(|| comment.tag(N::DocTagClass).and_then(tags::name));
        let Some(class) = class else { return };
        self.out.members.push(Member {
            owners: vec![class.into()],
            name: name.into(),
            kind: MemberKind::DocField,
            node: tag.id(),
            ty: Some(Reader::new(self.file).tag(tag)),
            is_static: false,
            deprecated: false,
            visibility: tags::visibility(tag),
        });
    }

    fn comment(&mut self, comment: Comment) {
        for tag in comment.0.children() {
            match tag.kind() {
                N::DocTagClass => self.class(comment, tag),
                N::DocTagField => self.field(comment, tag),
                N::DocTagAlias => {
                    if let Some(name) = tags::name(tag) {
                        let ty = Reader::new(self.file).alias(tag);
                        self.out.aliases.push(AliasDef { name: name.into(), ty, node: tag.id() });
                    }
                }
                _ => {}
            }
        }
    }
}

/// What `src`, the file `file`, declares. `cx` only reads that file: nothing is looked up in the index.
pub fn declare(cx: &Cx, file: FileId, src: &Source) -> Declared {
    let module = module_name(src);
    let mut declaring = Declaring {
        cx,
        file,
        src,
        module: module.clone().unwrap_or_else(|| G.into()),
        out: Declared { module, ..Declared::default() },
    };
    for node in src.root().descendants() {
        match node.kind() {
            N::ClassMethodDef => declaring.method(node),
            N::FuncDef => {
                if let Some(name) = psi::function_name(node).filter(|_| node.child(N::FuncBody).is_some()) {
                    let (module, ty) = (declaring.module.clone(), declaring.alone(node));
                    declaring.member(vec![module], name, MemberKind::Function, (node, node), Some(ty));
                }
            }
            N::AssignStat => declaring.assignment(node),
            _ => {}
        }
    }
    for comment in src.docs().children_of(N::DocComment) {
        declaring.comment(Comment(comment));
    }
    declaring.out
}
