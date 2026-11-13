use crate::luals::{self, Contents, LUARC_FILES};
use crate::project::{self, FileKind, Resource, Side};
use crate::text::{parent, slash};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static LISTINGS: AtomicU64 = AtomicU64::new(1);

pub const SCAN: &[(&str, u32)] = &[
    ("**/*.lua", 20_000),
    ("**/*.{js,jsx,mjs,cjs,ts,tsx,mts,cts,vue,svelte,html,htm}", 8_000),
    ("**/{vite,webpack}.config.*", 200),
    // Every other file, listed without reading it: manifest and escrow checks need to know they exist.
    ("**/*", 60_000),
];

pub const MAX_FILE: usize = 3_000_000;

/// What a text is known by once it is no longer kept: its length and a hash of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp {
    len: usize,
    hash: u64,
}

impl Stamp {
    pub fn of(text: &str) -> Stamp {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        Stamp { len: text.len(), hash: hasher.finish() }
    }

    /// The length of the text, in bytes.
    pub fn size(self) -> usize {
        self.len
    }
}

#[derive(Clone)]
pub struct WsFile {
    pub path: String,
    pub kind: FileKind,
    /// The text, while it is kept: always for a manifest, a configuration or what the editor sent; for a
    /// script read from the disk, until it is indexed.
    pub text: Option<Arc<str>>,
    /// The stamp of the text the file was last given, kept or not.
    pub stamp: Option<Stamp>,
    /// The text came from the editor: the disk may hold another one.
    pub edited: bool,
}

/// Tells the sizes of files of the project `root`, in the order they are given. Past `until` it stops: it
/// answers for the first files only.
pub type Sizer = fn(root: &str, paths: &[&str], until: Instant) -> Vec<Option<u64>>;

/// What files weigh together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weight {
    pub bytes: u64,
    /// The files that are only listed and whose size was not told in time.
    pub unweighed: usize,
}

#[derive(Default)]
pub struct Workspace {
    pub root: String,
    files: BTreeMap<String, WsFile>,
    resources: BTreeMap<String, Resource>,
    pub generation: u64,
    /// Changes when a path is added or removed, unique across workspaces: caches of the file list key on it.
    pub listing: u64,
    pub scanned: bool,
    /// The first scan is still reading and indexing files.
    pub indexing: bool,
    pub native_backslashes: bool,
    definitions: BTreeMap<String, Contents>,
    /// What lua-language-server loads of the definitions this plugin wrote, and completes itself.
    pub luals: Contents,
    /// Reads a file of the project where it is stored, for a text that is no longer kept.
    loader: Option<fn(&str) -> Option<String>>,
    /// Tells the sizes of files of the project `root` where they are stored, for files that are only listed.
    sizer: Option<Sizer>,
}

impl Workspace {
    pub fn new(root: &str) -> Self {
        Self {
            root: slash(root).trim_end_matches('/').to_owned(),
            native_backslashes: root.contains('\\'),
            ..Self::default()
        }
    }

    /// The same workspace, reading with `loader` the texts it no longer keeps.
    pub fn reading(mut self, loader: fn(&str) -> Option<String>) -> Self {
        self.loader = Some(loader);
        self
    }

    /// The same workspace, asking `sizer` the size of the files it only lists.
    pub fn sizing(mut self, sizer: Sizer) -> Self {
        self.sizer = Some(sizer);
        self
    }

    pub fn from_files(root: &str, files: &[(&str, &str)]) -> Self {
        let mut ws = Self::new(root);
        for (rel, text) in files {
            let path = format!("{}/{}", ws.root, rel.trim_start_matches('/'));
            ws.set(&path, Some(text));
        }
        ws.scanned = true;
        ws
    }

    pub fn contains(&self, path: &str) -> bool {
        let p = slash(path);
        !self.root.is_empty() && (p == self.root || p.starts_with(&format!("{}/", self.root)))
    }

    pub fn relative<'a>(&self, path: &'a str) -> &'a str {
        path.strip_prefix(self.root.as_str()).map_or(path, |r| r.trim_start_matches('/'))
    }

    pub fn absolute(&self, rel: &str) -> String {
        format!("{}/{}", self.root, rel.trim_start_matches('/'))
    }

    /// Gives the file the text read where it is stored, or none for a file that is only listed.
    pub fn set(&mut self, path: &str, text: Option<&str>) {
        self.give(path, text, false);
    }

    /// Gives the file the text the editor shows, which the disk may not hold yet.
    pub fn edit(&mut self, path: &str, text: &str) {
        self.give(path, Some(text), true);
    }

    fn give(&mut self, path: &str, text: Option<&str>, edited: bool) {
        let path = slash(path);
        let kind = project::file_kind(&path);
        let definitions = match text.and_then(Contents::of) {
            Some(contents) => self.definitions.insert(path.clone(), contents.clone()) != Some(contents),
            None => self.definitions.remove(&path).is_some(),
        };
        let text = text.filter(|t| t.len() <= MAX_FILE);
        let stamp = text.map(Stamp::of);
        if let Some(old) = self.files.get(&path)
            && old.stamp == stamp
        {
            if definitions {
                self.refresh_luals();
            }
            return;
        }
        let manifest = kind == FileKind::Manifest;
        let file = WsFile { path: path.clone(), kind, text: text.map(Arc::from), stamp, edited };
        if self.files.insert(path.clone(), file).is_none() {
            self.listing = LISTINGS.fetch_add(1, Ordering::Relaxed);
        }
        if manifest {
            self.reload_resource(parent(&path).to_owned());
        }
        if definitions || self.is_luarc(&path) {
            self.refresh_luals();
        }
        self.generation += 1;
    }

    fn is_luarc(&self, path: &str) -> bool {
        path.strip_prefix(self.root.as_str())
            .and_then(|r| r.strip_prefix('/'))
            .is_some_and(|name| LUARC_FILES.contains(&name))
    }

    fn refresh_luals(&mut self) {
        self.luals = luals::loaded(self, self.definitions.iter());
    }

    fn reload_resource(&mut self, dir: String) {
        let manifest = project::MANIFESTS.iter().find_map(|name| {
            let file = self.files.get(&format!("{dir}/{name}"))?;
            Some((*name, file.text.clone()?))
        });
        match manifest {
            Some((name, text)) => {
                self.resources.insert(dir.clone(), Resource::parse(&dir, name, &text));
            }
            None => {
                self.resources.remove(&dir);
            }
        }
    }

    pub fn remove(&mut self, path: &str) {
        let path = slash(path);
        if self.files.remove(&path).is_some() {
            self.listing = LISTINGS.fetch_add(1, Ordering::Relaxed);
            if project::file_kind(&path) == FileKind::Manifest {
                self.reload_resource(parent(&path).to_owned());
            }
            if self.definitions.remove(&path).is_some() || self.is_luarc(&path) {
                self.refresh_luals();
            }
            self.generation += 1;
        }
    }

    pub fn file(&self, path: &str) -> Option<&WsFile> {
        self.files.get(&slash(path))
    }

    /// The text of a file, while it is kept.
    pub fn text(&self, path: &str) -> Option<&str> {
        self.files.get(&slash(path)).and_then(|f| f.text.as_deref())
    }

    /// Whether `text` is the text the file was last given.
    pub fn holds(&self, path: &str, text: &str) -> bool {
        match self.files.get(&slash(path)) {
            Some(WsFile { text: Some(kept), .. }) => **kept == *text,
            Some(file) => file.stamp == Some(Stamp::of(text)),
            None => false,
        }
    }

    pub fn stamp(&self, path: &str) -> Option<Stamp> {
        self.files.get(&slash(path)).and_then(|f| f.stamp)
    }

    /// What files weigh together: the length of the text that was read of a file, and what its storage tells
    /// of a file that is only listed. The storage is asked once for all of them, until `until`: each file
    /// costs it a call.
    pub fn weight<'a>(&self, paths: impl Iterator<Item = &'a str>, until: Instant) -> Weight {
        let (mut bytes, mut listed) = (0, Vec::new());
        for path in paths {
            match self.files.get(path).map(|file| file.stamp) {
                Some(Some(stamp)) => bytes += stamp.size() as u64,
                Some(None) => listed.push(path),
                None => {}
            }
        }
        let mut stored = match self.sizer {
            Some(sizer) if !listed.is_empty() => sizer(&self.root, &listed, until),
            _ => Vec::new(),
        };
        stored.truncate(listed.len());
        let unweighed = listed.len() - stored.len();
        Weight { bytes: bytes + stored.into_iter().flatten().sum::<u64>(), unweighed }
    }

    /// Lets go of the text of a script read from the disk, once what it declares is indexed: its stamp
    /// stays. What the editor sent is kept, the disk may not hold it.
    pub fn release(&mut self, path: &str) {
        if let Some(file) = self.files.get_mut(&slash(path))
            && matches!(file.kind, FileKind::Lua | FileKind::Web)
            && !file.edited
        {
            file.text = None;
        }
    }

    /// The text the file was last given: the one kept, or else the one read again where the file is
    /// stored, when it is still that text.
    pub fn read(&self, path: &str) -> Option<Arc<str>> {
        let file = self.files.get(&slash(path))?;
        if let Some(kept) = &file.text {
            return Some(kept.clone());
        }
        let read = (self.loader?)(&file.path)?;
        (file.stamp == Some(Stamp::of(&read))).then(|| Arc::from(read))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(&slash(path))
    }

    pub fn files(&self) -> impl Iterator<Item = &WsFile> {
        self.files.values()
    }

    pub fn texts(&self, kind: FileKind) -> impl Iterator<Item = (&str, &str)> {
        self.files.values().filter(move |f| f.kind == kind).filter_map(|f| Some((f.path.as_str(), f.text.as_deref()?)))
    }

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

    pub fn resource_named(&self, name: &str) -> Option<&Resource> {
        self.resources.values().find(|r| r.name.eq_ignore_ascii_case(name))
    }

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

    #[test]
    fn fxmanifest_wins_over_resource_lua() {
        let mut ws = Workspace::from_files(
            "/p",
            &[("res/fxmanifest.lua", "client_script 'a.lua'"), ("res/__resource.lua", "server_script 'a.lua'")],
        );
        assert_eq!(ws.resource("/p/res").unwrap().manifest, "fxmanifest.lua");
        assert_eq!(ws.side_of("/p/res/a.lua"), crate::project::Side::Client);
        ws.remove("/p/res/fxmanifest.lua");
        assert_eq!(ws.resource("/p/res").unwrap().manifest, "__resource.lua");
        assert_eq!(ws.side_of("/p/res/a.lua"), crate::project::Side::Server);
    }

    #[test]
    fn a_script_read_from_the_disk_lets_go_of_its_text_once_indexed() {
        fn disk(path: &str) -> Option<String> {
            match path {
                "/p/res/a.lua" => Some("local a = 1".to_owned()),
                "/p/res/changed.lua" => Some("local changed = 2".to_owned()),
                _ => None,
            }
        }
        let files = [
            ("res/fxmanifest.lua", "client_script 'a.lua'"),
            ("res/a.lua", "local a = 1"),
            ("res/changed.lua", "local changed = 1"),
            ("res/ui.js", "fetch('x')"),
            ("res/vite.config.js", "export default {}"),
            ("res/zones.json", "{}"),
        ];
        let mut ws = Workspace::from_files("/p", &files).reading(disk);
        let generation = ws.generation;
        for (rel, _) in files {
            ws.release(&format!("/p/{rel}"));
        }
        assert_eq!(ws.text("/p/res/a.lua"), None);
        assert_eq!(ws.text("/p/res/ui.js"), None);
        assert!(ws.text("/p/res/fxmanifest.lua").is_some(), "a manifest is read at every question");
        assert!(ws.text("/p/res/vite.config.js").is_some() && ws.text("/p/res/zones.json").is_some());
        assert_eq!(ws.generation, generation, "nothing changed");
        assert!(ws.exists("/p/res/a.lua") && ws.resource_of("/p/res/a.lua").is_some());

        assert!(ws.holds("/p/res/a.lua", "local a = 1") && !ws.holds("/p/res/a.lua", "local a = 2"));
        assert!(!ws.holds("/p/res/none.lua", ""));
        assert_eq!(ws.stamp("/p/res/a.lua"), Some(Stamp::of("local a = 1")));
        assert_eq!(ws.read("/p/res/a.lua").as_deref(), Some("local a = 1"), "read again where it is stored");
        assert_eq!(ws.read("/p/res/changed.lua"), None, "the disk holds another text: it is not the one indexed");
        assert_eq!(ws.read("/p/res/ui.js"), None, "it cannot be read any more");
        assert_eq!(ws.read("/p/res/zones.json").as_deref(), Some("{}"), "a kept text is not read again");

        ws.set("/p/res/a.lua", Some("local a = 1"));
        assert_eq!((ws.text("/p/res/a.lua"), ws.generation), (None, generation), "the same text is not kept again");
        ws.edit("/p/res/a.lua", "local a = 1");
        assert_eq!(ws.text("/p/res/a.lua"), None, "what the editor shows is what the disk holds");
        ws.edit("/p/res/a.lua", "local a = 1\nlocal b");
        ws.release("/p/res/a.lua");
        assert_eq!(ws.text("/p/res/a.lua"), Some("local a = 1\nlocal b"), "the disk does not hold it: it is kept");
        assert_eq!(ws.read("/p/res/a.lua").as_deref(), Some("local a = 1\nlocal b"));
        ws.set("/p/res/a.lua", Some("local a = 3"));
        ws.release("/p/res/a.lua");
        assert_eq!(ws.text("/p/res/a.lua"), None, "saved and read from the disk again");
    }
}
