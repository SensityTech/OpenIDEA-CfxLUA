//! The type of what a declaration declares (`Declarations.kt`).

use super::context::{Cx, Psi};
use super::doc::Reader;
use super::expressions::{self, infer_expr, unpacked, value_at, values};
use super::{
    ANONYMOUS, FileId, FuncRef, Function, GLOBAL, Param, Ret, SELF, SELF_FUNCTION, Signature, Ty, TyParam, casts,
    members, psi,
};
use crate::lua::doc::tags::{self, Comment};
use crate::lua::tree::{N, Node};

/// `<offset>@<file>`, the class made for a local variable or a parameter.
pub fn anonymous_type_name(file: FileId, offset: usize) -> String {
    format!("{offset}@{file}")
}

fn anonymous(def: Psi) -> Ty {
    Ty::flagged(&anonymous_type_name(def.file, def.start()), def.node.code(), ANONYMOUS)
}

impl Cx<'_> {
    /// The type of an expression or of what a declaration declares.
    pub fn infer(&self, psi: Psi) -> Ty {
        self.cached(psi.elem(), || match psi.node.kind() {
            _ if psi::is_function(psi.node) => function(self, psi),
            N::NameExpr => match casts::cast(self, psi) {
                cast if cast.is_invalid() => infer_expr(self, psi),
                cast => cast,
            },
            kind if kind.is_expr() => infer_expr(self, psi),
            N::ParamNameDef => param(self, psi),
            N::NameDef => name_def(self, psi),
            N::TableField => table_field(self, psi),
            N::File => file(self, psi),
            _ => Ty::Unknown,
        })
    }

    /// What a signature returns: for a function of the code, what its body returns.
    pub fn ret(&self, ret: &Ret) -> Ty {
        match ret {
            Ret::Ty(ty) => ty.clone(),
            Ret::Body(func) => body_return(self, *func),
        }
    }
}

/// The statement or the table field whose comment documents `node`: itself, or the nearest one around it.
fn documented(node: Node) -> Option<Node> {
    std::iter::once(node).chain(node.ancestors()).find(|owner| owner.kind().is_statement() || owner.is(N::TableField))
}

fn comment(psi: Psi) -> Option<Comment> {
    documented(psi.node).and_then(|owner| psi.src.comment_of(owner))
}

/// The comment of a function statement; a function written as an expression has none of its own.
fn own_comment(owner: Psi) -> Option<Comment> {
    if owner.node.is(N::ClosureExpr) { None } else { owner.src.comment_of(owner.node) }
}

/// The type of the extra arguments of a function that takes some: `@vararg T`, or `@param ... T`.
pub fn vararg_type(owner: Psi) -> Option<Ty> {
    if !psi::has_varargs(owner.node) {
        return None;
    }
    let documented = own_comment(owner).and_then(|comment| Reader::new(owner.file).vararg(comment));
    Some(documented.unwrap_or(Ty::Unknown))
}

/// The parameters a function declares, typed by the comment that documents it.
fn params(cx: &Cx, owner: Psi) -> Vec<Param> {
    let comment = comment(owner);
    let reader = Reader { bounded_generics: cx.options.bounded_generics, ..Reader::new(owner.file) };
    psi::params(owner.node)
        .map(|param| {
            let name = param.code();
            let tag = comment.and_then(|comment| Some((comment.param(name)?, comment)));
            Param {
                name: name.into(),
                ty: tag.map_or(Ty::Unknown, |(tag, comment)| reader.param(tag, comment)),
                optional: tag.is_some_and(|(tag, _)| tags::is_optional(tag)),
            }
        })
        .collect()
}

fn generics(owner: Psi) -> Vec<TyParam> {
    let comment = own_comment(owner);
    comment.map_or(Vec::new(), |comment| Reader::new(owner.file).generics(comment.all(N::DocGenericDef)))
}

/// The type of a function of the code. What it returns is its `@return`, or else is read from its body
/// when asked for.
pub fn function(cx: &Cx, owner: Psi) -> Ty {
    let method = owner.node.is(N::ClassMethodDef) && !psi::is_static(owner.node);
    let comment = own_comment(owner);
    let reader = Reader::new(owner.file);
    let documented = comment.and_then(|comment| reader.returns(comment));
    let main = Signature {
        colon_call: method,
        params: params(cx, owner),
        ret: documented.map_or(Ret::Body(FuncRef { file: owner.file, at: owner.start() as u32 }), Ret::Ty),
        vararg: vararg_type(owner),
        generics: generics(owner),
    };
    let overloads = comment.map_or(Vec::new(), |comment| reader.overloads(comment, method));
    let flags = if owner.node.is(N::FuncDef) { GLOBAL } else { 0 } | if method { SELF_FUNCTION } else { 0 };
    Ty::function(Function { main, overloads, flags })
}

/// The function of the code that starts at `func.at`.
pub fn function_at<R>(cx: &Cx, func: FuncRef, read: impl FnOnce(Psi) -> R) -> Option<R> {
    let src = cx.source(func.file)?;
    let inner = src.root().tree().node_at(src.root().source(), func.at as usize);
    let starts = |node: &Node| psi::is_function(*node) && node.code_range().0 == func.at as usize;
    let owner = std::iter::once(inner).chain(inner.ancestors()).find(starts)?;
    Some(read(Psi { file: func.file, src: &src, node: owner }))
}

/// The `return` statements of a function: those of its body and of the blocks nested in it, not those of
/// the functions it declares.
fn returns(owner: Node) -> Vec<Node> {
    let mut found = Vec::new();
    let mut blocks: Vec<Node> = owner.child(N::FuncBody).and_then(|body| body.child(N::Block)).into_iter().collect();
    while let Some(block) = blocks.pop() {
        for stat in block.children() {
            match stat.kind() {
                N::ReturnStat => found.push(stat),
                N::DoStat | N::WhileStat | N::RepeatStat | N::IfStat | N::ForAStat | N::ForBStat => {
                    blocks.extend(stat.children_of(N::Block));
                }
                _ => {}
            }
        }
    }
    found.sort_by_key(|stat| stat.id());
    found
}

/// What the body of a function returns: the union of what its `return` statements give, each one a type
/// or a tuple; void without any.
fn body_return(cx: &Cx, func: FuncRef) -> Ty {
    let read = |owner: Psi| {
        cx.guarded(owner.elem(), None, || {
            let mut ty = Ty::Void;
            for stat in returns(owner.node) {
                let given = stat.child(N::ExprList).map_or(Ty::Unknown, |list| values(cx, owner.at(list)));
                for member in given.each() {
                    ty.add(member);
                }
            }
            ty
        })
    };
    let returns_itself =
        |ty: &Ty| matches!(ty, Ty::Function(function) if matches!(function.main.ret, Ret::Body(own) if own == func));
    cx.with_value(-1, || {
        cx.returned(func, || function_at(cx, func, read).filter(|ty| !returns_itself(ty)).unwrap_or(Ty::Unknown))
    })
}

fn name_def(cx: &Cx, def: Psi) -> Ty {
    let Some(local) = def.node.ancestors().find(|node| node.is(N::LocalDef)) else { return Ty::Unknown };
    let annotated = def.src.comment_of(local).and_then(|comment| Reader::new(def.file).declared(comment));
    if let Some(annotated) = annotated {
        return annotated;
    }
    let mut ty = match local.child(N::ExprList) {
        Some(list) => value_at(cx, def.at(list), psi::rank(def.node) as i32),
        None => unpacked(cx, def, psi::in_source(local), Some(def.node.code())),
    };
    if !matches!(ty, Ty::Primitive(_)) || ty == Ty::TABLE {
        ty.add(&anonymous(def));
    }
    ty
}

fn table_field(cx: &Cx, field: Psi) -> Ty {
    let annotated = field.src.comment_of(field.node).and_then(|comment| Reader::new(field.file).typed(comment));
    if let Some(annotated) = annotated {
        return annotated;
    }
    if psi::is_set_field(field.node) {
        return Ty::BOOLEAN;
    }
    psi::field_value(field.node).map_or(Ty::Unknown, |value| cx.with_value(0, || cx.infer(field.at(value))))
}

/// What a file gives to the one that requires it: the class of its module, or what its last `return` gives.
fn file(cx: &Cx, root: Psi) -> Ty {
    let inferred = cx.once(root.elem(), || {
        if let Some(module) = members::module_of(root.src) {
            return Ty::class(&module);
        }
        let last = root.node.children_of(N::ReturnStat).last();
        let list = last.and_then(|stat| stat.child(N::ExprList));
        list.map_or(Ty::Unknown, |list| value_at(cx, root.at(list), 0))
    });
    inferred.unwrap_or(Ty::Unknown)
}

/// The function, or the loop, a parameter belongs to.
fn param_owner(param: Node) -> Option<Node> {
    let parent = param.parent()?;
    if parent.is(N::FuncBody) { parent.parent() } else { Some(parent) }
}

fn param(cx: &Cx, def: Psi) -> Ty {
    let ty = param_type(cx, def);
    if ty.is_invalid() { anonymous(def) } else { ty }
}

fn param_type(cx: &Cx, def: Psi) -> Ty {
    let name = def.node.code();
    let Some(owner) = param_owner(def.node) else { return Ty::Unknown };
    let reader = Reader { bounded_generics: cx.options.bounded_generics, ..Reader::new(def.file) };
    if let Some(comment) = comment(def.at(owner))
        && let Some(tag) = comment.param(name)
    {
        return reader.param(tag, comment);
    }
    match owner.kind() {
        N::ClassMethodDef => inherited_param(cx, def.at(owner), name),
        N::FuncDef if name == SELF => members::module_of(def.src).map_or(Ty::Unknown, |module| Ty::class(&module)),
        N::ForBStat => iterated(cx, def, owner),
        N::ForAStat => Ty::NUMBER,
        N::ClosureExpr => {
            let rank = psi::rank(def.node);
            let mut ty = Ty::Unknown;
            for expected in should_be(cx, def.at(owner)).each() {
                if let Ty::Function(function) = expected {
                    ty.add(&function.main.param_ty(rank));
                }
            }
            ty
        }
        _ => Ty::Unknown,
    }
}

/// The type a parent class gives to the parameter `name` of the method it declares under the same name.
fn inherited_param(cx: &Cx, method: Psi, name: &str) -> Ty {
    let (Some(on), Some(method_name)) = (psi::method_owner(method.node), psi::function_name(method.node)) else {
        return Ty::Unknown;
    };
    let owner = cx.with_value(0, || cx.infer(method.at(on)));
    let Some(class) = owner.perfect_class() else { return Ty::Unknown };
    for parent in members::supers(cx, class) {
        let Some(inherited) = members::find(cx, &Ty::class(&parent), method_name) else { continue };
        if !inherited.is_function() {
            continue;
        }
        for ty in members::type_of(cx, &inherited).each() {
            let Ty::Function(function) = ty else { continue };
            if let Some(param) =
                function.main.params.iter().find(|param| &*param.name == name && param.ty != Ty::Unknown)
            {
                return param.ty.clone();
            }
        }
    }
    Ty::Unknown
}

/// The type of a variable of `for a, b in iterator(...)`: what the function the call gives returns, at the
/// rank of the variable.
fn iterated(cx: &Cx, def: Psi, stat: Node) -> Ty {
    let list = stat.child(N::ExprList);
    let call = list.and_then(|list| list.descendants().find(|node| node.is(N::CallExpr)));
    let Some(call) = call else { return Ty::Unknown };
    let rank = psi::rank(def.node);
    let mut result = Ty::Unknown;
    for ty in cx.with_value(0, || cx.infer(def.at(call))).each() {
        let Ty::Function(function) = ty else { continue };
        match cx.ret(&function.main.ret) {
            Ty::Tuple(list) => result.add(list.get(rank).unwrap_or(&Ty::Unknown)),
            returned if rank == 0 => result.add(&returned),
            _ => {}
        }
    }
    result
}

/// The type the place of an expression asks for (`shouldBe`): the type of what it is assigned to, of the
/// parameter it is given to, or of the field of an expected class it is the value of.
pub fn should_be(cx: &Cx, expr: Psi) -> Ty {
    members::resolve_aliases(cx, &expected(cx, expr))
}

fn expected(cx: &Cx, expr: Psi) -> Ty {
    let Some(parent) = expr.node.parent() else { return Ty::Unknown };
    let single = |node: Node| cx.with_value(0, || cx.infer(expr.at(node)));
    match parent.kind() {
        N::ExprList => {
            let Some(stat) = parent.parent() else { return Ty::Unknown };
            let receiver = match stat.kind() {
                N::AssignStat => stat.child(N::VarList).and_then(Node::expr),
                N::LocalDef => stat.child(N::NameList).and_then(|names| names.child(N::NameDef)),
                _ => None,
            };
            receiver.map_or(Ty::Unknown, single)
        }
        N::ListArgs | N::SingleArg => {
            let Some(call) = parent.parent().filter(|call| call.is(N::CallExpr)) else { return Ty::Unknown };
            let rank = if parent.is(N::SingleArg) { 0 } else { psi::rank(expr.node) };
            let call = expr.at(call);
            let colon = psi::is_colon_call(call.node);
            let mut ty = Ty::Unknown;
            for callee in psi::prefix(call.node).map_or(Ty::Unknown, single).each() {
                let Ty::Function(function) = callee else { continue };
                for signature in function.signatures() {
                    let args = expressions::solved(cx, call, signature).args(None, colon);
                    ty.add(args.get(rank).map_or(&Ty::Unknown, |param| &param.ty));
                }
            }
            ty
        }
        N::TableField => {
            let (Some(name), Some(table)) = (psi::field_name(expr.src, parent), parent.parent()) else {
                return Ty::Unknown;
            };
            let mut ty = Ty::Unknown;
            for class in should_be(cx, expr.at(table)).top_classes() {
                if let Some(member) = members::find(cx, class, &name) {
                    ty.add(&members::type_of(cx, &member));
                }
            }
            ty
        }
        _ => Ty::Unknown,
    }
}
