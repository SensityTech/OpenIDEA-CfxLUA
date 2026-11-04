use super::catalog::PathKind;
use crate::project::CASE_SENSITIVE;
use crate::project::glob::Glob;
use crate::workspace::Workspace;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const SKIPPED_DIRS: [&str; 4] = ["node_modules", ".git", ".svn", ".idea"];
const MAX_MATCHES: usize = 500;
const CACHED_TREES: usize = 16;

thread_local! {
    static TREES: RefCell<Vec<(String, u64, Rc<Tree>)>> = const { RefCell::new(Vec::new()) };
}

fn listed(ext: &str) -> bool {
    crate::workspace::SCAN.iter().filter_map(|(glob, _)| glob.strip_prefix("**/*.")).any(|exts| {
        let exts = exts.strip_prefix('{').and_then(|e| e.strip_suffix('}')).unwrap_or(exts);
        exts.split(',').any(|e| e.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
    })
}

pub fn normalize(p: &str) -> String {
    let mut s = p.trim().replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_owned();
    }
    s.trim_start_matches('/').to_owned()
}

pub fn is_glob(p: &str) -> bool {
    p.contains(['*', '?'])
}

pub fn is_url(v: &str) -> bool {
    v.contains("://")
}

pub fn split_resource_path(v: &str) -> Option<(&str, &str)> {
    let rest = v.trim().strip_prefix('@')?;
    Some(rest.split_once('/').unwrap_or((rest, "")))
}

pub fn checkable(pattern: &str, kind: PathKind) -> bool {
    let last = pattern.rsplit(['/', '\\']).next().unwrap_or(pattern);
    match last.rsplit_once('.') {
        Some((_, ext)) if !is_glob(ext) => listed(ext),
        // No extension (or a wildcard one): scripts are Lua, JavaScript or assemblies.
        _ => kind == PathKind::Script,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub rel: String,
    pub dir: bool,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    key: String,
    rel: String,
    // Bit `s`: visible when the skipped directories of set `s` (see `skipped_by`) are skipped.
    visible: u16,
}

pub struct Tree {
    pub base: String,
    files: Vec<Entry>,
    dirs: Vec<Entry>,
}

fn key(rel: &str) -> String {
    if CASE_SENSITIVE { rel.to_owned() } else { rel.to_lowercase() }
}

fn skipped_by(pattern: &str) -> usize {
    let lower = pattern.to_lowercase();
    SKIPPED_DIRS.iter().enumerate().filter(|(_, d)| !lower.contains(*d)).fold(0, |s, (i, _)| s | 1 << i)
}

fn visibility(rel: &str) -> u16 {
    let dirs = rel.rsplit_once('/').map_or("", |(d, _)| d);
    let inside = SKIPPED_DIRS
        .iter()
        .enumerate()
        .filter(|(_, d)| dirs.split('/').any(|s| s.eq_ignore_ascii_case(d)))
        .fold(0, |m, (i, _)| m | 1 << i);
    (0..16).filter(|s| s & inside == 0).fold(0, |l, s| l | 1 << s)
}

fn starting_with<'a>(entries: &'a [Entry], prefix: &str) -> &'a [Entry] {
    let start = entries.partition_point(|e| e.key.as_str() < prefix);
    let len = entries[start..].partition_point(|e| e.key.starts_with(prefix));
    &entries[start..start + len]
}

// Every match starts with the glob's literal prefix; its ASCII part has the same bytes in the lowercased keys.
fn globbed<'a>(entries: &'a [Entry], glob: &'a Glob, skipped: usize) -> impl Iterator<Item = &'a Entry> + 'a {
    let literal = glob.literal_prefix();
    let ascii = literal.find(|c: char| !c.is_ascii()).map_or(literal.as_str(), |i| &literal[..i]);
    starting_with(entries, ascii).iter().filter(move |e| e.visible & 1 << skipped != 0 && glob.matches(&e.rel))
}

impl Tree {
    pub fn new(ws: &Workspace, base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        let prefix = format!("{base}/");
        let mut files: Vec<Entry> = ws
            .files_under(&base)
            .filter_map(|f| f.path.strip_prefix(&prefix))
            .map(|rel| Entry { key: key(rel), rel: rel.to_owned(), visible: visibility(rel) })
            .collect();
        files.sort();
        let mut dirs: HashMap<&str, u16> = HashMap::new();
        for f in &files {
            for (i, _) in f.rel.match_indices('/') {
                *dirs.entry(&f.rel[..i]).or_default() |= f.visible;
            }
        }
        let mut dirs: Vec<Entry> =
            dirs.into_iter().map(|(rel, visible)| Entry { key: key(rel), rel: rel.to_owned(), visible }).collect();
        dirs.sort();
        Self { base, files, dirs }
    }

    pub fn cached(ws: &Workspace, base: &str) -> Rc<Self> {
        let base = base.trim_end_matches('/');
        TREES.with_borrow_mut(|trees| {
            if let Some((_, _, tree)) = trees.iter().find(|(_, listing, t)| *listing == ws.listing && t.base == base) {
                return Rc::clone(tree);
            }
            trees.retain(|(root, listing, _)| *root != ws.root || *listing == ws.listing);
            if trees.len() >= CACHED_TREES {
                trees.remove(0);
            }
            let tree = Rc::new(Self::new(ws, base));
            trees.push((ws.root.clone(), ws.listing, Rc::clone(&tree)));
            tree
        })
    }

    fn with_prefix<'a>(&'a self, prefix: &str) -> &'a [Entry] {
        starting_with(&self.files, prefix)
    }

    pub fn file(&self, rel: &str) -> Option<&str> {
        let k = key(rel);
        self.files.binary_search_by(|e| e.key.as_str().cmp(&k)).ok().map(|i| self.files[i].rel.as_str())
    }

    pub fn has_dir(&self, rel: &str) -> bool {
        let rel = rel.trim_end_matches('/');
        !rel.is_empty() && !self.with_prefix(&key(&format!("{rel}/"))).is_empty()
    }

    pub fn children(&self, dir: &str) -> (Vec<String>, Vec<String>) {
        let mut dirs: Vec<String> = Vec::new();
        let mut files = Vec::new();
        for Entry { rel, .. } in self.with_prefix(&key(dir)) {
            let Some(rest) = rel.get(dir.len()..) else { continue };
            match rest.split_once('/') {
                Some((d, _)) => {
                    if dirs.last().is_none_or(|l| !l.eq_ignore_ascii_case(d)) {
                        dirs.push(d.to_owned());
                    }
                }
                None => files.push(rest.to_owned()),
            }
        }
        (dirs, files)
    }

    pub fn matches(&self, pattern: &str, kind: PathKind) -> Vec<Match> {
        let p = normalize(pattern);
        if p.is_empty() || p.starts_with('@') {
            return Vec::new();
        }
        if !is_glob(&p) {
            if let Some(rel) = self.file(&p) {
                return vec![Match { rel: rel.to_owned(), dir: false }];
            }
            if kind.directories() && self.has_dir(&p) {
                return vec![Match { rel: p.trim_end_matches('/').to_owned(), dir: true }];
            }
            if kind.prefix() {
                let (dir, last) = match p.rfind('/') {
                    Some(i) => (&p[..=i], &p[i + 1..]),
                    None => ("", p.as_str()),
                };
                let (dirs, files) = self.children(dir);
                let last = key(last);
                let named = |n: &String| key(n).starts_with(&last);
                return files
                    .iter()
                    .filter(|n| named(n))
                    .map(|n| Match { rel: format!("{dir}{n}"), dir: false })
                    .chain(dirs.iter().filter(|n| named(n)).map(|n| Match { rel: format!("{dir}{n}"), dir: true }))
                    .take(MAX_MATCHES)
                    .collect();
            }
            return Vec::new();
        }
        let Some(glob) = Glob::new(&p, CASE_SENSITIVE) else { return Vec::new() };
        let skipped = skipped_by(&p);
        let mut out: Vec<Match> = globbed(&self.files, &glob, skipped)
            .take(MAX_MATCHES)
            .map(|e| Match { rel: e.rel.clone(), dir: false })
            .collect();
        if kind.directories() && out.len() < MAX_MATCHES {
            let mut dirs: Vec<&str> = globbed(&self.dirs, &glob, skipped).map(|e| e.rel.as_str()).collect();
            dirs.sort_unstable();
            let room = MAX_MATCHES - out.len();
            out.extend(dirs.into_iter().take(room).map(|rel| Match { rel: rel.to_owned(), dir: true }));
        }
        out
    }

    pub fn matches_any(&self, pattern: &str, kind: PathKind) -> bool {
        let p = normalize(pattern);
        let glob = if is_glob(&p) { Glob::new(&p, CASE_SENSITIVE) } else { None };
        let Some(glob) = glob else { return !self.matches(pattern, kind).is_empty() };
        let skipped = skipped_by(&p);
        globbed(&self.files, &glob, skipped).next().is_some()
            || kind.directories() && globbed(&self.dirs, &glob, skipped).next().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        let ws = Workspace::from_files(
            "/p",
            &[
                ("res/fxmanifest.lua", ""),
                ("res/client/main.lua", ""),
                ("res/client/utils.lua", ""),
                ("res/client/sub/deep.lua", ""),
                ("res/Config.lua", ""),
                ("res/audio/game.dat151.rel", ""),
                ("res/node_modules/x/index.js", ""),
                ("other/init.lua", ""),
            ],
        );
        Tree::new(&ws, "/p/res")
    }

    fn rels(m: Vec<Match>) -> Vec<String> {
        m.into_iter().map(|m| if m.dir { format!("{}/", m.rel) } else { m.rel }).collect()
    }

    #[test]
    fn plain_paths_globs_and_directories() {
        let t = tree();
        assert_eq!(rels(t.matches("./client\\main.lua", PathKind::Script)), ["client/main.lua"]);
        assert_eq!(rels(t.matches("config.lua", PathKind::Script)), ["Config.lua"], "case-insensitive");
        assert!(t.matches("client", PathKind::Script).is_empty(), "scripts are files");
        assert_eq!(rels(t.matches("client", PathKind::File)), ["client/"]);
        assert_eq!(rels(t.matches("client/*.lua", PathKind::Script)), ["client/main.lua", "client/utils.lua"]);
        assert_eq!(rels(t.matches("client/**/*.lua", PathKind::Script)).len(), 3);
        assert_eq!(rels(t.matches("audio/game.dat", PathKind::DataFile)), ["audio/game.dat151.rel"]);
        assert!(t.matches("audio/game.dat", PathKind::File).is_empty());
        assert_eq!(rels(t.matches("audio", PathKind::DataFile)), ["audio/"]);
        assert!(t.matches("**/*.js", PathKind::Script).is_empty(), "node_modules is skipped");
        assert_eq!(t.matches("node_modules/**/*.js", PathKind::Script).len(), 1);
        assert_eq!(rels(t.matches("client/s*", PathKind::File)), ["client/sub/"]);
        assert!(t.matches("@other/init.lua", PathKind::Script).is_empty());
        let (dirs, files) = t.children("client/");
        assert_eq!((dirs, files), (vec!["sub".to_owned()], vec!["main.lua".to_owned(), "utils.lua".to_owned()]));
    }

    // The full scan the indexed lookup replaces: every listed file, then every directory that holds one.
    fn scanned(t: &Tree, pattern: &str, kind: PathKind) -> Vec<Match> {
        let p = normalize(pattern);
        let Some(glob) = Glob::new(&p, CASE_SENSITIVE) else { return Vec::new() };
        let lower = p.to_lowercase();
        let skipped = |s: &str| SKIPPED_DIRS.iter().any(|d| !lower.contains(d) && s.eq_ignore_ascii_case(d));
        let listed: Vec<&str> =
            t.files.iter().map(|e| e.rel.as_str()).filter(|r| !r.split('/').rev().skip(1).any(&skipped)).collect();
        let dirs: std::collections::BTreeSet<&str> =
            listed.iter().flat_map(|r| r.match_indices('/').map(|(i, _)| &r[..i])).collect();
        let files = listed.iter().filter(|r| glob.matches(r)).map(|r| Match { rel: (*r).to_owned(), dir: false });
        let dirs = dirs
            .into_iter()
            .filter(|d| kind.directories() && glob.matches(d))
            .map(|d| Match { rel: d.to_owned(), dir: true });
        files.chain(dirs).take(MAX_MATCHES).collect()
    }

    #[test]
    fn indexed_lookups_match_the_full_scan() {
        let mut paths: Vec<String> = (0..12)
            .flat_map(|i| {
                [
                    format!("client/mod{i}/main.lua"),
                    format!("client/mod{i}/sub/deep/cl_{i}.lua"),
                    format!("Client/Mod{i}/UPPER.LUA"),
                ]
            })
            .chain((0..520).map(|i| format!("stream/p{i}.ytd")))
            .collect();
        paths.extend(
            [
                "stream/sub/a.ytd",
                "fxmanifest.lua",
                "config.lua",
                "web/node_modules/x/index.js",
                "web/dist/index.html",
                "vendor/node_modules/y/index.js",
                ".git/config",
                "html/.idea/ws.xml",
                "données/é.lua",
                "Écran/main.lua",
                "\u{212a}elvin.lua",
                "\u{130}stanbul/a.lua",
                "a/b/c/d/e.lua",
                "a//b.lua",
                "audio/game.dat151.rel",
            ]
            .map(str::to_owned),
        );
        let files: Vec<(String, &str)> = paths.iter().map(|r| (format!("res/{r}"), "")).collect();
        let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), *t)).collect();
        let t = Tree::new(&Workspace::from_files("/p", &files), "/p/res");
        let kinds = [PathKind::Script, PathKind::File, PathKind::Page, PathKind::DataFile];
        let globs = [
            "**/*.lua",
            "**/*",
            "**",
            "*",
            "*.lua",
            "client/**/*.lua",
            "client/*/main.lua",
            "client/mod1*/**",
            "client/mod1?/*.lua",
            "CLIENT/MOD3/*.lua",
            "Client/*",
            "client/**/cl_*.lua",
            "./client\\mod2/*.lua",
            "/client/mod4/**/*",
            "client/mod1*",
            "stream/*",
            "stream/s*",
            "st*",
            "web/**/*.js",
            "web/node_modules/**/*.js",
            "**/node_modules",
            "**/node_modules/**",
            "vendor*",
            "*/node_modules",
            ".git/*",
            "**/.idea/*",
            "html/*",
            "données/*.lua",
            "DONNÉES/*.lua",
            "écran/*",
            "Écran/*.lua",
            "k*.lua",
            "i*/*.lua",
            "a/**/e.lua",
            "a/*/*/*/*.lua",
            "a/*",
            "audio/game.dat*",
            "missing/**/*.lua",
            "@other/*.lua",
        ];
        for p in globs {
            for kind in kinds {
                let expected = scanned(&t, p, kind);
                assert_eq!(t.matches(p, kind), expected, "{p} {kind:?}");
                assert_eq!(t.matches_any(p, kind), !expected.is_empty(), "{p} {kind:?}");
            }
        }
        assert_eq!(t.matches("stream/*", PathKind::File).len(), MAX_MATCHES);
        assert!(t.matches("vendor*", PathKind::File).is_empty(), "only node_modules below it");
        assert_eq!(rels(t.matches("vendor/node_modules", PathKind::File)), ["vendor/node_modules/"]);
        for p in [
            "client/mod1/main.lua",
            "CLIENT/mod1/MAIN.lua",
            "client",
            "client/",
            "audio/game.dat",
            "audio/x",
            "nope.lua",
        ] {
            for kind in kinds {
                assert_eq!(t.matches_any(p, kind), !t.matches(p, kind).is_empty(), "{p} {kind:?}");
            }
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(normalize(" ././/a\\b.lua "), "a/b.lua");
        assert_eq!(split_resource_path("@ox_lib/init.lua"), Some(("ox_lib", "init.lua")));
        assert_eq!(split_resource_path("@ox_lib"), Some(("ox_lib", "")));
        assert_eq!(split_resource_path("ox_lib/init.lua"), None);
        assert!(checkable("client/*.lua", PathKind::Script) && checkable("client", PathKind::Script));
        assert!(!checkable("html/style.css", PathKind::File) && !checkable("stream/*", PathKind::File));
        assert!(checkable("html/index.html", PathKind::Page) && !checkable("data/*.meta", PathKind::DataFile));
    }
}
