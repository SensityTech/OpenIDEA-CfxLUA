//! Events: handlers and triggers across the project, built-in events, navigation, gutter, completion and checks.
//!
//! Each Lua file is reduced to [`flow::FileFacts`] when indexed; the project's
//! [`db::Db`] (helpers, sites, aliases) is rebuilt from them after a batch,
//! and the providers only look it up.

pub mod ast;
pub mod checks;
pub mod db;
pub mod editor;
pub mod flow;
pub mod model;
pub mod spec;

use crate::feature::{Ctx, Feature, Problem};
use crate::lua::Source;
use crate::project::FileKind;
use crate::settings::Settings;
use crate::workspace::Workspace;
use db::{Config, Db};
use flow::FileFacts;
use openidea_plugin::Location;
use openidea_plugin::providers::{CompletionItem, LineMarker};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// Settings keys.
pub mod keys {
    /// Detect event helpers from the parameter flow (bool, default true).
    pub const DETECT_HELPERS: &str = "events.detectHelpers";
    /// Helpers declared by hand, one per line: `Path annotation-text` (string, default empty).
    pub const HELPERS: &str = "events.helpers";
    /// Functions declaring aliases (`RegisterEventAlias('wire', 'Type', 'Name')`), comma separated.
    pub const ALIAS_FUNCTIONS: &str = "events.aliasFunctions";
    /// The argument-count check also reports handlers ignoring values (bool, default true).
    pub const REPORT_IGNORED_VALUES: &str = "events.reportIgnoredValues";
}

impl Config {
    pub fn read(settings: &Settings) -> Self {
        Self {
            detect: settings.bool(keys::DETECT_HELPERS, true),
            helpers: settings
                .string(keys::HELPERS, "")
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect(),
            aliases: settings.list(keys::ALIAS_FUNCTIONS, "RegisterEventAlias"),
        }
    }
}

/// The facts and database of one project.
#[derive(Default)]
struct Project {
    files: BTreeMap<String, FileFacts>,
    /// Facts (or a manifest) changed since the database was built.
    dirty: bool,
    db: Option<Rc<Db>>,
    /// The settings the database was built with.
    config: Config,
}

#[derive(Default)]
pub struct EventsFeature {
    /// By project root.
    projects: RefCell<HashMap<String, Project>>,
}

impl EventsFeature {
    /// The database of the request's project, rebuilt when the settings changed; `None` until the project is scanned.
    fn db(&self, cx: &Ctx) -> Option<Rc<Db>> {
        if !cx.ws.scanned {
            return None;
        }
        let config = Config::read(cx.settings);
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(cx.ws.root.clone()).or_default();
        if p.db.is_none() || p.dirty || p.config != config {
            p.db = Some(Rc::new(Db::build(cx.ws, &p.files, &config)));
            p.config = config;
            p.dirty = false;
        }
        p.db.clone()
    }

    fn with_files<R>(&self, cx: &Ctx, f: impl FnOnce(&BTreeMap<String, FileFacts>) -> R) -> R {
        let projects = self.projects.borrow();
        match projects.get(&cx.ws.root) {
            Some(p) => f(&p.files),
            None => f(&BTreeMap::new()),
        }
    }
}

impl Feature for EventsFeature {
    fn index_file(&self, ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(ws.root.clone()).or_default();
        match kind {
            FileKind::Lua => {
                let facts = flow::facts(src);
                if p.files.get(path) != Some(&facts) {
                    p.files.insert(path.to_owned(), facts);
                    p.dirty = true;
                }
            }
            // sides and resources come from the manifests
            FileKind::Manifest => p.dirty = true,
            _ => {}
        }
    }

    fn forget_file(&self, path: &str) {
        for p in self.projects.borrow_mut().values_mut() {
            if p.files.remove(path).is_some() || crate::project::file_kind(path) == FileKind::Manifest {
                p.dirty = true;
            }
        }
    }

    fn indexed(&self, ws: &Workspace) {
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(ws.root.clone()).or_default();
        if p.dirty && ws.scanned {
            p.db = Some(Rc::new(Db::build(ws, &p.files, &p.config)));
            p.dirty = false;
        }
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !cx.settings.enabled() || !cx.doc.is_lua() {
            return Vec::new();
        }
        let db = self.db(cx);
        self.with_files(cx, |files| editor::completion(cx, db.as_deref(), files, at))
    }

    fn definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        if !cx.settings.enabled() {
            return Vec::new();
        }
        self.db(cx).map(|db| editor::definition(cx, &db, at)).unwrap_or_default()
    }

    fn line_markers(&self, cx: &Ctx) -> Vec<LineMarker> {
        if !cx.settings.enabled() {
            return Vec::new();
        }
        self.db(cx).map(|db| editor::line_markers(cx, &db)).unwrap_or_default()
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !cx.settings.enabled() || !cx.doc.is_lua() || !cx.ws.contains(&cx.doc.path) {
            return Vec::new();
        }
        let mut out = checks::annotations(cx);
        if let Some(db) = self.db(cx) {
            out.extend(checks::sites(cx, &db));
        }
        out.sort_by_key(|p| (p.start, p.end));
        out
    }
}

/// Path for display: from the resource folder (`my_resource/server/main.lua`), else from the project root.
pub fn display_path(ws: &Workspace, path: &str) -> String {
    if let Some(r) = ws.resource_of(path)
        && let Some(rel) = r.relative(path)
    {
        return format!("{}/{rel}", r.name);
    }
    if ws.contains(path) {
        return ws.relative(path).to_owned();
    }
    crate::text::file_name(path).to_owned()
}

/// `my_resource/server/main.lua:12` (`line` 0-based).
pub fn location(ws: &Workspace, path: &str, line: u32) -> String {
    format!("{}:{}", display_path(ws, path), line + 1)
}

/// Escapes Markdown punctuation in plain text.
pub fn md(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `1 value` / `{n} values` / `no value` style counts.
pub fn count(n: usize, one: &str, many: &str, zero: Option<&str>) -> String {
    match (n, zero) {
        (0, Some(z)) => z.to_owned(),
        (1, _) => format!("1 {one}"),
        _ => format!("{n} {many}"),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod editor_tests;
