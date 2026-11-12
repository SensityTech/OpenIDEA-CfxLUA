//! What the editor answers about the libraries: the standard library of Lua, the Cfx runtime and the natives.

use super::EditorFeature;
use super::completion::tests::{MAIN, MANIFEST};
use crate::feature::Feature;
use crate::natives::{Native, NativeSet, Param, Return};
use crate::project::Side;
use crate::settings::Settings;
use crate::testing::{Fixture, native};
use openidea_plugin::providers::CompletionKind;
use serde_json::Value;

const BOTH_SIDES: (&str, &str) = (
    "shop/fxmanifest.lua",
    "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'\n",
);
const SERVER_MAIN: &str = "shop/server/main.lua";

/// `GetEntityCoords(entity, alive)` of the client and `GetPlayerName(player)` of the server, as their catalogs
/// describe them.
fn natives() -> Vec<Native> {
    let param = |name: &str, lua_type: &str| Param { name: name.into(), lua_type: lua_type.into(), ..Param::default() };
    let returned = |lua_type: &str| Return { lua_type: lua_type.into(), ..Return::default() };
    vec![
        Native {
            params: vec![param("entity", "integer"), param("alive", "boolean|nil")],
            returns: vec![returned("vector3")],
            ..native(NativeSet::Gta5, "GetEntityCoords", Side::Client)
        },
        Native {
            params: vec![param("player", "integer|string")],
            returns: vec![returned("string")],
            ..native(NativeSet::Server, "GetPlayerName", Side::Server)
        },
    ]
}

fn fixture(files: &[(&str, &str)], settings: Settings) -> (Fixture, EditorFeature) {
    let fx = Fixture::new(files).with_natives(natives()).with_settings(settings);
    let feature = EditorFeature::default();
    fx.index(&feature);
    (fx, feature)
}

/// What hovering the `<caret>` of the last file shows.
fn hovered(files: &[(&str, &str)]) -> String {
    let (fx, feature) = fixture(files, Settings::defaults());
    let (doc, at) = fx.caret(files.last().expect("a file").0);
    feature.hover(&fx.cx(&doc), at).map(|hover| hover.markdown).unwrap_or_default()
}

/// The declaration a hover starts with.
fn declared(files: &[(&str, &str)]) -> String {
    hovered(files).lines().nth(1).unwrap_or_default().to_owned()
}

fn signatures(files: &[(&str, &str)]) -> Vec<String> {
    let (fx, feature) = fixture(files, Settings::defaults());
    let (doc, at) = fx.caret(files.last().expect("a file").0);
    let help = feature.signature_help(&fx.cx(&doc), at);
    help.map_or(Vec::new(), |help| help.signatures.into_iter().map(|shown| shown.label).collect())
}

/// The names a completion lists at the `<caret>` of the last file, each once, the words of the file left out.
fn listed(files: &[(&str, &str)], settings: Settings) -> Vec<String> {
    let (fx, feature) = fixture(files, settings);
    let (doc, at) = fx.caret(files.last().expect("a file").0);
    let items = feature.completion(&fx.cx(&doc), at, None);
    let mut labels: Vec<String> =
        items.into_iter().filter(|item| item.kind != Some(CompletionKind::Text)).map(|item| item.label).collect();
    labels.dedup();
    labels
}

fn client(main: &str) -> [(&str, &str); 2] {
    [MANIFEST, (MAIN, main)]
}

#[test]
fn the_standard_library_is_documented() {
    let print = hovered(&client("pri<caret>nt('x')"));
    assert!(
        print.starts_with("```lua\nfunction print(...: any): void\n```\n\nReceives any number of arguments"),
        "{print}"
    );
    assert_eq!(
        declared(&client("local s = string.for<caret>mat('%d', 1)")),
        "function string.format(formatstring: string, ...: any): string"
    );
    assert_eq!(declared(&client("local up = ('x'):up<caret>per()")), "function string.upper(s: string): string");
    assert_eq!(declared(&client("---@type str<caret>ing\nlocal s")), "class string");
    assert_eq!(declared(&client("local n = math.floor(1.5)\nprint(n<caret>)")), "local n: number");
}

#[test]
fn the_cfx_runtime_is_documented_from_its_stubs() {
    let wait = hovered(&client("Citizen.Wa<caret>it(0)"));
    assert!(
        wait.starts_with("```lua\nfunction Citizen.Wait(ms: number): void\n```\n\nPauses the current scheduler"),
        "{wait}"
    );
    assert!(wait.contains("**Parameters**\n- `ms`: `number` — Number of milliseconds to wait."), "{wait}");
    assert_eq!(
        hovered(&client("local v = vector3(1, 2, 3)\nprint(v.x<caret>)")),
        "```lua\nvector3.x: number\n```\n\nFirst component."
    );
    assert_eq!(declared(&client("local v = vector3(1, 2, 3)\nprint(v<caret>)")), "local v: vector3");
}

#[test]
fn a_native_is_the_function_its_catalog_describes() {
    assert_eq!(hovered(&client("local c = GetEntity<caret>Coords(1)")), "", "documented by the natives themselves");
    assert_eq!(declared(&client("local c = GetEntityCoords(1)\nprint(c<caret>)")), "local c: vector3");
    assert_eq!(signatures(&client("GetEntityCoords(<caret>)")), ["entity: number, alive?: boolean"]);
    assert_eq!(
        listed(&client("local c = GetEntityCoords(1)\nprint(c.<caret>)"), Settings::defaults())[..3],
        ["b", "dim", "g"],
        "the fields of what it returns"
    );
    let named = "local function GetEntityCoords(a) end\nGetEntityCoords(<caret>)";
    assert_eq!(signatures(&client(named)), ["a: any"], "a function of the file of that name stands for it");
}

#[test]
fn a_script_sees_the_libraries_of_its_side() {
    let server = |main: &'static str| [BOTH_SIDES, (SERVER_MAIN, main)];
    assert_eq!(declared(&client("SendNUI<caret>Message({})")), "function SendNUIMessage(message: any): void");
    assert_eq!(hovered(&server("SendNUI<caret>Message({})")), "", "a server script has no NUI");
    assert_eq!(declared(&server("GetPlay<caret>ers()")), "function GetPlayers(): string[]");
    assert_eq!(hovered(&client("GetPlay<caret>ers()")), "", "nor a client script the list of players");
    assert_eq!(signatures(&server("GetPlayerName(<caret>)")), ["player: number|string"]);
    assert_eq!(signatures(&client("GetPlayerName(<caret>)")), Vec::<String>::new(), "a native of the server");
    assert_eq!(signatures(&server("GetEntityCoords(<caret>)")), Vec::<String>::new(), "a native of the game");
    assert_eq!(declared(&server("Citizen.Wa<caret>it(0)")), "function Citizen.Wait(ms: number): void", "shared");
}

#[test]
fn the_names_and_the_members_of_the_libraries_are_completed() {
    assert_eq!(listed(&client("pri<caret>"), Settings::defaults())[0], "print");
    assert_eq!(listed(&client("Cit<caret>"), Settings::defaults())[0], "Citizen");
    let string = listed(&client("local s = string.<caret>"), Settings::defaults());
    assert!(["byte", "format", "gsub", "upper", "strsplit"].iter().all(|name| string.contains(&(*name).to_owned())));
    let method = listed(&client("local s = ('x'):<caret>"), Settings::defaults());
    assert!(method.contains(&"upper".to_owned()) && !method.contains(&"char".to_owned()), "what takes a string");
    let off = || Settings::defaults().with("runtimeApi", Value::from(false));
    assert_eq!(listed(&client("pri<caret>"), off())[0], "print", "Lua itself is always there");
    let hidden = listed(&client("Cit<caret>"), off());
    assert!(!hidden.contains(&"Citizen".to_owned()), "the runtime API is switched off: {hidden:?}");
}

#[test]
fn the_names_of_library_parameters_are_hinted() {
    let (fx, feature) =
        fixture(&client("local c = GetEntityCoords(1, true)\nCitizen.Wait(0)\nprint(1)"), Settings::defaults());
    let doc = fx.doc(MAIN);
    let hints = feature.inlay_hints(&fx.cx(&doc), 0, doc.text().len());
    let labels: Vec<&str> = hints.iter().map(|hint| hint.label.as_str()).collect();
    assert_eq!(labels, ["entity:", "alive:", "ms:"], "`print` takes any number of values: none is named");
}

#[test]
fn a_library_has_no_file_to_go_to() {
    let (fx, feature) = fixture(&client("pri<caret>nt('x')"), Settings::defaults());
    let (doc, at) = fx.caret(MAIN);
    assert!(feature.definition(&fx.cx(&doc), at).is_empty());
}
