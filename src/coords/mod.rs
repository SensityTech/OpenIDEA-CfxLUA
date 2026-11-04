pub mod checks;
pub mod expr;
pub mod paste;
pub mod text;
pub mod zones;

use crate::feature::{CommandCx, CommandResult, Ctx, Feature, Fix, Intention, Problem};
use crate::lua::Source;
use crate::natives::NativeSet;
use crate::perf::ast::{self, Ast, DeclKind, ExprKind, Node};
use crate::project::FileKind;
use crate::workspace::Workspace;
use expr::{Coord, Form, analyze, constructor_for, decimals, find_at, is_heading_name, is_xyz_at, nodes_at, round};
use openidea_plugin::{Error, TextEdit};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use zones::ZoneMap;

pub mod keys {
    pub const ROUND_DECIMALS: &str = "coords.roundDecimals";
    pub const ZONES_FILE: &str = "coords.zonesFile";
    pub const CONFIG_TABLES: &str = "coords.configTables";
}

pub const CONVERT_SELECTION: &str = "coords.convert-selection";

const MAX_ZONES_FILE: usize = 20 * 1024 * 1024;

#[derive(Default)]
struct ParamIndex {
    files: HashMap<String, Vec<(String, Vec<String>)>>,
    by_name: HashMap<String, BTreeSet<String>>,
}

impl ParamIndex {
    fn set(&mut self, path: &str, functions: Vec<(String, Vec<String>)>) {
        self.remove(path);
        for (name, _) in &functions {
            self.by_name.entry(name.clone()).or_default().insert(path.to_owned());
        }
        self.files.insert(path.to_owned(), functions);
    }

    fn remove(&mut self, path: &str) {
        for (name, _) in self.files.remove(path).unwrap_or_default() {
            if let Some(files) = self.by_name.get_mut(&name) {
                files.remove(path);
                if files.is_empty() {
                    self.by_name.remove(&name);
                }
            }
        }
    }

    fn params(&self, name: &str, except: &str) -> Option<Vec<String>> {
        let file = self.by_name.get(name)?.iter().find(|f| f.as_str() != except)?;
        self.files.get(file)?.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone())
    }
}

fn function_params(ast: &Ast, f: usize) -> Option<Vec<String>> {
    match &ast.exprs[ast.unwrap(f)].kind {
        ExprKind::Function { params, method: false, .. } => {
            Some(params.iter().map(|&d| ast.decls[d].name.clone()).collect())
        }
        _ => None,
    }
}

fn global_functions(ast: &Ast) -> Vec<(String, Vec<String>)> {
    ast.named_functions()
        .into_iter()
        .filter(|n| n.root.is_none())
        .filter_map(|n| Some((n.path, function_params(ast, n.func)?)))
        .collect()
}

struct LoadedZones {
    path: String,
    text: Arc<str>,
    map: Option<Rc<ZoneMap>>,
}

#[derive(Default)]
pub struct CoordsFeature {
    params: RefCell<ParamIndex>,
    zones: RefCell<Option<LoadedZones>>,
}

struct Resolver<'a> {
    cx: &'a Ctx<'a>,
    ast: &'a Ast,
    index: &'a ParamIndex,
    sets: Vec<NativeSet>,
}

impl Resolver<'_> {
    fn new<'a>(cx: &'a Ctx<'a>, ast: &'a Ast, index: &'a ParamIndex) -> Resolver<'a> {
        let detected = cx.ws.detected_games();
        let games = cx.ws.resource_of(&cx.doc.path).and_then(|r| r.games.clone());
        let sets = crate::natives::visible_sets(cx.settings, &detected, cx.doc.side, games.as_deref());
        Resolver { cx, ast, index, sets }
    }

    fn params(&self, call: usize) -> Option<Vec<String>> {
        let ast = self.ast;
        let ExprKind::Call { callee, method: None, .. } = ast.exprs[call].kind else { return None };
        match &ast.exprs[callee].kind {
            ExprKind::Name { decl: Some(d), .. } => match ast.decls[*d].kind {
                DeclKind::Local { .. } | DeclKind::LocalFunction { .. } => function_params(ast, ast.decls[*d].init?),
                _ => None,
            },
            ExprKind::Name { name, decl: None } => match self.cx.natives.find(name, &self.sets) {
                Some(native) => Some(native.lua_params().map(|p| p.name.clone()).collect()),
                None => self.project(name),
            },
            ExprKind::Field { .. } => {
                let path = ast.path(callee)?;
                match ast.root_name(callee).and_then(|r| ast.decl_of(r)) {
                    Some(root) => ast
                        .named_functions()
                        .into_iter()
                        .find(|n| n.root == Some(root) && n.path == path)
                        .and_then(|n| function_params(ast, n.func)),
                    None => self.project(&path),
                }
            }
            _ => None,
        }
    }

    fn project(&self, path: &str) -> Option<Vec<String>> {
        if let Some((_, params)) = global_functions(self.ast).into_iter().find(|(n, _)| n == path) {
            return Some(params);
        }
        self.index.params(path, &self.cx.doc.path)
    }

    fn spread(&self, at: usize) -> Option<Fix> {
        let (ast, src) = (self.ast, &self.cx.doc.src);
        for start in nodes_at(ast, src, at) {
            let mut n = Some(start);
            while let Some(node) = n {
                if matches!(node, Node::Stat(_)) {
                    break;
                }
                if let Node::Expr(arg) = node
                    && let Some(Node::Expr(call)) = ast.parent(node)
                    && let Some(index) = ast.call_args(call).iter().position(|&a| a == arg)
                    && let Some(components) = self.spread_components(call, index, arg)
                {
                    let (s, e) = ast.span(node);
                    return Some(Fix::new("Spread into x, y, z arguments").replace(s, e, text::join(&components)));
                }
                n = ast.parent(node);
            }
        }
        None
    }

    fn spread_components(&self, call: usize, index: usize, arg: usize) -> Option<Vec<String>> {
        let (ast, src) = (self.ast, &self.cx.doc.src);
        let names = self.params(call)?;
        if !is_xyz_at(&names, index) {
            return None;
        }
        let with_heading = is_heading_name(names.get(index + 3));
        if let Some(c) = analyze(ast, src, arg) {
            return Some(
                c.texts(ast, src).into_iter().take(if with_heading { 4 } else { 3 }).map(str::to_owned).collect(),
            );
        }
        // A vector local: its components, read without side effects.
        if !matches!(ast.exprs[arg].kind, ExprKind::Name { .. }) {
            return None;
        }
        let size = match crate::perf::distance::created_vector(ast, self.cx.natives, &self.sets, arg)? {
            "vector4" => 4,
            _ => 3,
        };
        let name = ast.text(src, Node::Expr(arg));
        let count = if with_heading && size == 4 { 4 } else { 3 };
        Some(text::KEYS[..count].iter().map(|k| format!("{name}.{k}")).collect())
    }
}

fn conversions(ast: &Ast, src: &Source, c: &Coord, decimals_kept: usize) -> Vec<Fix> {
    let texts = c.texts(ast, src);
    let n = texts.len();
    let (start, stop) = ast.span(Node::Expr(c.expr));
    let whole = |title: String, text: String| Fix::new(title).replace(start, stop, text);
    let mut out = Vec::new();
    if c.form != Form::Vector {
        out.push(whole(format!("Convert to vector{n}(…)"), text::vector(&format!("vector{n}"), &texts)));
    }
    if c.form != Form::Keyed {
        let keys: Vec<String> = text::KEYS[..n].iter().map(|k| format!("{k} = …")).collect();
        out.push(whole(format!("Convert to {{{}}}", keys.join(", ")), text::keyed(&texts)));
    }
    if c.form != Form::Array {
        out.push(whole(format!("Convert to {{{}}} array", text::KEYS[..n].join(", ")), text::array(&texts)));
    }
    let roundable: Vec<(usize, String)> = c
        .components
        .iter()
        .filter_map(|&comp| Some((comp, ast.number_text(src, comp)?)))
        .filter(|(_, t)| decimals(t).unwrap_or(0) > decimals_kept)
        .collect();
    if !roundable.is_empty() {
        let unit = match decimals_kept {
            1 => "1 decimal".to_owned(),
            d => format!("{d} decimals"),
        };
        let mut fix = Fix::new(format!("Round coordinates to {unit}"));
        for (comp, t) in roundable {
            let (s, e) = ast.span(Node::Expr(comp));
            fix = fix.replace(s, e, round(&t, decimals_kept));
        }
        out.push(fix);
    }
    let rebuild = |components: &[&str], size: usize| match c.form {
        Form::Vector => text::vector(&constructor_for(c.constructor.as_deref(), size), components),
        Form::Keyed => text::keyed(components),
        Form::Array => text::array(components),
    };
    let vector_title = |verb: &str, size: usize| match c.form {
        Form::Vector => format!("{verb} heading (convert to {})", constructor_for(c.constructor.as_deref(), size)),
        _ => format!("{verb} heading"),
    };
    if n == 3 {
        let mut with = texts.clone();
        with.push("0.0");
        out.push(whole(vector_title("Add", 4), rebuild(&with, 4)));
    } else {
        out.push(whole(vector_title("Remove", 3), rebuild(&texts[..3], 3)));
    }
    out
}

impl CoordsFeature {
    fn applies(cx: &Ctx) -> bool {
        cx.settings.enabled() && cx.doc.is_lua() && !cx.doc.is_manifest()
    }

    fn zone_map(&self, cx: &Ctx) -> Option<Rc<ZoneMap>> {
        let setting = cx.settings.string(keys::ZONES_FILE, "");
        let setting = setting.trim();
        if setting.is_empty() {
            return None;
        }
        let slashed = crate::text::slash(setting);
        let path =
            if slashed.starts_with('/') || slashed.get(1..2) == Some(":") { slashed } else { cx.ws.absolute(&slashed) };
        let text = cx.ws.file(&path)?.text.clone()?;
        let mut cache = self.zones.borrow_mut();
        if let Some(loaded) = cache.as_ref()
            && loaded.path == path
            && Arc::ptr_eq(&loaded.text, &text)
        {
            return loaded.map.clone();
        }
        let map = (text.len() <= MAX_ZONES_FILE).then(|| ZoneMap::parse(&text)).flatten().map(Rc::new);
        *cache = Some(LoadedZones { path, text, map: map.clone() });
        map
    }

    fn convert_selection(&self, cx: &CommandCx, arg: Option<&str>) -> CommandResult {
        if !cx.settings.enabled() {
            return Ok(None);
        }
        let target = match arg.map(str::trim).filter(|a| !a.is_empty()) {
            Some(a) => Some(paste::Target::parse(a).ok_or_else(|| {
                Error::Invalid(format!("unknown coordinates format '{a}' (vector, keyed, array, list or json)"))
            })?),
            None => None,
        };
        let (Some(doc), Some(editor)) = (cx.doc, cx.inv.editor()) else {
            return Ok(Some("Select coordinates to convert".into()));
        };
        let selections: Vec<_> = editor.selections()?.into_iter().filter(|r| r.start != r.end).collect();
        if selections.is_empty() {
            return Ok(Some("Select coordinates to convert".into()));
        }
        let empty = Workspace::default();
        let ctx = Ctx { doc, ws: cx.ws.unwrap_or(&empty), settings: cx.settings, natives: cx.natives };
        let tree = ast::parse(&doc.src);
        let index = self.params.borrow();
        let resolver = Resolver::new(&ctx, &tree, &index);
        let mut edits = Vec::new();
        for range in selections {
            let (a, b) = (doc.offset(range.start), doc.offset(range.end));
            let site = paste::site(&tree, &doc.src, a);
            if let Some(text) = paste::convert(&doc.text()[a..b], site, target, &|call| resolver.params(call)) {
                edits.push(TextEdit { range, text });
            }
        }
        if edits.is_empty() {
            return Ok(Some("No coordinates found in the selection".into()));
        }
        editor.apply_edits(doc.version, &edits)?;
        Ok(None)
    }
}

impl Feature for CoordsFeature {
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut index = self.params.borrow_mut();
        if kind == FileKind::Lua {
            index.set(path, global_functions(&ast::parse(src)));
        } else {
            index.remove(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.params.borrow_mut().remove(path);
    }

    fn command(&self, cx: &CommandCx, id: &str, arg: Option<&str>) -> Option<CommandResult> {
        (id == CONVERT_SELECTION).then(|| self.convert_selection(cx, arg))
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let tree = ast::parse(&cx.doc.src);
        let roots: Vec<String> =
            cx.settings.list(keys::CONFIG_TABLES, "Config, Cfg").into_iter().map(|r| r.to_ascii_lowercase()).collect();
        let zones = self.zone_map(cx);
        checks::problems(cx, &tree, &roots, zones.as_deref())
    }

    fn intentions(&self, cx: &Ctx, start: usize, _end: usize) -> Vec<Intention> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let tree = ast::parse(src);
        let mut fixes = Vec::new();
        if let Some(c) = find_at(&tree, src, start) {
            let kept = cx.settings.int(keys::ROUND_DECIMALS, 2).clamp(0, 6) as usize;
            fixes.extend(conversions(&tree, src, &c, kept));
        }
        let index = self.params.borrow();
        fixes.extend(Resolver::new(cx, &tree, &index).spread(start));
        fixes.into_iter().map(|fix| Intention { fix, kind: "refactor" }).collect()
    }
}

#[cfg(test)]
mod tests;
