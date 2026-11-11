use super::EventsFeature;
use super::db::{Config, Db};
use super::model::{Dirs, EventKey, Origin, Role, Site};
use super::spec::format;
use crate::feature::{Ctx, Doc, Feature};
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;
use std::rc::Rc;

pub const PROJECT: &[(&str, &str)] = &[
    (
        "mixed_res/client/main.lua",
        "MyHelper.AddEventHandler('Mixed', 'Refresh', function(data)\n    TriggerEvent('mixed:refreshed', data)\nend)\n\n\
         AddEventHandler('mixed:refreshed', function(data)\n    print(json.encode(data))\nend)\n\n\
         CreateThread(function()\n    TriggerServerEvent('Mixed', 'Ready')\nend)\n",
    ),
    (
        "mixed_res/fxmanifest.lua",
        "-- Synthetic \"mixed\" resource.\nfx_version 'cerulean'\ngame 'gta5'\n\nshared_script '@studio/shared/codec.lua'\n\n\
         client_scripts {\n    '@studio/client/helper.lua',\n    '@studio/client/net.lua',\n    'client/*.lua',\n}\n\n\
         server_scripts {\n    '@studio/server/helper.lua',\n    'server/*.lua',\n}\n",
    ),
    (
        "mixed_res/server/main.lua",
        "SrvHelper.AddEventHandler('Mixed', 'Ready', function(actor)\n    SrvHelper.TriggerClientEvent('Mixed', 'Refresh', actor.source, { ready = true })\nend)\n\n\
         RegisterNetEvent('mixed:report', function(reason)\n    print('report', source, reason)\nend)\n",
    ),
    (
        "standard_res/client.lua",
        "RegisterNetEvent('shop:openMenu', function(items, title)\n    for _, item in ipairs(items) do\n        print(title, item)\n    end\nend)\n\n\
         AddEventHandler('onClientResourceStart', function(resourceName)\n    print('started', resourceName)\nend)\n\n\
         RegisterCommand('buy', function(_, args)\n    TriggerServerEvent('shop:buy', args[1], tonumber(args[2]))\nend, false)\n",
    ),
    (
        "standard_res/fxmanifest.lua",
        "-- Synthetic \"standard\" resource.\nfx_version 'cerulean'\ngame 'gta5'\n\nclient_script 'client.lua'\nserver_script 'server.lua'\n",
    ),
    (
        "standard_res/server.lua",
        "RegisterNetEvent('shop:buy')\nAddEventHandler('shop:buy', function(item, amount)\n    local src = source\n    \
         TriggerClientEvent('shop:openMenu', src, { item }, 'Shop')\n    TriggerEvent('shop:purchased', src, item, amount)\nend)\n\n\
         AddEventHandler('shop:purchased', function(src, item, amount)\n    print(src, item, amount)\nend)\n\n\
         RegisterCommand('respawnall', function()\n    TriggerClientEvent('ev_legacy_spawn', -1, vector3(0.0, 0.0, 72.0), 0.0)\nend, true)\n",
    ),
    (
        "studio/client/helper.lua",
        "MyHelper = {}\n\nlocal Initialized = false\nlocal queue = {}\n\n\
         function MyHelper.AddEventHandler(eType, eName, callback, requiredInit)\n    local wire = (function()\n        \
         local encType, encName = EncodeEventPart(eType), EncodeEventPart(eName)\n        if IS_DEV then\n            encType = eType\n            \
         encName = eName\n        end\n        return '404' .. encType .. encName\n    end)()\n    RegisterNetEvent(wire)\n    \
         AddEventHandler(wire, function(...)\n        local args = { ... }\n        if requiredInit and not Initialized then\n            return\n        end\n        \
         callback(table.unpack(args))\n    end)\nend\n\n\
         function MyHelper.AddQueueEventHandler(eType, eName, cb)\n    MyHelper.AddEventHandler(eType, eName, function(...)\n        \
         local payload = table.pack(...)\n        queue[#queue + 1] = function()\n            cb(table.unpack(payload, 1, payload.n))\n        end\n    end)\nend\n\n\
         CreateThread(function()\n    while true do\n        Wait(0)\n        Initialized = true\n        local job = table.remove(queue, 1)\n        \
         if job then\n            job()\n        end\n    end\nend)\n",
    ),
    (
        "studio/client/main.lua",
        "MyHelper.AddEventHandler('Gamemode', 'SpawnPlayer', function(coords, heading)\n    \
         SetEntityCoords(PlayerPedId(), coords.x, coords.y, coords.z, false, false, false, false)\n    SetEntityHeading(PlayerPedId(), heading)\nend, true)\n\n\
         MyHelper.AddQueueEventHandler('Gamemode', 'ShowNotification', function(text)\n    print(text)\nend)\n\n\
         RegisterCommand('model', function(_, args)\n    TriggerServerEvent('Gamemode', 'RequestModel', args[1])\nend, false)\n\n\
         RegisterCommand('door', function(_, args)\n    MyHelper.TriggerServerEvent('House', 'Enter', tonumber(args[1]))\nend, false)\n",
    ),
    (
        "studio/client/net.lua",
        "-- The resource redefines TriggerServerEvent: events are keyed by (eType, eName).\nlocal _TriggerServerEvent = TriggerServerEvent\n\n\
         function TriggerServerEvent(eType, eName, ...)\n    local wire = IS_DEV and (eType .. eName) or ('404' .. EncodeEventPart(eType) .. EncodeEventPart(eName))\n    \
         _TriggerServerEvent(wire, ...)\nend\n\n\
         function MyHelper.TriggerServerEvent(eType, eName, ...)\n    TriggerServerEvent(eType, eName, ...)\nend\n",
    ),
    (
        "studio/fxmanifest.lua",
        "-- Synthetic \"studio\" resource.\nfx_version 'cerulean'\ngame 'gta5'\n\nshared_scripts {\n    'shared/codec.lua',\n    'shared/alias.lua',\n}\n\n\
         client_scripts {\n    'client/helper.lua',\n    'client/net.lua',\n    'client/main.lua',\n}\n\n\
         server_scripts {\n    'server/helper.lua',\n    'server/objects.lua',\n    'server/main.lua',\n}\n",
    ),
    (
        "studio/server/helper.lua",
        "SrvHelper = {}\nActors = {}\n\nlocal function wireOf(eType, eName)\n    if IS_DEV then\n        return eType .. eName\n    end\n    \
         return '404' .. EncodeEventPart(eType) .. EncodeEventPart(eName)\nend\n\n\
         function SrvHelper.AddEventHandler(eType, eName, callback)\n    RegisterNetEvent(wireOf(eType, eName), function(...)\n        \
         local actor = Actors[source]\n        if not actor then\n            return\n        end\n        callback(actor, ...)\n    end)\nend\n\n\
         function SrvHelper.AddHouseEventHandler(eType, eName, rateLimit, onlyOwner, callback)\n    local lastCall = {}\n    \
         SrvHelper.AddEventHandler(eType, eName, function(actor, ...)\n        local now = GetGameTimer()\n        \
         if lastCall[actor.id] and now - lastCall[actor.id] < rateLimit then\n            return\n        end\n        lastCall[actor.id] = now\n        \
         local house = actor.house\n        if not house or (onlyOwner and house.owner ~= actor.id) then\n            return\n        end\n        \
         callback(actor, house, ...)\n    end)\nend\n\n\
         function SrvHelper.TriggerClientEvent(eType, eName, target, ...)\n    TriggerClientEvent(wireOf(eType, eName), target, ...)\nend\n",
    ),
    (
        "studio/server/main.lua",
        "SrvHelper.AddEventHandler('Gamemode', 'RequestModel', function(actor, model)\n    local player = Player.new(actor.source)\n    \
         player:TriggerEvent('Gamemode', 'SpawnPlayer', vector3(0.0, 0.0, 72.0), 90.0)\nend)\n\n\
         SrvHelper.AddHouseEventHandler('House', 'Enter', 1000, true, function(actor, house, door)\n    \
         SrvHelper.TriggerClientEvent('Gamemode', 'ShowNotification', actor.source, 'Welcome to ' .. house.name)\nend)\n\n\
         local arena = Zone.new('arena')\n\nAddEventHandler('onResourceStart', function(resourceName)\n    \
         if resourceName == GetCurrentResourceName() then\n        arena:TriggerEvent('Arena', 'Gamemode', 'ShowNotification', 'Round started')\n    end\nend)\n",
    ),
    (
        "studio/server/objects.lua",
        "Player = {}\nPlayer.__index = Player\n\nfunction Player.new(source)\n    return setmetatable({ source = source }, Player)\nend\n\n\
         function Player:TriggerEvent(type, name, ...)\n    SrvHelper.TriggerClientEvent(type, name, self.source, ...)\nend\n\n\
         Zone = {}\nZone.__index = Zone\n\nfunction Zone.new(name)\n    return setmetatable({ name = name, members = {} }, Zone)\nend\n\n\
         function Zone:TriggerEvent(areaType, eType, eName, ...)\n    for _, member in ipairs(self.members) do\n        \
         SrvHelper.TriggerClientEvent(eType, eName, member, areaType, ...)\n    end\nend\n",
    ),
    (
        "studio/shared/alias.lua",
        "-- Legacy network names kept for older resources.\nEventAliases = {}\n\nfunction RegisterEventAlias(wire, eType, eName)\n    \
         EventAliases[wire] = { eType, eName }\nend\n\nRegisterEventAlias('ev_legacy_spawn', 'Gamemode', 'SpawnPlayer')\n",
    ),
    (
        "studio/shared/codec.lua",
        "-- Network names are obfuscated outside of development builds.\nIS_DEV = false\n\nfunction EncodeEventPart(value)\n    local out = {}\n    \
         for i = 1, #value do\n        out[#out + 1] = string.format('%02x', (string.byte(value, i) + 7) % 256)\n    end\n    \
         return table.concat(out)\nend\n",
    ),
];

pub fn project(extra: &[(&str, &str)]) -> Fixture {
    let mut files: Vec<(&str, &str)> = PROJECT.to_vec();
    for (path, text) in extra {
        files.retain(|(p, _)| p != path);
        files.push((path, text));
    }
    Fixture::new(&files)
}

pub fn indexed(fx: &Fixture) -> EventsFeature {
    let feature = EventsFeature::default();
    fx.index(&feature);
    feature
}

pub fn db(fx: &Fixture) -> Rc<Db> {
    let feature = indexed(fx);
    let doc = fx.doc("standard_res/client.lua");
    let cx = fx.cx(&doc);
    feature.db(&cx).unwrap()
}

fn rel(site: &Site) -> String {
    format!("{}:{}", site.file.trim_start_matches("/p/"), site.line + 1)
}

fn describe(site: &Site) -> String {
    let role = match site.role {
        Role::Handler => "HANDLER",
        Role::Trigger => "TRIGGER",
        Role::Register => "REGISTER",
        Role::Alias => "ALIAS",
    };
    format!("{role} {}", rel(site))
}

fn sites_of(db: &Db, key: &EventKey, aliases: bool) -> Vec<String> {
    let mut v: Vec<String> = db.sites_of(key, aliases).into_iter().map(describe).collect();
    v.sort();
    v
}

pub fn wire(name: &str) -> EventKey {
    EventKey::wire(name)
}

pub fn tuple(parts: &[&str]) -> EventKey {
    EventKey::tuple(parts)
}

fn helpers(db: &Db) -> Vec<String> {
    db.helpers.iter().map(|h| format!("{}: {}", h.path, format(h))).collect()
}

#[test]
fn helpers_are_detected_from_the_parameter_flow() {
    let db = db(&project(&[]));
    assert_eq!(
        helpers(&db),
        [
            "MyHelper.AddEventHandler: handler net client key(1, 2) callback(3)",
            "MyHelper.AddQueueEventHandler: handler net client key(1, 2) callback(3)",
            "MyHelper.TriggerServerEvent: trigger server key(1, 2) payload(...)",
            "Player:TriggerEvent: trigger client key(1, 2) payload(...)",
            "RegisterEventAlias: alias wire(1)",
            "SrvHelper.AddEventHandler: handler net server key(1, 2) callback(3) inject(1)",
            "SrvHelper.AddHouseEventHandler: handler net server key(1, 2) callback(5) inject(2)",
            "SrvHelper.TriggerClientEvent: trigger client key(1, 2) target(3) payload(...)",
            "TriggerServerEvent: trigger server key(1, 2) payload(...)",
            "Zone:TriggerEvent: trigger client key(2, 3)",
        ]
    );
    assert!(db.helpers.iter().find(|h| h.path == "TriggerServerEvent").unwrap().redefinition);
}

#[test]
fn standard_events() {
    let db = db(&project(&[]));
    assert_eq!(
        sites_of(&db, &wire("shop:buy"), false),
        [
            "HANDLER standard_res/server.lua:2",
            "REGISTER standard_res/server.lua:1",
            "TRIGGER standard_res/client.lua:12"
        ]
    );
    assert_eq!(
        sites_of(&db, &wire("shop:openMenu"), false),
        ["HANDLER standard_res/client.lua:1", "TRIGGER standard_res/server.lua:4"]
    );
    assert_eq!(
        sites_of(&db, &wire("shop:purchased"), false),
        ["HANDLER standard_res/server.lua:8", "TRIGGER standard_res/server.lua:5"]
    );
    let open = db.handlers(&wire("shop:openMenu"));
    assert_eq!(open.len(), 1);
    assert!(open[0].net);
    assert_eq!(open[0].callback_arity, Some(2));
    let buy = db.triggers(&wire("shop:buy"));
    assert_eq!((buy.len(), buy[0].directions, buy[0].payload_arity), (1, Dirs::SERVER, Some(2)));
    let purchased = db.triggers(&wire("shop:purchased"));
    assert_eq!((purchased[0].directions, purchased[0].payload_arity), (Dirs::LOCAL, Some(3)));
    let reached: Vec<String> = db.handlers_reached_by(buy[0]).into_iter().map(describe).collect();
    assert_eq!(reached, ["HANDLER standard_res/server.lua:2"]);
}

#[test]
fn tuple_keys_through_helpers() {
    let db = db(&project(&[]));
    let spawn = tuple(&["Gamemode", "SpawnPlayer"]);
    assert_eq!(
        sites_of(&db, &spawn, false),
        ["ALIAS studio/shared/alias.lua:8", "HANDLER studio/client/main.lua:1", "TRIGGER studio/server/main.lua:3"]
    );
    let h = db.handlers(&spawn);
    assert_eq!((h.len(), h[0].net, h[0].callback_arity), (1, true, Some(2)));
    let t: Vec<&Site> = db.sites_of(&spawn, false).into_iter().filter(|s| s.role == Role::Trigger).collect();
    assert_eq!(t[0].payload_arity, Some(2));

    let model = tuple(&["Gamemode", "RequestModel"]);
    assert_eq!(sites_of(&db, &model, false), ["HANDLER studio/server/main.lua:1", "TRIGGER studio/client/main.lua:11"]);
    assert_eq!(db.handlers(&model)[0].callback_arity, Some(1));
    assert_eq!(db.triggers(&model)[0].payload_arity, Some(1));

    let enter = tuple(&["House", "Enter"]);
    assert_eq!(sites_of(&db, &enter, false), ["HANDLER studio/server/main.lua:6", "TRIGGER studio/client/main.lua:15"]);
    assert_eq!(db.handlers(&enter)[0].callback_arity, Some(1));
    assert_eq!(db.triggers(&enter)[0].payload_arity, Some(1));

    assert_eq!(
        sites_of(&db, &tuple(&["Gamemode", "ShowNotification"]), false),
        ["HANDLER studio/client/main.lua:6", "TRIGGER studio/server/main.lua:14", "TRIGGER studio/server/main.lua:7"]
    );
    let roles: std::collections::HashSet<Role> = db.sites_in("/p/studio/server/main.lua").map(|s| s.role).collect();
    assert_eq!(roles, [Role::Handler, Role::Trigger].into());
}

#[test]
fn obfuscated_names_leave_no_dynamic_site() {
    let db = db(&project(&[]));
    let dynamic: Vec<String> = db.dynamic.iter().map(|d| d.file.to_string()).collect();
    assert!(dynamic.is_empty(), "{dynamic:?}");
}

#[test]
fn aliases_link_a_network_name_and_a_tuple_key() {
    let db = db(&project(&[]));
    assert_eq!(
        sites_of(&db, &tuple(&["Gamemode", "SpawnPlayer"]), true),
        [
            "ALIAS studio/shared/alias.lua:8",
            "ALIAS studio/shared/alias.lua:8",
            "HANDLER studio/client/main.lua:1",
            "TRIGGER standard_res/server.lua:13",
            "TRIGGER studio/server/main.lua:3",
        ]
    );
    let legacy = wire("ev_legacy_spawn");
    let triggers = db.triggers(&legacy);
    assert_eq!(triggers.len(), 2);
    let own = triggers.iter().find(|t| *t.key == legacy).unwrap();
    let reached: Vec<String> = db.handlers_reached_by(own).into_iter().map(describe).collect();
    assert_eq!(reached, ["HANDLER studio/client/main.lua:1"]);
}

#[test]
fn mixed_resource_loading_the_helpers_of_another_resource() {
    let db = db(&project(&[]));
    assert_eq!(
        sites_of(&db, &tuple(&["Mixed", "Refresh"]), false),
        ["HANDLER mixed_res/client/main.lua:1", "TRIGGER mixed_res/server/main.lua:2"]
    );
    assert_eq!(
        sites_of(&db, &tuple(&["Mixed", "Ready"]), false),
        ["HANDLER mixed_res/server/main.lua:1", "TRIGGER mixed_res/client/main.lua:10"]
    );
    assert_eq!(
        sites_of(&db, &wire("mixed:refreshed"), false),
        ["HANDLER mixed_res/client/main.lua:5", "TRIGGER mixed_res/client/main.lua:2"]
    );
    assert_eq!(sites_of(&db, &wire("mixed:report"), false), ["HANDLER mixed_res/server/main.lua:5"]);
}

#[test]
fn the_redefinition_only_applies_to_its_resource() {
    let db = db(&project(&[
        ("other_res/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'"),
        ("other_res/client.lua", "TriggerServerEvent('Gamemode', 'RequestModel')"),
    ]));
    assert_eq!(sites_of(&db, &wire("Gamemode"), false), ["TRIGGER other_res/client.lua:1"]);
    assert_eq!(db.triggers(&tuple(&["Gamemode", "RequestModel"])).len(), 1);
}

#[test]
fn the_helper_example_of_the_guide() {
    let db = db(&project(&[
        (
            "guide_res/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'events.lua'\nclient_script 'client.lua'\nserver_script 'server.lua'",
        ),
        (
            "guide_res/events.lua",
            "Events = {}\nfunction Events.On(category, name, callback)\n    RegisterNetEvent('evt:' .. category .. ':' .. name, function(...)\n        \
             callback(source, ...)\n    end)\nend\nfunction Events.Send(category, name, ...)\n    TriggerServerEvent('evt:' .. category .. ':' .. name, ...)\nend",
        ),
        ("guide_res/server.lua", "Events.On('shop', 'buy', function(playerId, item) end)"),
        ("guide_res/client.lua", "Events.Send('shop', 'buy', 'bread')"),
    ]));
    let h = helpers(&db);
    assert!(
        h.contains(&"Events.On: handler net both key('evt:' .. 1 .. ':' .. 2) callback(3) inject(1)".to_owned()),
        "{h:?}"
    );
    let key = wire("evt:shop:buy");
    assert_eq!(sites_of(&db, &key, false), ["HANDLER guide_res/server.lua:1", "TRIGGER guide_res/client.lua:1"]);
    assert_eq!(db.handlers(&key)[0].callback_arity, Some(1));
    assert_eq!(db.triggers(&key)[0].payload_arity, Some(1));
}

// ---------------------------------------------------------------------------------------------
// customization

const BRIDGE: &str = "---Listens to an event of the bridge resource.\n\
    ---@cfx.event handler server key(eType, eName) callback(cb) inject(1)\n\
    function SrvHelper.OnBridge(eType, eName, cb)\n    exports.bridge:Listen(eType .. '/' .. eName, cb)\nend";

#[test]
fn annotated_helpers() {
    let db = db(&project(&[
        ("studio/server/bridge.lua", BRIDGE),
        ("studio/server/bridge_use.lua", "SrvHelper.OnBridge('Bridge', 'Sync', function(actor, data) end)"),
        ("studio/client/bridge_trigger.lua", "TriggerServerEvent('Bridge', 'Sync', { a = 1 })"),
    ]));
    let helper = db.helpers.iter().find(|h| h.path == "SrvHelper.OnBridge").unwrap();
    assert_eq!(helper.origin, Origin::Annotation);
    assert_eq!(format(helper), "handler net server key(1, 2) callback(3) inject(1)");
    let key = tuple(&["Bridge", "Sync"]);
    assert_eq!(
        sites_of(&db, &key, false),
        ["HANDLER studio/server/bridge_use.lua:1", "TRIGGER studio/client/bridge_trigger.lua:1"]
    );
    assert_eq!(db.handlers(&key)[0].callback_arity, Some(1));
}

#[test]
fn annotations_override_the_detection() {
    let db = db(&project(&[(
        "studio/server/override.lua",
        "---@cfx.event handler server key(1, 2) callback(3) inject(0)\nfunction SrvHelper.AddRawEventHandler(eType, eName, callback)\n    \
         SrvHelper.AddEventHandler(eType, eName, function(actor, ...) callback(...) end)\nend\n\
         SrvHelper.AddRawEventHandler('Raw', 'Ping', function(a, b) end)",
    )]));
    assert_eq!(db.handlers(&tuple(&["Raw", "Ping"]))[0].callback_arity, Some(2));
}

fn db_with(fx: Fixture, settings: Settings) -> Rc<Db> {
    let fx = fx.with_settings(settings);
    let feature = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    let cx: Ctx = fx.cx(&doc);
    feature.db(&cx).unwrap()
}

#[test]
fn helpers_declared_in_the_settings() {
    let fx = || project(&[("standard_res/cl_legacy.lua", "Legacy.Emit('shop:buy', 'bread', 2)")]);
    assert_eq!(db(&fx()).triggers(&wire("shop:buy")).len(), 1);
    let db = db_with(
        fx(),
        Settings::defaults().with(super::keys::HELPERS, Value::from("Legacy.Emit trigger server key(1)")),
    );
    let mut triggers: Vec<String> = db.triggers(&wire("shop:buy")).into_iter().map(describe).collect();
    triggers.sort();
    assert_eq!(triggers, ["TRIGGER standard_res/cl_legacy.lua:1", "TRIGGER standard_res/client.lua:12"]);
    let legacy = db.triggers(&wire("shop:buy")).into_iter().find(|t| t.file.contains("legacy")).unwrap();
    assert_eq!(legacy.payload_arity, Some(2));
}

#[test]
fn settings_override_a_detected_helper() {
    let model = tuple(&["Gamemode", "RequestModel"]);
    assert_eq!(db(&project(&[])).handlers(&model)[0].callback_arity, Some(1));
    let settings = Settings::defaults()
        .with(super::keys::HELPERS, Value::from("SrvHelper.AddEventHandler handler net server key(1, 2) callback(3)"));
    let db = db_with(project(&[]), settings);
    let h = db.handlers(&model);
    assert_eq!((h[0].function.origin, h[0].callback_arity), (Origin::Settings, Some(2)));
}

#[test]
fn detection_can_be_turned_off() {
    let db = db_with(project(&[]), Settings::defaults().with(super::keys::DETECT_HELPERS, Value::from(false)));
    assert_eq!(helpers(&db), ["RegisterEventAlias: alias wire(1)"]);
    assert!(db.sites_of(&tuple(&["Gamemode", "RequestModel"]), true).is_empty());
    assert_eq!(db.sites_of(&wire("shop:buy"), true).len(), 3);
}

#[test]
fn alias_functions_are_configurable() {
    let legacy = wire("ev_legacy_spawn");
    let db = db_with(project(&[]), Settings::defaults().with(super::keys::ALIAS_FUNCTIONS, Value::from("")));
    let triggers = db.triggers(&legacy);
    assert_eq!(triggers.len(), 1);
    assert!(db.handlers_reached_by(triggers[0]).is_empty());
    assert_eq!(Config::read(&Settings::defaults()), Config::default());
}

#[test]
fn settings_changes_rebuild_the_database() {
    let fx = project(&[]).with_settings(Settings::defaults().with(super::keys::DETECT_HELPERS, Value::from(false)));
    // indexed with the defaults, asked with detection off
    let feature = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    let db = feature.db(&fx.cx(&doc)).unwrap();
    assert_eq!(db.helpers.len(), 1);
}

#[test]
fn updating_edited_files_matches_a_full_build() {
    let fx = project(&[]);
    let config = Config::read(&Settings::defaults());
    let lua: Vec<String> = fx
        .ws
        .files()
        .filter(|f| f.path.ends_with(".lua") && !f.path.ends_with("fxmanifest.lua"))
        .map(|f| f.path.clone())
        .collect();
    let mut files: std::collections::BTreeMap<String, super::flow::FileFacts> =
        lua.iter().map(|p| (p.clone(), super::flow::facts(&crate::lua::Source::new(fx.ws.text(p).unwrap())))).collect();
    let mut db = Db::build(&fx.ws, &files, &config);
    let (mut incremental, mut undetected) = (0, 0);
    for (n, path) in lua.iter().cycle().take(lua.len() * 7).enumerate() {
        let text = fx.ws.text(path).unwrap().to_owned();
        let at = text.char_indices().map(|(i, _)| i).nth(n * 7 % text.chars().count().max(1)).unwrap_or(0);
        let edited = match n % 7 {
            0 => format!("local typed = 1\n{text}"),
            1 => format!("{}\n{}", &text[..at], &text[at..]),
            2 => format!("{} x{}", &text[..at], &text[at..]),
            3 => text.replacen("'", "'renamed_", 1),
            4 => text.lines().skip(1).collect::<Vec<_>>().join("\n"),
            5 => format!("function Typed{n}(a)\n    Unrelated(a)\nend\n{text}"),
            _ => format!("{text}\nTriggerServerEvent('late:{n}')\n"),
        };
        let facts = super::flow::facts(&crate::lua::Source::new(&edited));
        let old = files.insert(path.clone(), facts).unwrap();
        let redetect = !db.detects_alike(&old, &files[path]);
        let changed = std::collections::BTreeSet::from([path.clone()]);
        let full = Db::build(&fx.ws, &files, &config);
        db = match db.update(&fx.ws, &files, &config, &changed, redetect) {
            Some(updated) => {
                incremental += 1;
                undetected += usize::from(!redetect);
                assert_eq!(updated.dump(), full.dump(), "edit {n} of {path}, detected again: {redetect}");
                updated
            }
            None => {
                assert!(redetect, "edit {n} of {path}: only a new detection gives the database up");
                full
            }
        };
    }
    assert!(incremental > lua.len() * 4, "most edits update in place ({incremental})");
    assert!(undetected > lua.len() * 3, "most edits leave the event functions alone ({undetected})");
}

fn facts_of(text: &str) -> super::flow::FileFacts {
    super::flow::facts(&crate::lua::Source::new(text))
}

#[test]
fn the_detection_runs_again_only_for_what_it_reads() {
    let fx = project(&[]);
    let db = db(&fx);
    let helper = fx.ws.text(&Fixture::path("studio/client/helper.lua")).unwrap();
    let old = facts_of(helper);
    let same = |edited: String| db.detects_alike(&old, &facts_of(&edited));
    assert!(same(format!("local typed = 1\n\n{helper}")), "the functions only moved");
    assert!(same(helper.replace("local queue = {}", "local queue = {}\nlocal function Typed(a)\n    Other(a)\nend")));
    assert!(same(helper.replace("callback(table.unpack(args))", "callback(table.unpack(args))\n        print(eName)")));
    assert!(!same(helper.replace("RegisterNetEvent(wire)", "RegisterNetEvent(eName)")), "another key");
    assert!(!same(
        helper.replace("MyHelper.AddQueueEventHandler(eType, eName, cb)", "MyHelper.AddQueued(eType, eName, cb)")
    ));
    assert!(!same(format!("---@cfx.event handler key(1)\nfunction Typed(name) end\n{helper}")), "an annotation");
    assert!(!same(format!("function Typed(name)\n    TriggerEvent(name)\nend\n{helper}")), "a new event function");
    assert!(!db.detects_alike(&old, &facts_of("")), "the file is gone");
    let plain = facts_of("TriggerEvent('a')\nfunction F(x) Other(x) end");
    assert!(db.detects_alike(&facts_of(""), &plain), "a new file that forwards nothing to an event function");
}

#[test]
fn an_edit_above_an_event_function_updates_the_database_in_place() {
    let fx = project(&[]);
    let feature = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    assert!(feature.db(&fx.cx(&doc)).is_some());
    let path = Fixture::path("studio/server/helper.lua");
    let edit = |text: &str| {
        feature.index_file(&fx.ws, &path, crate::project::FileKind::Lua, &crate::lua::Source::new(text));
        let redetect = feature.projects.borrow()[&fx.ws.root].redetect;
        (redetect, feature.db(&fx.cx(&doc)).unwrap().dump())
    };
    let rebuilt = |text: &str| db(&project(&[("studio/server/helper.lua", text)])).dump();

    let moved = format!("local typed = 1\n\n{}", fx.ws.text(&path).unwrap());
    let (redetect, updated) = edit(&moved);
    assert!(!redetect, "the event functions of the file only moved");
    assert_eq!(updated, rebuilt(&moved));
    assert!(updated.contains("SrvHelper.AddHouseEventHandler"));

    let renamed = moved.replace("function SrvHelper.TriggerClientEvent(", "function SrvHelper.Emit(");
    let (redetect, updated) = edit(&renamed);
    assert!(redetect, "an event function was renamed");
    assert_eq!(updated, rebuilt(&renamed));
    assert!(!updated.contains("SrvHelper.TriggerClientEvent"));
}

#[test]
fn a_completion_outside_a_string_leaves_the_database_for_a_later_call() {
    let fx = project(&[]);
    let feature = indexed(&fx);
    let doc = fx.doc("standard_res/client.lua");
    assert!(feature.db(&fx.cx(&doc)).is_some());
    let path = Fixture::path("standard_res/client.lua");
    let typed = format!("TriggerEv\n{}", fx.ws.text(&path).unwrap());
    feature.index_file(&fx.ws, &path, crate::project::FileKind::Lua, &crate::lua::Source::new(&typed));
    let typed = Doc::new(&fx.ws, &path, "lua", 2, &typed);
    assert!(feature.completion(&fx.cx(&typed), "TriggerEv".len(), None).is_empty());
    assert!(feature.projects.borrow()[&fx.ws.root].dirty, "nothing was rebuilt for a name being typed");
    let in_string = typed.text().find("'shop:openMenu'").unwrap() + 2;
    assert!(!feature.completion(&fx.cx(&typed), in_string, None).is_empty());
    assert!(!feature.projects.borrow()[&fx.ws.root].dirty);
}

#[test]
#[ignore = "verification: CFXLUA_PROJECT=<dir> cargo test --release --lib updating_a_real_project_matches_a_full_build -- --ignored --nocapture"]
fn updating_a_real_project_matches_a_full_build() {
    use crate::project::{FileKind, file_kind, is_escrowed};
    let all: Vec<(String, String)> = crate::testing::real_project()
        .into_iter()
        .filter(|(p, t)| file_kind(p) != FileKind::Other && t.len() <= crate::workspace::MAX_FILE)
        .collect();
    let refs: Vec<(&str, &str)> = all.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let ws = crate::workspace::Workspace::from_files("/real", &refs);
    let facts = |text: &str| {
        let src = if is_escrowed(text) { crate::lua::Source::plain(text) } else { crate::lua::Source::new(text) };
        super::flow::facts(&src)
    };
    let lua: Vec<(&str, &str)> = ws
        .files()
        .filter(|f| f.kind == FileKind::Lua)
        .filter_map(|f| Some((f.path.as_str(), f.text.as_deref()?)))
        .collect();
    let mut files: std::collections::BTreeMap<String, super::flow::FileFacts> =
        lua.iter().map(|(p, t)| ((*p).to_owned(), facts(t))).collect();
    let config = Config::default();
    let mut db = Db::build(&ws, &files, &config);
    let step = (lua.len() / 150).max(1);
    let (mut kept, mut detected, mut rebuilt) = (0, 0, 0);
    for (n, (path, text)) in lua.iter().enumerate().filter(|(n, _)| n % step == 0) {
        let middle = (0..=text.len() / 2).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        let edited = match n / step % 5 {
            0 => format!("local typed = 1\n{text}"),
            1 => format!("{}\n\n{}", &text[..middle], &text[middle..]),
            2 => format!("{text}\nTriggerServerEvent('typed:{n}')\n"),
            3 => format!("function Typed{n}(payload)\n    TriggerEvent('typed:{n}', payload)\nend\n{text}"),
            _ => format!("function Typed{n}(name)\n    TriggerEvent(name)\nend\n{text}\nTyped{n}('typed:{n}')\n"),
        };
        let old = files.insert((*path).to_owned(), facts(&edited)).unwrap();
        let redetect = !db.detects_alike(&old, &files[*path]);
        let changed = std::collections::BTreeSet::from([(*path).to_owned()]);
        let full = Db::build(&ws, &files, &config);
        db = match db.update(&ws, &files, &config, &changed, redetect) {
            Some(updated) => {
                assert!(updated.dump() == full.dump(), "edit {n}: the update is not what a full build gives");
                if redetect {
                    detected += 1;
                } else {
                    kept += 1;
                }
                updated
            }
            None => {
                assert!(redetect, "edit {n}: only a new detection gives the database up");
                rebuilt += 1;
                full
            }
        };
    }
    eprintln!(
        "{} Lua files, {} sites, one file in {step} edited: {kept} updated without detecting, {detected} after \
         detecting again, {rebuilt} rebuilt",
        lua.len(),
        db.sites.len()
    );
}
