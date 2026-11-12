//! What a Lua value can be, as far as annotations and code tell: the type model of the JetBrains plugin
//! (`ty/`). A class is a name: what it extends and what it holds is looked up when it is asked for.

use std::rc::Rc;

mod casts;
pub mod context;
pub mod declarations;
pub mod doc;
pub mod expressions;
pub mod members;
pub mod psi;
mod render;
pub mod resolve;
mod subst;
pub mod subtype;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod tests;

pub use render::{Render, Simple, display, made_for_variable, plain};
pub use subst::{Bindings, analyze};

/// The type made for a local variable that has no other.
pub const ANONYMOUS: u8 = 0x1;
pub const GLOBAL: u8 = 0x2;
/// A function declared with a colon: `function a:b()`.
pub const SELF_FUNCTION: u8 = 0x4;
/// The table of `local a = {}`.
pub const ANONYMOUS_TABLE: u8 = 0x8;

pub const G: &str = "_G";
pub const SELF: &str = "self";
pub const ELLIPSIS: &str = "...";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    String,
    Number,
    Boolean,
    Table,
    Function,
}

impl Primitive {
    pub fn name(self) -> &'static str {
        match self {
            Primitive::String => "string",
            Primitive::Number => "number",
            Primitive::Boolean => "boolean",
            Primitive::Table => "table",
            Primitive::Function => "function",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Unknown,
    Void,
    Nil,
    Primitive(Primitive),
    Class(Rc<Class>),
    Array(Rc<Ty>),
    Function(Rc<Function>),
    Generic(Rc<Generic>),
    Tuple(Rc<[Ty]>),
    StringLiteral(Rc<str>),
    /// At least two types, none of them a union, each one once: built by `union` only.
    Union(Rc<[Ty]>),
}

#[derive(Clone, Debug)]
pub struct Class {
    pub name: Rc<str>,
    /// The variable an anonymous or a global type was made for.
    pub var_name: Rc<str>,
    pub flags: u8,
    pub detail: Detail,
}

#[derive(Clone, Debug)]
pub enum Detail {
    Named,
    /// A generic parameter, and the class it is bound to in `T : Base`.
    Param(Option<Rc<str>>),
    /// `{ name: T }` written in a documentation comment.
    Table(Rc<[DocField]>),
}

#[derive(Clone, Debug)]
pub struct DocField {
    pub name: Rc<str>,
    pub ty: Ty,
    pub optional: bool,
}

/// Two classes are the same when name and flags are: `Foo` the class and `Foo` the global are not.
impl PartialEq for Class {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.flags == other.flags
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Generic {
    pub base: Ty,
    pub params: Vec<Ty>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub main: Signature,
    pub overloads: Vec<Signature>,
    pub flags: u8,
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub colon_call: bool,
    pub params: Vec<Param>,
    pub ret: Ret,
    pub vararg: Option<Ty>,
    pub generics: Vec<TyParam>,
}

/// Two signatures are the same when they take the same types, whatever they return.
impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.params.len() == other.params.len() && self.params.iter().zip(&other.params).all(|(a, b)| a.ty == b.ty)
    }
}

#[derive(Clone, Debug)]
pub enum Ret {
    Ty(Ty),
    /// What the body of a function of the code returns, inferred when it is read.
    Body(FuncRef),
}

/// A file of the project, as the index numbers it.
pub type FileId = u32;

/// A function of the code: its file, and where it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncRef {
    pub file: FileId,
    pub at: u32,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Rc<str>,
    pub ty: Ty,
    /// A call may leave it out: `name?`, or a type that admits nil.
    pub optional: bool,
}

impl Param {
    pub fn new(name: &str, ty: Ty) -> Param {
        Param { name: name.into(), ty, optional: false }
    }

    /// The receiver of a method, when a call has to count it among its arguments.
    pub fn receiver(ty: Option<Ty>) -> Param {
        Param::new(SELF, ty.unwrap_or(Ty::Unknown))
    }

    /// The name as signatures show it: `name?` when the parameter may be left out.
    pub fn display_name(&self) -> String {
        if self.optional { format!("{}?", self.name) } else { self.name.to_string() }
    }
}

/// A generic parameter of a signature: `T`, or `T : Base`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TyParam {
    pub name: Rc<str>,
    pub base: Option<Rc<str>>,
}

impl Signature {
    pub fn new(colon_call: bool, params: Vec<Param>, ret: Ty) -> Signature {
        Signature { colon_call, params, ret: Ret::Ty(ret), vararg: None, generics: Vec::new() }
    }

    pub fn is_generic(&self) -> bool {
        !self.generics.is_empty()
    }

    /// The parameters a call gives values to, in the order of its arguments. A method called with a dot
    /// takes its receiver first; a function called with a colon was given its first parameter already.
    pub fn args(&self, receiver: Option<&Ty>, colon_style: bool) -> Vec<Param> {
        let mut out = Vec::with_capacity(self.params.len() + 1);
        let mut skipped = 0;
        if colon_style && !self.colon_call {
            skipped = 1;
        } else if !colon_style && self.colon_call {
            out.push(Param::receiver(receiver.cloned()));
        }
        out.extend(self.params.iter().skip(skipped).cloned());
        out
    }

    /// The parameters as the declaration counts them: the receiver of a method comes first.
    pub fn declared(&self, receiver: Option<&Ty>) -> Vec<Param> {
        let mut out = Vec::with_capacity(self.params.len() + 1);
        if self.colon_call {
            out.push(Param::receiver(receiver.cloned()));
        }
        out.extend(self.params.iter().cloned());
        out
    }

    pub fn param_ty(&self, index: usize) -> Ty {
        self.params.get(index).map_or(Ty::Unknown, |param| param.ty.clone())
    }
}

impl Function {
    pub fn new(main: Signature) -> Function {
        Function { main, overloads: Vec::new(), flags: 0 }
    }

    pub fn is_colon_call(&self) -> bool {
        self.flags & SELF_FUNCTION != 0
    }

    pub fn signatures(&self) -> impl Iterator<Item = &Signature> {
        std::iter::once(&self.main).chain(&self.overloads)
    }

    /// The signature whose parameter count is the closest to `args`, the first one among equals.
    pub fn perfect_signature(&self, args: usize) -> &Signature {
        let mut best = &self.main;
        let mut distance = usize::MAX;
        for signature in self.signatures() {
            let offset = signature.params.len().abs_diff(args);
            if offset < distance {
                distance = offset;
                best = signature;
            }
        }
        best
    }
}

/// `$name`, the class that holds what is assigned to the fields of the global `name`.
pub fn global_type_name(name: &str) -> String {
    if name == G { name.to_owned() } else { format!("${name}") }
}

/// The class of a table written in a documentation comment of `file`, at `offset`.
pub fn doc_table_name(file: FileId, offset: usize) -> String {
    format!("10|{file}|{offset}")
}

impl Ty {
    pub const STRING: Ty = Ty::Primitive(Primitive::String);
    pub const NUMBER: Ty = Ty::Primitive(Primitive::Number);
    pub const BOOLEAN: Ty = Ty::Primitive(Primitive::Boolean);
    pub const TABLE: Ty = Ty::Primitive(Primitive::Table);
    pub const FUNCTION: Ty = Ty::Primitive(Primitive::Function);

    /// The type a name of the language stands for; `integer` is a number, `true` and `false` booleans.
    pub fn builtin(name: &str) -> Option<Ty> {
        Some(match name {
            "nil" => Ty::Nil,
            "void" => Ty::Void,
            "any" | "unknown" => Ty::Unknown,
            "boolean" | "true" | "false" => Ty::BOOLEAN,
            "string" => Ty::STRING,
            "number" | "integer" => Ty::NUMBER,
            "table" => Ty::TABLE,
            "function" => Ty::FUNCTION,
            _ => return None,
        })
    }

    /// The type a name written in an annotation stands for: one of the language, or else a class.
    pub fn named(name: &str) -> Ty {
        Ty::builtin(name).unwrap_or_else(|| Ty::class(name))
    }

    pub fn class(name: &str) -> Ty {
        Ty::flagged(name, name, 0)
    }

    pub fn flagged(name: &str, var_name: &str, flags: u8) -> Ty {
        let name: Rc<str> = name.into();
        let var_name = if *name == *var_name { name.clone() } else { var_name.into() };
        Ty::Class(Rc::new(Class { name, var_name, flags, detail: Detail::Named }))
    }

    /// A generic parameter standing for a class that extends `base`.
    pub fn param(name: &str, base: Option<Rc<str>>) -> Ty {
        let name: Rc<str> = name.into();
        Ty::Class(Rc::new(Class { var_name: name.clone(), name, flags: 0, detail: Detail::Param(base) }))
    }

    pub fn array(base: Ty) -> Ty {
        Ty::Array(Rc::new(base))
    }

    pub fn generic(base: Ty, params: Vec<Ty>) -> Ty {
        Ty::Generic(Rc::new(Generic { base, params }))
    }

    pub fn tuple(list: Vec<Ty>) -> Ty {
        Ty::Tuple(list.into())
    }

    pub fn function(function: Function) -> Ty {
        Ty::Function(Rc::new(function))
    }

    /// A type that says nothing about the value: unknown, nil or void.
    pub fn is_invalid(&self) -> bool {
        matches!(self, Ty::Unknown | Ty::Nil | Ty::Void)
    }

    /// The types of a union, or the type itself.
    pub fn each(&self) -> &[Ty] {
        match self {
            Ty::Union(members) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// Both types, each member once. A type that says nothing is dropped: `T|nil` is `T`.
    pub fn union(&self, other: &Ty) -> Ty {
        if self.is_invalid() {
            return other.clone();
        }
        if other.is_invalid() {
            return self.clone();
        }
        let (first, second) =
            if matches!(other, Ty::Union(_)) && !matches!(self, Ty::Union(_)) { (other, self) } else { (self, other) };
        let mut members = first.each().to_vec();
        for member in second.each() {
            if !members.contains(member) {
                members.push(member.clone());
            }
        }
        if members.len() == 1 { members.remove(0) } else { Ty::Union(members.into()) }
    }

    /// Adds `other` to the types gathered so far.
    pub fn add(&mut self, other: &Ty) {
        *self = self.union(other);
    }

    pub fn flags(&self) -> u8 {
        match self {
            Ty::Class(class) => class.flags,
            Ty::Function(function) => function.flags,
            _ => 0,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.flags() & ANONYMOUS != 0
    }

    pub fn is_global(&self) -> bool {
        self.flags() & GLOBAL != 0
    }

    /// The name members are looked up under: a class, or `string`, whose members are those of the library.
    pub fn class_name(&self) -> Option<&str> {
        match self {
            Ty::Class(class) => Some(&class.name),
            Ty::Primitive(Primitive::String) => Some(Primitive::String.name()),
            _ => None,
        }
    }

    pub fn is_class(&self) -> bool {
        self.class_name().is_some()
    }

    pub fn as_function(&self) -> Option<&Function> {
        match self {
            Ty::Function(function) => Some(function),
            _ => None,
        }
    }

    /// The classes a value of this type may be: those of a union, or of the first value of a tuple.
    pub fn top_classes(&self) -> Vec<&Ty> {
        match self {
            Ty::Tuple(list) => list.first().map(Ty::top_classes).unwrap_or_default(),
            _ => self.each().iter().filter(|ty| ty.is_class()).collect(),
        }
    }

    /// The class that says the most: a declared one, or else a global, or else an anonymous one.
    pub fn perfect_class(&self) -> Option<&Ty> {
        let (mut anonymous, mut global) = (None, None);
        for ty in self.each().iter().filter(|ty| ty.is_class()) {
            if ty.is_anonymous() {
                anonymous = Some(ty);
            } else if ty.is_global() {
                global = Some(ty);
            } else {
                return Some(ty);
            }
        }
        global.or(anonymous)
    }
}

/// The value of a string literal type: `'"value"'`, the older notation, holds its own quotes.
pub fn literal_value(content: &str) -> &str {
    let quoted = content.len() >= 2
        && content.starts_with(['"', '\''])
        && content.as_bytes().first() == content.as_bytes().last();
    if quoted { &content[1..content.len() - 1] } else { content }
}
