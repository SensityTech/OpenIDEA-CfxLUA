//! The bundled tables of `data/values` (sources in the README there), read
//! once on first use. Every table is sorted by id; lookups are binary searches.

use crate::lua::joaat::joaat;
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

pub const CONTROLS_URL: &str = "https://docs.fivem.net/docs/game-references/controls/";
pub const BLIPS_URL: &str = "https://docs.fivem.net/docs/game-references/blips/";
pub const BLIP_COLOURS_URL: &str = "https://docs.fivem.net/docs/game-references/blips/#blip-colors";
pub const MARKERS_URL: &str = "https://docs.fivem.net/docs/game-references/markers/";
pub const HUD_COLOURS_URL: &str = "https://docs.fivem.net/docs/game-references/hud-colors/";

/// A GTA V control: `38` is `INPUT_PICKUP`, `E` on a keyboard, `LB` on a pad.
#[derive(Debug)]
pub struct Control {
    pub id: i64,
    pub name: String,
    pub keyboard: String,
    pub pad: String,
}

impl Control {
    /// `E / LB`, `LB`, or empty without documented binding.
    pub fn keys(&self) -> String {
        [self.keyboard.as_str(), self.pad.as_str()]
            .into_iter()
            .filter(|k| !k.is_empty())
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// A value of an enumeration (blip sprite, marker type, explosion type, animation flag bit).
#[derive(Debug)]
pub struct Named {
    pub id: i64,
    pub name: String,
}

#[derive(Debug)]
pub struct BlipColour {
    pub id: i64,
    pub name: String,
    pub hex: String,
}

#[derive(Debug)]
pub struct HudColour {
    pub id: i64,
    pub name: String,
    pub rgba: [u8; 4],
}

pub struct Tables {
    pub controls: Vec<Control>,
    /// RDR3 controls are joaat hashes of the GTA V names (`` `INPUT_ENTER` `` = `0xCEFD9220`).
    controls_by_hash: HashMap<i32, usize>,
    pub blip_sprites: Vec<Named>,
    pub blip_colours: Vec<BlipColour>,
    pub markers: Vec<Named>,
    pub hud_colours: Vec<HudColour>,
    pub explosions_gta5: Vec<Named>,
    pub explosions_rdr3: Vec<Named>,
    /// `eScriptedAnimFlags` bits, lowest first.
    pub anim_flags: Vec<Named>,
    /// Bundled hash names: hash → (name, category index).
    hash_names: HashMap<i32, (String, usize)>,
    categories: Vec<String>,
}

/// Binary search of a table sorted by id.
pub fn by_id<T>(table: &[T], id: i64, key: impl Fn(&T) -> i64) -> Option<&T> {
    table.binary_search_by_key(&id, key).ok().map(|i| &table[i])
}

impl Tables {
    pub fn get() -> &'static Tables {
        static TABLES: OnceLock<Tables> = OnceLock::new();
        TABLES.get_or_init(Tables::load)
    }

    fn load() -> Self {
        let mut controls: Vec<Control> =
            parse::<(i64, String, String, String)>(include_str!("../../data/values/controls.json"))
                .into_iter()
                .map(|(id, name, keyboard, pad)| Control { id, name, keyboard, pad })
                .collect();
        controls.sort_by_key(|c| c.id);
        let controls_by_hash = controls.iter().enumerate().map(|(i, c)| (joaat(&c.name), i)).collect();
        let mut blip_colours: Vec<BlipColour> =
            parse::<(i64, String, String)>(include_str!("../../data/values/blip-colours.json"))
                .into_iter()
                .map(|(id, name, hex)| BlipColour { id, name, hex })
                .collect();
        blip_colours.sort_by_key(|c| c.id);
        let mut hud_colours: Vec<HudColour> =
            parse::<(i64, String, u8, u8, u8, u8)>(include_str!("../../data/values/hud-colours.json"))
                .into_iter()
                .map(|(id, name, r, g, b, a)| HudColour { id, name, rgba: [r, g, b, a] })
                .collect();
        hud_colours.sort_by_key(|c| c.id);
        let mut explosions: HashMap<String, Vec<(i64, String)>> =
            serde_json::from_str(include_str!("../../data/values/explosions.json")).unwrap_or_default();
        let (hash_names, categories) = hash_names();
        Self {
            controls,
            controls_by_hash,
            blip_sprites: named(parse(include_str!("../../data/values/blip-sprites.json"))),
            blip_colours,
            markers: named(parse(include_str!("../../data/values/markers.json"))),
            hud_colours,
            explosions_gta5: named(explosions.remove("gta5").unwrap_or_default()),
            explosions_rdr3: named(explosions.remove("rdr3").unwrap_or_default()),
            anim_flags: named(parse(include_str!("../../data/values/anim-flags.json"))),
            hash_names,
            categories,
        }
    }

    pub fn control(&self, id: i64) -> Option<&Control> {
        by_id(&self.controls, id, |c| c.id)
    }

    pub fn control_by_hash(&self, hash: i32) -> Option<&Control> {
        self.controls_by_hash.get(&hash).map(|&i| &self.controls[i])
    }

    /// A bundled name of `hash` and its category (`vehicle`, `ped`, `weapon`, `weapon component`).
    pub fn hash_name(&self, hash: i32) -> Option<(&str, &str)> {
        self.hash_names.get(&hash).map(|(n, c)| (n.as_str(), self.categories.get(*c).map_or("", String::as_str)))
    }
}

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> Vec<T> {
    serde_json::from_str(json).unwrap_or_default()
}

fn named(rows: Vec<(i64, String)>) -> Vec<Named> {
    let mut out: Vec<Named> = rows.into_iter().map(|(id, name)| Named { id, name }).collect();
    out.sort_by_key(|n| n.id);
    out
}

/// `hash-names.txt.gz`: one name per line, `#category` header lines; the first name of a hash wins.
fn hash_names() -> (HashMap<i32, (String, usize)>, Vec<String>) {
    let mut text = String::new();
    let gz = include_bytes!("../../data/values/hash-names.txt.gz");
    if flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut text).is_err() {
        return Default::default();
    }
    let mut names = HashMap::with_capacity(4096);
    let mut categories: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(category) = line.strip_prefix('#') {
            categories.push(category.to_owned());
            continue;
        }
        names.entry(joaat(line)).or_insert_with(|| (line.to_owned(), categories.len().saturating_sub(1)));
    }
    (names, categories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_load() {
        let t = Tables::get();
        assert_eq!(t.controls.len(), 361);
        assert_eq!(t.control(38).map(|c| (c.name.as_str(), c.keys())), Some(("INPUT_PICKUP", "E / LB".to_owned())));
        assert_eq!(t.control(359).map(Control::keys), Some(String::new()));
        assert_eq!(t.control_by_hash(0xCEFD_9220_u32 as i32).map(|c| c.name.as_str()), Some("INPUT_ENTER"));
        assert_eq!(t.blip_sprites.len(), 966);
        assert_eq!(t.blip_colours.len(), 86);
        assert_eq!(t.markers.len(), 42);
        assert!(by_id(&t.markers, 41, |m| m.id).is_none());
        assert_eq!(t.hud_colours.len(), 212);
        assert_eq!(by_id(&t.hud_colours, 6, |h| h.id).map(|h| h.rgba), Some([224, 50, 50, 255]));
        assert_eq!(t.explosions_gta5.len(), 84);
        assert_eq!(t.explosions_rdr3.len(), 36);
        assert_eq!(t.anim_flags.len(), 31);
        assert_eq!(t.hash_name(-1216765807), Some(("adder", "vehicle")));
        assert_eq!(t.hash_name(joaat("weapon_pistol")).map(|h| h.1), Some("weapon"));
        assert_eq!(t.hash_names.len(), 2621);
    }
}
