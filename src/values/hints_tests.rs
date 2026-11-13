use super::*;
use crate::lua::joaat::joaat;
use crate::natives::{Native, NativeSet};
use crate::settings::Settings;
use crate::testing::Fixture;
use crate::values::sites::tests::with_params;
use serde_json::Value;

const FIVEM: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const REDM: &str = "fx_version 'cerulean'\ngame 'rdr3'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";

struct Project<'a> {
    manifest: &'a str,
    natives: Vec<Native>,
    settings: Settings,
    extra: &'a [(&'a str, &'a str)],
}

fn gta5() -> Project<'static> {
    Project { manifest: FIVEM, natives: Vec::new(), settings: Settings::defaults(), extra: &[] }
}

fn with(key: &str, value: bool) -> Project<'static> {
    Project { settings: Settings::defaults().with(key, Value::from(value)), ..gta5() }
}

fn hints_in(project: Project, text: &str, from: usize, to: usize) -> Vec<Hint> {
    let mut files = vec![("res/fxmanifest.lua", project.manifest), (CLIENT, text)];
    files.extend_from_slice(project.extra);
    let fx = Fixture::new(&files).with_natives(project.natives).with_settings(project.settings);
    let feature = ValuesFeature::default();
    fx.index(&feature);
    let doc = fx.doc(CLIENT);
    hints(&feature, &fx.cx(&doc), from, to)
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

fn check_in(project: Project, expected: &str) -> Vec<Hint> {
    let text = unmarked(expected);
    let hints = hints_in(project, &text, 0, text.len());
    assert!(hints.iter().all(|hint| hint.kind == HintKind::Value && hint.tooltip.is_some()), "{hints:?}");
    assert_eq!(marked(&text, &hints), expected);
    hints
}

fn check(expected: &str) -> Vec<Hint> {
    check_in(gta5(), expected)
}

fn tooltips(hints: &[Hint]) -> Vec<&str> {
    hints.iter().filter_map(|hint| hint.tooltip.as_deref()).collect()
}

fn labels(project: Project, text: &str) -> Vec<String> {
    hints_in(project, text, 0, text.len()).into_iter().map(|hint| hint.label).collect()
}

fn catalog_control() -> Vec<Native> {
    vec![with_params(NativeSet::Gta5, "IsControlJustPressed", "PAD", &[("padIndex", "int"), ("control", "int")])]
}

#[test]
fn the_settings_preview_is_hinted_after_each_value() {
    let hints = check(
        r#"Citizen.CreateThread(function()
    while true do
        Wait(0)
        if IsControlJustPressed(0, 38/*<# INPUT_PICKUP (E / LB) #>*/) then
            local vehicle = CreateVehicle(-1216765807/*<# `adder` #>*/, 0.0, 0.0, 72.0, 0.0, true, false)
            TaskPlayAnim(PlayerPedId(), "anim@mp_player_intcelebrationmale@wave", "wave", 8.0, -8.0, -1, 49/*<# LOOPING | UPPERBODY | SECONDARY #>*/, 0, false, false, false)
        end
        DrawMarker(1/*<# MarkerTypeVerticalCylinder #>*/, 0.0, 0.0, 71.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 0, 0, 200, false, true, 2, false, nil, nil, false)
    end
end)"#,
    );
    assert_eq!(
        tooltips(&hints),
        [
            "Control 38: INPUT_PICKUP (E / LB)",
            "Hash -1216765807: `adder`",
            "Animation flags 49: LOOPING | UPPERBODY | SECONDARY",
            "Marker type 1: MarkerTypeVerticalCylinder",
        ]
    );
}

#[test]
fn a_control_id_is_named_from_the_natives_catalog() {
    let catalog = Project { natives: catalog_control(), ..gta5() };
    check_in(
        catalog,
        "if IsControlJustPressed(0, 38/*<# INPUT_PICKUP (E / LB) #>*/) then end\n\
         if IsControlJustPressed(0, 0/*<# INPUT_NEXT_CAMERA (V / BACK) #>*/) then end\n\
         local function helper(a, b) end\n\
         helper(0, 38)",
    );
}

#[test]
fn anim_flags_blips_markers_hud_colours_and_explosions_are_named() {
    check(
        "TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 49/*<# LOOPING | UPPERBODY | SECONDARY #>*/, 0, false, false, false)\n\
         SetBlipSprite(blip, 1/*<# radar_level #>*/)\n\
         SetBlipColour(blip, 1/*<# Red #>*/)\n\
         DrawMarker(1/*<# MarkerTypeVerticalCylinder #>*/, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 0, 0, 200, false, true, 2, false, nil, nil, false)\n\
         local r, g, b, a = GetHudColour(6/*<# HUD_COLOUR_RED #>*/)\n\
         AddExplosion(0.0, 0.0, 0.0, 2/*<# STICKYBOMB #>*/, 1.0, true, false, 1.0)",
    );
}

#[test]
fn hashes_are_named_from_the_project_and_the_bundled_list() {
    let custom = joaat("my_custom_prop");
    let hints = check(&format!(
        "local model = 'my_custom_prop'\n\
         local h = {custom}/*<# `my_custom_prop` #>*/\n\
         local veh = CreateVehicle(-1216765807/*<# `adder` #>*/, 0.0, 0.0, 0.0, 0.0, true, false)\n\
         local hex = 0xB779A091/*<# `adder` #>*/\n\
         local unknown = 123456789\n\
         local small = 1234"
    ));
    assert_eq!(tooltips(&hints)[2], "Hash 3078201489: `adder`");
    let props = Project { extra: &[("res/client/props.lua", "local p = 'my_custom_prop'")], ..gta5() };
    check_in(props, &format!("local h = {custom}/*<# `my_custom_prop` #>*/"));
}

#[test]
fn each_kind_of_value_has_its_own_setting() {
    check_in(
        Project {
            settings: Settings::defaults().with(keys::CONTROLS, false.into()).with(keys::ANIM_FLAGS, false.into()),
            ..gta5()
        },
        "if IsControlJustPressed(0, 38) then end\n\
         TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 49, 0, false, false, false)\n\
         local veh = CreateVehicle(-1216765807/*<# `adder` #>*/, 0.0, 0.0, 0.0, 0.0, true, false)",
    );
    check_in(
        with(keys::HASHES, false),
        "if IsControlJustPressed(0, 38/*<# INPUT_PICKUP (E / LB) #>*/) then end\n\
         local veh = CreateVehicle(-1216765807, 0.0, 0.0, 0.0, 0.0, true, false)",
    );
    let text = "IsControlJustPressed(0, 38)\n\
                CreateVehicle(-1216765807, 0.0, 0.0, 0.0, 0.0, true, false)\n\
                SetBlipSprite(blip, 1)\n\
                SetBlipColour(blip, 1)\n\
                DrawMarker(1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 128, 0, 200, false)\n\
                GetHudColour(6)\n\
                AddExplosion(0.0, 0.0, 0.0, 2, 1.0, true, false, 1.0)\n\
                TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 49, 0, false, false, false)";
    let all = [
        "INPUT_PICKUP (E / LB)",
        "`adder`",
        "radar_level",
        "Red",
        "MarkerTypeVerticalCylinder",
        "HUD_COLOUR_RED",
        "STICKYBOMB",
        "LOOPING | UPPERBODY | SECONDARY",
    ];
    assert_eq!(labels(gta5(), text), all);
    let off: [(&str, &[&str]); 7] = [
        (keys::CONTROLS, &["INPUT_PICKUP (E / LB)"]),
        (keys::HASHES, &["`adder`"]),
        (keys::BLIPS, &["radar_level", "Red"]),
        (keys::MARKERS, &["MarkerTypeVerticalCylinder"]),
        (keys::HUD_COLOURS, &["HUD_COLOUR_RED"]),
        (keys::EXPLOSIONS, &["STICKYBOMB"]),
        (keys::ANIM_FLAGS, &["LOOPING | UPPERBODY | SECONDARY"]),
    ];
    for (key, gone) in off {
        let kept: Vec<&str> = all.iter().copied().filter(|label| !gone.contains(label)).collect();
        assert_eq!(labels(with(key, false), text), kept, "{key} off");
    }
    assert!(labels(with(keys::HINTS, false), text).is_empty(), "values.hints off");
}

#[test]
fn the_curated_list_names_values_without_a_natives_catalog() {
    check(
        "if IsDisabledControlJustPressed(0, 38/*<# INPUT_PICKUP (E / LB) #>*/) then end\n\
         TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 1/*<# LOOPING #>*/, 0, false, false, false)\n\
         SomeUnknownNative(0, 38)\n\
         local function IsControlPressed(a, b) end\n\
         IsControlPressed(0, 38)",
    );
}

#[test]
fn a_value_already_named_by_a_comment_has_no_hint() {
    check("if IsControlJustPressed(0, 38 --[[ INPUT_PICKUP ]]) then end");
    check(
        "local function helper(a, b) end\nhelper(0, 38)\n\
         local IsControlPressed = function() end\nIsControlPressed(0, 38)\n\
         Unknown(0, 38)\nlocal small = 1234\nlocal h = 123456789\n\
         IsControlJustPressed(0, 38 --[[ INPUT_PICKUP ]])\nDrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255)",
    );
}

#[test]
fn nothing_is_hinted_when_cfxlua_is_switched_off() {
    check_in(
        with("enabled", false),
        "if IsControlJustPressed(0, 38) then end\n\
         local veh = CreateVehicle(-1216765807, 0.0, 0.0, 0.0, 0.0, true, false)",
    );
}

#[test]
fn redm_control_hashes_and_explosions_are_named() {
    let pressed =
        with_params(NativeSet::Rdr3, "IsControlJustPressed", "PAD", &[("control", "int"), ("action", "Hash")]);
    let hints = check_in(
        Project { manifest: REDM, natives: vec![pressed], ..gta5() },
        "if IsControlJustPressed(0, 0xCEFD9220/*<# INPUT_ENTER #>*/) then end\n\
         AddExplosion(0.0, 0.0, 0.0, 25/*<# DYNAMITE #>*/, 1.0, true, false, 1.0)",
    );
    assert_eq!(tooltips(&hints)[0], "Control (hash) 3472724512: INPUT_ENTER");
}

#[test]
fn only_the_values_that_end_in_the_range_are_hinted() {
    let text = "IsControlJustPressed(0, 38)\nSetBlipSprite(blip, 1)\nGetHudColour(6)";
    let second = text.find("SetBlipSprite").unwrap_or_default();
    let end = text.find("1)").unwrap_or_default() + 1;
    let at = |from: usize, to: usize| -> Vec<(usize, String)> {
        hints_in(gta5(), text, from, to).into_iter().map(|hint| (hint.at, hint.label)).collect()
    };
    assert_eq!(at(second, text.len() - "GetHudColour(6)".len() - 1), [(end, "radar_level".to_owned())]);
    assert_eq!(at(end, end), [(end, "radar_level".to_owned())], "a hint right at the end of the range");
    assert!(at(second, end - 1).is_empty(), "the value ends after the range");
    assert!(at(end + 1, end + 2).is_empty(), "the value ends before the range");
}
