//! Acceptance tests of the original plugin (`CfxStateBag*Test`): a resource
//! whose server script writes player, entity and global keys.

use super::sites::{Kind, SERVER};
use super::{StateBagsFeature, key_doc};
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;

const MANIFEST: (&str, &str) = (
    "res/fxmanifest.lua",
    "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'",
);
const MAIN: (&str, &str) = ("res/server/main.lua", SERVER);

fn fixture(extra: &[(&str, &str)]) -> Fixture {
    let mut files = vec![MANIFEST];
    files.extend_from_slice(extra);
    Fixture::new(&files)
}

fn indexed(fx: &Fixture) -> StateBagsFeature {
    let f = StateBagsFeature::default();
    fx.index(&f);
    f
}

fn labels(fx: &Fixture, rel: &str) -> Vec<String> {
    let f = indexed(fx);
    let (doc, at) = fx.caret(rel);
    f.completion(&fx.cx(&doc), at, None).into_iter().map(|i| i.label).collect()
}

fn has(items: &[String], keys: &[&str]) -> bool {
    keys.iter().all(|k| items.iter().any(|i| i == k))
}

fn has_none(items: &[String], keys: &[&str]) -> bool {
    !keys.iter().any(|k| items.iter().any(|i| i == k))
}

#[test]
fn project_queries_merge_the_files_and_keep_the_kinds_apart() {
    let fx = fixture(&[
        MAIN,
        (
            "res/client/main.lua",
            "AddStateBagChangeHandler('watched', nil, function() end)\nprint(LocalPlayer.state.readOnly)",
        ),
    ]);
    let f = indexed(&fx);
    let projects = f.projects.borrow();
    let p = projects.get("/p").unwrap();
    let keys = |kind| p.known_keys(kind).into_iter().map(|(k, _)| k.to_owned()).collect::<Vec<_>>();
    assert_eq!(keys(Kind::Player), ["grade", "job", "test", "test2", "watched"]);
    assert_eq!(keys(Kind::Global), ["watched", "weather"]);
    assert_eq!(keys(Kind::Entity), ["fuel", "watched"]);
    let occurrences = p.occurrences("test2", Kind::Player);
    let write = super::writes(&occurrences)[0];
    assert_eq!(super::is_replicated(&fx.ws, write), Some(true));
    assert_eq!(crate::events::display_path(&fx.ws, &write.path), "res/server/main.lua");
}

#[test]
fn local_player_safe_navigation_completes_keys_set_on_the_server() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer?.state?.<caret>")]);
    let items = labels(&fx, "res/client/main.lua");
    assert!(has(&items, &["test", "test2", "job", "grade"]) && has_none(&items, &["weather", "fuel"]), "{items:?}");
    // code on the next line reads as the field name while typing
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer.state.<caret>\nprint(v)")]);
    assert!(has(&labels(&fx, "res/client/main.lua"), &["test", "grade"]));
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer.state.gr<caret>")]);
    assert_eq!(labels(&fx, "res/client/main.lua"), ["grade"]);
}

#[test]
fn presentation_shows_types_and_origin() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer.state.<caret>")]);
    let f = indexed(&fx);
    let (doc, at) = fx.caret("res/client/main.lua");
    let items = f.completion(&fx.cx(&doc), at, None);
    let detail = |k: &str| items.iter().find(|i| i.label == k).and_then(|i| i.detail.clone()).unwrap();
    assert_eq!(detail("test"), "integer · set in res/server/main.lua (replicated)");
    assert_eq!(detail("job"), "table · set in res/server/main.lua (local)");
    assert_eq!(detail("test2"), "string · set in res/server/main.lua (replicated)");
    assert!(items.iter().all(|i| i.replace.is_none()));
}

#[test]
fn methods_and_wrappers() {
    let fx = fixture(&[
        MAIN,
        ("res/client/main.lua", "LocalPlayer.state:<caret>"),
        ("res/server/other.lua", "local s = Player(source).state\nprint(s.<caret>)"),
        ("res/client/global.lua", "print(GlobalState.<caret>)"),
        ("res/client/entity.lua", "print(Entity(PlayerPedId()).state.<caret>)"),
    ]);
    assert!(labels(&fx, "res/client/main.lua").is_empty(), "no key after ':'");
    let player = labels(&fx, "res/server/other.lua");
    assert!(has(&player, &["test", "test2", "job", "grade"]) && has_none(&player, &["weather", "fuel"]), "{player:?}");
    assert_eq!(labels(&fx, "res/client/global.lua"), ["weather"]);
    assert_eq!(labels(&fx, "res/client/entity.lua"), ["fuel"]);
}

#[test]
fn string_keys_are_completed() {
    let fx = fixture(&[
        MAIN,
        ("res/client/bracket.lua", "print(LocalPlayer.state['te<caret>'])"),
        ("res/client/handler.lua", "AddStateBagChangeHandler('<caret>', nil, function() end)"),
        ("res/client/handler2.lua", "AddStateBagChangeHandler('<caret>', 'player:1', function() end)"),
        ("res/client/native.lua", "print(GetStateBagValue('global', '<caret>'))"),
    ]);
    let bracket = labels(&fx, "res/client/bracket.lua");
    assert!(has(&bracket, &["test", "test2"]) && has_none(&bracket, &["job"]), "{bracket:?}");
    assert!(has(&labels(&fx, "res/client/handler.lua"), &["test", "test2", "job", "grade", "weather", "fuel"]));
    let player = labels(&fx, "res/client/handler2.lua");
    assert!(has(&player, &["test"]) && has_none(&player, &["weather"]), "{player:?}");
    let global = labels(&fx, "res/client/native.lua");
    assert!(has(&global, &["weather"]) && has_none(&global, &["test"]), "{global:?}");
}

#[test]
fn set_key_argument_is_completed_and_replaced() {
    let fx = fixture(&[MAIN, ("res/server/set.lua", "Player(source).state:set('gr<caret>', 4, true)")]);
    let f = indexed(&fx);
    let (doc, at) = fx.caret("res/server/set.lua");
    let items = f.completion(&fx.cx(&doc), at, None);
    assert!(items.iter().all(|i| i.label != "weather"));
    let grade = items.iter().find(|i| i.label == "grade").unwrap();
    let range = grade.replace.unwrap();
    let (s, e) = (doc.offset(range.start), doc.offset(range.end));
    let text = doc.text();
    assert_eq!(
        format!("{}{}{}", &text[..s], grade.insert_text, &text[e..]),
        "Player(source).state:set('grade', 4, true)"
    );
}

#[test]
fn disabled_setting_turns_completion_off() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer.state.<caret>")])
        .with_settings(Settings::defaults().with("enabled", Value::from(false)));
    assert!(labels(&fx, "res/client/main.lua").is_empty());
}

// ---------------------------------------------------------------------------------------------
// navigation and documentation

const SECOND: (&str, &str) = (
    "res/server/second.lua",
    "AddEventHandler('playerJoining', function()\n    Player(source).state.test = 'joined'\nend)",
);
const HANDLERS: (&str, &str) = (
    "res/client/handlers.lua",
    "AddStateBagChangeHandler('test', nil, function(bagName, key, value) end)\nprint(LocalPlayer.state.test, GlobalState.weather)",
);

fn definitions(fx: &Fixture, rel: &str) -> Vec<String> {
    let f = indexed(fx);
    let (doc, at) = fx.caret(rel);
    let mut out: Vec<String> = f
        .definition(&fx.cx(&doc), at)
        .into_iter()
        .map(|l| format!("{}:{}", l.path.trim_start_matches("/p/"), l.range.start.line + 1))
        .collect();
    out.sort();
    out
}

#[test]
fn definition_goes_to_every_write() {
    let fx = fixture(&[MAIN, SECOND, HANDLERS, ("res/client/main.lua", "local v = LocalPlayer?.state?.te<caret>st")]);
    assert_eq!(definitions(&fx, "res/client/main.lua"), ["res/server/main.lua:3", "res/server/second.lua:2"]);
    let fx = fixture(&[MAIN, SECOND, HANDLERS, ("res/client/main.lua", "print(LocalPlayer.state['jo<caret>b'])")]);
    assert_eq!(definitions(&fx, "res/client/main.lua"), ["res/server/main.lua:6"]);
    let fx = fixture(&[
        MAIN,
        SECOND,
        HANDLERS,
        ("res/client/handler2.lua", "AddStateBagChangeHandler('wea<caret>ther', nil, function() end)"),
    ]);
    assert_eq!(definitions(&fx, "res/client/handler2.lua"), ["res/server/main.lua:8"]);
}

#[test]
fn definition_keeps_the_kinds_apart_and_skips_plain_tables() {
    let fx = fixture(&[
        MAIN,
        ("res/server/entity.lua", "Entity(1).state.test = 5"),
        ("res/client/main.lua", "print(Entity(PlayerPedId()).state.te<caret>st)"),
        ("res/client/plain.lua", "local t = { test = 1 }\nprint(t.te<caret>st)"),
        ("res/client/watched.lua", "print(LocalPlayer.state.wat<caret>ched)"),
        ("res/client/handler.lua", "AddStateBagChangeHandler('watched', nil, function() end)"),
    ]);
    assert_eq!(definitions(&fx, "res/client/main.lua"), ["res/server/entity.lua:1"]);
    assert!(definitions(&fx, "res/client/plain.lua").is_empty());
    assert_eq!(
        definitions(&fx, "res/client/watched.lua"),
        ["res/client/handler.lua:1"],
        "a key only watched leads to its handler"
    );
}

#[test]
fn documentation_describes_the_key() {
    let fx = fixture(&[MAIN, SECOND, HANDLERS]);
    let f = indexed(&fx);
    let projects = f.projects.borrow();
    let p = projects.get("/p").unwrap();
    let doc = key_doc(&fx.ws, p, "test", Kind::Player);
    for part in [
        "player state bag key **test**",
        "Player state bag key, shared by `LocalPlayer.state` and `Player(id).state`.",
        "**Type:** integer | string",
        "**Replicated:** yes",
        "res/server/main.lua:3 (server, player, integer, replicated)",
        "res/server/second.lua:2 (server, player, string, replicated)",
        "**Change handlers:** res/client/handlers.lua:1 (client)",
        "**Read in:** res/client/handlers.lua:2 (client, player)",
    ] {
        assert!(doc.contains(part), "{part} in {doc}");
    }
    let missing = key_doc(&fx.ws, p, "missing", Kind::Global);
    assert!(
        missing.contains("global state bag key **missing**") && missing.contains("Never set in this project."),
        "{missing}"
    );
}

// ---------------------------------------------------------------------------------------------
// the unknown-key check

/// The messages of the document's problems and the result of its first fix.
fn check(fx: &Fixture, rel: &str) -> (Vec<String>, Option<(String, String)>) {
    let f = indexed(fx);
    let doc = fx.doc(rel);
    let problems = f.problems(&fx.cx(&doc));
    let fixed = problems.first().and_then(|p| p.fixes.first()).map(|fix| {
        let (s, e, t) = &fix.edits[0];
        (fix.title.clone(), format!("{}{t}{}", &doc.text()[..*s], &doc.text()[*e..]))
    });
    (problems.into_iter().map(|p| p.message).collect(), fixed)
}

#[test]
fn typo_is_reported_and_fixed_with_the_closest_key() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "local v = LocalPlayer?.state?.tset")]);
    let (messages, fixed) = check(&fx, "res/client/main.lua");
    assert_eq!(messages, ["State bag key 'tset' is never set in this project"]);
    assert_eq!(fixed, Some(("Replace with 'test'".into(), "local v = LocalPlayer?.state?.test".into())));
}

#[test]
fn string_keys_are_reported_and_fixed() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "print(GetStateBagValue('global', 'wether'))")]);
    let (messages, fixed) = check(&fx, "res/client/main.lua");
    assert_eq!(messages, ["State bag key 'wether' is never set in this project"]);
    assert_eq!(fixed, Some(("Replace with 'weather'".into(), "print(GetStateBagValue('global', 'weather'))".into())));
    let fx = fixture(&[MAIN, ("res/client/main.lua", "print(LocalPlayer.state[\"jbo\"])")]);
    assert_eq!(check(&fx, "res/client/main.lua").1.map(|f| f.1).as_deref(), Some("print(LocalPlayer.state[\"job\"])"));
}

#[test]
fn keys_set_anywhere_watched_or_written_are_not_reported() {
    let fx = fixture(&[
        MAIN,
        ("res/client/handlers.lua", "AddStateBagChangeHandler('watched', nil, function() end)"),
        (
            "res/client/main.lua",
            "print(LocalPlayer.state.test, LocalPlayer.state['test2'], GlobalState.weather, Entity(1).state.fuel)\n\
             print(LocalPlayer.state.watched)\nprint(Entity(1).state.test)\nLocalPlayer.state.local1 = true\n\
             print(LocalPlayer.state.local1)\nlocal t = {}\nprint(t.unknown)",
        ),
    ]);
    assert!(check(&fx, "res/client/main.lua").0.is_empty());
}

#[test]
fn projects_without_any_write_are_not_checked() {
    let fx = fixture(&[("res/client/main.lua", "print(LocalPlayer.state.isLoggedIn)")]);
    assert!(check(&fx, "res/client/main.lua").0.is_empty());
}

#[test]
fn dynamic_writes_silence_the_bag_kind() {
    let fx = fixture(&[
        MAIN,
        ("res/server/dynamic.lua", "local function set(src, k, v) Player(src).state:set(k, v, true) end"),
        ("res/client/main.lua", "print(LocalPlayer.state.anything)"),
        ("res/client/global.lua", "print(GlobalState.other)"),
    ]);
    assert!(check(&fx, "res/client/main.lua").0.is_empty());
    assert_eq!(check(&fx, "res/client/global.lua").0, ["State bag key 'other' is never set in this project"]);
}

#[test]
fn disabled_setting_turns_the_check_off() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "print(LocalPlayer.state.tset)")])
        .with_settings(Settings::defaults().with("enabled", Value::from(false)));
    assert!(check(&fx, "res/client/main.lua").0.is_empty());
}

#[test]
fn reindexing_a_file_updates_the_project() {
    let fx = fixture(&[MAIN, ("res/client/main.lua", "print(GlobalState.weather)")]);
    let f = indexed(&fx);
    let path = Fixture::path("res/server/main.lua");
    f.index_file(&fx.ws, &path, crate::project::FileKind::Lua, &crate::lua::Source::new("GlobalState.rain = true"));
    f.indexed(&fx.ws);
    let doc = fx.doc("res/client/main.lua");
    let messages: Vec<String> = f.problems(&fx.cx(&doc)).into_iter().map(|p| p.message).collect();
    assert_eq!(messages, ["State bag key 'weather' is never set in this project"]);
    f.forget_file(&path);
    assert!(f.problems(&fx.cx(&doc)).is_empty(), "no write left: nothing checked");
}
