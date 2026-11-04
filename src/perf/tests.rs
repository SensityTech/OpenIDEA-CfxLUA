//! Acceptance tests of the performance checks, on a `res` resource with
//! client, server and shared folders and a small natives catalog.

use super::*;
use crate::feature::Feature;
use crate::natives::{Native, NativeSet, Return};
use crate::settings::Settings;
use crate::testing::{CARET, Fixture, native};
use serde_json::Value;

const MANIFEST: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'\nshared_script 'shared/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";
const SERVER: &str = "res/server/main.lua";

fn catalog() -> Vec<Native> {
    let mut out: Vec<Native> = [
        "DrawMarker",
        "DrawText",
        "SetTextFont",
        "BeginTextCommandDisplayHelp",
        "EndTextCommandDisplayHelp",
        "IsControlJustPressed",
        "IsControlPressed",
        "DisableControlAction",
        "HideHudComponentThisFrame",
        "GetEntityHeading",
        "IsPedInAnyVehicle",
        "GetVehiclePedIsIn",
        "GetPlayerPed",
        "SetEntityCoords",
        "SetEntityHealth",
        "SetPlayerModel",
        "HasModelLoaded",
        "RequestModel",
        "HasAnimDictLoaded",
        "RequestAnimDict",
        "GetDistanceBetweenCoords",
        "Vdist",
        "Vdist2",
        "GetPlayerServerId",
        "IsPedShooting",
        "GetHashKey",
    ]
    .iter()
    .map(|n| native(NativeSet::Gta5, n, Side::Client))
    .collect();
    let mut pulse = native(NativeSet::Gta5, "IsWorldPulseActive", Side::Client);
    pulse.description = "Only returns true for one frame after the pulse.".into();
    out.push(pulse);
    let mut coords = native(NativeSet::Gta5, "GetEntityCoords", Side::Client);
    coords.returns = vec![Return {
        name: "retval".into(),
        lua_type: "vector3".into(),
        native_type: "Vector3".into(),
        description: None,
    }];
    out.push(coords);
    out.extend(
        ["GetEntityCoords", "DropPlayer", "GetPlayerPed", "SetEntityCoords"]
            .iter()
            .map(|n| native(NativeSet::Server, n, Side::Server)),
    );
    out
}

/// Removes the common indentation (like Kotlin's `trimIndent`).
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.trim_matches('\n').lines().collect();
    let indent =
        lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() }).collect::<Vec<_>>().join("\n")
}

struct Run {
    fx: Fixture,
    rel: String,
    feature: PerfFeature,
}

fn run_with(rel: &str, text: &str, settings: Settings, natives: Vec<Native>) -> Run {
    let text = dedent(text);
    let fx =
        Fixture::new(&[("res/fxmanifest.lua", MANIFEST), (rel, &text)]).with_natives(natives).with_settings(settings);
    let feature = PerfFeature::default();
    fx.index(&feature);
    Run { fx, rel: rel.to_owned(), feature }
}

fn run(rel: &str, text: &str) -> Run {
    run_with(rel, text, Settings::defaults(), catalog())
}

impl Run {
    fn all(&self) -> Vec<Problem> {
        let doc = self.fx.doc(&self.rel);
        let mut p = self.feature.problems(&self.fx.cx(&doc));
        p.sort_by_key(|p| p.start);
        p
    }

    /// `highlighted text: message` of one check.
    fn problems(&self, code: &str) -> Vec<String> {
        let doc = self.fx.doc(&self.rel);
        self.all()
            .into_iter()
            .filter(|p| p.code == code)
            .map(|p| format!("{}: {}", &doc.text()[p.start..p.end], p.message))
            .collect()
    }

    /// 0-based lines of the problems of one check.
    fn lines(&self, code: &str) -> Vec<usize> {
        let doc = self.fx.doc(&self.rel);
        self.all().into_iter().filter(|p| p.code == code).map(|p| doc.src.lines.line_of(p.start)).collect()
    }

    /// The text after applying the fix `title` of a problem at the caret.
    fn fixed(&self, title: &str) -> String {
        let (doc, at) = self.fx.caret(&self.rel);
        let fixes: Vec<Fix> =
            self.all().into_iter().filter(|p| p.start <= at && at <= p.end).flat_map(|p| p.fixes).collect();
        let fix = fixes
            .iter()
            .find(|f| f.title == title)
            .unwrap_or_else(|| panic!("no '{title}' in {:?}", fixes.iter().map(|f| &f.title).collect::<Vec<_>>()));
        apply(doc.text(), &fix.edits)
    }

    fn fixes_at_caret(&self) -> Vec<String> {
        let (_, at) = self.fx.caret(&self.rel);
        self.all().into_iter().filter(|p| p.start <= at && at <= p.end).flat_map(|p| p.fixes).map(|f| f.title).collect()
    }

    fn markers(&self) -> Vec<String> {
        let doc = self.fx.doc(&self.rel);
        self.feature.line_markers(&self.fx.cx(&doc)).into_iter().map(|m| format!("{}: {}", m.line, m.tooltip)).collect()
    }
}

pub fn apply(text: &str, edits: &[(usize, usize, String)]) -> String {
    let mut out = text.to_owned();
    let mut sorted: Vec<&(usize, usize, String)> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
    for (s, e, t) in sorted {
        out.replace_range(*s..*e, t);
    }
    out
}

// 1. per-frame polling

#[test]
fn polling_loop_is_reported() {
    let r = run(
        CLIENT,
        "
        local zone = vector3(1.0, 2.0, 3.0)
        local wasInside = false
        CreateThread(function()
            while true do
                Wait(0)
                local inside = #(GetEntityCoords(PlayerPedId()) - zone) < 5.0
                if inside ~= wasInside then
                    wasInside = inside
                    TriggerServerEvent('zone:changed', inside)
                end
            end
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::PER_FRAME_LOOP),
        [
            "Wait(0): 'Wait(0)' makes this loop run every frame, but it only reads state (no drawing, no control input): it can sleep between checks"
        ]
    );
}

#[test]
fn citizen_wait_and_flag_loops_are_reported() {
    let r = run(
        CLIENT,
        "
        local active = true
        Citizen.CreateThread(function()
            while active do
                Citizen.Wait(0)
                if IsPedInAnyVehicle(PlayerPedId(), false) then active = false end
            end
        end)
    ",
    );
    assert_eq!(r.problems(checks::PER_FRAME_LOOP).len(), 1);
}

#[test]
fn loops_doing_per_frame_work_are_not_reported() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsControlJustPressed(0, 38) then TriggerServerEvent('press') end
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                DisableControlAction(0, 24, true)
                HideHudComponentThisFrame(14)
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsWorldPulseActive() then print('pulse') end
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                SetEntityHealth(PlayerPedId(), 200)
            end
        end)
    ",
    );
    assert!(r.problems(checks::PER_FRAME_LOOP).is_empty());
}

#[test]
fn loops_calling_unknown_code_sleeping_or_ending_are_not_reported() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                local data = ESX.GetPlayerData()
                TriggerEvent('hud:update', data)
            end
        end)
        CreateThread(function()
            while true do
                Wait(500)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
        CreateThread(function()
            while true do
                local sleep = 1000
                if #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0)) < 10.0 then
                    sleep = 0
                    DrawMarker(1, 0.0, 0.0, 0.0)
                end
                Wait(sleep)
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsPedInAnyVehicle(PlayerPedId(), false) then break end
            end
        end)
        CreateThread(function()
            while not HasModelLoaded(1) do
                Wait(0)
            end
        end)
    ",
    );
    assert!(r.problems(checks::PER_FRAME_LOOP).is_empty());
}

#[test]
fn functions_declared_as_polling() {
    let text = "
        CreateThread(function()
            while true do
                Wait(0)
                local data = ESX.GetPlayerData()
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                local job = ESX.GetJob()
            end
        end)
    ";
    let settings = Settings::defaults().with(keys::POLLING_FUNCTIONS, Value::from("ESX.GetPlayerData"));
    assert_eq!(run_with(CLIENT, text, settings, catalog()).lines(checks::PER_FRAME_LOOP), [2]);
}

#[test]
fn localized_globals_are_followed() {
    let r = run(
        CLIENT,
        "
        local Wait, PlayerPedId, GetEntityCoords = Citizen.Wait, PlayerPedId, GetEntityCoords
        local DrawMarker = DrawMarker
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
    ",
    );
    assert_eq!(r.lines(checks::PER_FRAME_LOOP), [4]);
}

#[test]
fn project_functions_are_followed() {
    let r = run(
        CLIENT,
        "
        local function isNear(pos)
            return #(GetEntityCoords(PlayerPedId()) - pos) < 3.0
        end
        local function drawHelp()
            BeginTextCommandDisplayHelp('STRING')
            EndTextCommandDisplayHelp(0, false, true, -1)
        end
        CreateThread(function()
            while true do
                Wait(0)
                if isNear(vector3(1.0, 2.0, 3.0)) then print('near') end
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                drawHelp()
            end
        end)
    ",
    );
    assert_eq!(r.lines(checks::PER_FRAME_LOOP), [9]);
}

#[test]
fn global_functions_of_other_files_are_followed() {
    let fx = Fixture::new(&[
        ("res/fxmanifest.lua", MANIFEST),
        ("res/client/util.lua", "function DrawShop()\n    DrawMarker(1, 0.0, 0.0, 0.0)\nend\nfunction Utils.isNear() return GetEntityCoords(PlayerPedId()) end"),
        (CLIENT, "CreateThread(function()\n    while true do\n        Wait(0)\n        DrawShop()\n    end\nend)\nCreateThread(function()\n    while true do\n        Wait(0)\n        Utils.isNear()\n    end\nend)"),
    ])
    .with_natives(catalog());
    let feature = PerfFeature::default();
    fx.index(&feature);
    let doc = fx.doc(CLIENT);
    let lines: Vec<usize> = feature
        .problems(&fx.cx(&doc))
        .iter()
        .filter(|p| p.code == checks::PER_FRAME_LOOP)
        .map(|p| doc.src.lines.line_of(p.start))
        .collect();
    assert_eq!(lines, [8]);
}

#[test]
fn throttle_fix() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wa<caret>it(0)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Wait 500 ms between iterations"),
        dedent(
            "
        CreateThread(function()
            while true do
                Wait(500)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
    "
        )
    );
}

#[test]
fn adaptive_sleep_fix_on_a_polling_loop() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wa<caret>it(0)
                local dist = #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0))
                if dist < 2.0 then
                    -- near the shop
                    print('near')
                end
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Use adaptive sleep (1000 ms when far, every frame when near)"),
        dedent(
            "
        CreateThread(function()
            while true do
                local sleep = 1000
                local dist = #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0))
                if dist < 2.0 then
                    sleep = 0
                    -- near the shop
                    print('near')
                end
                Wait(sleep)
            end
        end)
    "
        )
    );
}

#[test]
fn adaptive_sleep_is_offered_without_highlighting_when_per_frame_work_only_runs_near() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wa<caret>it(0) -- every frame
                local coords = GetEntityCoords(PlayerPedId())
                for _, shop in ipairs(Config.Shops) do
                    if #(coords - shop.pos) < 10.0 then
                        DrawMarker(1, shop.pos.x, shop.pos.y, shop.pos.z)
                        if IsControlJustPressed(0, 38) then
                            TriggerEvent('shop:open', shop)
                        end
                    end
                end
            end
        end)
    ",
    );
    let found: Vec<Problem> = r.all().into_iter().filter(|p| p.code == checks::PER_FRAME_LOOP).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, openidea_plugin::Severity::Hint);
    assert!(!r.fixes_at_caret().contains(&"Wait 500 ms between iterations".to_owned()));
    assert_eq!(
        r.fixed("Use adaptive sleep (1000 ms when far, every frame when near)"),
        dedent(
            "
        CreateThread(function()
            while true do
                local sleep = 1000
                local coords = GetEntityCoords(PlayerPedId())
                for _, shop in ipairs(Config.Shops) do
                    if #(coords - shop.pos) < 10.0 then
                        sleep = 0
                        DrawMarker(1, shop.pos.x, shop.pos.y, shop.pos.z)
                        if IsControlJustPressed(0, 38) then
                            TriggerEvent('shop:open', shop)
                        end
                    end
                end
                Wait(sleep) -- every frame
            end
        end)
    "
        )
    );
}

#[test]
fn adaptive_sleep_fix_with_the_wait_in_the_middle_of_the_loop() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                local coords = GetEntityCoords(PlayerPedId())
                Wa<caret>it(0) -- one frame
                if #(coords - vector3(1.0, 2.0, 3.0)) < 5.0 then print('near') end
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Use adaptive sleep (1000 ms when far, every frame when near)"),
        dedent(
            "
        CreateThread(function()
            while true do
                local sleep = 1000
                local coords = GetEntityCoords(PlayerPedId())
                if #(coords - vector3(1.0, 2.0, 3.0)) < 5.0 then sleep = 0 print('near') end
                Wait(sleep) -- one frame
            end
        end)
    "
        )
    );
}

#[test]
fn near_only_suggestion_can_be_turned_off() {
    let text = "
        CreateThread(function()
            while true do
                Wait(0)
                if #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0)) < 10.0 then
                    DrawMarker(1, 0.0, 0.0, 0.0)
                end
            end
        end)
    ";
    assert_eq!(run(CLIENT, text).problems(checks::PER_FRAME_LOOP).len(), 1);
    let off = Settings::defaults().with(keys::SUGGEST_NEAR_ONLY, Value::from(false));
    assert!(run_with(CLIENT, text, off, catalog()).problems(checks::PER_FRAME_LOOP).is_empty());
}

#[test]
fn the_sleep_local_does_not_shadow_names_in_use() {
    let r = run(
        CLIENT,
        "
        local sleep = 'zzz'
        CreateThread(function()
            while true do
                local dist = #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0))
                if dist < 2.0 then print(sleep) end
                Wa<caret>it(0)
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Use adaptive sleep (1000 ms when far, every frame when near)"),
        dedent(
            "
        local sleep = 'zzz'
        CreateThread(function()
            while true do
                local sleep2 = 1000
                local dist = #(GetEntityCoords(PlayerPedId()) - vector3(1.0, 2.0, 3.0))
                if dist < 2.0 then sleep2 = 0 print(sleep) end
                Wait(sleep2)
            end
        end)
    "
        )
    );
}

#[test]
fn server_loops() {
    let r = run(
        SERVER,
        "
        CreateThread(function()
            while true do
                Wait(0)
                for _, id in ipairs(GetPlayers()) do
                    local coords = GetEntityCoords(GetPlayerPed(id))
                end
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
    ",
    );
    assert_eq!(r.problems(checks::PER_FRAME_LOOP).len(), 1);
}

#[test]
fn natives_are_classified_by_their_name_without_catalog() {
    let r = run_with(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                SetEntityHealth(PlayerPedId(), 200)
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsControlJustPressed(0, 38) then print('E') end
            end
        end)
    ",
        Settings::defaults(),
        Vec::new(),
    );
    assert_eq!(r.problems(checks::PER_FRAME_LOOP).len(), 1);
}

// 2. endless loops that never wait

#[test]
fn endless_loops_that_never_wait() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
        CreateThread(function()
            repeat
                local coords = GetEntityCoords(PlayerPedId())
            until false
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::LOOP_WITHOUT_WAIT),
        [
            "while: Endless loop that never waits: it blocks its thread and freezes the game",
            "repeat: Endless loop that never waits: it blocks its thread and freezes the game",
        ]
    );
}

#[test]
fn endless_loops_that_only_wait_on_some_paths() {
    let r = run(
        SERVER,
        "
        CreateThread(function()
            while true do
                if #GetPlayers() > 0 then
                    Wait(1000)
                end
            end
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::LOOP_WITHOUT_WAIT),
        ["while: An iteration of this endless loop can finish without waiting: when it does, it freezes the server"]
    );
}

#[test]
fn loops_waiting_on_every_path_leaving_or_calling_unknown_code_are_not_reported() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                if IsControlPressed(0, 38) then
                    Wait(0)
                elseif IsPedInAnyVehicle(PlayerPedId(), false) then
                    Citizen.Wait(100)
                else
                    Wait(1000)
                end
            end
        end)
        CreateThread(function()
            while true do
                for i = 1, 10 do
                    Wait(10)
                end
            end
        end)
        CreateThread(function()
            while true do
                local value = Citizen.Await(promise.new())
            end
        end)
        CreateThread(function()
            while true do
                if IsControlPressed(0, 38) then break end
            end
        end)
        CreateThread(function()
            while true do
                ESX.TriggerServerCallback('x', function() end)
            end
        end)
        CreateThread(function()
            repeat
                Wait(0)
            until false
        end)
        CreateThread(function()
            while true do
                error('stop')
            end
        end)
        local function sleep() Wait(500) end
        CreateThread(function()
            while true do
                sleep()
            end
        end)
    ",
    );
    assert!(r.problems(checks::LOOP_WITHOUT_WAIT).is_empty());
}

#[test]
fn loading_loops_that_never_wait() {
    let r = run(
        CLIENT,
        "
        local model = GetHashKey('adder')
        RequestModel(model)
        while not HasModelLoaded(model) do
            RequestModel(model)
        end
        repeat RequestAnimDict('dict') until HasAnimDictLoaded('dict')
        while not HasModelLoaded(model) do
            Wait(0)
        end
    ",
    );
    assert_eq!(
        r.problems(checks::LOOP_WITHOUT_WAIT),
        [
            "while: 'HasModelLoaded' only changes between frames: this loop never waits, so it never ends and freezes the game",
            "repeat: 'HasAnimDictLoaded' only changes between frames: this loop never waits, so it never ends and freezes the game",
        ]
    );
}

#[test]
fn insert_wait_fix() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            wh<caret>ile true do
                DrawMarker(1, 0.0, 0.0, 0.0) -- marker
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Insert Wait(0) at the end of the loop"),
        dedent(
            "
        CreateThread(function()
            while true do
                DrawMarker(1, 0.0, 0.0, 0.0) -- marker
                Wait(0)
            end
        end)
    "
        )
    );
}

#[test]
fn insert_wait_fix_in_an_empty_loading_loop_uses_the_file_spelling() {
    let r = run(
        CLIENT,
        "
        Citizen.CreateThread(function()
            Citizen.Wait(100)
            wh<caret>ile not HasModelLoaded(1) do
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Insert Citizen.Wait(0) at the end of the loop"),
        dedent(
            "
        Citizen.CreateThread(function()
            Citizen.Wait(100)
            while not HasModelLoaded(1) do
                Citizen.Wait(0)
            end
        end)
    "
        )
    );
}

// 3. repeated natives

#[test]
fn repeated_calls_in_a_per_frame_loop() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                if IsPedInAnyVehicle(PlayerPedId(), false) then
                    local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
                    DrawMarker(1, 0.0, 0.0, 0.0)
                end
            end
        end)
    ",
    );
    let message = "PlayerPedId(): 'PlayerPedId()' is called 2 times in the same frame: cache the value in a local";
    assert_eq!(r.problems(checks::REPEATED_NATIVE), [message, message]);
}

#[test]
fn calls_that_may_see_another_value_are_not_grouped() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                local a = GetEntityCoords(PlayerPedId())
                SetEntityCoords(PlayerPedId(), 0.0, 0.0, 0.0)
                local b = GetEntityCoords(PlayerPedId())
            end
        end)
        CreateThread(function()
            while true do
                local a = PlayerPedId()
                Wait(0)
                local b = PlayerPedId()
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsControlPressed(0, 38) then
                    print(GetGameTimer())
                else
                    print(GetGameTimer())
                end
            end
        end)
        CreateThread(function()
            local ped = 0
            while true do
                Wait(0)
                print(GetEntityCoords(ped))
                ped = GetVehiclePedIsIn(ped, false)
                print(GetEntityCoords(ped))
            end
        end)
    ",
    );
    let found = r.problems(checks::REPEATED_NATIVE);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|p| p.starts_with("PlayerPedId()")));
}

#[test]
fn loops_that_do_not_run_every_frame_are_reported_only_when_configured() {
    let text = "
        CreateThread(function()
            while true do
                Wait(1000)
                print(PlayerPedId(), PlayerPedId())
            end
        end)
    ";
    assert!(run(CLIENT, text).problems(checks::REPEATED_NATIVE).is_empty());
    let all = Settings::defaults().with(keys::ALL_LOOPS, Value::from(true));
    assert_eq!(run_with(CLIENT, text, all, catalog()).problems(checks::REPEATED_NATIVE).len(), 2);
}

#[test]
fn cache_in_a_local_fix() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                if IsPedInAnyVehicle(Player<caret>PedId(), false) then
                    local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
                end
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Cache 'PlayerPedId()' in local 'ped'"),
        dedent(
            "
        CreateThread(function()
            while true do
                Wait(0)
                local ped = PlayerPedId()
                if IsPedInAnyVehicle(ped, false) then
                    local vehicle = GetVehiclePedIsIn(ped, false)
                end
            end
        end)
    "
        )
    );
}

#[test]
fn existing_local_is_reused() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
                if #(coords - vector3(1.0, 2.0, 3.0)) < 5.0 then
                    print(GetEntityCoords(Player<caret>PedId()))
                end
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Use local 'coords' instead of calling 'GetEntityCoords(PlayerPedId())' again"),
        dedent(
            "
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
                if #(coords - vector3(1.0, 2.0, 3.0)) < 5.0 then
                    print(coords)
                end
            end
        end)
    "
        )
    );
}

#[test]
fn get_player_ped_minus_one_is_replaced_with_player_ped_id() {
    assert!(run(SERVER, "local ped = GetPlayerPed(-1)").problems(checks::REPEATED_NATIVE).is_empty());
    let r = run(CLIENT, "local ped = GetPlayer<caret>Ped(-1)");
    assert_eq!(
        r.problems(checks::REPEATED_NATIVE),
        ["GetPlayerPed(-1): 'GetPlayerPed(-1)' is a slower way to get the player's ped: use 'PlayerPedId()'"]
    );
    assert_eq!(r.fixed("Replace with PlayerPedId()"), "local ped = PlayerPedId()");
}

// 4. distance natives

fn replaced(call: &str) -> String {
    let text = format!("local a = vector3(1.0, 2.0, 3.0)\nlocal d = {}", call.replacen('(', &format!("{CARET}("), 1));
    let r = run(CLIENT, &text);
    let fix = r
        .fixes_at_caret()
        .into_iter()
        .find(|f| f.starts_with("Replace with"))
        .unwrap_or_else(|| panic!("no fix for {call}"));
    let out = r.fixed(&fix);
    out.split_once("local d = ").map(|(_, d)| d.to_owned()).unwrap_or_default()
}

#[test]
fn distance_natives_are_reported() {
    let r = run(
        CLIENT,
        "
        local a = GetDistanceBetweenCoords(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, true)
        local b = Vdist(1.0, 2.0, 3.0, 4.0, 5.0, 6.0)
        local c = #(vector3(1.0, 2.0, 3.0) - vector3(4.0, 5.0, 6.0))
    ",
    );
    assert_eq!(
        r.problems(checks::DISTANCE_NATIVE),
        [
            "GetDistanceBetweenCoords: 'GetDistanceBetweenCoords' calls into the game for simple math: compute the distance with vectors, #(a - b)",
            "Vdist: 'Vdist' calls into the game for simple math: compute the distance with vectors, #(a - b)",
        ]
    );
    assert!(
        run(SERVER, "local a = GetDistanceBetweenCoords(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, true)")
            .problems(checks::DISTANCE_NATIVE)
            .is_empty()
    );
}

#[test]
fn distance_replacements() {
    assert_eq!(
        replaced("GetDistanceBetweenCoords(x1, y1, z1, x2, y2, z2, true)"),
        "#(vector3(x1, y1, z1) - vector3(x2, y2, z2))"
    );
    assert_eq!(
        replaced("GetDistanceBetweenCoords(x1, y1, z1, x2, y2, z2, 1)"),
        "#(vector3(x1, y1, z1) - vector3(x2, y2, z2))"
    );
    assert_eq!(replaced("Vdist(x1, y1, z1, x2, y2, z2)"), "#(vector3(x1, y1, z1) - vector3(x2, y2, z2))");
    assert_eq!(
        replaced("GetDistanceBetweenCoords(x1, y1, z1, x2, y2, z2, false)"),
        "#(vector2(x1, y1) - vector2(x2, y2))"
    );
    assert_eq!(replaced("GetDistanceBetweenCoords(x1, y1, z1, x2, y2, z2)"), "#(vector2(x1, y1) - vector2(x2, y2))");
    assert_eq!(
        replaced("GetDistanceBetweenCoords(a.x, a.y, a.z, 4.0, 5.0, 6.0, true)"),
        "#(a - vector3(4.0, 5.0, 6.0))"
    );
    assert_eq!(
        replaced("GetDistanceBetweenCoords(a.x, a.y, a.z, 4.0, 5.0, 6.0, false)"),
        "#(a.xy - vector2(4.0, 5.0))"
    );
    assert_eq!(
        replaced("Vdist(GetEntityCoords(1).x, GetEntityCoords(1).y, GetEntityCoords(1).z, 4.0, 5.0, 6.0)"),
        "#(GetEntityCoords(1) - vector3(4.0, 5.0, 6.0))"
    );
    assert_eq!(replaced("Vdist(t.x, t.y, t.z, 4.0, 5.0, 6.0)"), "#(vector3(t.x, t.y, t.z) - vector3(4.0, 5.0, 6.0))");
    assert_eq!(replaced("Vdist(x1, y1, z1, x2, y2, z2) ^ 2"), "(#(vector3(x1, y1, z1) - vector3(x2, y2, z2))) ^ 2");
}

#[test]
fn no_distance_fix_when_the_meaning_would_change() {
    let r = run(CLIENT, "local d = GetDistanceBetweenCoords<caret>(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, useZ)");
    assert_eq!(r.problems(checks::DISTANCE_NATIVE).len(), 1);
    assert!(!r.fixes_at_caret().iter().any(|f| f.starts_with("Replace with")));
    let r = run(
        CLIENT,
        "local d = GetDistanceBetweenCoords<caret>(1.0, 2.0, SetEntityCoords(1, 0.0, 0.0, 0.0), 4.0, 5.0, 6.0, false)",
    );
    assert!(!r.fixes_at_caret().iter().any(|f| f.starts_with("Replace with")));
}

#[test]
fn distance_code_action_works_in_server_scripts() {
    let r = run(SERVER, "local d = Vdist(x1, y1, z1, x2, <caret>y2, z2)");
    let (doc, at) = r.fx.caret(&r.rel);
    let intentions = r.feature.intentions(&r.fx.cx(&doc), at, at);
    assert_eq!(intentions.len(), 1);
    assert_eq!(intentions[0].fix.title, "Replace Vdist with #(a - b)");
    assert_eq!(apply(doc.text(), &intentions[0].fix.edits), "local d = #(vector3(x1, y1, z1) - vector3(x2, y2, z2))");
}

// 5. per-frame spam

#[test]
fn spam_on_every_frame() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
                print(coords)
                if #(coords - vector3(1.0, 2.0, 3.0)) < 2.0 then
                    TriggerServerEvent('bank:near', json.encode({ x = coords.x }))
                end
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::PER_FRAME_SPAM),
        [
            "print: 'print' runs every frame here: it floods the console and costs time on each frame",
            "TriggerServerEvent: 'TriggerServerEvent' runs every frame here: it floods the server with network events",
            "json.encode: 'json.encode' runs every frame here: serializing on each frame wastes CPU time",
        ]
    );
}

#[test]
fn calls_that_run_once_in_a_while_are_not_reported() {
    let r = run(
        CLIENT,
        "
        local sent = false
        CreateThread(function()
            while true do
                Wait(0)
                if IsControlJustPressed(0, 38) then
                    TriggerServerEvent('press')
                end
                if not sent then
                    sent = true
                    print('once')
                end
                if Config.Debug then
                    print('debugging')
                end
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if IsPedInAnyVehicle(PlayerPedId(), false) then
                    print('adaptive loop')
                    Wait(1000)
                end
            end
        end)
        CreateThread(function()
            while true do
                Wait(500)
                print('throttled')
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                if ESX.IsReady() then print('unknown condition') end
            end
        end)
    ",
    );
    assert!(r.problems(checks::PER_FRAME_SPAM).is_empty());
}

#[test]
fn network_events_of_the_other_side_are_ignored() {
    let r = run(
        SERVER,
        "
        CreateThread(function()
            while true do
                Wait(0)
                TriggerClientEvent('tick', -1)
            end
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::PER_FRAME_SPAM),
        ["TriggerClientEvent: 'TriggerClientEvent' runs every frame here: it floods the clients with network traffic"]
    );
}

#[test]
fn remove_print_fix() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                pr<caret>int('tick') -- debug
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
    ",
    );
    assert_eq!(
        r.fixed("Remove the 'print' call"),
        dedent(
            "
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
    "
        )
    );
}

// 6. repeated registrations

#[test]
fn registrations_in_handlers_commands_and_loops() {
    let r = run(
        CLIENT,
        "
        RegisterNetEvent('job:update', function(job)
            AddEventHandler('job:changed', function() print(job) end)
        end)
        AddEventHandler('playerSpawned', function()
            CreateThread(function()
                while true do
                    Wait(1000)
                end
            end)
        end)
        RegisterCommand('track', function()
            RegisterNetEvent('track:pos', function() end)
        end)
        CreateThread(function()
            while true do
                Wait(1000)
                RegisterNUICallback('close', function(data, cb) cb('ok') end)
            end
        end)
    ",
    );
    assert_eq!(
        r.problems(checks::REPEATED_REGISTRATION),
        [
            "AddEventHandler: 'AddEventHandler' inside the 'job:update' handler: each 'job:update' event adds one more handler that is never removed",
            "CreateThread: 'CreateThread' starts an endless thread inside the 'playerSpawned' handler: each 'playerSpawned' event starts one more",
            "RegisterNetEvent: 'RegisterNetEvent' inside the 'track' command: each use of the command adds one more handler that is never removed",
            "RegisterNUICallback: 'RegisterNUICallback' inside a loop: each iteration adds one more handler that is never removed",
        ]
    );
}

#[test]
fn registrations_running_once_are_not_reported() {
    let r = run(
        CLIENT,
        "
        RegisterNetEvent('job:update')
        AddEventHandler('job:update', function() end)
        CreateThread(function()
            RegisterNetEvent('ready', function() end)
            while true do
                Wait(1000)
            end
        end)
        AddEventHandler('onClientResourceStart', function(resource)
            if resource ~= GetCurrentResourceName() then return end
            AddEventHandler('other', function() end)
        end)
        local handler
        AddEventHandler('vehicle:enter', function()
            if handler then RemoveEventHandler(handler) end
            handler = AddEventHandler('vehicle:leave', function() end)
        end)
        local registered = false
        AddEventHandler('esx:playerLoaded', function()
            if not registered then
                registered = true
                AddEventHandler('esx:setJob', function() end)
            end
            RegisterNetEvent('only:name')
        end)
        AddEventHandler('anim:play', function()
            CreateThread(function()
                Wait(5000)
                ClearPedTasks(PlayerPedId())
            end)
        end)
        for _, name in ipairs({ 'a', 'b' }) do
            AddEventHandler(name, function() end)
        end
    ",
    );
    assert!(r.problems(checks::REPEATED_REGISTRATION).is_empty());
}

// Loop cost markers

#[test]
fn cost_classes() {
    let r = run(
        CLIENT,
        "
        CreateThread(function()
            while true do
                Wait(0)
                DrawMarker(1, 0.0, 0.0, 0.0)
            end
        end)
        CreateThread(function()
            while true do
                local sleep = 1000
                if IsPedInAnyVehicle(PlayerPedId(), false) then sleep = 0 end
                Wait(sleep)
            end
        end)
        CreateThread(function()
            repeat
                Citizen.Wait(2000)
            until false
        end)
        CreateThread(function()
            while true do
                local job = Citizen.Await(promise.new())
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                local coords = GetEntityCoords(PlayerPedId())
            end
        end)
        CreateThread(function()
            while true do
                Wait(0)
                ESX.ShowNotification('hi')
            end
        end)
    ",
    );
    assert_eq!(
        r.markers(),
        [
            "1: Runs every frame (Wait(0)) — Per-frame work: DrawMarker",
            "7: Adaptive: every frame or every 1000 ms depending on the state",
            "14: Throttled: runs every 2000 ms",
            "19: Event-driven: resumes when 'Citizen.Await' returns",
            "24: Runs every frame (Wait(0)) — No per-frame work found: the loop could wait longer between iterations",
            "30: Runs every frame (Wait(0)) — Calls code the plugin can't follow: ESX.ShowNotification",
        ]
    );
    let doc = r.fx.doc(&r.rel);
    let markers = r.feature.line_markers(&r.fx.cx(&doc));
    assert_eq!(markers[0].icon.as_deref(), Some("run"));
    assert_eq!(markers[1].icon.as_deref(), Some("navigate"));
    assert_eq!(markers[0].target.as_ref().map(|t| t.range.start.line), Some(1));
}

#[test]
fn short_loops_have_no_icon() {
    let r = run(
        CLIENT,
        "
        RequestModel(1)
        while not HasModelLoaded(1) do
            Wait(0)
        end
        for i = 1, 10 do
            Wait(0)
        end
    ",
    );
    assert!(r.markers().is_empty());
}

#[test]
fn manifests_and_disabled_projects_are_not_checked() {
    assert_eq!(run(CLIENT, "while true do end").problems(checks::LOOP_WITHOUT_WAIT).len(), 1);
    assert!(run("other/fxmanifest.lua", "while true do end").all().is_empty());
    let off = Settings::defaults().with("enabled", Value::from(false));
    assert!(run_with(CLIENT, "while true do end", off, catalog()).all().is_empty());
}
