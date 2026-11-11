pub mod coords;
pub mod docs;
pub mod editor;
pub mod escrow;
pub mod events;
pub mod feature;
pub mod lua;
pub mod luals;
pub mod manifest;
pub mod natives;
pub mod nui;
pub mod perf;
pub mod project;
pub mod runtime;
pub mod settings;
pub mod statebags;
pub mod syntax;
pub mod text;
pub mod values;
pub mod workspace;

#[cfg(test)]
pub mod testing;

use feature::{CommandCx, Ctx, Doc, Feature, Problem};
use lua::Source;
use natives::Natives;
use openidea_plugin::language_features::{DocumentSymbol, FoldRange, LanguageFeatures};
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::semantic_tokens::{SemanticToken, SemanticTokens, TokenKind, TokenModifier};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, LogLevel, Plugin, Position, Range, StatusItem,
    TextEdit, host, register_plugin,
};
use project::{FileKind, Side};
use settings::Settings;
use text::slash;
use workspace::Workspace;

pub const STATUS_SIDE: &str = "side";

// A call's CPU budget is fuel (wasm instructions), not time: the scan is split in slices of files read and bytes indexed.
#[derive(Clone, Copy)]
struct Slice {
    reads: usize,
    bytes: usize,
    heavy: bool,
}

// Measured: up to ~1 800 instructions per indexed byte; web files cost twice Lua (counted twice).
const ACTIVATION: Slice = Slice { reads: 1_500, bytes: 512 << 10, heavy: true };
const EVENTS: Slice = Slice { reads: 1_500, bytes: 1 << 20, heavy: true };
const VIEW: Slice = Slice { reads: 64, bytes: 256 << 10, heavy: false };
const EDIT: Slice = Slice { reads: 8, bytes: 24 << 10, heavy: false };
// Bigger files are generated data or bundles: lexing one would take a call on its own.
const MAX_INDEXED: usize = 1 << 20;
const DERIVE_EVERY: u32 = 6;
// The IDE stops the plugin when an editor call runs 500 ms, without saying which call it stopped.
const SLOW_CALL_MS: u128 = 250;

/// Names in the log an editor call that came close to that limit.
struct Timed {
    what: &'static str,
    bytes: usize,
    since: std::time::Instant,
}

impl Timed {
    fn start(what: &'static str, d: &Document) -> Self {
        Self { what, bytes: d.text.len(), since: std::time::Instant::now() }
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        let ms = self.since.elapsed().as_millis();
        if ms >= SLOW_CALL_MS {
            host::log(LogLevel::Warn, &format!("{} took {ms} ms on {} KB", self.what, self.bytes >> 10));
        }
    }
}

pub fn all_features() -> Vec<Box<dyn Feature>> {
    vec![
        Box::<natives::NativesFeature>::default(),
        Box::<runtime::RuntimeFeature>::default(),
        Box::<manifest::ManifestFeature>::default(),
        Box::<statebags::StateBagsFeature>::default(),
        Box::<events::EventsFeature>::default(),
        Box::<nui::NuiFeature>::default(),
        Box::<values::ValuesFeature>::default(),
        Box::<coords::CoordsFeature>::default(),
        Box::<perf::PerfFeature>::default(),
        Box::<syntax::SyntaxFeature>::default(),
        Box::<editor::EditorFeature>::default(),
        Box::<escrow::EscrowFeature>::default(),
        Box::<luals::LualsFeature>::default(),
    ]
}

/// The document being edited, while some features have not indexed its last text.
struct Stale {
    ws: usize,
    path: String,
    /// A bit per feature that still has to index it.
    features: u32,
}

pub struct DevKit {
    features: Vec<Box<dyn Feature>>,
    docs: docs::Docs,
    settings: Settings,
    natives: Natives,
    workspaces: Vec<Workspace>,
    last: Option<(Doc, Option<Vec<Problem>>)>,
    stale: Option<Stale>,
    unread: Vec<(usize, String)>,
    pending: Vec<(usize, String)>,
    catalogs: Vec<usize>,
    derive: Vec<usize>,
    derived: usize,
    deriving: bool,
    slices: u32,
    indexing: bool,
    focus: Option<(usize, String)>,
    status_for: Option<String>,
}

impl Default for DevKit {
    fn default() -> Self {
        Self {
            features: all_features(),
            docs: docs::Docs::default(),
            settings: Settings::live(),
            natives: Natives::bundled(),
            workspaces: Vec::new(),
            last: None,
            stale: None,
            unread: Vec::new(),
            pending: Vec::new(),
            catalogs: Vec::new(),
            derive: Vec::new(),
            derived: 0,
            deriving: false,
            slices: 0,
            indexing: false,
            focus: None,
            status_for: None,
        }
    }
}

impl DevKit {
    fn workspace_for(&mut self, path: &str) -> usize {
        let path = slash(path);
        let best = |wss: &[Workspace]| {
            wss.iter().enumerate().filter(|(_, w)| w.contains(&path)).max_by_key(|(_, w)| w.root.len()).map(|(i, _)| i)
        };
        if let Some(i) = best(&self.workspaces) {
            return i;
        }
        // A project opened since: ask the IDE for the roots.
        if let Ok(projects) = host::projects() {
            for p in projects {
                if let Ok(root) = p.root()
                    && !self.workspaces.iter().any(|w| w.root == slash(&root).trim_end_matches('/'))
                {
                    self.workspaces.push(Workspace::new(&root));
                }
            }
        }
        if let Some(i) = best(&self.workspaces) {
            return i;
        }
        // Outside every project: a workspace of its own directory, never scanned.
        let dir = text::parent(&path).to_owned();
        if let Some(i) = self.workspaces.iter().position(|w| !w.scanned && w.root == dir) {
            return i;
        }
        self.workspaces.push(Workspace::new(&dir));
        self.workspaces.len() - 1
    }

    fn scan(&mut self, project: &openidea_plugin::Project, root: &str) {
        let i = self.workspace_for(&format!("{}/", slash(root).trim_end_matches('/')));
        let mut paths: Vec<String> = Vec::new();
        for (glob, limit) in workspace::SCAN {
            match project.find_files(glob, *limit) {
                Ok(found) => paths.extend(found),
                Err(e) => host::log(LogLevel::Warn, &format!("listing {glob}: {e}")),
            }
        }
        paths.sort();
        paths.dedup();
        let ws = &mut self.workspaces[i];
        let pending = self.unread.len();
        for path in paths {
            ws.set(&path, None);
            if project::file_kind(&path) != FileKind::Other {
                self.unread.push((i, path));
            }
        }
        let zones = self.settings.string("coords.zonesFile", "");
        if !zones.trim().is_empty() {
            let zones = slash(zones.trim());
            let path = if zones.contains(':') || zones.starts_with('/') { zones } else { ws.absolute(&zones) };
            if let Ok(text) = project.read_file(&path) {
                ws.set(&path, Some(&text));
            }
        }
        ws.scanned = true;
        ws.indexing = true;
        // Read last, popped first: manifests say which resource and side every other file belongs to.
        self.unread.sort_by_key(|(_, p)| project::file_kind(p) == FileKind::Manifest);
        self.catalogs.push(i);
        self.indexing = true;
        self.read_luals(i);
        host::log(LogLevel::Info, &format!("{} files of {root} to read", self.unread.len() - pending));
    }

    fn step(&mut self, slice: Slice) {
        if slice.heavy {
            let wanted = self.natives.take_wanted();
            if !wanted.is_empty() {
                for set in wanted {
                    self.natives.catalog(set);
                }
                return;
            }
            if natives::resume(&self.natives) {
                self.natives_changed();
                return;
            }
        }
        let left = self.unread.len() + self.pending.len();
        if left == 0 && self.catalogs.is_empty() && self.derive.is_empty() && !self.indexing {
            return;
        }
        if !slice.heavy {
            self.read_slice(slice.reads);
            if slice.bytes > 0 {
                self.index_pending(slice);
            }
            return;
        }
        if self.deriving || !self.derive.is_empty() && (left == 0 || self.slices >= DERIVE_EVERY) {
            self.deriving = !self.derive_slice();
            if !self.deriving {
                self.slices = 0;
            }
        } else if left > 0 {
            let task = host::begin_task("CfxLua: indexing resources", false);
            self.read_slice(slice.reads);
            // Catalogs as soon as the manifests are read: loading one in an editor call overruns its 50 ms.
            let bytes = if self.load_catalogs() { slice.bytes / 4 } else { slice.bytes };
            self.index_pending(Slice { bytes, ..slice });
            self.slices += 1;
            task.report(0, Some((self.unread.len() + self.pending.len()) as u32), None);
        } else {
            self.load_catalogs();
        }
        if self.indexing && self.unread.is_empty() && self.pending.is_empty() && self.derive.is_empty() {
            self.indexing = false;
            for ws in &mut self.workspaces {
                ws.indexing = false;
            }
            host::log(LogLevel::Info, "resources indexed");
            self.docs.indexed();
        }
    }

    // Derived indexes (event links, NUI bridge, state bags) grow with the project: one real rebuild per call.
    fn derive_slice(&mut self) -> bool {
        let start = std::time::Instant::now();
        while let Some(&i) = self.derive.first() {
            while self.derived < self.features.len() {
                if start.elapsed().as_millis() > 10 {
                    return false;
                }
                self.features[self.derived].indexed(&self.workspaces[i]);
                self.derived += 1;
            }
            self.derived = 0;
            self.derive.remove(0);
        }
        true
    }

    fn read_slice(&mut self, max: usize) {
        if self.unread.is_empty() {
            return;
        }
        let Ok(projects) = host::projects() else { return };
        let projects: Vec<(String, openidea_plugin::Project)> = projects
            .into_iter()
            .filter_map(|p| Some((slash(&p.root().ok()?).trim_end_matches('/').to_owned(), p)))
            .collect();
        for _ in 0..max {
            let Some((i, path)) = self.unread.pop() else { break };
            let ws = &mut self.workspaces[i];
            let text = projects.iter().find(|(root, _)| *root == ws.root).and_then(|(_, p)| p.read_file(&path).ok());
            ws.set(&path, text.as_deref());
            if text.is_some() {
                self.pending.push((i, slash(&path)));
            }
        }
    }

    fn load_catalogs(&mut self) -> bool {
        let ready: Vec<usize> = self
            .catalogs
            .iter()
            .copied()
            .filter(|i| !self.unread.iter().any(|(j, p)| j == i && project::file_kind(p) == FileKind::Manifest))
            .collect();
        self.catalogs.retain(|i| !ready.contains(i));
        let mut loaded = false;
        for i in ready {
            let games = natives::project_games(&self.settings, &self.workspaces[i].detected_games());
            for set in games.into_iter().map(natives::NativeSet::for_game).chain([natives::NativeSet::Server]) {
                loaded |= !self.natives.is_loaded(set);
                self.natives.catalog(set);
            }
        }
        loaded
    }

    fn index_pending(&mut self, slice: Slice) {
        if self.pending.is_empty() {
            return;
        }
        let mut used = 0;
        while let Some((i, path)) = self.pending.pop() {
            let ws = &self.workspaces[i];
            let Some(f) = ws.file(&path) else { continue };
            let Some(text) = f.text.as_deref() else { continue };
            if text.len() > MAX_INDEXED {
                continue;
            }
            let cost = if f.kind == FileKind::Web { 2 * text.len() } else { text.len() };
            if used + cost > slice.bytes && (used > 0 || !slice.heavy) {
                self.pending.push((i, path));
                break;
            }
            used += cost;
            let src = parse(f.kind, text);
            for feature in &self.features {
                feature.index_file(ws, &f.path, f.kind, &src);
            }
            // The workspace holds what the editor last sent: nothing of that document is left to index.
            self.stale.take_if(|stale| stale.ws == i && stale.path == path);
            if self.derive.first() == Some(&i) {
                self.derived = 0;
            } else if !self.derive.contains(&i) {
                self.derive.push(i);
            }
        }
        if self.pending.is_empty() {
            self.last = None;
        }
    }

    fn focus_on(&mut self, path: &str) {
        let i = self.workspace_for(path);
        self.prioritize(i, path);
    }

    fn prioritize(&mut self, ws: usize, path: &str) {
        let Some(dir) = self.workspaces[ws].resource_of(path).map(|r| format!("{}/", r.dir)) else { return };
        if self.focus.as_ref() == Some(&(ws, dir.clone())) {
            return;
        }
        let first = |(i, p): &(usize, String)| *i == ws && slash(p).starts_with(&dir);
        self.unread.sort_by_key(|item| (first(item), project::file_kind(&item.1) == FileKind::Manifest));
        self.pending.sort_by_key(|item| first(item));
        self.focus = Some((ws, dir));
    }

    fn reread(&mut self, path: &str) {
        let i = self.workspace_for(path);
        if !self.workspaces[i].scanned {
            return;
        }
        let key = slash(path);
        match read_disk(path) {
            Disk::Text(t) => {
                let ws = &mut self.workspaces[i];
                if ws.text(&key) == Some(t.as_str()) {
                    return;
                }
                ws.set(path, Some(&t));
                // Indexed by the next slice, under its budget.
                if ws.text(&key).is_some() {
                    self.pending.push((i, key));
                }
            }
            Disk::Missing => {
                if !self.workspaces[i].exists(&key) {
                    return;
                }
                self.workspaces[i].remove(path);
                for f in &self.features {
                    f.forget_file(&key);
                }
                self.mark_derive(i);
            }
            Disk::Unreadable => return,
        }
        self.last = None;
    }

    // The host does not list a `.cfxlua` that Git ignores: the definitions are read by name.
    fn read_luals(&mut self, i: usize) {
        self.reread_luarc(i);
        for path in luals::candidates(&self.workspaces[i]) {
            self.reread(&path);
        }
    }

    fn reread_luarc(&mut self, i: usize) {
        for name in luals::LUARC_FILES {
            let path = self.workspaces[i].absolute(name);
            self.reread(&path);
        }
    }

    fn command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let (ws, doc) = self.command_doc(ctx);
        let cx = CommandCx {
            ws: ws.map(|i| &self.workspaces[i]),
            settings: &self.settings,
            natives: &self.natives,
            doc: doc.as_ref(),
            inv: ctx,
        };
        let result = std::iter::once(&self.docs as &dyn Feature)
            .chain(self.features.iter().map(|f| f.as_ref()))
            .find_map(|f| f.command(&cx, id, arg))
            .unwrap_or_else(|| Err(Error::NotFound(format!("command {id}"))));
        if matches!(id, natives::UPDATE | natives::SCHEDULED_UPDATE) {
            self.natives_changed();
        }
        // A .luarc.json written outside the IDE, or not saved yet, sent no event.
        if id == luals::DEFINITIONS
            && let Some(i) = ws
        {
            self.read_luals(i);
        }
        result
    }

    // Reloaded by a later heavy call: this one already spent its share of the budget on the update.
    fn natives_changed(&mut self) {
        let changed = natives::take_changed();
        if !changed.is_empty() {
            self.natives.invalidate(&changed);
            self.catalogs = (0..self.workspaces.len()).collect();
            self.last = None;
        }
    }

    fn mark_derive(&mut self, i: usize) {
        if self.derive.first() == Some(&i) {
            self.derived = 0;
        } else if !self.derive.contains(&i) {
            self.derive.push(i);
        }
    }

    fn prepare(&mut self, d: &Document) -> Option<usize> {
        self.natives.freeze(true);
        if !self.settings.enabled() {
            return None;
        }
        let i = self.workspace_for(&d.path);
        self.prioritize(i, &d.path);
        self.step(EDIT);
        // The editor focus event that started the plugin came before it subscribed.
        if self.status_for.as_deref() != Some(d.path.as_str()) {
            self.update_status(Some(&d.path));
        }
        let path = slash(&d.path);
        if let Some((doc, _)) = &self.last
            && doc.path == path
            && doc.version == d.version
        {
            return Some(i);
        }
        // Generated data, bundles and declaration files: lexing one would overrun an editor call.
        if d.text.len() > MAX_INDEXED || d.text.starts_with("---@meta") {
            self.last = None;
            return None;
        }
        self.settings.reset();
        // Another document was left before every feature indexed it: what is asked next may read it.
        if self.stale.as_ref().is_some_and(|stale| stale.ws != i || stale.path != path) {
            self.flush_stale();
        }
        let ws = &mut self.workspaces[i];
        let changed = ws.text(&path) != Some(d.text.as_str());
        ws.set(&path, Some(&d.text));
        if changed {
            // Indexed by the first call that reads the indexes: a completion is asked at every keystroke.
            self.stale = Some(Stale { ws: i, path: path.clone(), features: (1 << self.features.len()) - 1 });
        }
        let doc = Doc::new(&self.workspaces[i], &path, &d.language, d.version, &d.text);
        self.last = Some((doc, None));
        Some(i)
    }

    /// The document, indexed by every feature: where an answer that reads the indexes starts.
    fn prepare_indexed(&mut self, d: &Document) -> Option<usize> {
        let ws = self.prepare(d)?;
        self.index_stale(|_, _| true);
        Some(ws)
    }

    /// Gives the edited document to the features that have not indexed it, among those `wanted` keeps.
    fn index_stale(&mut self, wanted: impl Fn(&dyn Feature, &Ctx) -> bool) {
        let Some(mut stale) = self.stale.take() else { return };
        let Some((doc, _)) = self.last.as_ref().filter(|(doc, _)| doc.path == stale.path) else {
            self.stale = Some(stale);
            return;
        };
        let ws = &self.workspaces[stale.ws];
        let cx = Ctx { doc, ws, settings: &self.settings, natives: &self.natives };
        let left = stale.features;
        for (n, feature) in self.features.iter().enumerate() {
            if stale.features & (1 << n) != 0 && wanted(feature.as_ref(), &cx) {
                feature.index_file(ws, &doc.path, doc.kind, &doc.src);
                stale.features &= !(1 << n);
            }
        }
        let (ws, indexed) = (stale.ws, stale.features != left);
        if stale.features != 0 {
            self.stale = Some(stale);
        }
        if indexed {
            // Derived indexes are rebuilt by the next heavy call, never on the editing path.
            self.mark_derive(ws);
        }
    }

    /// Before a command, a view or an event reads the indexes: nothing of the edited document is left out.
    fn flush_stale(&mut self) {
        self.index_stale(|_, _| true);
        if let Some(left) = self.stale.take() {
            self.index_later(left);
        }
    }

    /// A document the editor moved away from: the next slices index it from the workspace, under their budget.
    fn index_later(&mut self, left: Stale) {
        let entry = (left.ws, left.path);
        if !self.pending.contains(&entry) {
            self.pending.push(entry);
        }
    }

    fn with_ctx<R>(&self, ws: usize, f: impl FnOnce(&Ctx, &[Box<dyn Feature>]) -> R) -> Option<R> {
        let (doc, _) = self.last.as_ref()?;
        let cx = Ctx { doc, ws: &self.workspaces[ws], settings: &self.settings, natives: &self.natives };
        Some(f(&cx, &self.features))
    }

    fn native_path(&self, ws: usize, path: &str) -> String {
        let root = &self.workspaces[ws];
        if root.native_backslashes { path.replace('/', "\\") } else { path.to_owned() }
    }

    fn native_location(&self, ws: usize, l: Location) -> Location {
        Location { path: self.native_path(ws, &l.path), range: l.range }
    }

    fn problems(&mut self, ws: usize) -> Vec<Problem> {
        if let Some((_, Some(problems))) = &self.last {
            return problems.clone();
        }
        let problems =
            self.with_ctx(ws, |cx, features| features.iter().flat_map(|f| f.problems(cx)).collect::<Vec<_>>());
        let problems = problems.unwrap_or_default();
        if let Some((_, p)) = &mut self.last {
            *p = Some(problems.clone());
        }
        problems
    }

    fn publish(&self, ws: usize, path: &str, problems: &[Problem]) {
        let Some((doc, _)) = &self.last else { return };
        let items: Vec<Diagnostic> = problems
            .iter()
            .map(|p| Diagnostic {
                deprecated: p.deprecated,
                ..Diagnostic::new(doc.range(p.start, p.end), p.severity, p.message.clone()).code(p.code)
            })
            .collect();
        if let Err(e) = host::publish_diagnostics(&self.native_path(ws, path), &items) {
            host::log(LogLevel::Debug, &format!("diagnostics of {path}: {e}"));
        }
    }

    fn command_doc(&mut self, inv: &Invocation) -> (Option<usize>, Option<Doc>) {
        let ws =
            inv.project().and_then(|p| p.root().ok()).map(|root| self.workspace_for(&format!("{}/", slash(&root))));
        let doc = inv.editor().and_then(|e| {
            let d = e.document().ok()?;
            if d.text.len() > MAX_INDEXED {
                return None;
            }
            let path = e.path();
            let i = ws.unwrap_or_else(|| self.workspace_for(&path));
            Some(Doc::new(&self.workspaces[i], &path, &e.language(), d.version, &d.text))
        });
        (ws, doc)
    }

    fn update_status(&mut self, path: Option<&str>) {
        self.status_for = path.map(str::to_owned);
        let text = path.and_then(|p| {
            let kind = project::file_kind(p);
            if !matches!(kind, FileKind::Lua | FileKind::Manifest) {
                return None;
            }
            let i = self.workspace_for(p);
            let ws = &self.workspaces[i];
            Some(status_text(ws, &self.settings, p))
        });
        let item = match text {
            Some((text, tooltip)) => StatusItem { text, tooltip: Some(tooltip), command: Some("show-docs".into()) },
            None => StatusItem {
                text: "CfxLua".into(),
                tooltip: Some("OpenIDEA-CfxLUA".into()),
                command: Some("show-docs".into()),
            },
        };
        host::set_status_item(STATUS_SIDE, &item);
    }
}

pub fn status_text(ws: &Workspace, settings: &Settings, path: &str) -> (String, String) {
    let side = ws.side_of(path);
    let resource = ws.resource_of(path);
    let detected = ws.detected_games();
    let games = match resource.and_then(|r| r.games.clone()).filter(|g| !g.is_empty()) {
        Some(g) if settings.string(settings::keys::GAME, "auto") == "auto" => g,
        _ => natives::project_games(settings, &detected),
    };
    let game = project::describe_games(&games);
    let what = match side {
        Side::Manifest => "Resource manifest".to_owned(),
        Side::Unknown => "Lua script".to_owned(),
        s => format!("{} script", s.label()),
    };
    let text = match resource {
        Some(r) => format!("{what} · {} · {game}", r.name),
        None => format!("{what} · {game}"),
    };
    let why = match (side, resource) {
        (Side::Manifest, _) => "the resource's manifest".to_owned(),
        (Side::Unknown, Some(_)) => "not listed by the manifest, no client/server naming convention".to_owned(),
        (_, Some(r)) if r.relative(&slash(path)).and_then(|rel| r.listed_side(rel)).is_some() => {
            format!("listed in {}", r.manifest)
        }
        (Side::Unknown, None) => "outside any resource".to_owned(),
        _ => "file or folder naming convention".to_owned(),
    };
    (text, format!("CfxLua: {} ({why}). Click for the documentation.", side.label()))
}

enum Disk {
    Text(String),
    Missing,
    Unreadable,
}

fn read_disk(path: &str) -> Disk {
    let Ok(projects) = host::projects() else { return Disk::Unreadable };
    let mut missing = false;
    for p in projects {
        match p.read_file(path) {
            Ok(text) => return Disk::Text(text),
            // OpenIDEA reports a missing file as invalid ("<path>: no such file"), the SDK mock as not found.
            Err(Error::NotFound(_)) => missing = true,
            Err(Error::Invalid(m)) if m.ends_with("no such file") => missing = true,
            Err(_) => {}
        }
    }
    if missing { Disk::Missing } else { Disk::Unreadable }
}

fn parse(kind: FileKind, text: &str) -> Source {
    if matches!(kind, FileKind::Lua | FileKind::Manifest) && !project::is_escrowed(text) {
        Source::new(text)
    } else {
        Source::plain(text)
    }
}

impl Plugin for DevKit {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        self.natives.freeze(false);
        for kind in [
            EventKind::FileOpened,
            EventKind::FileSaved,
            EventKind::EditorFocused,
            EventKind::VcsChanged,
            EventKind::Scheduled,
        ] {
            if let Err(e) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("events: {e}"));
            }
        }
        if !self.settings.enabled() {
            return Ok(());
        }
        // Index every open project once per instance (the instance is reclaimed when idle).
        if let Ok(projects) = host::projects() {
            for p in projects {
                if let Ok(root) = p.root() {
                    self.scan(&p, &root);
                }
            }
        }
        self.step(ACTIVATION);
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        self.natives.freeze(false);
        self.settings.reset();
        self.flush_stale();
        self.step(VIEW);
        self.command(id, ctx, arg)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.natives.freeze(false);
        self.flush_stale();
        let mut slice = EVENTS;
        for e in events {
            match e {
                Event::FileOpened(path) => self.focus_on(path),
                Event::FileSaved(path) => {
                    self.reread(path);
                    if project::file_kind(path) == FileKind::Manifest {
                        self.update_status(Some(path));
                    }
                }
                Event::EditorFocused(path) => {
                    if let Some(path) = path {
                        self.focus_on(path);
                        // An edit outside the IDE sends no event: lua-language-server would see it, not us.
                        if let Some(i) = self.workspaces.iter().position(|w| w.scanned && w.contains(path)) {
                            self.reread_luarc(i);
                        }
                    }
                    self.update_status(path.as_deref());
                }
                Event::VcsChanged(repo) => {
                    let repo = slash(repo).trim_end_matches('/').to_owned();
                    for i in 0..self.workspaces.len() {
                        let ws = &self.workspaces[i];
                        if ws.scanned && (ws.contains(&repo) || ws.root.starts_with(&format!("{repo}/"))) {
                            self.read_luals(i);
                        }
                    }
                }
                Event::Scheduled => {
                    let _ = self.command(natives::SCHEDULED_UPDATE, ctx, None);
                    slice = VIEW;
                }
                _ => {}
            }
        }
        self.step(slice);
    }
}

impl Providers for DevKit {
    fn completion(&mut self, d: &Document, at: Position, trigger: Option<&str>) -> Vec<CompletionItem> {
        let _timed = Timed::start("completion", d);
        let Some(ws) = self.prepare(d) else { return Vec::new() };
        self.index_stale(|feature, cx| feature.completes_from_index(cx, cx.doc.offset(at)));
        self.with_ctx(ws, |cx, features| {
            let at = cx.doc.offset(at);
            features.iter().flat_map(|f| f.completion(cx, at, trigger)).collect()
        })
        .unwrap_or_default()
    }

    fn definition(&mut self, d: &Document, at: Position) -> Vec<Location> {
        let _timed = Timed::start("definition", d);
        let Some(ws) = self.prepare_indexed(d) else { return Vec::new() };
        let found = self
            .with_ctx(ws, |cx, features| {
                let at = cx.doc.offset(at);
                features.iter().flat_map(|f| f.definition(cx, at)).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        found.into_iter().map(|l| self.native_location(ws, l)).collect()
    }

    fn line_markers(&mut self, d: &Document) -> Vec<LineMarker> {
        let _timed = Timed::start("line markers", d);
        let Some(ws) = self.prepare_indexed(d) else {
            // Switched off: clear what was published.
            let _ = host::publish_diagnostics(&d.path, &[]);
            return Vec::new();
        };
        // Asked once per document version: the moment to publish its problems too.
        let problems = self.problems(ws);
        self.publish(ws, &slash(&d.path), &problems);
        let markers =
            self.with_ctx(ws, |cx, features| features.iter().flat_map(|f| f.line_markers(cx)).collect::<Vec<_>>());
        markers
            .unwrap_or_default()
            .into_iter()
            .map(|mut m| {
                m.target = m.target.map(|l| self.native_location(ws, l));
                m
            })
            .collect()
    }

    fn code_actions(&mut self, d: &Document, selection: Range) -> Vec<CodeAction> {
        let _timed = Timed::start("code actions", d);
        let Some(ws) = self.prepare_indexed(d) else { return Vec::new() };
        let problems = self.problems(ws);
        let Some((doc, _)) = &self.last else { return Vec::new() };
        let (start, end) = (doc.offset(selection.start), doc.offset(selection.end));
        let mut fixes: Vec<(feature::Fix, &'static str)> = problems
            .iter()
            .filter(|p| p.start <= end && start <= p.end)
            .flat_map(|p| p.fixes.iter().cloned().map(|f| (f, "quickfix")))
            .collect();
        if let Some(intentions) = self
            .with_ctx(ws, |cx, features| features.iter().flat_map(|f| f.intentions(cx, start, end)).collect::<Vec<_>>())
        {
            fixes.extend(intentions.into_iter().map(|i| (i.fix, i.kind)));
        }
        let path = self.native_path(ws, &doc.path);
        fixes
            .into_iter()
            .map(|(fix, kind)| {
                let mut edits: Vec<(String, Vec<TextEdit>)> = Vec::new();
                if !fix.edits.is_empty() {
                    let own = fix.edits.iter().map(|(s, e, t)| doc.edit(*s, *e, t.clone())).collect();
                    edits.push((path.clone(), own));
                }
                for (p, e) in fix.other {
                    edits.push((self.native_path(ws, &p), e));
                }
                CodeAction {
                    title: fix.title,
                    kind: Some(kind.to_owned()),
                    preferred: fix.preferred,
                    edits,
                    data: None,
                }
            })
            .collect()
    }
}

impl Views for DevKit {
    fn render(&mut self, view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        self.natives.freeze(false);
        self.flush_stale();
        self.step(VIEW);
        let (ws, doc) = self.command_doc(ctx);
        let cx = CommandCx {
            ws: ws.map(|i| &self.workspaces[i]),
            settings: &self.settings,
            natives: &self.natives,
            doc: doc.as_ref(),
            inv: ctx,
        };
        self.docs.render(&cx, view, viewport)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        self.natives.freeze(false);
        self.settings.reset();
        self.flush_stale();
        // Opening the window or searching moves the scan on; rows, details and preferences are answered at once.
        if let ViewEvent::Message(text) = &event
            && serde_json::from_str::<serde_json::Value>(text)
                .is_ok_and(|m| matches!(m["op"].as_str(), Some("init" | "search")))
        {
            self.step(VIEW);
        }
        let (ws, doc) = self.command_doc(ctx);
        let cx = CommandCx {
            ws: ws.map(|i| &self.workspaces[i]),
            settings: &self.settings,
            natives: &self.natives,
            doc: doc.as_ref(),
            inv: ctx,
        };
        let Some(command) = self.docs.handle(&cx, view, event)? else { return Ok(()) };
        let result = self.command(command, ctx, None);
        self.docs.refresh(command);
        result.map(|_| ())
    }
}

/// What the SDK's enums cannot say about a token: a kind in its place, modifiers on top of its own.
type BeyondSdk = (Option<&'static str>, &'static [&'static str]);

thread_local! {
    /// For each token of the last answer, the names the editor styles that the SDK's enums lack
    /// (`self-keyword`, `global`, `mutable`): the plugin API takes any LSP name.
    static TOKEN_NAMES: std::cell::RefCell<Vec<BeyondSdk>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl SemanticTokens for DevKit {
    fn semantic_tokens(&mut self, d: &Document) -> Vec<SemanticToken> {
        let _timed = Timed::start("semantic tokens", d);
        let tokens = self.prepare_indexed(d).and_then(|ws| {
            self.with_ctx(ws, |cx, features| {
                use editor::semantic::{Kind, Mods};
                let tokens: Vec<editor::semantic::Token> =
                    features.iter().flat_map(|f| f.semantic_tokens(cx)).collect();
                let mut cursor = cx.doc.src.lines.cursor(cx.doc.text());
                let mut names = Vec::with_capacity(tokens.len());
                let mut out = Vec::with_capacity(tokens.len());
                for token in &tokens {
                    let kind = match token.kind {
                        Kind::Variable => TokenKind::Variable,
                        Kind::Parameter | Kind::SelfName => TokenKind::Parameter,
                        Kind::Function => TokenKind::Function,
                        Kind::Method => TokenKind::Method,
                        Kind::Property => TokenKind::Property,
                        Kind::Hash => TokenKind::String,
                        Kind::Type => TokenKind::Type,
                        Kind::Tag => TokenKind::Decorator,
                        Kind::Keyword => TokenKind::Keyword,
                    };
                    let modifiers = [
                        (Mods::DECLARATION, TokenModifier::Declaration),
                        (Mods::STATIC, TokenModifier::Static),
                        (Mods::DEPRECATED, TokenModifier::Deprecated),
                        (Mods::LIBRARY, TokenModifier::DefaultLibrary),
                    ];
                    let modifiers = modifiers.into_iter().filter(|(flag, _)| token.mods.has(*flag)).map(|(_, m)| m);
                    names.push(token.beyond_sdk());
                    out.push(SemanticToken {
                        range: cursor.range(token.start, token.end),
                        kind,
                        modifiers: modifiers.collect(),
                    });
                }
                (out, names)
            })
        });
        let (tokens, names) = tokens.unwrap_or_default();
        TOKEN_NAMES.with(|kept| *kept.borrow_mut() = names);
        tokens
    }
}

impl LanguageFeatures for DevKit {
    fn folding(&mut self, d: &Document) -> Vec<FoldRange> {
        let _timed = Timed::start("folding", d);
        let folds = self.prepare_indexed(d).and_then(|ws| {
            self.with_ctx(ws, |cx, features| {
                let folds = features.iter().flat_map(|f| f.folding(cx));
                let lines = folds.map(|fold| {
                    let kind = match fold.kind {
                        editor::folding::FoldKind::Region => "region",
                        editor::folding::FoldKind::Comment => "comment",
                    };
                    (cx.doc.position(fold.start).line, cx.doc.position(fold.end).line, kind)
                });
                lines
                    .filter(|(start, end, _)| end > start)
                    .map(|(start_line, end_line, kind)| FoldRange { start_line, end_line, kind: kind.to_owned() })
                    .collect::<Vec<_>>()
            })
        });
        folds.unwrap_or_default()
    }

    fn document_symbols(&mut self, d: &Document) -> Vec<DocumentSymbol> {
        let _timed = Timed::start("structure", d);
        let symbols = self.prepare_indexed(d).and_then(|ws| {
            self.with_ctx(ws, |cx, features| {
                let symbols = features.iter().flat_map(|f| f.symbols(cx));
                symbols
                    .map(|symbol| DocumentSymbol {
                        name: symbol.name,
                        detail: symbol.detail,
                        kind: symbol.kind.to_owned(),
                        range: cx.doc.range(symbol.range.0, symbol.range.1),
                        selection_range: cx.doc.range(symbol.selection.0, symbol.selection.1),
                        parent: symbol.parent,
                    })
                    .collect::<Vec<_>>()
            })
        });
        symbols.unwrap_or_default()
    }

    fn document_highlights(
        &mut self,
        d: &Document,
        at: Position,
    ) -> Vec<openidea_plugin::language_features::Highlight> {
        let _timed = Timed::start("occurrences", d);
        let highlights = self.prepare_indexed(d).and_then(|ws| {
            self.with_ctx(ws, |cx, features| {
                let at = cx.doc.offset(at);
                let highlights = features.iter().flat_map(|f| f.highlights(cx, at));
                highlights
                    .map(|highlight| openidea_plugin::language_features::Highlight {
                        range: cx.doc.range(highlight.start, highlight.end),
                        kind: match highlight.access {
                            editor::highlights::Access::Text => "text",
                            editor::highlights::Access::Read => "read",
                            editor::highlights::Access::Write => "write",
                        }
                        .to_owned(),
                    })
                    .collect::<Vec<_>>()
            })
        });
        highlights.unwrap_or_default()
    }
}

register_plugin!(DevKit, providers, views, language_features);

// The semantic tokens are exported here rather than by `register_plugin!`, to send the names of
// `TOKEN_NAMES` in place of the SDK's.
#[cfg(target_arch = "wasm32")]
const _: () = {
    use openidea_plugin::semantic_bindings::exports::openidea::plugin::semantic_tokens_provider as wit;

    struct Export;

    impl wit::Guest for Export {
        fn semantic_tokens(doc: wit::Document) -> Vec<wit::SemanticToken> {
            let mut tokens = openidea_plugin::__rt::semantic_tokens::<DevKit>(doc);
            TOKEN_NAMES.with(|names| {
                let names = names.borrow();
                if names.len() != tokens.len() {
                    return;
                }
                for (token, (kind, modifiers)) in tokens.iter_mut().zip(names.iter()) {
                    if let Some(kind) = kind {
                        token.kind = (*kind).to_owned();
                    }
                    token.modifiers.extend(modifiers.iter().map(|modifier| (*modifier).to_owned()));
                }
            });
            tokens
        }
    }

    openidea_plugin::semantic_bindings::__export_semantic_tokens_world!(Export with_types_in openidea_plugin::semantic_bindings);
};

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURE_NAMES: [&str; 13] = [
        "natives",
        "runtime",
        "manifest",
        "statebags",
        "events",
        "nui",
        "values",
        "coords",
        "perf",
        "syntax",
        "editor",
        "escrow",
        "luals",
    ];

    type Section = (String, Vec<(String, String)>);

    fn manifest_sections() -> Vec<Section> {
        let text =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin.toml")).unwrap();
        let mut sections: Vec<Section> = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if line.starts_with('[') {
                sections.push((line.trim_matches(['[', ']']).to_owned(), Vec::new()));
            } else if let (Some((key, value)), Some((_, entries))) = (line.split_once(" = "), sections.last_mut()) {
                entries.push((key.to_owned(), value.trim_matches('"').to_owned()));
            }
        }
        sections
    }

    fn values<'a>(sections: &'a [Section], kind: &str, key: &str) -> Vec<&'a str> {
        let field = |e: &'a [(String, String)]| e.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        sections.iter().filter(|(k, _)| k == kind).filter_map(|(_, e)| field(e)).collect()
    }

    #[test]
    fn an_editor_call_close_to_the_limit_is_named_in_the_log() {
        let host = openidea_plugin::mock::MockHost::install();
        let ago = |ms: u64| {
            std::time::Instant::now().checked_sub(std::time::Duration::from_millis(ms)).expect("an earlier instant")
        };
        drop(Timed { what: "semantic tokens", bytes: 925 << 10, since: ago(300) });
        drop(Timed { what: "folding", bytes: 925 << 10, since: ago(20) });
        let warned: Vec<String> = host
            .logs()
            .into_iter()
            .filter(|(level, _)| matches!(level, LogLevel::Warn))
            .map(|(_, text)| text)
            .collect();
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].starts_with("semantic tokens took ") && warned[0].ends_with(" ms on 925 KB"), "{warned:?}");
    }

    #[test]
    fn every_declared_command_is_handled_and_every_file_exists() {
        let sections = manifest_sections();
        let host = openidea_plugin::mock::MockHost::install();
        let inv = host.invocation();
        let (settings, natives) = (Settings::defaults(), Natives::from_natives(Vec::new()));
        let cx = CommandCx { ws: None, settings: &settings, natives: &natives, doc: None, inv: &inv };
        let docs = docs::Docs::default();
        let features = all_features();
        for id in values(&sections, "contributes.commands", "id") {
            let handled = std::iter::once(&docs as &dyn Feature)
                .chain(features.iter().map(|f| f.as_ref()))
                .any(|f| f.command(&cx, id, None).is_some());
            assert!(handled, "command {id} is declared but not handled");
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = [
            ("contributes.fileTemplates", "path"),
            ("contributes.fileIcons", "icon"),
            ("contributes.fileIcons", "iconDark"),
            ("contributes.views", "web"),
            ("contributes.settings", "schema"),
        ];
        for (kind, key) in files {
            for path in values(&sections, kind, key) {
                assert!(root.join(path).is_file(), "{kind}.{key} = {path} does not exist");
            }
        }
    }

    #[test]
    fn run_detectors_follow_the_host_rules() {
        let sections = manifest_sections();
        let detectors: Vec<&Vec<(String, String)>> =
            sections.iter().filter(|(k, _)| k == "contributes.runDetectors").map(|(_, e)| e).collect();
        assert!(!detectors.is_empty());
        for entries in detectors {
            let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("");
            let keys = ["id", "name", "files", "command", "args", "test"];
            assert!(entries.iter().all(|(k, _)| keys.contains(&k.as_str())), "{entries:?}");
            let id = get("id");
            assert!(
                id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c))
            );
            assert!((1..=128).contains(&get("name").trim().len()));
            assert!(get("files").starts_with('[') && !get("files").contains(".."), "{}", get("files"));
            assert!(!get("command").is_empty() && !get("command").contains(char::is_whitespace));
        }
    }

    #[test]
    fn status_names_side_resource_and_game() {
        let ws = Workspace::from_files(
            "/p",
            &[("shop/fxmanifest.lua", "game 'gta5'\nclient_script 'client/*.lua'"), ("shop/client/main.lua", "")],
        );
        let (text, tooltip) = status_text(&ws, &Settings::defaults(), "/p/shop/client/main.lua");
        assert_eq!(text, "Client script · shop · FiveM");
        assert!(tooltip.contains("listed in fxmanifest.lua"), "{tooltip}");
        let (text, _) = status_text(&ws, &Settings::defaults(), "/p/shop/fxmanifest.lua");
        assert_eq!(text, "Resource manifest · shop · FiveM");
    }

    fn windows_server_resource(host: &openidea_plugin::mock::MockHost) -> (DevKit, String, String) {
        host.set_project_root(Some(r"C:\srv\res"));
        host.add_file(
            "fxmanifest.lua",
            "fx_version 'cerulean'\r\ngames { 'gta5', 'gta5enhanced' }\r\n\
             server_scripts {\r\n    '@mysql-async/lib/MySQL.lua',\r\n    \"src/api/core/S*.lua\",\r\n}\r\n",
        );
        let body = "local SMain = {}\r\nfunction SMain.Start() end\r\n".to_owned();
        host.add_file("src/api/core/SMain.lua", &body);
        let mut kit = DevKit::default();
        kit.activate(&host.context("sensity.cfxlua")).unwrap();
        (kit, r"C:\srv\res\src\api\core\SMain.lua".to_owned(), body)
    }

    #[test]
    fn definitions_the_host_does_not_list_are_read_by_name_and_followed() {
        let host = openidea_plugin::mock::MockHost::install();
        host.add_file("res/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n");
        host.add_file("res/client/main.lua", "local x = 1\n");
        host.add_file(luals::LUARC, include_str!("../templates/luarc.json"));
        let sets = [natives::NativeSet::Gta5, natives::NativeSet::Server];
        let (text, _) = luals::definitions(&Natives::bundled(), &sets, &[runtime::Part::Shared]);
        // Not a `.lua` name: the scan lists it without reading it, standing for a folder the host does not list.
        let hidden = "/project/.cfxlua/natives.meta";
        host.add_file(".cfxlua/natives.meta", &text);
        host::storage::set("luals.definitions", &serde_json::to_string(&[hidden]).unwrap()).unwrap();
        let mut kit = DevKit::default();
        kit.activate(&host.context("sensity.cfxlua")).unwrap();
        assert_eq!(kit.workspaces[0].luals.sets, sets, "read at activation");
        let path = "/project/res/client/main.lua".to_owned();
        let doc = Document { path: path.clone(), language: "lua".into(), version: 2, text: "GetEntityCo".into() };
        let coords = |kit: &mut DevKit| {
            kit.completion(&doc, Position::new(0, 11), None).iter().any(|i| i.label == "GetEntityCoords")
        };
        assert!(!coords(&mut kit), "left to lua-language-server");
        host.add_file(luals::LUARC, r#"{ "workspace.library": ["types"] }"#);
        kit.handle_events(&[Event::EditorFocused(Some(path.clone()))], &host.invocation());
        assert!(kit.workspaces[0].luals.sets.is_empty(), "a .luarc.json edited outside the IDE");
        assert!(coords(&mut kit));
        host.add_file(luals::LUARC, r#"{ "Lua.workspace.library": ["${workspaceFolder}/.cfxlua"], }"#);
        kit.handle_events(&[Event::VcsChanged("/project".into())], &host.invocation());
        assert_eq!(kit.workspaces[0].luals.sets, sets, "after a checkout");
        assert!(host.subscriptions().contains(&EventKind::VcsChanged));
    }

    #[test]
    fn natives_are_loaded_at_activation_and_survive_update_checks() {
        let host = openidea_plugin::mock::MockHost::install();
        let (mut kit, path, body) = windows_server_resource(&host);
        assert!(kit.natives.is_loaded(natives::NativeSet::Server), "loaded by the activation, not by an editor call");
        let inv = host.invocation();
        kit.handle_events(&[Event::FileOpened(path.clone()), Event::EditorFocused(Some(path.clone()))], &inv);
        let doc = Document { path, language: "lua".into(), version: 2, text: format!("{body}\nGetEntityCo") };
        let coords = |kit: &mut DevKit| {
            kit.completion(&doc, Position::new(3, 11), None).iter().any(|i| i.label == "GetEntityCoords")
        };
        assert!(coords(&mut kit));
        kit.handle_events(&[Event::Scheduled], &inv);
        let _ = kit.run_command(natives::UPDATE, &inv, None);
        assert!(kit.natives.is_loaded(natives::NativeSet::Server), "an update that changed nothing keeps the catalogs");
        assert!(coords(&mut kit));
    }

    #[test]
    fn editor_calls_never_load_a_catalog() {
        let host = openidea_plugin::mock::MockHost::install();
        let (mut kit, path, body) = windows_server_resource(&host);
        kit.natives.invalidate(&natives::NativeSet::ALL);
        let doc = |version| Document {
            path: path.clone(),
            language: "lua".into(),
            version,
            text: format!("{body}\nGetEntityCo"),
        };
        let coords = |kit: &mut DevKit, version| {
            kit.completion(&doc(version), Position::new(3, 11), None).iter().any(|i| i.label == "GetEntityCoords")
        };
        assert!(!coords(&mut kit, 2));
        assert!(!kit.natives.is_loaded(natives::NativeSet::Server), "left to the next heavy call");
        kit.handle_events(&[Event::EditorFocused(Some(path.clone()))], &host.invocation());
        assert!(kit.natives.is_loaded(natives::NativeSet::Server));
        assert!(coords(&mut kit, 3));
    }

    #[test]
    fn huge_and_generated_files_are_left_alone() {
        let host = openidea_plugin::mock::MockHost::install();
        let (mut kit, path, _) = windows_server_resource(&host);
        for text in
            [format!("local t = {{ {} }}\nGetEntityCo", "1, ".repeat(MAX_INDEXED / 3)), "---@meta\nGetEntityCo".into()]
        {
            let lines = text.lines().count() as u32;
            let doc = Document { path: path.clone(), language: "lua".into(), version: 9, text };
            assert!(kit.completion(&doc, Position::new(lines - 1, 11), None).is_empty());
            assert!(kit.line_markers(&doc).is_empty());
        }
    }

    const SHOP_CLIENT: &str = "/project/res/client/main.lua";
    const SHOP_SERVER: &str = "/project/res/server/main.lua";

    /// A resource whose client triggers an event that nothing handles, read to the end.
    fn shop_resource(host: &openidea_plugin::mock::MockHost) -> DevKit {
        let manifest =
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'\n";
        host.add_file("res/fxmanifest.lua", manifest);
        host.add_file("res/client/main.lua", "TriggerServerEvent('shop:sell', 1)\n");
        host.add_file("res/server/main.lua", "RegisterNetEvent('shop:buy', function(n) end)\n");
        let mut kit = DevKit::default();
        kit.activate(&host.context("sensity.cfxlua")).unwrap();
        while kit.indexing {
            kit.handle_events(&[Event::EditorFocused(None)], &host.invocation());
        }
        kit
    }

    fn lua(path: &str, version: u64, text: &str) -> Document {
        Document { path: path.to_owned(), language: "lua".into(), version, text: text.to_owned() }
    }

    /// The features that have not indexed the document being edited.
    fn waiting(kit: &DevKit) -> Vec<&'static str> {
        let left = kit.stale.as_ref().map_or(0, |stale| stale.features);
        FEATURE_NAMES.iter().enumerate().filter(|(n, _)| left & (1 << n) != 0).map(|(_, name)| *name).collect()
    }

    #[test]
    fn a_name_being_typed_is_completed_before_the_document_is_indexed() {
        let host = openidea_plugin::mock::MockHost::install();
        let mut kit = shop_resource(&host);
        let typed = lua(SHOP_SERVER, 2, "GetEnt\nRegisterNetEvent('shop:sell', function(n) end)\n");
        assert!(kit.completion(&typed, Position::new(0, 6), None).iter().any(|i| i.label == "GetEntityCoords"));
        assert_eq!(waiting(&kit), ["statebags", "events", "nui", "values", "coords", "perf", "editor", "escrow"]);
        assert!(!kit.document_highlights(&typed, Position::new(1, 3)).is_empty());
        assert!(kit.stale.is_none(), "the first call that is not a completion indexes the document");
        let again = lua(SHOP_SERVER, 3, &typed.text);
        assert!(!kit.completion(&again, Position::new(0, 6), None).is_empty());
        assert!(kit.stale.is_none(), "the same text is not indexed twice");
    }

    #[test]
    fn a_completion_that_reads_an_index_indexes_the_document_first() {
        let host = openidea_plugin::mock::MockHost::install();
        let mut kit = shop_resource(&host);
        let typed = lua(SHOP_CLIENT, 2, "AddEventHandler('typed:here', function() end)\nTriggerEvent('ty')\n");
        let labels: Vec<String> =
            kit.completion(&typed, Position::new(1, 16), None).into_iter().map(|i| i.label).collect();
        assert!(labels.iter().any(|l| l == "typed:here"), "the event of the line above: {labels:?}");
        assert_eq!(waiting(&kit), ["statebags", "values", "coords", "perf", "editor", "escrow"]);
        let typed = lua(SHOP_CLIENT, 3, "LocalPlayer.state.typedKey = 1\nlocal v = LocalPlayer.state.ty\n");
        let labels: Vec<String> =
            kit.completion(&typed, Position::new(1, 30), None).into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["typedKey"], "the key of the line above");
        assert_eq!(waiting(&kit), ["events", "nui", "values", "coords", "perf", "editor", "escrow"]);
    }

    #[test]
    fn an_edited_document_is_indexed_before_another_one_is_answered() {
        let host = openidea_plugin::mock::MockHost::install();
        let mut kit = shop_resource(&host);
        let client = lua(SHOP_CLIENT, 1, "TriggerServerEvent('shop:sell', 1)\n");
        assert!(kit.line_markers(&client).is_empty(), "nothing handles the event yet");
        let typed = lua(SHOP_SERVER, 2, "GetEnt\nRegisterNetEvent('shop:sell', function(n) end)\n");
        assert!(!kit.completion(&typed, Position::new(0, 6), None).is_empty());
        assert!(kit.stale.is_some());
        assert_eq!(kit.line_markers(&client).len(), 1, "the handler typed in the other document");
        assert!(kit.stale.is_none());
    }

    #[test]
    fn an_edited_document_is_indexed_before_an_event_or_a_command() {
        let host = openidea_plugin::mock::MockHost::install();
        let mut kit = shop_resource(&host);
        let inv = host.invocation();
        for flush in 0..2 {
            let text = format!("GetEnt\nRegisterNetEvent('shop:sell', function(n) end)\n-- {flush}\n");
            assert!(!kit.completion(&lua(SHOP_SERVER, 2 + flush, &text), Position::new(0, 6), None).is_empty());
            assert!(kit.stale.is_some());
            if flush == 0 {
                kit.handle_events(&[Event::EditorFocused(None)], &inv);
            } else {
                let _ = kit.run_command("show-docs", &inv, None);
            }
            assert!(kit.stale.is_none(), "flush {flush}");
        }
        let client = lua(SHOP_CLIENT, 1, "TriggerServerEvent('shop:sell', 1)\n");
        assert_eq!(kit.line_markers(&client).len(), 1);
    }

    #[test]
    #[ignore = "measurement: CFXLUA_FILES=<file>;<file> cargo test --release --lib edit_cost_of_real_files -- --ignored --nocapture"]
    fn edit_cost_of_real_files() {
        let host = openidea_plugin::mock::MockHost::install();
        let (mut kit, path, _) = windows_server_resource(&host);
        let files = std::env::var("CFXLUA_FILES").expect("CFXLUA_FILES");
        for (n, file) in files.split(';').filter(|file| !file.is_empty()).enumerate() {
            let text = std::fs::read_to_string(file).expect("a readable file").replace("\r\n", "\n");
            let typed = format!("GetEnt\n{text}");
            let at = Position::new(0, 6);
            eprintln!("{} bytes, what reading the file costs, each stage after the ones above it:", typed.len());
            let stage = |what: &str, run: &mut dyn FnMut()| {
                let start = std::time::Instant::now();
                run();
                eprintln!("  {what}: {:?}", start.elapsed());
            };
            let mut fine = None;
            stage("lexer", &mut || fine = Some(lua::token::lex(&typed)));
            stage("tokens of the features", &mut || _ = lua::lexer::coarse(&typed, fine.as_ref().unwrap()));
            let mut src = None;
            stage("source (lexer, tokens, brackets, lines)", &mut || src = Some(Source::new(&typed)));
            let src = src.unwrap();
            stage("tree", &mut || _ = src.tree());
            stage("scopes", &mut || _ = src.scopes());
            stage("documentation comments", &mut || _ = src.docs());
            stage("events model", &mut || _ = events::ast::parse(&src));
            stage("perf model", &mut || _ = perf::ast::parse(&src));
            let end = typed.len();
            stage("caret tests of the completions, at the end of the file", &mut || {
                _ = (src.quoted_at(end), src.names_field(end), src.in_parentheses(end));
            });
            let ws = kit.workspace_for(&path);
            let ws = &kit.workspaces[ws];
            for (name, feature) in FEATURE_NAMES.iter().zip(all_features()) {
                stage(&format!("index {name}"), &mut || feature.index_file(ws, &slash(&path), FileKind::Lua, &src));
            }
            let doc = Document { path: path.clone(), language: "lua".into(), version: (n + 2) as _, text: typed };
            eprintln!("{} bytes, what completing the name costs, feature by feature:", doc.text.len());
            let start = std::time::Instant::now();
            let ws = kit.prepare(&doc).expect("a document the plugin reads");
            eprintln!("  document: {:?}", start.elapsed());
            kit.with_ctx(ws, |cx, features| {
                let at = cx.doc.offset(at);
                for (name, feature) in FEATURE_NAMES.iter().zip(features) {
                    let start = std::time::Instant::now();
                    let count = feature.completion(cx, at, None).len();
                    eprintln!("  {name}: {count} in {:?}", start.elapsed());
                }
            });
            kit.last = None;
            eprintln!("{} bytes, what the editor asks after an edit, in its order:", doc.text.len());
            let mut timed = |what: &str, run: &mut dyn FnMut(&mut DevKit) -> usize| {
                let start = std::time::Instant::now();
                let count = run(&mut kit);
                eprintln!("  {what}: {count} in {:?}", start.elapsed());
            };
            timed("completion", &mut |kit| kit.completion(&doc, at, None).len());
            timed("occurrences", &mut |kit| kit.document_highlights(&doc, at).len());
            timed("line markers and problems", &mut |kit| kit.line_markers(&doc).len());
            timed("code actions", &mut |kit| kit.code_actions(&doc, Range::new(at, at)).len());
            timed("semantic tokens", &mut |kit| kit.semantic_tokens(&doc).len());
            timed("folding", &mut |kit| kit.folding(&doc).len());
            timed("structure", &mut |kit| kit.document_symbols(&doc).len());
        }
    }

    #[test]
    #[ignore = "measurement: CFXLUA_PROJECT=<dir> cargo test --release --lib edit_cost_in_a_real_project -- --ignored --nocapture"]
    fn edit_cost_in_a_real_project() {
        let all = testing::real_project();
        let host = openidea_plugin::mock::MockHost::install();
        for (rel, text) in &all {
            host.add_file(rel, text);
        }
        let mut kit = DevKit::default();
        let start = std::time::Instant::now();
        kit.activate(&host.context("sensity.cfxlua")).unwrap();
        let activation = start.elapsed();
        let mut batches = 0;
        while kit.indexing && batches < 5_000 {
            kit.handle_events(&[Event::EditorFocused(None)], &host.invocation());
            batches += 1;
        }
        eprintln!(
            "{} files: activation {activation:?}, read in {:?} over {batches} event batches",
            all.len(),
            start.elapsed()
        );
        let small = |(p, t): &&(String, String)| {
            p.ends_with(".lua") && !p.ends_with("fxmanifest.lua") && (4_000..8_000).contains(&t.len())
        };
        let (rel, text) = all.iter().find(small).expect("a small script");
        let path = format!("/project/{rel}");
        for typed in ["GetEnt", "GetEnti", "GetEntit"] {
            let text = format!("{typed}\n{text}");
            let at = Position::new(0, typed.len() as u32);
            let doc = Document { path: path.clone(), language: "lua".into(), version: typed.len() as _, text };
            eprintln!("{} bytes, `{typed}` typed:", doc.text.len());
            let mut timed = |what: &str, run: &mut dyn FnMut(&mut DevKit) -> usize| {
                let start = std::time::Instant::now();
                let count = run(&mut kit);
                eprintln!("  {what}: {count} in {:?}", start.elapsed());
            };
            timed("workspace", &mut |kit| kit.workspace_for(&doc.path));
            timed("prioritize", &mut |kit| {
                let ws = kit.workspace_for(&doc.path);
                kit.prioritize(ws, &doc.path);
                0
            });
            timed("step", &mut |kit| {
                kit.step(EDIT);
                0
            });
            timed("status", &mut |kit| {
                kit.update_status(Some(&doc.path));
                0
            });
            timed("prepare", &mut |kit| kit.prepare(&doc).unwrap_or(0));
            timed("completion", &mut |kit| kit.completion(&doc, at, None).len());
            timed("occurrences", &mut |kit| kit.document_highlights(&doc, at).len());
            timed("line markers and problems", &mut |kit| kit.line_markers(&doc).len());
            timed("semantic tokens", &mut |kit| kit.semantic_tokens(&doc).len());
        }
    }

    #[test]
    #[ignore = "measurement: CFXLUA_PROJECT=<dir> cargo test --release --lib index_cost_of_a_real_project -- --ignored --nocapture"]
    fn index_cost_of_a_real_project() {
        let files: Vec<(String, String)> = testing::real_project()
            .into_iter()
            .filter(|(p, t)| project::file_kind(p) != FileKind::Other && t.len() <= workspace::MAX_FILE)
            .collect();
        let bytes: usize = files.iter().map(|(_, t)| t.len()).sum();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let ws = Workspace::from_files("/real", &refs);
        let features = all_features();
        let names = FEATURE_NAMES;
        assert_eq!(names.len(), features.len(), "one name per feature, in the order of all_features()");
        let mut parse_time = std::time::Duration::ZERO;
        let mut index_time = vec![std::time::Duration::ZERO; features.len()];
        for f in ws.files() {
            let Some(text) = f.text.as_deref() else { continue };
            let start = std::time::Instant::now();
            let src = parse(f.kind, text);
            parse_time += start.elapsed();
            for (n, feature) in features.iter().enumerate() {
                let start = std::time::Instant::now();
                feature.index_file(&ws, &f.path, f.kind, &src);
                index_time[n] += start.elapsed();
            }
        }
        eprintln!("{} files, {bytes} bytes: parse {parse_time:?}", files.len());
        for ((feature, name), t) in features.iter().zip(names).zip(index_time) {
            let start = std::time::Instant::now();
            feature.indexed(&ws);
            eprintln!("{name}: index_file {t:?}, indexed {:?}", start.elapsed());
        }
        let (path, text) =
            files.iter().filter(|(p, _)| p.ends_with(".lua")).max_by_key(|(_, t)| t.len() / 1000 % 7).unwrap();
        let path = format!("/real/{path}");
        let edited = format!("{text}\nlocal typed = 1\n");
        let mut ws = ws;
        ws.set(&path, Some(&edited));
        let timed = |what: &str, run: &dyn Fn() -> usize| {
            let start = std::time::Instant::now();
            let count = run();
            eprintln!("keystroke, {} bytes, {what}: {count} in {:?}", edited.len(), start.elapsed());
        };
        timed("tokens", &|| parse(project::file_kind(&path), &edited).toks.len());
        let models = parse(project::file_kind(&path), &edited);
        timed("tree (nodes)", &|| models.tree().len());
        timed("scopes (declarations)", &|| models.scopes().decls().len());
        timed("events model (expressions)", &|| events::ast::Ast::parse(&models).exprs.len());
        timed("perf model (expressions)", &|| perf::ast::Ast::new(&models).exprs.len());
        timed("syntax problems", &|| syntax::check(&models).len());
        let src = parse(project::file_kind(&path), &edited);
        for (feature, name) in features.iter().zip(names) {
            let start = std::time::Instant::now();
            feature.index_file(&ws, &path, project::file_kind(&path), &src);
            let indexed = start.elapsed();
            feature.indexed(&ws);
            eprintln!("keystroke {name}: index_file {indexed:?}, indexed {:?}", start.elapsed() - indexed);
        }
        // What completing a native name costs in a small file of this project, feature by feature.
        let (path, text) = files
            .iter()
            .filter(|(p, t)| p.ends_with(".lua") && !p.ends_with("fxmanifest.lua") && (4_000..8_000).contains(&t.len()))
            .find(|(p, _)| ws.resource_of(&format!("/real/{p}")).is_some())
            .expect("a small script of a resource");
        let path = format!("/real/{path}");
        let typed = format!("{text}\nGetEnt");
        let (settings, natives) = (Settings::defaults(), Natives::bundled());
        let start = std::time::Instant::now();
        let doc = Doc::new(&ws, &path, "lua", 2, &typed);
        eprintln!("completion in {} bytes: document {:?}", typed.len(), start.elapsed());
        let cx = Ctx { doc: &doc, ws: &ws, settings: &settings, natives: &natives };
        let start = std::time::Instant::now();
        let games = ws.detected_games();
        eprintln!("completion: {} detected games in {:?}", games.len(), start.elapsed());
        for round in ["first", "second"] {
            for (feature, name) in features.iter().zip(names) {
                let start = std::time::Instant::now();
                let count = feature.completion(&cx, typed.len(), None).len();
                eprintln!("completion, {round} call, {name}: {count} in {:?}", start.elapsed());
            }
        }
    }

    #[test]
    #[ignore = "measurement: cargo test --release --lib index_cost_of_a_large_project -- --ignored --nocapture"]
    fn index_cost_of_a_large_project() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut lua = testing::files_under(&root.join("data/runtime"));
        lua.extend(testing::files_under(&root.join("examples")));
        let bodies: Vec<String> = lua.into_iter().filter(|(p, _)| p.ends_with(".lua")).map(|(_, t)| t).collect();
        let mut files = Vec::new();
        for r in 0..150 {
            let manifest =
                "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'\n";
            files.push((format!("res{r}/fxmanifest.lua"), manifest.to_owned()));
            for (i, body) in bodies.iter().enumerate() {
                let side = if i % 2 == 0 { "client" } else { "server" };
                files.push((format!("res{r}/{side}/f{i}.lua"), body.replace("shop:", &format!("res{r}:"))));
            }
        }
        let lines: usize = files.iter().map(|(_, t)| t.lines().count()).sum();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let ws = Workspace::from_files("/big", &refs);
        let features = all_features();
        let start = std::time::Instant::now();
        for f in ws.files() {
            let Some(text) = f.text.as_deref() else { continue };
            let src = parse(f.kind, text);
            for feature in &features {
                feature.index_file(&ws, &f.path, f.kind, &src);
            }
        }
        let indexed = start.elapsed();
        for feature in &features {
            feature.indexed(&ws);
        }
        eprintln!(
            "{} files, {lines} lines: index_file {indexed:?}, indexed {:?}",
            files.len(),
            start.elapsed() - indexed
        );
        let names = FEATURE_NAMES;
        assert_eq!(names.len(), features.len(), "one name per feature, in the order of all_features()");
        let (path, text) =
            files.iter().find(|(p, t)| p.starts_with("res7/client/") && t.contains("TriggerServerEvent")).unwrap();
        let path = format!("/big/{path}");
        let settings = Settings::defaults();
        let natives = Natives::bundled();
        let query = |ws: &Workspace, text: &str, label: &str| {
            let doc = Doc::new(ws, &path, "lua", 1, text);
            let cx = Ctx { doc: &doc, ws, settings: &settings, natives: &natives };
            for (feature, name) in features.iter().zip(names) {
                let start = std::time::Instant::now();
                let n = feature.problems(&cx).len() + feature.completion(&cx, text.len(), None).len();
                eprintln!("{label} {name}: problems+completion {:?} ({n})", start.elapsed());
            }
        };
        query(&ws, text, "cold");
        query(&ws, text, "warm");
        let edited = format!("{text}\nlocal typed = 1\n");
        let mut ws = ws;
        ws.set(&path, Some(&edited));
        let src = parse(project::file_kind(&path), &edited);
        for feature in &features {
            feature.index_file(&ws, &path, project::file_kind(&path), &src);
            feature.indexed(&ws);
        }
        query(&ws, &edited, "keystroke");

        let mut heavy = Vec::new();
        for r in 0..440 {
            heavy.push((
                format!("r{r}/fxmanifest.lua"),
                "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'cl.lua'\nserver_script 'sv.lua'\n".to_owned(),
            ));
            let mut server = String::new();
            for k in 0..55 {
                server.push_str(&format!(
                    "RegisterNetEvent('r{r}:e{k}')\nAddEventHandler('r{r}:e{k}', function(a, b)\n    TriggerClientEvent('r{r}:c{k}', source, a, b)\nend)\n"
                ));
            }
            heavy.push((format!("r{r}/sv.lua"), server));
        }
        let refs: Vec<(&str, &str)> = heavy.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let mut ws = Workspace::from_files("/heavy", &refs);
        let events = events::EventsFeature::default();
        for f in ws.files() {
            if let Some(text) = f.text.as_deref() {
                events.index_file(&ws, &f.path, f.kind, &parse(f.kind, text));
            }
        }
        let start = std::time::Instant::now();
        events.indexed(&ws);
        eprintln!("heavy events: {} files, first build {:?}", heavy.len(), start.elapsed());
        let path = "/heavy/r7/sv.lua";
        let edited = format!("local typed = 1\n{}", ws.text(path).unwrap());
        ws.set(path, Some(&edited));
        let start = std::time::Instant::now();
        events.index_file(&ws, path, FileKind::Lua, &parse(FileKind::Lua, &edited));
        let file = start.elapsed();
        events.indexed(&ws);
        eprintln!("heavy events keystroke: index_file {file:?}, indexed {:?}", start.elapsed() - file);
    }

    #[test]
    fn demo_resources_end_to_end() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let files = testing::files_under(&root);
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let ws = Workspace::from_files("/demo", &refs);
        let features = all_features();
        for f in ws.files() {
            let Some(text) = f.text.as_deref() else { continue };
            let src = parse(f.kind, text);
            for feature in &features {
                feature.index_file(&ws, &f.path, f.kind, &src);
            }
        }
        for feature in &features {
            feature.indexed(&ws);
        }
        let settings = Settings::defaults();
        let natives = Natives::bundled();
        assert_eq!(ws.side_of("/demo/resources/[demo]/shop/client/main.lua"), Side::Client);
        assert_eq!(ws.side_of("/demo/resources/[demo]/shop/server/main.lua"), Side::Server);
        assert_eq!(ws.side_of("/demo/resources/[demo]/shop/shared/config.lua"), Side::Shared);
        assert_eq!(ws.side_of("/demo/resources/[demo]/garage/cl_garage.lua"), Side::Client);
        let mut report = String::new();
        for (rel, text) in &files {
            let path = format!("/demo/{rel}");
            let doc = Doc::new(&ws, &path, testing::language_of(rel), 1, text);
            let cx = Ctx { doc: &doc, ws: &ws, settings: &settings, natives: &natives };
            for p in features.iter().flat_map(|f| f.problems(&cx)) {
                let pos = doc.position(p.start);
                report.push_str(&format!("{rel}:{}: {:?} [{}] {}\n", pos.line + 1, p.severity, p.code, p.message));
            }
            for m in features.iter().flat_map(|f| f.line_markers(&cx)) {
                report.push_str(&format!(
                    "{rel}:{}: marker {:?} {}\n",
                    m.line + 1,
                    m.icon,
                    m.tooltip.replace('\n', " | ")
                ));
            }
        }
        eprintln!("{report}");
        assert!(!report.contains("Error ["), "the demo has no errors:\n{report}");

        let client = "/demo/resources/[demo]/shop/client/main.lua";
        let text = ws.text(client).unwrap().replace("local bIsShopOpen", "GetEntity\nlocal bIsShopOpen");
        let doc = Doc::new(&ws, client, "lua", 2, &text);
        let cx = Ctx { doc: &doc, ws: &ws, settings: &settings, natives: &natives };
        let at = text.find("GetEntity").unwrap() + "GetEntity".len();
        let items: Vec<String> = features.iter().flat_map(|f| f.completion(&cx, at, None)).map(|i| i.label).collect();
        assert!(items.iter().any(|l| l == "GetEntityCoords"), "{items:?}");
        assert!(!items.iter().any(|l| l == "GetPlayerIdentifiers"), "no server natives in a client script");
    }
}
