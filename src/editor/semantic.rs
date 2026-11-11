use crate::feature::Ctx;
use crate::lua::scope::{DeclKind, Scopes};
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::natives::NativeSet;
use crate::runtime::stubs::{Part, runtime};

/// What the IDE keeps of a document's tokens.
pub const MAX_TOKENS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Variable,
    Parameter,
    Function,
    Method,
    Property,
    /// The implicit `self` of a method.
    SelfName,
    /// A CfxLua `` `hash` `` literal.
    Hash,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods(u8);

impl Mods {
    pub const NONE: Mods = Mods(0);
    pub const DECLARATION: Mods = Mods(1);
    pub const STATIC: Mods = Mods(2);
    pub const DEPRECATED: Mods = Mods(4);
    /// Natives, the Cfx runtime API and the Lua standard library.
    pub const LIBRARY: Mods = Mods(8);
    pub const GLOBAL: Mods = Mods(16);
    /// A local of an enclosing function.
    pub const UPVALUE: Mods = Mods(32);

    pub fn has(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Mods) -> Mods {
        Mods(self.0 | other.0)
    }

    fn when(self, other: Mods, yes: bool) -> Mods {
        if yes { self.with(other) } else { self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
    pub mods: Mods,
}

impl Token {
    /// The LSP token type the editor styles, in the kebab-case of the plugin API.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Variable => "variable",
            Kind::Parameter => "parameter",
            Kind::Function => "function",
            Kind::Method => "method",
            Kind::Property => "property",
            Kind::SelfName => "self-keyword",
            Kind::Hash => "string",
        }
    }

    pub fn modifier_names(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let names = [
            (Mods::DECLARATION, "declaration"),
            (Mods::STATIC, "static"),
            (Mods::DEPRECATED, "deprecated"),
            (Mods::LIBRARY, "default-library"),
            (Mods::GLOBAL, "global"),
            (Mods::UPVALUE, "mutable"),
        ];
        for (flag, name) in names {
            if self.mods.has(flag) {
                out.push(name);
            }
        }
        // The editor has no style for a string token: this modifier carries the string colour.
        if self.kind == Kind::Hash {
            out.push("byte-string");
        }
        out
    }
}

struct Painter<'a> {
    cx: &'a Ctx<'a>,
    scopes: &'a Scopes,
    sets: Vec<NativeSet>,
    parts: Vec<Part>,
    functions: &'a dyn Fn(&str) -> bool,
    out: Vec<Token>,
}

fn is_callee(node: Node) -> bool {
    node.parent().is_some_and(|parent| parent.is(N::CallExpr) && parent.expr() == Some(node))
}

/// Whether `node` reads the `self` of a method from a function nested in it.
fn self_upvalue(node: Node) -> bool {
    let Some(body) = node.ancestor(N::FuncBody) else { return false };
    let Some(method) = node.ancestor(N::ClassMethodDef) else { return false };
    let colon = method.child(N::ClassMethodName).is_some_and(|name| name.has(T::Colon));
    colon && method.child(N::FuncBody).is_some_and(|own| own.id() < body.id())
}

impl<'a> Painter<'a> {
    /// A name of the natives, the runtime API or the standard library: is it a function, is it deprecated.
    fn library(&self, name: &str) -> Option<(bool, bool)> {
        if let Some(native) = self.cx.natives.find(name, &self.sets) {
            return Some((true, native.is_alias()));
        }
        if let Some(global) = runtime().global(name) {
            let decl = runtime().decl(global.decl);
            if self.parts.contains(&decl.part) {
                return Some((decl.is_function(), false));
            }
        }
        crate::lua::std::global(name).map(|function| (function, false))
    }

    fn name(&self, node: Node) -> (Kind, Mods) {
        let name = node.code();
        match self.scopes.resolve(node) {
            Some(decl) if decl.kind != DeclKind::Global => {
                let kind = match decl.kind {
                    DeclKind::Param | DeclKind::LoopVar => Kind::Parameter,
                    DeclKind::Function => Kind::Function,
                    _ => Kind::Variable,
                };
                (kind, Mods::NONE.when(Mods::UPVALUE, self.scopes.is_upvalue(node)))
            }
            _ if name == "self" => (Kind::SelfName, Mods::NONE.when(Mods::UPVALUE, self_upvalue(node))),
            _ => match self.library(name) {
                Some((function, deprecated)) => {
                    let kind = if function || is_callee(node) { Kind::Function } else { Kind::Variable };
                    (kind, Mods::LIBRARY.when(Mods::DEPRECATED, deprecated))
                }
                None if (self.functions)(name) => (Kind::Function, Mods::GLOBAL),
                None => (Kind::Variable, Mods::GLOBAL),
            },
        }
    }

    fn push(&mut self, node: Node, kind: Kind, mods: Mods) {
        if let Some(id) = node.token(T::Id) {
            self.out.push(Token { start: id.start(), end: id.end(), kind, mods });
        }
    }

    fn member(&mut self, node: Node) {
        let library =
            node.expr().is_some_and(|prefix| prefix.is(N::NameExpr) && self.name(prefix).1.has(Mods::LIBRARY));
        let callee = is_callee(node);
        let (kind, mods) = match (library, callee) {
            (true, true) => (Kind::Function, Mods::LIBRARY),
            (true, false) => (Kind::Variable, Mods::LIBRARY),
            (false, true) => (Kind::Method, Mods::NONE.when(Mods::STATIC, !node.has(T::Colon))),
            // `a:b` without arguments is an error, reported with the syntax problems.
            (false, false) if node.has(T::Colon) => return,
            (false, false) => (Kind::Property, Mods::NONE),
        };
        self.push(node, kind, mods);
    }

    fn paint(&mut self, node: Node) {
        match node.kind() {
            N::NameDef => self.push(node, Kind::Variable, Mods::DECLARATION),
            N::ParamNameDef => self.push(node, Kind::Parameter, Mods::DECLARATION),
            N::LocalFuncDef => self.push(node, Kind::Function, Mods::DECLARATION),
            // `function f() end` over a `local f` assigns to that local.
            N::FuncDef => match self.scopes.resolve(node).map(|decl| decl.kind) {
                Some(DeclKind::Function) => self.push(node, Kind::Function, Mods::NONE),
                Some(DeclKind::Global) | None => self.push(node, Kind::Function, Mods::DECLARATION.with(Mods::GLOBAL)),
                Some(_) => self.push(node, Kind::Variable, Mods::NONE),
            },
            N::ClassMethodName => {
                self.push(node, Kind::Method, Mods::DECLARATION.when(Mods::STATIC, node.has(T::Dot)));
            }
            N::TableField if node.has(T::Assign) || node.has(T::Dot) => self.push(node, Kind::Property, Mods::NONE),
            N::NameExpr => {
                let (kind, mods) = self.name(node);
                self.push(node, kind, mods);
            }
            N::IndexExpr => self.member(node),
            _ => {}
        }
    }
}

/// The tokens of the document, as the IntelliJ plugin's annotator colours them. `functions` tells
/// whether a `function name()` statement of the project defines the global `name`.
pub fn tokens(cx: &Ctx, functions: &dyn Fn(&str) -> bool) -> Vec<Token> {
    let doc = cx.doc;
    let root = doc.src.root();
    let detected = cx.ws.detected_games();
    let games = cx.ws.resource_of(&doc.path).and_then(|resource| resource.games.clone());
    let parts =
        if doc.is_manifest() { vec![Part::Manifest] } else { crate::runtime::visible_parts(cx.settings, doc.side) };
    let mut painter = Painter {
        cx,
        scopes: doc.src.scopes(),
        sets: crate::natives::visible_sets(cx.settings, &detected, doc.side, games.as_deref()),
        parts,
        functions,
        out: Vec::new(),
    };
    for node in root.descendants() {
        painter.paint(node);
    }
    let mut out = painter.out;
    let tokens = root.tree().tokens();
    for i in (0..tokens.len()).filter(|i| tokens.kind(*i) == T::HashString) {
        out.push(Token { start: tokens.start(i), end: tokens.end(i), kind: Kind::Hash, mods: Mods::NONE });
    }
    out.sort_by_key(|token| token.start);
    out.truncate(MAX_TOKENS);
    out
}
