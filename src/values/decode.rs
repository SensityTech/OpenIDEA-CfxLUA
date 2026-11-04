use super::sites::Kind;
use super::tables::{self, Tables, by_id};
use crate::project::Game;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Project,
    Bundled(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashName {
    pub name: String,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub kind: Kind,
    pub value: i64,
    pub label: String,
    pub name: String,
    pub title: &'static str,
    pub details: Vec<(String, String)>,
    pub doc_url: Option<&'static str>,
    pub hash_name: Option<HashName>,
}

const ANIM_FLAG_ALIASES: &[(i64, &str)] =
    &[(1, "REPEAT"), (2, "STOP_LAST_FRAME"), (16, "UPPERBODY"), (32, "ENABLE_PLAYER_CONTROL")];

fn in_hash_range(value: i64) -> bool {
    (-0x8000_0000..=0xFFFF_FFFF).contains(&value)
}

pub fn decode(kind: Kind, value: i64, game: Game, hash_name: &dyn Fn(i32) -> Option<HashName>) -> Option<Decoded> {
    let t = Tables::get();
    let enumerated = i32::try_from(value).ok().map(i64::from);
    let plain = |label: &str, title, details, doc_url| Decoded {
        kind,
        value,
        label: label.to_owned(),
        name: label.to_owned(),
        title,
        details,
        doc_url,
        hash_name: None,
    };
    match kind {
        Kind::Control => control(value, hash_name),
        Kind::BlipSprite => by_id(&t.blip_sprites, enumerated?, |n| n.id)
            .map(|b| plain(&b.name, "Blip sprite", Vec::new(), Some(tables::BLIPS_URL))),
        Kind::BlipColour => by_id(&t.blip_colours, enumerated?, |c| c.id).map(|c| {
            plain(&c.name, "Blip colour", vec![("Colour".into(), c.hex.clone())], Some(tables::BLIP_COLOURS_URL))
        }),
        Kind::MarkerType => by_id(&t.markers, enumerated?, |m| m.id)
            .map(|m| plain(&m.name, "Marker type", Vec::new(), Some(tables::MARKERS_URL))),
        Kind::HudColour => by_id(&t.hud_colours, enumerated?, |h| h.id).map(|h| {
            let [r, g, b, a] = h.rgba;
            plain(
                &h.name,
                "HUD colour",
                vec![("RGBA".into(), format!("{r}, {g}, {b}, {a}"))],
                Some(tables::HUD_COLOURS_URL),
            )
        }),
        Kind::ExplosionType => {
            let table = if game == Game::Rdr3 { &t.explosions_rdr3 } else { &t.explosions_gta5 };
            by_id(table, enumerated?, |e| e.id)
                .map(|e| plain(&e.name, "Explosion type", vec![("Enum".into(), format!("EXP_TAG_{}", e.name))], None))
        }
        Kind::AnimFlags => anim_flags(value),
        Kind::Hash => {
            if !in_hash_range(value) {
                return None;
            }
            let hash = value as i32;
            let name = hash_name(hash)?;
            let source = match &name.origin {
                Origin::Project => "a string of the project".to_owned(),
                Origin::Bundled(category) => format!("the bundled names ({category})"),
            };
            let mut details = vec![("Name from".to_owned(), source)];
            details.extend(hash_details(hash));
            Some(Decoded {
                kind,
                value,
                label: format!("`{}`", name.name),
                name: name.name.clone(),
                title: "Hash",
                details,
                doc_url: None,
                hash_name: Some(name),
            })
        }
    }
}

fn control(value: i64, hash_name: &dyn Fn(i32) -> Option<HashName>) -> Option<Decoded> {
    let t = Tables::get();
    if (0..=0xFFFF).contains(&value) {
        let c = t.control(value)?;
        let keys = c.keys();
        let mut details = Vec::new();
        if !c.keyboard.is_empty() {
            details.push(("Keyboard".to_owned(), c.keyboard.clone()));
        }
        if !c.pad.is_empty() {
            details.push(("Pad".to_owned(), c.pad.clone()));
        }
        let label = if keys.is_empty() { c.name.clone() } else { format!("{} ({keys})", c.name) };
        return Some(Decoded {
            kind: Kind::Control,
            value,
            label,
            name: c.name.clone(),
            title: "Control",
            details,
            doc_url: Some(tables::CONTROLS_URL),
            hash_name: None,
        });
    }
    if !in_hash_range(value) {
        return None;
    }
    // RDR3: the hash of the control name.
    let hash = value as i32;
    let name = t.control_by_hash(hash).map(|c| c.name.clone()).or_else(|| hash_name(hash).map(|h| h.name))?;
    Some(Decoded {
        kind: Kind::Control,
        value,
        label: name.clone(),
        name,
        title: "Control (hash)",
        details: hash_details(hash),
        doc_url: None,
        hash_name: None,
    })
}

fn anim_flags(value: i64) -> Option<Decoded> {
    if value <= 0 || value > 0xFFFF_FFFF {
        return None;
    }
    let mut set: Vec<(i64, String)> = Vec::new();
    let mut rest = value;
    for flag in &Tables::get().anim_flags {
        let bit = flag.id & 0xFFFF_FFFF;
        if bit != 0 && value & bit == bit {
            set.push((bit, flag.name.clone()));
            rest &= !bit;
        }
    }
    if rest != 0 {
        set.push((rest, format!("0x{rest:X}")));
    }
    let label = set.iter().map(|(_, n)| n.strip_prefix("AF_").unwrap_or(n)).collect::<Vec<_>>().join(" | ");
    let name = set.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" | ");
    let details = set
        .iter()
        .map(|(bit, n)| {
            let alias =
                ANIM_FLAG_ALIASES.iter().find(|(b, _)| b == bit).map(|(_, a)| format!(" ({a})")).unwrap_or_default();
            (bit.to_string(), format!("{n}{alias}"))
        })
        .collect();
    Some(Decoded {
        kind: Kind::AnimFlags,
        value,
        label,
        name,
        title: "Animation flags",
        details,
        doc_url: None,
        hash_name: None,
    })
}

fn hash_details(hash: i32) -> Vec<(String, String)> {
    vec![
        ("Signed".into(), hash.to_string()),
        ("Unsigned".into(), (hash as u32).to_string()),
        ("Hexadecimal".into(), crate::lua::joaat::hex(hash)),
    ]
}

pub fn argument(index: usize, param: Option<&str>, native: &str) -> String {
    match param {
        Some(p) => format!("Argument {} ({p}) of {native}", index + 1),
        None => format!("Argument {} of {native}", index + 1),
    }
}

impl Decoded {
    pub fn message(&self) -> String {
        let mut out = format!("{} {}: {}", self.title, self.value, self.label);
        if !self.details.is_empty() {
            let rows: Vec<String> = self.details.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            out.push_str(" — ");
            out.push_str(&rows.join(", "));
        }
        out
    }

    pub fn markdown(&self, expr: &str, argument: Option<&str>) -> String {
        let mut out = format!("**{}** `{expr}` = **{}**\n", self.title, self.name);
        if let Some(a) = argument {
            out.push_str(&format!("\n{a}\n"));
        }
        if !self.details.is_empty() {
            out.push('\n');
            for (k, v) in &self.details {
                out.push_str(&format!("- {k}: `{v}`\n"));
            }
        }
        if let Some(url) = self.doc_url {
            out.push_str(&format!("\n[Reference table]({url})\n"));
        }
        out
    }

    pub fn comment(&self) -> String {
        format!("--[[ {} ]]", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::joaat::joaat;

    fn bundled(hash: i32) -> Option<HashName> {
        Tables::get()
            .hash_name(hash)
            .map(|(n, c)| HashName { name: n.to_owned(), origin: Origin::Bundled(c.to_owned()) })
    }

    fn label(kind: Kind, value: i64, game: Game) -> Option<String> {
        decode(kind, value, game, &bundled).map(|d| d.label)
    }

    #[test]
    fn labels() {
        assert_eq!(label(Kind::Control, 38, Game::Gta5).as_deref(), Some("INPUT_PICKUP (E / LB)"));
        assert_eq!(label(Kind::Control, 0, Game::Gta5).as_deref(), Some("INPUT_NEXT_CAMERA (V / BACK)"));
        assert_eq!(label(Kind::Control, 359, Game::Gta5).as_deref(), Some("INPUT_RESPAWN_FASTER"));
        assert_eq!(label(Kind::Control, 5000, Game::Gta5), None);
        assert_eq!(label(Kind::Control, 0xCEFD_9220, Game::Rdr3).as_deref(), Some("INPUT_ENTER"));
        assert_eq!(label(Kind::BlipSprite, 1, Game::Gta5).as_deref(), Some("radar_level"));
        assert_eq!(label(Kind::BlipColour, 1, Game::Gta5).as_deref(), Some("Red"));
        assert_eq!(label(Kind::MarkerType, 1, Game::Gta5).as_deref(), Some("MarkerTypeVerticalCylinder"));
        assert_eq!(label(Kind::HudColour, 6, Game::Gta5).as_deref(), Some("HUD_COLOUR_RED"));
        assert_eq!(label(Kind::ExplosionType, 2, Game::Gta5).as_deref(), Some("STICKYBOMB"));
        assert_eq!(label(Kind::ExplosionType, 25, Game::Rdr3).as_deref(), Some("DYNAMITE"));
        assert_eq!(label(Kind::ExplosionType, -1, Game::Gta5), None);
        assert_eq!(label(Kind::AnimFlags, 49, Game::Gta5).as_deref(), Some("LOOPING | UPPERBODY | SECONDARY"));
        assert_eq!(label(Kind::AnimFlags, 1, Game::Gta5).as_deref(), Some("LOOPING"));
        assert_eq!(label(Kind::AnimFlags, 18, Game::Gta5).as_deref(), Some("HOLD_LAST_FRAME | UPPERBODY"));
        assert_eq!(label(Kind::AnimFlags, 0xC000_0000, Game::Gta5).as_deref(), Some("USE_FULL_BLENDING | 0x80000000"));
        assert_eq!(label(Kind::AnimFlags, 0, Game::Gta5), None);
        assert_eq!(label(Kind::Hash, -1216765807, Game::Gta5).as_deref(), Some("`adder`"));
        assert_eq!(label(Kind::Hash, 3_078_201_489, Game::Gta5).as_deref(), Some("`adder`"));
        assert_eq!(
            label(Kind::Hash, i64::from(joaat("weapon_pistol")), Game::Gta5).as_deref(),
            Some("`weapon_pistol`")
        );
        assert_eq!(label(Kind::Hash, 123_456_789, Game::Gta5), None);
    }

    #[test]
    fn details_messages_and_docs() {
        let d = decode(Kind::Control, 38, Game::Gta5, &bundled).unwrap();
        assert_eq!(d.message(), "Control 38: INPUT_PICKUP (E / LB) — Keyboard: E, Pad: LB");
        assert_eq!(
            d.markdown("38", Some(&argument(1, Some("control"), "IsControlJustPressed"))),
            "**Control** `38` = **INPUT_PICKUP**\n\nArgument 2 (control) of IsControlJustPressed\n\n- Keyboard: `E`\n- Pad: `LB`\n\n[Reference table](https://docs.fivem.net/docs/game-references/controls/)\n"
        );
        let h = decode(Kind::Hash, -1216765807, Game::Gta5, &bundled).unwrap();
        assert_eq!(
            h.message(),
            "Hash -1216765807: `adder` — Name from: the bundled names (vehicle), Signed: -1216765807, Unsigned: 3078201489, Hexadecimal: 0xB779A091"
        );
        assert_eq!(h.comment(), "--[[ adder ]]");
        let a = decode(Kind::AnimFlags, 49, Game::Gta5, &bundled).unwrap();
        assert_eq!(a.name, "AF_LOOPING | AF_UPPERBODY | AF_SECONDARY");
        assert_eq!(a.details[0], ("1".to_owned(), "AF_LOOPING (REPEAT)".to_owned()));
        assert_eq!(a.details[2], ("32".to_owned(), "AF_SECONDARY (ENABLE_PLAYER_CONTROL)".to_owned()));
        assert_eq!(argument(0, None, "DrawRect"), "Argument 1 of DrawRect");
        let hud = decode(Kind::HudColour, 6, Game::Gta5, &bundled).unwrap();
        assert_eq!(hud.details, [("RGBA".to_owned(), "224, 50, 50, 255".to_owned())]);
    }
}
