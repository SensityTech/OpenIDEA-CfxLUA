use super::hover;
use crate::events::editor::builtin_doc;
use crate::events::tests::{indexed, project};
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;

const SOLO: &str = "solo/client.lua";

/// The covered text and the page shown at `marker` shifted by `shift` in `rel`.
fn shown(fx: &Fixture, rel: &str, marker: &str, shift: usize) -> Option<(String, String)> {
    let doc = fx.doc(rel);
    let at = doc.text().find(marker).unwrap_or_else(|| panic!("{marker} in {rel}")) + shift;
    let found = hover(&indexed(fx), &fx.cx(&doc), at)?;
    let (start, end) = found.range;
    Some((doc.text()[start..end].to_owned(), found.markdown))
}

fn solo(text: &str) -> Fixture {
    project(&[
        ("solo/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\n"),
        (SOLO, text),
    ])
}

fn shown_at_caret(text: &str) -> Option<String> {
    let fx = solo(text);
    let (doc, at) = fx.caret(SOLO);
    hover(&indexed(&fx), &fx.cx(&doc), at).map(|h| h.markdown)
}

#[test]
fn a_network_event_of_the_project_lists_its_handlers_and_triggers() {
    let fx = project(&[]);
    let (range, page) = shown(&fx, "standard_res/client.lua", "'shop:buy'", 3).expect("a page");
    assert_eq!(range, "'shop:buy'");
    let page = page.replace('\\', "");
    for part in [
        "network event **shop:buy**",
        "1 handler, 1 trigger in this project",
        "standard_res/server.lua:2 (server, takes 2 values)",
        "standard_res/client.lua:12 (client, to the server, sends 2 values)",
        "**Safe for net:**",
    ] {
        assert!(page.contains(part), "{part} in {page}");
    }
}

#[test]
fn a_tuple_key_names_the_helper_that_computes_it() {
    let fx = project(&[]);
    let (range, page) = shown(&fx, "studio/client/main.lua", "'RequestModel'", 1).expect("a page");
    assert_eq!(range, "'RequestModel'");
    let page = page.replace('\\', "");
    for part in [
        "event **(Gamemode, RequestModel)**",
        "the helper computes the network name",
        "studio/server/main.lua:1 (server, takes 1 value, via SrvHelper.AddEventHandler)",
        "studio/client/main.lua:11 (client, to the server, sends 1 value, via TriggerServerEvent)",
    ] {
        assert!(page.contains(part), "{part} in {page}");
    }
}

#[test]
fn a_built_in_event_keeps_the_catalog_page_even_when_the_project_handles_it() {
    let fx = project(&[]);
    let (range, page) = shown(&fx, "standard_res/client.lua", "'onClientResourceStart'", 3).expect("a page");
    assert_eq!(range, "'onClientResourceStart'");
    let event = crate::docs::data::event("onClientResourceStart").expect("in the catalog");
    assert_eq!(page, builtin_doc(event));
    assert!(page.starts_with("event **onClientResourceStart**(resourceName)"), "{page}");
    assert!(!page.contains("in this project"), "{page}");
}

#[test]
fn a_built_in_event_name_given_to_an_event_function_shows_its_catalog_page() {
    let page = shown_at_caret("AddEventHandler('onResource<caret>Start', function(name) end)").expect("a page");
    assert!(page.contains("**onResourceStart**(resourceName)"), "{page}");
    assert!(page.contains("**Cancelable:**"), "{page}");
    let page = shown_at_caret("TriggerEvent(\"chat:addMes<caret>sage\")").expect("a page");
    assert!(page.contains("chat:addMessage"), "{page}");
}

#[test]
fn a_built_in_event_name_elsewhere_shows_nothing() {
    assert!(shown_at_caret("print('onResource<caret>Start')").is_none());
    assert!(shown_at_caret("AddEventHandler(name, 'onResource<caret>Start')").is_none());
}

#[test]
fn an_event_unknown_to_the_catalog_shows_the_page_of_the_project() {
    let page = shown_at_caret("AddEventHandler('myResource:no<caret>tBuiltIn', function() end)").expect("a page");
    assert!(page.replace('\\', "").starts_with("network event **myResource:notBuiltIn**"), "{page}");
    assert!(page.contains("Handled but never triggered in this project"), "{page}");
}

#[test]
fn a_nui_callback_event_shows_nothing() {
    assert!(shown_at_caret("AddEventHandler('__cfx_nui:cl<caret>ose', function(data, cb) cb('ok') end)").is_none());
}

#[test]
fn a_plain_string_shows_nothing() {
    assert!(shown_at_caret("print('shop:b<caret>uy')").is_none());
}

#[test]
fn the_string_is_found_from_its_opening_quote_to_right_after_it() {
    let fx = project(&[]);
    let rel = "standard_res/client.lua";
    let literal = "'shop:buy'";
    let doc = fx.doc(rel);
    let start = doc.text().find(literal).expect("the literal");
    let end = start + literal.len();
    let feature = indexed(&fx);
    let cx = fx.cx(&doc);
    for at in [start, start + 4, end - 1, end] {
        assert_eq!(hover(&feature, &cx, at).map(|h| h.range), Some((start, end)), "at {at}");
    }
    assert!(hover(&feature, &cx, start - 1).is_none());
}

#[test]
fn nothing_is_shown_when_the_plugin_is_off() {
    let fx = project(&[]).with_settings(Settings::defaults().with("enabled", Value::from(false)));
    assert!(shown(&fx, "standard_res/client.lua", "'shop:buy'", 3).is_none());
    assert!(shown(&fx, "standard_res/client.lua", "'onClientResourceStart'", 3).is_none());
}
