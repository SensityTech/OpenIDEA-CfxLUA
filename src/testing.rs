use crate::feature::{Ctx, Doc, Feature};
use crate::lua::Source;
use crate::natives::{Native, Natives};
use crate::project::FileKind;
use crate::settings::Settings;
use crate::workspace::Workspace;
use std::collections::HashMap;

pub const ROOT: &str = "/p";
pub const CARET: &str = "<caret>";

pub struct Fixture {
    pub ws: Workspace,
    pub settings: Settings,
    pub natives: Natives,
    carets: HashMap<String, usize>,
}

impl Fixture {
    pub fn new(files: &[(&str, &str)]) -> Self {
        let mut carets = HashMap::new();
        let stripped: Vec<(String, String)> = files
            .iter()
            .map(|(rel, text)| {
                if let Some(at) = text.find(CARET) {
                    carets.insert(format!("{ROOT}/{rel}"), at);
                }
                ((*rel).to_owned(), text.replacen(CARET, "", 1))
            })
            .collect();
        let refs: Vec<(&str, &str)> = stripped.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        Self {
            ws: Workspace::from_files(ROOT, &refs),
            settings: Settings::defaults(),
            natives: Natives::from_natives(Vec::new()),
            carets,
        }
    }

    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_natives(mut self, natives: Vec<Native>) -> Self {
        self.natives = Natives::from_natives(natives);
        self
    }

    pub fn path(rel: &str) -> String {
        format!("{ROOT}/{rel}")
    }

    pub fn index(&self, feature: &dyn Feature) {
        for f in self.ws.files() {
            let Some(text) = f.text.as_deref() else { continue };
            let lua = matches!(f.kind, FileKind::Lua | FileKind::Manifest) && !crate::project::is_escrowed(text);
            let src = if lua { Source::new(text) } else { Source::plain(text) };
            feature.index_file(&self.ws, &f.path, f.kind, &src);
        }
        feature.indexed(&self.ws);
    }

    pub fn doc(&self, rel: &str) -> Doc {
        let path = Self::path(rel);
        let text = self.ws.text(&path).unwrap_or_else(|| panic!("no fixture file {rel}"));
        Doc::new(&self.ws, &path, language_of(rel), 1, text)
    }

    pub fn caret(&self, rel: &str) -> (Doc, usize) {
        let at = *self.carets.get(&Self::path(rel)).unwrap_or_else(|| panic!("no <caret> in {rel}"));
        (self.doc(rel), at)
    }

    pub fn cx<'a>(&'a self, doc: &'a Doc) -> Ctx<'a> {
        Ctx { doc, ws: &self.ws, settings: &self.settings, natives: &self.natives }
    }
}

/// The text files under `root`, by their path from it: a project on the disk, for a measurement.
pub fn files_under(root: &std::path::Path) -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((crate::text::slash(&path.strip_prefix(root).unwrap().to_string_lossy()), text));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// The project a measurement is asked to read: `CFXLUA_PROJECT`.
pub fn real_project() -> Vec<(String, String)> {
    files_under(std::path::Path::new(&std::env::var("CFXLUA_PROJECT").expect("CFXLUA_PROJECT")))
}

pub fn language_of(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    match ext.as_str() {
        "lua" => "lua",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "vue" => "vue",
        "svelte" => "svelte",
        "html" | "htm" => "html",
        "json" => "json",
        _ => "plaintext",
    }
}

pub fn native(set: crate::natives::NativeSet, lua_name: &str, apiset: crate::project::Side) -> Native {
    Native {
        set,
        lua_name: lua_name.to_owned(),
        original_name: Some(lua_name.to_owned()),
        hash: "0x00000000".into(),
        namespace: "CFX".into(),
        apiset,
        game: None,
        rpc: false,
        params: Vec::new(),
        returns: Vec::new(),
        description: String::new(),
        lua_example: None,
        doc_url: String::new(),
        aliases: Vec::new(),
        alias_of: None,
        alt_name: None,
    }
}
