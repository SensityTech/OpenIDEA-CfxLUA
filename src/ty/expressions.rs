//! The type of an expression (`Expressions.kt`).

use super::context::{Cx, Psi};
use super::declarations::vararg_type;
use super::doc::Reader;
use super::psi::{self, Literal};
use super::{
    ANONYMOUS, ANONYMOUS_TABLE, Bindings, Class, FileId, G, GLOBAL, Ret, SELF, Signature, Ty, analyze,
    global_type_name, members, resolve,
};
use crate::lua::token::T;
use crate::lua::tree::{N, Node};

/// `<file>@(<offset>)table`, the class of a table constructor.
pub fn table_type_name(file: FileId, offset: usize) -> String {
    format!("{file}@({offset})table")
}

/// The class of the table `psi` writes.
pub fn table_type(psi: Psi) -> Ty {
    Ty::flagged(&table_type_name(psi.file, psi.start()), "", ANONYMOUS | ANONYMOUS_TABLE)
}

/// The classes the global `name` stands for: what is assigned to its fields and, unless only what the file
/// declares is wanted, the class of the same name.
pub fn global_type(cx: &Cx, name: &str, stored: bool) -> Ty {
    let fields = Ty::flagged(&global_type_name(name), name, GLOBAL);
    if !stored && cx.options.global_name_as_type { Ty::flagged(name, name, GLOBAL).union(&fields) } else { fields }
}

pub fn infer_expr(cx: &Cx, psi: Psi) -> Ty {
    if matches!(psi.node.kind(), N::IndexExpr | N::NameExpr)
        && let Some(declaration) = resolve::in_file(psi.src, psi.node)
    {
        return cx.infer(psi.at(declaration));
    }
    match psi.node.kind() {
        N::UnaryExpr => unary(cx, psi),
        N::BinaryExpr => binary(cx, psi),
        N::CallExpr => call(cx, psi),
        N::TableExpr => table(psi),
        N::ParenExpr => single(cx, psi, psi.node.expr()),
        N::NameExpr => name(cx, psi),
        N::LiteralExpr => literal(psi),
        N::IndexExpr => index(cx, psi),
        _ => Ty::Unknown,
    }
}

/// The one value an operand, a prefix or an argument stands for.
fn single(cx: &Cx, psi: Psi, expr: Option<Node>) -> Ty {
    expr.map_or(Ty::Unknown, |expr| cx.with_value(0, || cx.infer(psi.at(expr))))
}

fn operator(node: Node, kind: N) -> Option<T> {
    node.child(kind).and_then(|operator| operator.first_token()).map(|token| token.kind())
}

fn unary(cx: &Cx, psi: Psi) -> Ty {
    match operator(psi.node, N::UnaryOp) {
        Some(T::Minus) => single(cx, psi, psi.node.expr()),
        Some(T::GetN) => Ty::NUMBER,
        _ => Ty::Unknown,
    }
}

fn binary(cx: &Cx, psi: Psi) -> Ty {
    let mut operands = psi.node.exprs();
    let (left, right) = (operands.next(), operands.next());
    match operator(psi.node, N::BinaryOp) {
        Some(T::Concat) => Ty::STRING,
        Some(T::Le | T::Eq | T::Lt | T::Ne | T::Ge | T::Gt) => Ty::BOOLEAN,
        Some(T::And) => single(cx, psi, right),
        Some(T::Or) => single(cx, psi, left).union(&single(cx, psi, right)),
        Some(
            T::BitAnd
            | T::BitLtLt
            | T::BitOr
            | T::BitRtRt
            | T::BitTilde
            | T::Exp
            | T::Plus
            | T::Minus
            | T::Mult
            | T::Div
            | T::DoubleDiv
            | T::Mod,
        ) => single(cx, psi, left),
        _ => Ty::Unknown,
    }
}

/// What each generic parameter of `signature` stands for in the call `call`, read from its arguments.
pub fn bindings(cx: &Cx, call: Psi, signature: &Signature) -> Option<Bindings> {
    if !signature.is_generic() {
        return None;
    }
    let mut given: Vec<Ty> = Vec::new();
    if psi::is_colon_call(call.node) {
        given.extend(psi::receiver(call.node).map(|receiver| single(cx, call, Some(receiver))));
    }
    given.extend(psi::args(call.node).into_iter().map(|arg| single(cx, call, Some(arg))));
    let mut bound: Bindings = signature.generics.iter().map(|generic| (generic.name.clone(), Ty::Unknown)).collect();
    let ret = |ret: &Ret| cx.ret(ret);
    let first = usize::from(signature.colon_call);
    for (rank, param) in signature.params.iter().enumerate() {
        if let Some(arg) = given.get(first + rank) {
            analyze(arg, &param.ty, &mut bound, &ret);
        }
    }
    let rest = if signature.params.is_empty() { 0 } else { first + signature.params.len() };
    if let (Some(vararg), Some(arg)) = (&signature.vararg, given.get(rest)) {
        analyze(arg, vararg, &mut bound, &ret);
    }
    for generic in &signature.generics {
        let Some(base) = &generic.base else { continue };
        if let Some((_, ty)) = bound.iter_mut().find(|(name, ty)| *name == generic.name && ty.is_invalid()) {
            *ty = Ty::named(base);
        }
    }
    Some(bound)
}

/// What a class that names a generic parameter stands for.
fn bound_to(bound: &Bindings, class: &Class) -> Option<Ty> {
    bound.iter().find(|(name, _)| *name == class.name).map(|(_, ty)| ty.clone())
}

/// The signature with its generic parameters replaced by what they stand for in the call.
pub fn solved(cx: &Cx, call: Psi, signature: &Signature) -> Signature {
    match bindings(cx, call, signature) {
        Some(bound) => signature.substitute(&mut |class| bound_to(&bound, class)),
        None => signature.clone(),
    }
}

/// What the call gives through one signature of its function, at the value rank asked for.
fn returned(cx: &Cx, call: Psi, signature: &Signature) -> Option<Ty> {
    let mut ty = cx.ret(&signature.ret);
    if let Some(bound) = bindings(cx, call, signature) {
        ty = ty.substitute(&mut |class| bound_to(&bound, class));
    }
    let mut receiver: Option<Ty> = None;
    ty = ty.substitute(&mut |class| {
        (&*class.name == SELF)
            .then(|| receiver.get_or_insert_with(|| single(cx, call, psi::receiver(call.node))).clone())
    });
    match &ty {
        Ty::Tuple(list) if !cx.wants_tuple() => list.get(cx.value() as usize).cloned(),
        _ => (cx.wants_tuple() || cx.value() == 0).then_some(ty),
    }
}

fn call(cx: &Cx, psi: Psi) -> Ty {
    let Some(callee) = psi::prefix(psi.node) else { return Ty::Unknown };
    if callee.is(N::NameExpr) && cx.options.require_like.iter().any(|name| name == callee.code()) {
        let path = psi::first_string_arg(psi.node).and_then(|arg| psi::string_value(psi.src, arg));
        let file = path.and_then(|path| cx.files.required(&path));
        return file.and_then(|file| cx.at(file, 0, |root| cx.infer(root))).unwrap_or(Ty::Unknown);
    }
    let mut ret = Ty::Unknown;
    for ty in single(cx, psi, Some(callee)).each() {
        match ty {
            Ty::Function(function) => {
                for signature in function.signatures() {
                    if let Some(target) = returned(cx, psi, signature) {
                        ret.add(&target);
                    }
                }
            }
            // A class that is called builds one of its own.
            class if class.is_class() => ret.add(class),
            _ => {}
        }
    }
    if callee.is(N::IndexExpr)
        && let Some(name) = psi::member_name(psi.src, callee)
        && cx.options.constructors.iter().any(|constructor| *constructor == *name)
    {
        ret.add(&single(cx, psi, psi::prefix(callee)));
    }
    ret
}

/// The type the declaration `def`, a name that is assigned, gives to its global: its annotation, its value,
/// and the classes that hold what is assigned to its fields.
pub fn assigned_name(cx: &Cx, def: Psi) -> Ty {
    let stat = psi::assign_stat(def.node);
    let comment = stat.and_then(|stat| def.src.comment_of(stat));
    let mut ty = comment.and_then(|comment| Reader::new(def.file).declared(comment)).unwrap_or(Ty::Unknown);
    if let Some(stat) = stat {
        if let Some(values) = stat.child(N::ExprList) {
            let rank = psi::rank(def.node) as i32;
            if ty.is_invalid() || psi::value_at(values, rank).is_some_and(|value| value.is(N::TableExpr)) {
                ty.add(&value_at(cx, def.at(values), rank));
            }
        } else if ty.is_invalid() {
            ty.add(&unpacked(cx, def, psi::in_source(stat), Some(def.node.code())));
        }
    }
    if !resolve::is_local(def) && !matches!(ty, Ty::Primitive(_)) {
        ty.add(&global_type(cx, def.node.code(), cx.alone()));
    }
    ty
}

fn name(cx: &Cx, psi: Psi) -> Ty {
    let elem = psi.elem();
    let text = psi.node.code();
    let inferred = cx.once(elem, || {
        let mut ty = cx.guarded(elem, Some(text), || match resolve::self_owner(psi.node) {
            Some(owner) => cx.infer(psi.at(owner)),
            None => {
                let mut ty = Ty::Unknown;
                for found in members::globals(cx, psi, text).into_iter().take(10) {
                    ty.add(&members::declared_type(cx, found));
                }
                ty
            }
        });
        if ty.is_invalid() {
            ty = assigned_name(cx, psi);
        }
        ty
    });
    inferred.unwrap_or(Ty::Unknown)
}

fn literal(psi: Psi) -> Ty {
    match psi::literal_kind(psi.node) {
        Literal::Bool => Ty::BOOLEAN,
        Literal::String => Ty::STRING,
        Literal::Number | Literal::Hash => Ty::NUMBER,
        Literal::Varargs => {
            let owner = psi::enclosing_function(psi.node);
            owner.and_then(|owner| vararg_type(psi.at(owner))).unwrap_or(Ty::Unknown)
        }
        Literal::Nil | Literal::Unknown => Ty::Unknown,
    }
}

/// The type of the member `name` of one class.
fn field_type(cx: &Cx, class: &Ty, name: &str) -> Ty {
    if class.class_name() == Some(G) {
        return global_type(cx, name, false);
    }
    let mut ty = Ty::Unknown;
    for member in members::named(cx, class, name) {
        ty.add(&members::type_of(cx, &member));
    }
    ty
}

/// What a CfxLua `in` unpacking reads from its source: the member `key`, or an element when it has no name.
pub fn unpacked(cx: &Cx, psi: Psi, source: Option<Node>, key: Option<&str>) -> Ty {
    let source = single(cx, psi, source);
    let mut result = Ty::Unknown;
    if let Some(key) = key {
        for class in source.top_classes() {
            result.add(&field_type(cx, class, key));
        }
    }
    let keyed = if key.is_some() { Ty::STRING } else { Ty::NUMBER };
    for ty in source.each() {
        match ty {
            Ty::Array(base) if key.is_none() => result.add(base),
            Ty::Generic(generic) if generic.params.first() == Some(&keyed) => {
                result.add(generic.params.get(1).unwrap_or(&Ty::Unknown));
            }
            _ => {}
        }
    }
    result
}

fn index(cx: &Cx, psi: Psi) -> Ty {
    let inferred = cx.once(psi.elem(), || {
        let mut owner: Option<Ty> = None;
        if psi::is_brack(psi.node) {
            let prefix = single(cx, psi, psi::prefix(psi.node));
            let mut ty = Ty::Unknown;
            for member in prefix.each() {
                if let Ty::Array(base) = member {
                    ty.add(base);
                }
            }
            if ty == Ty::Unknown {
                for member in prefix.each() {
                    if let Ty::Generic(generic) = member {
                        ty.add(generic.params.get(1).unwrap_or(&Ty::Unknown));
                    }
                }
            }
            if ty == Ty::Unknown && !cx.alone() {
                let key = psi::key_expr(psi.node).map(|key| single(cx, psi, Some(key)));
                ty = members::indexed(cx, &prefix, key.as_ref());
            }
            if ty != Ty::Unknown {
                return ty;
            }
            owner = Some(prefix);
        }
        let stat = psi::assign_stat(psi.node);
        let comment = stat.and_then(|stat| psi.src.comment_of(stat));
        if let Some(annotated) = comment.and_then(|comment| Reader::new(psi.file).typed(comment)) {
            return annotated;
        }
        let name = psi::member_name(psi.src, psi.node);
        let mut result = match stat {
            Some(stat) => match stat.child(N::ExprList) {
                Some(values) => value_at(cx, psi.at(values), psi::rank(psi.node) as i32),
                None => unpacked(cx, psi, psi::in_source(stat), name.as_deref()),
            },
            None => Ty::Unknown,
        };
        if let Some(name) = name {
            let prefix = owner.unwrap_or_else(|| single(cx, psi, psi::prefix(psi.node)));
            for class in prefix.top_classes() {
                result.add(&field_type(cx, class, &name));
            }
            for ty in prefix.each() {
                if let Ty::Generic(generic) = ty
                    && generic.params.first() == Some(&Ty::STRING)
                {
                    result.add(generic.params.get(1).unwrap_or(&Ty::Unknown));
                }
            }
            if result.is_invalid() {
                result = members::indexed(cx, &prefix, Some(&Ty::STRING));
            }
        }
        result
    });
    inferred.unwrap_or(Ty::Unknown)
}

fn table(psi: Psi) -> Ty {
    let mut fields = psi.node.children_of(N::TableField);
    if let (Some(only), None) = (fields.next(), fields.next())
        && psi::field_value(only).is_some_and(|value| psi::literal_kind(value) == Literal::Varargs)
        && let Some(owner) = psi::enclosing_function(psi.node)
        && let Some(extra) = vararg_type(psi.at(owner))
    {
        return Ty::array(extra);
    }
    table_type(psi)
}

/// The type of the value a list gives to the name or the target of rank `rank`: when the list is shorter,
/// its last expression gives its values of the following ranks.
pub fn value_at(cx: &Cx, list: Psi, rank: i32) -> Ty {
    let Some(expr) = psi::value_at(list.node, rank) else { return Ty::Unknown };
    let count = list.node.exprs().count() as i32;
    let at = if count <= 1 {
        rank
    } else if rank + 1 > count {
        rank + 1 - count
    } else {
        0
    };
    cx.with_value(at, || cx.infer(list.at(expr)))
}

/// The types of all the values of a list: one type, or a tuple.
pub fn values(cx: &Cx, list: Psi) -> Ty {
    let mut exprs = list.node.exprs();
    match (exprs.next(), exprs.next()) {
        (Some(only), None) => cx.infer(list.at(only)),
        _ => Ty::tuple(list.node.exprs().map(|expr| cx.infer(list.at(expr))).collect()),
    }
}
