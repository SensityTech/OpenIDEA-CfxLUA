//! What a name or a field stands for inside its file (`LuaDeclarationTree`, `LuaPsiResolveUtil`).

use super::SELF;
use super::context::Psi;
use super::psi;
use crate::lua::Source;
use crate::lua::scope::DeclKind;
use crate::lua::tree::{N, Node};
use std::collections::HashMap;

/// What declares a name or a field: a declaration of the scopes, or the first assignment of a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Owner {
    Decl(u32),
    Field(u32),
}

/// The fields a file gives to its own names: `M.a = 1` and `function M.b() end` declare `a` and `b` on what
/// declares `M`, and so on for the fields of a field.
pub struct Fields {
    first: HashMap<(Owner, Box<str>), u32>,
}

impl Fields {
    pub fn new(src: &Source) -> Fields {
        let mut fields = Fields { first: HashMap::new() };
        for node in src.root().descendants() {
            match node.kind() {
                N::AssignStat => {
                    for target in node.child(N::VarList).into_iter().flat_map(Node::children) {
                        if target.is(N::IndexExpr) {
                            fields.declare(src, psi::prefix(target), psi::member_name(src, target).as_deref(), target);
                        }
                    }
                }
                N::ClassMethodDef => fields.declare(src, psi::method_owner(node), psi::function_name(node), node),
                _ => {}
            }
        }
        fields
    }

    fn declare(&mut self, src: &Source, on: Option<Node>, name: Option<&str>, by: Node) {
        let (Some(on), Some(name)) = (on, name) else { return };
        if let Some(owner) = self.owner(src, on) {
            self.first.entry((owner, name.into())).or_insert(by.id());
        }
    }

    fn owner(&self, src: &Source, expr: Node) -> Option<Owner> {
        match expr.kind() {
            N::NameExpr => src.scopes().id_of(expr).map(Owner::Decl),
            N::IndexExpr => {
                let on = self.owner(src, psi::prefix(expr)?)?;
                let name = psi::member_name(src, expr)?;
                self.first.get(&(on, name.as_ref().into())).map(|by| Owner::Field(*by))
            }
            _ => None,
        }
    }
}

/// The node that first declares what `expr`, a name or a field chain, stands for in its file: a local, a
/// parameter, a local function, the first assignment of a global or of a field, or a method.
pub fn declaration<'s>(src: &'s Source, expr: Node<'s>) -> Option<Node<'s>> {
    let node = match src.model(Fields::new).owner(src, expr)? {
        Owner::Decl(id) => src.scopes().decl(id).node,
        Owner::Field(node) => node,
    };
    Some(expr.tree().node(expr.source(), node))
}

/// The declaration a name or a field chain refers to in its file, when it is not that declaration itself.
pub fn in_file<'s>(src: &'s Source, expr: Node<'s>) -> Option<Node<'s>> {
    declaration(src, expr).filter(|declaration| *declaration != expr)
}

/// What `self` stands for where no local is named so: the expression the enclosing method is declared on.
pub fn self_owner(name: Node) -> Option<Node> {
    if name.code() != SELF {
        return None;
    }
    let method = name.ancestors().find(|node| node.is(N::ClassMethodDef)).filter(|method| !psi::is_static(*method))?;
    psi::method_owner(method)
}

/// Whether a name is one of its file's locals: a local variable, a parameter or a local function.
pub fn is_local(psi: Psi) -> bool {
    psi.src.scopes().resolve(psi.node).is_some_and(|decl| decl.kind != DeclKind::Global)
}
