use super::anchors;
use crate::events::model::STANDARD_NAMES;
use crate::lua::Source;
use crate::refactor::edges::Family;

const CLIENT: &str = "RegisterNetEvent('shop:openMenu', function(items, title)
    for _, item in ipairs(items) do
        print(title, item)
    end
end)

AddEventHandler('onClientResourceStart', function(resourceName)
    print('started', resourceName)
end)

RegisterCommand('buy', function(_, args)
    TriggerServerEvent('shop:buy', args[1], tonumber(args[2]))
end, false)
";

const SERVER: &str = "RegisterNetEvent('shop:buy')
AddEventHandler('shop:buy', function(item, amount)
    local src = source
    TriggerClientEvent('shop:openMenu', src, { item }, 'Shop')
    TriggerEvent('shop:purchased', src, item, amount)
end)

AddEventHandler('shop:purchased', function(src, item, amount)
    print(src, item, amount)
end)

RegisterCommand('respawnall', function()
    TriggerClientEvent('ev_legacy_spawn', -1, vector3(0.0, 0.0, 72.0), 0.0)
end, true)
";

/// Each anchor of a text: its key and the text it covers.
fn written(text: &str) -> Vec<(String, String)> {
    anchors(&Source::new(text))
        .into_iter()
        .map(|anchor| (anchor.key, text[anchor.start..anchor.end].to_owned()))
        .collect()
}

fn keys(text: &str) -> Vec<String> {
    written(text).into_iter().map(|(key, _)| key).collect()
}

#[test]
fn find_usages_of_an_event_gathers_its_sites_on_both_sides() {
    let mut usages = Vec::new();
    for (path, text) in [("standard_res/client.lua", CLIENT), ("standard_res/server.lua", SERVER)] {
        let src = Source::new(text);
        for anchor in anchors(&src).into_iter().filter(|anchor| anchor.key == "cfx.event#shop:buy") {
            assert_eq!(anchor.family, Family::Cfx);
            assert_eq!(&text[anchor.start..anchor.end], "shop:buy");
            usages.push(format!("{path}:{}", src.lines.line_of(anchor.start) + 1));
        }
    }
    assert_eq!(usages, ["standard_res/client.lua:12", "standard_res/server.lua:1", "standard_res/server.lua:2"]);
}

#[test]
fn every_event_name_of_a_file_is_anchored_in_order() {
    assert_eq!(
        keys(SERVER),
        [
            "cfx.event#shop:buy",
            "cfx.event#shop:buy",
            "cfx.event#shop:openMenu",
            "cfx.event#shop:purchased",
            "cfx.event#shop:purchased",
            "cfx.event#ev_legacy_spawn",
        ]
    );
}

#[test]
fn every_standard_event_function_names_its_event_first() {
    for name in STANDARD_NAMES {
        let text = format!("{name}('shop:buy', -1, 'x')\n");
        assert_eq!(written(&text), [("cfx.event#shop:buy".to_owned(), "shop:buy".to_owned())], "{name}");
    }
}

#[test]
fn both_quotes_and_long_strings_give_the_same_key() {
    let text = "TriggerEvent('a:b')\nTriggerEvent(\"a:b\")\nTriggerEvent([[a:b]])\nTriggerEvent([==[a:b]==])\n";
    let expected = ("cfx.event#a:b".to_owned(), "a:b".to_owned());
    assert_eq!(written(text), [expected.clone(), expected.clone(), expected.clone(), expected]);
}

#[test]
fn a_computed_or_concatenated_name_gives_no_anchor() {
    let text = "local name = 'shop:buy'
TriggerEvent(name)
TriggerEvent('shop:' .. 'buy')
TriggerEvent(prefix .. ':buy')
TriggerServerEvent(GetCurrentResourceName() .. ':ready')
TriggerEvent(('%s:buy'):format('shop'))
";
    assert_eq!(written(text), []);
}

#[test]
fn a_name_with_an_escape_gives_no_anchor() {
    let text = r#"TriggerEvent('shop\58buy')
TriggerEvent("shop:\"buy\"")
AddEventHandler('shop\x3abuy', function() end)
TriggerEvent('')
"#;
    assert_eq!(written(text), []);
}

#[test]
fn nui_callbacks_are_left_to_the_nui_bridge() {
    let text = "AddEventHandler('__cfx_nui:close', function(data, cb) cb('ok') end)
RegisterNUICallback('close', function(data, cb) cb('ok') end)
TriggerEvent('__cfx_nui:open')
";
    assert_eq!(written(text), []);
}

#[test]
fn the_event_functions_of_a_project_give_no_anchor() {
    let text = "MyHelper.AddEventHandler('Mixed', 'Refresh', function(data) end)
SrvHelper.TriggerClientEvent('Mixed', 'Refresh', -1, {})
zone:TriggerEvent('zone:entered')
local function emit(name, ...)
    TriggerServerEvent(name, ...)
end
emit('shop:buy', 1)
";
    assert_eq!(written(text), []);
}

#[test]
fn a_runtime_function_kept_in_a_local_is_still_standard() {
    let text = "local trigger = TriggerServerEvent\ntrigger('shop:buy', 1)\n";
    assert_eq!(written(text), [("cfx.event#shop:buy".to_owned(), "shop:buy".to_owned())]);
}

#[test]
fn a_file_without_event_functions_gives_nothing() {
    assert_eq!(written("print('shop:buy')\nlocal t = { TriggerEvent = 1 }\n"), []);
    assert_eq!(written(""), []);
}
