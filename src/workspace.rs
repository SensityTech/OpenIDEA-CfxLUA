//! The project files the plugin knows about. The sandbox has no file
//! system: files are listed with `find_files` and read with `read_file` once
//! (when the plugin starts for a project), then kept up to date from the
//! documents the IDE sends (unsaved text wins) and the `file-saved` events.
//!
//! Features never read the host directly: they go through [`Workspace`], so
//! they are unit-tested with an in-memory one ([`Workspace::from_files`]).

use crate::project::{self, FileKind, Resource, Side};
use crate::text::{file_name, parent, slash};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Globs listed when a project is scanned, with their caps.
pub const SCAN: &[(&str, u32)] = &[
    ("**/*.lua", 20_000),
    ("**/*.{js,jsx,mjs,cjs,ts,tsx,mts,cts,vue,svelte,html,htm}", 8_000),
    ("**/{vite,webpack}.config.*", 200),
    ("**/*.{dll,net.dll}", 500),
];

/// Files larger than this are listed but not kept (minified bundles, data dumps).
pub const MAX_FILE: usize = 3_000_000;

#[derive(Clone)]
pub struct WsFile {
    pub path: String,
    pub kind: FileKind,
    /// `None` for files listed but not read (binaries, too large).
    pub text: Option<Arc<str>>,
}

#[derive(Default)]
pub struct Workspace {
    /// Project root, forward slashes, no trailing slash.
    pub root: String,
    files: BTreeMap<String, WsFile>,
    /// Resources by directory, from their manifests.
    resources: BTreeMap<String, Resource>,
    /// Bumped on every change: features rebuild their project-wide tables when it moves.
    pub generation: u64,
    /// Whether the whole project was read (else only what the IDE sent).
    pub scanned: bool,
    /// The IDE gave the root with backslashes (Windows): paths go back that way.
    pub native_backslashes: bool,
}

impl Workspace {
    pub fn new(root: &str) -> Self {
        Self {
            root: slash(root).trim_end_matches('/').to_owned(),
            native_backslashes: root.contains('\\'),
            ..Self::default()
        }
    }

    /// An in-memory workspace (tests): `(project-relative path, text)`.
    pub fn from_files(root: &str, files: &[(&str, &str)]) -> Self {
        let mut ws = Self::new(root);
        for (rel, text) in files {
            let path = format!("{}/{}", ws.root, rel.trim_start_matches('/'));
            ws.set(&path, Some(text));
        }
        ws.scanned = true;
        ws
    }

    /// Whether `path` is inside the project.
    pub fn contains(&self, path: &str) -> bool {
        let p = slash(path);
        !self.root.is_empty() && (p == self.root || p.starts_with(&format!("{}/", self.root)))
    }

    /// Project-relative form of an absolute path.
    pub fn relative<'a>(&self, path: &'a str) -> &'a str {
        path.strip_prefix(self.root.as_str()).map_or(path, |r| r.trim_start_matches('/'))
    }

    pub fn absolute(&self, rel: &str) -> String {
        format!("{}/{}", self.root, rel.trim_start_matches('/'))
    }

    /// Adds, replaces (`Some`) or forgets the text of a file (`None` keeps it listed, unread).
    pub fn set(&mut self, path: &str, text: Option<&str>) {
        let path = slash(path);
        let kind = project::file_kind(&path);
        let text = text.filter(|t| t.len() <= MAX_FILE).map(Arc::from);
        if let Some(old) = self.files.get(&path)
            && old.text.as_deref() == text.as_deref()
        {
            return;
        }
        let manifest = kind == FileKind::Manifest;
        self.files.insert(path.clone(), WsFile { path: path.clone(), kind, text: text.clone() });
        if manifest {
            let dir = parent(&path).to_owned();
            match text {
                Some(t) => {
                    self.resources.insert(dir.clone(), Resource::parse(&dir, file_name(&path), &t));
                }
                None => {
                    self.resources.remove(&dir);
                }
            }
        }
        self.generation += 1;
    }

    pub fn remove(&mut self, path: &str) {
        let path = slash(path);
        if self.files.remove(&path).is_some() {
            if project::file_kind(&path) == FileKind::Manifest {
                self.resources.remove(parent(&path));
            }
            self.generation += 1;
        }
    }

    pub fn file(&self, path: &str) -> Option<&WsFile> {
        self.files.get(&slash(path))
    }

    pub fn text(&self, path: &str) -> Option<&str> {
        self.files.get(&slash(path)).and_then(|f| f.text.as_deref())
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(&slash(path))
    }

    /// Every known file.
    pub fn files(&self) -> impl Iterator<Item = &WsFile> {
        self.files.values()
    }

    /// Known files of one kind, with their text.
    pub fn texts(&self, kind: FileKind) -> impl Iterator<Item = (&str, &str)> {
        self.files.values().filter(move |f| f.kind == kind).filter_map(|f| Some((f.path.as_str(), f.text.as_deref()?)))
    }

    /// Files under directory `dir` (any depth).
    pub fn files_under<'a>(&'a self, dir: &str) -> impl Iterator<Item = &'a WsFile> + 'a {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        self.files.range(prefix.clone()..).take_while(move |(p, _)| p.starts_with(&prefix)).map(|(_, f)| f)
    }

    pub fn resources(&self) -> impl Iterator<Item = &Resource> {
        self.resources.values()
    }

    pub fn resource(&self, dir: &str) -> Option<&Resource> {
        self.resources.get(dir)
    }

    /// The resource a file belongs to: the nearest directory above it with a manifest.
    pub fn resource_of(&self, path: &str) -> Option<&Resource> {
        let path = slash(path);
        let mut dir = parent(&path);
        loop {
            if let Some(r) = self.resources.get(dir) {
                return Some(r);
            }
            if dir.is_empty() || dir.len() <= self.root.len() && !self.root.is_empty() {
                return None;
            }
            dir = parent(dir);
        }
    }

    /// A resource by folder name (`dependency 'ox_lib'`, `@ox_lib/init.lua`).
    pub fn resource_named(&self, name: &str) -> Option<&Resource> {
        self.resources.values().find(|r| r.name.eq_ignore_ascii_case(name))
    }

    /// Games declared by the manifests of the project (`game` / `games`), in order.
    pub fn detected_games(&self) -> Vec<project::Game> {
        let mut out = Vec::new();
        for g in self.resources.values().filter_map(|r| r.games.as_ref()).flatten() {
            if !out.contains(g) {
                out.push(*g);
            }
        }
        out.sort();
        out
    }

    /// Client / server / shared side of a Lua file.
    pub fn side_of(&self, path: &str) -> Side {
        let path = slash(path);
        match self.resource_of(&path) {
            Some(r) => r.side_of(&path),
            None => project::side_by_convention(&path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_and_files() {
        let ws = Workspace::from_files(
            "C:/srv",
            &[
                (
                    "resources/[core]/shop/fxmanifest.lua",
                    "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n",
                ),
                ("resources/[core]/shop/client/main.lua", "print(1)"),
                ("loose.lua", "print(2)"),
            ],
        );
        let r = ws.resource_of("C:/srv/resources/[core]/shop/client/main.lua").unwrap();
        assert_eq!(r.name, "shop");
        assert!(ws.resource_of("C:/srv/loose.lua").is_none());
        assert_eq!(ws.files_under("C:/srv/resources/[core]/shop").count(), 2);
        assert_eq!(ws.relative("C:/srv/loose.lua"), "loose.lua");
    }
}
