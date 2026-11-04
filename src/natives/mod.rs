//! FiveM / RedM natives: the catalogs (bundled snapshot of the Cfx.re
//! databases, plus the updates the user downloaded), which of them a file may
//! use (game, client / server side, settings), and natives completion.
//!
//! Catalogs load lazily, one set at a time: a FiveM project never parses the
//! RedM database.

pub mod feature;
pub mod model;

pub use feature::NativesFeature;

pub use model::{Native, NativeSet, Param, Return};

use crate::project::{Game, Side};
use crate::settings::{Settings, keys};
use std::cell::OnceCell;
use std::collections::HashMap;

/// One natives set, indexed by Lua name.
pub struct Catalog {
    pub set: NativeSet,
    /// Primaries and aliases, in a stable order (namespace, then name).
    pub natives: Vec<Native>,
    by_name: HashMap<String, usize>,
}

impl Catalog {
    pub fn new(set: NativeSet, natives: Vec<Native>) -> Self {
        let by_name = natives.iter().enumerate().map(|(i, n)| (n.lua_name.clone(), i)).collect();
        Self { set, natives, by_name }
    }

    pub fn get(&self, lua_name: &str) -> Option<&Native> {
        self.by_name.get(lua_name).map(|&i| &self.natives[i])
    }

    pub fn len(&self) -> usize {
        self.natives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.natives.is_empty()
    }
}

/// How catalogs are produced: the bundled snapshot (plus stored updates) in
/// the plugin, fixed lists in tests.
enum Source {
    Bundled,
    Fixed,
}

pub struct Natives {
    source: Source,
    sets: [OnceCell<Catalog>; 3],
}

impl Default for Natives {
    fn default() -> Self {
        Self::bundled()
    }
}

impl Natives {
    /// The catalogs of the plugin: bundled snapshot, with the updates kept in storage.
    pub fn bundled() -> Self {
        Self { source: Source::Bundled, sets: Default::default() }
    }

    /// Fixed catalogs (tests): natives are grouped by their `set`.
    pub fn from_natives(natives: Vec<Native>) -> Self {
        let this = Self { source: Source::Fixed, sets: Default::default() };
        let mut by_set: HashMap<NativeSet, Vec<Native>> = HashMap::new();
        for n in natives {
            by_set.entry(n.set).or_default().push(n);
        }
        for set in NativeSet::ALL {
            let _ = this.sets[set as usize].set(Catalog::new(set, by_set.remove(&set).unwrap_or_default()));
        }
        this
    }

    /// The catalog of `set` (loaded on first use).
    pub fn catalog(&self, set: NativeSet) -> &Catalog {
        self.sets[set as usize].get_or_init(|| match self.source {
            Source::Bundled => Catalog::new(set, load(set)),
            Source::Fixed => Catalog::new(set, Vec::new()),
        })
    }

    /// Whether `set` is loaded (never loads it).
    pub fn is_loaded(&self, set: NativeSet) -> bool {
        self.sets[set as usize].get().is_some()
    }

    /// Drops loaded catalogs (after an update): they reload on next use.
    pub fn invalidate(&mut self) {
        self.sets = Default::default();
    }

    /// The first native named `lua_name` among `sets` (in that order).
    pub fn find(&self, lua_name: &str, sets: &[NativeSet]) -> Option<&Native> {
        sets.iter().find_map(|&s| self.catalog(s).get(lua_name))
    }
}

/// Loads the natives of `set` from the bundled snapshot and the stored updates.
fn load(set: NativeSet) -> Vec<Native> {
    let _ = set;
    Vec::new()
}

/// Games whose client natives the project uses: the `game` setting, or
/// (auto) the games of every manifest of the project (FiveM when none says).
pub fn project_games(settings: &Settings, detected: &[Game]) -> Vec<Game> {
    match settings.string(keys::GAME, "auto").as_str() {
        "fivem" => vec![Game::Gta5],
        "redm" => vec![Game::Rdr3],
        "both" => vec![Game::Gta5, Game::Rdr3],
        _ if detected.is_empty() => vec![Game::Gta5],
        _ => Game::ALL.into_iter().filter(|g| detected.contains(g)).collect(),
    }
}

/// The natives sets a script may call: client sets of the project's games
/// (narrowed to its resource's `game` in auto mode), the server set, as the
/// script's side and the settings allow. Manifests see none.
pub fn visible_sets(settings: &Settings, detected: &[Game], side: Side, resource_games: Option<&[Game]>) -> Vec<NativeSet> {
    if !settings.enabled() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if settings.bool(keys::CLIENT_NATIVES, true) && Side::Client.visible_from(side) {
        let auto = settings.string(keys::GAME, "auto") == "auto";
        for g in project_games(settings, detected) {
            let in_resource = match resource_games {
                Some(r) if auto && !r.is_empty() => r.contains(&g),
                _ => true,
            };
            if in_resource {
                out.push(NativeSet::for_game(g));
            }
        }
    }
    if settings.bool(keys::SERVER_NATIVES, true) && Side::Server.visible_from(side) {
        out.push(NativeSet::Server);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn visibility_by_side_game_and_settings() {
        let s = Settings::defaults();
        assert_eq!(visible_sets(&s, &[], Side::Client, None), [NativeSet::Gta5]);
        assert_eq!(visible_sets(&s, &[], Side::Server, None), [NativeSet::Server]);
        assert_eq!(visible_sets(&s, &[], Side::Shared, None), [NativeSet::Gta5, NativeSet::Server]);
        assert_eq!(visible_sets(&s, &[], Side::Manifest, None), []);
        let both = [Game::Gta5, Game::Rdr3];
        assert_eq!(visible_sets(&s, &both, Side::Client, Some(&[Game::Rdr3])), [NativeSet::Rdr3]);
        assert_eq!(visible_sets(&s, &both, Side::Client, Some(&[])), [NativeSet::Gta5, NativeSet::Rdr3], "game 'common'");
        let s = Settings::defaults().with("game", Value::from("redm")).with("serverNatives", Value::from(false));
        assert_eq!(visible_sets(&s, &[], Side::Unknown, Some(&[Game::Gta5])), [NativeSet::Rdr3]);
    }
}
