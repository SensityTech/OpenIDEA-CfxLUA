use super::*;
use crate::editor::EditorFeature;
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;

const MANIFEST: (&str, &str) = (
    "shop/fxmanifest.lua",
    "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\nclient_script 'client/*.lua'\n",
);
const MAIN: &str = "shop/client/main.lua";

/// What the editor reports in the last file of a project that has been read, for one inspection.
fn reported(files: &[(&str, &str)], inspection: Inspection) -> Vec<Problem> {
    let fx = Fixture::new(files);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let doc = fx.doc(files.last().expect("a file").0);
    feature.problems(&fx.cx(&doc)).into_iter().filter(|problem| problem.code == inspection.code).collect()
}

/// The same, as `text: message`.
fn found(files: &[(&str, &str)], inspection: Inspection) -> Vec<String> {
    let text = files.last().expect("a file").1;
    reported(files, inspection).iter().map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
}

fn in_main(text: &str, inspection: Inspection) -> Vec<String> {
    found(&[MANIFEST, (MAIN, text)], inspection)
}

#[test]
fn a_class_declared_twice_is_an_error() {
    let shop = ("shop/shared/shop.lua", "---@class Shop\nlocal Shop = {}\nreturn Shop");
    let again = "---@class Shop\nlocal Other = {}\n\n---@class Cart\nlocal Cart = {}\nreturn Other, Cart";
    assert_eq!(
        found(&[MANIFEST, shop, (MAIN, again)], DUPLICATE_CLASS),
        ["Shop: Duplicate class found in the file: shop/shared/shop.lua"]
    );
    assert_eq!(reported(&[MANIFEST, shop, (MAIN, again)], DUPLICATE_CLASS)[0].severity, Severity::Error);
    let twice = "---@class Door\nlocal a = {}\n\n---@class Door\nlocal b = {}\nreturn a, b";
    assert_eq!(
        in_main(twice, DUPLICATE_CLASS),
        ["Door: Duplicate class found in the file: shop/client/main.lua"; 2],
        "each one names the other"
    );
    assert_eq!(
        in_main("---@class vector3\nlocal v = {}\nreturn v", DUPLICATE_CLASS),
        ["vector3: Duplicate class found in a bundled library"]
    );
    assert!(in_main("---@class Door\nlocal a = {}\nreturn a", DUPLICATE_CLASS).is_empty());
}

#[test]
fn a_name_that_stands_for_something_deprecated_is_struck_through() {
    let api = (
        "shop/shared/api.lua",
        "---@deprecated\nfunction OldOpen() end\nfunction Open() end\nShop = {}\n\
         ---@deprecated use Shop.open\nfunction Shop.close() end\nfunction Shop.open() end\n\
         Config = {\n    ---@deprecated\n    legacy = 1,\n    current = 2,\n}\n\
         ---@deprecated\n---@class OldDoor\nlocal OldDoor = {}\n\n---@class Door\nlocal Door = {}\nreturn OldDoor, Door",
    );
    let uses = "OldOpen()\nOpen()\nShop.close()\nShop.open()\nprint(Config.legacy, Config.current)\n\
                ---@type OldDoor\nlocal door\n---@type Door\nlocal other\nprint(door, other)\n\
                local close = 1\nprint(close)";
    assert_eq!(
        found(&[MANIFEST, api, (MAIN, uses)], DEPRECATED),
        [
            "OldOpen: 'OldOpen' is deprecated",
            "close: 'close' is deprecated",
            "legacy: 'legacy' is deprecated",
            "OldDoor: 'OldDoor' is deprecated",
        ]
    );
    let struck = &reported(&[MANIFEST, api, (MAIN, uses)], DEPRECATED)[0];
    assert_eq!((struck.severity, struck.deprecated, struck.unnecessary), (Severity::Hint, true, false));
    // A member a statement assigns is one where it is declared, a function statement is not a name that is read.
    assert_eq!(
        in_main("Shop = {}\n---@deprecated\nShop.old = 1\n---@deprecated\nfunction Shop.older() end", DEPRECATED),
        ["old: 'old' is deprecated"]
    );
    assert!(in_main("local t = { old = 1 }\nprint(t.old, Unknown.old)", DEPRECATED).is_empty());
}

#[test]
fn a_type_nothing_declares_is_an_error() {
    // `LuaCatsInspectionsTest`, "built-in and literal types are known"
    let text = "---@param a integer\n---@param b unknown\n---@param c true | false | 1 | 'x'\n---@param d NotAType\n\
                local function f(a, b, c, d) end\nf()";
    assert_eq!(in_main(text, UNRESOLVED_TYPE), ["NotAType: Unresolved type \"NotAType\""]);
    assert_eq!(reported(&[MANIFEST, (MAIN, text)], UNRESOLVED_TYPE)[0].severity, Severity::Error);
    let known = "---@class Shop\n---@field doors Door[]\n---@field byName table<string, Door>\nlocal Shop = {}\n\n\
                 ---@alias Side 'left'|'right'\n\n---@generic T\n---@param value T\n---@param side Side\n\
                 ---@param with fun<K>(key: K): K\n---@return T, userdata, thread, vector3, nil, any, table, function\n\
                 function Shop.keep(value, side, with) return value end";
    let door = ("shop/shared/door.lua", "---@class Door\nlocal Door = {}\nreturn Door");
    assert!(found(&[MANIFEST, door, (MAIN, known)], UNRESOLVED_TYPE).is_empty());
    assert_eq!(
        in_main("---@type table<string, Missing>|Lost[]\nlocal t = {}\nreturn t", UNRESOLVED_TYPE),
        ["Missing: Unresolved type \"Missing\"", "Lost: Unresolved type \"Lost\""]
    );
    // After the name of a returned value the line is a description; after a comma another type is read, as the
    // JetBrains plugin reads it: a description takes a `#`.
    let returned = |line: &str| in_main(&format!("{line}\nlocal function f() end\nf()"), UNRESOLVED_TYPE);
    assert!(returned("---@return boolean ok true on success, or nil").is_empty());
    assert!(returned("---@return boolean ok # true, or nil").is_empty());
    assert_eq!(returned("---@return number x, y"), ["y: Unresolved type \"y\""]);
    assert_eq!(returned("---@return boolean, or nil"), ["or: Unresolved type \"or\""]);
}

#[test]
fn a_param_tag_that_names_no_parameter_is_reported() {
    let text = "---@param door number\n---@param side string\nlocal function open(door) end\nopen(1)";
    assert_eq!(in_main(text, UNRESOLVED_PARAM), ["side: Cant resolve symbol 'side'"]);
    let reported = reported(&[MANIFEST, (MAIN, text)], UNRESOLVED_PARAM);
    assert_eq!(reported[0].severity, Severity::Info);
    assert_eq!(reported[0].fixes[0].title, "Remove 'side'");
    let (from, to, with) = &reported[0].fixes[0].edits[0];
    assert_eq!((&text[*from..*to], with.as_str()), ("---@param side string\n", ""));
    // The variable arguments, a loop and a function given as a value have parameters too.
    assert!(in_main("---@param ... string\nlocal function f(...) end\nf()", UNRESOLVED_PARAM).is_empty());
    assert_eq!(
        in_main("---@param ... string\nlocal function f() end\nf()", UNRESOLVED_PARAM),
        ["...: Cant resolve symbol '...'"]
    );
    assert!(in_main("---@param i number\nfor i = 1, 2 do print(i) end", UNRESOLVED_PARAM).is_empty());
    assert!(in_main("---@param a number\nlocal f = function(a) return a end\nf()", UNRESOLVED_PARAM).is_empty());
    assert_eq!(in_main("---@param a number\nlocal b = 1\nprint(b)", UNRESOLVED_PARAM), ["a: Cant resolve symbol 'a'"]);
}

#[test]
fn they_wait_for_the_project_to_be_read_and_follow_their_settings() {
    let text = "---@type Missing\nlocal a = nil\n---@param b number\nlocal function f() end\nf(a)";
    let files = [MANIFEST, (MAIN, text)];
    let mut fx = Fixture::new(&files);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let codes = |fx: &Fixture| -> Vec<&'static str> {
        feature.problems(&fx.cx(&fx.doc(MAIN))).iter().map(|problem| problem.code).collect()
    };
    assert_eq!(codes(&fx), ["UnresolvedClass", "SimplifyLocalAssignment", "UnresolvedSymbolInEmmyDoc"]);
    fx.ws.indexing = true;
    assert_eq!(codes(&fx), ["SimplifyLocalAssignment"], "a project half read declares half its types");
    fx.ws.indexing = false;
    let fx = fx.with_settings(Settings::defaults().with(UNRESOLVED_TYPE.setting, false.into()));
    assert_eq!(codes(&fx), ["SimplifyLocalAssignment", "UnresolvedSymbolInEmmyDoc"]);
}
