use super::{Require, lua_file_name, moved, renamed, required, requires, searched_word};
use crate::lua::Source;
use crate::manifest::edits::apply;
use crate::testing::Fixture;

fn loaders() -> Vec<String> {
    vec!["require".to_owned()]
}

fn text(fx: &Fixture, rel: &str) -> String {
    fx.ws.text(&Fixture::path(rel)).unwrap_or_else(|| panic!("no fixture file {rel}")).to_owned()
}

fn after_rename(fx: &Fixture, rel: &str, file: &str, name: &str) -> String {
    let text = text(fx, rel);
    let src = Source::new(&text);
    let edits = renamed(&fx.ws, (&Fixture::path(rel), &src), &loaders(), &Fixture::path(file), name);
    apply(&text, &edits)
}

fn after_move(fx: &Fixture, rel: &str, file: &str, to: &str) -> String {
    let text = text(fx, rel);
    let src = Source::new(&text);
    let edits = moved(&fx.ws, (&Fixture::path(rel), &src), &loaders(), &Fixture::path(file), &Fixture::path(to));
    apply(&text, &edits)
}

#[test]
fn renaming_a_file_renames_its_require_with_the_same_quotes() {
    let fx = Fixture::new(&[("A.lua", "print(\"a\")"), ("B.lua", "require('A')")]);
    assert_eq!(after_rename(&fx, "B.lua", "A.lua", "C.lua"), "require('C')");
}

#[test]
fn moving_a_file_rewrites_its_require_from_the_project() {
    let fx = Fixture::new(&[("A.lua", ""), ("B.lua", "require(\"A\")"), ("to/C.lua", "")]);
    assert_eq!(after_move(&fx, "B.lua", "A.lua", "to/A.lua"), "require(\"to.A\")");
}

#[test]
fn renaming_keeps_the_folders_written_before_the_name() {
    let fx = Fixture::new(&[("a/b/A.lua", ""), ("B.lua", "local m = require('a.b.A')\n")]);
    assert_eq!(after_rename(&fx, "B.lua", "a/b/A.lua", "C.lua"), "local m = require('a.b.C')\n");
}

#[test]
fn renaming_an_init_file_names_it_after_its_folder() {
    let fx = Fixture::new(&[("x/init.lua", ""), ("B.lua", "require 'x'\nrequire 'x.init'\n")]);
    assert_eq!(after_rename(&fx, "B.lua", "x/init.lua", "main.lua"), "require 'x.main'\nrequire 'x.main'\n");
}

#[test]
fn moving_an_init_file_names_its_new_folder() {
    let fx = Fixture::new(&[("x/init.lua", ""), ("B.lua", "require('x')")]);
    assert_eq!(after_move(&fx, "B.lua", "x/init.lua", "lib/x/init.lua"), "require('lib.x')");
}

#[test]
fn a_require_without_parentheses_is_renamed() {
    let fx = Fixture::new(&[("A.lua", ""), ("B.lua", "require \"A\"")]);
    assert_eq!(after_rename(&fx, "B.lua", "A.lua", "C.lua"), "require \"C\"");
}

#[test]
fn only_the_file_a_require_loads_follows_among_two_of_the_same_name() {
    let fx = Fixture::new(&[
        ("one/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("one/utils.lua", ""),
        ("one/client.lua", "require('utils')"),
        ("two/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("two/utils.lua", ""),
        ("two/client.lua", "require('utils')"),
    ]);
    assert_eq!(after_rename(&fx, "one/client.lua", "one/utils.lua", "helpers.lua"), "require('helpers')");
    assert_eq!(after_rename(&fx, "two/client.lua", "one/utils.lua", "helpers.lua"), "require('utils')");
}

#[test]
fn a_move_writes_the_path_from_the_resource_of_the_requiring_file() {
    let fx = Fixture::new(&[
        ("res/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("res/shared/utils.lua", ""),
        ("res/client/main.lua", "require('shared.utils')"),
    ]);
    assert_eq!(
        after_move(&fx, "res/client/main.lua", "res/shared/utils.lua", "res/lib/utils.lua"),
        "require('lib.utils')"
    );
}

#[test]
fn a_move_keeps_the_root_a_path_was_written_from() {
    let fx = Fixture::new(&[("a/A.lua", ""), ("a/b/B.lua", "require('a.A')\nrequire('A')")]);
    assert_eq!(
        after_move(&fx, "a/b/B.lua", "a/A.lua", "a/b/c/A.lua"),
        "require('a.b.c.A')\nrequire('c.A')",
        "from the project as before; the one found by its name alone takes the shortest path"
    );
    let fx = Fixture::new(&[("a/b/A.lua", ""), ("a/b/B.lua", "require('A')\nrequire('a.b.A')")]);
    assert_eq!(
        after_move(&fx, "a/b/B.lua", "a/b/A.lua", "a/b/c/A.lua"),
        "require('c.A')\nrequire('a.b.c.A')",
        "from the folder of the file that asks, and from the project"
    );
    assert_eq!(
        after_move(&fx, "a/b/B.lua", "a/b/A.lua", "a/A.lua"),
        "require('a.A')\nrequire('a.A')",
        "out of the folder it was written from: the shortest path that loads it again"
    );
}

#[test]
fn a_path_written_with_slashes_keeps_its_slashes() {
    let fx = Fixture::new(&[("a/b/A.lua", ""), ("B.lua", "require('a/b/A')")]);
    assert_eq!(after_rename(&fx, "B.lua", "a/b/A.lua", "C.lua"), "require('a/b/C')");
    assert_eq!(after_move(&fx, "B.lua", "a/b/A.lua", "c/A.lua"), "require('c/A')");
}

#[test]
fn a_string_with_an_escape_is_left_alone() {
    let fx = Fixture::new(&[("A.lua", ""), ("B.lua", "require('\\65')")]);
    assert_eq!(required(&fx.ws, &Fixture::path("B.lua"), "A").as_deref(), Some("/p/A.lua"));
    assert_eq!(after_rename(&fx, "B.lua", "A.lua", "C.lua"), "require('\\65')");
}

#[test]
fn a_computed_require_is_left_alone() {
    let fx = Fixture::new(&[("A.lua", ""), ("B.lua", "local name = 'A'\nrequire(name)\nrequire('A' .. '')\n")]);
    let unchanged = text(&fx, "B.lua");
    assert_eq!(after_rename(&fx, "B.lua", "A.lua", "C.lua"), unchanged);
    assert_eq!(after_move(&fx, "B.lua", "A.lua", "to/A.lua"), unchanged);
}

#[test]
fn a_file_nobody_requires_gives_no_edit() {
    let fx = Fixture::new(&[("A.lua", ""), ("D.lua", ""), ("B.lua", "require('A')")]);
    assert_eq!(after_rename(&fx, "B.lua", "D.lua", "C.lua"), "require('A')");
    assert_eq!(after_move(&fx, "B.lua", "D.lua", "to/D.lua"), "require('A')");
}

#[test]
fn a_path_written_from_the_resource_stays_written_from_it() {
    let fx = Fixture::new(&[
        ("res/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("res/A.lua", ""),
        ("res/client/B.lua", "require('A')"),
    ]);
    let src = Source::new(&text(&fx, "res/client/B.lua"));
    let from = Fixture::path("res/client/B.lua");
    let (file, to) = (Fixture::path("res/A.lua"), Fixture::path("res/client/A.lua"));
    // `A` would still be found beside the file that asks, but the runtime reads a path from the resource.
    assert_eq!(moved(&fx.ws, (&from, &src), &loaders(), &file, &to), [(9, 10, "client.A".to_owned())]);
    assert_eq!(moved(&fx.ws, (&from, &src), &loaders(), &file, &file), Vec::new(), "a file that does not move");
}

#[test]
fn the_requires_of_a_file_give_the_inside_of_their_quotes() {
    let src = Source::new("require('a.b')\nrequire [[c]]\nfoo('d')\nrequire('e' .. f)\nrequire('\\x67')\nrequire('h");
    let found = requires(&src, &loaders());
    let expected = vec![
        Require { path: "a.b".to_owned(), start: 9, end: 12 },
        Require { path: "c".to_owned(), start: 25, end: 26 },
    ];
    assert_eq!(found, expected);
}

#[test]
fn a_require_is_found_in_the_resource_before_the_project() {
    let fx = Fixture::new(&[
        ("utils.lua", ""),
        ("res/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("res/utils.lua", ""),
        ("res/client/main.lua", ""),
        ("other/deep/tool.lua", ""),
    ]);
    let from = Fixture::path("res/client/main.lua");
    assert_eq!(required(&fx.ws, &from, "utils").as_deref(), Some("/p/res/utils.lua"));
    assert_eq!(required(&fx.ws, &from, "tool").as_deref(), Some("/p/other/deep/tool.lua"));
    assert_eq!(required(&fx.ws, &from, "missing"), None);
}

#[test]
fn the_searched_word_is_the_name_of_the_module() {
    assert_eq!(searched_word("/p/a/b/A.lua"), "A");
    assert_eq!(searched_word("/p/a/x/init.lua"), "x");
}

#[test]
fn a_lua_file_name_gets_its_extension() {
    assert_eq!(lua_file_name("C"), Ok("C.lua".to_owned()));
    assert_eq!(lua_file_name(" C.lua "), Ok("C.lua".to_owned()));
}

#[test]
fn a_lua_file_name_is_refused_when_empty_or_with_a_separator() {
    assert_eq!(lua_file_name(""), Err("Enter a file name"));
    assert_eq!(lua_file_name("   "), Err("Enter a file name"));
    assert_eq!(lua_file_name(".lua"), Err("Enter a file name"));
    assert_eq!(lua_file_name("a/C.lua"), Err("A file name cannot contain '/' or '\\'"));
    assert_eq!(lua_file_name("a\\C"), Err("A file name cannot contain '/' or '\\'"));
}
