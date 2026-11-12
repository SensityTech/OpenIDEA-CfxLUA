use super::*;
use crate::editor::EditorFeature;
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;

fn all(text: &str) -> Vec<Problem> {
    let src = Source::new(text);
    problems(&src, &|_| true)
}

fn shown(text: &str, problems: &[Problem]) -> Vec<String> {
    problems.iter().map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
}

/// What one inspection finds, as `text: message`.
fn found(text: &str, inspection: Inspection) -> Vec<String> {
    let kept: Vec<Problem> = all(text).into_iter().filter(|p| p.code == inspection.code).collect();
    shown(text, &kept)
}

fn fixes(text: &str) -> Vec<String> {
    all(text).iter().flat_map(|p| &p.fixes).map(|fix| fix.title.clone()).collect()
}

fn fixed(text: &str, title: &str) -> String {
    let problems = all(text);
    let fix = problems.iter().flat_map(|p| &p.fixes).find(|fix| fix.title == title);
    let fix = fix.unwrap_or_else(|| panic!("no fix '{title}' in {:?}", fixes(text)));
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut out = text.to_owned();
    for (start, end, with) in edits {
        out.replace_range(start..end, &with);
    }
    out
}

#[test]
fn a_local_nothing_refers_to_is_unused() {
    assert_eq!(found("local a = 1\nlocal b = 2\nprint(b)", UNUSED), ["a: Unused local : 'a'"]);
    assert_eq!(found("local a, b = f()\nprint(a)", UNUSED), ["b: Unused local : 'b'"]);
    // An assignment refers to the local, as it does for the JetBrains plugin.
    assert!(found("local a\na = 1", UNUSED).is_empty());
    assert!(found("local a = 1\na += 1", UNUSED).is_empty());
    assert!(found("local _ = f()\nlocal _, _ = g()", UNUSED).is_empty());
    assert!(found("total = 1\nfunction count() end", UNUSED).is_empty(), "globals are someone else's to read");
    let unused = &all("local a = 1")[0];
    assert_eq!((unused.severity, unused.unnecessary, unused.code), (Severity::Hint, true, "RemoveUnusedLocal"));
}

#[test]
fn parameters_and_loop_variables_nothing_reads_are_unused() {
    assert_eq!(found("function f(a, b) return b end", UNUSED), ["a: Unused parameter : 'a'"]);
    assert_eq!(found("local t = {}\nfunction t:m(a) return self end", UNUSED), ["a: Unused parameter : 'a'"]);
    assert_eq!(found("f(function(a, _) end)", UNUSED), ["a: Unused parameter : 'a'"]);
    assert_eq!(found("for k, v in pairs(t) do print(v) end", UNUSED), ["k: Unused loop variable : 'k'"]);
    assert_eq!(found("for i = 1, 3 do print(1) end", UNUSED), ["i: Unused loop variable : 'i'"]);
    assert!(found("function f(...) return ... end\nfor _ = 1, 3 do print(1) end", UNUSED).is_empty());
    // A `@param` names a parameter without reading it.
    assert_eq!(found("---@param a number\nfunction f(a) end", UNUSED), ["a: Unused parameter : 'a'"]);
}

#[test]
fn a_local_function_nothing_calls_is_unused() {
    assert_eq!(found("local function f() end", UNUSED), ["f: Unused local function : 'f'"]);
    assert!(found("local function f() end\nf()", UNUSED).is_empty());
    assert!(found("local function f() return f() end", UNUSED).is_empty(), "it refers to itself");
    assert!(found("local f\nfunction f() end", UNUSED).is_empty(), "the statement assigns the local");
}

#[test]
fn what_is_unused_is_removed_or_renamed() {
    assert_eq!(fixed("local a = 1\nlocal b = 2\nprint(b)", "Remove unused local 'a'"), "local b = 2\nprint(b)");
    assert_eq!(fixed("do\n    local a = 1\nend", "Remove unused local 'a'"), "do\nend");
    assert_eq!(fixed("local a = 1 print(2)", "Remove unused local 'a'"), " print(2)", "a line it shares is kept");
    assert_eq!(fixed("---@type number\nlocal a = 1\nprint(2)", "Remove unused local 'a'"), "print(2)");
    assert_eq!(fixed("local a = 1", "Rename to '_'"), "local _ = 1");
    assert_eq!(fixes("local a, b = f()\nprint(b)"), ["Rename to '_'"], "the statement declares another local");
    assert_eq!(fixed("local a, b = f()\nprint(b)", "Rename to '_'"), "local _, b = f()\nprint(b)");
    // `CfxInspectionsTest.testUnusedUnpackedLocalIsNotRenamed`: `local a in src` reads `src.a`.
    assert_eq!(
        fixes("local src = {}\nlocal a in src\nlocal b = 1"),
        ["Remove unused local 'a'", "Remove unused local 'b'", "Rename to '_'"]
    );
    assert_eq!(
        fixed("local function f()\n    return 1\nend\nprint(2)", "Remove unused local function : 'f'"),
        "print(2)"
    );
    assert_eq!(fixed("for k, v in pairs(t) do print(v) end", "Rename to '_'"), "for _, v in pairs(t) do print(v) end");
    assert_eq!(
        fixed("---@param a number\n---@param b number\nfunction f(a, b) return b end", "Rename to '_'"),
        "---@param _ number\n---@param b number\nfunction f(_, b) return b end"
    );
    assert_eq!(
        fixed("---@param a number\nlocal f = function(a) end\nf()", "Rename to '_'"),
        "---@param _ number\nlocal f = function(_) end\nf()"
    );
}

#[test]
fn a_local_that_hides_another_one_is_reported() {
    // `LocalNameShadowedTest.testLocalNameHidesPrevious`
    assert_eq!(
        found("local var1 = 1\nprint(var1)\nlocal var1 = \"123\"", SHADOWED),
        ["var1: Local name shadowed, 'var1' was previously defined on line 1"]
    );
    let line = |n: u32| format!("a: Local name shadowed, 'a' was previously defined on line {n}");
    assert_eq!(found("local a\nlocal function f(a) end", SHADOWED), [line(1)]);
    assert_eq!(found("local a\nfor a = 1, 2 do end", SHADOWED), [line(1)]);
    assert_eq!(found("local a\nfor _, a in pairs(t) do end", SHADOWED), [line(1)]);
    assert_eq!(found("local a\ndo\n local a\n do\n  local a\n end\nend", SHADOWED), [line(1), line(3)]);
    assert_eq!(found("local function a() end\nlocal function a() end", SHADOWED), [line(1)]);
    assert_eq!(found("function f(a, a) end", SHADOWED), [line(1)]);
    assert_eq!(found("for a = 1, 2 do\n local a\nend", SHADOWED), [line(1)]);
    assert_eq!(found("local a = 1\nlocal a = a", SHADOWED), [line(1)], "its value still reads the first one");
    assert!(found("local a, a = 1, 2", SHADOWED).is_empty(), "one statement declares both");
    assert!(found("for a, a in pairs(t) do end", SHADOWED).is_empty());
    assert!(found("do local a end\nlocal a", SHADOWED).is_empty(), "the first one is out of sight");
    assert!(found("a = 1\nlocal a", SHADOWED).is_empty(), "a global is not a local");
    assert!(found("local _\nlocal _", SHADOWED).is_empty());
    assert_eq!(all("local a\nlocal a\nprint(a)")[1].severity, Severity::Warning);
}

#[test]
fn a_local_given_nil_can_be_simpler() {
    assert_eq!(found("local a = nil", SIMPLIFY), ["nil: Local assignment can be simplified"]);
    assert_eq!(fixed("local a = nil\nprint(a)", "Simplify local assignment"), "local a\nprint(a)");
    assert_eq!(fixed("local a, b = nil\nprint(a, b)", "Simplify local assignment"), "local a, b\nprint(a, b)");
    assert_eq!(fixed("local a <const> = nil\nprint(a)", "Simplify local assignment"), "local a <const>\nprint(a)");
    assert!(found("local a = nil, 1\nlocal b = f()\nlocal c\nd = nil", SIMPLIFY).is_empty());
    let simpler = &all("local _ = nil")[0];
    assert_eq!((simpler.severity, simpler.unnecessary), (Severity::Hint, true));
}

#[test]
fn a_loop_or_a_block_that_does_nothing_has_an_empty_body() {
    assert_eq!(found("for _ = 1, 2 do end", EMPTY_BODY), ["for: Empty for body"]);
    assert_eq!(found("for _ in pairs(t) do end", EMPTY_BODY), ["for: Empty for body"]);
    assert_eq!(found("do end", EMPTY_BODY), ["do: Empty do body"]);
    assert_eq!(found("while f() do end", EMPTY_BODY), ["while: Empty while body"]);
    assert_eq!(found("do\n -- later\n local a = 1\n local function f() end\nend", EMPTY_BODY), ["do: Empty do body"]);
    assert!(found("repeat until f()\nif a then end\nfunction g() end\ndo f() end", EMPTY_BODY).is_empty());
    assert!(found("while a do", EMPTY_BODY).is_empty(), "a statement that is still being written");
    assert_eq!(
        found("for _ = 1, f(function() g() end) do end", EMPTY_BODY),
        ["for: Empty for body"],
        "the block is the one of the loop, not the first one found in it"
    );
    assert_eq!(fixed("f()\nfor _ = 1, 2 do\nend\ng()", "Remove empty for"), "f()\ng()");
    assert_eq!(fixed("while f() do end", "Remove empty while"), "");
    assert_eq!(all("do end")[0].severity, Severity::Warning);
}

#[test]
fn what_follows_a_return_in_its_block_is_unreachable() {
    let text = "function f(a)\n    if a then return 1 end\n    return 2\n    print(3)\n    return 4\nend";
    assert_eq!(found(text, UNREACHABLE), ["print(3): Unreachable statement", "return 4: Unreachable statement"]);
    assert_eq!(
        fixed("function f()\n    return 2\n    print(3)\nend", "Remove unreachable statement"),
        "function f()\n    return 2\nend"
    );
    assert!(found("if a then return end\nprint(1)\ndo return end", UNREACHABLE).is_empty());
    assert!(found("return {\n    a = 1,\n};", UNREACHABLE).is_empty(), "the semicolon ends the return");
}

#[test]
fn an_array_is_not_indexed_from_zero() {
    assert_eq!(found("local v = t[0]\nt.list[0] = 1", ZERO_INDEX), ["0: 0 index", "0: 0 index"]);
    assert_eq!(fixed("print(t[0])", "Replace with index 1"), "print(t[1])");
    assert!(found("print(t[1], t['0'], t[0.0], t[00], t[a][10], f(0))", ZERO_INDEX).is_empty());
}

/// `CfxInspectionsTest.testAllLuaInspectionsOnTheCorpus`: every inspection reads the CfxLua syntax, valid or
/// broken, and what its fixes replace is text of the file.
#[test]
fn the_inspections_read_the_whole_cfxlua_syntax() {
    let corpus = [
        include_str!("../lua/testdata/valid/cComment.lua"),
        include_str!("../lua/testdata/valid/compound.lua"),
        include_str!("../lua/testdata/valid/hash.lua"),
        include_str!("../lua/testdata/valid/inUnpack.lua"),
        include_str!("../lua/testdata/valid/lua54.lua"),
        include_str!("../lua/testdata/valid/safeNav.lua"),
        include_str!("../lua/testdata/valid/setConstructor.lua"),
        "a,b+=1\n(a+=1)\nx?\ni++\n++i\nz, x?.y = 1\nlocal a in {a=1}\nf() += 1\nt = { .end }",
        "local\nfor\nwhile do\nreturn return\nlocal function\nfunction(a, a",
    ];
    for text in corpus {
        for problem in all(text) {
            assert!(problem.start <= problem.end && text.get(problem.start..problem.end).is_some(), "{problem:?}");
            for (start, end, _) in problem.fixes.iter().flat_map(|fix| &fix.edits) {
                assert!(start <= end && text.get(*start..*end).is_some(), "{problem:?}");
            }
        }
    }
}

#[test]
fn an_inspection_is_asked_for() {
    let src = Source::new("local a = nil\nlocal a\nfor _ = 1, 2 do end\nreturn 1\nprint(t[0])");
    let codes = |wanted: &dyn Fn(Inspection) -> bool| {
        let mut codes: Vec<&str> = problems(&src, wanted).iter().map(|problem| problem.code).collect();
        codes.sort_unstable();
        codes.dedup();
        codes
    };
    let mut every: Vec<&str> = ALL.iter().map(|inspection| inspection.code).collect();
    every.sort_unstable();
    assert_eq!(codes(&|_| true), every);
    assert!(codes(&|_| false).is_empty());
    for inspection in ALL {
        assert_eq!(codes(&|asked| asked == inspection), [inspection.code]);
    }
}

#[test]
fn the_editor_reports_them_in_lua_files_unless_a_setting_says_no() {
    let fx = Fixture::new(&[
        ("res/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'"),
        ("res/client.lua", "local a = 1\nlocal a = 2"),
        ("res/web/app.js", "local a = 1"),
    ]);
    let feature = EditorFeature::default();
    let codes = |fx: &Fixture, rel: &str| -> Vec<&'static str> {
        feature.problems(&fx.cx(&fx.doc(rel))).iter().map(|problem| problem.code).collect()
    };
    assert_eq!(codes(&fx, "res/client.lua"), ["RemoveUnusedLocal", "RemoveUnusedLocal", "LocalNameShadowed"]);
    assert!(codes(&fx, "res/web/app.js").is_empty());
    assert!(codes(&fx, "res/fxmanifest.lua").is_empty());
    let fx = fx.with_settings(Settings::defaults().with(UNUSED.setting, false.into()));
    assert_eq!(codes(&fx, "res/client.lua"), ["LocalNameShadowed"]);
    let fx = fx.with_settings(Settings::defaults().with("enabled", false.into()));
    assert!(codes(&fx, "res/client.lua").is_empty());
}
