use super::EditorFeature;
use super::semantic::Token;
use crate::feature::Feature;
use crate::natives::NativeSet;
use crate::project::Side;
use crate::testing::{Fixture, native};

const CLIENT: &str = "res/client.lua";
const MANIFEST: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nshared_script 'shared.lua'\n";

fn fixture(client: &str, shared: &str) -> Fixture {
    Fixture::new(&[("res/fxmanifest.lua", MANIFEST), (CLIENT, client), ("res/shared.lua", shared)])
        .with_natives(vec![native(NativeSet::Gta5, "GetEntityCoords", Side::Client)])
}

/// Each token as `text kind modifiers`, in source order.
fn painted(client: &str, shared: &str) -> Vec<String> {
    let fx = fixture(client, shared);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let doc = fx.doc(CLIENT);
    let tokens: Vec<Token> = feature.semantic_tokens(&fx.cx(&doc));
    tokens
        .iter()
        .map(|token| {
            let mut parts = vec![doc.text()[token.start..token.end].to_owned(), token.kind_name().to_owned()];
            parts.extend(token.modifier_names().into_iter().map(str::to_owned));
            parts.join(" ")
        })
        .collect()
}

#[test]
fn locals_parameters_and_upvalues() {
    let found = painted(
        "local count = 0\nlocal function add(n)\n  count = count + n\n  return function() return n end\nend",
        "",
    );
    assert_eq!(
        found,
        [
            "count variable declaration",
            "add function declaration",
            "n parameter declaration",
            "count variable mutable",
            "count variable mutable",
            "n parameter",
            "n parameter mutable",
        ]
    );
}

#[test]
fn globals_and_functions_of_the_project() {
    let found = painted("function Start() end\nStart()\nHelper()\nUnknown()\nvalue = other", "function Helper() end");
    assert_eq!(
        found,
        [
            "Start function declaration global",
            "Start function global",
            "Helper function global",
            "Unknown variable global",
            "value variable global",
            "other variable global",
        ]
    );
}

#[test]
fn natives_runtime_and_standard_library() {
    let found = painted(
        "local p = GetEntityCoords(ped)\nCitizen.CreateThread(f)\nprint(string.format('%s', p), math.pi)\nlocal type = 1\nprint(type)",
        "",
    );
    assert_eq!(
        found,
        [
            "p variable declaration",
            "GetEntityCoords function default-library",
            "ped variable global",
            "Citizen variable default-library",
            "CreateThread function default-library",
            "f variable global",
            "print function default-library",
            "string variable default-library",
            "format function default-library",
            "p variable",
            "math variable default-library",
            "pi variable default-library",
            "type variable declaration",
            "print function default-library",
            "type variable",
        ]
    );
}

#[test]
fn fields_methods_and_self() {
    let found = painted(
        "local M = { name = 'x', .ready }\nfunction M.create(a) return M.name end\nfunction M:run()\n  self.count = self:step()\n  return function() return self end\nend\nM.util.log(M:get())",
        "",
    );
    assert_eq!(
        found,
        [
            "M variable declaration",
            "name property",
            "ready property",
            "M variable",
            "create method declaration static",
            "a parameter declaration",
            "M variable mutable",
            "name property",
            "M variable",
            "run method declaration",
            "self self-keyword",
            "count property",
            "self self-keyword",
            "step method",
            "self self-keyword mutable",
            "M variable",
            "util property",
            "log method static",
            "M variable",
            "get method",
        ]
    );
}

#[test]
fn hashes_take_the_string_colour() {
    assert_eq!(painted("local h = `adder`", ""), ["h variable declaration", "`adder` string byte-string"]);
}

#[test]
fn loop_variables_are_parameters_and_function_statements_over_locals_are_not_globals() {
    let found = painted("local f\nfunction f() end\nfor i, v in pairs(t) do f(i, v) end", "");
    assert_eq!(
        found,
        [
            "f variable declaration",
            "f variable",
            "i parameter declaration",
            "v parameter declaration",
            "pairs function default-library",
            "t variable global",
            "f variable",
            "i parameter",
            "v parameter",
        ]
    );
}
