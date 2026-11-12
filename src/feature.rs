use crate::lua::Source;
use crate::project::{FileKind, Side};
use crate::settings::Settings;
use crate::workspace::Workspace;
use openidea_plugin::providers::{CompletionItem, LineMarker};
use openidea_plugin::{Location, Position, Range, Severity, TextEdit};

pub struct Doc {
    pub path: String,
    pub language: String,
    pub version: u64,
    pub src: Source,
    pub kind: FileKind,
    pub side: Side,
    pub resource: Option<String>,
}

impl Doc {
    pub fn new(ws: &Workspace, path: &str, language: &str, version: u64, text: &str) -> Self {
        let path = crate::text::slash(path);
        let kind = crate::project::file_kind(&path);
        let lua = (language == "lua" || matches!(kind, FileKind::Lua | FileKind::Manifest))
            && !crate::project::is_escrowed(text);
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

#[derive(Clone, Debug)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
    pub code: &'static str,
    pub fixes: Vec<Fix>,
    pub deprecated: bool,
}

impl Problem {
    pub fn new(code: &'static str, severity: Severity, start: usize, end: usize, message: impl Into<String>) -> Self {
        Self { start, end, severity, message: message.into(), code, fixes: Vec::new(), deprecated: false }
    }

    pub fn fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    pub fn deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Fix {
    pub title: String,
    pub preferred: bool,
    pub edits: Vec<(usize, usize, String)>,
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

#[derive(Clone, Debug)]
pub struct Intention {
    pub fix: Fix,
    pub kind: &'static str,
}

pub struct Ctx<'a> {
    pub doc: &'a Doc,
    pub ws: &'a Workspace,
    pub settings: &'a Settings,
    pub natives: &'a crate::natives::Natives,
}

pub struct CommandCx<'a> {
    pub ws: Option<&'a Workspace>,
    pub settings: &'a Settings,
    pub natives: &'a crate::natives::Natives,
    pub doc: Option<&'a Doc>,
    pub inv: &'a openidea_plugin::Invocation<'a>,
}

pub type CommandResult = Result<Option<String>, openidea_plugin::Error>;

// Providers run on the editing path, 50 ms shared by every plugin: project-wide tables are built in
// index_file / indexed (background work), never in a provider. The edited document itself is given to
// index_file by the first provider that reads those tables, not at every keystroke.
pub trait Feature {
    fn index_file(&self, _ws: &Workspace, _path: &str, _kind: FileKind, _src: &Source) {}

    fn forget_file(&self, _path: &str) {}

    fn indexed(&self, _ws: &Workspace) {}

    fn command(&self, _cx: &CommandCx, _id: &str, _arg: Option<&str>) -> Option<CommandResult> {
        None
    }

    fn completion(&self, _cx: &Ctx, _at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        Vec::new()
    }

    /// Whether `completion` at `at` reads what `index_file` keeps of the document, which is then indexed first.
    /// Asked at every keystroke, before anything is derived from the text: answer from the tokens.
    fn completes_from_index(&self, _cx: &Ctx, _at: usize) -> bool {
        true
    }

    fn definition(&self, _cx: &Ctx, _at: usize) -> Vec<Location> {
        Vec::new()
    }

    fn hover(&self, _cx: &Ctx, _at: usize) -> Option<crate::editor::hover::Hover> {
        None
    }

    fn signature_help(&self, _cx: &Ctx, _at: usize) -> Option<crate::editor::signature::Help> {
        None
    }

    fn line_markers(&self, _cx: &Ctx) -> Vec<LineMarker> {
        Vec::new()
    }

    fn problems(&self, _cx: &Ctx) -> Vec<Problem> {
        Vec::new()
    }

    fn intentions(&self, _cx: &Ctx, _start: usize, _end: usize) -> Vec<Intention> {
        Vec::new()
    }

    fn semantic_tokens(&self, _cx: &Ctx) -> Vec<crate::editor::semantic::Token> {
        Vec::new()
    }

    fn folding(&self, _cx: &Ctx) -> Vec<crate::editor::folding::Fold> {
        Vec::new()
    }

    fn symbols(&self, _cx: &Ctx) -> Vec<crate::editor::symbols::Symbol> {
        Vec::new()
    }

    fn highlights(&self, _cx: &Ctx, _at: usize) -> Vec<crate::editor::highlights::Highlight> {
        Vec::new()
    }
}
