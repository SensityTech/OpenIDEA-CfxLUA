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

fn source(text: &str) -> crate::lua::Source {
    crate::lua::Source::new(text)
}

/// Each fold as `first line..last line` (1-based) and its kind.
fn folded(text: &str) -> Vec<String> {
    let src = source(text);
    let line = |at: usize| text[..at].matches('\n').count() + 1;
    super::folding::folds(&src)
        .iter()
        .map(|fold| format!("{}..{} {:?}", line(fold.start), line(fold.end), fold.kind))
        .collect()
}

#[test]
fn blocks_tables_comments_and_regions_fold() {
    let text = "function f(a)\n  if a then\n    g()\n  elseif b then\n    h()\n  else\n    i()\n  end\nend\n\
                local t = {\n  1,\n}\n--[[ a\n comment ]]\n--- doc\n--- more\nlocal s = [[\nlong]]\n\
                --region r\nx = 1\n--endregion\nlocal one = { 1 } do f() end";
    assert_eq!(
        folded(text),
        [
            "1..9 Region",
            "2..3 Region",
            "4..5 Region",
            "6..8 Region",
            "10..12 Region",
            "13..14 Comment",
            "15..16 Comment",
            "17..18 Region",
            "19..21 Region",
        ]
    );
}

/// The highlighted texts with the caret before the `nth` occurrence of `needle`, with their access.
fn highlighted(text: &str, needle: &str, nth: usize) -> Vec<String> {
    let src = source(text);
    let at = text.match_indices(needle).nth(nth).map(|(at, _)| at).expect("needle");
    let line = |at: usize| text[..at].matches('\n').count() + 1;
    super::highlights::highlights(&src, at)
        .iter()
        .map(|h| format!("{}@{} {:?}", &text[h.start..h.end], line(h.start), h.access))
        .collect()
}

#[test]
fn the_uses_of_a_name_are_highlighted_with_their_access() {
    let text = "local n = 1\nn = n + 1\nfunction f(n) return n end\nprint(n)";
    assert_eq!(highlighted(text, "n +", 0), ["n@1 Write", "n@2 Write", "n@2 Read", "n@4 Read"]);
    assert_eq!(highlighted(text, "n)", 0), ["n@3 Write", "n@3 Read"]);
    let text = "total = 0\nfunction add() total += 1 end\nprint(total, other)";
    assert_eq!(highlighted(text, "total", 2), ["total@1 Write", "total@2 Write", "total@3 Read"]);
}

#[test]
fn members_are_matched_on_the_object_they_are_read_from() {
    let text =
        "Config.debug = true\nif Config.debug then print(Other.debug, Config?.debug) end\nfunction Config.debug() end";
    assert_eq!(highlighted(text, "debug", 1), ["debug@1 Write", "debug@2 Read", "debug@2 Read", "debug@3 Write"]);
}

#[test]
fn return_break_and_operators() {
    let text = "function f()\n  if a then return 1 end\n  local g = function() return 2 end\n  return 3\nend";
    assert_eq!(highlighted(text, "return", 2), ["return 1@2 Text", "return 3@4 Text"]);
    let text = "while a do\n  for i = 1, 2 do break end\nend";
    assert_eq!(highlighted(text, "break", 0), ["for@2 Text", "end@2 Text", "break@2 Text"]);
    assert_eq!(highlighted("x = a + b * c", "+", 0), ["a + b * c@1 Text"]);
}

/// The outline as `depth name detail kind`.
fn outlined(text: &str) -> Vec<String> {
    let symbols = super::symbols::symbols(&source(text));
    symbols
        .iter()
        .map(|symbol| {
            let mut depth = 0;
            let mut parent = symbol.parent;
            while let Some(index) = parent {
                depth += 1;
                parent = symbols[index as usize].parent;
            }
            format!("{depth} {}{} {}", symbol.name, symbol.detail, symbol.kind)
        })
        .collect()
}

#[test]
fn the_structure_lists_declarations_with_what_they_hold() {
    let text = "local M = { name = 1, run = function(a) end }\nlocal function helper(x, ...)\n  local inner = 2\nend\n\
                function Start() local hidden end\nfunction M.sub.go(self) local step end\nfunction M:stop() end\n\
                Config = { debug = true }\nConfig.Locale.fr = {}\nOnReady(function(cb) local state end)\nM = nil";
    assert_eq!(
        outlined(text),
        [
            "0 M variable",
            "1 name field",
            "1 run(a) function",
            "1 sub field",
            "2 go(self) method",
            "3 step variable",
            "1 stop() method",
            "0 helper(x, ...) function",
            "1 inner variable",
            "0 Start() function",
            "0 Config variable",
            "1 debug field",
            "1 Locale.fr field",
            "0 <anonymous>(cb) function",
            "1 state variable",
        ]
    );
}
