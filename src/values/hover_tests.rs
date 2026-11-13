use super::hover;
use crate::lua::joaat::joaat;
use crate::natives::{Native, NativeSet};
use crate::settings::Settings;
use crate::testing::Fixture;
use crate::values::ValuesFeature;
use crate::values::sites::tests::with_params;
use serde_json::Value;

const FIVEM: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";

fn shown_with(text: &str, natives: Vec<Native>, settings: Settings) -> Option<(String, String)> {
    let fx =
        Fixture::new(&[("res/fxmanifest.lua", FIVEM), (CLIENT, text)]).with_natives(natives).with_settings(settings);
    let values = ValuesFeature::default();
    fx.index(&values);
    let (doc, at) = fx.caret(CLIENT);
    let found = hover(&values, &fx.cx(&doc), at)?;
    let (start, end) = found.range;
    Some((doc.text()[start..end].to_owned(), found.markdown))
}

fn shown(text: &str) -> Option<(String, String)> {
    shown_with(text, Vec::new(), Settings::defaults())
}

fn pressed() -> Native {
    with_params(NativeSet::Gta5, "IsControlJustPressed", "PAD", &[("padIndex", "int"), ("control", "int")])
}

#[test]
fn a_control_argument_shows_its_name_its_keys_and_the_reference_table() {
    let (range, page) =
        shown_with("if IsControlJustPressed(0, 3<caret>8) then end", vec![pressed()], Settings::defaults())
            .expect("a page");
    assert_eq!(range, "38");
    assert_eq!(
        page,
        "**Control** `38` = **INPUT_PICKUP**\n\nArgument 2 (control) of IsControlJustPressed\n\n- Keyboard: `E`\n- Pad: `LB`\n\n[Reference table](https://docs.fivem.net/docs/game-references/controls/)\n"
    );
}

#[test]
fn nothing_is_shown_on_an_argument_that_takes_no_value_nor_outside_a_call() {
    assert!(
        shown_with("if IsControlJustPressed(<caret>0, 38) then end", vec![pressed()], Settings::defaults()).is_none()
    );
    assert!(shown("local x = 3<caret>8").is_none());
}

#[test]
fn a_number_given_to_no_known_native_shows_nothing() {
    assert!(shown("Foo(0, 3<caret>8)").is_none());
    assert!(shown("print(12345<caret>6789)").is_none());
}

#[test]
fn a_hash_number_shows_where_its_name_comes_from() {
    let (range, page) =
        shown("local veh = CreateVehicle(-12167<caret>65807, 0.0, 0.0, 0.0, 0.0, true, false)").expect("a page");
    assert_eq!(range, "-1216765807");
    assert!(page.starts_with("**Hash** `-1216765807` = **adder**\n\nArgument 1 of CreateVehicle\n"), "{page}");
    for part in ["Name from: `the bundled names (vehicle)`", "`3078201489`", "`0xB779A091`"] {
        assert!(page.contains(part), "{part} in {page}");
    }
    let hash = joaat("my_custom_prop").to_string();
    let text = format!("local name = 'my_custom_prop'\nlocal h = {}<caret>{}\n", &hash[..2], &hash[2..]);
    let (_, page) = shown(&text).expect("a page");
    assert!(page.contains("= **my_custom_prop**") && page.contains("a string of the project"), "{page}");
}

#[test]
fn animation_flags_list_every_flag() {
    let (_, page) =
        shown("TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 4<caret>9, 0, false, false, false)").expect("a page");
    for part in ["AF_LOOPING (REPEAT)", "AF_UPPERBODY (UPPERBODY)", "AF_SECONDARY (ENABLE_PLAYER_CONTROL)"] {
        assert!(page.contains(part), "{part} in {page}");
    }
}

#[test]
fn the_number_is_found_from_its_first_character_to_right_after_it() {
    let text = "IsControlJustPressed(0, 38)";
    let start = text.find("38").expect("38");
    let fx = Fixture::new(&[("res/fxmanifest.lua", FIVEM), (CLIENT, text)]).with_natives(vec![pressed()]);
    let values = ValuesFeature::default();
    let doc = fx.doc(CLIENT);
    let cx = fx.cx(&doc);
    for at in [start, start + 1, start + 2] {
        assert_eq!(hover(&values, &cx, at).map(|h| h.range), Some((start, start + 2)), "at {at}");
    }
    assert!(hover(&values, &cx, start - 1).is_none());
}

#[test]
fn a_hash_literal_shows_its_three_values() {
    let (range, page) = shown("local h = `ad<caret>der`").expect("a page");
    assert_eq!(range, "`adder`");
    assert_eq!(
        page,
        "hash **\\`adder\\`**\n\nCompile-time Jenkins one-at-a-time hash (joaat) of the ASCII lower-cased text, replaced by a signed 32-bit integer.\n\n**Signed (int32):** `-1216765807`  \n**Unsigned (uint32):** `3078201489`  \n**Hex:** `0xB779A091`"
    );
    let (_, upper) = shown("local h = `AD<caret>DER`").expect("a page");
    assert!(upper.contains("`0xB779A091`"), "lower-cased like the runtime: {upper}");
}

#[test]
fn an_invalid_hash_literal_says_so() {
    let invalid = "Invalid hash literal: it is unterminated or has an escape sequence the runtime rejects.";
    let (range, page) = shown("local h = `ad<caret>der").expect("a page");
    assert_eq!(range, "`adder");
    assert_eq!(page, format!("hash **\\`adder**\n\n{invalid}"));
    let (_, page) = shown("local h = `ba<caret>d\\q`").expect("a page");
    assert!(page.ends_with(invalid) && !page.contains("Signed"), "{page}");
}

#[test]
fn nothing_is_shown_when_the_plugin_is_off() {
    let off = || Settings::defaults().with("enabled", Value::from(false));
    assert!(shown_with("local h = `ad<caret>der`", Vec::new(), off()).is_none());
    assert!(shown_with("IsControlJustPressed(0, 3<caret>8)", vec![pressed()], off()).is_none());
}
