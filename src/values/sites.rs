//! Which native argument means what: the roles of a native's parameters
//! (from their names and native types in the catalog, or a curated list of
//! common natives without catalog), and the decodable integer literals.

use crate::feature::Ctx;
use crate::lua::Source;
use crate::natives::{Native, NativeSet};
use crate::perf::ast::{Ast, ExprKind, Node};
use crate::project::Game;
use std::collections::HashMap;
use std::sync::OnceLock;

/// What an integer argument of a native means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Control index; RDR3 controls are hashes of the control names.
    Control,
    BlipSprite,
    BlipColour,
    MarkerType,
    HudColour,
    ExplosionType,
    /// `TASK_PLAY_ANIM` flags: a bit mask.
    AnimFlags,
    /// A joaat hash: a `Hash` parameter, or any large integer literal.
    Hash,
}

impl Kind {
    /// The setting turning its hints on and off.
    pub fn setting(self) -> &'static str {
        match self {
            Kind::Control => super::keys::CONTROLS,
            Kind::BlipSprite | Kind::BlipColour => super::keys::BLIPS,
            Kind::MarkerType => super::keys::MARKERS,
            Kind::HudColour => super::keys::HUD_COLOURS,
            Kind::ExplosionType => super::keys::EXPLOSIONS,
            Kind::AnimFlags => super::keys::ANIM_FLAGS,
            Kind::Hash => super::keys::HASHES,
        }
    }
}

/// Red, green, blue (and alpha) arguments starting at `red`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    pub red: usize,
    pub alpha: bool,
    pub floats: bool,
}

impl Colour {
    pub fn last(&self) -> usize {
        self.red + if self.alpha { 3 } else { 2 }
    }
}

/// Meaning of the Lua arguments of one native.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roles {
    pub kinds: Vec<(usize, Kind)>,
    pub colours: Vec<Colour>,
}

impl Roles {
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.colours.is_empty()
    }

    pub fn kind(&self, index: usize) -> Option<Kind> {
        self.kinds.iter().find(|(i, _)| *i == index).map(|(_, k)| *k)
    }
}

/// A call of a native whose arguments can be decoded.
pub struct ValueCall<'a> {
    pub call: usize,
    pub name: String,
    pub native: Option<&'a Native>,
    pub roles: Roles,
    pub game: Game,
}

impl ValueCall<'_> {
    /// Name of the Lua parameter at `index` in the catalog.
    pub fn param(&self, index: usize) -> Option<&str> {
        self.native?.lua_params().nth(index).map(|p| p.name.as_str())
    }
}

/// An integer literal (with its sign) and what it means.
pub struct Site<'a> {
    /// The literal, or `-literal`.
    pub expr: usize,
    pub value: i64,
    pub kind: Kind,
    pub game: Game,
    pub call: Option<ValueCall<'a>>,
    /// Argument index in the call (`None` when not an argument).
    pub index: Option<usize>,
}

/// Integer literals below this magnitude are hashes only when passed to a `Hash` parameter.
pub const MIN_FREE_HASH: i64 = 0x10000;

/// What the decoder needs from the request.
pub struct Scope<'a> {
    pub cx: &'a Ctx<'a>,
    pub ast: &'a Ast,
    pub sets: Vec<NativeSet>,
    /// RedM when the project only targets RDR3.
    pub project_game: Game,
}

impl<'a> Scope<'a> {
    pub fn new(cx: &'a Ctx<'a>, ast: &'a Ast) -> Self {
        let detected = cx.ws.detected_games();
        let games = cx.ws.resource_of(&cx.doc.path).and_then(|r| r.games.clone());
        let sets = crate::natives::visible_sets(cx.settings, &detected, cx.doc.side, games.as_deref());
        let project = crate::natives::project_games(cx.settings, &detected);
        let project_game = if project == [Game::Rdr3] { Game::Rdr3 } else { Game::Gta5 };
        Self { cx, ast, sets, project_game }
    }

    pub fn src(&self) -> &'a Source {
        &self.cx.doc.src
    }

    /// The native call `call` when one of its arguments can be decoded.
    pub fn call_at(&self, call: usize) -> Option<ValueCall<'a>> {
        let ExprKind::Call { callee, method: None, .. } = self.ast.exprs[call].kind else { return None };
        let ExprKind::Name { name, decl: None } = &self.ast.exprs[callee].kind else { return None };
        if !name.starts_with(|c: char| c.is_uppercase()) {
            return None;
        }
        let native = self.cx.natives.find(name, &self.sets);
        let roles = match native {
            Some(n) => roles_of(n),
            None => fallback().get(name.as_str())?.clone(),
        };
        if roles.is_empty() {
            return None;
        }
        let game = match native.map(|n| n.set) {
            Some(NativeSet::Gta5) => Game::Gta5,
            Some(NativeSet::Rdr3) => Game::Rdr3,
            _ => self.project_game,
        };
        Some(ValueCall { call, name: name.clone(), native, roles, game })
    }

    /// The decodable integer of number literal `lit`.
    pub fn site_at(&self, lit: usize) -> Option<Site<'a>> {
        let ast = self.ast;
        if !ast.is_number(lit) {
            return None;
        }
        let raw = integer_value(ast.text(self.src(), Node::Expr(lit)))?;
        let (expr, value) = match ast.parent(Node::Expr(lit)) {
            Some(Node::Expr(u)) if matches!(ast.exprs[u].kind, ExprKind::Unary { op: "-", .. }) => {
                (u, raw.wrapping_neg())
            }
            _ => (lit, raw),
        };
        let call = match ast.parent(Node::Expr(expr)) {
            Some(Node::Expr(c)) if ast.call_args(c).contains(&expr) => self.call_at(c),
            _ => None,
        };
        let mut index = None;
        if let Some(c) = &call {
            let i = ast.call_args(c.call).iter().position(|&a| a == expr)?;
            index = Some(i);
            if let Some(kind) = c.roles.kind(i) {
                let game = c.game;
                return Some(Site { expr, value, kind, game, call, index });
            }
            if c.roles.colours.iter().any(|col| (col.red..=col.last()).contains(&i)) {
                return None;
            }
        }
        if !(-0x8000_0000..=0xFFFF_FFFF).contains(&value) || (-MIN_FREE_HASH < value && value < MIN_FREE_HASH) {
            return None;
        }
        let game = call.as_ref().map_or(self.project_game, |c| c.game);
        Some(Site { expr, value, kind: Kind::Hash, game, call, index })
    }
}

/// Value of an integer literal (`38`, `0xB779A091`), `None` for floats and malformed numbers.
pub fn integer_value(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Some(digits) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if digits.is_empty() || digits.len() > 16 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return u64::from_str_radix(digits, 16).ok().map(|v| v as i64);
    }
    if t.is_empty() || t.len() > 18 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

/// Meaning of the parameters of a catalog native, from their names and native types.
pub fn roles_of(native: &Native) -> Roles {
    let params: Vec<_> = native.lua_params().collect();
    let lower_native = native.lua_name.to_ascii_lowercase();
    // RDR3 PAD natives: `control` is the pad index, `action` the control hash.
    let action_hash = native.namespace == "PAD" && params.iter().any(|p| p.name == "action" && p.native_type == "Hash");
    let mut kinds = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let name = p.name.to_ascii_lowercase();
        let ty = p.native_type.as_str();
        let int = ty == "int";
        let control = if action_hash {
            name == "action"
        } else {
            int && name == "control" && (native.namespace == "PAD" || lower_native.contains("control"))
        };
        let kind = if control {
            Some(Kind::Control)
        } else if int && lower_native.contains("blip") && ["spriteid", "blipsprite", "sprite"].contains(&name.as_str())
        {
            Some(Kind::BlipSprite)
        } else if int
            && lower_native.contains("blip")
            && lower_native.contains("colo")
            && ["color", "colour"].contains(&name.as_str())
        {
            Some(Kind::BlipColour)
        } else if int && lower_native.contains("marker") && name == "type" {
            Some(Kind::MarkerType)
        } else if int && is_hud_colour_param(&name) {
            Some(Kind::HudColour)
        } else if int && name == "explosiontype" {
            Some(Kind::ExplosionType)
        } else if native.set != NativeSet::Rdr3
            && ["TaskPlayAnim", "TaskPlayAnimAdvanced"].contains(&native.lua_name.as_str())
            && ["flag", "flags"].contains(&name.as_str())
            && (int || ty == "Any")
        {
            Some(Kind::AnimFlags)
        } else if ty == "Hash" {
            Some(Kind::Hash)
        } else {
            None
        };
        if let Some(k) = kind {
            kinds.push((i, k));
        }
    }
    let mut colours = Vec::new();
    let mut i = 0;
    while i + 2 < params.len() {
        let (r, g, b) = (params[i], params[i + 1], params[i + 2]);
        let ty = r.native_type.as_str();
        if (ty == "int" || ty == "float")
            && g.native_type == ty
            && b.native_type == ty
            && channel(&r.name, 'r', "red")
            && channel(&g.name, 'g', "green")
            && channel(&b.name, 'b', "blue")
        {
            let alpha = params
                .get(i + 3)
                .is_some_and(|a| a.native_type == ty && ["a", "alpha", "opacity"].contains(&trimmed(&a.name).as_str()));
            colours.push(Colour { red: i, alpha, floats: ty == "float" });
            i += if alpha { 4 } else { 3 };
        } else {
            i += 1;
        }
    }
    Roles { kinds, colours }
}

/// `^hudcolou?r(index|id)?[0-9]?$`
fn is_hud_colour_param(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("hudcolour").or_else(|| name.strip_prefix("hudcolor")) else { return false };
    let rest = rest.strip_prefix("index").or_else(|| rest.strip_prefix("id")).unwrap_or(rest);
    rest.is_empty() || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
}

fn trimmed(name: &str) -> String {
    name.to_ascii_lowercase().trim_end_matches(|c: char| c.is_ascii_digit()).to_owned()
}

/// `r`, `red`, `colorR`, `colourR`, `colR`, `r1`: a colour channel parameter name.
fn channel(name: &str, letter: char, word: &str) -> bool {
    let n = trimmed(name);
    let l = letter.to_string();
    n == l || n == word || n == format!("color{l}") || n == format!("colour{l}") || n == format!("col{l}")
}

/// Curated roles of the most used GTA V natives, used without catalog entry.
fn fallback() -> &'static HashMap<&'static str, Roles> {
    static FALLBACK: OnceLock<HashMap<&'static str, Roles>> = OnceLock::new();
    FALLBACK.get_or_init(|| {
        let mut map: HashMap<&'static str, Roles> = HashMap::new();
        let mut kind = |kind: Kind, index: usize, names: &[&'static str]| {
            for n in names {
                map.entry(n).or_default().kinds.push((index, kind));
            }
        };
        kind(
            Kind::Control,
            1,
            &[
                "IsControlJustPressed",
                "IsControlPressed",
                "IsControlJustReleased",
                "IsControlReleased",
                "IsDisabledControlJustPressed",
                "IsDisabledControlPressed",
                "IsDisabledControlJustReleased",
                "IsDisabledControlReleased",
                "DisableControlAction",
                "EnableControlAction",
                "IsControlEnabled",
                "GetControlNormal",
                "GetDisabledControlNormal",
                "GetControlUnboundNormal",
                "GetDisabledControlUnboundNormal",
                "GetControlValue",
                "SetControlNormal",
                "SetInputExclusive",
                "GetControlInstructionalButton",
            ],
        );
        kind(Kind::BlipSprite, 1, &["SetBlipSprite"]);
        kind(Kind::BlipColour, 1, &["SetBlipColour", "SetBlipRouteColour"]);
        kind(Kind::MarkerType, 0, &["DrawMarker"]);
        kind(
            Kind::HudColour,
            0,
            &["GetHudColour", "ReplaceHudColour", "ReplaceHudColourWithRgba", "FlashMinimapDisplayWithColor"],
        );
        kind(Kind::HudColour, 1, &["ReplaceHudColour"]);
        kind(Kind::ExplosionType, 3, &["AddExplosion"]);
        kind(Kind::ExplosionType, 4, &["AddOwnedExplosion"]);
        kind(
            Kind::ExplosionType,
            0,
            &["IsExplosionInArea", "IsExplosionInSphere", "IsExplosionActiveInArea", "IsExplosionInAngledArea"],
        );
        kind(Kind::AnimFlags, 6, &["TaskPlayAnim"]);
        kind(Kind::AnimFlags, 12, &["TaskPlayAnimAdvanced"]);
        kind(
            Kind::Hash,
            0,
            &[
                "RequestModel",
                "HasModelLoaded",
                "IsModelValid",
                "IsModelInCdimage",
                "IsModelAVehicle",
                "SetModelAsNoLongerNeeded",
                "CreateVehicle",
                "CreateObject",
                "CreateObjectNoOffset",
            ],
        );
        kind(
            Kind::Hash,
            1,
            &[
                "CreatePed",
                "GiveWeaponToPed",
                "SetCurrentPedWeapon",
                "HasPedGotWeapon",
                "RemoveWeaponFromPed",
                "GiveWeaponComponentToPed",
            ],
        );
        let mut colour = |red: usize, alpha: bool, names: &[&'static str]| {
            for n in names {
                map.entry(n).or_default().colours.push(Colour { red, alpha, floats: false });
            }
        };
        colour(13, true, &["DrawMarker"]);
        colour(4, true, &["DrawRect"]);
        colour(6, true, &["DrawLine", "DrawBox"]);
        colour(6, false, &["DrawSpotLight"]);
        colour(9, true, &["DrawPoly"]);
        colour(7, true, &["DrawSprite"]);
        colour(0, true, &["SetTextColour"]);
        colour(
            1,
            true,
            &[
                "SetTextDropshadow",
                "SetTextEdge",
                "SetCheckpointRgba",
                "SetCheckpointRgba2",
                "ReplaceHudColourWithRgba",
            ],
        );
        colour(3, false, &["DrawLightWithRange"]);
        colour(8, true, &["CreateCheckpoint"]);
        colour(
            1,
            false,
            &[
                "SetVehicleCustomPrimaryColour",
                "SetVehicleCustomSecondaryColour",
                "SetVehicleTyreSmokeColor",
                "SetVehicleNeonLightsColour",
            ],
        );
        map
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::natives::Param;
    use crate::project::Side;
    use crate::testing::native;

    pub fn param(name: &str, native_type: &str) -> Param {
        Param { name: name.into(), lua_type: "number".into(), native_type: native_type.into(), ..Param::default() }
    }

    pub fn with_params(set: NativeSet, name: &str, namespace: &str, params: &[(&str, &str)]) -> Native {
        let mut n = native(set, name, Side::Client);
        n.namespace = namespace.into();
        n.params = params.iter().map(|(p, t)| param(p, t)).collect();
        n
    }

    #[test]
    fn roles_from_the_catalog() {
        let roles = |n: &Native| roles_of(n);
        let pressed =
            with_params(NativeSet::Gta5, "IsControlJustPressed", "PAD", &[("padIndex", "int"), ("control", "int")]);
        assert_eq!(roles(&pressed).kinds, [(1, Kind::Control)]);
        let rdr =
            with_params(NativeSet::Rdr3, "IsControlJustPressed", "PAD", &[("control", "int"), ("action", "Hash")]);
        assert_eq!(roles(&rdr).kinds, [(1, Kind::Control)]);
        let anim = [
            ("ped", "Ped"),
            ("animDictionary", "char*"),
            ("animationName", "char*"),
            ("blendInSpeed", "float"),
            ("blendOutSpeed", "float"),
            ("duration", "int"),
            ("flag", "int"),
            ("playbackRate", "float"),
        ];
        assert_eq!(roles(&with_params(NativeSet::Gta5, "TaskPlayAnim", "TASK", &anim)).kinds, [(6, Kind::AnimFlags)]);
        assert_eq!(roles(&with_params(NativeSet::Rdr3, "TaskPlayAnim", "TASK", &anim)).kinds, []);
        let vehicle =
            with_params(NativeSet::Gta5, "CreateVehicle", "VEHICLE", &[("modelHash", "Hash"), ("x", "float")]);
        assert_eq!(roles(&vehicle).kinds, [(0, Kind::Hash)]);
        let sprite = with_params(NativeSet::Gta5, "SetBlipSprite", "HUD", &[("blip", "Blip"), ("spriteId", "int")]);
        assert_eq!(roles(&sprite).kinds, [(1, Kind::BlipSprite)]);
        let colour = with_params(NativeSet::Gta5, "SetBlipColour", "HUD", &[("blip", "Blip"), ("color", "int")]);
        assert_eq!(roles(&colour).kinds, [(1, Kind::BlipColour)]);
        let explosion = with_params(
            NativeSet::Gta5,
            "AddExplosion",
            "FIRE",
            &[("x", "float"), ("y", "float"), ("z", "float"), ("explosionType", "int")],
        );
        assert_eq!(roles(&explosion).kinds, [(3, Kind::ExplosionType)]);
        let mut hud = with_params(
            NativeSet::Gta5,
            "GetHudColour",
            "HUD",
            &[("hudColorIndex", "int"), ("r", "int"), ("g", "int"), ("b", "int"), ("a", "int")],
        );
        for p in &mut hud.params[1..] {
            p.out_pointer = true;
        }
        assert_eq!(roles(&hud), Roles { kinds: vec![(0, Kind::HudColour)], colours: vec![] });
        let mut marker: Vec<(&str, &str)> = vec![("type", "int")];
        marker.extend(
            ["posX", "posY", "posZ", "dirX", "dirY", "dirZ", "rotX", "rotY", "rotZ", "scaleX", "scaleY", "scaleZ"]
                .map(|n| (n, "float")),
        );
        marker.extend([("red", "int"), ("green", "int"), ("blue", "int"), ("alpha", "int"), ("bobUpAndDown", "BOOL")]);
        let draw = roles(&with_params(NativeSet::Gta5, "DrawMarker", "GRAPHICS", &marker));
        assert_eq!(draw.kinds, [(0, Kind::MarkerType)]);
        assert_eq!(draw.colours, [Colour { red: 13, alpha: true, floats: false }]);
        let ptfx = [("ptfxHandle", "int"), ("r", "float"), ("g", "float"), ("b", "float"), ("bLocalOnly", "BOOL")];
        assert_eq!(
            roles(&with_params(NativeSet::Gta5, "SetParticleFxLoopedColour", "GRAPHICS", &ptfx)).colours,
            [Colour { red: 1, alpha: false, floats: true }]
        );
    }

    #[test]
    fn integers() {
        assert_eq!(integer_value("38"), Some(38));
        assert_eq!(integer_value("0xB779A091"), Some(3_078_201_489));
        assert_eq!(integer_value("38.0"), None);
        assert_eq!(integer_value("1e3"), None);
        assert_eq!(integer_value("0x"), None);
        assert!(
            is_hud_colour_param("hudcolorindex")
                && is_hud_colour_param("hudcolour2")
                && !is_hud_colour_param("hudcolourx")
        );
        assert_eq!(fallback()["ReplaceHudColour"].kinds, [(0, Kind::HudColour), (1, Kind::HudColour)]);
        assert!(fallback()["DrawRect"].kinds.is_empty());
    }
}
