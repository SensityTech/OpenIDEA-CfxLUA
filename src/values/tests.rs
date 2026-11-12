use super::*;
use crate::natives::{Native, NativeSet};
use crate::settings::Settings;
use crate::testing::Fixture;
use serde_json::Value;
use sites::tests::with_params;

const FIVEM: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n";
const REDM: &str = "fx_version 'cerulean'\ngame 'rdr3'\nclient_script 'client/*.lua'\n";
const CLIENT: &str = "res/client/main.lua";

struct Run {
    fx: Fixture,
    feature: ValuesFeature,
}

fn run_full(manifest: &str, text: &str, natives: Vec<Native>, settings: Settings, extra: &[(&str, &str)]) -> Run {
    let mut files = vec![("res/fxmanifest.lua", manifest), (CLIENT, text)];
    files.extend_from_slice(extra);
    let fx = Fixture::new(&files).with_natives(natives).with_settings(settings);
    let feature = ValuesFeature::default();
    fx.index(&feature);
    Run { fx, feature }
}

fn run(text: &str) -> Run {
    run_full(FIVEM, text, Vec::new(), Settings::defaults(), &[])
}

impl Run {
    fn hints(&self) -> Vec<String> {
        let doc = self.fx.doc(CLIENT);
        self.feature
            .problems(&self.fx.cx(&doc))
            .into_iter()
            .map(|p| format!("{}: {}", &doc.text()[p.start..p.end], p.message))
            .collect()
    }

    fn labels(&self) -> Vec<String> {
        self.hints()
            .into_iter()
            .map(|h| {
                let message = h.split_once(": ").map_or("", |m| m.1).to_owned();
                let label = message.split_once(": ").map_or("", |m| m.1);
                label.split(" — ").next().unwrap_or("").to_owned()
            })
            .collect()
    }

    fn intentions(&self) -> Vec<Fix> {
        let (doc, at) = self.fx.caret(CLIENT);
        self.feature.intentions(&self.fx.cx(&doc), at, at).into_iter().map(|i| i.fix).collect()
    }

    fn apply(&self, title: &str) -> String {
        let doc = self.fx.doc(CLIENT);
        let fixes = self.intentions();
        let fix = fixes
            .iter()
            .find(|f| f.title == title)
            .unwrap_or_else(|| panic!("no {title} in {:?}", fixes.iter().map(|f| &f.title).collect::<Vec<_>>()));
        crate::perf::tests::apply(doc.text(), &fix.edits)
    }

    fn completion(&self) -> Vec<CompletionItem> {
        let (doc, at) = self.fx.caret(CLIENT);
        self.feature.completion(&self.fx.cx(&doc), at, None)
    }
}

#[test]
fn hints_of_the_curated_natives() {
    let r = run("IsControlJustPressed(0, 38)\n\
                 CreateVehicle(-1216765807, 0.0, 0.0, 0.0, 0.0, true, false)\n\
                 local h = 0xB779A091\n\
                 TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 49, 0, false, false, false)\n\
                 DrawMarker(1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 128, 0, 200, false)\n\
                 SetBlipSprite(blip, 1)\n\
                 SetBlipColour(blip, 1)\n\
                 GetHudColour(6)\n\
                 AddExplosion(0.0, 0.0, 0.0, 2, 1.0, true, false, 1.0)");
    assert_eq!(
        r.labels(),
        [
            "INPUT_PICKUP (E / LB)",
            "`adder`",
            "`adder`",
            "LOOPING | UPPERBODY | SECONDARY",
            "MarkerTypeVerticalCylinder",
            "radar_level",
            "Red",
            "HUD_COLOUR_RED",
            "STICKYBOMB",
        ]
    );
    assert_eq!(r.hints()[0], "38: Control 38: INPUT_PICKUP (E / LB) — Keyboard: E, Pad: LB");
    assert!(
        r.hints()[1].starts_with("-1216765807: Hash -1216765807: `adder` — Name from: the bundled names (vehicle)")
    );
    assert!(r.hints()[2].starts_with("0xB779A091: Hash 3078201489: `adder`"));
}

#[test]
fn redm_controls_and_explosions() {
    let pressed =
        with_params(NativeSet::Rdr3, "IsControlJustPressed", "PAD", &[("control", "int"), ("action", "Hash")]);
    let r = run_full(
        REDM,
        "IsControlJustPressed(0, 0xCEFD9220)\nAddExplosion(0.0, 0.0, 0.0, 25, 1.0, true, false, 1.0)",
        vec![pressed],
        Settings::defaults(),
        &[],
    );
    assert_eq!(r.labels(), ["INPUT_ENTER", "DYNAMITE"]);
    assert!(r.hints()[0].starts_with("0xCEFD9220: Control (hash) 3472724512: INPUT_ENTER — Signed: "));
}

#[test]
fn catalog_roles_are_used() {
    let pressed =
        with_params(NativeSet::Gta5, "IsControlJustPressed", "PAD", &[("padIndex", "int"), ("control", "int")]);
    let r = run_full(
        FIVEM,
        "IsControlJustPressed(0, 38)\nIsControlJustPressed(38, 1000)",
        vec![pressed],
        Settings::defaults(),
        &[],
    );
    assert_eq!(r.labels(), ["INPUT_PICKUP (E / LB)"]);
}

#[test]
fn no_hints() {
    let r = run("local function helper(a, b) end\nhelper(0, 38)\n\
                 local IsControlPressed = function() end\nIsControlPressed(0, 38)\n\
                 Unknown(0, 38)\nlocal small = 1234\nlocal h = 123456789\n\
                 IsControlJustPressed(0, 38 --[[ INPUT_PICKUP ]])\nDrawRect(0.5, 0.5, 0.1, 0.1, 255, 0, 0, 255)");
    assert!(r.hints().is_empty(), "{:?}", r.hints());
    let off = Settings::defaults().with("enabled", Value::from(false));
    assert!(run_full(FIVEM, "IsControlJustPressed(0, 38)", Vec::new(), off, &[]).hints().is_empty());
    let off = Settings::defaults().with(keys::HINTS, Value::from(false));
    assert!(run_full(FIVEM, "IsControlJustPressed(0, 38)", Vec::new(), off, &[]).hints().is_empty());
    let controls_off = Settings::defaults().with(keys::CONTROLS, Value::from(false));
    let r = run_full(FIVEM, "IsControlJustPressed(0, 38)\nSetBlipSprite(blip, 1)", Vec::new(), controls_off, &[]);
    assert_eq!(r.labels(), ["radar_level"]);
}

#[test]
fn project_strings_name_hashes() {
    let hash = joaat("my_custom_prop");
    let text = format!("local h = {hash}<caret>");
    let r = run_full(
        FIVEM,
        &text,
        Vec::new(),
        Settings::defaults(),
        &[("res/client/props.lua", "local p = 'my_custom_prop'")],
    );
    assert_eq!(r.labels(), ["`my_custom_prop`"]);
    assert!(r.hints()[0].contains("Name from: a string of the project"));
    assert_eq!(r.apply("Replace with `my_custom_prop`"), "local h = `my_custom_prop`");
}

#[test]
fn indexed_names() {
    let src = Source::new(
        "local a = 'my_prop'\nlocal b = `WEAPON_PISTOL`\nlocal c = [[long_string]]\nlocal d = \"has space\"\nlocal e = \"esc\\\\aped\"\nlocal f = 'x'",
    );
    let mut names: Vec<String> = names_of(&src).into_iter().map(|(_, n)| n).collect();
    names.sort();
    assert_eq!(names, ["WEAPON_PISTOL", "long_string", "my_prop"]);
    let mut index = HashIndex::default();
    index.set("/a.lua", vec![(1, "b_name".into())]);
    index.set("/b.lua", vec![(1, "a_name".into())]);
    assert_eq!(index.find(1), Some("a_name"));
    index.remove("/b.lua");
    assert_eq!(index.find(1), Some("b_name"));
    index.set("/a.lua", Vec::new());
    assert_eq!(index.find(1), None);
    assert!(index.names.is_empty() && index.files["/a.lua"].is_empty());

    // A spelling stays as long as a file writes it, and is kept once however many do.
    index.set("/a.lua", vec![(1, "b_name".into()), (2, "other".into())]);
    index.set("/b.lua", vec![(1, "b_name".into()), (1, "a_name".into())]);
    index.set("/c.lua", vec![(1, "c_name".into()), (1, "b_name".into())]);
    assert_eq!(
        index.names[&1].iter().map(|(name, files)| (&**name, *files)).collect::<Vec<_>>(),
        [("a_name", 1), ("b_name", 3), ("c_name", 1)]
    );
    assert_eq!(Rc::strong_count(&index.names[&1][1].0), 4, "the files share the one copy");
    index.remove("/b.lua");
    assert_eq!((index.find(1), index.find(2)), (Some("b_name"), Some("other")));
    index.set("/a.lua", vec![(1, "b_name".into())]);
    index.remove("/c.lua");
    assert_eq!((index.find(1), index.find(2), index.names[&1][0].1), (Some("b_name"), None, 1));
    index.remove("/a.lua");
    assert!(index.names.is_empty() && index.files.is_empty());
}

#[test]
fn escapes_of_hash_literals() {
    assert_eq!(unescape("a\\x41\\65\\u{48}\\n"), Some(b"aAAH\n".to_vec()));
    assert_eq!(unescape("\\u{20AC}"), Some("€".as_bytes().to_vec()));
    assert_eq!(unescape("bad\\q"), None);
    assert_eq!(unescape("\\256"), None);
}

#[test]
fn annotate_intention() {
    let r = run("IsControlJustPressed(0, 3<caret>8)");
    assert_eq!(r.apply("Add '--[[ INPUT_PICKUP ]]' comment"), "IsControlJustPressed(0, 38 --[[ INPUT_PICKUP ]])");
    assert!(!r.intentions().iter().any(|f| f.title.starts_with("Replace with")), "enum values are not hashes");
    let r = run("TaskPlayAnim(ped, 'dict', 'anim', 8.0, -8.0, -1, 49<caret>, 0, false, false, false)");
    assert_eq!(r.intentions()[0].title, "Add '--[[ AF_LOOPING | AF_UPPERBODY | AF_SECONDARY ]]' comment");
    let r = run("IsControlJustPressed(0, 3<caret>8 --[[ INPUT_PICKUP ]])");
    assert!(r.intentions().is_empty());
}

#[test]
fn replace_hash_intention() {
    let r = run("CreateVehicle(-121676<caret>5807, 0.0, 0.0, 0.0)");
    assert_eq!(r.apply("Add '--[[ adder ]]' comment"), "CreateVehicle(-1216765807 --[[ adder ]], 0.0, 0.0, 0.0)");
    assert_eq!(r.apply("Replace with `adder`"), "CreateVehicle(`adder`, 0.0, 0.0, 0.0)");
    let pressed =
        with_params(NativeSet::Rdr3, "IsControlJustPressed", "PAD", &[("control", "int"), ("action", "Hash")]);
    let r = run_full(REDM, "IsControlJustPressed(0, 0xCEFD<caret>9220)", vec![pressed], Settings::defaults(), &[]);
    assert_eq!(r.apply("Replace with `INPUT_ENTER`"), "IsControlJustPressed(0, `INPUT_ENTER`)");
    assert!(run("local h = 12345<caret>6789").intentions().is_empty());
}

#[test]
fn completion_of_enumerated_arguments() {
    let r = run("IsControlJustPressed(0, INPUT_PICK<caret>)");
    let items = r.completion();
    assert_eq!(items.len(), 361);
    let pickup = items.iter().find(|i| i.label == "INPUT_PICKUP").unwrap();
    assert_eq!((pickup.insert_text.as_str(), pickup.detail.as_deref()), ("38", Some("= 38 (E / LB)")));
    assert_eq!(pickup.sort_text.as_deref(), Some("00038"));
    assert!(pickup.documentation.as_deref().is_some_and(|d| d.contains("Argument 2 of IsControlJustPressed")));
    let doc = r.fx.doc(CLIENT);
    let range = pickup.replace.unwrap();
    let (a, b) = (doc.offset(range.start), doc.offset(range.end));
    let mut text = doc.text().to_owned();
    text.replace_range(a..b, &pickup.insert_text);
    assert_eq!(text, "IsControlJustPressed(0, 38)");
    assert_eq!(items.first().map(|i| i.label.as_str()), Some("INPUT_NEXT_CAMERA"), "table order");
    assert!(run("IsControlJustPressed(INPUT_PICK<caret>, 0)").completion().is_empty());
    assert_eq!(run("SetBlipColour(blip, <caret>)").completion()[1].detail.as_deref(), Some("= 1 (#e03232)"));
    assert!(run("TaskPlayAnim(ped, 'd', 'a', 8.0, -8.0, -1, <caret>)").completion().is_empty());
    let r = run_full(REDM, "AddExplosion(0.0, 0.0, 0.0, <caret>)", Vec::new(), Settings::defaults(), &[]);
    assert_eq!(r.completion().len(), 36);
    let r = run_full(REDM, "SetBlipSprite(blip, <caret>)", Vec::new(), Settings::defaults(), &[]);
    assert!(r.completion().is_empty(), "GTA V tables only");
}

#[test]
fn the_token_test_never_hides_a_value() {
    let text = "IsControlJustPressed(0, INPUT_PICK)\nSetBlipColour(blip, )\nCreateThread(function()\n    \
                SetBlipColour(blip, col)\n    local x = IsControlPressed(0, )\nend)\n\
                DrawMarker(1, f(a, { b }), IsControlJustPressed(0, IN), 0.0)\nSetBlipColour(blip, \nlocal done = 1\n\
                IsControlJustPressed(0, ";
    let r = run(text);
    let doc = r.fx.doc(CLIENT);
    let cx = r.fx.cx(&doc);
    let mut offered = 0;
    for at in 0..=text.len() {
        let values = ValuesFeature::argument_values(&cx, at);
        assert_eq!(r.feature.completion(&cx, at, None).len(), values.len(), "at {at}: `{}`", &text[..at]);
        offered += usize::from(!values.is_empty());
    }
    assert!(offered > 10, "values are offered at {offered} places of the sample");
}
