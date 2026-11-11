use super::ast::{Ast, DeclKind, ExprKind, Key, NONE, StmtKind, raw_string};
use super::model::{FnKind, Seg, Sources, Span, VARARG, is_standard_name, simple_name};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_ARGS: usize = 16;
const MAX_ITERATIONS: usize = 8;
const MAX_DEPTH: u32 = 12;

const SKIPPED_CALLEES: &[&str] = &[
    "print",
    "tostring",
    "tonumber",
    "type",
    "error",
    "assert",
    "pairs",
    "ipairs",
    "next",
    "select",
    "rawget",
    "rawset",
    "rawequal",
    "rawlen",
    "require",
    "setmetatable",
    "getmetatable",
    "pcall",
    "xpcall",
    "unpack",
    "load",
    "loadstring",
    "collectgarbage",
    "Wait",
    "GetHashKey",
    "joaat",
    "CreateThread",
    "SetTimeout",
    "RegisterCommand",
    "RegisterKeyMapping",
    "RegisterNUICallback",
    "SendNUIMessage",
    "SendNuiMessage",
    "AddStateBagChangeHandler",
    "GetConvar",
    "GetConvarInt",
    "LoadResourceFile",
    "SaveResourceFile",
    "GetResourceState",
    "GetCurrentResourceName",
    "vector2",
    "vector3",
    "vector4",
    "vec2",
    "vec3",
    "vec4",
    "quat",
];

const SKIPPED_NAMESPACES: &[&str] =
    &["string", "table", "math", "json", "msgpack", "os", "io", "debug", "utf8", "coroutine", "Citizen", "promise"];

const UNPACK: &[&str] = &["table.unpack", "unpack"];

const SINGLE_RESULT: &[&str] = &[
    "tonumber",
    "tostring",
    "type",
    "vector2",
    "vector3",
    "vector4",
    "vec2",
    "vec3",
    "vec4",
    "quat",
    "GetHashKey",
    "joaat",
    "json.encode",
    "json.decode",
    "string.format",
    "string.upper",
    "string.lower",
    "string.rep",
    "string.sub",
    "string.len",
    "math.floor",
    "math.ceil",
    "math.abs",
    "math.max",
    "math.min",
    "math.random",
    "math.sqrt",
    "GetGameTimer",
    "GetCurrentResourceName",
    "PlayerPedId",
    "PlayerId",
    "GetPlayerPed",
    "GetPlayerServerId",
    "GetEntityCoords",
    "GetEntityHeading",
];

const CONSTRUCTORS: &[&str] = &["new", "create", "New", "Create"];

pub const TAG: &str = "cfx.event";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    pub path: String,
    pub kind: FnKind,
    pub params: Vec<String>,
    pub vararg: bool,
    /// Which definition of `path` in the file, from 0.
    pub nth: u32,
    pub annotation: Option<String>,
}

impl FunctionInfo {
    pub fn class_name(&self) -> Option<&str> {
        if self.kind == FnKind::Method { self.path.rsplit_once(':').map(|(c, _)| c) } else { None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackCall {
    pub param: usize,
    pub closure_params: usize,
    pub closure_vararg: bool,
    pub inputs: Vec<Sources>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Str { value: String, lit: Option<Span> },
    Template(Vec<Seg>),
    Tainted(Sources),
    Closure { params: usize, vararg: bool, callback: Option<CallbackCall> },
    FunctionRef { params: usize, vararg: bool },
    Spread(Sources),
    Dynamic(String),
}

impl Arg {
    pub fn single_param(&self) -> Option<usize> {
        match self {
            Arg::Template(segs) => match segs.as_slice() {
                [Seg::Param(i)] => Some(*i),
                _ => None,
            },
            _ => None,
        }
    }

    fn alike(&self, other: &Arg) -> bool {
        match (self, other) {
            (Arg::Str { value: a, .. }, Arg::Str { value: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallRecord {
    pub callee: String,
    pub colon: bool,
    pub original: bool,
    pub offset: u32,
    pub line: u32,
    pub callee_span: Span,
    pub args: Vec<Arg>,
    pub last_arg_multi: bool,
    pub function: Option<u32>,
    pub local_receiver: bool,
    pub receiver_class: Option<String>,
}

impl CallRecord {
    pub fn simple_name(&self) -> &str {
        simple_name(&self.callee)
    }

    pub fn receiver(&self) -> Option<&str> {
        if self.colon { self.callee.rsplit_once(':').map(|(r, _)| r) } else { None }
    }

    /// Whether an argument is computed from the parameters of the function the call is in.
    pub fn forwards_params(&self) -> bool {
        self.args.iter().any(|a| matches!(a, Arg::Template(_) | Arg::Tainted(_)))
    }

    /// The same call, wherever it stands in its file.
    pub fn alike(&self, other: &CallRecord) -> bool {
        self.callee == other.callee
            && self.colon == other.colon
            && self.original == other.original
            && self.last_arg_multi == other.last_arg_multi
            && self.local_receiver == other.local_receiver
            && self.receiver_class == other.receiver_class
            && self.args.len() == other.args.len()
            && self.args.iter().zip(&other.args).all(|(a, b)| a.alike(b))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileFacts {
    pub functions: Vec<FunctionInfo>,
    pub calls: Vec<CallRecord>,
    pub by_name: HashMap<String, Vec<u32>>,
}

impl FileFacts {
    pub fn calls_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a CallRecord> + 'a {
        self.by_name.get(name).into_iter().flatten().map(|&i| &self.calls[i as usize])
    }

    pub fn function_of(&self, call: &CallRecord) -> Option<&FunctionInfo> {
        call.function.map(|i| &self.functions[i as usize])
    }
}

// ---------------------------------------------------------------------------------------------
// abstract values

#[derive(Clone, Debug, PartialEq, Eq)]
enum V {
    Lit(String),
    Tmpl(Vec<Seg>),
    Taint(Sources),
    None,
}

fn sources(v: &V) -> Sources {
    match v {
        V::Tmpl(segs) => {
            segs.iter().filter_map(|s| if let Seg::Param(i) = s { Some(*i as i32) } else { None }).collect()
        }
        V::Taint(s) => s.clone(),
        _ => Sources::new(),
    }
}

fn taint(s: Sources) -> V {
    if s.is_empty() { V::None } else { V::Taint(s) }
}

fn is_pure(v: &V) -> bool {
    matches!(v, V::Lit(_) | V::Tmpl(_))
}

fn segs_of(v: V) -> Vec<Seg> {
    match v {
        V::Lit(t) => vec![Seg::Lit(t)],
        V::Tmpl(s) => s,
        _ => Vec::new(),
    }
}

pub fn merge_literals(segs: Vec<Seg>) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    for seg in segs {
        match (out.last_mut(), seg) {
            (Some(Seg::Lit(last)), Seg::Lit(t)) => last.push_str(&t),
            (_, seg) => out.push(seg),
        }
    }
    out
}

fn concat(left: V, right: V) -> V {
    if !is_pure(&left) || !is_pure(&right) {
        let mut s = sources(&left);
        s.extend(sources(&right));
        return taint(s);
    }
    let mut segs = segs_of(left);
    segs.extend(segs_of(right));
    let segs = merge_literals(segs);
    match segs.as_slice() {
        [Seg::Lit(t)] => V::Lit(t.clone()),
        _ => V::Tmpl(segs),
    }
}

#[derive(Clone, Copy)]
struct Ev<'a> {
    src: &'a Source,
    ast: &'a Ast,
    file: Option<&'a Env>,
}

struct Env {
    root: u32,
    params: HashMap<u32, usize>,
    bindings: Vec<(u32, u32)>,
    defs: HashMap<u32, Vec<Option<u32>>>,
    impure: HashSet<u32>,
    env: HashMap<u32, Sources>,
}

fn value_at(ast: &Ast, values: &[u32], i: usize) -> Option<u32> {
    values.get(i).copied().or_else(|| {
        let last = *values.last()?;
        (i >= values.len() && matches!(ast.kind(last), ExprKind::Call(..) | ExprKind::Vararg)).then_some(last)
    })
}

fn root_decl(ast: &Ast, e: u32) -> u32 {
    let e = ast.unparen(e);
    match ast.kind(e) {
        ExprKind::Name(d) => *d,
        ExprKind::Index(obj, _) => root_decl(ast, *obj),
        _ => NONE,
    }
}

impl Env {
    fn new(ev: &Ev, root: u32) -> Self {
        let params = if root == NONE {
            HashMap::new()
        } else {
            ev.ast.func(root).params.iter().enumerate().map(|(i, &d)| (d, i)).collect()
        };
        let mut env = Self {
            root,
            params,
            bindings: Vec::new(),
            defs: HashMap::new(),
            impure: HashSet::new(),
            env: HashMap::new(),
        };
        env.scan(ev);
        env.solve(ev);
        env
    }

    fn bind(&mut self, target: u32, value: Option<u32>, pure: bool) {
        if target == NONE {
            return;
        }
        self.defs.entry(target).or_default().push(value);
        if !pure {
            self.impure.insert(target);
        }
        if let Some(v) = value {
            self.bindings.push((target, v));
        }
    }

    fn knows(&self, decl: u32) -> bool {
        self.defs.contains_key(&decl)
    }

    fn single_def(&self, decl: u32) -> Option<u32> {
        if self.impure.contains(&decl) {
            return None;
        }
        match self.defs.get(&decl)?.as_slice() {
            [v] => *v,
            _ => None,
        }
    }

    fn scan(&mut self, ev: &Ev) {
        let ast = ev.ast;
        let ((e0, e1), (s0, s1)) = if self.root == NONE {
            ((0, ast.exprs.len() as u32), (0, ast.stmts.len() as u32))
        } else {
            let f = ast.func(self.root);
            (f.exprs, f.stmts)
        };
        for s in s0..s1 {
            match &ast.stmts[s as usize].kind {
                StmtKind::Local { names, values, unpack } => match unpack {
                    Some(u) => names.iter().for_each(|&n| self.bind(n, Some(*u), false)),
                    None => {
                        for (i, &n) in names.iter().enumerate() {
                            self.bind(n, value_at(ast, values, i), i < values.len());
                        }
                    }
                },
                StmtKind::Assign { targets, values, unpack } => {
                    for (i, &t) in targets.iter().enumerate() {
                        let value = if unpack.is_some() { *unpack } else { value_at(ast, values, i) };
                        match ast.kind(t) {
                            ExprKind::Name(d) => self.bind(*d, value, unpack.is_none() && i < values.len()),
                            _ => self.bind(root_decl(ast, t), value, false),
                        }
                    }
                }
                StmtKind::Compound { target, value, .. } => self.bind(root_decl(ast, *target), *value, false),
                StmtKind::NumFor { var, exprs } => exprs.iter().for_each(|&e| self.bind(*var, Some(e), false)),
                StmtKind::GenFor { vars, exprs } => {
                    for &v in vars {
                        exprs.iter().for_each(|&e| self.bind(v, Some(e), false));
                    }
                }
                _ => {}
            }
        }
        for e in e0..e1 {
            let ExprKind::Call(callee, args) = ast.kind(e) else { continue };
            if let Some(f) = self.called_function(ev, e) {
                // parameters of local functions and immediately invoked closures hold their arguments
                for (i, &p) in ast.func(f).params.iter().enumerate() {
                    self.bind(p, value_at(ast, args, i), true);
                }
            } else if args.len() >= 2 && ast.path(ev.src, *callee).as_deref() == Some("table.insert") {
                self.bind(root_decl(ast, args[0]), args.last().copied(), false);
            }
        }
    }

    fn solve(&mut self, ev: &Ev) {
        for _ in 0..MAX_ITERATIONS {
            let mut changed = false;
            for k in 0..self.bindings.len() {
                let (target, value) = self.bindings[k];
                let found = sources(&self.eval(ev, Some(value), 0));
                if found.is_empty() {
                    continue;
                }
                let set = self.env.entry(target).or_default();
                let before = set.len();
                set.extend(found);
                changed |= set.len() != before;
            }
            if !changed {
                return;
            }
        }
    }

    fn called_function(&self, ev: &Ev, call: u32) -> Option<u32> {
        let ast = ev.ast;
        let ExprKind::Call(callee, _) = ast.kind(call) else { return None };
        let callee = ast.unparen(*callee);
        match ast.kind(callee) {
            ExprKind::Func(f) => Some(*f),
            ExprKind::Name(d) if *d != NONE => match ast.decl(*d).kind {
                DeclKind::LocalFunction(f) if f != NONE => Some(f),
                DeclKind::Local { .. } => {
                    let value = match ev.file {
                        Some(file) if !self.knows(*d) => file.single_def(*d),
                        _ => self.single_def(*d),
                    }?;
                    match ast.kind(ast.unparen(value)) {
                        ExprKind::Func(f) => Some(*f),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn eval(&self, ev: &Ev, e: Option<u32>, depth: u32) -> V {
        let Some(e) = e else { return V::None };
        if depth > MAX_DEPTH {
            return V::None;
        }
        let ast = ev.ast;
        match ast.kind(e) {
            ExprKind::Paren(inner) => self.eval(ev, Some(*inner), depth + 1),
            ExprKind::Str => V::Lit(raw_string(ev.src, ast.expr(e).first as usize)),
            ExprKind::Vararg => {
                if ast.expr(e).func == self.root {
                    V::Taint(Sources::from([VARARG]))
                } else {
                    V::None
                }
            }
            ExprKind::Name(d) => self.eval_name(ev, *d, depth),
            ExprKind::Binary(op, l, r) => {
                let (l, r) = (self.eval(ev, Some(*l), depth + 1), self.eval(ev, Some(*r), depth + 1));
                if *op == super::ast::BinOp::Concat {
                    concat(l, r)
                } else {
                    let mut s = sources(&l);
                    s.extend(sources(&r));
                    taint(s)
                }
            }
            ExprKind::Unary(_, x) => taint(sources(&self.eval(ev, Some(*x), depth + 1))),
            ExprKind::Index(obj, key) => {
                let mut s = sources(&self.eval(ev, Some(*obj), depth + 1));
                if let Key::Bracket(Some(k)) = key {
                    s.extend(sources(&self.eval(ev, Some(*k), depth + 1)));
                }
                taint(s)
            }
            ExprKind::Call(..) => self.eval_call(ev, e, depth),
            ExprKind::Table(fields) => {
                let mut s = Sources::new();
                for f in fields {
                    if let Some(k) = f.key {
                        s.extend(sources(&self.eval(ev, Some(k), depth + 1)));
                    }
                    s.extend(sources(&self.eval(ev, Some(f.value), depth + 1)));
                }
                taint(s)
            }
            _ => V::None,
        }
    }

    fn eval_name(&self, ev: &Ev, decl: u32, depth: u32) -> V {
        if decl == NONE {
            return V::None;
        }
        if let Some(&index) = self.params.get(&decl) {
            if !self.knows(decl) {
                return V::Tmpl(vec![Seg::Param(index)]);
            }
            let mut s = self.env.get(&decl).cloned().unwrap_or_default();
            s.insert(index as i32);
            return taint(s);
        }
        if self.knows(decl) {
            if let Some(value) = self.single_def(decl) {
                let single = self.eval(ev, Some(value), depth + 1);
                if is_pure(&single) {
                    return single;
                }
            }
            return taint(self.env.get(&decl).cloned().unwrap_or_default());
        }
        // a constant declared elsewhere in the file (`local PREFIX = 'shop:'`)
        match ev.file {
            Some(file) => file.constant(ev, decl, depth),
            None => V::None,
        }
    }

    fn constant(&self, ev: &Ev, decl: u32, depth: u32) -> V {
        let Some(value) = self.single_def(decl) else { return V::None };
        match self.eval(&Ev { file: None, ..*ev }, Some(value), depth + 1) {
            lit @ V::Lit(_) => lit,
            _ => V::None,
        }
    }

    fn eval_call(&self, ev: &Ev, call: u32, depth: u32) -> V {
        let ast = ev.ast;
        let ExprKind::Call(callee, args) = ast.kind(call) else { return V::None };
        if args.len() == 1 && ast.path(ev.src, *callee).as_deref() == Some("tostring") {
            return self.eval(ev, Some(args[0]), depth + 1);
        }
        let mut s = Sources::new();
        for &a in args {
            s.extend(sources(&self.eval(ev, Some(a), depth + 1)));
        }
        // the receiver of `eType:upper()` and the table of `self.encode(x)`
        if let ExprKind::Index(obj, _) = ast.kind(*callee) {
            s.extend(sources(&self.eval(ev, Some(*obj), depth + 1)));
        }
        if let Some(f) = self.called_function(ev, call)
            && ast.within(f, self.root)
        {
            let returns: Vec<V> = return_values(ast, f).map(|r| self.eval(ev, Some(r), depth + 1)).collect();
            if let Some(first) = returns.first()
                && is_pure(first)
                && returns.iter().all(|r| r == first)
            {
                return first.clone();
            }
            for r in &returns {
                s.extend(sources(r));
            }
        }
        taint(s)
    }
}

fn return_values(ast: &Ast, f: u32) -> impl Iterator<Item = u32> + '_ {
    let (s0, s1) = ast.func(f).stmts;
    (s0..s1).filter_map(move |s| {
        let stmt = &ast.stmts[s as usize];
        match &stmt.kind {
            StmtKind::Return(values) if stmt.func == f => values.first().copied(),
            _ => None,
        }
    })
}

fn calls_in(ast: &Ast, lo: u32, hi: u32) -> Vec<u32> {
    let mut calls: Vec<u32> = (lo..hi).filter(|&e| matches!(ast.kind(e), ExprKind::Call(..))).collect();
    calls.sort_by_key(|&e| (ast.expr(e).first, Reverse(ast.expr(e).last)));
    calls
}

// ---------------------------------------------------------------------------------------------
// doc comments

pub fn doc_block(src: &Source, tok: usize) -> Vec<usize> {
    let Some(t) = src.toks.get(tok) else { return Vec::new() };
    let mut j = src.all.partition_point(|a| a.start < t.start);
    let mut line = src.lines.line_of(t.start);
    let mut out = Vec::new();
    while j > 0 {
        let c = src.all[j - 1];
        if c.kind != Tok::LineComment || !c.text(&src.text).starts_with("---") {
            break;
        }
        let l = src.lines.line_of(c.start);
        if l + 1 != line {
            break;
        }
        out.push(j - 1);
        line = l;
        j -= 1;
    }
    out.reverse();
    out
}

pub fn event_tag(src: &Source, i: usize) -> Option<(usize, usize, String)> {
    let c = src.all.get(i)?;
    let text = c.text(&src.text);
    let body = text.strip_prefix("---")?;
    let lead = body.len() - body.trim_start().len();
    let rest = body[lead..].strip_prefix('@')?;
    let name_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if &rest[..name_len] != TAG {
        return None;
    }
    let start = c.start + 3 + lead + 1;
    Some((start, c.start + text.trim_end().len(), rest[name_len..].trim().to_owned()))
}

pub fn annotation(src: &Source, tok: usize) -> Option<String> {
    doc_block(src, tok).into_iter().find_map(|i| event_tag(src, i)).map(|(_, _, text)| text)
}

// ---------------------------------------------------------------------------------------------
// the analysis of a file

struct Callee {
    path: String,
    colon: bool,
    original: bool,
    tok: u32,
    local_receiver: bool,
    receiver_class: Option<String>,
}

pub struct Analysis<'a> {
    src: &'a Source,
    ast: &'a Ast,
    file_env: Env,
    named: Vec<Option<u32>>,
    named_func: Vec<u32>,
    functions: Vec<FunctionInfo>,
    envs: RefCell<HashMap<u32, Rc<Env>>>,
    globals: HashMap<&'a str, u32>,
}

impl<'a> Analysis<'a> {
    pub fn new(src: &'a Source, ast: &'a Ast) -> Self {
        let file_env = Env::new(&Ev { src, ast, file: None }, NONE);
        let mut a = Self {
            src,
            ast,
            file_env,
            named: vec![None; ast.funcs.len()],
            named_func: Vec::new(),
            functions: Vec::new(),
            envs: RefCell::new(HashMap::new()),
            globals: HashMap::new(),
        };
        a.collect_named();
        let mut seen: HashMap<&'a str, Option<u32>> = HashMap::new();
        for s in &ast.stmts {
            if let StmtKind::Function { target: Some(t), func } = &s.kind
                && let Some(name) = ast.name(src, *t)
            {
                seen.entry(name).and_modify(|f| *f = None).or_insert(Some(*func));
            }
        }
        a.globals = seen.into_iter().filter_map(|(n, f)| Some((n, f?))).collect();
        a
    }

    pub fn facts(&self) -> FileFacts {
        let calls: Vec<CallRecord> = calls_in(self.ast, 0, self.ast.exprs.len() as u32)
            .into_iter()
            .filter_map(|c| self.record(c, true))
            .collect();
        let mut by_name: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, c) in calls.iter().enumerate() {
            by_name.entry(c.simple_name().to_owned()).or_default().push(i as u32);
        }
        FileFacts { functions: self.functions.clone(), calls, by_name }
    }

    pub fn functions(&self) -> &[FunctionInfo] {
        &self.functions
    }

    fn ev(&self, env: &Env) -> Ev<'_> {
        Ev { src: self.src, ast: self.ast, file: (env.root != NONE).then_some(&self.file_env) }
    }

    fn add_named(&mut self, func: u32, path: String, kind: FnKind, owner: u32) {
        let f = self.ast.func(func);
        let info = FunctionInfo {
            path,
            kind,
            params: f.params.iter().map(|&d| self.src.tok_text(self.ast.decl(d).tok as usize).to_owned()).collect(),
            vararg: f.vararg,
            nth: 0,
            annotation: annotation(self.src, owner as usize),
        };
        self.named[func as usize] = Some(self.functions.len() as u32);
        self.named_func.push(func);
        self.functions.push(info);
    }

    fn add_table_fields(&mut self, prefix: &str, table: u32) {
        let ast = self.ast;
        let ExprKind::Table(fields) = ast.kind(table) else { return };
        for f in fields {
            if let (Some(name), ExprKind::Func(func)) = (f.name, ast.kind(f.value)) {
                let path = format!("{prefix}.{}", self.src.tok_text(name as usize));
                self.add_named(*func, path, FnKind::Field, f.first);
            }
        }
    }

    fn collect_named(&mut self) {
        let (ast, src) = (self.ast, self.src);
        let top = |f: u32| ast.func(f).parent == NONE;
        for stmt in &ast.stmts {
            match &stmt.kind {
                StmtKind::Function { target: Some(t), func } => match ast.kind(*t) {
                    ExprKind::Name(_) => {
                        let tok = ast.expr(*t).first;
                        self.add_named(*func, src.tok_text(tok as usize).to_owned(), FnKind::Global, stmt.first);
                    }
                    ExprKind::Index(obj, Key::Field(Some(n)) | Key::Method(Some(n))) => {
                        let Some(prefix) = ast.path(src, *obj) else { continue };
                        let method = matches!(ast.kind(*t), ExprKind::Index(_, Key::Method(_)));
                        let (sep, kind) = if method { (':', FnKind::Method) } else { ('.', FnKind::Field) };
                        let path = format!("{prefix}{sep}{}", src.tok_text(*n as usize));
                        self.add_named(*func, path, kind, stmt.first);
                    }
                    _ => {}
                },
                StmtKind::LocalFunction { decl, func } if top(*func) => {
                    let tok = ast.decl(*decl).tok;
                    self.add_named(*func, src.tok_text(tok as usize).to_owned(), FnKind::Local, stmt.first);
                }
                StmtKind::Local { names, values, .. } => {
                    for (&v, &n) in values.iter().zip(names) {
                        let tok = ast.decl(n).tok;
                        let name = src.tok_text(tok as usize);
                        match ast.kind(v) {
                            ExprKind::Func(f) if top(*f) => {
                                self.add_named(*f, name.to_owned(), FnKind::Local, stmt.first)
                            }
                            ExprKind::Table(_) => self.add_table_fields(name, v),
                            _ => {}
                        }
                    }
                }
                StmtKind::Assign { targets, values, .. } => {
                    for (&v, &t) in values.iter().zip(targets) {
                        match ast.kind(v) {
                            ExprKind::Func(f) => {
                                let named = match ast.kind(t) {
                                    ExprKind::Name(d) if *d != NONE => top(*f).then_some(FnKind::Local),
                                    ExprKind::Name(_) => Some(FnKind::Global),
                                    _ => Some(FnKind::Field),
                                };
                                if let (Some(kind), Some(path)) = (named, ast.path(src, t)) {
                                    self.add_named(*f, path, kind, stmt.first);
                                }
                            }
                            ExprKind::Table(_) => {
                                if let Some(path) = ast.path(src, t) {
                                    self.add_table_fields(&path, v);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        let mut defined: HashMap<&str, u32> = HashMap::new();
        let nths: Vec<u32> = self
            .functions
            .iter()
            .map(|f| {
                let n = defined.entry(f.path.as_str()).or_default();
                *n += 1;
                *n - 1
            })
            .collect();
        for (f, nth) in self.functions.iter_mut().zip(nths) {
            f.nth = nth;
        }
    }

    fn enclosing_named(&self, mut f: u32) -> Option<u32> {
        while f != NONE {
            if let Some(i) = self.named[f as usize] {
                return Some(i);
            }
            f = self.ast.func(f).parent;
        }
        None
    }

    fn env_of(&self, info: u32) -> Rc<Env> {
        let func = self.named_func[info as usize];
        if let Some(env) = self.envs.borrow().get(&func) {
            return env.clone();
        }
        let env = Rc::new(Env::new(&Ev { src: self.src, ast: self.ast, file: Some(&self.file_env) }, func));
        self.envs.borrow_mut().insert(func, env.clone());
        env
    }

    fn saved_runtime_function(&self, decl: u32) -> Option<String> {
        let value = self.ast.unparen(self.file_env.single_def(decl)?);
        let name = self.ast.global(self.src, value)?;
        is_standard_name(name).then(|| name.to_owned())
    }

    fn receiver_class(&self, receiver: u32) -> Option<String> {
        let (ast, src) = (self.ast, self.src);
        let decl = ast.decl_of(ast.unparen(receiver));
        if decl == NONE {
            return None;
        }
        let value = ast.unparen(self.file_env.single_def(decl)?);
        let ExprKind::Call(callee, args) = ast.kind(value) else { return None };
        if ast.path(src, *callee).as_deref() == Some("setmetatable") {
            return ast.global(src, ast.unparen(*args.get(1)?)).map(str::to_owned);
        }
        let ExprKind::Index(owner, Key::Field(Some(n)) | Key::Method(Some(n))) = ast.kind(*callee) else { return None };
        if !CONSTRUCTORS.contains(&src.tok_text(*n as usize)) {
            return None;
        }
        ast.global(src, ast.unparen(*owner)).map(str::to_owned)
    }

    fn local_root(&self, mut e: u32) -> bool {
        loop {
            match self.ast.kind(e) {
                ExprKind::Name(d) => return *d != NONE,
                ExprKind::Index(obj, _) => e = *obj,
                _ => return false,
            }
        }
    }

    fn callee_of(&self, callee: u32) -> Option<Callee> {
        let (ast, src) = (self.ast, self.src);
        match ast.kind(callee) {
            ExprKind::Name(d) => {
                let tok = ast.expr(callee).first;
                let saved = if *d == NONE { None } else { self.saved_runtime_function(*d) };
                let (path, original) = match saved {
                    Some(name) => (name, true),
                    None => (src.tok_text(tok as usize).to_owned(), false),
                };
                Some(Callee { path, colon: false, original, tok, local_receiver: false, receiver_class: None })
            }
            ExprKind::Index(obj, Key::Field(Some(n))) => {
                let prefix = ast.path(src, *obj)?;
                Some(Callee {
                    path: format!("{prefix}.{}", src.tok_text(*n as usize)),
                    colon: false,
                    original: false,
                    tok: *n,
                    local_receiver: self.local_root(callee),
                    receiver_class: None,
                })
            }
            ExprKind::Index(obj, Key::Method(Some(n))) => Some(Callee {
                path: format!("{}:{}", ast.path(src, *obj).as_deref().unwrap_or("?"), src.tok_text(*n as usize)),
                colon: true,
                original: false,
                tok: *n,
                local_receiver: self.local_root(callee),
                receiver_class: self.receiver_class(*obj),
            }),
            _ => None,
        }
    }

    pub fn record(&self, call: u32, filter: bool) -> Option<CallRecord> {
        let (ast, src) = (self.ast, self.src);
        let ExprKind::Call(callee_expr, arg_exprs) = ast.kind(call) else { return None };
        let callee = self.callee_of(*callee_expr)?;
        if filter && !callee.original && is_skipped(&callee.path) {
            return None;
        }
        let function = self.enclosing_named(ast.expr(call).func);
        let env = function.map(|i| self.env_of(i));
        let env: &Env = env.as_deref().unwrap_or(&self.file_env);
        let n = arg_exprs.len().min(MAX_ARGS);
        let args: Vec<Arg> =
            arg_exprs[..n].iter().enumerate().map(|(i, &a)| self.arg_of(a, i + 1 == n, env, function)).collect();
        let last_arg_multi = n > 0 && {
            match ast.kind(arg_exprs[n - 1]) {
                ExprKind::Call(c, _) => ast
                    .path(src, *c)
                    .is_none_or(|p| !UNPACK.contains(&p.as_str()) && !SINGLE_RESULT.contains(&p.as_str())),
                _ => false,
            }
        };
        if filter && !is_relevant(&args, function.is_some(), &callee.path) {
            return None;
        }
        let start = ast.start(src, call);
        let tok = src.toks[callee.tok as usize];
        Some(CallRecord {
            callee: callee.path,
            colon: callee.colon,
            original: callee.original,
            offset: start as u32,
            line: src.lines.line_of(start) as u32,
            callee_span: Span::of(src, tok.start, tok.end),
            args,
            last_arg_multi,
            function,
            local_receiver: callee.local_receiver,
            receiver_class: callee.receiver_class,
        })
    }

    fn arg_of(&self, a: u32, is_last: bool, env: &Env, function: Option<u32>) -> Arg {
        let (ast, src) = (self.ast, self.src);
        let ev = self.ev(env);
        match ast.kind(a) {
            ExprKind::Str => {
                let tok = src.toks[ast.expr(a).first as usize];
                return Arg::Str {
                    value: raw_string(src, ast.expr(a).first as usize),
                    lit: Some(Span::of(src, tok.start, tok.end)),
                };
            }
            ExprKind::Vararg if is_last => return Arg::Spread(sources(&env.eval(&ev, Some(a), 0))),
            ExprKind::Func(f) => {
                let func = ast.func(*f);
                let callback = function.and_then(|fi| self.callback_of(*f, fi));
                return Arg::Closure { params: func.params.len(), vararg: func.vararg, callback };
            }
            ExprKind::Call(c, inner) if is_last && ast.path(src, *c).is_some_and(|p| UNPACK.contains(&p.as_str())) => {
                return Arg::Spread(sources(&env.eval(&ev, inner.first().copied(), 0)));
            }
            ExprKind::Name(_) => {
                if let Some(r) = self.function_ref(a) {
                    return r;
                }
            }
            _ => {}
        }
        match env.eval(&ev, Some(a), 0) {
            V::Lit(value) => Arg::Str { value, lit: None },
            V::Tmpl(segs) => Arg::Template(segs),
            V::Taint(s) => Arg::Tainted(s),
            V::None => Arg::Dynamic(self.literal_prefix(a, env)),
        }
    }

    fn function_ref(&self, name: u32) -> Option<Arg> {
        let ast = self.ast;
        let decl = ast.decl_of(name);
        let f = if decl == NONE {
            *self.globals.get(ast.name(self.src, name)?)?
        } else {
            match ast.decl(decl).kind {
                DeclKind::LocalFunction(f) if f != NONE => f,
                DeclKind::Local { .. } => match ast.kind(ast.unparen(self.file_env.single_def(decl)?)) {
                    ExprKind::Func(f) => *f,
                    _ => return None,
                },
                _ => return None,
            }
        };
        let func = ast.func(f);
        Some(Arg::FunctionRef { params: func.params.len(), vararg: func.vararg })
    }

    fn literal_prefix(&self, e: u32, env: &Env) -> String {
        fn flatten(ast: &Ast, e: u32, out: &mut Vec<u32>) {
            let e = ast.unparen(e);
            match ast.kind(e) {
                ExprKind::Binary(super::ast::BinOp::Concat, l, r) => {
                    flatten(ast, *l, out);
                    flatten(ast, *r, out);
                }
                _ => out.push(e),
            }
        }
        let mut operands = Vec::new();
        flatten(self.ast, e, &mut operands);
        if operands.len() < 2 {
            return String::new();
        }
        let ev = self.ev(env);
        let mut prefix = String::new();
        for o in operands {
            match env.eval(&ev, Some(o), 0) {
                V::Lit(t) => prefix.push_str(&t),
                _ => break,
            }
        }
        prefix
    }

    fn callback_of(&self, closure: u32, function: u32) -> Option<CallbackCall> {
        let ast = self.ast;
        let fenv = self.env_of(function);
        let (lo, hi) = ast.func(closure).exprs;
        for call in calls_in(ast, lo, hi) {
            let ExprKind::Call(callee, args) = ast.kind(call) else { continue };
            let ExprKind::Name(d) = ast.kind(*callee) else { continue };
            let Some(&param) = fenv.params.get(d) else { continue };
            let ev = Ev { src: self.src, ast, file: Some(&self.file_env) };
            let cenv = Env::new(&ev, closure);
            let inputs = args.iter().map(|&a| sources(&cenv.eval(&ev, Some(a), 0))).collect();
            let c = ast.func(closure);
            return Some(CallbackCall { param, closure_params: c.params.len(), closure_vararg: c.vararg, inputs });
        }
        None
    }
}

fn is_skipped(path: &str) -> bool {
    SKIPPED_CALLEES.contains(&path) || path.split_once('.').is_some_and(|(ns, _)| SKIPPED_NAMESPACES.contains(&ns))
}

fn is_relevant(args: &[Arg], in_function: bool, path: &str) -> bool {
    is_standard_name(simple_name(path))
        || args.iter().any(|a| match a {
            Arg::Str { .. } | Arg::Closure { .. } => true,
            Arg::Template(_) | Arg::Tainted(_) => in_function,
            Arg::Spread(s) => in_function && s.contains(&VARARG),
            _ => false,
        })
}

pub fn facts(src: &Source) -> FileFacts {
    let ast = super::ast::parse(src);
    Analysis::new(src, &ast).facts()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis_of(text: &str) -> FileFacts {
        facts(&Source::new(text))
    }

    fn call<'a>(f: &'a FileFacts, callee: &str) -> &'a CallRecord {
        f.calls.iter().find(|c| c.callee == callee).unwrap_or_else(|| panic!("no call {callee}: {:?}", f.calls))
    }

    fn set(v: &[i32]) -> Sources {
        v.iter().copied().collect()
    }

    #[test]
    fn named_functions() {
        let f = analysis_of(
            "function G(a, ...) end\nfunction A.b.c() end\nfunction A.b:m(x) end\nlocal function l() end\n\
             H = function() end\nT.f = function() end\nlocal v = function() end\nM = { k = function(p) end }\n\
             local N = { j = function() end }\nfunction outer() local function inner() end end",
        );
        let paths: Vec<(&str, FnKind)> = f.functions.iter().map(|i| (i.path.as_str(), i.kind)).collect();
        for expected in [
            ("G", FnKind::Global),
            ("A.b.c", FnKind::Field),
            ("A.b:m", FnKind::Method),
            ("l", FnKind::Local),
            ("H", FnKind::Global),
            ("T.f", FnKind::Field),
            ("v", FnKind::Local),
            ("M.k", FnKind::Field),
            ("N.j", FnKind::Field),
            ("outer", FnKind::Global),
        ] {
            assert!(paths.contains(&expected), "{expected:?} in {paths:?}");
        }
        assert!(!paths.iter().any(|(p, _)| *p == "inner"), "nested locals are anonymous");
        let g = f.functions.iter().find(|i| i.path == "G").unwrap();
        assert_eq!((g.params.clone(), g.vararg), (vec!["a".to_owned()], true));
        assert_eq!(f.functions.iter().find(|i| i.path == "A.b:m").unwrap().class_name(), Some("A.b"));
    }

    #[test]
    fn obfuscated_wire_through_an_iife_and_wrapped_callback() {
        let f = analysis_of(
            "function MyHelper.AddEventHandler(eType, eName, callback, requiredInit)\n\
             local wire = (function()\n\
                 local encType, encName = EncodeEventPart(eType), EncodeEventPart(eName)\n\
                 if IS_DEV then encType = eType encName = eName end\n\
                 return '404' .. encType .. encName\n\
             end)()\n\
             RegisterNetEvent(wire)\n\
             AddEventHandler(wire, function(...)\n\
                 local args = { ... }\n\
                 callback(table.unpack(args))\n\
             end)\n\
             end",
        );
        let reg = call(&f, "RegisterNetEvent");
        assert_eq!(reg.args, vec![Arg::Tainted(set(&[0, 1]))]);
        let add = call(&f, "AddEventHandler");
        let Arg::Closure { params: 0, vararg: true, callback: Some(cb) } = &add.args[1] else {
            panic!("{:?}", add.args)
        };
        assert_eq!((cb.param, cb.inputs.clone()), (2, vec![set(&[VARARG])]));
        assert_eq!(f.function_of(add).map(|i| i.path.as_str()), Some("MyHelper.AddEventHandler"));
    }

    #[test]
    fn templates_constants_saved_runtime_functions_and_receivers() {
        let f = analysis_of(
            "local PREFIX = 'shop:'\nlocal _Trigger = TriggerServerEvent\n\
             function Events.Send(category, name, ...)\n\
                 TriggerServerEvent('evt:' .. category .. ':' .. name, ...)\n\
                 _Trigger(PREFIX, 1)\n\
             end\n\
             local zone = Zone.new('x')\nzone:TriggerEvent('a', 'b')\n\
             local p = setmetatable({}, Player)\np:Go('x')\n\
             TriggerServerEvent('shop:' .. GetCurrentResourceName())\nTriggerServerEvent(GetValues())",
        );
        let send =
            f.calls.iter().find(|c| c.callee == "TriggerServerEvent" && !c.original && c.function.is_some()).unwrap();
        assert_eq!(
            send.args[0],
            Arg::Template(vec![Seg::Lit("evt:".into()), Seg::Param(0), Seg::Lit(":".into()), Seg::Param(1)])
        );
        assert_eq!(send.args[1], Arg::Spread(set(&[VARARG])));
        let saved = f.calls.iter().find(|c| c.original).unwrap();
        assert_eq!(saved.callee, "TriggerServerEvent");
        assert_eq!(saved.args[0], Arg::Str { value: "shop:".into(), lit: None });
        assert_eq!(call(&f, "zone:TriggerEvent").receiver_class.as_deref(), Some("Zone"));
        assert!(call(&f, "zone:TriggerEvent").local_receiver);
        assert_eq!(call(&f, "p:Go").receiver_class.as_deref(), Some("Player"));
        let dynamic =
            f.calls.iter().filter(|c| c.callee == "TriggerServerEvent" && c.function.is_none() && !c.original);
        let args: Vec<&Arg> = dynamic.map(|c| &c.args[0]).collect();
        assert_eq!(args[0], &Arg::Dynamic("shop:".into()));
        assert_eq!(args[1], &Arg::Dynamic(String::new()));
        assert!(f.calls.iter().rev().find(|c| c.callee == "TriggerServerEvent").unwrap().last_arg_multi);
    }

    #[test]
    fn wrapped_callbacks_with_injected_values() {
        let f = analysis_of(
            "function SrvHelper.AddHouseEventHandler(eType, eName, rateLimit, onlyOwner, callback)\n\
             SrvHelper.AddEventHandler(eType, eName, function(actor, ...)\n\
                 local house = actor.house\n\
                 callback(actor, house, ...)\n\
             end)\n\
             end",
        );
        let c = call(&f, "SrvHelper.AddEventHandler");
        assert_eq!(c.args[0], Arg::Template(vec![Seg::Param(0)]));
        let Arg::Closure { callback: Some(cb), .. } = &c.args[2] else { panic!() };
        assert_eq!((cb.param, cb.closure_params), (4, 1));
        assert_eq!(cb.inputs, vec![set(&[0]), set(&[0]), set(&[VARARG])]);
    }

    #[test]
    fn index_filter() {
        let f = analysis_of(
            "print('x')\nstring.format('%d', 1)\nfoo(1)\nbar('x')\nAddEventHandler(name)\nlocal function h(n) baz(n) end",
        );
        let callees: Vec<&str> = f.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["bar", "AddEventHandler", "baz"]);
    }

    #[test]
    fn annotations() {
        let f = analysis_of(
            "---Listens.\n---@cfx.event handler server key(eType, eName) callback(cb)\nfunction S.On(eType, eName, cb) end\n\n\
             ---@cfx.event trigger\n\nfunction S.Off() end",
        );
        let on = f.functions.iter().find(|i| i.path == "S.On").unwrap();
        assert_eq!(on.annotation.as_deref(), Some("handler server key(eType, eName) callback(cb)"));
        assert_eq!(f.functions.iter().find(|i| i.path == "S.Off").unwrap().annotation, None, "a blank line detaches");
    }
}
