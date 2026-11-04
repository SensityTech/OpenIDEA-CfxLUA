use super::catalog::PathKind;
use crate::project::CASE_SENSITIVE;
use crate::project::glob::Glob;
use crate::workspace::Workspace;
use std::collections::BTreeSet;

const SKIPPED_DIRS: [&str; 4] = ["node_modules", ".git", ".svn", ".idea"];
const MAX_MATCHES: usize = 500;

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

pub struct Tree {
    pub base: String,
    files: Vec<(String, String)>,
}

fn key(rel: &str) -> String {
    if CASE_SENSITIVE { rel.to_owned() } else { rel.to_lowercase() }
}

impl Tree {
    pub fn new(ws: &Workspace, base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        let prefix = format!("{base}/");
        let mut files: Vec<(String, String)> = ws
            .files_under(&base)
            .filter_map(|f| f.path.strip_prefix(&prefix))
            .map(|rel| (key(rel), rel.to_owned()))
            .collect();
        files.sort();
        Self { base, files }
    }

    fn with_prefix<'a>(&'a self, prefix: &str) -> &'a [(String, String)] {
        let start = self.files.partition_point(|(k, _)| k.as_str() < prefix);
        let len = self.files[start..].partition_point(|(k, _)| k.starts_with(prefix));
        &self.files[start..start + len]
    }

    pub fn file(&self, rel: &str) -> Option<&str> {
        let k = key(rel);
        self.files.binary_search_by(|(f, _)| f.as_str().cmp(&k)).ok().map(|i| self.files[i].1.as_str())
    }

    pub fn has_dir(&self, rel: &str) -> bool {
        let rel = rel.trim_end_matches('/');
        !rel.is_empty() && !self.with_prefix(&key(&format!("{rel}/"))).is_empty()
    }

    pub fn children(&self, dir: &str) -> (Vec<String>, Vec<String>) {
        let mut dirs: Vec<String> = Vec::new();
        let mut files = Vec::new();
        for (_, rel) in self.with_prefix(&key(dir)) {
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
        let lower = p.to_lowercase();
        let skipped: Vec<&str> = SKIPPED_DIRS.iter().copied().filter(|d| !lower.contains(d)).collect();
        let mut out = Vec::new();
        let mut dirs = BTreeSet::new();
        for (_, rel) in &self.files {
            let mut segs: Vec<&str> = rel.split('/').collect();
            segs.pop();
            if segs.iter().any(|s| skipped.iter().any(|d| s.eq_ignore_ascii_case(d))) {
                continue;
            }
            if glob.matches(rel) {
                out.push(Match { rel: rel.clone(), dir: false });
                if out.len() >= MAX_MATCHES {
                    return out;
                }
            }
            if kind.directories() {
                for n in 1..=segs.len() {
                    dirs.insert(segs[..n].join("/"));
                }
            }
        }
        for d in dirs {
            if out.len() >= MAX_MATCHES {
                break;
            }
            if glob.matches(&d) {
                out.push(Match { rel: d, dir: true });
            }
        }
        out
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
