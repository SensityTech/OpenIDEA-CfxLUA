use super::model::{Native, NativeSet, Param, Return};
use super::parse::{Kind, NativeType, Origin, RawNative};
use crate::project::{Game, Side};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

pub const FIVEM_DOC_URL: &str = "https://docs.fivem.net/natives/?_";
pub const RDR3_DOC_URL: &str = "https://alloc8or.re/rdr3/nativedb/?n=";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overrides {
    #[serde(default)]
    pub return_types: HashMap<String, String>,
    #[serde(default)]
    pub param_types: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub string_param_types: HashMap<String, String>,
}

impl Overrides {
    pub fn bundled() -> Self {
        serde_json::from_str(super::sources::OVERRIDES).unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alias {
    pub lua: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub native: Native,
    pub aliases: Vec<Alias>,
}

pub fn lua_name(native: &str) -> String {
    let lower = native.to_lowercase().replace("0x", "n_0x");
    let mut out = String::with_capacity(lower.len());
    let mut chars = lower.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '_' && next.is_ascii_alphabetic() => {
                out.push(next.to_ascii_uppercase());
                chars.next();
            }
            _ => out.push(c),
        }
    }
    // The first letter that has something after it.
    if let Some(p) = out.find(|c: char| c.is_ascii_alphabetic())
        && p + 1 < out.len()
    {
        out[p..=p].make_ascii_uppercase();
    }
    out
}

pub fn param_name(raw: &str, index: usize) -> String {
    let mut name: String =
        raw.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if name.bytes().all(|b| b == b'_') {
        name = format!("p{index}");
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    if crate::lua::lexer::is_keyword(&name) {
        name.push('_');
    }
    name
}

fn belongs_to(raw: &RawNative, set: NativeSet) -> bool {
    let apiset = raw.apiset.as_ref();
    let client_side = apiset == "client" || apiset == "shared";
    let game = raw.game.as_deref();
    match set {
        NativeSet::Gta5 => client_side && game.is_none_or(|g| g == "gta5"),
        NativeSet::Rdr3 => client_side && game.is_none_or(|g| g == "rdr3"),
        NativeSet::Server => apiset == "server" || apiset == "shared",
    }
}

fn param_type(t: &NativeType, server: bool) -> &'static str {
    match t.kind {
        Kind::Int | Kind::Handle => "integer",
        Kind::Float => "number",
        Kind::Bool => "boolean",
        Kind::String => "string",
        Kind::Hash => "integer|string",
        Kind::Player if server => "string|integer",
        Kind::Player => "integer",
        Kind::Vector3 => "vector3",
        Kind::Func => "function",
        Kind::Any | Kind::Object | Kind::Unknown => "any",
    }
}

fn out_type(t: &NativeType) -> &'static str {
    match t.kind {
        Kind::Float => "number",
        Kind::Vector3 => "vector3",
        _ => "integer",
    }
}

fn kept_pointer_type(t: &NativeType) -> &'static str {
    match t.kind {
        Kind::Float => "number|nil",
        k if k.int_like() => "integer|nil",
        _ => "any",
    }
}

fn return_type(t: &NativeType, server: bool) -> &'static str {
    if t.pointer {
        return "integer";
    }
    match t.kind {
        Kind::Float => "number",
        Kind::Bool => "boolean",
        Kind::String => "string",
        Kind::Player if server => "string",
        Kind::Vector3 => "vector3",
        Kind::Func => "function",
        Kind::Object => "any",
        _ => "integer",
    }
}

fn normalize_line_breaks(text: &str) -> Cow<'_, str> {
    if text.contains('\r') { Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n")) } else { Cow::Borrowed(text) }
}

fn relative_link(s: &str) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if !matches!(b.first(), Some(b'#' | b'?')) {
        return None;
    }
    let mut i = 1 + usize::from(b.get(1) == Some(&b'\\'));
    if b.get(i) != Some(&b'_') || !s[i + 1..].starts_with("0x") {
        return None;
    }
    i += 1;
    let start = i;
    let digits = s[i + 2..].bytes().take_while(u8::is_ascii_hexdigit).count();
    i += 2 + digits;
    (digits > 0 && b.get(i) == Some(&b')')).then(|| (&s[start..i], i + 1))
}

pub fn normalize_markdown(text: &str) -> String {
    let text = normalize_line_breaks(text);
    let mut rest = text.trim();
    let mut out = String::with_capacity(rest.len());
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]);
        let after = &rest[i + 2..];
        match relative_link(after) {
            Some((hash, len)) => {
                out.push_str(FIVEM_DOC_URL);
                out.push_str(hash);
                out.push(')');
                rest = &after[len..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

pub fn normalize_code(code: &str) -> String {
    let code = normalize_line_breaks(code);
    let lines: Vec<&str> = code.split('\n').map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_owned()
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn build(set: NativeSet, raw: &RawNative, overrides: &Overrides) -> Native {
    let server = set == NativeSet::Server;
    let pointers = raw.params.iter().filter(|p| p.ty.pointer).count();
    let single_trailing = pointers == 1 && raw.params.last().is_some_and(|p| p.ty.pointer);
    let param_overrides = raw.name.as_deref().and_then(|n| overrides.param_types.get(n));
    let mut used: HashSet<String> = HashSet::new();
    let mut params = Vec::with_capacity(raw.params.len());
    for (i, p) in raw.params.iter().enumerate() {
        let description = p.description.as_deref().map(normalize_markdown).and_then(non_empty);
        let native_type = p.ty.display.clone();
        if p.ty.vararg {
            let lua_type = "any".to_owned();
            params.push(Param { name: "...".into(), lua_type, native_type, description, ..Param::default() });
            continue;
        }
        let base = param_name(&p.name, i);
        let mut name = base.clone();
        let mut suffix = 2;
        while !used.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        let (lua_type, out_pointer, kept_trailing_pointer) = if p.ty.pointer && !single_trailing {
            (out_type(&p.ty).to_owned(), true, false)
        } else if p.ty.pointer {
            (kept_pointer_type(&p.ty).to_owned(), false, true)
        } else {
            let string_override = || {
                (raw.origin == Origin::CfxJson && p.ty.kind == Kind::String)
                    .then(|| overrides.string_param_types.get(p.name.as_ref()))
                    .flatten()
            };
            let lua = param_overrides.and_then(|m| m.get(p.name.as_ref())).or_else(string_override);
            (lua.map_or_else(|| param_type(&p.ty, server).to_owned(), Clone::clone), false, false)
        };
        params.push(Param { name, lua_type, native_type, description, out_pointer, kept_trailing_pointer });
    }

    let mut returns = Vec::new();
    if let Some(result) = &raw.result {
        let lua_type = raw.name.as_deref().and_then(|n| overrides.return_types.get(n));
        returns.push(Return {
            name: "retval".into(),
            lua_type: lua_type.map_or_else(|| return_type(result, server).to_owned(), Clone::clone),
            native_type: result.display.clone(),
            description: raw.results_description.as_deref().map(normalize_markdown).and_then(non_empty),
        });
    }
    for (raw_param, p) in raw.params.iter().zip(&params) {
        if raw_param.ty.pointer {
            returns.push(Return {
                name: p.name.clone(),
                lua_type: out_type(&raw_param.ty).to_owned(),
                native_type: p.native_type.clone(),
                description: p.description.clone(),
            });
        }
    }

    let description = normalize_markdown(&raw.description);
    let rpc = raw.origin == Origin::CfxJson
        && raw.apiset == "server"
        && description.to_ascii_lowercase().contains("server-side rpc native");
    let doc_url = if raw.origin == Origin::Rdr3Dsl { RDR3_DOC_URL } else { FIVEM_DOC_URL };
    Native {
        set,
        lua_name: lua_name(raw.name.as_deref().unwrap_or(&raw.hash)),
        original_name: raw.name.as_deref().map(str::to_owned),
        hash: raw.hash.to_string(),
        namespace: raw.namespace.to_string(),
        apiset: Side::parse(&raw.apiset).unwrap_or(Side::Client),
        game: raw.game.as_deref().and_then(Game::from_manifest),
        rpc,
        params,
        returns,
        description,
        lua_example: raw.lua_example.as_deref().map(normalize_code).and_then(non_empty),
        doc_url: format!("{doc_url}{}", raw.hash),
        aliases: Vec::new(),
        alias_of: None,
        alt_name: raw.alt_name.clone(),
    }
}

struct Candidate<'a> {
    origin: Origin,
    native: Native,
    alias_sources: Vec<Cow<'a, str>>,
}

fn merge<'a>(existing: &mut Candidate<'a>, mut other: Candidate<'a>) {
    if other.origin == Origin::CfxJson && existing.origin != Origin::CfxJson {
        std::mem::swap(existing, &mut other);
    }
    if existing.native.description.trim().is_empty() {
        existing.native.description = std::mem::take(&mut other.native.description);
    }
    existing.alias_sources.append(&mut other.alias_sources);
}

pub fn convert(set: NativeSet, natives: Vec<RawNative<'_>>, overrides: &Overrides) -> Vec<Group> {
    let mut primaries: Vec<Candidate> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for raw in natives {
        if !belongs_to(&raw, set) {
            continue;
        }
        let native = build(set, &raw, overrides);
        let candidate = Candidate { origin: raw.origin, native, alias_sources: raw.aliases };
        match index.get(&candidate.native.lua_name) {
            Some(&i) => merge(&mut primaries[i], candidate),
            None => {
                index.insert(candidate.native.lua_name.clone(), primaries.len());
                primaries.push(candidate);
            }
        }
    }

    let mut used_aliases: HashSet<String> = HashSet::new();
    let mut groups: Vec<Group> = primaries
        .into_iter()
        .map(|c| {
            let mut aliases = Vec::new();
            for source in &c.alias_sources {
                let lua = lua_name(source);
                // A primary always wins over an alias; the first alias wins over a later one.
                if lua == c.native.lua_name || index.contains_key(&lua) || !used_aliases.insert(lua.clone()) {
                    continue;
                }
                let hash_alias = source.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("0x"));
                aliases.push(Alias { lua, name: (!hash_alias).then(|| source.to_string()) });
            }
            let mut native = c.native;
            native.aliases = aliases.iter().map(|a| a.lua.clone()).collect();
            Group { native, aliases }
        })
        .collect();
    sort_groups(&mut groups);
    groups
}

pub fn cmp_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

pub fn sort_groups(groups: &mut [Group]) {
    groups.sort_by(|a, b| {
        let (a, b) = (&a.native, &b.native);
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| cmp_ignore_case(&a.lua_name, &b.lua_name))
            .then_with(|| a.lua_name.cmp(&b.lua_name))
    });
}

pub fn flatten(groups: Vec<Group>) -> Vec<Native> {
    let mut out = Vec::with_capacity(groups.iter().map(|g| 1 + g.aliases.len()).sum());
    for g in groups {
        let primary = g.native;
        let aliases: Vec<Native> = g
            .aliases
            .into_iter()
            .map(|a| Native {
                set: primary.set,
                lua_name: a.lua,
                original_name: a.name,
                hash: primary.hash.clone(),
                namespace: primary.namespace.clone(),
                apiset: primary.apiset,
                game: primary.game,
                rpc: primary.rpc,
                params: primary.params.clone(),
                returns: primary.returns.clone(),
                description: primary.description.clone(),
                lua_example: primary.lua_example.clone(),
                doc_url: primary.doc_url.clone(),
                aliases: Vec::new(),
                alias_of: Some(primary.lua_name.clone()),
                alt_name: None,
            })
            .collect();
        out.push(primary);
        out.extend(aliases);
    }
    out
}

pub fn functions(groups: &[Group]) -> usize {
    groups.iter().map(|g| 1 + g.aliases.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::natives::parse::parse_json;

    pub const GTA_JSON: &str = r#"{
      "MISC": {
        "0xC906A7DAB05C8D2B": {
          "name": "GET_GROUND_Z_FOR_3D_COORD",
          "params": [
            {"name": "x", "type": "float", "description": "Position on the X-axis to get ground elevation."},
            {"name": "y", "type": "float", "description": "Position on the Y-axis to get ground elevation."},
            {"name": "z", "type": "float", "description": "Position on the Z-axis to get ground elevation."},
            {"name": "groundZ", "type": "float*", "description": "The Z coordinate for the ground."},
            {"name": "includeWater", "type": "BOOL", "description": "Determines if the top water level\nshould be considered the ground elevation."}
          ],
          "results": "BOOL",
          "description": "This native gets the ground level.\r\nSee [GET_ENTITY_COORDS](#\\_0x3FEF770D40960D5A).\n@not a tag\n\tindented with a tab",
          "examples": [
            {"lang": "js", "code": "const [retval, groundZ] = GetGroundZFor_3dCoord(x, y, z, true);"},
            {"lang": "lua", "code": "local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, true)   \r\n---@type number\n-- @also not a tag\n"}
          ],
          "hash": "0xC906A7DAB05C8D2B", "ns": "MISC", "jhash": "0xA1BFD5E0", "manualHash": true,
          "resultsDescription": "true if the ground elevation has been found"
        },
        "0x0B40ED49D7D6FF84": {
          "params": [{"name": "p0", "type": "Any", "description": ""}],
          "results": "void", "description": "", "examples": [],
          "hash": "0x0B40ED49D7D6FF84", "ns": "MISC", "manualHash": true
        }
      },
      "SHAPETEST": {
        "0x3D87450E15D98694": {
          "name": "GET_SHAPE_TEST_RESULT",
          "params": [
            {"name": "shapeTestHandle", "type": "int", "description": "A shape test handle."},
            {"name": "hit", "type": "BOOL*", "description": "Whether or not the shape test hit any collisions."},
            {"name": "endCoords", "type": "Vector3*", "description": "The resulting coordinates."},
            {"name": "surfaceNormal", "type": "Vector3*", "description": "The surface normal of the hit position."},
            {"name": "entityHit", "type": "Entity*", "description": "Any dynamic entity hit by the shape test."}
          ],
          "results": "int", "description": "Returns the result of a shape test.", "examples": [],
          "hash": "0x3D87450E15D98694", "ns": "SHAPETEST", "aliases": ["_GET_RAYCAST_RESULT", "0x3D87450E15D98694", "GET_ENTITY_COORDS"],
          "manualHash": true, "resultsDescription": "`0` invalid, `1` pending, `2` completed."
        }
      },
      "ENTITY": {
        "0xAE3CBE5BF394C9C9": {
          "name": "DELETE_ENTITY",
          "params": [{"name": "entity", "type": "Entity*", "description": "The entity to delete."}],
          "results": "void", "description": "Delete the specified entity.", "examples": [],
          "hash": "0xAE3CBE5BF394C9C9", "ns": "ENTITY", "manualHash": true
        },
        "0x3FEF770D40960D5A": {
          "name": "GET_ENTITY_COORDS",
          "params": [{"name": "entity", "type": "Entity"}, {"name": "alive", "type": "BOOL"}],
          "results": "Vector3", "description": "Gets the current coordinates for a specified entity.", "examples": [],
          "hash": "0x3FEF770D40960D5A", "ns": "ENTITY", "manualHash": true
        }
      },
      "HUD": {
        "0x63145D9C883A1A70": {
          "name": "SET_TEXT_WRAP",
          "params": [{"name": "start", "type": "float"}, {"name": "end", "type": "float"}, {"name": "start", "type": "float"}],
          "results": "void", "description": "", "examples": [],
          "hash": "0x63145D9C883A1A70", "ns": "HUD", "manualHash": true
        }
      },
      "PED": {
        "0x76BBA2CEE66D47E9": {
          "name": "_GET_PED_EYE_COLOR",
          "params": [{"name": "ped", "type": "Ped"}],
          "results": "int", "description": "Game native description.", "examples": [],
          "hash": "0x76BBA2CEE66D47E9", "ns": "PED", "aliases": ["GET_PED_EYE_COLOUR"], "manualHash": true
        }
      }
    }"#;

    pub const CFX_JSON: &str = r#"{
      "CFX": {
        "0x5FA79B0F": {
          "name": "REGISTER_COMMAND",
          "params": [
            {"name": "commandName", "type": "char*", "description": "The command you want to register."},
            {"name": "handler", "type": "func", "description": "A handler function."},
            {"name": "restricted", "type": "BOOL", "description": "Requires an ace permission."}
          ],
          "results": "void", "description": "Registers a command.", "examples": [],
          "hash": "0x5FA79B0F", "ns": "CFX", "apiset": "shared"
        },
        "0x774A4C54": {
          "name": "SET_PLAYER_MODEL",
          "params": [
            {"name": "player", "type": "Player", "description": "The player to set the model for"},
            {"name": "model", "type": "Hash", "description": "The model to use"}
          ],
          "results": "void",
          "description": "Set the model.\n\n**This is the server-side RPC native equivalent of the client native [SET_PLAYER_MODEL](?\\_0x00A1CADD00108836).**",
          "examples": [], "hash": "0x774A4C54", "ns": "CFX", "apiset": "server"
        },
        "0xA61C8FC6": {
          "name": "GET_PLAYER_IDENTIFIER_BY_TYPE",
          "params": [
            {"name": "playerSrc", "type": "char*", "description": "The player to get the identifier for"},
            {"name": "identifierType", "type": "char*", "description": "The string to match"}
          ],
          "results": "char*", "description": "Get an identifier.", "examples": [{"lang": "lua", "code": "GetPlayerIdentifierByType(source, 'license')"}],
          "hash": "0xA61C8FC6", "ns": "CFX", "apiset": "server", "resultsDescription": "The identifier"
        },
        "0xCF143FB9": {
          "name": "GET_ACTIVE_PLAYERS",
          "params": [], "results": "object", "description": "Returns all player indices.", "examples": [],
          "hash": "0xCF143FB9", "ns": "CFX", "apiset": "client"
        },
        "0xA47B860F": {
          "name": "GET_PED_EYE_COLOR",
          "params": [{"name": "ped", "type": "Ped", "description": "The target ped"}],
          "results": "int", "description": "", "examples": [],
          "hash": "0xA47B860F", "ns": "CFX", "apiset": "client", "game": "gta5", "aliases": ["_GET_PED_EYE_COLOUR"]
        },
        "0xD8B5E8C1": {
          "name": "SET_CURSOR_LOCATION",
          "params": [{"name": "x", "type": "float"}, {"name": "y", "type": "float"}],
          "results": "BOOL", "description": "RedM only.", "examples": [],
          "hash": "0xD8B5E8C1", "ns": "CFX", "apiset": "client", "game": "rdr3"
        },
        "0x1647F1CB": {
          "name": "GET_ENTITY_COORDS",
          "params": [{"name": "entity", "type": "Entity"}],
          "results": "Vector3", "description": "Server version.", "examples": [],
          "hash": "0x1647F1CB", "ns": "CFX", "apiset": "server"
        },
        "0x00000001": {
          "name": "FORMAT_ARGS",
          "params": [{"name": "fmt", "type": "char*"}, {"name": "", "type": ""}],
          "results": "void", "description": "", "examples": [],
          "hash": "0x00000001", "ns": "CFX", "apiset": "shared", "game": "ny"
        }
      }
    }"#;

    fn groups(set: NativeSet, gta: bool) -> Vec<Group> {
        let mut raw = if gta { parse_json(GTA_JSON, Origin::Gta5Json).unwrap() } else { Vec::new() };
        raw.extend(parse_json(CFX_JSON, Origin::CfxJson).unwrap());
        convert(set, raw, &Overrides::bundled())
    }

    fn native<'a>(natives: &'a [Native], lua: &str) -> &'a Native {
        natives.iter().find(|n| n.lua_name == lua).unwrap_or_else(|| panic!("no {lua}"))
    }

    fn names(natives: &[Native]) -> Vec<&str> {
        natives.iter().map(|n| n.lua_name.as_str()).collect()
    }

    #[test]
    fn lua_names_follow_codegen_out_lua() {
        for (native, lua) in [
            ("GET_PLAYER_PED", "GetPlayerPed"),
            ("GET_GROUND_Z_FOR_3D_COORD", "GetGroundZFor_3dCoord"),
            ("DRAW_DEBUG_TEXT_2D", "DrawDebugText_2d"),
            ("_GET_ENTITY_BONE_POSITION_2", "GetEntityBonePosition_2"),
            ("_DYNAMIC_MIXER_RELATED_FN", "DynamicMixerRelatedFn"),
            ("0x0B40ED49D7D6FF84", "N_0x0b40ed49d7d6ff84"),
            ("_0x53E4D0C079CA6855", "N_0x53e4d0c079ca6855"),
            ("NETWORK_IS_SESSION_ACTIVE", "NetworkIsSessionActive"),
            ("SET_TEXT_SCALE", "SetTextScale"),
            ("_SET_PED_EYE_COLOR", "SetPedEyeColor"),
            ("A__B", "A_B"),
            ("_2_", "_2_"),
            ("X", "x"),
        ] {
            assert_eq!(lua_name(native), lua, "{native}");
        }
    }

    #[test]
    fn parameter_names_are_valid_identifiers() {
        for (raw, index, name) in [
            ("end", 0, "end_"),
            ("repeat", 0, "repeat_"),
            ("local", 0, "local_"),
            ("function", 0, "function_"),
            ("", 2, "p2"),
            ("__", 1, "p1"),
            ("1st", 0, "_1st"),
            ("a-b", 0, "a_b"),
            (" ped ", 0, "ped"),
        ] {
            assert_eq!(param_name(raw, index), name, "{raw:?}");
        }
    }

    #[test]
    fn out_pointers_are_returned_after_the_result() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let n = native(&gta5, "GetGroundZFor_3dCoord");
        let lua: Vec<&str> = n.lua_params().map(|p| p.name.as_str()).collect();
        assert_eq!(lua, ["x", "y", "z", "includeWater"]);
        assert!(n.params[3].out_pointer);
        assert_eq!(n.params[3].native_type, "float*");
        let rets: Vec<(&str, &str)> = n.returns.iter().map(|r| (r.name.as_str(), r.lua_type.as_str())).collect();
        assert_eq!(rets, [("retval", "boolean"), ("groundZ", "number")]);
        assert_eq!(n.returns[0].native_type, "BOOL");
        assert_eq!(n.returns[0].description.as_deref(), Some("true if the ground elevation has been found"));
        assert_eq!(n.returns[1].description.as_deref(), Some("The Z coordinate for the ground."));

        let shape = native(&gta5, "GetShapeTestResult");
        assert_eq!(shape.lua_params().count(), 1);
        let types: Vec<&str> = shape.returns.iter().map(|r| r.lua_type.as_str()).collect();
        assert_eq!(types, ["integer", "integer", "vector3", "vector3", "integer"]);
        let rets: Vec<&str> = shape.returns.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(rets, ["retval", "hit", "endCoords", "surfaceNormal", "entityHit"]);
    }

    #[test]
    fn single_trailing_pointer_stays_in_the_call() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let n = native(&gta5, "DeleteEntity");
        let entity = &n.params[0];
        assert!(entity.kept_trailing_pointer && !entity.out_pointer);
        assert_eq!(entity.lua_type, "integer|nil");
        assert_eq!(n.returns.len(), 1);
        assert_eq!((n.returns[0].name.as_str(), n.returns[0].lua_type.as_str()), ("entity", "integer"));
        assert_eq!(n.call(), "DeleteEntity(entity)");
    }

    #[test]
    fn parameter_names_are_unique_and_unreserved() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        assert_eq!(native(&gta5, "SetTextWrap").call(), "SetTextWrap(start, end_, start_2)");
        let unnamed = native(&gta5, "N_0x0b40ed49d7d6ff84");
        assert_eq!((unnamed.original_name.as_deref(), unnamed.params[0].lua_type.as_str()), (None, "any"));
        let server = flatten(groups(NativeSet::Server, false));
        let format = native(&server, "FormatArgs");
        assert_eq!(format.call(), "FormatArgs(fmt, ...)", "the vararg is not a named parameter");
        assert!(format.params[1].is_vararg());
        assert_eq!(format.params[1].native_type, "...");
    }

    #[test]
    fn aliases_are_deprecated_copies() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let primary = native(&gta5, "GetShapeTestResult");
        assert_eq!(primary.aliases, ["GetRaycastResult", "N_0x3d87450e15d98694"]);
        let alias = native(&gta5, "GetRaycastResult");
        assert_eq!(alias.alias_of.as_deref(), Some("GetShapeTestResult"));
        assert_eq!(alias.original_name.as_deref(), Some("_GET_RAYCAST_RESULT"));
        assert_eq!((alias.returns.clone(), alias.hash.as_str()), (primary.returns.clone(), "0x3D87450E15D98694"));
        assert!(alias.aliases.is_empty() && alias.alt_name.is_none());
        assert_eq!(native(&gta5, "N_0x3d87450e15d98694").original_name, None);
        // Aliases follow their primary.
        let at = gta5.iter().position(|n| n.lua_name == "GetShapeTestResult").unwrap();
        assert_eq!(names(&gta5[at..at + 3]), ["GetShapeTestResult", "GetRaycastResult", "N_0x3d87450e15d98694"]);
    }

    #[test]
    fn primary_beats_alias_and_cfx_beats_game_native() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        assert_eq!(native(&gta5, "GetEntityCoords").original_name.as_deref(), Some("GET_ENTITY_COORDS"));
        let eye = native(&gta5, "GetPedEyeColor");
        assert_eq!(eye.hash, "0xA47B860F");
        assert_eq!(eye.description, "Game native description.");
        assert_eq!(eye.namespace, "CFX");
        assert_eq!(eye.aliases, ["GetPedEyeColour"], "aliases of both: the first one wins");
        assert_eq!(native(&gta5, "GetPedEyeColour").original_name.as_deref(), Some("_GET_PED_EYE_COLOUR"));
        assert_eq!(gta5.iter().filter(|n| n.lua_name == "GetPedEyeColor").count(), 1);
    }

    #[test]
    fn sets_follow_apiset_and_game() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let gta_names = names(&gta5);
        assert!(gta_names.contains(&"RegisterCommand") && gta_names.contains(&"GetActivePlayers"));
        assert!(!gta_names.contains(&"SetCursorLocation"), "RedM only");
        assert!(!gta_names.contains(&"GetPlayerIdentifierByType"), "server native");
        assert!(!gta_names.contains(&"FormatArgs"), "game 'ny'");
        let rdr3 = flatten(groups(NativeSet::Rdr3, false));
        assert_eq!(names(&rdr3), ["GetActivePlayers", "RegisterCommand", "SetCursorLocation"]);
        let server = flatten(groups(NativeSet::Server, false));
        assert_eq!(
            names(&server),
            ["FormatArgs", "GetEntityCoords", "GetPlayerIdentifierByType", "RegisterCommand", "SetPlayerModel"]
        );
    }

    #[test]
    fn type_mapping_and_overrides() {
        let server = flatten(groups(NativeSet::Server, false));
        let model = native(&server, "SetPlayerModel");
        let types: Vec<(&str, &str)> =
            model.lua_params().map(|p| (p.lua_type.as_str(), p.native_type.as_str())).collect();
        assert_eq!(types, [("string|integer", "Player"), ("integer|string", "Hash")]);
        assert!(model.rpc);
        assert_eq!(model.apiset, Side::Server);
        let id = native(&server, "GetPlayerIdentifierByType");
        let types: Vec<&str> = id.lua_params().map(|p| p.lua_type.as_str()).collect();
        assert_eq!(types, ["string|integer", "string"], "stringParamTypes");
        assert_eq!(id.returns[0].lua_type, "string");
        assert_eq!(id.lua_example.as_deref(), Some("GetPlayerIdentifierByType(source, 'license')"));
        let handler = &native(&server, "RegisterCommand").params[1];
        assert_eq!(
            (handler.lua_type.as_str(), handler.native_type.as_str()),
            ("fun(source:integer, args:string[], rawCommand:string)", "func")
        );

        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let coords = native(&gta5, "GetEntityCoords");
        let types: Vec<&str> = coords.lua_params().map(|p| p.lua_type.as_str()).collect();
        assert_eq!(types, ["integer", "boolean"]);
        assert_eq!(coords.returns[0].lua_type, "vector3");
        let players = &native(&gta5, "GetActivePlayers").returns[0];
        assert_eq!((players.lua_type.as_str(), players.native_type.as_str()), ("integer[]", "object"));
        assert_eq!(native(&gta5, "RegisterCommand").game, None);
        assert_eq!(native(&gta5, "GetPedEyeColor").game, Some(Game::Gta5));
    }

    #[test]
    fn documentation_is_normalized() {
        let gta5 = flatten(groups(NativeSet::Gta5, true));
        let n = native(&gta5, "GetGroundZFor_3dCoord");
        assert_eq!(
            n.description,
            "This native gets the ground level.\nSee [GET_ENTITY_COORDS](https://docs.fivem.net/natives/?_0x3FEF770D40960D5A).\n@not a tag\n\tindented with a tab"
        );
        assert_eq!(
            n.lua_example.as_deref(),
            Some("local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, true)\n---@type number\n-- @also not a tag")
        );
        assert_eq!(n.doc_url, "https://docs.fivem.net/natives/?_0xC906A7DAB05C8D2B");
        assert_eq!(
            n.params[4].description.as_deref(),
            Some("Determines if the top water level\nshould be considered the ground elevation.")
        );
        let server = flatten(groups(NativeSet::Server, false));
        assert!(
            native(&server, "SetPlayerModel")
                .description
                .contains("[SET_PLAYER_MODEL](https://docs.fivem.net/natives/?_0x00A1CADD00108836)")
        );
        assert_eq!(
            normalize_markdown("[a](#0x1) [b](#_0x) [c](?_0xZ) [d](#\\_0xAb1)]("),
            "[a](#0x1) [b](#_0x) [c](?_0xZ) [d](https://docs.fivem.net/natives/?_0xAb1)]("
        );
        assert_eq!(normalize_code("\n\nx  \r\n\r\ny\n\n"), "x\n\ny");
    }
}
