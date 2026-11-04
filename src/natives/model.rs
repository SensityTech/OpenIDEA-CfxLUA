use crate::project::{Game, Side};
use serde::{Deserialize, Serialize};

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

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Gta5 => "FiveM",
            Self::Rdr3 => "RedM",
            Self::Server => "Server",
        }
    }

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
    pub name: String,
    pub lua_type: String,
    pub native_type: String,
    pub description: Option<String>,
    pub out_pointer: bool,
    pub kept_trailing_pointer: bool,
}

impl Param {
    pub fn is_vararg(&self) -> bool {
        self.name == "..."
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Return {
    pub name: String,
    pub lua_type: String,
    pub native_type: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Native {
    pub set: NativeSet,
    pub lua_name: String,
    pub original_name: Option<String>,
    pub hash: String,
    pub namespace: String,
    pub apiset: Side,
    pub game: Option<Game>,
    pub rpc: bool,
    pub params: Vec<Param>,
    pub returns: Vec<Return>,
    pub description: String,
    pub lua_example: Option<String>,
    pub doc_url: String,
    pub aliases: Vec<String>,
    pub alias_of: Option<String>,
    pub alt_name: Option<String>,
}

impl Native {
    pub fn is_alias(&self) -> bool {
        self.alias_of.is_some()
    }

    pub fn is_unnamed(&self) -> bool {
        self.original_name.is_none()
    }

    pub fn lua_params(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(|p| !p.out_pointer)
    }

    pub fn hash_display(&self) -> String {
        let h = self.hash.trim();
        let digits = h.strip_prefix("0x").or_else(|| h.strip_prefix("0X")).unwrap_or(h);
        format!("0x{}", digits.to_ascii_uppercase())
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.set.id(), self.lua_name)
    }

    pub fn call(&self) -> String {
        let args: Vec<&str> = self.lua_params().map(|p| p.name.as_str()).collect();
        format!("{}({})", self.lua_name, args.join(", "))
    }

    pub fn call_statement(&self) -> String {
        if self.returns.is_empty() {
            return self.call();
        }
        let rets: Vec<&str> = self.returns.iter().map(|r| r.name.as_str()).collect();
        format!("local {} = {}", rets.join(", "), self.call())
    }

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

    pub fn side_label(&self) -> &'static str {
        self.apiset.label()
    }

    pub fn game_label(&self) -> &'static str {
        match (self.game, self.set) {
            (Some(g), _) => g.label(),
            (None, NativeSet::Gta5) => "FiveM",
            (None, NativeSet::Rdr3) => "RedM",
            (None, NativeSet::Server) => "FiveM + RedM",
        }
    }
}
