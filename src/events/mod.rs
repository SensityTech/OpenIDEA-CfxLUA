pub mod ast;
pub mod checks;
pub mod db;
pub mod editor;
pub mod flow;
mod hover;
pub mod model;
pub mod packed;
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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

pub mod keys {
    pub const DETECT_HELPERS: &str = "events.detectHelpers";
    pub const HELPERS: &str = "events.helpers";
    pub const ALIAS_FUNCTIONS: &str = "events.aliasFunctions";
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

#[derive(Default)]
struct Project {
    files: BTreeMap<String, FileFacts>,
    dirty: bool,
    /// Lua files whose facts changed since the last build.
    changed: BTreeSet<String>,
    /// One of those changes touched what the detection of the event functions reads.
    redetect: bool,
    /// A manifest changed: sides and resources may differ everywhere.
    manifests_changed: bool,
    db: Option<Rc<Db>>,
    config: Config,
}

const MAX_UPDATED_FILES: usize = 8;

impl Project {
    fn rebuild(&mut self, ws: &Workspace, config: Config) {
        let incremental = !self.manifests_changed && self.config == config && self.changed.len() <= MAX_UPDATED_FILES;
        let updated = self
            .db
            .take()
            .filter(|_| incremental)
            .and_then(|db| Rc::try_unwrap(db).ok())
            .and_then(|db| db.update(ws, &self.files, &config, &self.changed, self.redetect));
        self.db = Some(Rc::new(updated.unwrap_or_else(|| Db::build(ws, &self.files, &config))));
        self.config = config;
        self.dirty = false;
        self.changed.clear();
        self.redetect = false;
        self.manifests_changed = false;
    }

    /// The facts of a Lua file were replaced or removed: `old` is what the database may still hold.
    fn edited(&mut self, path: &str, old: Option<FileFacts>) {
        let none = FileFacts::default();
        if let Some(db) = &self.db {
            let new = self.files.get(path).unwrap_or(&none);
            self.redetect |= !db.detects_alike(old.as_ref().unwrap_or(&none), new);
        }
        self.changed.insert(path.to_owned());
        self.dirty = true;
    }
}

#[derive(Default)]
pub struct EventsFeature {
    projects: RefCell<HashMap<String, Project>>,
}

impl EventsFeature {
    fn db(&self, cx: &Ctx) -> Option<Rc<Db>> {
        if !cx.ws.scanned {
            return None;
        }
        let config = Config::read(cx.settings);
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(cx.ws.root.clone()).or_default();
        // Queries build the first database, follow a settings change or update a few files; other full rebuilds
        // happen in `indexed` (heavy calls).
        let full = p.manifests_changed || p.changed.len() > MAX_UPDATED_FILES;
        if p.db.is_none() || p.config != config || p.dirty && !full {
            p.rebuild(cx.ws, config);
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
                    let old = p.files.insert(path.to_owned(), facts);
                    p.edited(path, old);
                }
            }
            FileKind::Manifest => {
                p.manifests_changed = true;
                p.dirty = true;
            }
            _ => {}
        }
    }

    fn forget_file(&self, path: &str) {
        let manifest = crate::project::file_kind(path) == FileKind::Manifest;
        for p in self.projects.borrow_mut().values_mut() {
            if let Some(old) = p.files.remove(path) {
                p.edited(path, Some(old));
            }
            if manifest {
                p.manifests_changed = true;
                p.dirty = true;
            }
        }
    }

    fn indexed(&self, ws: &Workspace) {
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(ws.root.clone()).or_default();
        if p.dirty && ws.scanned {
            let config = p.config.clone();
            p.rebuild(ws, config);
        }
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !cx.settings.enabled() || !cx.doc.is_lua() {
            return Vec::new();
        }
        // Asked at every keystroke: only a caret in a string brings the database up to date.
        let Some(string) = cx.doc.src.quoted_at(at) else { return Vec::new() };
        let db = self.db(cx);
        self.with_files(cx, |files| editor::completion(cx, db.as_deref(), files, string, at))
    }

    fn completes_from_index(&self, cx: &Ctx, at: usize) -> bool {
        cx.doc.src.quoted_at(at).is_some()
    }

    fn hover(&self, cx: &Ctx, at: usize) -> Option<crate::editor::hover::Hover> {
        hover::hover(self, cx, at)
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

pub fn location(ws: &Workspace, path: &str, line: u32) -> String {
    format!("{}:{}", display_path(ws, path), line + 1)
}

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
