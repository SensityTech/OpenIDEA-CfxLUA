//! The Lua view of a native, as every feature uses it (completion, docs,
//! values, perf). Built by the converter from the Cfx.re databases.

use crate::project::{Game, Side};
use serde::{Deserialize, Serialize};

/// The three natives sets. FiveM projects use GTA5 + SERVER, RedM ones RDR3 + SERVER.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NativeSet {
    Gta5,
    Rdr3,
    Server,
}

impl NativeSet {
    pub const ALL: [NativeSet; 3] = [NativeSet::Gta5, NativeSet::Rdr3, NativeSet::Server];

    pub fn id(self) -> &'static str {
        match self {
            Self::Gta5 => "GTA5",
            Self::Rdr3 => "RDR3",
            Self::Server => "SERVER",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id().eq_ignore_ascii_case(id))
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Gta5 => "FiveM client natives (GTA V)",
            Self::Rdr3 => "RedM client natives (RDR3)",
            Self::Server => "Server natives (FXServer)",
        }
    }

    /// `FiveM`, `RedM`, `Server`.
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Gta5 => "FiveM",
            Self::Rdr3 => "RedM",
            Self::Server => "Server",
        }
    }

    /// The client game of the set (none for the server set).
    pub fn game(self) -> Option<Game> {
        match self {
            Self::Gta5 => Some(Game::Gta5),
            Self::Rdr3 => Some(Game::Rdr3),
            Self::Server => None,
        }
    }

    pub fn for_game(game: Game) -> Self {
        match game {
            Game::Gta5 => Self::Gta5,
            Game::Rdr3 => Self::Rdr3,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    /// Sanitised Lua name, or `...` for a vararg.
    pub name: String,
    /// For an out-pointer: the type of the returned value.
    pub lua_type: String,
    /// Display type of the database: `Ped`, `Hash`, `BOOL`, `char*`, `Vector3*`, `func`, `...`.
    pub native_type: String,
    pub description: Option<String>,
    /// Not passed in the Lua call: returned instead.
    pub out_pointer: bool,
    /// A single trailing pointer kept in the call (optional initial value) and also returned.
    pub kept_trailing_pointer: bool,
}

impl Param {
    pub fn is_vararg(&self) -> bool {
        self.name == "..."
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Return {
    /// `retval` for the native's result, else the pointer parameter's Lua name.
    pub name: String,
    pub lua_type: String,
    pub native_type: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Native {
    pub set: NativeSet,
    pub lua_name: String,
    /// `GET_ENTITY_COORDS`; `None` for unnamed natives (hash only).
    pub original_name: Option<String>,
    /// As written in the database: `0xC906A7DAB05C8D2B` (CFX natives: 32-bit `0x406B4B20`).
    pub hash: String,
    pub namespace: String,
    /// `client` / `server` / `shared`.
    pub apiset: Side,
    /// `None`: every game of the set.
    pub game: Option<Game>,
    /// Server-side RPC version of a client native (OneSync).
    pub rpc: bool,
    /// All parameters, in declaration order (out-pointers included).
    pub params: Vec<Param>,
    /// What the Lua call returns, in order.
    pub returns: Vec<Return>,
    /// Markdown.
    pub description: String,
    pub lua_example: Option<String>,
    pub doc_url: String,
    /// Deprecated alias names of this primary.
    pub aliases: Vec<String>,
    /// The primary's Lua name, when this entry is a deprecated alias.
    pub alias_of: Option<String>,
    /// RDR3 NativeDB name when it differs (documentation only).
    pub alt_name: Option<String>,
}

impl Native {
    pub fn is_alias(&self) -> bool {
        self.alias_of.is_some()
    }

    pub fn is_unnamed(&self) -> bool {
        self.original_name.is_none()
    }

    /// The parameters written in the call (out-pointers left out).
    pub fn lua_params(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(|p| !p.out_pointer)
    }

    /// `0x` + upper-case hex digits.
    pub fn hash_display(&self) -> String {
        let h = self.hash.trim();
        let digits = h.strip_prefix("0x").or_else(|| h.strip_prefix("0X")).unwrap_or(h);
        format!("0x{}", digits.to_ascii_uppercase())
    }

    /// Tool window id: `GTA5:GetEntityCoords`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.set.id(), self.lua_name)
    }

    /// `GetEntityCoords(entity, alive)`.
    pub fn call(&self) -> String {
        let args: Vec<&str> = self.lua_params().map(|p| p.name.as_str()).collect();
        format!("{}({})", self.lua_name, args.join(", "))
    }

    /// `local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, ignoreWater)`.
    pub fn call_statement(&self) -> String {
        if self.returns.is_empty() {
            return self.call();
        }
        let rets: Vec<&str> = self.returns.iter().map(|r| r.name.as_str()).collect();
        format!("local {} = {}", rets.join(", "), self.call())
    }

    /// `GetGroundZFor_3dCoord(x: number, y: number, z: number, ignoreWater: boolean): boolean, number`.
    pub fn typed_signature(&self) -> String {
        let args: Vec<String> = self.lua_params().map(|p| format!("{}: {}", p.name, p.lua_type)).collect();
        let mut s = format!("{}({})", self.lua_name, args.join(", "));
        if !self.returns.is_empty() {
            let rets: Vec<&str> = self.returns.iter().map(|r| r.lua_type.as_str()).collect();
            s.push_str(": ");
            s.push_str(&rets.join(", "));
        }
        s
    }

    /// `Client` / `Server` / `Shared`.
    pub fn side_label(&self) -> &'static str {
        self.apiset.label()
    }

    /// `FiveM`, `RedM`, or `FiveM + RedM` for server natives.
    pub fn game_label(&self) -> &'static str {
        match (self.game, self.set) {
            (Some(g), _) => g.label(),
            (None, NativeSet::Gta5) => "FiveM",
            (None, NativeSet::Rdr3) => "RedM",
            (None, NativeSet::Server) => "FiveM + RedM",
        }
    }
}
