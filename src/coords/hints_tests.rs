use super::*;
use crate::coords::zones::tests::ZONES;
use crate::coords::{CoordsFeature, keys};
use crate::perf::ast;
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;

const FIVEM: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const REDM: &str = "fx_version 'cerulean'\ngame 'rdr3'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";
const ZONES_FILE: &str = "data/zones.json";

fn zoned() -> Settings {
    Settings::defaults().with(keys::ZONES_FILE, Value::from(ZONES_FILE))
}

fn hints_in(feature: &CoordsFeature, manifest: &str, settings: Settings, zones: &str, text: &str) -> Vec<Hint> {
    let fx =
        Fixture::new(&[("res/fxmanifest.lua", manifest), (CLIENT, text), (ZONES_FILE, zones)]).with_settings(settings);
    fx.index(feature);
    let doc = fx.doc(CLIENT);
    let cx = fx.cx(&doc);
    let tree = ast::parse(&doc.src);
    let zones = feature.zone_map(&cx);
    hints(&cx, &tree, zones.as_deref(), 0, text.len())
}

fn hints_of(manifest: &str, settings: Settings, text: &str) -> Vec<Hint> {
    hints_in(&CoordsFeature::default(), manifest, settings, ZONES, text)
}

/// The text without its `/*<# label #>*/` marks, as the inlay tests of IntelliJ write it.
fn unmarked(expected: &str) -> String {
    let mut out = String::new();
    let mut rest = expected;
    while let Some(open) = rest.find("/*<#") {
        out.push_str(&rest[..open]);
        let close = rest[open..].find("#>*/").map_or(rest.len(), |close| open + close + 4);
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

fn marked(text: &str, hints: &[Hint]) -> String {
    let mut out = text.to_owned();
    for hint in hints.iter().rev() {
        out.insert_str(hint.at, &format!("/*<# {} #>*/", hint.label));
    }
    out
}

const DAVIS: &str = "local davis = vector3(100.0, -1600.0, 30.0)";

#[test]
fn zone_names_are_hinted_after_literal_coordinates() {
    let expected = "local davis = vector3(100.0, -1600.0, 30.0)/*<# ≈ Davis #>*/\n\
                    Config.Spots = {\n    { coords = vec4(100.0, -1300.0, 30.0, 90.0)/*<# ≈ Strawberry #>*/ },\n    \
                    {x = 900.0, y = -900.0, z = 30.0}/*<# ≈ Los Santos #>*/,\n}\n\
                    local outside = vector3(5000.0, 5000.0, 30.0)\n\
                    local computed = vector3(x, -1600.0, 30.0)\n\
                    local list = {100.0, -1600.0, 30.0}\n\
                    local offset = vector3(100.0, -1600.0, 30.0)";
    let text = unmarked(expected);
    let hints = hints_of(FIVEM, zoned(), &text);
    assert!(hints.iter().all(|hint| hint.kind == HintKind::Value), "{hints:?}");
    assert_eq!(marked(&text, &hints), expected);
    let tooltips: Vec<&str> = hints.iter().filter_map(|hint| hint.tooltip.as_deref()).collect();
    assert_eq!(
        tooltips,
        [
            "Approximate GTA V zone: Davis (DAVIS), from bounding boxes",
            "Approximate GTA V zone: Strawberry (STRAW), from bounding boxes",
            "Approximate GTA V zone: Los Santos (LOSSANTOS), from bounding boxes",
        ]
    );
}

#[test]
fn no_zones_file_no_hints() {
    assert!(hints_of(FIVEM, Settings::defaults(), DAVIS).is_empty());
    let blank = Settings::defaults().with(keys::ZONES_FILE, Value::from("  "));
    assert!(hints_of(FIVEM, blank, DAVIS).is_empty());
    let missing = Settings::defaults().with(keys::ZONES_FILE, Value::from("data/other.json"));
    assert!(hints_of(FIVEM, missing, DAVIS).is_empty());
}

#[test]
fn redm_resources_get_no_gta_v_zones() {
    let absolute = || Settings::defaults().with(keys::ZONES_FILE, Value::from("/p/data/zones.json"));
    assert_eq!(hints_of(FIVEM, absolute(), DAVIS).len(), 1, "an absolute path is read too");
    assert!(hints_of(REDM, absolute(), DAVIS).is_empty());
    let redm = zoned().with(crate::settings::keys::GAME, Value::from("redm"));
    assert!(hints_of(FIVEM, redm, DAVIS).is_empty(), "the game setting wins over the manifest");
}

#[test]
fn the_zones_file_is_read_again_when_it_changes() {
    let feature = CoordsFeature::default();
    let label = |zones: &str| -> Vec<String> {
        hints_in(&feature, FIVEM, zoned(), zones, DAVIS).into_iter().map(|hint| hint.label).collect()
    };
    assert_eq!(label(ZONES), ["≈ Davis"]);
    assert_eq!(label(&ZONES.replace("\"Davis\"", "\"Davis Quartz\"")), ["≈ Davis Quartz"]);
    assert!(label("not json").is_empty());
}

#[test]
fn the_hint_follows_the_whole_expression() {
    let text = "SetEntityCoords(ped, vector3(100.0, -1600.0, 30.0), false)\n\
                TriggerEvent('spot', { x = 100.0, y = -1600.0, z = 30.0 })";
    assert_eq!(
        marked(text, &hints_of(FIVEM, zoned(), text)),
        "SetEntityCoords(ped, vector3(100.0, -1600.0, 30.0)/*<# ≈ Davis #>*/, false)\n\
         TriggerEvent('spot', { x = 100.0, y = -1600.0, z = 30.0 }/*<# ≈ Davis #>*/)"
    );
}

#[test]
fn only_the_coordinates_that_end_in_the_range_are_hinted() {
    let text = "local a = vector3(100.0, -1600.0, 30.0)\nlocal b = vector3(100.0, -1300.0, 30.0)\n\
                local c = vector3(900.0, -900.0, 30.0)";
    let fx = Fixture::new(&[("res/fxmanifest.lua", FIVEM), (CLIENT, text), (ZONES_FILE, ZONES)]).with_settings(zoned());
    let feature = CoordsFeature::default();
    fx.index(&feature);
    let doc = fx.doc(CLIENT);
    let cx = fx.cx(&doc);
    let tree = ast::parse(&doc.src);
    let zones = feature.zone_map(&cx);
    let second = text.find("local b").unwrap_or_default();
    let end = text.find("\nlocal c").unwrap_or_default();
    let at = |from: usize, to: usize| -> Vec<(usize, String)> {
        hints(&cx, &tree, zones.as_deref(), from, to).into_iter().map(|hint| (hint.at, hint.label)).collect()
    };
    assert_eq!(at(second, end), [(end, "≈ Strawberry".to_owned())]);
    assert_eq!(at(second + 20, end), [(end, "≈ Strawberry".to_owned())], "the call starts before the range");
    assert!(at(second, end - 1).is_empty(), "the call ends after the range");
    assert!(at(end + 1, end + 5).is_empty());
    assert_eq!(at(0, text.len()).len(), 3);
}
