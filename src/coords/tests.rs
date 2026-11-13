use super::*;
use crate::natives::{Native, Param};
use crate::project::Side;
use crate::settings::Settings;
use crate::testing::{Fixture, native};
use serde_json::Value;

const FIVEM: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";

fn with_params(name: &str, params: &[&str]) -> Native {
    let mut n = native(NativeSet::Gta5, name, Side::Client);
    n.params = params
        .iter()
        .map(|p| Param {
            name: (*p).into(),
            lua_type: "number".into(),
            native_type: "float".into(),
            ..Param::default()
        })
        .collect();
    n
}

fn catalog() -> Vec<Native> {
    let mut coords = native(NativeSet::Gta5, "GetEntityCoords", Side::Client);
    coords.returns = vec![crate::natives::Return {
        name: "retval".into(),
        lua_type: "vector3".into(),
        native_type: "Vector3".into(),
        description: None,
    }];
    vec![
        with_params("SetEntityCoords", &["entity", "xPos", "yPos", "zPos", "xAxis", "yAxis", "zAxis", "clearArea"]),
        with_params("CreatePed", &["pedType", "modelHash", "x", "y", "z", "heading", "isNetwork", "bScriptHostPed"]),
        coords,
    ]
}

struct Run {
    fx: Fixture,
    feature: CoordsFeature,
}

fn run_with(manifest: &str, text: &str, settings: Settings, extra: &[(&str, &str)]) -> Run {
    let mut files = vec![("res/fxmanifest.lua", manifest), (CLIENT, text)];
    files.extend_from_slice(extra);
    let fx = Fixture::new(&files).with_natives(catalog()).with_settings(settings);
    let feature = CoordsFeature::default();
    fx.index(&feature);
    Run { fx, feature }
}

fn run(text: &str) -> Run {
    run_with(FIVEM, text, Settings::defaults(), &[])
}

impl Run {
    fn titles(&self) -> Vec<String> {
        let (doc, at) = self.fx.caret(CLIENT);
        self.feature.intentions(&self.fx.cx(&doc), at, at).into_iter().map(|i| i.fix.title).collect()
    }

    fn apply(&self, title: &str) -> String {
        let (doc, at) = self.fx.caret(CLIENT);
        let all = self.feature.intentions(&self.fx.cx(&doc), at, at);
        let fix =
            all.iter().find(|i| i.fix.title == title).unwrap_or_else(|| panic!("no {title} in {:?}", self.titles()));
        crate::perf::tests::apply(doc.text(), &fix.fix.edits)
    }

    fn problems(&self, code: &str) -> Vec<String> {
        let doc = self.fx.doc(CLIENT);
        let mut found = self.feature.problems(&self.fx.cx(&doc));
        found.sort_by_key(|p| p.start);
        found
            .into_iter()
            .filter(|p| p.code == code)
            .map(|p| format!("{}: {}", &doc.text()[p.start..p.end], p.message))
            .collect()
    }
}

fn converted(text: &str, title: &str) -> String {
    run(&format!("local c = {text}")).apply(title).trim_start_matches("local c = ").to_owned()
}

#[test]
fn conversions() {
    assert_eq!(converted("{x = 1.5,<caret> y = -2.5, z = 3.5}", "Convert to vector3(…)"), "vector3(1.5, -2.5, 3.5)");
    assert_eq!(converted("{1.5, 2.5, 3.5, 90.0}<caret>", "Convert to vector4(…)"), "vector4(1.5, 2.5, 3.5, 90.0)");
    assert_eq!(converted("{x = 1, y = 2, z = 3, heading = 4<caret>}", "Convert to vector4(…)"), "vector4(1, 2, 3, 4)");
    assert_eq!(converted("vector3(a, <caret>b, c)", "Convert to {x = …, y = …, z = …}"), "{ x = a, y = b, z = c }");
    assert_eq!(
        converted("vec4(1.0, 2.0, 3.0, 4.0)<caret>", "Convert to {x = …, y = …, z = …, w = …}"),
        "{ x = 1.0, y = 2.0, z = 3.0, w = 4.0 }"
    );
    assert_eq!(converted("vector3(1.0, 2.0, 3.0<caret>)", "Convert to {x, y, z} array"), "{ 1.0, 2.0, 3.0 }");
    assert_eq!(converted("{x = 1.0, y = 2.0, z = <caret>3.0}", "Convert to {x, y, z} array"), "{ 1.0, 2.0, 3.0 }");
    for not in [
        "{a, <caret>b, c}",
        "{x = 1, <caret>y = 2}",
        "{x = 1, y = 2, z = 3, <caret>name = 4}",
        "vector2(1.0, <caret>2.0)",
        "vector3(<caret>v)",
        "vector(1.0, <caret>v)",
        "print(1.0, <caret>2.0, 3.0)",
    ] {
        assert!(run(&format!("local c = {not}")).titles().is_empty(), "{not}");
    }
    let titles = run("local c = vector3(1.0, 2.0, 3.0<caret>)").titles();
    assert!(!titles.contains(&"Convert to vector3(…)".to_owned()), "{titles:?}");
}

#[test]
fn rounding() {
    assert_eq!(
        converted("vector4(-268.41234, -957.305, 31.2, 90<caret>)", "Round coordinates to 2 decimals"),
        "vector4(-268.41, -957.31, 31.2, 90)"
    );
    let zero = Settings::defaults().with(keys::ROUND_DECIMALS, Value::from(0));
    let r = run_with(FIVEM, "local c = {x = 1.6, y = x + 1.25, z = -0.4<caret>}", zero, &[]);
    assert_eq!(r.apply("Round coordinates to 0 decimals"), "local c = {x = 2.0, y = x + 1.25, z = 0.0}");
    let one = Settings::defaults().with(keys::ROUND_DECIMALS, Value::from(1));
    assert!(
        run_with(FIVEM, "local c = vector3(1.25, 2.0, 3.0<caret>)", one, &[])
            .titles()
            .contains(&"Round coordinates to 1 decimal".to_owned())
    );
    for not in ["vector3(1, 2, <caret>3)", "vector3(a, b, <caret>c)"] {
        assert!(!run(&format!("local c = {not}")).titles().iter().any(|t| t.starts_with("Round")), "{not}");
    }
}

#[test]
fn headings() {
    assert_eq!(
        converted("vector3(1.0, 2.0, 3.0<caret>)", "Add heading (convert to vector4)"),
        "vector4(1.0, 2.0, 3.0, 0.0)"
    );
    assert_eq!(converted("vec3(1.0, 2.0, 3.0<caret>)", "Add heading (convert to vec4)"), "vec4(1.0, 2.0, 3.0, 0.0)");
    assert_eq!(
        converted("{x = 1.0, y = 2.0, z = 3.0<caret>}", "Add heading"),
        "{ x = 1.0, y = 2.0, z = 3.0, w = 0.0 }"
    );
    assert_eq!(
        converted("vector4(1.0, 2.0, 3.0, 90.0<caret>)", "Remove heading (convert to vector3)"),
        "vector3(1.0, 2.0, 3.0)"
    );
    assert_eq!(
        converted("{x = 1.0, y = 2.0, z = 3.0, h = 90.0<caret>}", "Remove heading"),
        "{ x = 1.0, y = 2.0, z = 3.0 }"
    );
    assert_eq!(converted("{1.0, 2.0, 3.0, 90.0<caret>}", "Remove heading"), "{ 1.0, 2.0, 3.0 }");
}

#[test]
fn spread_vector_arguments() {
    let r = run("SetEntityCoords(ped, vector3(1.5, <caret>2.5, 3.5), false)");
    assert_eq!(r.apply("Spread into x, y, z arguments"), "SetEntityCoords(ped, 1.5, 2.5, 3.5, false)");
    let r = run("SetEntityCoords(ped, {x = 1.5, y = 2.5, z = 3.5, w = 9.0}<caret>)");
    assert_eq!(r.apply("Spread into x, y, z arguments"), "SetEntityCoords(ped, 1.5, 2.5, 3.5)");
    let r = run("local spawn = vector4(1.0, 2.0, 3.0, 4.0)\nCreatePed(4, model, sp<caret>awn, true)");
    assert_eq!(
        r.apply("Spread into x, y, z arguments"),
        "local spawn = vector4(1.0, 2.0, 3.0, 4.0)\nCreatePed(4, model, spawn.x, spawn.y, spawn.z, spawn.w, true)"
    );
    let r = run("local coords = GetEntityCoords(ped)\nSetEntityCoords(ped, coords<caret>)");
    assert_eq!(
        r.apply("Spread into x, y, z arguments"),
        "local coords = GetEntityCoords(ped)\nSetEntityCoords(ped, coords.x, coords.y, coords.z)"
    );
    let r = run_with(
        FIVEM,
        "MoveTo(ped, vector3(1.0, 2.0, 3.0)<caret>)",
        Settings::defaults(),
        &[("res/client/util.lua", "function MoveTo(ped, posX, posY, posZ) end")],
    );
    assert_eq!(r.apply("Spread into x, y, z arguments"), "MoveTo(ped, 1.0, 2.0, 3.0)");
    let spread = |text: &str| run(text).titles().contains(&"Spread into x, y, z arguments".to_owned());
    assert!(!spread("SetEntityCoords(vector3(1.0, 2.0, 3.0)<caret>)"), "index 0 is the entity");
    assert!(!spread("local function TeleportTo(target) end\nTeleportTo(vector3(1.0, 2.0, 3.0)<caret>)"));
    assert!(!spread("local n = 5\nSetEntityCoords(ped, n<caret>)"));
    assert!(!spread("SetEntityCoords(ped, GetPos()<caret>)"));
}

#[test]
fn zero_z() {
    let r = run("local a = vector3(-1037.74, -2738.03, 0.0)\nlocal b = {x = 213.8, y = -808.5, z = 0}\n\
                 local c = vector3(-1037.74, -2738.03, 1.0)\nlocal d = vector3(0.0, 0.0, 0.0)\nlocal e = vector3(1.0, 2.0, 0.0)\n\
                 local offset = vector3(150.0, 200.0, 0.0)\nlocal t = { markerRotation = vector3(180.0, 200.0, 0.0) }\n\
                 local tint = vector3(255, 120, 0)\nlocal f = {150.0, 200.0, 0.0}\nlocal g = vector3(x, 200.0, 0.0)");
    assert_eq!(
        r.problems(checks::ZERO_Z),
        ["0.0: z is 0.0: was the height forgotten?", "0: z is 0.0: was the height forgotten?"]
    );
}

#[test]
fn out_of_bounds() {
    let r = run(
        "local a = vector3(100.0, 12000.0, 30.0)\nlocal b = vector3(-9000.0, 0.0, 30.0)\nlocal c = {x = 1.0, y = 2.0, z = 5000.0}\n\
                 local scale = vector3(1.0, 1.0, 9999.0)\nlocal arr = {1.0, 99999.0, 3.0}\nlocal d = vector3(99999.0, 99999.0, 1.0)",
    );
    assert_eq!(
        r.problems(checks::OUT_OF_BOUNDS),
        [
            "12000.0: y is outside of the FiveM map (-8000 to 10000)",
            "-9000.0: x is outside of the FiveM map (-8000 to 8000)",
            "5000.0: z is outside of the FiveM map (-1000 to 3000)",
            "99999.0: x is outside of the FiveM map (-8000 to 8000)",
        ]
    );
    let redm = "fx_version 'cerulean'\ngame 'rdr3'\nclient_script 'client/*.lua'\n";
    let r = run_with(redm, "local a = vector3(100.0, 6000.0, 30.0)", Settings::defaults(), &[]);
    assert_eq!(r.problems(checks::OUT_OF_BOUNDS), ["6000.0: y is outside of the RedM map (-8000 to 5000)"]);
    let both = "fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\nclient_script 'client/*.lua'\n";
    let r = run_with(both, "local a = vector3(100.0, 12000.0, 30.0)", Settings::defaults(), &[]);
    assert_eq!(r.problems(checks::OUT_OF_BOUNDS), ["12000.0: y is outside of the FiveM + RedM map (-8000 to 10000)"]);
}

#[test]
fn duplicates_in_config_tables() {
    let text = "Config.Shops = {\n\
                    a = { coords = vector3(29.5, -1340.2, 29.5), blip = vector3(1.0, 2.0, 3.0) },\n\
                    b = { coords = vector3(29.5, -1340.2, 29.5), blip = vector3(4.0, 5.0, 6.0) },\n\
                    Uptown = { coords = vector3(29.50, -1340.2, 29.5) },\n\
                    c = { coords = vector3(1.0, 2.0, 3.0) },\n\
                }\n\
                Config.Spawns = {\n    vector4(1, 2, 3, 90),\n    {x = 1, y = 2, z = 3, w = 90},\n}\n\
                Config.Other = { vector4(1, 2, 3, 90) }\n\
                local Lists = { a = { vector3(1.0, 2.0, 3.0), vector3(1.0, 2.0, 3.0) } }\n\
                local Config = {\n    Garages = { vector3(5.0, 5.0, 5.0), vector3(5.0, 5.0, 5.0) },\n    Hospitals = { vector3(5.0, 5.0, 5.0) },\n}";
    let r = run(text);
    let found = r.problems(checks::DUPLICATE);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found[0].ends_with("Same coordinates as line 2"));
    assert!(found[1].starts_with("vector3(29.50") && found[1].ends_with("line 2"));
    assert!(found[2].starts_with("{x = 1") && found[2].ends_with("line 8"));
    assert!(found[3].ends_with("line 14"));
    let settings = Settings::defaults().with(keys::CONFIG_TABLES, Value::from("Settings"));
    let r = run_with(
        FIVEM,
        "Config.Spots = { vector3(1.0, 2.0, 3.0), vector3(1.0, 2.0, 3.0) }\nSettings.Spots = { vector3(1.0, 2.0, 3.0), vector3(1.0, 2.0, 3.0) }",
        settings,
        &[],
    );
    assert_eq!(r.problems(checks::DUPLICATE).len(), 1);
}

#[test]
fn manifests_are_not_checked() {
    let r = run_with("local a = vector3(-1037.74, -2738.03, 0.0)", "", Settings::defaults(), &[]);
    let doc = r.fx.doc("res/fxmanifest.lua");
    assert!(r.feature.problems(&r.fx.cx(&doc)).is_empty());
}

fn command(text: &str, from: &str, to: &str, arg: Option<&str>) -> (CommandResult, String) {
    let mock = openidea_plugin::mock::MockHost::install();
    let fx = Fixture::new(&[("res/fxmanifest.lua", FIVEM), (CLIENT, text)]).with_natives(catalog());
    let feature = CoordsFeature::default();
    fx.index(&feature);
    let (a, b) = (text.find(from).unwrap(), text.rfind(to).unwrap() + to.len());
    let pos = |at: usize| {
        let line = text[..at].matches('\n').count() as u32;
        let col = (at - text[..at].rfind('\n').map_or(0, |i| i + 1)) as u32;
        (line, col)
    };
    let ((l1, c1), (l2, c2)) = (pos(a), pos(b));
    let path = Fixture::path(CLIENT);
    mock.open_editor(&path, "lua", text, &[(l1, c1, l2, c2)]);
    let doc = fx.doc(CLIENT);
    let inv = mock.invocation();
    let cx = CommandCx { ws: Some(&fx.ws), settings: &fx.settings, natives: &fx.natives, doc: Some(&doc), inv: &inv };
    let result = feature.command(&cx, CONVERT_SELECTION, arg).unwrap();
    (result, mock.editor_text())
}

#[test]
fn convert_selection_command() {
    let (r, text) = command("local pos = 123.4, 56.7, 8.9", "123.4", "8.9", None);
    assert_eq!((r, text.as_str()), (Ok(None), "local pos = vector3(123.4, 56.7, 8.9)"));
    let (_, text) = command("SetEntityCoords(ped, vector3(1.5, 2.5, 3.5))", "vector3", "3.5)", None);
    assert_eq!(text, "SetEntityCoords(ped, 1.5, 2.5, 3.5)");
    let (_, text) = command("local pos = vector3(1.5, 2.5, 3.5)", "vector3", "3.5)", Some("keyed"));
    assert_eq!(text, "local pos = { x = 1.5, y = 2.5, z = 3.5 }");
    let (_, text) = command("-- 1.5, 2.5, 3.5", "1.5", "3.5", None);
    assert_eq!(text, "-- vector3(1.5, 2.5, 3.5)", "no site: one value");
    let (r, text) = command("local pos = hello", "hello", "hello", None);
    assert_eq!((r, text.as_str()), (Ok(Some("No coordinates found in the selection".into())), "local pos = hello"));
    let (r, _) = command("local pos = 1.5, 2.5, 3.5", "1.5", "3.5", Some("hex"));
    assert!(matches!(r, Err(Error::Invalid(_))));
    let mock = openidea_plugin::mock::MockHost::install();
    let inv = mock.invocation();
    let (settings, natives) = (Settings::defaults(), crate::natives::Natives::from_natives(Vec::new()));
    let cx = CommandCx { ws: None, settings: &settings, natives: &natives, doc: None, inv: &inv };
    assert!(CoordsFeature::default().command(&cx, "other", None).is_none());
    assert_eq!(
        CoordsFeature::default().command(&cx, CONVERT_SELECTION, None),
        Some(Ok(Some("Select coordinates to convert".into())))
    );
}
