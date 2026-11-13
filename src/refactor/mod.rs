pub mod edges;
pub mod rename;

use crate::natives::{NativeSet, Natives};
use crate::runtime::stubs::runtime;
use edges::{Anchor, Family};

/// The host refuses a document that gives more anchors than this.
pub const MOST_IN_A_FILE: usize = 4096;
/// The host keeps 200,000 anchors for a project and refuses the file that passes them; Find Usages then waits
/// for an index that never comes.
pub const MOST_IN_A_PROJECT: usize = 180_000;
/// Up to how many Lua files a project gives every family, then its globals. Measured on 3,500 files: 3 class
/// anchors, 54 global ones and 65 member ones a file. A family is given by every file of a project or by none,
/// a name found in half the files being worse than the text search the IDE falls back to.
const EVERY_FAMILY: usize = 600;
const GLOBALS_TOO: usize = 1200;

/// Whether a global name is declared by what no project writes: a native of a catalog that is loaded, the
/// runtime, the standard library. A catalog that is not loaded is not loaded for it.
pub fn is_library(natives: &Natives, name: &str) -> bool {
    let native = |set: &NativeSet| natives.is_loaded(*set) && natives.catalog(*set).get(name).is_some();
    crate::lua::std::global(name).is_some() || runtime().global(name).is_some() || NativeSet::ALL.iter().any(native)
}

/// The families a project of that many Lua files gives the host.
pub fn families(lua_files: usize) -> &'static [Family] {
    if lua_files <= EVERY_FAMILY {
        &[Family::Class, Family::Global, Family::Member]
    } else if lua_files <= GLOBALS_TOO {
        &[Family::Class, Family::Global]
    } else {
        &[Family::Class]
    }
}

/// The anchors of a file its project may give, `room` being what the project may still give: a family that
/// does not fit is left out whole, the members first.
pub fn kept(mut anchors: Vec<Anchor>, lua_files: usize, room: usize) -> Vec<Anchor> {
    let given = families(lua_files);
    anchors.retain(|anchor| given.contains(&anchor.family));
    let room = room.min(MOST_IN_A_FILE);
    for family in [Family::Member, Family::Global] {
        if anchors.len() > room {
            anchors.retain(|anchor| anchor.family != family);
        }
    }
    anchors.truncate(room);
    anchors
}

#[cfg(test)]
mod edges_tests;
