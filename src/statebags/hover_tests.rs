use super::hover;
use crate::settings::Settings;
use crate::statebags::tests::{HANDLERS, MAIN, SECOND, fixture, indexed};
use crate::testing::Fixture;
use serde_json::Value;

const CLIENT: &str = "res/client/main.lua";

fn shown_in(fx: &Fixture) -> Option<(String, String)> {
    let (doc, at) = fx.caret(CLIENT);
    let found = hover(&indexed(fx), &fx.cx(&doc), at)?;
    let (start, end) = found.range;
    Some((doc.text()[start..end].to_owned(), found.markdown))
}

fn shown(text: &str) -> Option<(String, String)> {
    shown_in(&fixture(&[MAIN, SECOND, HANDLERS, (CLIENT, text)]))
}

#[test]
fn a_player_key_lists_its_types_and_where_it_is_set_watched_and_read() {
    let (range, page) = shown("local v = LocalPlayer.state.te<caret>st").expect("a page");
    assert_eq!(range, "test");
    for part in [
        "player state bag key **test**",
        "**Type:** integer | string",
        "res/server/main.lua:3 (server, player, integer, replicated)",
        "res/server/second.lua:2 (server, player, string, replicated)",
        "**Change handlers:** res/client/handlers.lua:1 (client)",
        "**Read in:**",
    ] {
        assert!(page.contains(part), "{part} in {page}");
    }
}

#[test]
fn a_key_never_set_says_so() {
    let (range, page) = shown("local v = GlobalState.mis<caret>sing").expect("a page");
    assert_eq!(range, "missing");
    assert!(page.starts_with("global state bag key **missing**"), "{page}");
    assert!(page.contains("Never set in this project."), "{page}");
}

#[test]
fn a_key_written_as_a_name_or_as_a_string_shows_the_same_page() {
    let (name_range, by_name) = shown("local v = LocalPlayer.state.te<caret>st").expect("a page");
    let (string_range, by_string) = shown("local v = LocalPlayer.state['te<caret>st']").expect("a page");
    assert_eq!((name_range.as_str(), string_range.as_str()), ("test", "'test'"));
    assert_eq!(by_name, by_string);
    let (_, by_call) = shown("LocalPlayer.state:set('te<caret>st', 2, true)").expect("a page");
    assert!(by_call.starts_with("player state bag key **test**"), "{by_call}");
}

#[test]
fn a_plain_table_field_shows_nothing() {
    assert!(shown("local t = { test = 1 }\nprint(t.te<caret>st)").is_none());
}

#[test]
fn nothing_is_shown_when_the_plugin_is_off() {
    let fx = fixture(&[MAIN, SECOND, HANDLERS, (CLIENT, "local v = LocalPlayer.state.te<caret>st")])
        .with_settings(Settings::defaults().with("enabled", Value::from(false)));
    assert!(shown_in(&fx).is_none());
}
