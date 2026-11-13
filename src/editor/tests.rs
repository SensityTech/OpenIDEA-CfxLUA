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
fn documentation_comments() {
    let found = painted(
        "---@class Shop : Base @a shop\n---@field private name string the name\n---@field [string] Item\n\
         local Shop = {}\n\n---@alias Kind\n---| 'a' # first\n\n---@generic T\n---@param list T[] the list\n\
         ---@param cb fun(item: T, vararg any): boolean?\n---@return table<string, T> found\n\
         ---@deprecated use other\n---@suppress unused\nlocal function find(list, cb) end",
        "",
    );
    assert_eq!(
        found,
        [
            "class decorator",
            "Shop type declaration",
            "Base type",
            "field decorator",
            "name property",
            "string type",
            "field decorator",
            "string type",
            "Item type",
            "Shop variable declaration",
            "alias decorator",
            "Kind type declaration",
            "| decorator",
            "generic decorator",
            "param decorator",
            "list parameter",
            "T type",
            "param decorator",
            "cb parameter",
            "fun self-type-keyword",
            "T type",
            "vararg self-type-keyword",
            "any type",
            "boolean type",
            "return decorator",
            "table type",
            "string type",
            "T type",
            "deprecated decorator",
            "find function declaration",
            "list parameter declaration",
            "cb parameter declaration",
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

#[test]
fn the_global_names_of_a_file_are_its_free_names_and_what_it_reads_through_the_globals_table() {
    let src = source(
        "local kept, t = 1, {}\nfunction Start() end\nfunction t.method() end\nlocal function helper() end\n\
         count = kept + other\nprint(_G.viaTable, _G['viaKey'], _G[computed], t.field, helper())\n\
         later = 1\nfunction later() end",
    );
    let (functions, used) = super::global_names(&src);
    assert_eq!(functions, ["Start"], "a name assigned before its function statement stays a variable");
    assert_eq!(used, ["Start", "_G", "computed", "count", "later", "other", "print", "viaKey", "viaTable"]);
    let src = source("local _G = {}\nprint(_G.name)");
    assert_eq!(super::global_names(&src).1, ["print"], "a local named _G is a table like another");
}

#[test]
fn a_name_is_elsewhere_when_a_file_other_than_the_asking_one_holds_it() {
    let mut names = super::NamesByFile::default();
    names.set("a.lua", &["count", "total"]);
    names.set("b.lua", &["total"]);
    assert!(names.holds("count") && names.holds("total") && !names.holds("other"));
    assert!(!names.elsewhere("a.lua", "count"), "only the file that asks holds it");
    assert!(names.elsewhere("b.lua", "count") && names.elsewhere("c.lua", "count"));
    assert!(names.elsewhere("a.lua", "total") && names.elsewhere("b.lua", "total"));
    assert!(!names.elsewhere("a.lua", "other"));
    names.set("b.lua", &["count"]);
    assert!(!names.elsewhere("a.lua", "total"), "the other file was read again without it");
    assert!(names.elsewhere("a.lua", "count"));
    names.remove("b.lua");
    assert!(!names.elsewhere("a.lua", "count"));
    names.remove("a.lua");
    assert!(!names.holds("count") && !names.holds("total"));
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

/// Each entry as `depth name detail kind`.
fn depths(symbols: &[super::symbols::Symbol]) -> Vec<String> {
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

/// The outline of the JetBrains plugin.
fn outlined(text: &str) -> Vec<String> {
    depths(&super::symbols::as_intellij(&source(text)))
}

/// The outline OpenIDEA is given: every parent comes first and is written around its children.
fn listed(text: &str) -> Vec<String> {
    let symbols = super::symbols::symbols(&source(text));
    for (index, symbol) in symbols.iter().enumerate() {
        if let Some(parent) = symbol.parent {
            let holder = &symbols[parent as usize];
            assert!((parent as usize) < index, "{} comes before {}", symbol.name, holder.name);
            assert!(
                holder.range.0 <= symbol.range.0 && symbol.range.1 <= holder.range.1,
                "{} in {}",
                symbol.name,
                holder.name
            );
        }
    }
    depths(&symbols)
}

/// The fixture of the JetBrains plugin's structure view, whose comments say what each line gives.
#[test]
fn the_structure_of_the_intellij_fixture() {
    assert_eq!(
        outlined(include_str!("testdata/structure.lua")),
        [
            "0 Emmy2 class",
            "1 name field",
            "1 age field",
            "1 doit() method",
            "1 sayHello() method",
            "1 baz() method",
            "0 f variable",
            "0 a variable",
            "0 globalFunc() function",
            "0 globalFuncWithArgs(a, b, c) function",
            "0 localFunc() function",
            "0 localFunc2() function",
            "1 localFuncNestedInLocalFunc2() function",
            "1 bobby variable",
            "0 d variable",
            "1 foo() function",
            "1 bar(a, b) function",
            "0 g variable",
            // The last line of the fixture is not Lua: what follows `local F` reads as `A.x = function`.
            "0 A variable",
            "1 B.C field",
            "2 doit() method",
            "2 doitAgain() function",
            "2 var field",
            "1 x() function",
            "2 x variable",
            "0 globalVar variable",
            "0 globalFunc2() function",
            "1 j variable",
            "0 globalTable variable",
            "1 a() function",
            "0 globalC2 variable",
            "0 xyz variable",
            "0 <anonymous>() function",
            "1 a variable",
            "1 F variable",
        ]
    );
    assert_eq!(
        outlined(
            "function A.B.C.doit() end
A.B.C.again = function() end"
        ),
        ["0 A.B.C variable", "1 doit() method", "1 again() function"]
    );
    assert_eq!(outlined("---@class A\nlocal m = {}\nfunction m:method()end"), ["0 A class", "1 method() method"]);
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

#[test]
fn an_entry_declared_away_from_its_holder_is_listed_where_it_is_written() {
    let text = "local M = { name = 1, run = function(a) end }\nlocal function helper(x, ...)\n  local inner = 2\nend\n\
                function Start() local hidden end\nfunction M.sub.go(self) local step end\nfunction M:stop() end\n\
                Config = { debug = true }\nConfig.Locale.fr = {}\nOnReady(function(cb) local state end)\nM = nil";
    assert_eq!(
        listed(text),
        [
            "0 M variable",
            "1 name field",
            "1 run(a) function",
            "0 helper(x, ...) function",
            "1 inner variable",
            "0 Start() function",
            "0 M.sub.go(self) method",
            "1 step variable",
            "0 M:stop() method",
            "0 Config.debug variable",
            "0 Config.Locale.fr field",
            "0 <anonymous>(cb) function",
            "1 state variable",
        ]
    );
    assert_eq!(
        listed("Config.debug = true\nConfig.zones = { a = 1, b = 2 }"),
        ["0 Config.debug variable", "0 Config.zones variable", "1 a field", "1 b field"]
    );
    assert_eq!(
        listed(
            "function Open()\n  self.name = 1\nend\nlocal t = {}\nlocal function fill()\n  t.x = function() local y end\nend"
        ),
        ["0 Open() function", "0 t variable", "0 fill() function", "1 t.x() function", "2 y variable"]
    );
}

#[test]
fn the_intellij_fixture_as_openidea_nests_it() {
    assert_eq!(
        listed(include_str!("testdata/structure.lua")),
        [
            "0 Emmy2 class",
            "1 name field",
            "1 age field",
            "1 doit() method",
            "0 f variable",
            "0 e:sayHello() method",
            "0 a variable",
            "0 globalFunc() function",
            "0 globalFuncWithArgs(a, b, c) function",
            "0 localFunc() function",
            "0 localFunc2() function",
            "1 localFuncNestedInLocalFunc2() function",
            "1 bobby variable",
            "1 e.baz() method",
            "0 d variable",
            "1 foo() function",
            "1 bar(a, b) function",
            "0 g variable",
            "0 A.B.C.doit() method",
            "0 A.B.C.doitAgain() function",
            "0 A.B.C.var field",
            "0 globalVar variable",
            "0 globalFunc2() function",
            "1 j variable",
            "0 globalTable variable",
            "1 a() function",
            "0 globalC2 variable",
            "0 xyz variable",
            "0 <anonymous>() function",
            "1 a variable",
            "1 F variable",
            "1 A.x() function",
            "2 x variable",
        ]
    );
}

#[test]
#[ignore = "measurement: CFXLUA_FILES=<file>;<file> cargo test --release --lib editor_cost_of_real_files -- --ignored --nocapture"]
fn editor_cost_of_real_files() {
    let files = std::env::var("CFXLUA_FILES").expect("CFXLUA_FILES");
    for path in files.split(';').filter(|path| !path.is_empty()) {
        let text = std::fs::read_to_string(path).expect("a readable file");
        let longest = text.lines().map(str::len).max().unwrap_or(0);
        let fx = fixture(&text, "");
        let feature = EditorFeature::default();
        fx.index(&feature);
        let timed = |what: &str, run: &dyn Fn() -> usize| {
            let start = std::time::Instant::now();
            let count = run();
            eprintln!("  {what}: {count} in {:?}", start.elapsed());
        };
        eprintln!("{} bytes, {} lines, longest line {longest} bytes", text.len(), text.lines().count());
        timed("parse (nodes)", &|| crate::lua::tree::Tree::parse(&text).len());
        let doc = fx.doc(CLIENT);
        let cx = fx.cx(&doc);
        timed("tree + scopes (declarations)", &|| doc.src.scopes().decls().len());
        timed("semantic tokens", &|| feature.semantic_tokens(&cx).len());
        let tokens = feature.semantic_tokens(&cx);
        timed("their ranges", &|| {
            tokens.iter().filter(|token| doc.range(token.start, token.end).end.character < u32::MAX).count()
        });
        timed("folds", &|| feature.folding(&cx).len());
        timed("structure", &|| feature.symbols(&cx).len());
        timed("syntax problems", &|| crate::syntax::check(&doc.src).len());
    }
}

#[test]
#[ignore = "measurement: CFXLUA_PROJECT=<dir> cargo test --release --lib syntax_problems_of_a_real_project -- --ignored --nocapture"]
fn syntax_problems_of_a_real_project() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if name.ends_with(".lua") {
                out.push(path);
            }
        }
    }
    let root = std::path::PathBuf::from(std::env::var("CFXLUA_PROJECT").expect("CFXLUA_PROJECT"));
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    let (mut files, mut flagged) = (0, 0);
    let mut by_message: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.len() > crate::workspace::MAX_FILE || crate::project::is_escrowed(&text) {
            continue;
        }
        files += 1;
        let src = source(&text);
        let problems: Vec<_> =
            crate::syntax::check(&src).into_iter().filter(|p| p.code == crate::syntax::errors::CODE).collect();
        if problems.is_empty() {
            continue;
        }
        flagged += 1;
        if std::env::var("CFXLUA_SHAPES").is_ok() {
            // The kinds of the tokens around each problem, never their text.
            let tokens = src.tree().tokens();
            for problem in &problems {
                let at = tokens.at(problem.start).unwrap_or(0);
                let around: Vec<&str> = (at.saturating_sub(6)..(at + 8).min(tokens.len()))
                    .filter(|i| tokens.kind(*i) != crate::lua::token::T::Whitespace)
                    .map(|i| tokens.kind(i).name())
                    .collect();
                eprintln!("  line {}: {}", src.lines.line_of(problem.start) + 1, around.join(" "));
            }
        }
        for problem in &problems {
            // The symbol of an "Unexpected symbol" is a piece of the file: only the kind of error is kept.
            let message = problem.message.split('\'').next().unwrap_or_default().trim().to_owned();
            *by_message.entry(message).or_default() += 1;
        }
    }
    eprintln!("{files} files, {flagged} with syntax problems: {by_message:?}");
}
