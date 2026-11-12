use super::facts;
use super::rules::{self, EscrowFile, NUI_EXTENSIONS, Status};
use crate::feature::CommandCx;
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::manifest::catalog::{PathKind, RDR3_WARNING};
use crate::manifest::files::{Tree, checkable, is_glob, normalize};
use crate::manifest::view::View;
use crate::project::glob::Glob;
use crate::project::{CASE_SENSITIVE, Resource};
use crate::workspace::Workspace;
use openidea_plugin::{NotificationLevel, host};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub const COMMAND: &str = "escrow.check";
const MAX_LISTED_FILES: usize = 40;
const NOTIFIED: usize = 5;
const NOTIFIED_RESOURCES: usize = 20;
const SCRIPT_DIRECTIVES: [&str; 6] =
    ["client_script", "client_scripts", "server_script", "server_scripts", "shared_script", "shared_scripts"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Blocker,
    Warning,
    Note,
}

impl Level {
    const ALL: [Level; 3] = [Level::Blocker, Level::Warning, Level::Note];

    fn heading(self) -> &'static str {
        match self {
            Self::Blocker => "Blockers",
            Self::Warning => "Warnings",
            Self::Note => "Notes",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub level: Level,
    pub message: String,
    pub path: Option<String>,
    pub line: usize,
    pub pattern: Option<String>,
}

impl Finding {
    fn new(level: Level, message: String, path: Option<String>, line: usize, pattern: Option<String>) -> Self {
        Self { level, message, path, line, pattern }
    }

    fn markdown(&self) -> String {
        let location = match (&self.path, self.line) {
            (Some(p), 0) => format!("`{p}`: "),
            (Some(p), l) => format!("`{p}:{l}`: "),
            (None, _) => String::new(),
        };
        let fix = self.pattern.as_ref().map(|p| format!(" Fix: add `{p}` to escrow_ignore.")).unwrap_or_default();
        format!("- {location}{}{fix}", self.message)
    }
}

pub struct Report {
    pub name: String,
    pub folder: String,
    pub findings: Vec<Finding>,
    pub files: Rc<Vec<EscrowFile>>,
    pub unchecked: usize,
}

impl Report {
    pub fn count(&self, level: Level) -> usize {
        self.findings.iter().filter(|f| f.level == level).count()
    }

    pub fn patterns(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.findings.iter().filter_map(|f| f.pattern.as_ref()) {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }

    fn with_status(&self, status: Status) -> impl Iterator<Item = &EscrowFile> {
        self.files.iter().filter(move |f| f.status == status)
    }
}

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", grouped(n), if n == 1 { one } else { many })
}

/// Checks the resource of the focused file, or every resource, notifies a summary and returns the folders checked.
pub fn run(cx: &CommandCx) -> Vec<String> {
    if !cx.settings.enabled() {
        return Vec::new();
    }
    let resources = cx.ws.map(|ws| resources_for(ws, cx.doc.map(|d| d.path.as_str()))).unwrap_or_default();
    let (Some(ws), false) = (cx.ws, resources.is_empty()) else {
        host::notify(NotificationLevel::Info, "Escrow check: no resource (folder with an fxmanifest.lua) found", "");
        return Vec::new();
    };
    let reports: Vec<Rc<Report>> = resources.iter().map(|r| cached(ws, r)).collect();
    let (level, title, body) = notification(&reports);
    host::notify(level, &title, &format!("{body}\nThe full report is in the Escrow tab of the CfxLua tool window."));
    resources.iter().map(|r| r.dir.clone()).collect()
}

pub fn resources_for<'a>(ws: &'a Workspace, path: Option<&str>) -> Vec<&'a Resource> {
    if let Some(r) = path.and_then(|p| ws.resource_of(p)) {
        return vec![r];
    }
    let mut all: Vec<&Resource> = ws.resources().collect();
    all.sort_by_key(|r| r.name.to_lowercase());
    all
}

pub fn syntax_error(src: &Source) -> Option<(usize, String)> {
    let mut first: Option<(usize, String)> = None;
    let mut unclosed: Option<(usize, String)> = None;
    let mut note = |at: usize, message: String| {
        if first.as_ref().is_none_or(|(f, _)| at < *f) {
            first = Some((at, message));
        }
    };
    for t in &src.all {
        let text = t.text(&src.text);
        match t.kind {
            Tok::Str { closed: false } | Tok::Hash { closed: false } => note(t.start, "unfinished string".into()),
            Tok::LongStr if !long_closed(text) => note(t.start, "unfinished long string".into()),
            Tok::BlockComment if !long_closed(&text[2..]) => note(t.start, "unfinished long comment".into()),
            Tok::CComment if text.len() < 4 || !text.ends_with("*/") => note(t.start, "unfinished long comment".into()),
            Tok::Unknown => note(t.start, format!("unexpected symbol '{text}'")),
            _ => {}
        }
    }
    for (i, t) in src.toks.iter().enumerate() {
        if t.kind != Tok::Op {
            continue;
        }
        let text = t.text(&src.text);
        let next = src.toks.get(i + 1);
        match text {
            "(" | "[" | "{" if src.pair[i].is_none() && unclosed.is_none() => {
                let close = match text {
                    "(" => ")",
                    "{" => "}",
                    _ => "]",
                };
                unclosed = Some((t.start, format!("'{close}' expected")));
            }
            ")" | "]" | "}" if src.pair[i].is_none() => note(t.start, format!("unexpected '{text}'")),
            "," | "=" if next.is_some_and(|n| n.kind == Tok::Op && n.text(&src.text) == text) => {
                note(next.map_or(t.start, |n| n.start), format!("unexpected '{text}'"));
            }
            _ => {}
        }
    }
    first.or(unclosed)
}

fn long_closed(text: &str) -> bool {
    let level = text.bytes().skip(1).take_while(|b| *b == b'=').count();
    let close = format!("]{}]", "=".repeat(level));
    text.len() >= 2 * (level + 2) && text.ends_with(&close)
}

fn line_of(src: &Source, at: usize) -> usize {
    src.lines.line_of(at) + 1
}

pub fn check(ws: &Workspace, res: &Resource) -> Report {
    let manifest = ws.text(&res.manifest_path()).unwrap_or("");
    let msrc = Source::new(manifest);
    let view = View::new(&msrc);
    let layout = layout(ws, res, &msrc, &view);
    let files = &layout.files;
    let m = || Some(res.manifest.clone());
    let mut out: Vec<Finding> = Vec::new();

    // Blockers: the upload fails or the resource does not start.
    if let Some((at, error)) = syntax_error(&msrc) {
        let message =
            format!("Syntax error in the manifest ({error}): the upload fails with \"error loading manifest file\"");
        out.push(Finding::new(Level::Blocker, message, m(), line_of(&msrc, at), None));
    }
    let legacy = res.manifest.eq_ignore_ascii_case("__resource.lua");
    if legacy {
        let message = "Legacy __resource.lua manifest: Asset Escrow reads fxmanifest.lua; rename it and declare fx_version and game";
        out.push(Finding::new(Level::Blocker, message.into(), m(), 0, None));
    }
    if !legacy && !view.has("fx_version") {
        let message = "Missing fx_version: FXServer does not start the resource";
        out.push(Finding::new(Level::Blocker, message.into(), m(), 0, None));
    }
    let games: Vec<&str> = view
        .decls
        .iter()
        .filter(|d| d.metadata_key() == "game")
        .flat_map(|d| d.values())
        .map(|v| v.text.as_str())
        .collect();
    if !legacy && games.is_empty() {
        let message = "Missing game: FXServer does not start the resource";
        out.push(Finding::new(Level::Blocker, message.into(), m(), 0, None));
    }
    if games.contains(&"rdr3") && !view.named("rdr3_warning").any(|d| d.values().any(|v| v.text == RDR3_WARNING)) {
        let message =
            "RedM resource without the exact rdr3_warning acknowledgement: FXServer does not start it on RedM";
        out.push(Finding::new(Level::Blocker, message.into(), m(), 0, None));
    }

    // Warnings: the protected build does not work as expected.
    if !view.has("escrow_ignore") {
        let message = "No escrow_ignore: every Lua, YFT, YDD and YDR file will be encrypted, configuration included";
        out.push(Finding::new(Level::Warning, message.into(), m(), 0, None));
    }
    out.extend(layout.editable.iter().cloned());
    let manifest_path = res.manifest_path();
    let mut protected: Vec<&str> = Vec::new();
    let mut unchecked = 0;
    // The indexing reads every file: lexing them here again would overrun the call.
    let mut budget = if ws.indexing { 0 } else { facts::LEXED_PER_CALL };
    for f in files.iter().filter(|f| rules::extension(&f.rel) == "lua" && f.path != manifest_path) {
        if ws.stamp(&f.path).is_none() {
            if ws.indexing {
                unchecked += 1;
            }
            continue;
        }
        let Some(facts) = facts::of(ws, &f.path, &mut budget) else {
            unchecked += 1;
            continue;
        };
        if facts.protected {
            protected.push(&f.rel);
            continue;
        }
        if f.status == Status::Encrypted
            && let Some((line, error)) = &facts.syntax
        {
            let message =
                format!("Syntax error ({error}) in a file that will be encrypted: buyers will not be able to fix it.");
            out.push(Finding::new(Level::Warning, message, Some(f.rel.clone()), *line, None));
        }
        for load in &facts.loads {
            let Some((target, Status::Encrypted)) = rules::target(ws, res, load) else { continue };
            let message = match load.module {
                Some(callee) => format!(
                    "{callee} loads '{target}', which will be encrypted: it cannot load it in the protected build."
                ),
                None => format!(
                    "LoadResourceFile loads '{target}', which will be encrypted: it returns the encrypted bytes in the protected build."
                ),
            };
            out.push(Finding::new(Level::Warning, message, Some(f.rel.clone()), load.line, Some(target)));
        }
    }
    out.extend(layout.unmatched.iter().cloned());
    if !protected.is_empty() || rules::has_key_file(ws, res) {
        let examples = protected.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        let message = match protected.len() {
            0 => "The folder holds a .fxap key file: this looks like a protected build, upload the original sources."
                .to_owned(),
            1 => format!(
                "1 Lua file is already encrypted ({examples}): this looks like a protected build, upload the original sources."
            ),
            n => format!(
                "{n} Lua files are already encrypted ({examples}): this looks like a protected build, upload the original sources."
            ),
        };
        out.push(Finding::new(Level::Warning, message, None, 0, None));
    }

    // Notes: what stays readable.
    for v in view.decls.iter().filter(|d| SCRIPT_DIRECTIVES.contains(&d.name())).flat_map(|d| d.values()) {
        let value = v.text.as_str();
        if value.starts_with('@') {
            continue;
        }
        let message = match rules::extension(value).as_str() {
            "js" => format!("'{value}' is JavaScript: Asset Escrow only encrypts Lua, it ships readable."),
            "dll" => format!("'{value}' is a C# assembly: Asset Escrow only encrypts Lua, it ships as is."),
            _ => continue,
        };
        out.push(Finding::new(Level::Note, message, m(), line_of(&msrc, v.start), None));
    }
    let nui = files.iter().filter(|f| NUI_EXTENSIONS.contains(&rules::extension(&f.rel).as_str())).count();
    if nui > 0 || view.has("ui_page") {
        let message =
            format!("NUI is not supported by Asset Escrow: the HTML, JavaScript and CSS files ({nui}) ship readable.");
        out.push(Finding::new(Level::Note, message, None, 0, None));
    }
    if !view.has("lua54") && files.iter().any(|f| f.status == Status::Encrypted && rules::extension(&f.rel) == "lua") {
        let message = "No lua54 directive. The 2021 escrow announcement required lua54 set to yes; every script runs on Lua 5.4 since June 2025 and the flag has no effect: add it only if the portal asks for it.";
        out.push(Finding::new(Level::Note, message.into(), m(), 0, None));
    }

    out.sort_by_key(|f| f.level);
    let folder = match ws.relative(&res.dir) {
        "" => res.name.clone(),
        rel => rel.to_owned(),
    };
    Report { name: res.name.clone(), folder, findings: out, files: layout.files.clone(), unchecked }
}

// What follows from the manifest and the file names alone, kept while both stay the same.
struct Layout {
    dir: String,
    manifest: Arc<str>,
    listing: Vec<String>,
    files: Rc<Vec<EscrowFile>>,
    editable: Vec<Finding>,
    unmatched: Vec<Finding>,
}

thread_local! {
    static LAYOUTS: RefCell<Vec<Rc<Layout>>> = const { RefCell::new(Vec::new()) };
}

fn listing<'a>(ws: &'a Workspace, res: &'a Resource) -> impl Iterator<Item = &'a str> + 'a {
    let prefix = format!("{}/", res.dir);
    let nested = ws.resources().map(|r| r.dir.as_str()).filter(move |d| d.starts_with(&prefix));
    nested.chain(ws.files_under(&res.dir).map(|f| f.path.as_str()))
}

fn layout(ws: &Workspace, res: &Resource, msrc: &Source, view: &View) -> Rc<Layout> {
    let manifest = ws.file(&res.manifest_path()).and_then(|f| f.text.clone());
    let same = |l: &Layout| {
        manifest.as_ref().is_some_and(|m| Arc::ptr_eq(m, &l.manifest))
            && listing(ws, res).eq(l.listing.iter().map(String::as_str))
    };
    if let Some(hit) = LAYOUTS.with_borrow(|layouts| layouts.iter().find(|l| same(l)).cloned()) {
        return hit;
    }
    let files = rules::files_of(ws, res);
    let mut editable = Vec::new();
    for (pattern, group) in rules::editable_groups(&files) {
        let message = if pattern == group[0].rel {
            format!("'{pattern}' will be encrypted: buyers will not be able to edit it.")
        } else {
            let paths: Vec<&str> = group.iter().map(|f| f.rel.as_str()).collect();
            format!(
                "'{pattern}' will be encrypted ({}): buyers will not be able to edit these files.",
                paths.join(", ")
            )
        };
        editable.push(Finding::new(Level::Warning, message, Some(group[0].rel.clone()), 0, Some(pattern)));
    }
    let mut unmatched = Vec::new();
    let mut tree: Option<std::rc::Rc<Tree>> = None;
    for v in view.named("escrow_ignore").flat_map(|d| d.values()) {
        let pattern = v.text.as_str();
        if pattern.trim().is_empty()
            || !checkable(pattern, PathKind::File)
            || names_a_file(ws, res, &files, pattern)
            || tree.get_or_insert_with(|| Tree::cached(ws, &res.dir)).matches_any(pattern, PathKind::File)
        {
            continue;
        }
        let message = format!("escrow_ignore pattern '{pattern}' matches no file of the resource.");
        unmatched.push(Finding::new(Level::Warning, message, Some(res.manifest.clone()), line_of(msrc, v.start), None));
    }
    let layout = Rc::new(Layout {
        dir: res.dir.clone(),
        manifest: manifest.unwrap_or_else(|| Arc::from("")),
        listing: listing(ws, res).map(str::to_owned).collect(),
        files: Rc::new(files),
        editable,
        unmatched,
    });
    LAYOUTS.with_borrow_mut(|layouts| {
        layouts.retain(|l| l.dir != res.dir && ws.resource(&l.dir).is_some());
        layouts.push(layout.clone());
    });
    layout
}

// Enough for the tree to match it, without listing every directory of the resource per pattern.
fn names_a_file(ws: &Workspace, res: &Resource, files: &[EscrowFile], pattern: &str) -> bool {
    let p = normalize(pattern);
    if !is_glob(&p) {
        return ws.exists(&format!("{}/{p}", res.dir));
    }
    Glob::new(&p, CASE_SENSITIVE).is_some_and(|g| files.iter().any(|f| g.matches(&f.rel)))
}

struct Cached {
    manifest_path: String,
    manifest: Arc<str>,
    generation: u64,
    report: Rc<Report>,
}

impl Cached {
    fn fresh(&self, ws: &Workspace) -> bool {
        self.generation == ws.generation
            && ws
                .file(&self.manifest_path)
                .and_then(|f| f.text.as_ref())
                .is_some_and(|t| Arc::ptr_eq(t, &self.manifest))
    }
}

thread_local! {
    static REPORTS: RefCell<Vec<Cached>> = const { RefCell::new(Vec::new()) };
}

/// The report of the workspace as it is, computed once per generation.
pub fn cached(ws: &Workspace, res: &Resource) -> Rc<Report> {
    let manifest_path = res.manifest_path();
    let hit = REPORTS.with_borrow(|reports| {
        reports.iter().find(|c| c.manifest_path == manifest_path && c.fresh(ws)).map(|c| c.report.clone())
    });
    if let Some(report) = hit {
        return report;
    }
    let report = Rc::new(check(ws, res));
    if let Some(manifest) = ws.file(&manifest_path).and_then(|f| f.text.clone())
        && report.unchecked == 0
    {
        REPORTS.with_borrow_mut(|reports| {
            reports.retain(|c| c.fresh(ws) && c.manifest_path != manifest_path);
            reports.push(Cached { manifest_path, manifest, generation: ws.generation, report: report.clone() });
        });
    }
    report
}

fn unchecked_note(n: usize) -> String {
    format!(
        "Not checked yet: {} (CfxLua is still reading the project). Run CfxLua: Check Resource Before Escrow Upload again in a moment.",
        count(n, "Lua file", "Lua files")
    )
}

pub fn markdown<R: Borrow<Report>>(reports: &[R]) -> String {
    let mut md = String::new();
    let mut block = |text: &str| {
        md.push_str(text);
        md.push_str("\n\n");
    };
    let reports: Vec<&Report> = reports.iter().map(Borrow::borrow).collect();
    match reports.as_slice() {
        [one] => block(&format!("# Escrow pre-publish check: {}", one.name)),
        many => block(&format!("# Escrow pre-publish check: {} resources", grouped(many.len()))),
    }
    block(
        "Cfx Asset Escrow encrypts the Lua, YFT, YDD and YDR files of a resource, except those matched by escrow_ignore in fxmanifest.lua. The manifest, NUI files and every other file type ship as they are.",
    );
    for r in &reports {
        if reports.len() > 1 {
            block("---");
            block(&format!("# Resource {}", r.name));
        }
        let (b, w, n) = (r.count(Level::Blocker), r.count(Level::Warning), r.count(Level::Note));
        let encrypted: Vec<&EscrowFile> = r.with_status(Status::Encrypted).collect();
        let ignored: Vec<&EscrowFile> = r.with_status(Status::Ignored).collect();
        let unprotected = r.files.len() - encrypted.len() - ignored.len();
        let open = |n: usize| count(n, "file", "files");
        block("## Summary");
        block(
            &[
                format!("- Folder: `{}`", r.folder),
                format!(
                    "- {}, {}, {}",
                    count(b, "blocker", "blockers"),
                    count(w, "warning", "warnings"),
                    count(n, "note", "notes")
                ),
                format!("- Encrypted: {}", count(encrypted.len(), "file", "files")),
                format!(
                    "- Open: {} in escrow_ignore, {} of types escrow never encrypts",
                    open(ignored.len()),
                    open(unprotected)
                ),
            ]
            .join("\n"),
        );
        if r.unchecked > 0 {
            block(&unchecked_note(r.unchecked));
        } else if b + w == 0 {
            block("No blocker and no warning: the resource is ready for the upload.");
        }
        for level in Level::ALL {
            let items: Vec<String> = r.findings.iter().filter(|f| f.level == level).map(Finding::markdown).collect();
            if !items.is_empty() {
                block(&format!("## {}", level.heading()));
                block(&items.join("\n"));
            }
        }
        let patterns = r.patterns();
        if !patterns.is_empty() {
            block("## Suggested escrow_ignore additions");
            let lines: String = patterns.iter().map(|p| format!("    '{p}',\n")).collect();
            block(&format!("```lua\nescrow_ignore {{\n{lines}}}\n```"));
        }
        block("## Files kept open by escrow_ignore");
        if ignored.is_empty() {
            block("None.");
        } else {
            block(&ignored.iter().map(|f| format!("- `{}`", f.rel)).collect::<Vec<_>>().join("\n"));
        }
        block("## Files that will be encrypted");
        if encrypted.is_empty() {
            block("None.");
        } else {
            let mut lines: Vec<String> =
                encrypted.iter().take(MAX_LISTED_FILES).map(|f| format!("- `{}`", f.rel)).collect();
            if encrypted.len() > MAX_LISTED_FILES {
                lines.push(format!("- ... and {} more", grouped(encrypted.len() - MAX_LISTED_FILES)));
            }
            block(&lines.join("\n"));
        }
    }
    md
}

pub fn notification<R: Borrow<Report>>(reports: &[R]) -> (NotificationLevel, String, String) {
    let reports: Vec<&Report> = reports.iter().map(Borrow::borrow).collect();
    let b: usize = reports.iter().map(|r| r.count(Level::Blocker)).sum();
    let w: usize = reports.iter().map(|r| r.count(Level::Warning)).sum();
    let unchecked: usize = reports.iter().map(|r| r.unchecked).sum();
    let level = if b > 0 {
        NotificationLevel::Error
    } else if w > 0 {
        NotificationLevel::Warning
    } else {
        NotificationLevel::Info
    };
    let title = match reports.as_slice() {
        [one] => format!("Escrow check of {}", one.name),
        many => format!("Escrow check of {} resources", grouped(many.len())),
    };
    let mut lines = vec![format!("{}, {}", count(b, "blocker", "blockers"), count(w, "warning", "warnings"))];
    if unchecked > 0 {
        lines.push(unchecked_note(unchecked));
    }
    match reports.as_slice() {
        [r] => {
            if b + w + unchecked == 0 {
                lines.push("No blocker and no warning: the resource is ready for the upload.".into());
            }
            for level in Level::ALL {
                let findings: Vec<&Finding> = r.findings.iter().filter(|f| f.level == level).collect();
                if findings.is_empty() {
                    continue;
                }
                lines.push(level.heading().into());
                lines.extend(findings.iter().take(NOTIFIED).map(|f| f.markdown()));
                if findings.len() > NOTIFIED {
                    lines.push(format!("- ... and {} more", grouped(findings.len() - NOTIFIED)));
                }
            }
        }
        many => {
            for r in many.iter().take(NOTIFIED_RESOURCES) {
                let (b, w, n) = (r.count(Level::Blocker), r.count(Level::Warning), r.count(Level::Note));
                lines.push(format!(
                    "Resource {}: {}, {}, {}",
                    r.name,
                    count(b, "blocker", "blockers"),
                    count(w, "warning", "warnings"),
                    count(n, "note", "notes")
                ));
            }
            if many.len() > NOTIFIED_RESOURCES {
                lines.push(format!("... and {} more", grouped(many.len() - NOTIFIED_RESOURCES)));
            }
        }
    }
    (level, title, lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escrow::EscrowFeature;
    use crate::manifest::edits::apply;
    use crate::testing::Fixture;

    fn report(files: &[(&str, &str)]) -> Report {
        let ws = Workspace::from_files("/p", files);
        let res = ws.resource("/p/res").expect("res");
        check(&ws, res)
    }

    fn kinds(r: &Report) -> Vec<(Level, &str)> {
        r.findings.iter().map(|f| (f.level, f.message.as_str())).collect()
    }

    #[test]
    fn good_resource() {
        let r = report(&[
            (
                "res/fxmanifest.lua",
                "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'config.lua'\nclient_script 'client/main.lua'\nserver_script 'server/main.lua'\nescrow_ignore { 'config.lua', 'locales/*.lua' }\n",
            ),
            ("res/config.lua", "Config = {}"),
            ("res/locales/en.lua", "Locales = {}"),
            ("res/client/main.lua", "print(1)"),
            ("res/server/main.lua", "print(2)"),
        ]);
        assert_eq!(r.findings.len(), 1, "{:?}", kinds(&r));
        assert!(r.findings[0].message.starts_with("No lua54 directive."));
        let enc: Vec<&str> = r.with_status(Status::Encrypted).map(|f| f.rel.as_str()).collect();
        assert_eq!(enc, ["client/main.lua", "server/main.lua"]);
        let ign: Vec<&str> = r.with_status(Status::Ignored).map(|f| f.rel.as_str()).collect();
        assert_eq!(ign, ["config.lua", "locales/en.lua"]);
        let md = markdown(&[r]);
        assert!(md.contains("No blocker and no warning: the resource is ready for the upload."));
        assert!(md.starts_with("# Escrow pre-publish check: res\n\nCfx Asset Escrow encrypts"), "{md}");
        assert!(md.contains("## Summary\n\n- Folder: `res`\n- 0 blockers, 0 warnings, 1 note\n- Encrypted: 2 files\n- Open: 2 files in escrow_ignore, 1 file of types escrow never encrypts\n\n"), "{md}");
        assert!(md.contains("## Files kept open by escrow_ignore\n\n- `config.lua`\n- `locales/en.lua`\n\n"));
        assert!(md.ends_with("## Files that will be encrypted\n\n- `client/main.lua`\n- `server/main.lua`\n\n"));
    }

    const BAD_MANIFEST: &str = "game 'rdr3'\nclient_scripts {\n    'client/*.lua',\n    'client/ui.js',\n}\nserver_script 'server/main.lua'\nui_page 'html/index.html'\nescrow_ignore {\n    'missing/*.lua',\n}\n";

    fn bad() -> Vec<(&'static str, &'static str)> {
        vec![
            ("res/fxmanifest.lua", BAD_MANIFEST),
            ("res/config.lua", "Config = {}"),
            ("res/locales/en.lua", "Locales = {}"),
            (
                "res/client/main.lua",
                "local items = load(LoadResourceFile(GetCurrentResourceName(), 'data/items.lua'))()",
            ),
            ("res/client/ui.js", ""),
            ("res/data/items.lua", "return {}"),
            ("res/server/main.lua", "local x = = 1"),
            ("res/html/index.html", ""),
        ]
    }

    #[test]
    fn bad_resource() {
        let r = report(&bad());
        let k = kinds(&r);
        let expect = [
            (Level::Blocker, "Missing fx_version: FXServer does not start the resource".to_owned()),
            (Level::Blocker, "RedM resource without the exact rdr3_warning acknowledgement: FXServer does not start it on RedM".to_owned()),
            (Level::Warning, "'config.lua' will be encrypted: buyers will not be able to edit it.".to_owned()),
            (Level::Warning, "'locales/*.lua' will be encrypted (locales/en.lua): buyers will not be able to edit these files.".to_owned()),
            (Level::Warning, "LoadResourceFile loads 'data/items.lua', which will be encrypted: it returns the encrypted bytes in the protected build.".to_owned()),
            (Level::Warning, "Syntax error (unexpected '=') in a file that will be encrypted: buyers will not be able to fix it.".to_owned()),
            (Level::Warning, "escrow_ignore pattern 'missing/*.lua' matches no file of the resource.".to_owned()),
            (Level::Note, "'client/ui.js' is JavaScript: Asset Escrow only encrypts Lua, it ships readable.".to_owned()),
            (Level::Note, "NUI is not supported by Asset Escrow: the HTML, JavaScript and CSS files (2) ship readable.".to_owned()),
        ];
        assert_eq!(k.len(), expect.len() + 1, "{k:#?}");
        for (i, (level, message)) in expect.iter().enumerate() {
            assert_eq!((k[i].0, k[i].1), (*level, message.as_str()), "{i}");
        }
        assert!(k[expect.len()].1.starts_with("No lua54 directive."));
        let loaded = &r.findings[4];
        assert_eq!((loaded.path.as_deref(), loaded.line), (Some("client/main.lua"), 1));
        assert_eq!(r.findings[5].path.as_deref(), Some("server/main.lua"));
        assert_eq!((r.findings[6].path.as_deref(), r.findings[6].line), (Some("fxmanifest.lua"), 9));
        assert_eq!(r.patterns(), ["config.lua", "locales/*.lua", "data/items.lua"]);
        let md = markdown(&[r]);
        assert!(md.contains("- `client/main.lua:1`: LoadResourceFile loads 'data/items.lua', which will be encrypted: it returns the encrypted bytes in the protected build. Fix: add `data/items.lua` to escrow_ignore.\n"));
        assert!(md.contains("## Suggested escrow_ignore additions\n\n```lua\nescrow_ignore {\n    'config.lua',\n    'locales/*.lua',\n    'data/items.lua',\n}\n```\n\n"));
    }

    #[test]
    fn fixing_then_checking_again() {
        let r = report(&bad());
        let (s, e, t) = super::super::edits::pattern_edit(BAD_MANIFEST, &r.patterns()).unwrap();
        let fixed = apply(BAD_MANIFEST, &[(s, e, t)]);
        assert!(fixed.ends_with("escrow_ignore {\n    'missing/*.lua',\n    'config.lua',\n    'locales/*.lua',\n    'data/items.lua',\n}\n"), "{fixed}");
        let mut files: Vec<(&str, &str)> = bad();
        files[0] = ("res/fxmanifest.lua", fixed.as_str());
        let again = report(&files);
        assert!(again.patterns().is_empty(), "{:?}", kinds(&again));
    }

    #[test]
    fn other_vectors() {
        let r = report(&[
            ("res/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_scripts { 'a.lua',, }\n"),
            ("res/a.lua", ""),
        ]);
        assert_eq!(
            (r.findings[0].message.as_str(), r.findings[0].line),
            ("Syntax error in the manifest (unexpected ','): the upload fails with \"error loading manifest file\"", 3)
        );
        let r = report(&[("res/__resource.lua", "escrow_ignore {}\n")]);
        assert_eq!(
            kinds(&r),
            [(
                Level::Blocker,
                "Legacy __resource.lua manifest: Asset Escrow reads fxmanifest.lua; rename it and declare fx_version and game"
            )]
        );
        let r = report(&[
            (
                "res/fxmanifest.lua",
                "fx_version 'cerulean'\ngame 'gta5'\nlua54 'yes'\nescrow_ignore {}\nclient_script 'a.lua'",
            ),
            ("res/a.lua", ""),
        ]);
        assert!(r.findings.is_empty(), "{:?}", kinds(&r));
        let r = report(&[
            (
                "res/fxmanifest.lua",
                "fx_version 'cerulean'\ngame 'gta5'\nlua54 'yes'\nescrow_ignore {}\nclient_script 'client/main.lua'\nserver_script 'server/Bank.net.dll'",
            ),
            ("res/.fxap", ""),
            ("res/client/main.lua", "FXAP\u{1}\u{2}((("),
            ("res/server/Bank.net.dll", ""),
        ]);
        assert_eq!(
            kinds(&r),
            [
                (
                    Level::Warning,
                    "1 Lua file is already encrypted (client/main.lua): this looks like a protected build, upload the original sources."
                ),
                (
                    Level::Note,
                    "'server/Bank.net.dll' is a C# assembly: Asset Escrow only encrypts Lua, it ships as is."
                ),
            ]
        );
        assert!(r.files.iter().all(|f| f.rel != ".fxap"));
    }

    #[test]
    fn syntax_errors() {
        let e = |t: &str| syntax_error(&Source::new(t)).map(|(_, m)| m);
        assert_eq!(e("print('a')\nlocal t = { 1, 2 }"), None);
        assert_eq!(e("print('a"), Some("unfinished string".into()));
        assert_eq!(e("x = [==[ a ]]"), Some("unfinished long string".into()));
        assert_eq!(e("--[[ open"), Some("unfinished long comment".into()));
        assert_eq!(e("f(a, b"), Some("')' expected".into()));
        assert_eq!(e("end)"), Some("unexpected ')'".into()));
        assert_eq!(e("x = $"), Some("unexpected symbol '$'".into()));
        assert_eq!(e("a == b"), None);
    }

    #[test]
    fn notifications_and_plurals() {
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(count(0, "blocker", "blockers"), "0 blockers");
        assert_eq!(count(1, "blocker", "blockers"), "1 blocker");
        let r = report(&bad());
        let (level, title, body) = notification(&[r]);
        assert_eq!((level, title.as_str()), (NotificationLevel::Error, "Escrow check of res"));
        assert!(body.starts_with("2 blockers, 5 warnings\nBlockers\n- `fxmanifest.lua`: Missing fx_version: FXServer does not start the resource\n"), "{body}");
        assert!(body.contains("\nWarnings\n") && body.contains("\nNotes\n"));
        let many = [report(&bad()), report(&bad())];
        let (_, title, body) = notification(&many);
        assert_eq!(title, "Escrow check of 2 resources");
        assert!(body.contains("Resource res: 2 blockers, 5 warnings, 3 notes"), "{body}");
        assert!(markdown(&many).contains("---\n\n# Resource res\n\n## Summary"));
    }

    type Line = (Level, String, Option<String>, usize);

    fn lines(r: &Report) -> Vec<Line> {
        r.findings.iter().map(|f| (f.level, f.message.clone(), f.path.clone(), f.line)).collect()
    }

    fn res(fx: &Fixture) -> &Resource {
        fx.ws.resource("/p/res").expect("res")
    }

    #[test]
    fn indexed_files_are_not_lexed_again() {
        let expected = lines(&report(&bad()));
        facts::reset();
        let mut fx = Fixture::new(&bad());
        fx.ws.indexing = true;
        let r = check(&fx.ws, res(&fx));
        assert_eq!(r.unchecked, 5, "{:?}", kinds(&r));
        let note = "Not checked yet: 5 Lua files (CfxLua is still reading the project). Run CfxLua: Check Resource Before Escrow Upload again in a moment.";
        let md = markdown(&[&r]);
        assert!(md.contains(&format!("files of types escrow never encrypts\n\n{note}\n\n")), "{md}");
        let (_, _, body) = notification(&[&r]);
        assert!(body.starts_with(&format!("2 blockers, 3 warnings\n{note}\nBlockers\n")), "{body}");
        fx.index(&EscrowFeature);
        let r = check(&fx.ws, res(&fx));
        assert_eq!((r.unchecked, lines(&r)), (0, expected));
        assert!(!markdown(&[r]).contains("Not checked yet"));
    }

    #[test]
    fn a_file_edited_after_the_indexing_is_checked_as_it_is_now() {
        let mut fx = Fixture::new(&bad());
        fx.index(&EscrowFeature);
        fx.ws.set("/p/res/server/main.lua", Some("local x = 1"));
        fx.ws.set("/p/res/client/main.lua", Some("print(1)\nlocal y = = 2"));
        fx.ws.indexing = true;
        let stale = check(&fx.ws, res(&fx));
        assert_eq!(stale.unchecked, 2);
        assert!(stale.findings.iter().all(|f| f.path.as_deref() != Some("server/main.lua")), "{:?}", kinds(&stale));
        fx.ws.indexing = false;
        let now = check(&fx.ws, res(&fx));
        assert_eq!(now.unchecked, 0);
        let edited: Vec<Line> =
            lines(&now).into_iter().filter(|l| l.2.as_deref().is_some_and(|p| p.ends_with("main.lua"))).collect();
        assert_eq!(
            edited,
            [(
                Level::Warning,
                "Syntax error (unexpected '=') in a file that will be encrypted: buyers will not be able to fix it."
                    .to_owned(),
                Some("client/main.lua".to_owned()),
                2
            )]
        );
        fx.index(&EscrowFeature);
        fx.ws.indexing = true;
        assert_eq!(lines(&check(&fx.ws, res(&fx))), lines(&now));
    }

    #[test]
    fn reports_are_kept_until_the_workspace_changes() {
        let mut fx = Fixture::new(&bad());
        let first = cached(&fx.ws, res(&fx));
        assert!(Rc::ptr_eq(&first, &cached(&fx.ws, res(&fx))));
        let (s, e, t) = super::super::edits::pattern_edit(BAD_MANIFEST, &first.patterns()).unwrap();
        let fixed = apply(BAD_MANIFEST, &[(s, e, t)]);
        let mut files = bad();
        files[0] = ("res/fxmanifest.lua", fixed.as_str());
        let twin = Fixture::new(&files);
        assert_eq!(twin.ws.generation, fx.ws.generation);
        assert!(cached(&twin.ws, res(&twin)).patterns().is_empty(), "another workspace in the same generation");
        fx.ws.set("/p/res/locales/fr.lua", Some(""));
        let added = cached(&fx.ws, res(&fx));
        assert!(!Rc::ptr_eq(&first, &added));
        assert!(kinds(&added).iter().any(|k| k.1.contains("(locales/en.lua, locales/fr.lua)")), "{:?}", kinds(&added));
        fx.ws.set("/p/res/fxmanifest.lua", Some(&fixed));
        assert!(cached(&fx.ws, res(&fx)).patterns().is_empty());
        facts::reset();
        let mut pending = Fixture::new(&bad());
        pending.ws.indexing = true;
        let r = cached(&pending.ws, res(&pending));
        assert!(
            r.unchecked > 0 && !Rc::ptr_eq(&r, &cached(&pending.ws, res(&pending))),
            "an incomplete report is not kept"
        );
    }

    #[test]
    fn ignore_patterns_matching_without_their_case_or_as_a_folder() {
        let r = report(&[
            (
                "res/fxmanifest.lua",
                "fx_version 'cerulean'\ngame 'gta5'\nlua54 'yes'\nescrow_ignore { 'CONFIG.lua', 'Locales/*.lua', 'old.lua', 'missing.lua' }\n",
            ),
            ("res/config.lua", ""),
            ("res/locales/en.lua", ""),
            ("res/old.lua/notes.txt", ""),
        ]);
        assert_eq!(
            kinds(&r),
            [(Level::Warning, "escrow_ignore pattern 'missing.lua' matches no file of the resource.")]
        );
    }
}
