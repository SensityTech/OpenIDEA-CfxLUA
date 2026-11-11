//! The types a documentation comment writes (`LuaDocPsiImplUtil`).

use super::{Class, Detail, DocField, ELLIPSIS, FileId, Function, Param, Ret, Signature, Ty, TyParam, doc_table_name};
use crate::lua::doc::tags::{self, Comment};
use crate::lua::tree::{N, Node};
use std::rc::Rc;

/// Reads the types of the comments of one file.
#[derive(Clone, Copy, Debug)]
pub struct Reader {
    /// The file, which names the tables written in its comments.
    pub file: FileId,
    /// Whether a parameter typed with a bounded generic, `T : Base`, has the members of `Base`.
    pub bounded_generics: bool,
}

impl Reader {
    pub fn new(file: FileId) -> Reader {
        Reader { file, bounded_generics: false }
    }

    fn inner(&self, node: Node) -> Option<Ty> {
        tags::types(node).next().map(|inner| self.ty(inner))
    }

    fn union(&self, types: impl Iterator<Item = Ty>) -> Ty {
        types.fold(Ty::Unknown, |all, ty| all.union(&ty))
    }

    /// The type a type node writes.
    pub fn ty(&self, node: Node) -> Ty {
        match node.kind() {
            N::DocGeneralTy => Ty::named(node.code().trim_matches('`')),
            N::DocArrTy => Ty::array(self.inner(node).unwrap_or(Ty::Unknown)),
            N::DocNullableTy => self.inner(node).unwrap_or(Ty::Unknown).union(&Ty::Nil),
            N::DocParTy => self.inner(node).unwrap_or(Ty::Unknown),
            N::DocUnionTy => self.union(tags::types(node).map(|member| self.ty(member))),
            N::DocTupleTy => Ty::array(self.union(tags::types(node).map(|member| self.ty(member)))),
            N::DocNumberLiteralTy => Ty::NUMBER,
            N::DocStringLiteralTy => {
                let text = node.code();
                Ty::StringLiteral(if text.len() >= 2 { text[1..text.len() - 1].into() } else { "".into() })
            }
            N::DocGenericTy => {
                let mut types = tags::types(node).map(|part| self.ty(part));
                let base = types.next().unwrap_or(Ty::Unknown);
                Ty::generic(base, types.collect())
            }
            N::DocFunctionTy => Ty::function(Function::new(self.signature(node, false))),
            N::DocTableTy => node.child(N::DocTableDef).map_or(Ty::Unknown, |def| self.table(def)),
            _ => Ty::Unknown,
        }
    }

    /// `{ [K]: V }` is `table<K, V>`; any other table is a class of its own, with the fields it lists.
    fn table(&self, def: Node) -> Ty {
        let mut index = def.children_of(N::DocTableIndexField);
        if def.child(N::DocTableField).is_none()
            && let (Some(only), None) = (index.next(), index.next())
        {
            let part = |part: Option<Node>| part.map_or(Ty::Unknown, |ty| self.ty(ty));
            return Ty::generic(Ty::TABLE, vec![part(tags::key_ty(only)), part(tags::ty(only))]);
        }
        let fields: Vec<DocField> = def
            .children_of(N::DocTableField)
            .filter_map(|field| {
                Some(DocField {
                    name: tags::name(field)?.into(),
                    ty: tags::ty(field).map_or(Ty::Unknown, |ty| self.ty(ty)),
                    optional: tags::is_optional(field),
                })
            })
            .collect();
        let name: Rc<str> = doc_table_name(self.file, def.code_range().0).into();
        Ty::Class(Rc::new(Class { var_name: name.clone(), name, flags: 0, detail: Detail::Table(fields.into()) }))
    }

    /// What a type list gives: nothing, one type, or a tuple.
    pub fn values(&self, list: Option<Node>) -> Ty {
        let mut types: Vec<Ty> = list.into_iter().flat_map(tags::types).map(|ty| self.ty(ty)).collect();
        match types.len() {
            0 => Ty::Void,
            1 => types.remove(0),
            _ => Ty::tuple(types),
        }
    }

    pub fn generics<'a>(&self, defs: impl Iterator<Item = Node<'a>>) -> Vec<TyParam> {
        defs.filter_map(|def| {
            let base = def.child(N::DocClassNameRef).map(|base| base.code().into());
            Some(TyParam { name: tags::name(def)?.into(), base })
        })
        .collect()
    }

    /// The signature a `fun(a: T, ...: U): R` writes. `...: U` types the extra arguments: it is no parameter.
    pub fn signature(&self, function: Node, colon_call: bool) -> Signature {
        let typed = |node: Node| tags::ty(node).map_or(Ty::Unknown, |ty| self.ty(ty));
        let is_rest = |param: &Node| tags::name(*param) == Some(ELLIPSIS);
        let params = function
            .children_of(N::DocFunctionParam)
            .filter(|param| !is_rest(param))
            .filter_map(|param| {
                Some(Param { name: tags::name(param)?.into(), ty: typed(param), optional: tags::is_optional(param) })
            })
            .collect();
        let vararg = function
            .child(N::DocVarargParam)
            .or_else(|| function.children_of(N::DocFunctionParam).filter(is_rest).last())
            .map(typed);
        Signature {
            colon_call,
            params,
            ret: Ret::Ty(self.values(function.child(N::DocTypeList))),
            vararg,
            generics: self.generics(function.children_of(N::DocGenericDef)),
        }
    }

    /// The type of `@type T`, `@vararg T` and of any tag that holds one type.
    pub fn tag(&self, tag: Node) -> Ty {
        tags::ty(tag).map_or(Ty::Unknown, |ty| self.ty(ty))
    }

    /// The type of `@param name T`, the values of its `---| value` lines included.
    pub fn param(&self, tag: Node, comment: Comment) -> Ty {
        let members = tag.children_of(N::DocAliasMember).filter_map(tags::ty);
        let mut types = tags::ty(tag).into_iter().chain(members).map(|ty| self.ty(ty));
        let Some(first) = types.next() else { return Ty::Unknown };
        let ty = types.fold(first, |all, ty| all.union(&ty));
        if !self.bounded_generics {
            return ty;
        }
        let bounded = self.generics(comment.all(N::DocGenericDef));
        ty.substitute(&mut |class| {
            let generic = bounded.iter().find(|generic| generic.name == class.name)?;
            Some(Ty::param(&class.name, Some(generic.base.clone()?)))
        })
    }

    /// The type of `@alias Name T`, the values of its `---| value` lines included.
    pub fn alias(&self, tag: Node) -> Ty {
        let members = tag.children_of(N::DocAliasMember).filter_map(tags::ty);
        self.union(std::iter::once(self.tag(tag)).chain(members.map(|ty| self.ty(ty))))
    }

    /// What one `@return A, B` gives.
    pub fn returned(&self, tag: Node) -> Ty {
        self.values(tag.child(N::DocTypeList))
    }

    /// What the function a comment documents returns: its `@return` tag, or a tuple of the values of
    /// several ones; `None` without a tag.
    pub fn returns(&self, comment: Comment) -> Option<Ty> {
        let mut tags = comment.tags(N::DocTagReturn);
        let first = self.returned(tags.next()?);
        let Some(second) = tags.next() else { return Some(first) };
        let mut values = Vec::new();
        for ty in [first, self.returned(second)].into_iter().chain(tags.map(|tag| self.returned(tag))) {
            match ty {
                Ty::Tuple(list) => values.extend(list.iter().cloned()),
                Ty::Void => {}
                other => values.push(other),
            }
        }
        Some(Ty::tuple(values))
    }

    /// The class a `@class Name` declares.
    pub fn class(&self, tag: Node) -> Option<Ty> {
        tags::name(tag).map(Ty::class)
    }

    /// The type a comment gives to what it documents: the class it declares, or else its `@type`.
    pub fn declared(&self, comment: Comment) -> Option<Ty> {
        let class = comment.tag(N::DocTagClass).and_then(|tag| self.class(tag));
        class.or_else(|| self.typed(comment))
    }

    /// The type of the `@type` tag of a comment.
    pub fn typed(&self, comment: Comment) -> Option<Ty> {
        comment.tag(N::DocTagType).map(|tag| self.tag(tag))
    }

    /// The signatures of the `@overload` tags of a comment.
    pub fn overloads(&self, comment: Comment, colon_call: bool) -> Vec<Signature> {
        comment
            .all(N::DocTagOverload)
            .filter_map(|tag| tag.child(N::DocFunctionTy))
            .map(|function| self.signature(function, colon_call))
            .collect()
    }

    /// The type of the extra arguments: `@vararg T`, or `@param ... T`.
    pub fn vararg(&self, comment: Comment) -> Option<Ty> {
        let tag = comment.tag(N::DocTagVararg).map(|tag| self.tag(tag));
        tag.or_else(|| comment.param(ELLIPSIS).map(|tag| self.param(tag, comment)))
    }
}
