use super::*;
use crate::editor::EditorFeature;
use crate::feature::{Doc, Feature};
use crate::natives::NativeSet;
use crate::project::Side;
use crate::settings::Settings;
use crate::testing::{Fixture, native};

fn found_with(text: &str, elsewhere: &[&str]) -> Vec<Problem> {
    problems(&Source::new(text), &|name| elsewhere.contains(&name))
}

/// What is found, as `line:name -> fix titles`.
fn found(text: &str, elsewhere: &[&str]) -> Vec<String> {
    let line = |at: usize| text[..at].matches('\n').count() + 1;
    found_with(text, elsewhere)
        .iter()
        .map(|p| {
            let titles: Vec<&str> = p.fixes.iter().map(|fix| fix.title.as_str()).collect();
            format!("{}:{} -> {}", line(p.start), &text[p.start..p.end], titles.join(", "))
        })
        .collect()
}

fn fixed(text: &str) -> String {
    let problems = found_with(text, &[]);
    let fix = problems.iter().flat_map(|p| &p.fixes).next().unwrap_or_else(|| panic!("no fix in {text:?}"));
    let mut out = text.to_owned();
    for (start, end, with) in fix.edits.iter().rev() {
        out.replace_range(*start..*end, with);
    }
    out
}

const APPEND: &str = "Append \"local\"";

#[test]
fn a_global_only_its_file_uses_can_be_local() {
    let text = "count = 0\nprint(count)";
    let problems = found_with(text, &[]);
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].code, "GlobalNameCanbeLocal");
    assert_eq!(problems[0].message, "Global name \"count\" can be local");
    assert_eq!(problems[0].severity, Severity::Warning);
    assert_eq!(&text[problems[0].start..problems[0].end], "count");
    assert_eq!(found(text, &[]), [format!("1:count -> {APPEND}")]);
    assert_eq!(fixed(text), "local count = 0\nprint(count)");
}

#[test]
fn a_global_nothing_reads_is_still_reported() {
    assert_eq!(found("unused = 1", &[]), [format!("1:unused -> {APPEND}")]);
}

#[test]
fn a_global_another_file_uses_is_not_reported() {
    assert!(found("count = 0\nprint(count)", &["count"]).is_empty());
}

#[test]
fn only_the_first_assignment_of_a_global_is_reported() {
    let text = "x = 1\nx = 2\nfunction f()\n x = 3\nend";
    assert_eq!(found(text, &[]), [format!("1:x -> {APPEND}")]);
}

#[test]
fn a_name_already_local_is_not_reported() {
    assert!(found("local x\nx = 1", &[]).is_empty());
    assert!(found("local function f() end\nf = nil", &[]).is_empty());
    assert!(found("function g(p)\n p = 1\nend", &[]).is_empty());
}

#[test]
fn a_global_function_is_not_reported_and_explains_a_later_assignment() {
    assert!(found("function x() end\nx()", &[]).is_empty());
    assert!(found("function x() end\nx = 1", &[]).is_empty());
    assert_eq!(found("x = 1\nfunction x() end", &[]), [format!("1:x -> {APPEND}")]);
}

#[test]
fn a_compound_assignment_declares_nothing() {
    assert!(found("x += 1", &[]).is_empty());
}

#[test]
fn nothing_is_reported_in_a_module_file() {
    assert!(found("---@module M\nx = 1", &[]).is_empty());
}

#[test]
fn several_names_take_local_together() {
    let text = "a, b = 1, 2";
    assert_eq!(found(text, &[]), [format!("1:a -> {APPEND}"), format!("1:b -> {APPEND}")]);
    assert_eq!(fixed(text), "local a, b = 1, 2");
    assert_eq!(fixed("a, b in t"), "local a, b in t");
}

#[test]
fn a_target_that_cannot_be_local_leaves_the_problem_without_fix() {
    assert_eq!(found("local t = {}\na, t.x = 1, 2", &[]), ["2:a -> "]);
    assert_eq!(found("local b\na, b = 1, 2", &[]), ["2:a -> "]);
    assert_eq!(found("a, b = 1, 2", &["b"]), ["1:a -> "]);
}

#[test]
fn an_assignment_in_a_function_is_reported_and_made_local_there() {
    let text = "function f()\n x = 1\nend\nprint(x)";
    assert_eq!(found(text, &[]), [format!("2:x -> {APPEND}")]);
    assert_eq!(fixed(text), "function f()\n local x = 1\nend\nprint(x)");
}

#[test]
fn an_assignment_out_of_sight_resolves_to_the_first_one_the_index_holds() {
    let two_functions = "function f()\n x = 1\nend\nfunction g()\n x = 2\nend";
    assert_eq!(found(two_functions, &[]), [format!("2:x -> {APPEND}")]);
    assert_eq!(found("function f()\n x = 1\nend\nx = 0", &[]), [format!("2:x -> {APPEND}")]);
    assert_eq!(found("CreateThread(function()\n x = 1\nend)\nx = 0", &[]), [format!("2:x -> {APPEND}")]);
}

#[test]
fn the_index_holds_no_assignment_made_in_a_loop() {
    assert_eq!(found("for i = 1, 2 do\n x = i\nend\nx = 0", &[]), [format!("4:x -> {APPEND}")]);
    let two_loops = "for i = 1, 2 do\n x = i\nend\nwhile true do\n x = 0\nend";
    assert_eq!(found(two_loops, &[]), [format!("2:x -> {APPEND}"), format!("5:x -> {APPEND}")]);
    let named_in_loop = "while true do\n function f()\n  x = 1\n end\nend\nx = 0";
    assert_eq!(found(named_in_loop, &[]), [format!("3:x -> {APPEND}")]);
}

#[test]
fn a_library_name_the_file_assigns_again_is_not_reported() {
    assert!(found("print = function() end", &["print"]).is_empty());
}

const MANIFEST: (&str, &str) = ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nshared_script '*.lua'\n");
const MAIN: &str = "shop/main.lua";

fn project(main: &str, other: &str) -> Fixture {
    Fixture::new(&[MANIFEST, (MAIN, main), ("shop/other.lua", other)])
        .with_settings(Settings::defaults().with(GLOBAL_LOCAL.setting, true.into()))
}

/// The names the editor reports in a document of the project, once the project is read.
fn reported(fx: &Fixture, doc: &Doc) -> Vec<String> {
    let feature = EditorFeature::default();
    fx.index(&feature);
    let found = feature.problems(&fx.cx(doc));
    found.iter().filter(|p| p.code == GLOBAL_LOCAL.code).map(|p| doc.text()[p.start..p.end].to_owned()).collect()
}

#[test]
fn the_editor_leaves_the_inspection_off_until_it_is_asked_for() {
    let fx = project("count = 0\nprint(count)", "");
    assert_eq!(reported(&fx, &fx.doc(MAIN)), ["count"]);
    let fx = fx.with_settings(Settings::defaults());
    assert!(reported(&fx, &fx.doc(MAIN)).is_empty());
}

#[test]
fn a_global_another_file_of_the_project_uses_stays_global() {
    let uses =
        ["print(count)", "count = 1", "function count() end", "count.total = 1", "print(_G.count)", "_G['count'] = 1"];
    for other in uses {
        let fx = project("count = 0", other);
        assert!(reported(&fx, &fx.doc(MAIN)).is_empty(), "{other}");
    }
    for other in [
        "local count = 1\nprint(count)",
        "print(t.count)",
        "print('count')",
        "-- count",
        "local _G = {}\nprint(_G.count)",
    ] {
        let fx = project("count = 0", other);
        assert_eq!(reported(&fx, &fx.doc(MAIN)), ["count"], "{other}");
    }
}

#[test]
fn a_name_of_a_library_stays_global() {
    let natives = vec![native(NativeSet::Gta5, "GetEntityCoords", Side::Client)];
    let fx = project("GetEntityCoords = nil\nCitizen = nil\nprint = nil\nmine = nil", "").with_natives(natives);
    assert_eq!(reported(&fx, &fx.doc(MAIN)), ["mine"]);
}

#[test]
fn nothing_is_reported_while_the_project_is_being_read() {
    let mut fx = project("count = 0", "");
    fx.ws.indexing = true;
    assert!(reported(&fx, &fx.doc(MAIN)).is_empty());
}

#[test]
fn the_text_being_typed_is_read_against_the_other_files_as_they_were_indexed() {
    let fx = project("count = 0", "shared = 1");
    let path = Fixture::path(MAIN);
    let typed = Doc::new(&fx.ws, &path, "lua", 2, "count = 0\nshared = 2\nfresh = 3");
    assert_eq!(reported(&fx, &typed), ["count", "fresh"]);
}
