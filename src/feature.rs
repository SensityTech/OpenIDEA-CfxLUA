//! The contract between the plugin's entry points (completion, go to
//! definition, gutter, problems, quick fixes, view) and the features. Every
//! feature works on a [`Doc`] (the document the IDE asked about, parsed once
//! per version) and the [`Workspace`] (every project file the plugin indexed).
//!
//! Offsets are UTF-8 byte offsets into `doc.text`; [`Doc::range`] and
//! [`Doc::position`] convert for the IDE.

use crate::lua::Source;
use crate::project::{FileKind, Side};
use crate::settings::Settings;
use crate::workspace::Workspace;
use openidea_plugin::providers::{CompletionItem, LineMarker};
use openidea_plugin::{Location, Position, Range, Severity, TextEdit};

/// The document a request is about.
pub struct Doc {
    /// Absolute path, forward slashes.
    pub path: String,
    /// The IDE's language id (`lua`, `typescript`, …).
    pub language: String,
    pub version: u64,
    /// Lua documents only (others get an empty source).
    pub src: Source,
    pub kind: FileKind,
    /// Client / server / shared, for Lua scripts of a resource.
    pub side: Side,
    /// Resource directory (forward slashes), when the file belongs to one.
    pub resource: Option<String>,
}

impl Doc {
    /// Parses `text` as the document at `path` (Lua and manifests are
    /// tokenized; other languages keep their text only).
    pub fn new(ws: &Workspace, path: &str, language: &str, version: u64, text: &str) -> Self {
        let path = crate::text::slash(path);
        let kind = crate::project::file_kind(&path);
        let lua = language == "lua" || matches!(kind, FileKind::Lua | FileKind::Manifest);
        let src = if lua { Source::new(text) } else { Source::plain(text) };
        let side = if lua { ws.side_of(&path) } else { Side::Unknown };
        let resource = ws.resource_of(&path).map(|r| r.dir.clone());
        Self { path, language: language.to_owned(), version, src, kind, side, resource }
    }

    pub fn text(&self) -> &str {
        &self.src.text
    }

    pub fn position(&self, at: usize) -> Position {
        self.src.lines.position(&self.src.text, at)
    }

    pub fn range(&self, start: usize, end: usize) -> Range {
        self.src.lines.range(&self.src.text, start, end)
    }

    pub fn offset(&self, pos: Position) -> usize {
        self.src.lines.offset(&self.src.text, pos)
    }

    pub fn edit(&self, start: usize, end: usize, text: impl Into<String>) -> TextEdit {
        TextEdit { range: self.range(start, end), text: text.into() }
    }

    pub fn location(&self, start: usize, end: usize) -> Location {
        Location { path: self.path.clone(), range: self.range(start, end) }
    }

    pub fn is_lua(&self) -> bool {
        self.language == "lua"
    }

    pub fn is_manifest(&self) -> bool {
        self.kind == FileKind::Manifest
    }
}

/// A problem a feature found, with its quick fixes.
#[derive(Clone, Debug)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
    /// Inspection id (`CfxDecrementComment`).
    pub code: &'static str,
    pub fixes: Vec<Fix>,
}

impl Problem {
    pub fn new(code: &'static str, severity: Severity, start: usize, end: usize, message: impl Into<String>) -> Self {
        Self { start, end, severity, message: message.into(), code, fixes: Vec::new() }
    }

    pub fn fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }
}

/// A quick fix or intention: edits of the document (and possibly other files).
#[derive(Clone, Debug)]
pub struct Fix {
    pub title: String,
    pub preferred: bool,
    /// Edits of the document the problem is in: (start, end, new text) in bytes.
    pub edits: Vec<(usize, usize, String)>,
    /// Edits of other files (absolute path, IDE ranges).
    pub other: Vec<(String, Vec<TextEdit>)>,
}

impl Fix {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), preferred: false, edits: Vec::new(), other: Vec::new() }
    }

    pub fn replace(mut self, start: usize, end: usize, text: impl Into<String>) -> Self {
        self.edits.push((start, end, text.into()));
        self
    }

    pub fn insert(self, at: usize, text: impl Into<String>) -> Self {
        self.replace(at, at, text)
    }

    pub fn preferred(mut self) -> Self {
        self.preferred = true;
        self
    }
}

/// An intention offered at a range (not tied to a problem).
#[derive(Clone, Debug)]
pub struct Intention {
    pub fix: Fix,
    /// `refactor`, `quickfix`…
    pub kind: &'static str,
}

/// Everything a feature may read.
pub struct Ctx<'a> {
    pub doc: &'a Doc,
    pub ws: &'a Workspace,
    pub settings: &'a Settings,
    pub natives: &'a crate::natives::Natives,
}

/// What a command or the docs view may use. There may be no project (a
/// window without one) and no document.
pub struct CommandCx<'a> {
    pub ws: Option<&'a Workspace>,
    pub settings: &'a Settings,
    pub natives: &'a crate::natives::Natives,
    /// The focused editor's document (parsed), when there is one.
    pub doc: Option<&'a Doc>,
    pub inv: &'a openidea_plugin::Invocation<'a>,
}

pub type CommandResult = Result<Option<String>, openidea_plugin::Error>;

/// One feature of the plugin. Every method has a default that does nothing,
/// so a feature implements only what it provides.
///
/// Providers run on the editing path (about 50 ms for every plugin at once):
/// project-wide tables are built in [`Feature::index_file`] /
/// [`Feature::indexed`] (called from background work), never in a provider.
pub trait Feature {
    /// A project file was (re)read: keep what the feature needs from it. `src`
    /// is the parsed Lua for Lua files, [`Source::plain`] text for the others.
    /// Also called for the document being edited, on each new version.
    fn index_file(&self, _ws: &Workspace, _path: &str, _kind: FileKind, _src: &Source) {}

    /// A file is gone (deleted, or no longer part of the project).
    fn forget_file(&self, _path: &str) {}

    /// After a batch of [`Feature::index_file`] calls (a scan, a save):
    /// rebuild aggregate tables.
    fn indexed(&self, _ws: &Workspace) {}

    /// Runs command `id` if this feature owns it (`None` otherwise).
    fn command(&self, _cx: &CommandCx, _id: &str, _arg: Option<&str>) -> Option<CommandResult> {
        None
    }

    /// Completion at byte `at` (the caret). `trigger`: the typed character, if any.
    fn completion(&self, _cx: &Ctx, _at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        Vec::new()
    }

    /// Go to definition at byte `at`.
    fn definition(&self, _cx: &Ctx, _at: usize) -> Vec<Location> {
        Vec::new()
    }

    /// Gutter markers of the document.
    fn line_markers(&self, _cx: &Ctx) -> Vec<LineMarker> {
        Vec::new()
    }

    /// Problems of the document (published as diagnostics; their fixes are the quick fixes).
    fn problems(&self, _cx: &Ctx) -> Vec<Problem> {
        Vec::new()
    }

    /// Intentions available for the selection `[start, end]`.
    fn intentions(&self, _cx: &Ctx, _start: usize, _end: usize) -> Vec<Intention> {
        Vec::new()
    }
}
