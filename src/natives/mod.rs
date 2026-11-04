mod convert;
pub mod feature;
pub mod model;
mod parse;
mod sources;
mod update;

pub use feature::{NativesFeature, WRONG_SIDE, markdown};
pub use update::{AUTO_UPDATE, SCHEDULED_UPDATE, UPDATE, resume, take_changed};

pub use model::{Native, NativeSet, Param, Return};

use crate::project::{Game, Side};
use crate::settings::{Settings, keys};
use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

pub struct Catalog {
    pub set: NativeSet,
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

enum Source {
    Bundled,
    Fixed,
}

pub struct Natives {
    source: Source,
    sets: [OnceCell<Catalog>; 3],
    /// Editor calls: a catalog not loaded yet reads as empty and is loaded by the next heavy call.
    frozen: Cell<bool>,
    wanted: Cell<[bool; 3]>,
    empty: [OnceCell<Catalog>; 3],
}

impl Default for Natives {
    fn default() -> Self {
        Self::bundled()
    }
}

impl Natives {
    pub fn bundled() -> Self {
        Self::with_source(Source::Bundled)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            sets: Default::default(),
            frozen: Cell::new(false),
            wanted: Cell::new([false; 3]),
            empty: Default::default(),
        }
    }

    pub fn freeze(&self, frozen: bool) {
        self.frozen.set(frozen);
    }

    /// The catalogs editor calls asked for while frozen (taken once).
    pub fn take_wanted(&self) -> Vec<NativeSet> {
        let wanted = self.wanted.take();
        NativeSet::ALL.into_iter().filter(|&s| wanted[s as usize]).collect()
    }

    pub fn from_natives(natives: Vec<Native>) -> Self {
        let this = Self::with_source(Source::Fixed);
        let mut by_set: HashMap<NativeSet, Vec<Native>> = HashMap::new();
        for n in natives {
            by_set.entry(n.set).or_default().push(n);
        }
        for set in NativeSet::ALL {
            let _ = this.sets[set as usize].set(Catalog::new(set, by_set.remove(&set).unwrap_or_default()));
        }
        this
    }

    pub fn catalog(&self, set: NativeSet) -> &Catalog {
        if self.frozen.get() && !self.is_loaded(set) {
            let mut wanted = self.wanted.get();
            wanted[set as usize] = true;
            self.wanted.set(wanted);
            return self.empty[set as usize].get_or_init(|| Catalog::new(set, Vec::new()));
        }
        self.sets[set as usize].get_or_init(|| match self.source {
            Source::Bundled => Catalog::new(set, load(set)),
            Source::Fixed => Catalog::new(set, Vec::new()),
        })
    }

    pub fn is_loaded(&self, set: NativeSet) -> bool {
        self.sets[set as usize].get().is_some()
    }

    pub fn invalidate(&mut self, sets: &[NativeSet]) {
        for &set in sets {
            self.sets[set as usize] = OnceCell::new();
        }
    }

    pub fn find(&self, lua_name: &str, sets: &[NativeSet]) -> Option<&Native> {
        sets.iter().find_map(|&s| self.catalog(s).get(lua_name))
    }
}

fn load(set: NativeSet) -> Vec<Native> {
    let groups = sources::bundled_groups(set);
    let groups = match update::current_delta(set) {
        Some(delta) => update::apply(groups, &delta),
        None => groups,
    };
    convert::flatten(groups)
}

pub fn project_games(settings: &Settings, detected: &[Game]) -> Vec<Game> {
    match settings.string(keys::GAME, "auto").as_str() {
        "fivem" => vec![Game::Gta5],
        "redm" => vec![Game::Rdr3],
        "both" => vec![Game::Gta5, Game::Rdr3],
        _ if detected.is_empty() => vec![Game::Gta5],
        _ => Game::ALL.into_iter().filter(|g| detected.contains(g)).collect(),
    }
}

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
        let at_least = |(all, primaries, unnamed): (usize, usize, usize), min: (usize, usize, usize)| {
            all >= min.0 && primaries >= min.1 && unnamed >= min.2 && primaries <= all
        };
        assert!(at_least(count(NativeSet::Gta5), (9000, 6500, 500)), "{:?}", count(NativeSet::Gta5));
        assert!(at_least(count(NativeSet::Rdr3), (9000, 6500, 1000)), "{:?}", count(NativeSet::Rdr3));
        assert!(at_least(count(NativeSet::Server), (300, 300, 0)), "{:?}", count(NativeSet::Server));
        assert_eq!(count(NativeSet::Server).2, 0, "server natives all have names");
        let rdr3 = bundled(NativeSet::Rdr3);
        assert!(rdr3.iter().filter(|n| n.alt_name.is_some()).count() > 200);
        let server = bundled(NativeSet::Server);
        assert!(server.iter().filter(|n| !n.is_alias() && n.rpc).count() > 50);
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
        assert_eq!(natives.catalog(NativeSet::Server).len(), bundled(NativeSet::Server).len());
    }

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
