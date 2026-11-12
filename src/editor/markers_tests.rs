//! The gutter markers the editor feature gives to a file of a project.

use super::EditorFeature;
use super::markers::{CLASSES, OVERRIDES, RECURSIVE_CALLS, TAIL_CALLS};
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;
use openidea_plugin::Location;

const MANIFEST: (&str, &str) =
    ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\n");
const BASE: (&str, &str) = (
    "shop/shared/base.lua",
    "---@class Base\nBase = {}\nfunction Base:open() end\nfunction Base:close() end\nfunction Base.create() end\n",
);
const MID: (&str, &str) =
    ("shop/shared/mid.lua", "---@class Mid : Base\nMid = {}\nfunction Mid:open() end\nfunction Mid:lock() end\n");
const SHOP: (&str, &str) = (
    "shop/shared/shop.lua",
    "---@class Shop : Mid\nShop = {}\nfunction Shop:open() end\nfunction Shop:close() end\nfunction Shop:sell() end\n",
);
const MAIN: &str = "shop/shared/main.lua";

/// Each marker of the file `rel` as `line icon: tooltip -> file:line`, lines counted from 1.
fn shown(fx: &Fixture, rel: &str) -> Vec<String> {
    let feature = EditorFeature::default();
    fx.index(&feature);
    let doc = fx.doc(rel);
    let place = |target: &Location| {
        let path = target.path.strip_prefix("/p/shop/shared/").unwrap_or(&target.path);
        format!(" -> {path}:{}", target.range.start.line + 1)
    };
    let icon = |icon: &str| icon.trim_start_matches("icons/markers/").trim_end_matches(".svg").to_owned();
    let markers = feature.line_markers(&fx.cx(&doc));
    markers
        .iter()
        .map(|marker| {
            let to = marker.target.as_ref().map(place).unwrap_or_default();
            let tooltip = marker.tooltip.replace('\n', " / ");
            format!("{} {}: {tooltip}{to}", marker.line + 1, icon(marker.icon.as_deref().unwrap_or_default()))
        })
        .collect()
}

fn in_file(text: &str) -> Vec<String> {
    shown(&Fixture::new(&[MANIFEST, (MAIN, text)]), MAIN)
}

#[test]
fn a_method_shows_what_it_overrides_and_what_overrides_it() {
    let fx = Fixture::new(&[MANIFEST, BASE, MID, SHOP]);
    assert_eq!(
        shown(&fx, SHOP.0),
        [
            "1 class: Class Shop",
            "3 overriding: Overrides function in Mid -> mid.lua:3",
            "4 overriding: Overrides function in Base -> base.lua:4",
        ]
    );
    assert_eq!(
        shown(&fx, MID.0),
        [
            "1 overridden: Class Mid / Is subclassed by Shop -> shop.lua:1",
            "3 overriding: Overrides function in Base / Is overridden in Shop -> base.lua:3",
        ]
    );
    assert_eq!(
        shown(&fx, BASE.0),
        [
            "1 overridden: Class Base / Is subclassed by Mid, Shop -> mid.lua:1",
            "3 overridden: Is overridden in Mid, Shop -> mid.lua:3",
            "4 overridden: Is overridden in Shop -> shop.lua:4",
        ]
    );
}

#[test]
fn a_call_to_the_function_it_is_written_in_is_recursive() {
    assert_eq!(
        in_file("local function fact(n)\n    return n * fact(n - 1)\nend\nfact(3)"),
        ["2 recursive: Recursive call"]
    );
    assert_eq!(in_file("function walk(node)\n    walk(node.next)\nend"), ["2 recursive: Recursive call"]);
    assert_eq!(
        in_file("---@class Tree\nTree = {}\nfunction Tree:count()\n    return 1 + self:count()\nend")[1..],
        ["4 recursive: Recursive call"]
    );
    assert_eq!(
        in_file(
            "local function outer()\n    local function inner()\n        outer()\n    end\n    inner()\nend\nouter()"
        ),
        ["3 recursive: Recursive call"],
        "through a nested function"
    );
    let others = "local function a() end\nlocal function b()\n    a()\n    local b = a\n    b()\nend\nb()\n\
                  function walk(t)\n    t.walk()\n    other:walk()\nend";
    assert!(in_file(others).is_empty(), "another function, a local of the same name, a field of the same name");
}

#[test]
fn a_returned_call_is_a_tail_call_and_a_line_has_one_marker() {
    assert_eq!(in_file("function f(x)\n    return g(x)\nend"), ["2 tailCall: Tail call"]);
    assert_eq!(in_file("function f(x)\n    return x, g(x)\nend"), ["2 tailCall: Tail call"]);
    assert!(in_file("function f(x)\n    return g(x) + 1, (g(x)), x.y\nend\nreturn 1").is_empty());
    assert_eq!(
        in_file("function f(x)\n    return f(x - 1)\nend"),
        ["2 recursive: Recursive call"],
        "the recursion first"
    );
}

#[test]
fn markers_follow_their_settings_and_wait_for_the_project() {
    let text = "---@class Door\nDoor = {}\nfunction Door:open()\n    return self:open()\nend\n\
                function Door:close()\n    return f()\nend";
    let files = [MANIFEST, BASE, (MAIN, text)];
    let all = ["1 class: Class Door", "4 recursive: Recursive call", "7 tailCall: Tail call"];
    assert_eq!(shown(&Fixture::new(&files), MAIN), all);
    let without = |setting: &str| {
        let fx = Fixture::new(&files).with_settings(Settings::defaults().with(setting, false.into()));
        shown(&fx, MAIN)
    };
    assert_eq!(without(CLASSES), all[1..]);
    assert_eq!(without(RECURSIVE_CALLS), ["1 class: Class Door", "4 tailCall: Tail call", "7 tailCall: Tail call"]);
    assert_eq!(without(TAIL_CALLS), all[..2]);
    let mut fx = Fixture::new(&files);
    fx.ws.indexing = true;
    assert_eq!(shown(&fx, MAIN), all[1..], "who inherits from a class is not known yet");
    let fx = Fixture::new(&[MANIFEST, BASE, MID]).with_settings(Settings::defaults().with(OVERRIDES, false.into()));
    assert_eq!(shown(&fx, MID.0), ["1 class: Class Mid"], "no marker on its methods");
}
