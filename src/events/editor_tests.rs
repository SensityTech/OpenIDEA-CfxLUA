use super::EventsFeature;
use super::tests::{PROJECT, db, indexed, project, tuple, wire};
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;

fn definitions(fx: &Fixture, feature: &EventsFeature, rel: &str, marker: &str, shift: usize) -> Vec<String> {
    let doc = fx.doc(rel);
    let at = doc.text().find(marker).unwrap_or_else(|| panic!("{marker} in {rel}")) + shift;
    let mut out: Vec<String> = feature
        .definition(&fx.cx(&doc), at)
        .into_iter()
        .map(|l| format!("{}:{}", l.path.trim_start_matches("/p/"), l.range.start.line + 1))
        .collect();
    out.sort();
    out
}

#[test]
fn definition_from_a_standard_trigger_to_its_handler_and_back() {
    let fx = project(&[]);
    let f = indexed(&fx);
    assert_eq!(definitions(&fx, &f, "standard_res/client.lua", "'shop:buy'", 3), ["standard_res/server.lua:2"]);
    assert_eq!(
        definitions(&fx, &f, "standard_res/server.lua", "AddEventHandler('shop:buy'", 18),
        ["standard_res/client.lua:12"]
    );
    // right after the literal
    let doc = fx.doc("standard_res/client.lua");
    let end = doc.text().find("'shop:buy'").unwrap() + "'shop:buy'".len();
    assert_eq!(f.definition(&fx.cx(&doc), end).len(), 1);
}

#[test]
fn definition_on_a_tuple_key_component() {
    let fx = project(&[]);
    let f = indexed(&fx);
    assert_eq!(definitions(&fx, &f, "studio/client/main.lua", "'RequestModel'", 1), ["studio/server/main.lua:1"]);
    assert_eq!(
        definitions(&fx, &f, "studio/client/main.lua", "'Gamemode', 'RequestModel'", 1),
        ["studio/server/main.lua:1"]
    );
    assert_eq!(definitions(&fx, &f, "studio/server/main.lua", "'RequestModel'", 1), ["studio/client/main.lua:11"]);
    assert_eq!(definitions(&fx, &f, "studio/server/main.lua", "'SpawnPlayer'", 1), ["studio/client/main.lua:1"]);
    assert_eq!(
        definitions(&fx, &f, "studio/client/main.lua", "'SpawnPlayer'", 1),
        ["standard_res/server.lua:13", "studio/server/main.lua:3"]
    );
    assert_eq!(definitions(&fx, &f, "standard_res/server.lua", "'ev_legacy_spawn'", 1), ["studio/client/main.lua:1"]);
}

#[test]
fn plain_strings_are_not_event_keys() {
    let fx = project(&[("standard_res/cl_other.lua", "print('shop:b<caret>uy')")]);
    let f = indexed(&fx);
    let (doc, at) = fx.caret("standard_res/cl_other.lua");
    assert!(f.definition(&fx.cx(&doc), at).is_empty());
}

#[test]
fn gutter_markers_count_handlers_and_triggers() {
    let fx = project(&[]);
    let f = indexed(&fx);
    let doc = fx.doc("standard_res/server.lua");
    let markers = f.line_markers(&fx.cx(&doc));
    let tips: Vec<&str> = markers.iter().map(|m| m.tooltip.as_str()).collect();
    assert!(
        tips.iter().any(|t| t.starts_with("1 trigger 'shop:buy'") && t.contains("standard_res/client.lua:12 (Client)")),
        "{tips:?}"
    );
    assert!(
        tips.iter().any(|t| t.starts_with("1 handler") && t.contains("standard_res/client.lua:1 (Client)")),
        "{tips:?}"
    );
    assert!(
        tips.iter().any(|t| t.starts_with("1 handler") && t.contains("studio/client/main.lua:1 (Client)")),
        "{tips:?}"
    );
    let handler = markers.iter().find(|m| m.tooltip.starts_with("1 trigger")).unwrap();
    assert_eq!((handler.line, handler.icon.as_deref()), (1, Some("implementing")));
    let target = handler.target.as_ref().unwrap();
    assert_eq!((target.path.as_str(), target.range.start.line), ("/p/standard_res/client.lua", 11));
    // the network registration of line 1 gets no marker
    assert!(!markers.iter().any(|m| m.line == 0));

    let doc = fx.doc("studio/client/main.lua");
    let tips: Vec<String> = f.line_markers(&fx.cx(&doc)).into_iter().map(|m| m.tooltip).collect();
    assert!(tips.iter().any(|t| t.starts_with("2 triggers ('Gamemode', 'SpawnPlayer')")), "{tips:?}");
}

#[test]
fn documentation_of_project_keys() {
    let fx = project(&[]);
    let f = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    let cx = fx.cx(&doc);
    let db = f.db(&cx).unwrap();
    let text = super::editor::key_doc(&cx, &db, &wire("shop:buy")).replace('\\', "");
    for part in [
        "network event **shop:buy**",
        "1 handler, 1 trigger in this project.",
        "standard_res/server.lua:2 (server, takes 2 values)",
        "standard_res/client.lua:12 (client, to the server, sends 2 values)",
        "**Safe for net:** Server",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    let text = super::editor::key_doc(&cx, &db, &tuple(&["Gamemode", "RequestModel"])).replace('\\', "");
    for part in [
        "event **(Gamemode, RequestModel)**",
        "the helper computes the network name",
        "studio/server/main.lua:1 (server, takes 1 value, via SrvHelper.AddEventHandler)",
        "studio/client/main.lua:11 (client, to the server, sends 1 value, via TriggerServerEvent)",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    let text = super::editor::key_doc(&cx, &db, &tuple(&["Gamemode", "SpawnPlayer"]));
    assert!(text.contains("**Aliases:** 'ev\\_legacy\\_spawn'"), "{text}");
}

fn labels(fx: &Fixture, rel: &str) -> Vec<String> {
    let f = indexed(fx);
    let (doc, at) = fx.caret(rel);
    f.completion(&fx.cx(&doc), at, None).into_iter().map(|i| i.label).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn tuple_components_through_the_redefined_trigger() {
    let fx = project(&[
        ("studio/client/c1.lua", "TriggerServerEvent('<caret>')"),
        ("studio/client/c2.lua", "TriggerServerEvent('Gamemode', '<caret>')"),
    ]);
    assert_eq!(sorted(labels(&fx, "studio/client/c1.lua")), ["Gamemode", "House", "Mixed"]);
    assert_eq!(sorted(labels(&fx, "studio/client/c2.lua")), ["RequestModel", "ShowNotification", "SpawnPlayer"]);
}

#[test]
fn helper_handlers_complete_the_names_of_the_type() {
    let fx = project(&[("studio/server/c3.lua", "SrvHelper.AddEventHandler('House', '<caret>', function(actor) end)")]);
    let f = indexed(&fx);
    let (doc, at) = fx.caret("studio/server/c3.lua");
    let items = f.completion(&fx.cx(&doc), at, None);
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Enter"]);
    assert_eq!(items[0].detail.as_deref(), Some("handled on server"));
    assert_eq!(items[0].replace, Some(doc.range(at, at)));
    assert!(items[0].documentation.as_deref().unwrap_or("").contains("event **(House, Enter)**"));
}

#[test]
fn method_triggers_and_aliases_complete_their_slots() {
    let fx = project(&[
        ("studio/server/c4.lua", "local zone = Zone.new('x')\nzone:TriggerEvent('Arena', 'House', '<caret>')"),
        ("studio/shared/c5.lua", "RegisterEventAlias('zz_x', 'Gamemode', 'Sp<caret>')"),
        ("studio/shared/c6.lua", "RegisterEventAlias('ev_<caret>', 'Gamemode', 'SpawnPlayer')"),
    ]);
    assert_eq!(labels(&fx, "studio/server/c4.lua"), ["Enter"]);
    assert_eq!(labels(&fx, "studio/shared/c5.lua"), ["SpawnPlayer"]);
    assert_eq!(labels(&fx, "studio/shared/c6.lua"), ["ev_legacy_spawn"]);
}

#[test]
fn standard_functions_offer_built_in_and_project_names() {
    let fx = project(&[
        ("standard_res/cl_c7.lua", "TriggerServerEvent('<caret>')"),
        ("studio/client/c8.lua", "MyHelper.AddEventHandler('<caret>', 'x', function() end)"),
    ]);
    let items = labels(&fx, "standard_res/cl_c7.lua");
    for name in ["playerDropped", "shop:buy", "ev_legacy_spawn"] {
        assert!(items.contains(&name.to_owned()), "{name} in {items:?}");
    }
    assert!(!items.contains(&"Gamemode".to_owned()));
    assert!(!items.contains(&"onClientResourceStop".to_owned()), "a built-in event of the other side: {items:?}");
    let unique: std::collections::HashSet<&String> = items.iter().collect();
    assert_eq!(unique.len(), items.len());
    let helper = labels(&fx, "studio/client/c8.lua");
    assert!(
        helper.contains(&"Gamemode".to_owned()) && !helper.contains(&"onClientResourceStart".to_owned()),
        "{helper:?}"
    );
}

const MYRES: &[(&str, &str)] = &[
    (
        "myres/fxmanifest.lua",
        "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nserver_script 'server.lua'",
    ),
    ("myres/server.lua", "RegisterNetEvent('myres:hello')\nRegisterNetEvent('myres:openMenu')"),
];

fn myres(extra: (&str, &str)) -> Fixture {
    let mut files = MYRES.to_vec();
    files.push(extra);
    Fixture::new(&files)
}

#[test]
fn built_in_events_by_side_and_game() {
    let client = labels(&myres(("myres/client.lua", "AddEventHandler('<caret>')")), "myres/client.lua");
    for name in ["onResourceStart", "onClientResourceStart", "gameEventTriggered"] {
        assert!(client.contains(&name.to_owned()), "{name} in {client:?}");
    }
    assert!(!client.iter().any(|n| n == "playerConnecting" || n.contains('*')), "{client:?}");
    let server = labels(&myres(("myres/sv_two.lua", "RegisterNetEvent(\"<caret>\")")), "myres/sv_two.lua");
    assert!(
        server.contains(&"playerConnecting".to_owned()) && server.contains(&"onResourceStart".to_owned()),
        "{server:?}"
    );
    assert!(!server.contains(&"onClientResourceStart".to_owned()));
    assert!(!server.iter().any(|n| n == "lightningEvent"), "RedM events in a FiveM project");
    let to_server = labels(&myres(("myres/client.lua", "TriggerServerEvent('<caret>')")), "myres/client.lua");
    assert!(to_server.contains(&"playerDropped".to_owned()));
    let to_client = labels(&myres(("myres/sv_extra.lua", "TriggerClientEvent('<caret>', -1)")), "myres/sv_extra.lua");
    assert!(to_client.contains(&"chat:addMessage".to_owned()));
}

#[test]
fn project_event_names_by_prefix() {
    let fx = myres(("myres/client.lua", "AddEventHandler('myres:hel<caret>')"));
    assert_eq!(labels(&fx, "myres/client.lua"), ["myres:hello"]);
    let fx = myres(("myres/client.lua", "AddEventHandler('myres:<caret>')"));
    assert_eq!(sorted(labels(&fx, "myres/client.lua")), ["myres:hello", "myres:openMenu"]);
    let fx = myres(("myres/client.lua", "TriggerEvent('chat:<caret>')"));
    let chat = labels(&fx, "myres/client.lua");
    assert!(
        chat.contains(&"chat:addMessage".to_owned()) && chat.contains(&"chat:addSuggestion".to_owned()),
        "{chat:?}"
    );
    let f = indexed(&fx);
    let (doc, at) = fx.caret("myres/client.lua");
    let item = f.completion(&fx.cx(&doc), at, None).into_iter().find(|i| i.label == "chat:addMessage").unwrap();
    let docs = item.documentation.unwrap();
    assert!(docs.contains("event **chat:addMessage**(message)") && docs.contains("**Cancelable:** No"), "{docs}");
}

#[test]
fn other_strings_are_not_completed() {
    assert!(labels(&myres(("myres/client.lua", "print('onResource<caret>')")), "myres/client.lua").is_empty());
    assert!(
        labels(&myres(("myres/client.lua", "AddEventHandler(name, 'onResource<caret>')")), "myres/client.lua")
            .is_empty()
    );
}

// ---------------------------------------------------------------------------------------------
// checks

fn problems(fx: &Fixture, rel: &str) -> Vec<String> {
    let f = indexed(fx);
    let doc = fx.doc(rel);
    f.problems(&fx.cx(&doc)).iter().map(|p| format!("{} @ {}", p.message, &doc.text()[p.start..p.end])).collect()
}

#[test]
fn the_synthetic_project_has_no_event_problem() {
    let fx = project(&[]);
    for (rel, _) in PROJECT.iter().filter(|(p, _)| !p.ends_with("fxmanifest.lua")) {
        assert_eq!(problems(&fx, rel), Vec::<String>::new(), "{rel}");
    }
}

#[test]
fn trigger_without_handler() {
    let fx = project(&[(
        "standard_res/cl_extra.lua",
        "TriggerServerEvent('shop:buyy', 'bread', 1)\nTriggerServerEvent('esx:getSharedObject')\nTriggerServerEvent('noNamespace')\n\
         TriggerServerEvent('shop:' .. GetCurrentResourceName())\nTriggerEvent('onClientResourceStart', 'x')",
    )]);
    assert_eq!(
        problems(&fx, "standard_res/cl_extra.lua"),
        ["Event 'shop:buyy' has no handler in this project @ 'shop:buyy'"]
    );
}

#[test]
fn tuple_trigger_without_handler() {
    let fx = project(&[(
        "studio/client/extra.lua",
        "TriggerServerEvent('Gamemode', 'RequestModle', 'adder')\nTriggerServerEvent('Elsewhere', 'Thing')",
    )]);
    assert_eq!(
        problems(&fx, "studio/client/extra.lua"),
        ["Event ('Gamemode', 'RequestModle') has no handler in this project @ 'Gamemode', 'RequestModle'"]
    );
}

#[test]
fn trigger_whose_handlers_run_on_the_other_side() {
    let fx = project(&[("standard_res/sv_extra.lua", "TriggerClientEvent('shop:purchased', -1, 1, 'bread', 2)")]);
    assert_eq!(
        problems(&fx, "standard_res/sv_extra.lua"),
        ["Event 'shop:purchased' has no client handler in this project @ 'shop:purchased'"]
    );
}

const NET_MANIFEST: (&str, &str) = (
    "net_res/fxmanifest.lua",
    "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nserver_script 'server.lua'",
);
const NET_CLIENT: (&str, &str) =
    ("net_res/client.lua", "TriggerServerEvent('net:ping', 1)\nTriggerServerEvent('net:pong', 1)");

#[test]
fn handler_of_a_remote_event_without_register_net_event() {
    let server = "AddEventHandler('net:ping', function(n) end)\nRegisterNetEvent('net:pong')\nAddEventHandler('net:pong', function(n) end)";
    let fx = project(&[NET_MANIFEST, NET_CLIENT, ("net_res/server.lua", server)]);
    let found = problems(&fx, "net_res/server.lua");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("Event 'net:ping' is triggered from the other side (net_res/client.lua:1)"),
        "{}",
        found[0]
    );
    assert!(found[0].ends_with("the runtime drops it (\"event was not safe for net\") @ 'net:ping'"), "{}", found[0]);

    let f = indexed(&fx);
    let doc = fx.doc("net_res/server.lua");
    let problem = f.problems(&fx.cx(&doc)).into_iter().next().unwrap();
    let fix = &problem.fixes[0];
    assert_eq!(fix.title, "Use RegisterNetEvent (registers the event for the network)");
    let (s, e, t) = &fix.edits[0];
    let fixed = format!("{}{t}{}", &server[..*s], &server[*e..]);
    assert!(fixed.starts_with("RegisterNetEvent('net:ping', function(n) end)"));
    let fx = project(&[NET_MANIFEST, NET_CLIENT, ("net_res/server.lua", &fixed)]);
    assert!(problems(&fx, "net_res/server.lua").is_empty());
}

#[test]
fn register_net_event_in_another_resource_does_not_make_the_event_safe() {
    let fx = project(&[
        NET_MANIFEST,
        ("net_res/client.lua", "TriggerServerEvent('net:ping', 1)"),
        ("net_res/server.lua", "AddEventHandler('net:ping', function(n) end)"),
        ("other_res/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nserver_script 'server.lua'"),
        ("other_res/server.lua", "RegisterNetEvent('net:ping')"),
    ]);
    assert_eq!(problems(&fx, "net_res/server.lua").len(), 1);
}

#[test]
fn event_functions_called_from_the_wrong_side() {
    let fx = project(&[
        (
            "standard_res/sv_wrong.lua",
            "TriggerServerEvent('shop:buy', 'bread', 1)\nTriggerClientEvent('shop:openMenu', -1, {}, 'Shop')",
        ),
        (
            "standard_res/cl_wrong.lua",
            "RegisterServerEvent('shop:buy')\nTriggerClientEvent('shop:openMenu', -1, {}, 'Shop')",
        ),
        ("standard_res/sh_ok.lua", "TriggerServerEvent('shop:buy', 'bread', 1)"),
    ]);
    assert_eq!(
        problems(&fx, "standard_res/sv_wrong.lua"),
        ["'TriggerServerEvent' must be called from a client script, this is a server script @ TriggerServerEvent"]
    );
    let client = problems(&fx, "standard_res/cl_wrong.lua");
    for expected in [
        "'RegisterServerEvent' must be called from a server script, this is a client script @ RegisterServerEvent",
        "'TriggerClientEvent' must be called from a server script, this is a client script @ TriggerClientEvent",
    ] {
        assert!(client.contains(&expected.to_owned()), "{client:?}");
    }
    assert!(problems(&fx, "standard_res/sh_ok.lua").is_empty());
}

const ARITY: &str = "TriggerServerEvent('shop:buy', 'bread')\nTriggerServerEvent('shop:buy', 'bread', 2)\n\
    TriggerServerEvent('shop:buy', 'bread', 2, true)\nlocal values = { 'bread', 2 }\n\
    TriggerServerEvent('shop:buy', table.unpack(values))\nTriggerServerEvent('shop:buy', GetValues())";

#[test]
fn values_sent_and_taken() {
    let fx = project(&[("standard_res/cl_arity.lua", ARITY)]);
    assert_eq!(
        problems(&fx, "standard_res/cl_arity.lua"),
        [
            "Event 'shop:buy' sends 1 value but its handler at standard_res/server.lua:2 takes 2 @ 'shop:buy'",
            "Event 'shop:buy' sends 3 values but its handler at standard_res/server.lua:2 takes 2 @ 'shop:buy'",
        ]
    );
    let fx = project(&[("standard_res/cl_arity.lua", ARITY)])
        .with_settings(Settings::defaults().with(super::keys::REPORT_IGNORED_VALUES, Value::from(false)));
    assert_eq!(
        problems(&fx, "standard_res/cl_arity.lua"),
        ["Event 'shop:buy' sends 1 value but its handler at standard_res/server.lua:2 takes 2 @ 'shop:buy'"]
    );
}

#[test]
fn values_of_tuple_keys_exclude_the_injected_ones() {
    let fx = project(&[(
        "studio/client/arity.lua",
        "TriggerServerEvent('Gamemode', 'RequestModel', 'adder', 'extra')\nTriggerServerEvent('House', 'Enter', 12)\n\
         MyHelper.TriggerServerEvent('House', 'Enter')",
    )]);
    assert_eq!(
        problems(&fx, "studio/client/arity.lua"),
        [
            "Event ('Gamemode', 'RequestModel') sends 2 values but its handler at studio/server/main.lua:1 takes 1 @ 'Gamemode', 'RequestModel'",
            "Event ('House', 'Enter') sends no value but its handler at studio/server/main.lua:6 takes 1 @ 'House', 'Enter'",
        ]
    );
}

#[test]
fn duplicate_handlers_in_a_resource() {
    let fx = project(&[
        (
            "standard_res/sv_extra.lua",
            "AddEventHandler('shop:buy', function(item, amount) end)\nAddEventHandler('onResourceStart', function() end)\n\
             AddEventHandler('shop:never', function() end)\nAddEventHandler('shop:never', function() end)",
        ),
        ("standard_res/sv_more.lua", "AddEventHandler('onResourceStart', function() end)"),
    ]);
    assert_eq!(
        problems(&fx, "standard_res/sv_extra.lua"),
        ["Event 'shop:buy' already has a handler in this resource and side (standard_res/server.lua:2) @ 'shop:buy'"]
    );
    assert!(problems(&fx, "standard_res/server.lua").is_empty());
}

#[test]
fn computed_names_stay_silent() {
    let fx = project(&[
        (
            "dyn_res/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nserver_script 'server.lua'",
        ),
        (
            "dyn_res/server.lua",
            "local names = { 'open', 'close' }\nfor _, name in ipairs(names) do\n    RegisterNetEvent('dyn:' .. name)\nend\n\
             AddEventHandler('dyn:open', function() end)\nAddEventHandler(GetCurrentResourceName() .. ':ready', function() end)",
        ),
        (
            "dyn_res/client.lua",
            "TriggerServerEvent('dyn:open')\nTriggerServerEvent('dyn:other', 1, 2, 3)\n\
             local event = math.random() > 0.5 and 'dyn:a' or 'dyn:b'\nTriggerServerEvent(event)\nTriggerServerEvent('dyn_res:ready')",
        ),
    ]);
    assert!(problems(&fx, "dyn_res/server.lua").is_empty());
    assert!(problems(&fx, "dyn_res/client.lua").is_empty());
    assert!(!db(&fx).dynamic.is_empty());
}

#[test]
fn mistakes_in_annotations_are_reported() {
    let fx = project(&[(
        "studio/server/annotations.lua",
        "---@cfx.event handler key(eType, eKind) callback(cb)\nfunction SrvHelper.A(eType, eName, cb) end\n\
         ---@cfx.event listener key(1)\nfunction SrvHelper.B(name) end\n\
         ---@cfx.event trigger client key(1) payload(...)\nSrvHelper.C = function(name, ...) end\n\
         ---@cfx.event handler\nlocal value = 1",
    )]);
    assert_eq!(
        problems(&fx, "studio/server/annotations.lua"),
        [
            "Unknown parameter 'eKind' @ cfx.event handler key(eType, eKind) callback(cb)",
            "Unknown event kind: start with handler, trigger, register or alias @ cfx.event listener key(1)",
            "---@cfx.event must document a function @ cfx.event handler",
        ]
    );
}

#[test]
fn disabled_or_unscanned_projects_are_silent() {
    let fx = project(&[("standard_res/cl_extra.lua", "TriggerServerEvent('shop:buyy')")])
        .with_settings(Settings::defaults().with("enabled", Value::from(false)));
    assert!(problems(&fx, "standard_res/cl_extra.lua").is_empty());
    let mut fx = project(&[]);
    fx.ws.scanned = false;
    let f = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    assert!(f.line_markers(&fx.cx(&doc)).is_empty());
}
