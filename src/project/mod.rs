//! Resources and their scripts: which game a resource targets, and whether a
//! Lua file runs on the client, the server or both — the "dual" view every
//! other feature filters with (a client script is not offered server natives).
//!
//! Rules (same as FXServer, and the JetBrains plugin before this one):
//! manifest listing (`client_scripts`, `server_scripts`, `shared_scripts`,
//! globs) > file name (`client.lua`, `cl_*.lua`, …) > nearest folder name
//! inside the resource (`client/`, `cl/`, …) > unknown.

pub mod glob;
pub mod manifest;

use crate::lua::Source;
use crate::text::{file_name, parent};
use glob::Glob;
pub use manifest::{Entry, Value};

/// Manifest file names, by priority (FXServer ignores `__resource.lua` next to an `fxmanifest.lua`).
pub const MANIFESTS: [&str; 2] = ["fxmanifest.lua", "__resource.lua"];

pub fn is_manifest_name(name: &str) -> bool {
    MANIFESTS.iter().any(|m| m.eq_ignore_ascii_case(name))
}

/// A file encrypted by Cfx Asset Escrow (binary `FXAP` header): never parsed as Lua.
pub fn is_escrowed(text: &str) -> bool {
    text.starts_with("FXAP")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    Manifest,
    Lua,
    /// JS / TS / Vue / Svelte / HTML: NUI pages.
    Web,
    /// `vite.config.*` / `webpack.config.*`.
    BundlerConfig,
    Other,
}

pub fn file_kind(path: &str) -> FileKind {
    let name = file_name(path).to_ascii_lowercase();
    if is_manifest_name(&name) {
        return FileKind::Manifest;
    }
    if name.starts_with("vite.config.") || name.starts_with("webpack.config.") {
        return FileKind::BundlerConfig;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "lua" => FileKind::Lua,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "svelte" | "html" | "htm" => {
            FileKind::Web
        }
        _ => FileKind::Other,
    }
}

/// Where a script runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Client,
    Server,
    /// Listed as shared, or as both client and server.
    Shared,
    Manifest,
    Unknown,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Server => "Server",
            Self::Shared => "Shared",
            Self::Manifest => "Manifest",
            Self::Unknown => "Unknown",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
            Self::Shared => "shared",
            Self::Manifest => "manifest",
            Self::Unknown => "unknown",
        }
    }

    /// `client` / `server` / `shared` (an apiset or an event side).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "client" => Some(Self::Client),
            "server" => Some(Self::Server),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }

    /// Whether code of side `self` (a native's apiset, an API) is usable from a script on `script`.
    pub fn visible_from(self, script: Side) -> bool {
        match script {
            Side::Client => self != Side::Server,
            Side::Server => self != Side::Client,
            Side::Manifest => false,
            Side::Shared | Side::Unknown => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Game {
    Gta5,
    Rdr3,
}

impl Game {
    pub const ALL: [Game; 2] = [Game::Gta5, Game::Rdr3];

    pub fn from_manifest(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gta5" => Some(Self::Gta5),
            "rdr3" => Some(Self::Rdr3),
            _ => None,
        }
    }

    pub fn manifest_name(self) -> &'static str {
        match self {
            Self::Gta5 => "gta5",
            Self::Rdr3 => "rdr3",
        }
    }

    /// `FiveM` / `RedM`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Gta5 => "FiveM",
            Self::Rdr3 => "RedM",
        }
    }
}

/// `FiveM + RedM`, `none`.
pub fn describe_games(games: &[Game]) -> String {
    if games.is_empty() {
        return "none".into();
    }
    Game::ALL.iter().filter(|g| games.contains(g)).map(|g| g.label()).collect::<Vec<_>>().join(" + ")
}

/// A resource: a directory with a manifest.
#[derive(Clone, Debug)]
pub struct Resource {
    /// Absolute directory, forward slashes.
    pub dir: String,
    /// Folder name (the resource name FXServer uses).
    pub name: String,
    /// `fxmanifest.lua` or `__resource.lua`.
    pub manifest: String,
    pub entries: Vec<Entry>,
    pub client: Vec<Glob>,
    pub server: Vec<Glob>,
    pub shared: Vec<Glob>,
    /// `None` when the manifest declares no `game` / `games` (may be empty: `game 'common'`).
    pub games: Option<Vec<Game>>,
    /// `escrow_ignore` patterns, when declared.
    pub escrow_ignore: Option<Vec<Glob>>,
}

/// Windows (where FiveM servers mostly run) matches paths case-insensitively.
pub const CASE_SENSITIVE: bool = false;

impl Resource {
    pub fn parse(dir: &str, manifest: &str, text: &str) -> Self {
        let src = Source::new(text);
        let entries = manifest::entries(&src);
        let mut r = Self {
            dir: dir.to_owned(),
            name: file_name(dir).to_owned(),
            manifest: manifest.to_owned(),
            entries: Vec::new(),
            client: Vec::new(),
            server: Vec::new(),
            shared: Vec::new(),
            games: None,
            escrow_ignore: None,
        };
        for e in &entries {
            let globs = || e.values.iter().filter_map(|v| Glob::new(&v.text, CASE_SENSITIVE)).collect::<Vec<_>>();
            match e.name.as_str() {
                "client_script" | "client_scripts" => r.client.extend(globs()),
                "server_script" | "server_scripts" => r.server.extend(globs()),
                "shared_script" | "shared_scripts" => r.shared.extend(globs()),
                "game" | "games" => {
                    let games = r.games.get_or_insert_with(Vec::new);
                    for v in &e.values {
                        if let Some(g) = Game::from_manifest(&v.text)
                            && !games.contains(&g)
                        {
                            games.push(g);
                        }
                    }
                }
                "escrow_ignore" => r.escrow_ignore.get_or_insert_with(Vec::new).extend(globs()),
                _ => {}
            }
        }
        r.entries = entries;
        r
    }

    pub fn manifest_path(&self) -> String {
        format!("{}/{}", self.dir, self.manifest)
    }

    /// Path relative to the resource directory, or `None` outside it.
    pub fn relative<'a>(&self, path: &'a str) -> Option<&'a str> {
        path.strip_prefix(self.dir.as_str())?.strip_prefix('/')
    }

    /// Side the manifest gives `rel`, if it lists it.
    pub fn listed_side(&self, rel: &str) -> Option<Side> {
        let any = |g: &[Glob]| g.iter().any(|g| g.matches(rel));
        let (sh, c, s) = (any(&self.shared), any(&self.client), any(&self.server));
        if sh || (c && s) {
            Some(Side::Shared)
        } else if c {
            Some(Side::Client)
        } else if s {
            Some(Side::Server)
        } else {
            None
        }
    }

    pub fn side_of(&self, path: &str) -> Side {
        if is_manifest_name(file_name(path)) && parent(path) == self.dir {
            return Side::Manifest;
        }
        match self.relative(path) {
            Some(rel) => self.listed_side(rel).unwrap_or_else(|| guess_side(path, Some(&self.dir))),
            None => guess_side(path, None),
        }
    }

    /// Entries named one of `names`.
    pub fn entries_named<'a>(&'a self, names: &'a [&str]) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries.iter().filter(move |e| names.contains(&e.name.as_str()))
    }

    pub fn is_escrow_ignored(&self, rel: &str) -> bool {
        self.escrow_ignore.as_ref().is_some_and(|g| g.iter().any(|g| g.matches(rel)))
    }
}

/// Side of a file outside any resource: file name only.
pub fn side_by_convention(path: &str) -> Side {
    if is_manifest_name(file_name(path)) {
        return Side::Manifest;
    }
    guess_side(path, None)
}

/// File name conventions, then (inside `boundary`) the nearest folder name.
pub fn guess_side(path: &str, boundary: Option<&str>) -> Side {
    let name = file_name(path).to_ascii_lowercase();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
    if stem == "client" || stem.starts_with("cl_") {
        return Side::Client;
    }
    if stem == "server" || stem.starts_with("sv_") {
        return Side::Server;
    }
    if stem == "shared" || stem.starts_with("sh_") {
        return Side::Shared;
    }
    let Some(boundary) = boundary else { return Side::Unknown };
    let mut dir = parent(path);
    while !dir.is_empty() && dir != boundary && dir.len() > boundary.len() {
        match file_name(dir).to_ascii_lowercase().as_str() {
            "client" | "cl" => return Side::Client,
            "server" | "sv" => return Side::Server,
            "shared" | "sh" => return Side::Shared,
            _ => {}
        }
        dir = parent(dir);
    }
    Side::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn side_matrix() {
        let manifest = "client_scripts {'client/*.lua','ui/**.lua','src/c/*.lua'}\nserver_script 'server/**/*.lua'\n\
                        shared_scripts {'@ox_lib/init.lua','config.lua'}\nclient_script 'both.lua'\nserver_script 'both.lua'\n";
        let ws = Workspace::from_files("/p", &[("res/fxmanifest.lua", manifest)]);
        let side = |rel: &str| ws.side_of(&format!("/p/res/{rel}"));
        assert_eq!(side("client/main.lua"), Side::Client);
        assert_eq!(side("ui/menu.lua"), Side::Client);
        assert_eq!(side("ui/pages/deep/page.lua"), Side::Client);
        assert_eq!(side("server/main.lua"), Side::Server);
        assert_eq!(side("server/db/queries.lua"), Side::Server);
        assert_eq!(side("config.lua"), Side::Shared);
        assert_eq!(side("both.lua"), Side::Shared);
        assert_eq!(side("src/c/main.lua"), Side::Client);
        assert_eq!(side("src/c/nested/util.lua"), Side::Unknown);
        assert_eq!(side("ox_lib/init.lua"), Side::Unknown);
        assert_eq!(side("sv_utils.lua"), Side::Server);
        assert_eq!(side("cl_utils.lua"), Side::Client);
        assert_eq!(side("sh_utils.lua"), Side::Shared);
        assert_eq!(side("modules/sv/bank.lua"), Side::Server);
        assert_eq!(side("modules/cl/hud.lua"), Side::Client);
        assert_eq!(side("modules/misc.lua"), Side::Unknown);
        assert_eq!(side("fxmanifest.lua"), Side::Manifest);
        assert_eq!(ws.side_of("/p/scripts/client.lua"), Side::Client);
        assert_eq!(ws.side_of("/p/scripts/utils.lua"), Side::Unknown);
    }

    #[test]
    fn nearest_manifest_and_games() {
        let ws = Workspace::from_files(
            "/p",
            &[
                ("outer/fxmanifest.lua", "server_script 'inner/main.lua'\ngame 'rdr3'\nrdr3_warning 'x'"),
                ("outer/inner/fxmanifest.lua", "client_script 'main.lua'\ngames { 'gta5', 'rdr3' }"),
                ("common/fxmanifest.lua", "game 'common'"),
            ],
        );
        assert_eq!(ws.side_of("/p/outer/inner/main.lua"), Side::Client);
        assert_eq!(ws.resource("/p/outer").unwrap().games, Some(vec![Game::Rdr3]));
        assert_eq!(ws.resource("/p/outer/inner").unwrap().games, Some(vec![Game::Gta5, Game::Rdr3]));
        assert_eq!(ws.resource("/p/common").unwrap().games, Some(vec![]));
        assert_eq!(describe_games(&[Game::Rdr3, Game::Gta5]), "FiveM + RedM");
    }
}
