use crate::events::ast::{Ast, BinOp, DeclKind, ExprKind, Key, NONE, StmtKind, UnOp};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::collections::HashMap;

pub const DYNAMIC: &str = "\u{0}";

const MAX_ALIAS_DEPTH: u32 = 4;

pub const STRING: &str = "string";
pub const NUMBER: &str = "number";
pub const INTEGER: &str = "integer";
pub const BOOLEAN: &str = "boolean";
pub const TABLE: &str = "table";
pub const FUNCTION: &str = "function";
pub const NIL: &str = "nil";
pub const ANY: &str = "any";

const CONSTRUCTORS: &[(&str, &str)] = &[
    ("vector2", "vector2"),
    ("vec2", "vector2"),
    ("vector3", "vector3"),
    ("vec3", "vector3"),
    ("vec", "vector3"),
    ("vector4", "vector4"),
    ("vec4", "vector4"),
    ("quat", "quat"),
    ("tostring", STRING),
    ("tonumber", NUMBER),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Player,
    Entity,
    Global,
    Unknown,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Entity => "entity",
            Self::Global => "global",
            Self::Unknown => "any",
        }
    }

    pub fn matches(self, other: Kind) -> bool {
        self == Kind::Unknown || other == Kind::Unknown || self == other
    }

    pub fn description(self) -> String {
        match self {
            Self::Unknown => "state bag".into(),
            k => format!("{} state bag", k.name()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Write,
    Read,
    Handler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replication {
    Replicated,
    Local,
    BySide,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Ident,
    Str(char),
    Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub key: Option<String>,
    pub kind: Kind,
    pub access: Access,
    pub start: usize,
    pub end: usize,
    pub element: Element,
    pub replication: Replication,
    pub value_type: &'static str,
}

#[derive(Clone, Copy)]
enum Target {
    Assign(u32, usize),
    Unpack,
    Compound(u32),
    FunctionName,
}

pub struct Analyzer<'a> {
    src: &'a Source,
    ast: &'a Ast,
    targets: HashMap<u32, Target>,
}

fn is_integer_literal(text: &str) -> bool {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()),
    }
}

impl<'a> Analyzer<'a> {
    pub fn new(src: &'a Source, ast: &'a Ast) -> Self {
        let mut targets = HashMap::new();
        for (s, stmt) in ast.stmts.iter().enumerate() {
            match &stmt.kind {
                StmtKind::Assign { targets: t, unpack, .. } => {
                    for (pos, &e) in t.iter().enumerate() {
                        targets
                            .insert(e, if unpack.is_some() { Target::Unpack } else { Target::Assign(s as u32, pos) });
                    }
                }
                StmtKind::Compound { target, op, .. } => {
                    targets.insert(*target, Target::Compound(*op));
                }
                StmtKind::Function { target: Some(t), .. } => {
                    let mut e = *t;
                    while let ExprKind::Index(obj, _) = ast.kind(e) {
                        targets.insert(e, Target::FunctionName);
                        e = *obj;
                    }
                }
                _ => {}
            }
        }
        Self { src, ast, targets }
    }

    fn text(&self, tok: u32) -> &'a str {
        self.src.tok_text(tok as usize)
    }

    fn local_init(&self, decl: u32) -> Option<u32> {
        match self.ast.decl(decl).kind {
            DeclKind::Local { init } => init,
            _ => None,
        }
    }

    fn bag(&self, e: u32, depth: u32) -> Option<Kind> {
        let ast = self.ast;
        let e = ast.unparen(e);
        match ast.kind(e) {
            ExprKind::Name(d) if *d == NONE => (ast.name(self.src, e) == Some("GlobalState")).then_some(Kind::Global),
            ExprKind::Name(d) if depth < MAX_ALIAS_DEPTH => self.bag(self.local_init(*d)?, depth + 1),
            ExprKind::Index(obj, Key::Field(Some(n))) if self.text(*n) == "state" => self.wrapper(*obj, depth),
            _ => None,
        }
    }

    fn wrapper(&self, e: u32, depth: u32) -> Option<Kind> {
        let ast = self.ast;
        let e = ast.unparen(e);
        match ast.kind(e) {
            ExprKind::Name(d) if *d == NONE => (ast.name(self.src, e) == Some("LocalPlayer")).then_some(Kind::Player),
            ExprKind::Name(d) if depth < MAX_ALIAS_DEPTH => self.wrapper(self.local_init(*d)?, depth + 1),
            ExprKind::Call(callee, _) => match ast.global(self.src, *callee) {
                Some("Player") => Some(Kind::Player),
                Some("Entity") => Some(Kind::Entity),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn value_type(&self, e: Option<u32>) -> &'static str {
        let Some(e) = e else { return ANY };
        let ast = self.ast;
        match ast.kind(e) {
            ExprKind::Str => STRING,
            ExprKind::Hash => INTEGER,
            ExprKind::Number => {
                if is_integer_literal(self.text(ast.expr(e).first)) {
                    INTEGER
                } else {
                    NUMBER
                }
            }
            ExprKind::True | ExprKind::False => BOOLEAN,
            ExprKind::Nil => NIL,
            ExprKind::Table(_) => TABLE,
            ExprKind::Func(_) => FUNCTION,
            ExprKind::Paren(x) => self.value_type(Some(*x)),
            ExprKind::Unary(UnOp::Not, _) => BOOLEAN,
            ExprKind::Unary(UnOp::Len | UnOp::BNot, _) => INTEGER,
            ExprKind::Unary(UnOp::Neg, x) => {
                if self.value_type(Some(*x)) == INTEGER {
                    INTEGER
                } else {
                    NUMBER
                }
            }
            ExprKind::Binary(op, l, r) => match op {
                BinOp::Concat => STRING,
                BinOp::Compare => BOOLEAN,
                BinOp::Additive | BinOp::Multiplicative | BinOp::Pow => NUMBER,
                BinOp::BOr | BinOp::BXor | BinOp::BAnd | BinOp::Shift => INTEGER,
                BinOp::And => self.value_type(Some(*r)),
                BinOp::Or => {
                    let left = self.value_type(Some(*l));
                    if left == self.value_type(Some(*r)) { left } else { ANY }
                }
            },
            ExprKind::Call(callee, _) => {
                if let Some(name) = ast.global(self.src, *callee) {
                    return CONSTRUCTORS.iter().find(|(n, _)| *n == name).map_or(ANY, |(_, t)| t);
                }
                let (s, end) = ast.span(self.src, *callee);
                if &self.src.text[s..end] == "json.encode" { STRING } else { ANY }
            }
            _ => ANY,
        }
    }

    fn written_type(&self, e: Option<u32>) -> &'static str {
        let t = self.value_type(e);
        match e {
            Some(e) if t == ANY => {
                let d = self.ast.decl_of(self.ast.unparen(e));
                if d == NONE { t } else { self.local_init(d).map_or(t, |i| self.value_type(Some(i))) }
            }
            _ => t,
        }
    }

    fn element_of(&self, e: u32) -> (usize, usize, Element) {
        let ast = self.ast;
        let (s, end) = ast.span(self.src, e);
        if *ast.kind(e) == ExprKind::Str {
            let tok = self.src.toks[ast.expr(e).first as usize];
            let quote =
                if tok.kind == Tok::LongStr { '\'' } else { self.src.text[tok.start..].chars().next().unwrap_or('\'') };
            return (s, end, Element::Str(quote));
        }
        (s, end, Element::Expr)
    }

    pub fn index_site(&self, e: u32) -> Option<Site> {
        let ast = self.ast;
        let ExprKind::Index(obj, key) = ast.kind(e) else { return None };
        if matches!(self.targets.get(&e), Some(Target::FunctionName)) {
            return None;
        }
        let (key, start, end, element) = match key {
            Key::Field(Some(n)) => {
                let t = self.src.toks[*n as usize];
                (Some(self.text(*n).to_owned()), t.start, t.end, Element::Ident)
            }
            Key::Bracket(Some(k)) => {
                let (s, end, element) = self.element_of(*k);
                (ast.string(self.src, *k), s, end, element)
            }
            _ => return None,
        };
        if key.as_deref() == Some("set") {
            return None;
        }
        let kind = self.bag(*obj, 0)?;
        let site = |access, replication, value_type| Site {
            key: key.clone(),
            kind,
            access,
            start,
            end,
            element,
            replication,
            value_type,
        };
        Some(match self.targets.get(&e) {
            Some(Target::Unpack) => site(Access::Write, Replication::BySide, ANY),
            Some(Target::Assign(stmt, pos)) => {
                let StmtKind::Assign { values, .. } = &ast.stmts[*stmt as usize].kind else { return None };
                let value_type = match values.get(*pos) {
                    Some(&v) => self.written_type(Some(v)),
                    None if values
                        .last()
                        .is_some_and(|&l| matches!(ast.kind(l), ExprKind::Call(..) | ExprKind::Vararg)) =>
                    {
                        ANY
                    }
                    None => NIL,
                };
                site(Access::Write, Replication::BySide, value_type)
            }
            Some(Target::Compound(op)) => {
                let bitwise = matches!(self.text(*op), "&=" | "|=" | "^=" | "<<=" | ">>=");
                site(Access::Write, Replication::BySide, if bitwise { INTEGER } else { NUMBER })
            }
            _ => site(Access::Read, Replication::Unknown, ""),
        })
    }

    fn replication_of(&self, e: Option<u32>) -> Replication {
        match e.map(|e| self.ast.kind(e)) {
            None | Some(ExprKind::False | ExprKind::Nil) => Replication::Local,
            Some(ExprKind::True) => Replication::Replicated,
            _ => Replication::Unknown,
        }
    }

    pub fn bag_name_kind(&self, e: Option<u32>) -> Kind {
        let Some(prefix) = e.and_then(|e| self.bag_name_prefix(e)) else { return Kind::Unknown };
        if prefix == "global" {
            Kind::Global
        } else if prefix.starts_with("player:") {
            Kind::Player
        } else if prefix.starts_with("entity:") || prefix.starts_with("localEntity:") {
            Kind::Entity
        } else {
            Kind::Unknown
        }
    }

    fn bag_name_prefix(&self, e: u32) -> Option<String> {
        let ast = self.ast;
        match ast.kind(e) {
            ExprKind::Str => ast.string(self.src, e),
            ExprKind::Paren(x) | ExprKind::Binary(BinOp::Concat, x, _) => self.bag_name_prefix(*x),
            ExprKind::Call(callee, _) => match ast.kind(*callee) {
                ExprKind::Index(obj, Key::Method(Some(n))) if self.text(*n) == "format" => self.bag_name_prefix(*obj),
                _ => None,
            },
            _ => None,
        }
    }

    fn key_arg(&self, arg: u32) -> Result<Option<String>, ()> {
        match self.ast.string(self.src, arg) {
            Some(k) => Ok(Some(k)),
            None if self.ast.is_literal(arg) => Err(()),
            None => Ok(None),
        }
    }

    pub fn call_site(&self, e: u32) -> Option<Site> {
        let ast = self.ast;
        let ExprKind::Call(callee, args) = ast.kind(e) else { return None };
        let make = |arg: u32, key: Option<String>, kind, access, replication, value_type| {
            let (start, end, element) = self.element_of(arg);
            Site { key, kind, access, start, end, element, replication, value_type }
        };
        if let ExprKind::Index(obj, key) = ast.kind(*callee) {
            let Key::Method(Some(n)) = key else { return None };
            if self.text(*n) != "set" {
                return None;
            }
            let kind = self.bag(*obj, 0)?;
            let arg = *args.first()?;
            let key = self.key_arg(arg).ok()?;
            let value = args.get(1).copied();
            let value_type = if value.is_some() { self.written_type(value) } else { NIL };
            return Some(make(arg, key, kind, Access::Write, self.replication_of(args.get(2).copied()), value_type));
        }
        match ast.global(self.src, *callee)? {
            "AddStateBagChangeHandler" => {
                let arg = *args.first()?;
                let key = ast.string(self.src, arg)?;
                Some(make(
                    arg,
                    Some(key),
                    self.bag_name_kind(args.get(1).copied()),
                    Access::Handler,
                    Replication::Unknown,
                    "",
                ))
            }
            "SetStateBagValue" => {
                let arg = *args.get(1)?;
                let key = self.key_arg(arg).ok()?;
                let replication = self.replication_of(args.get(4).copied());
                Some(make(arg, key, self.bag_name_kind(args.first().copied()), Access::Write, replication, ANY))
            }
            "GetStateBagValue" | "StateBagHasKey" => {
                let arg = *args.get(1)?;
                let key = ast.string(self.src, arg)?;
                Some(make(
                    arg,
                    Some(key),
                    self.bag_name_kind(args.first().copied()),
                    Access::Read,
                    Replication::Unknown,
                    "",
                ))
            }
            _ => None,
        }
    }

    pub fn sites(&self) -> Vec<Site> {
        let mut out: Vec<Site> = (0..self.ast.exprs.len() as u32)
            .filter_map(|e| match self.ast.kind(e) {
                ExprKind::Index(..) => self.index_site(e),
                ExprKind::Call(..) => self.call_site(e),
                _ => None,
            })
            .collect();
        out.sort_by_key(|s| (s.start, s.end));
        out
    }

    pub fn field_at(&self, at: usize) -> Option<(Kind, usize)> {
        let src = self.src;
        for (id, e) in self.ast.exprs.iter().enumerate() {
            let ExprKind::Index(obj, Key::Field(name)) = &e.kind else { continue };
            // `bag.|` before the next line's code, which reads as the field name
            let dot = name.map_or(e.last, |n| n - 1) as usize;
            let start = if src.tok_before(at) == Some(dot) {
                at
            } else {
                match name.map(|n| src.toks[n as usize]) {
                    Some(t) if t.start <= at && at <= t.end => t.start,
                    _ => continue,
                }
            };
            if matches!(self.targets.get(&(id as u32)), Some(Target::FunctionName)) {
                return None;
            }
            return Some((self.bag(*obj, 0)?, start));
        }
        None
    }
}

pub fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub fn may_hold_bags(text: &str) -> bool {
    text.contains("tate")
}

pub fn analyze(src: &Source) -> Vec<Site> {
    let ast = crate::events::ast::parse(src);
    Analyzer::new(src, &ast).sites()
}

#[cfg(test)]
pub const SERVER: &str = "RegisterNetEvent('job:set', function()\n    local src = source\n    \
    Player(src).state:set('test', 1, true)\n    Player(src).state.test2 = 'x'\n    local state = Player(src).state\n    \
    state:set('job', { name = 'police' }, false)\n    state['grade'] = 3\n    GlobalState.weather = 'RAIN'\n    \
    Entity(GetPlayerPed(src)).state.fuel = 50.5\nend)";

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for s in analyze(&Source::new(text)) {
            let line = format!("{:?} {:?} {:?} {}", s.kind, s.access, s.replication, s.value_type).to_uppercase();
            out.entry(s.key.unwrap_or_else(|| DYNAMIC.into())).or_default().push(line.trim().to_owned());
        }
        out
    }

    fn one(e: &std::collections::BTreeMap<String, Vec<String>>, key: &str) -> Vec<String> {
        e.get(key).cloned().unwrap_or_default()
    }

    #[test]
    fn writes_on_player_entity_and_global_bags() {
        let e = entries(SERVER);
        assert_eq!(one(&e, "test"), ["PLAYER WRITE REPLICATED INTEGER"]);
        assert_eq!(one(&e, "test2"), ["PLAYER WRITE BYSIDE STRING"]);
        assert_eq!(one(&e, "job"), ["PLAYER WRITE LOCAL TABLE"]);
        assert_eq!(one(&e, "grade"), ["PLAYER WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "weather"), ["GLOBAL WRITE BYSIDE STRING"]);
        assert_eq!(one(&e, "fuel"), ["ENTITY WRITE BYSIDE NUMBER"]);
        assert!(!e.contains_key("state") && !e.contains_key("set"), "{e:?}");
    }

    #[test]
    fn reads_with_safe_navigation_and_brackets() {
        let e = entries(
            "local a = LocalPlayer?.state?.test\nlocal b = LocalPlayer.state['test2']\nlocal c = (Player(-1)).state?.job\n\
             local d = Entity(PlayerPedId())?.state.fuel\nprint(GlobalState.weather, a, b, c, d)\nif GlobalState?.weather then end",
        );
        assert_eq!(one(&e, "test"), ["PLAYER READ UNKNOWN"]);
        assert_eq!(one(&e, "test2"), ["PLAYER READ UNKNOWN"]);
        assert_eq!(one(&e, "job"), ["PLAYER READ UNKNOWN"]);
        assert_eq!(one(&e, "fuel"), ["ENTITY READ UNKNOWN"]);
        assert_eq!(one(&e, "weather"), ["GLOBAL READ UNKNOWN", "GLOBAL READ UNKNOWN"]);
    }

    #[test]
    fn local_aliases_of_bags_and_wrappers() {
        let e = entries(
            "local function setup(src, vehicle)\n    local state = Player(src).state\n    state:set('alias1', true, true)\n    \
             local ply = Player(src)\n    ply.state.alias2 = vector3(1.0, 2.0, 3.0)\n    local ent = Entity(vehicle)\n    \
             local bag = ent.state\n    bag.alias3 = #'abc'\n    local global = GlobalState\n    global['alias4'] = 'x' .. src\n    \
             local inner = state\n    inner.alias5 = not inner.alias1\nend\nlocal state = {}\nstate.notABag = 1\n\
             local function other(state)\n    state.param = 2\nend",
        );
        assert_eq!(one(&e, "alias1"), ["PLAYER WRITE REPLICATED BOOLEAN", "PLAYER READ UNKNOWN"]);
        assert_eq!(one(&e, "alias2"), ["PLAYER WRITE BYSIDE VECTOR3"]);
        assert_eq!(one(&e, "alias3"), ["ENTITY WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "alias4"), ["GLOBAL WRITE BYSIDE STRING"]);
        assert_eq!(one(&e, "alias5"), ["PLAYER WRITE BYSIDE BOOLEAN"]);
        assert!(!e.contains_key("notABag") && !e.contains_key("param"), "{e:?}");
    }

    #[test]
    fn change_handlers_natives_compound_and_dynamic_writes() {
        let e = entries(
            "AddStateBagChangeHandler('test', nil, function(bagName, key, value) end)\n\
             AddStateBagChangeHandler('fuel', ('entity:%d'):format(1), function() end)\n\
             AddStateBagChangeHandler(nil, 'global', function() end)\nlocal x = GetStateBagValue('global', 'weather')\n\
             local y = StateBagHasKey('player:' .. GetPlayerServerId(PlayerId()), 'test2')\nSetStateBagValue('player:1', 'raw', '', 0, true)\n\
             LocalPlayer.state.counter += 1\nlocal name = 'dyn'\nLocalPlayer.state[name] = 1\nLocalPlayer.state:set(name, 2)\nprint(x, y)",
        );
        assert_eq!(one(&e, "test"), ["UNKNOWN HANDLER UNKNOWN"]);
        assert_eq!(one(&e, "fuel"), ["ENTITY HANDLER UNKNOWN"]);
        assert_eq!(one(&e, "weather"), ["GLOBAL READ UNKNOWN"]);
        assert_eq!(one(&e, "test2"), ["PLAYER READ UNKNOWN"]);
        assert_eq!(one(&e, "raw"), ["PLAYER WRITE REPLICATED ANY"]);
        assert_eq!(one(&e, "counter"), ["PLAYER WRITE BYSIDE NUMBER"]);
        assert_eq!(one(&e, DYNAMIC), ["PLAYER WRITE BYSIDE INTEGER", "PLAYER WRITE LOCAL INTEGER"]);
    }

    #[test]
    fn values_and_edge_forms() {
        let e = entries(
            "GlobalState.a, GlobalState.b = 1\nGlobalState.c, GlobalState.d = f()\nGlobalState.e = 0x1F\nGlobalState.f = 1e3\n\
             GlobalState.g = -2\nGlobalState.h = json.encode({})\nGlobalState.i = x or 1\nGlobalState.j = 1 & 2\nGlobalState.k = `hash`\n\
             local coords = vector3(1.0, 2.0, 3.0)\nGlobalState.l = coords\nGlobalState.m.sub = 1\nGlobalState.flags |= 2\n\
             a, GlobalState.n in src\nGlobalState[1] = true\nGlobalState:set(1, 2)\nfunction GlobalState.method() end\n\
             GlobalState[ [[long]] ] = nil",
        );
        assert_eq!(one(&e, "a"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "b"), ["GLOBAL WRITE BYSIDE NIL"]);
        assert_eq!(one(&e, "d"), ["GLOBAL WRITE BYSIDE ANY"]);
        assert_eq!(one(&e, "e"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "f"), ["GLOBAL WRITE BYSIDE NUMBER"]);
        assert_eq!(one(&e, "g"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "h"), ["GLOBAL WRITE BYSIDE STRING"]);
        assert_eq!(one(&e, "i"), ["GLOBAL WRITE BYSIDE ANY"]);
        assert_eq!(one(&e, "j"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "k"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "l"), ["GLOBAL WRITE BYSIDE VECTOR3"], "the initializer of a local");
        assert_eq!(one(&e, "m"), ["GLOBAL READ UNKNOWN"]);
        assert_eq!(one(&e, "flags"), ["GLOBAL WRITE BYSIDE INTEGER"]);
        assert_eq!(one(&e, "n"), ["GLOBAL WRITE BYSIDE ANY"]);
        assert_eq!(
            one(&e, DYNAMIC),
            ["GLOBAL WRITE BYSIDE BOOLEAN"],
            "a number key is computed; `:set(1, …)` is no site"
        );
        assert!(!e.contains_key("method"));
        assert_eq!(one(&e, "long"), ["GLOBAL WRITE BYSIDE NIL"]);
    }

    #[test]
    fn plain_files_and_identifiers() {
        assert!(entries("local t = {}\nt.test = 1\nprint(t.test)").is_empty());
        assert!(!may_hold_bags("local t = {}"));
        assert!(is_identifier("test2") && is_identifier("_x") && !is_identifier("my-key") && !is_identifier("1a"));
    }

    #[test]
    fn fields_being_typed() {
        let src = Source::new("local v = LocalPlayer?.state?.\nprint(GlobalState.we)\nprint(t.x)");
        let ast = Ast::parse(&src);
        let a = Analyzer::new(&src, &ast);
        let dot = src.text.find("?.\n").unwrap() + 2;
        assert_eq!(a.field_at(dot), Some((Kind::Player, dot)));
        let we = src.text.find("we)").unwrap();
        assert_eq!(a.field_at(we + 2), Some((Kind::Global, we)));
        assert_eq!(a.field_at(src.text.find("x)").unwrap() + 1), None);
    }
}
