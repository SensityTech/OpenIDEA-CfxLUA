use super::ast::{Ast, DeclKind, ExprKind, Named, Node, StatKind};
use super::spec::{self, Spam, Spec, Work, Yield};
use crate::feature::Ctx;
use crate::lua::Source;
use crate::natives::{Native, NativeSet, Natives};
use crate::project::{Game, Side};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Callee {
    Path(String),
    Local(usize),
    Fixed(Spec),
}

#[derive(Clone, Debug, Default)]
pub struct FileFacts {
    pub functions: Vec<Vec<Callee>>,
    pub globals: Vec<(String, usize)>,
}

impl FileFacts {
    pub fn of(ast: &Ast, src: &Source) -> Self {
        let named = ast.named_functions();
        let locals = local_paths(ast, &named);
        let functions = ast
            .functions
            .iter()
            .map(|&f| {
                let ExprKind::Function { body, .. } = ast.exprs[f].kind else { return Vec::new() };
                ast.calls(Node::Block(body), &|_| false).into_iter().map(|c| callee(ast, src, &locals, c).1).collect()
            })
            .collect();
        let globals = named
            .into_iter()
            .filter(|n| n.root.is_none())
            .filter_map(|n| Some((n.path, ast.ordinal(n.func)?)))
            .collect();
        Self { functions, globals }
    }

    fn global(&self, path: &str) -> Option<usize> {
        self.globals.iter().find(|(p, _)| p == path).map(|(_, o)| *o)
    }
}

pub type LocalPaths = HashMap<(usize, String), usize>;

fn local_paths(ast: &Ast, named: &[Named]) -> LocalPaths {
    named.iter().filter_map(|n| Some(((n.root?, n.path.clone()), ast.ordinal(n.func)?))).collect()
}

#[derive(Default)]
pub struct ProjectIndex {
    files: HashMap<String, FileFacts>,
    by_name: HashMap<String, BTreeSet<String>>,
}

impl ProjectIndex {
    pub fn set(&mut self, path: &str, facts: FileFacts) {
        self.remove(path);
        for (name, _) in &facts.globals {
            self.by_name.entry(name.clone()).or_default().insert(path.to_owned());
        }
        self.files.insert(path.to_owned(), facts);
    }

    pub fn remove(&mut self, path: &str) {
        let Some(old) = self.files.remove(path) else { return };
        for (name, _) in &old.globals {
            if let Some(files) = self.by_name.get_mut(name) {
                files.remove(path);
                if files.is_empty() {
                    self.by_name.remove(name);
                }
            }
        }
    }

    fn function(&self, path: &str, except: &str) -> Option<(&str, usize)> {
        let file = self.by_name.get(path)?.iter().find(|f| f.as_str() != except)?;
        Some((file, self.files.get(file)?.global(path)?))
    }
}

pub fn callee(ast: &Ast, src: &Source, locals: &LocalPaths, call: usize) -> (Option<String>, Callee) {
    let ExprKind::Call { callee: target, method, args } = &ast.exprs[call].kind else {
        return (None, Callee::Fixed(Spec::UNKNOWN));
    };
    let Some(method) = method else { return callee_expr(ast, locals, *target, 0) };
    let path = ast.path(*target).map(|p| format!("{p}:{method}"));
    let obj = ast.unwrap(*target);
    if method == "set" && matches!(&ast.exprs[obj].kind, ExprKind::Field { name, .. } if name == "state") {
        let replicated = args.get(2).is_some_and(|&a| ast.text(src, Node::Expr(a)) == "true");
        let spec = if replicated { Spec { spam: Some(Spam::NetworkToClients), ..Spec::ACTION } } else { Spec::ACTION };
        return (path, Callee::Fixed(spec));
    }
    let (start, stop) = ast.callee_span(src, call);
    let text = &src.text[start..stop];
    if ["exports.oxmysql", "exports['oxmysql']", "exports[\"oxmysql\"]"].iter().any(|p| text.starts_with(p)) {
        return (path, Callee::Fixed(Spec { spam: Some(Spam::Database), ..Spec::UNKNOWN }));
    }
    (path, Callee::Fixed(Spec::UNKNOWN))
}

fn callee_expr(ast: &Ast, locals: &LocalPaths, e: usize, depth: usize) -> (Option<String>, Callee) {
    match &ast.exprs[e].kind {
        ExprKind::Name { name, decl: Some(d) } => {
            if let Some(alias) = alias(ast, locals, *d, depth) {
                return alias;
            }
            let decl = &ast.decls[*d];
            let function = match decl.kind {
                DeclKind::LocalFunction { .. } | DeclKind::Local { .. } => {
                    decl.init.map(|i| ast.unwrap(i)).and_then(|f| ast.ordinal(f))
                }
                _ => None,
            };
            (Some(name.clone()), function.map_or(Callee::Fixed(Spec::UNKNOWN), Callee::Local))
        }
        ExprKind::Name { name, decl: None } => (Some(name.clone()), Callee::Path(name.clone())),
        ExprKind::Field { .. } | ExprKind::Index { .. } => {
            let Some(path) = ast.path(e) else { return (None, Callee::Fixed(Spec::UNKNOWN)) };
            match ast.root_name(e).and_then(|r| ast.decl_of(r)) {
                Some(root) => {
                    let found = locals.get(&(root, path.clone())).copied();
                    (Some(path), found.map_or(Callee::Fixed(Spec::UNKNOWN), Callee::Local))
                }
                None => (Some(path.clone()), Callee::Path(path)),
            }
        }
        _ => (None, Callee::Fixed(Spec::UNKNOWN)),
    }
}

fn alias(ast: &Ast, locals: &LocalPaths, decl: usize, depth: usize) -> Option<(Option<String>, Callee)> {
    let d = &ast.decls[decl];
    if depth > 3 || !matches!(d.kind, DeclKind::Local { .. }) || !d.assigned.is_empty() {
        return None;
    }
    let value = ast.unwrap(d.init?);
    match &ast.exprs[value].kind {
        ExprKind::Name { decl: Some(v), .. } if *v == decl => None,
        ExprKind::Name { .. } | ExprKind::Field { .. } | ExprKind::Index { .. } => {
            Some(callee_expr(ast, locals, value, depth + 1))
        }
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitValue {
    Frame,
    Fixed(i64),
    Adaptive(i64),
    Unknown(String),
}

pub const PER_FRAME_MAX_MS: i64 = 10;

#[derive(Clone, Debug)]
pub struct CallInfo {
    pub call: usize,
    pub path: Option<String>,
    pub spec: Spec,
    pub wait: Option<WaitValue>,
    pub display: String,
}

impl CallInfo {
    pub fn work(&self) -> Work {
        self.spec.work
    }

    pub fn yields(&self) -> Yield {
        self.spec.yields
    }
}

#[derive(Clone)]
struct FileCx {
    file: Option<String>,
    side: Side,
    sets: Vec<NativeSet>,
    catalog: bool,
}

impl FileCx {
    fn may_run_on_client(&self) -> bool {
        self.side != Side::Server
    }
}

pub struct Perf<'a> {
    pub cx: &'a Ctx<'a>,
    pub src: &'a Source,
    pub ast: Rc<Ast>,
    pub side: Side,
    index: &'a ProjectIndex,
    detected: Vec<Game>,
    here: FileCx,
    facts: FileFacts,
    locals: LocalPaths,
    infos: RefCell<HashMap<usize, Rc<CallInfo>>>,
    globals: RefCell<HashMap<(Option<String>, String), Spec>>,
    summaries: RefCell<HashMap<FunctionKey, Option<Spec>>>,
}

type FunctionKey = (Option<String>, usize);

pub fn sets_for(cx: &Ctx, detected: &[Game], path: &str, side: Side) -> Vec<NativeSet> {
    let games = cx.ws.resource_of(path).and_then(|r| r.games.clone());
    crate::natives::visible_sets(cx.settings, detected, side, games.as_deref())
}

impl<'a> Perf<'a> {
    pub fn new(cx: &'a Ctx<'a>, index: &'a ProjectIndex) -> Self {
        let src = &cx.doc.src;
        let ast = super::ast::parse(src);
        let side = cx.doc.side;
        let detected = cx.ws.detected_games();
        let sets = sets_for(cx, &detected, &cx.doc.path, side);
        let catalog = has_catalog(cx.natives, &sets);
        let locals = local_paths(&ast, &ast.named_functions());
        let facts = FileFacts::of(&ast, src);
        Self {
            cx,
            src,
            ast,
            side,
            index,
            detected,
            here: FileCx { file: None, side, sets, catalog },
            facts,
            locals,
            infos: RefCell::default(),
            globals: RefCell::default(),
            summaries: RefCell::default(),
        }
    }

    pub fn may_run_on_client(&self) -> bool {
        self.side != Side::Server
    }

    pub fn may_run_on_server(&self) -> bool {
        self.side != Side::Client
    }

    pub fn sets(&self) -> &[NativeSet] {
        &self.here.sets
    }

    pub fn info(&self, call: usize) -> Rc<CallInfo> {
        if let Some(i) = self.infos.borrow().get(&call) {
            return i.clone();
        }
        let (path, target) = callee(&self.ast, self.src, &self.locals, call);
        let spec = self.spec_of(&target, &self.here);
        let wait = (spec.yields == Yield::Wait).then(|| self.wait_value(call));
        let display = path.clone().unwrap_or_else(|| {
            let (a, b) = self.ast.callee_span(self.src, call);
            self.src.text[a..b].to_owned()
        });
        let info = Rc::new(CallInfo { call, path, spec, wait, display });
        self.infos.borrow_mut().insert(call, info.clone());
        info
    }

    fn spec_of(&self, callee: &Callee, fcx: &FileCx) -> Spec {
        match callee {
            Callee::Fixed(spec) => *spec,
            Callee::Local(ordinal) => self.summary(fcx, *ordinal),
            Callee::Path(path) if path.contains('.') => spec::runtime(path)
                .or_else(|| self.project_function(path).map(|(f, o)| self.summary(&f, o)))
                .unwrap_or(Spec::UNKNOWN),
            Callee::Path(name) => {
                let key = (fcx.file.clone(), name.clone());
                if let Some(s) = self.globals.borrow().get(&key) {
                    return *s;
                }
                let spec = self.global(name, fcx);
                self.globals.borrow_mut().insert(key, spec);
                spec
            }
        }
    }

    fn global(&self, name: &str, fcx: &FileCx) -> Spec {
        if let Some(s) = spec::runtime(name) {
            return s;
        }
        if let Some(native) = self.cx.natives.find(name, &fcx.sets) {
            return native_spec(name, native, fcx.may_run_on_client());
        }
        if let Some((f, o)) = self.project_function(name) {
            return self.summary(&f, o);
        }
        if fcx.catalog { Spec::UNKNOWN } else { fallback_spec(name, fcx.may_run_on_client()) }
    }

    fn project_function(&self, path: &str) -> Option<(FileCx, usize)> {
        if let Some(o) = self.facts.global(path) {
            return Some((self.here.clone(), o));
        }
        let (file, ordinal) = self.index.function(path, &self.cx.doc.path)?;
        let side = self.cx.ws.side_of(file);
        let sets = sets_for(self.cx, &self.detected, file, side);
        let catalog = has_catalog(self.cx.natives, &sets);
        Some((FileCx { file: Some(file.to_owned()), side, sets, catalog }, ordinal))
    }

    fn summary(&self, fcx: &FileCx, ordinal: usize) -> Spec {
        let key = (fcx.file.clone(), ordinal);
        match self.summaries.borrow().get(&key) {
            Some(Some(spec)) => return *spec,
            Some(None) => return Spec::UNKNOWN,
            None => {}
        }
        self.summaries.borrow_mut().insert(key.clone(), None);
        let callees: Vec<Callee> = match &fcx.file {
            None => self.facts.functions.get(ordinal).cloned().unwrap_or_default(),
            Some(f) => {
                self.index.files.get(f).and_then(|facts| facts.functions.get(ordinal)).cloned().unwrap_or_default()
            }
        };
        let mut work = Work::Pure;
        let mut yields = Yield::None;
        let mut spam = None;
        for c in &callees {
            let s = self.spec_of(c, fcx);
            work = work.max(s.work);
            if s.yields != Yield::None {
                yields = Yield::Maybe;
            }
            spam = spam.or(s.spam);
        }
        let spec = Spec { work, yields, spam, ..Spec::of(work) };
        self.summaries.borrow_mut().insert(key, Some(spec));
        spec
    }

    fn wait_value(&self, call: usize) -> WaitValue {
        let ast = &self.ast;
        let Some(&arg) = ast.call_args(call).first() else { return WaitValue::Frame };
        let Some(values) = self.literal_values(arg) else {
            return WaitValue::Unknown(ast.text(self.src, Node::Expr(arg)).to_owned());
        };
        let idle: Vec<i64> = values.iter().copied().filter(|&v| v > PER_FRAME_MAX_MS).collect();
        let frame = values.iter().any(|&v| v <= PER_FRAME_MAX_MS);
        match (idle.iter().max(), frame) {
            (None, _) => WaitValue::Frame,
            (Some(&ms), false) if idle.len() == 1 => WaitValue::Fixed(ms),
            (Some(&max), _) => WaitValue::Adaptive(max),
        }
    }

    fn literal_values(&self, e: usize) -> Option<Vec<i64>> {
        let ast = &self.ast;
        if let Some(v) = self.literal(e) {
            return Some(vec![v]);
        }
        let name = ast.unwrap(e);
        let d = ast.decl_of(name)?;
        let decl = &ast.decls[d];
        if !matches!(decl.kind, DeclKind::Local { .. }) {
            return None;
        }
        let mut values = vec![self.literal(decl.init?)?];
        for &s in &decl.assigned {
            let StatKind::Assign { targets, values: rhs } = &ast.stats[s].kind else { continue };
            for (i, &t) in targets.iter().enumerate() {
                if ast.decl_of(t) == Some(d) {
                    values.push(rhs.get(i).and_then(|&v| self.literal(v))?);
                }
            }
        }
        Some(values)
    }

    fn literal(&self, e: usize) -> Option<i64> {
        let e = self.ast.unwrap(e);
        if !self.ast.is_number(e) {
            return None;
        }
        let v = lua_number(self.ast.text(self.src, Node::Expr(e)))?;
        (v >= 0.0 && v <= f64::from(i32::MAX)).then_some(v as i64)
    }
}

fn has_catalog(natives: &Natives, sets: &[NativeSet]) -> bool {
    sets.iter().any(|&s| !natives.catalog(s).is_empty())
}

fn native_spec(name: &str, native: &Native, client: bool) -> Spec {
    let query = spec::is_query_name(name);
    if client && native.set != NativeSet::Server {
        if spec::is_per_frame_name(name) || spec::is_per_frame_description(&native.description) {
            return Spec::PER_FRAME;
        }
        if query {
            return if spec::is_transient_query_name(name) { Spec::PER_FRAME } else { Spec::QUERY };
        }
        return Spec::EFFECT;
    }
    if query { Spec::QUERY } else { Spec::EFFECT }
}

fn fallback_spec(name: &str, client: bool) -> Spec {
    if !spec::looks_like_native(name) {
        return Spec::UNKNOWN;
    }
    if client && spec::is_per_frame_name(name) {
        return Spec::PER_FRAME;
    }
    if spec::is_query_name(name) {
        return if client && spec::is_transient_query_name(name) { Spec::PER_FRAME } else { Spec::QUERY };
    }
    Spec::UNKNOWN
}

pub fn lua_number(text: &str) -> Option<f64> {
    let t = text.trim();
    let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) else {
        return t.parse::<f64>().ok().filter(|v| v.is_finite());
    };
    let (mantissa, exponent) = match hex.find(['p', 'P']) {
        Some(i) => (&hex[..i], hex[i + 1..].parse::<i32>().ok()?),
        None => (hex, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let mut v = 0f64;
    for c in int.chars() {
        v = v * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0 / 16.0;
    for c in frac.chars() {
        v += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    Some(v * 2f64.powi(exponent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(lua_number("0x10"), Some(16.0));
        assert_eq!(lua_number("0x1p4"), Some(16.0));
        assert_eq!(lua_number("1e3"), Some(1000.0));
        assert_eq!(lua_number(".5"), Some(0.5));
        assert_eq!(lua_number("0x"), None);
    }

    #[test]
    fn facts_follow_aliases_and_local_tables() {
        let src = Source::new(
            "local Wait = Citizen.Wait\nlocal M = {}\nfunction M.tick() Wait(0) end\nfunction Loop() M.tick() Draw() local f = function() Hidden() end end\nUtils = { go = function() print(1) end }",
        );
        let ast = super::super::ast::parse(&src);
        let facts = FileFacts::of(&ast, &src);
        let names: Vec<&str> = facts.globals.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Loop", "Utils.go"]);
        let loop_ord = facts.global("Loop").unwrap();
        let tick = ast.named_functions().iter().find(|n| n.path == "M.tick").and_then(|n| ast.ordinal(n.func)).unwrap();
        assert_eq!(
            facts.functions[loop_ord],
            [Callee::Local(tick), Callee::Path("Draw".into())],
            "nested functions are skipped"
        );
        assert_eq!(facts.functions[tick], [Callee::Path("Citizen.Wait".into())]);
    }
}
