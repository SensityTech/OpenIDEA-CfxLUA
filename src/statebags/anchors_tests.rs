use super::anchors;
use crate::lua::Source;
use crate::refactor::edges::Family;
use crate::statebags::sites::SERVER;

const SECOND: &str = "AddEventHandler('playerJoining', function()
    Player(source).state.test = 'joined'
end)
";

const HANDLERS: &str = "AddStateBagChangeHandler('test', nil, function(bagName, key, value) end)
print(LocalPlayer.state.test, GlobalState.weather)
";

const CLIENT: &str = "local v = LocalPlayer.state.test\n";

/// Each anchor of a text: its key and the text it covers.
fn written(text: &str) -> Vec<(String, String)> {
    anchors(&Source::new(text))
        .into_iter()
        .map(|anchor| (anchor.key, text[anchor.start..anchor.end].to_owned()))
        .collect()
}

fn anchor(key: &str, name: &str) -> (String, String) {
    (key.to_owned(), name.to_owned())
}

#[test]
fn find_usages_of_a_key_gathers_its_writes_reads_and_handlers() {
    let files = [
        ("res/client/handlers.lua", HANDLERS),
        ("res/client/main.lua", CLIENT),
        ("res/server/main.lua", SERVER),
        ("res/server/second.lua", SECOND),
    ];
    let mut usages = Vec::new();
    for (path, text) in files {
        let src = Source::new(text);
        for anchor in anchors(&src).into_iter().filter(|anchor| anchor.key == "cfx.statebag:player#test") {
            assert_eq!(anchor.family, Family::Cfx);
            assert_eq!(&text[anchor.start..anchor.end], "test");
            usages.push(format!("{path}:{}", src.lines.line_of(anchor.start) + 1));
        }
    }
    assert_eq!(
        usages,
        [
            "res/client/handlers.lua:1",
            "res/client/handlers.lua:2",
            "res/client/main.lua:1",
            "res/server/main.lua:3",
            "res/server/second.lua:2",
        ]
    );
}

#[test]
fn every_key_of_a_file_is_anchored_under_the_kind_of_its_bag() {
    assert_eq!(
        written(SERVER),
        [
            anchor("cfx.statebag:player#test", "test"),
            anchor("cfx.statebag:player#test2", "test2"),
            anchor("cfx.statebag:player#job", "job"),
            anchor("cfx.statebag:player#grade", "grade"),
            anchor("cfx.statebag:global#weather", "weather"),
            anchor("cfx.statebag:entity#fuel", "fuel"),
        ]
    );
}

#[test]
fn a_bag_of_unknown_kind_gives_one_anchor_per_kind() {
    let text = "AddStateBagChangeHandler('fuel', nil, function() end)\n";
    assert_eq!(
        written(text),
        [
            anchor("cfx.statebag:entity#fuel", "fuel"),
            anchor("cfx.statebag:global#fuel", "fuel"),
            anchor("cfx.statebag:player#fuel", "fuel"),
        ]
    );
    let ranges: Vec<(usize, usize)> = anchors(&Source::new(text)).iter().map(|a| (a.start, a.end)).collect();
    assert!(ranges.windows(2).all(|pair| pair[0] == pair[1]), "{ranges:?}");
    let entity = "AddStateBagChangeHandler('fuel', 'entity:' .. id, function() end)\n";
    assert_eq!(written(entity), [anchor("cfx.statebag:entity#fuel", "fuel")]);
}

#[test]
fn a_key_written_as_an_identifier_or_a_string_gives_the_same_key() {
    let text = "print(LocalPlayer.state.fuel)
print(LocalPlayer.state['fuel'])
print(LocalPlayer.state[\"fuel\"])
print(LocalPlayer.state[ [[fuel]] ])
LocalPlayer.state:set('fuel', 1, true)
";
    let fuel = anchor("cfx.statebag:player#fuel", "fuel");
    assert_eq!(written(text), [fuel.clone(), fuel.clone(), fuel.clone(), fuel.clone(), fuel]);
}

#[test]
fn a_dynamic_or_escaped_key_gives_no_anchor() {
    let text = r#"print(LocalPlayer.state[key])
print(LocalPlayer.state['fu\101l'])
print(LocalPlayer.state['fu' .. 'el'])
LocalPlayer.state:set(name, 1, true)
print(GlobalState[''])
"#;
    assert_eq!(written(text), []);
}

#[test]
fn a_file_without_state_bags_gives_nothing() {
    assert_eq!(written("local state = {}\nstate.fuel = 1\n"), []);
    assert_eq!(written("print('fuel')\n"), []);
}
