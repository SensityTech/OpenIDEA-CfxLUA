//! FiveM / RedM natives: the catalogs (bundled snapshot of the Cfx.re
//! databases, plus the updates the user downloaded), which of them a file may
//! use (game, client / server side, settings), and natives completion.
//!
//! Catalogs load lazily, one set at a time: a FiveM project never parses the
//! RedM database.

mod convert;
pub mod feature;
pub mod model;
mod parse;
mod sources;
mod update;

pub use feature::{NativesFeature, WRONG_SIDE, markdown};
pub use update::{AUTO_UPDATE, SCHEDULED_UPDATE, UPDATE};

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

/// Loads the natives of `set` from the bundled snapshot and the stored update.
fn load(set: NativeSet) -> Vec<Native> {
    let groups = sources::bundled_groups(set);
    let groups = match update::current_delta(set) {
        Some(delta) => update::apply(groups, &delta),
        None => groups,
    };
    convert::flatten(groups)
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
pub fn visible_sets(
    settings: &Settings,
    detected: &[Game],
    side: Side,
    resource_games: Option<&[Game]>,
) -> Vec<NativeSet> {
    if !settings.enabled() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if settings.bool(keys::CLIENT_NATIVES, true) && Side::Client.visible_from(side) {
        out = client_sets(settings, detected, resource_games);
    }
    if settings.bool(keys::SERVER_NATIVES, true) && Side::Server.visible_from(side) {
        out.push(NativeSet::Server);
    }
    out
}

/// The client sets of a script's games: the project's, narrowed to its
/// resource's `game` in auto mode (whatever the side and the switches).
pub fn client_sets(settings: &Settings, detected: &[Game], resource_games: Option<&[Game]>) -> Vec<NativeSet> {
    let auto = settings.string(keys::GAME, "auto") == "auto";
    project_games(settings, detected)
        .into_iter()
        .filter(|g| match resource_games {
            Some(r) if auto && !r.is_empty() => r.contains(g),
            _ => true,
        })
        .map(NativeSet::for_game)
        .collect()
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
        assert_eq!(
            visible_sets(&s, &both, Side::Client, Some(&[])),
            [NativeSet::Gta5, NativeSet::Rdr3],
            "game 'common'"
        );
        let s = Settings::defaults().with("game", Value::from("redm")).with("serverNatives", Value::from(false));
        assert_eq!(visible_sets(&s, &[], Side::Unknown, Some(&[Game::Gta5])), [NativeSet::Rdr3]);
        assert_eq!(
            client_sets(&s, &both, Some(&[Game::Gta5])),
            [NativeSet::Rdr3],
            "explicit mode ignores the resource"
        );
    }

    /// The bundled catalogs, converted once for every test.
    pub fn bundled(set: NativeSet) -> &'static [Native] {
        static SETS: [std::sync::OnceLock<Vec<Native>>; 3] = [const { std::sync::OnceLock::new() }; 3];
        SETS[set as usize].get_or_init(|| load(set))
    }

    fn get(set: NativeSet, lua: &str) -> &'static Native {
        bundled(set).iter().find(|n| n.lua_name == lua).unwrap_or_else(|| panic!("{lua} not in {set:?}"))
    }

    #[test]
    fn bundled_counts() {
        let count = |set| {
            let natives = bundled(set);
            let primaries: Vec<&Native> = natives.iter().filter(|n| !n.is_alias()).collect();
            let unnamed = primaries.iter().filter(|n| n.is_unnamed()).count();
            (natives.len(), primaries.len(), unnamed)
        };
        assert_eq!(count(NativeSet::Gta5), (9930, 7087, 1001));
        assert_eq!(count(NativeSet::Rdr3), (10110, 7387, 2221));
        assert_eq!(count(NativeSet::Server), (394, 390, 0));
        let rdr3 = bundled(NativeSet::Rdr3);
        assert_eq!(rdr3.iter().filter(|n| n.alt_name.is_some()).count(), 354);
        let server = bundled(NativeSet::Server);
        assert_eq!(server.iter().filter(|n| !n.is_alias() && n.rpc).count(), 78);
        let apisets: std::collections::BTreeSet<Side> = server.iter().map(|n| n.apiset).collect();
        assert_eq!(apisets, [Side::Server, Side::Shared].into());
        for set in NativeSet::ALL {
            let names: std::collections::HashSet<&str> = bundled(set).iter().map(|n| n.lua_name.as_str()).collect();
            assert_eq!(names.len(), bundled(set).len(), "one function per Lua name in {set:?}");
        }
    }

    #[test]
    fn bundled_natives() {
        let ground = get(NativeSet::Gta5, "GetGroundZFor_3dCoord");
        assert_eq!(
            (ground.namespace.as_str(), ground.original_name.as_deref()),
            ("MISC", Some("GET_GROUND_Z_FOR_3D_COORD"))
        );
        let rets: Vec<&str> = ground.returns.iter().map(|r| r.lua_type.as_str()).collect();
        assert_eq!(rets, ["boolean", "number"]);
        assert_eq!(ground.game, Some(Game::Gta5));
        assert!(ground.lua_example.as_deref().is_some_and(|e| e.contains("GetGroundZFor_3dCoord(pedCoords.x")));
        assert_eq!(get(NativeSet::Gta5, "GetRaycastResult").alias_of.as_deref(), Some("GetShapeTestResult"));
        assert!(get(NativeSet::Gta5, "DeleteEntity").params[0].kept_trailing_pointer);
        assert_eq!(get(NativeSet::Gta5, "GetActivePlayers").returns[0].lua_type, "integer[]");
        let radio = get(NativeSet::Gta5, "IsVehicleRadioOn");
        assert_eq!(radio.aliases, ["N_0x0be4be946463f917", "IsVehicleRadioEnabled"], "source order of the aliases");
        let unnamed = get(NativeSet::Rdr3, "N_0x733077295ab51304");
        assert_eq!(unnamed.original_name, None);
        assert_eq!(unnamed.alt_name.as_deref(), Some("_ADD_COVER_BLOCKING_AREA"));
        assert_eq!(unnamed.doc_url, "https://alloc8or.re/rdr3/nativedb/?n=0x733077295AB51304");
        assert_eq!((unnamed.game, unnamed.params[0].lua_type.as_str()), (Some(Game::Rdr3), "integer|nil"));
        let weapon = get(NativeSet::Rdr3, "GetCurrentPedWeapon");
        let params: Vec<&str> = weapon.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(params, ["ped", "weaponHash", "p2", "attachPoint", "p4"]);
        let coords = get(NativeSet::Rdr3, "GetEntityCoords");
        assert!(coords.description.starts_with("```\nGets the current coordinates"));
        assert_eq!(coords.params[0].description.as_deref(), Some("The entity to get the coordinates from."));
        assert!(bundled(NativeSet::Rdr3).iter().any(|n| n.lua_name == "RegisterCommand"));
        assert!(!bundled(NativeSet::Rdr3).iter().any(|n| n.lua_name == "GetVehicleDashboardCurrentGear"), "GTA V only");
        assert_eq!(get(NativeSet::Rdr3, "RegisterCommand").doc_url, "https://docs.fivem.net/natives/?_0x5FA79B0F");
        assert_eq!(get(NativeSet::Server, "GetPlayerName").params[0].lua_type, "string|integer");
        let coords = get(NativeSet::Server, "SetEntityCoords");
        assert!(coords.rpc && coords.description.contains("(https://docs.fivem.net/natives/?_0x06843DA7060A026B)"));
        let command = get(NativeSet::Server, "RegisterCommand");
        assert_eq!(command.params[1].lua_type, "fun(source:integer, args:string[], rawCommand:string)");
        assert_eq!(get(NativeSet::Gta5, "GetPlayerPed").apiset, Side::Client);
        assert_eq!(get(NativeSet::Server, "GetPlayerPed").params[0].lua_type, "string|integer");
        assert_eq!(get(NativeSet::Server, "RegisterCommand").apiset, Side::Shared);
    }

    #[test]
    fn natives_find_in_set_order() {
        let natives = Natives::from_natives(bundled(NativeSet::Server).to_vec());
        assert_eq!(
            natives.find("GetPlayerName", &[NativeSet::Gta5, NativeSet::Server]).map(|n| n.set),
            Some(NativeSet::Server)
        );
        assert!(natives.is_loaded(NativeSet::Gta5), "fixed catalogs are always loaded");
        assert_eq!(natives.catalog(NativeSet::Server).len(), 394);
    }

    /// Building the FiveM catalog is the first completion's cost: run with
    /// `cargo test --release --lib natives::tests::load_time -- --nocapture`.
    #[test]
    fn load_time() {
        let start = std::time::Instant::now();
        let gta5 = load(NativeSet::Gta5);
        let elapsed = start.elapsed();
        eprintln!("GTA5 catalog: {} natives in {} ms", gta5.len(), elapsed.as_millis());
        if !cfg!(debug_assertions) {
            assert!(elapsed.as_millis() < 300, "{elapsed:?}");
        }
    }
}
