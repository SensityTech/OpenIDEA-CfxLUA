pub mod coords;
pub mod docs;
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
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
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
const SCAN_BUDGET_MS: u128 = 40_000;
// Indexing slices: at activation, per event batch, and on the editing path (shared 50 ms budget).
const ACTIVATION_INDEX_MS: u128 = 1_000;
const EVENT_INDEX_MS: u128 = 1_500;
const EDIT_INDEX_MS: u128 = 4;

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
        Box::<escrow::EscrowFeature>::default(),
        Box::<luals::LualsFeature>::default(),
    ]
}

pub struct DevKit {
    features: Vec<Box<dyn Feature>>,
    docs: docs::Docs,
    settings: Settings,
    natives: Natives,
    workspaces: Vec<Workspace>,
    last: Option<(Doc, Option<Vec<Problem>>)>,
    pending: Vec<(usize, String)>,
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
            pending: Vec::new(),
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
        let start = std::time::Instant::now();
        let i = self.workspace_for(&format!("{}/", slash(root).trim_end_matches('/')));
        let task = host::begin_task("CfxLua: indexing resources", true);
        let mut paths: Vec<String> = Vec::new();
        for (glob, limit) in workspace::SCAN {
            match project.find_files(glob, *limit) {
                Ok(found) => paths.extend(found),
                Err(e) => host::log(LogLevel::Warn, &format!("listing {glob}: {e}")),
            }
        }
        paths.sort();
        paths.dedup();
        let total = paths.len() as u32;
        let ws = &mut self.workspaces[i];
        for (n, path) in paths.iter().enumerate() {
            if task.is_cancelled() || start.elapsed().as_millis() > SCAN_BUDGET_MS {
                host::log(LogLevel::Warn, &format!("indexing stopped after {n} of {total} files"));
                break;
            }
            if n % 64 == 0 {
                task.report(n as u32, Some(total), Some(text::file_name(&slash(path))));
            }
            let kind = project::file_kind(path);
            let text = match kind {
                FileKind::Other => None,
                _ => project.read_file(path).ok(),
            };
            ws.set(path, text.as_deref());
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
        let readable: Vec<String> = ws.files().filter(|f| f.text.is_some()).map(|f| f.path.clone()).collect();
        self.pending.extend(readable.into_iter().map(|p| (i, p)));
        let games = natives::project_games(&self.settings, &self.workspaces[i].detected_games());
        for set in games.into_iter().map(natives::NativeSet::for_game).chain([natives::NativeSet::Server]) {
            self.natives.catalog(set);
        }
        self.index_pending(ACTIVATION_INDEX_MS);
        host::log(
            LogLevel::Info,
            &format!(
                "read {total} files of {root} in {} ms, {} left to index",
                start.elapsed().as_millis(),
                self.pending.len()
            ),
        );
    }

    // A call has a fixed CPU budget (fuel): big projects are indexed in slices over several calls.
    fn index_pending(&mut self, budget_ms: u128) {
        if self.pending.is_empty() {
            return;
        }
        let start = std::time::Instant::now();
        let mut touched: Vec<usize> = Vec::new();
        while start.elapsed().as_millis() < budget_ms {
            let Some((i, path)) = self.pending.pop() else { break };
            let ws = &self.workspaces[i];
            let Some(f) = ws.file(&path) else { continue };
            let Some(text) = f.text.as_deref() else { continue };
            let src = parse(f.kind, text);
            for feature in &self.features {
                feature.index_file(ws, &f.path, f.kind, &src);
            }
            if !touched.contains(&i) {
                touched.push(i);
            }
        }
        for i in touched {
            for feature in &self.features {
                feature.indexed(&self.workspaces[i]);
            }
        }
        if self.pending.is_empty() {
            self.last = None;
        }
    }

    fn reread(&mut self, path: &str) {
        let i = self.workspace_for(path);
        if !self.workspaces[i].scanned {
            return;
        }
        let kind = project::file_kind(path);
        let text = match host::projects() {
            Ok(ps) => ps.into_iter().find_map(|p| p.read_file(path).ok()),
            Err(_) => None,
        };
        let ws = &mut self.workspaces[i];
        match text {
            Some(t) => ws.set(path, Some(&t)),
            None => ws.remove(path),
        }
        let ws = &self.workspaces[i];
        match ws.text(path) {
            Some(t) => {
                let src = parse(kind, t);
                for f in &self.features {
                    f.index_file(ws, &slash(path), kind, &src);
                    f.indexed(ws);
                }
            }
            None => {
                for f in &self.features {
                    f.forget_file(&slash(path));
                    f.indexed(ws);
                }
            }
        }
        self.last = None;
    }

    fn prepare(&mut self, d: &Document) -> Option<usize> {
        if !self.settings.enabled() {
            return None;
        }
        self.index_pending(EDIT_INDEX_MS);
        let i = self.workspace_for(&d.path);
        let path = slash(&d.path);
        if let Some((doc, _)) = &self.last
            && doc.path == path
            && doc.version == d.version
        {
            return Some(i);
        }
        self.settings.reset();
        let ws = &mut self.workspaces[i];
        let changed = ws.text(&path) != Some(d.text.as_str());
        ws.set(&path, Some(&d.text));
        let ws = &self.workspaces[i];
        let doc = Doc::new(ws, &path, &d.language, d.version, &d.text);
        if changed {
            for f in &self.features {
                f.index_file(ws, &path, doc.kind, &doc.src);
                f.indexed(ws);
            }
        }
        self.last = Some((doc, None));
        Some(i)
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
                range: doc.range(p.start, p.end),
                severity: p.severity,
                message: p.message.clone(),
                code: Some(p.code.to_owned()),
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
            let path = e.path();
            let i = ws.unwrap_or_else(|| self.workspace_for(&path));
            Some(Doc::new(&self.workspaces[i], &path, &e.language(), d.version, &d.text))
        });
        (ws, doc)
    }

    fn update_status(&mut self, path: Option<&str>) {
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

fn parse(kind: FileKind, text: &str) -> Source {
    if matches!(kind, FileKind::Lua | FileKind::Manifest) && !project::is_escrowed(text) {
        Source::new(text)
    } else {
        Source::plain(text)
    }
}

impl Plugin for DevKit {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileSaved, EventKind::EditorFocused, EventKind::Scheduled] {
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
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        self.settings.reset();
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
        if matches!(id, "natives.update" | "natives.scheduled-update") {
            // The catalogs reload with the stored update on next use.
            self.natives.invalidate();
            self.last = None;
        }
        result
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.index_pending(EVENT_INDEX_MS);
        for e in events {
            match e {
                Event::FileSaved(path) => {
                    self.reread(path);
                    if project::file_kind(path) == FileKind::Manifest {
                        self.update_status(Some(path));
                    }
                }
                Event::EditorFocused(path) => self.update_status(path.as_deref()),
                Event::Scheduled => {
                    let _ = self.run_command("natives.scheduled-update", ctx, None);
                }
                _ => {}
            }
        }
    }
}

impl Providers for DevKit {
    fn completion(&mut self, d: &Document, at: Position, trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(ws) = self.prepare(d) else { return Vec::new() };
        self.with_ctx(ws, |cx, features| {
            let at = cx.doc.offset(at);
            features.iter().flat_map(|f| f.completion(cx, at, trigger)).collect()
        })
        .unwrap_or_default()
    }

    fn definition(&mut self, d: &Document, at: Position) -> Vec<Location> {
        let Some(ws) = self.prepare(d) else { return Vec::new() };
        let found = self
            .with_ctx(ws, |cx, features| {
                let at = cx.doc.offset(at);
                features.iter().flat_map(|f| f.definition(cx, at)).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        found.into_iter().map(|l| self.native_location(ws, l)).collect()
    }

    fn line_markers(&mut self, d: &Document) -> Vec<LineMarker> {
        let Some(ws) = self.prepare(d) else {
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
        let Some(ws) = self.prepare(d) else { return Vec::new() };
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
                CodeAction { title: fix.title, kind: Some(kind.to_owned()), preferred: fix.preferred, edits }
            })
            .collect()
    }
}

impl Views for DevKit {
    fn render(&mut self, view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
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
        self.settings.reset();
        let (ws, doc) = self.command_doc(ctx);
        let cx = CommandCx {
            ws: ws.map(|i| &self.workspaces[i]),
            settings: &self.settings,
            natives: &self.natives,
            doc: doc.as_ref(),
            inv: ctx,
        };
        self.docs.handle(&cx, view, event)
    }
}

register_plugin!(DevKit, providers, views);

#[cfg(test)]
mod tests {
    use super::*;

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

    fn demo_files(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                demo_files(&path, root, out);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((slash(&path.strip_prefix(root).unwrap().to_string_lossy()), text));
            }
        }
    }

    #[test]
    #[ignore = "measurement: cargo test --release --lib index_cost_of_a_large_project -- --ignored --nocapture"]
    fn index_cost_of_a_large_project() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut lua = Vec::new();
        demo_files(&root.join("data/runtime"), &root.join("data/runtime"), &mut lua);
        demo_files(&root.join("examples"), &root.join("examples"), &mut lua);
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
    }

    #[test]
    fn demo_resources_end_to_end() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut files = Vec::new();
        demo_files(&root, &root, &mut files);
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
