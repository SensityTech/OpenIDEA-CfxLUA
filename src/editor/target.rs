//! What a name of a file stands for: the element the JetBrains platform documents and navigates to
//! (`TargetElementUtil`, `reference/`, `comment/reference/`, `LuaPsiResolveUtil`).

use crate::index::{Found, MemberKind};
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::scope::DeclKind;
use crate::lua::token::{T, Tokens};
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Elem, Psi};
use crate::ty::doc::Reader;
use crate::ty::members::{self, Member};
use crate::ty::{Detail, DocField, FileId, SELF, Ty, psi, resolve};

#[derive(Clone, Debug)]
pub enum Target {
    /// A declaration of the Lua tree of a file, or a tag of its documentation tree.
    Elem(Elem),
    /// A field of a table written in a comment, and the type of that table.
    Written(Ty, DocField),
}

impl Target {
    fn declared(found: Found) -> Target {
        let doc = found.member.kind == MemberKind::DocField;
        Target::Elem(Elem { file: found.file, node: found.member.node, doc })
    }

    fn member(member: Member, class: &Ty) -> Target {
        match member {
            Member::Declared(found) => Target::declared(found),
            Member::Field(field) => Target::Elem(Elem { file: field.file, node: field.node, doc: false }),
            Member::Written(field) => Target::Written(class.clone(), field),
        }
    }
}

/// A name of the document, and what it stands for.
#[derive(Clone, Debug)]
pub struct Named {
    /// Where the name is written.
    pub range: (usize, usize),
    pub target: Target,
}

/// The token of one of the kinds `wanted` that holds the byte `at`, or that ends there.
pub(super) fn token_at(tokens: &Tokens, at: usize, wanted: impl Fn(T) -> bool) -> Option<usize> {
    let holds = |index: &usize| wanted(tokens.kind(*index)) && tokens.start(*index) <= at && at <= tokens.end(*index);
    let before = at.checked_sub(1).and_then(|before| tokens.at(before));
    tokens.at(at).into_iter().chain(before).find(holds)
}

/// What the name written at `at` in `src`, the file `file`, stands for. Nothing is inferred before the token
/// there is known to be a name.
pub fn at(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Named> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let index = token_at(tokens, at, |kind| matches!(kind, T::Id | T::String | T::DocComment))?;
    if tokens.kind(index) == T::DocComment {
        return in_comment(cx, file, src, at);
    }
    let psi = Psi { file, src, node: tree.node(root.source(), tree.node_of_token(index)) };
    let target = if tokens.kind(index) == T::Id { named(cx, psi) } else { keyed(cx, psi) };
    Some(Named { range: (tokens.start(index), tokens.end(index)), target: target? })
}

/// What the identifier written directly in `psi.node` stands for.
fn named(cx: &Cx, psi: Psi) -> Option<Target> {
    let own = Target::Elem(psi.elem());
    Some(match psi.node.kind() {
        N::NameExpr => name(cx, psi),
        N::IndexExpr => index(cx, psi)?,
        N::NameDef | N::ParamNameDef | N::LocalFuncDef | N::TableField => own,
        // `local f` then `function f() end`: the function is the local.
        N::FuncDef => match psi.src.scopes().resolve(psi.node).filter(|decl| decl.kind != DeclKind::Global) {
            Some(local) => Target::Elem(Elem { file: psi.file, node: local.node, doc: false }),
            None => own,
        },
        N::ClassMethodName => Target::Elem(psi.at(psi.node.parent()?).elem()),
        _ => return None,
    })
}

/// What a string stands for where it names a member: `t['name']` or `{ ['name'] = v }`.
fn keyed(cx: &Cx, psi: Psi) -> Option<Target> {
    let (literal, parent) = (psi.node, psi.node.parent()?);
    if !psi::is_string(literal) {
        return None;
    }
    match parent.kind() {
        N::IndexExpr if psi::key_expr(parent) == Some(literal) => index(cx, psi.at(parent)),
        N::TableField if parent.has(T::LBrack) && parent.expr() == Some(literal) => {
            Some(Target::Elem(psi.at(parent).elem()))
        }
        _ => None,
    }
}

/// What a name stands for (`LuaNameReference`): the local in sight, or else the first declaration the project
/// knows of a global of that name, or else the name itself.
fn name(cx: &Cx, psi: Psi) -> Target {
    let in_file = match psi.src.scopes().resolve(psi.node) {
        Some(decl) => Some(psi.node.tree().node(psi.node.source(), decl.node)),
        None => match resolve::self_owner(psi.node) {
            Some(owner) if owner.is(N::NameExpr) && owner.code() != SELF => return name(cx, psi.at(owner)),
            owner => owner,
        },
    };
    let global = match in_file {
        None => Some(psi.node),
        Some(first) => Some(first).filter(|first| first.is(N::NameExpr)),
    };
    let known = global.and_then(|global| members::globals(cx, psi, global.code()).into_iter().next());
    match known {
        Some(found) => Target::declared(found),
        None => Target::Elem(psi.at(in_file.unwrap_or(psi.node)).elem()),
    }
}

/// What the member an index names stands for (`LuaIndexReference`): among the classes of its object, the
/// member of that name that weighs most, the first one among equals; or else what first declares that field
/// in the file, or else the index itself.
fn index(cx: &Cx, psi: Psi) -> Option<Target> {
    let name = psi::member_name(psi.src, psi.node)?;
    let object = psi::prefix(psi.node).map_or(Ty::Unknown, |object| cx.with_value(0, || cx.infer(psi.at(object))));
    let mut best: Option<(u32, Target)> = None;
    for class in object.top_classes() {
        let Some(member) = members::find(cx, class, &name) else { continue };
        let worth = member.worth();
        if best.as_ref().is_none_or(|(kept, _)| worth > *kept) {
            best = Some((worth, Target::member(member, class)));
        }
    }
    let declared = || Target::Elem(psi.at(resolve::declaration(psi.src, psi.node).unwrap_or(psi.node)).elem());
    Some(best.map_or_else(declared, |(_, target)| target))
}

fn in_comment(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Named> {
    let docs = src.docs();
    let (tree, tokens) = (docs.tree(), docs.tree().tokens());
    let index = token_at(tokens, at, |kind| kind == T::Id)?;
    let node = tree.node(docs.source(), tree.node_of_token(index));
    let tag = Target::Elem(Elem { file, node: node.id(), doc: true });
    let target = match node.kind() {
        N::DocClassNameRef => type_named(cx, node)?,
        N::DocParamNameRef => Target::Elem(Elem { file, node: documented_param(src, node)?.id(), doc: false }),
        N::DocTagClass | N::DocTagField | N::DocTagAlias => tag,
        N::DocTagSee => seen(cx, node)?,
        N::DocTableField => written(file, node)?,
        _ => return None,
    };
    Some(Named { range: (tokens.start(index), tokens.end(index)), target })
}

/// The class or the alias a type names (`LuaClassNameReference`); a generic parameter is neither.
fn type_named(cx: &Cx, reference: Node) -> Option<Target> {
    let name = reference.code().trim_matches('`');
    let comment = Comment(reference.ancestor(N::DocComment)?);
    if comment.all(N::DocGenericDef).any(|generic| tags::name(generic) == Some(name)) {
        return None;
    }
    let class = cx.index.class_in(name).map(|(file, class)| (file, class.node));
    let (file, node) = class.or_else(|| cx.index.alias_in(name).map(|(file, alias)| (file, alias.node)))?;
    Some(Target::Elem(Elem { file, node, doc: true }))
}

/// The parameter a `@param name` documents (`LuaDocParamNameReference`): the one of that name of the first
/// function, or loop, of the statement the comment is bound to.
fn documented_param<'a>(src: &'a Source, reference: Node) -> Option<Node<'a>> {
    let owner = src.documented_by(Comment(reference.ancestor(N::DocComment)?))?;
    let name = reference.code();
    let named = |node: Node<'a>| {
        let params: Vec<Node<'a>> = match node.kind() {
            N::ForAStat | N::ForBStat => node.children_of(N::ParamNameDef).collect(),
            _ if psi::is_function(node) => psi::params(node).collect(),
            _ => Vec::new(),
        };
        params.into_iter().find(|param| param.code() == name)
    };
    std::iter::once(owner).chain(owner.descendants()).find_map(named)
}

/// The member a `@see Class#member` names (`LuaDocSeeReference`).
fn seen(cx: &Cx, tag: Node) -> Option<Target> {
    let class = Ty::class(tag.child(N::DocClassNameRef)?.code());
    members::find(cx, &class, tags::name(tag)?).map(|member| Target::member(member, &class))
}

/// A field of a table written in a type: `{ name: T }`.
fn written(file: FileId, field: Node) -> Option<Target> {
    let table = field.parent().filter(|def| def.is(N::DocTableDef))?.parent().filter(|ty| ty.is(N::DocTableTy))?;
    let owner = Reader::new(file).ty(table);
    let Ty::Class(class) = &owner else { return None };
    let Detail::Table(fields) = &class.detail else { return None };
    let name = tags::name(field)?;
    let found = fields.iter().find(|field| &*field.name == name)?.clone();
    Some(Target::Written(owner.clone(), found))
}
