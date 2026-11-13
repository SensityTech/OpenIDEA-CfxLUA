use super::name_at;
use crate::editor::EditorFeature;
use crate::feature::Feature;
use crate::index::project::Budget;
use crate::lua::Source;
use crate::manifest::edits::apply;
use crate::testing::{CARET, Fixture};
use std::time::Duration;

const TIME: Budget = Budget { time: Duration::from_secs(5), read: 4 << 20 };
const MANIFEST: (&str, &str) = ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nshared_script '*.lua'\n");

/// The files that change once the member at the `<caret>` is renamed `renamed`, as `file: text`, in the order
/// they are given.
fn renamed(files: &[(&str, &str)]) -> Vec<String> {
    let all: Vec<(String, &str)> = files.iter().map(|(name, text)| (format!("shop/{name}"), *text)).collect();
    let mut given: Vec<(&str, &str)> = vec![MANIFEST];
    given.extend(all.iter().map(|(rel, text)| (rel.as_str(), *text)));
    let fx = Fixture::new(&given);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let (asked, _) = all.iter().find(|(_, text)| text.contains(CARET)).expect("a <caret>");
    let (doc, at) = fx.caret(asked);
    let member = feature.member_at(&fx.cx(&doc), at, TIME).expect("a member at the caret");
    let mut out = Vec::new();
    for ((rel, _), (name, _)) in all.iter().zip(files) {
        let doc = fx.doc(rel);
        let (uses, _) = feature.member_uses(&fx.cx(&doc), &member, TIME).expect("told in time");
        let edits: Vec<_> = uses.into_iter().map(|(start, end)| (start, end, "renamed".to_owned())).collect();
        if !edits.is_empty() {
            out.push(format!("{name}: {}", apply(doc.text(), &edits)));
        }
    }
    out
}

const SHOP: &str =
    "---@class Shop\nShop = {}\n\nfunction Shop:open() end\n\nfunction Shop:close()\n    self:open()\nend\n";
const VISIT: &str = "---@param shop Shop\nlocal function visit(shop)\n    shop:open()\n    shop.open(shop)\n    \
                     shop['open'](shop)\nend\n\n---@type Shop\nlocal mine = Shop\nmine:open()\nDoor:open()\n\
                     unknown:open()\n";

#[test]
fn a_method_is_renamed_where_a_value_of_its_class_calls_it() {
    let shop = SHOP.replace("Shop:open", "Shop:op<caret>en");
    let expected = [
        format!("shop.lua: {}", SHOP.replace("open", "renamed")),
        "visit.lua: ---@param shop Shop\nlocal function visit(shop)\n    shop:renamed()\n    shop.renamed(shop)\n    \
         shop['renamed'](shop)\nend\n\n---@type Shop\nlocal mine = Shop\nmine:renamed()\nDoor:open()\nunknown:open()\n"
            .to_owned(),
    ];
    assert_eq!(renamed(&[("shop.lua", &shop), ("visit.lua", VISIT)]), expected);
    let visit = VISIT.replace("mine:open", "mine:op<caret>en");
    assert_eq!(renamed(&[("shop.lua", SHOP), ("visit.lua", &visit)]), expected, "from a call through a value");
}

const BASE: &str = "---@class Base\nBase = {}\nfunction Base:run() end\n";
const CHILD: &str = "---@class Child : Base\nChild = {}\nfunction Child:run() end\n";
const GRAND: &str = "---@class Grand : Child\nGrand = {}\nfunction Grand:run() end\n";
const OTHER: &str = "---@class Other : Base\nOther = {}\nfunction Other:run() end\n";
const CALLS: &str = "---@param b Base\n---@param c Child\n---@param g Grand\n---@param o Other\n\
                     local function all(b, c, g, o)\n    b:run()\n    c:run()\n    g:run()\n    o:run()\nend\n";

fn family(asked: &str) -> Vec<String> {
    let with_caret = |text: &str| match text.contains(&format!("function {asked}:run")) {
        true => text.replace(":run() end", ":r<caret>un() end"),
        false => text.to_owned(),
    };
    let texts = [BASE, CHILD, GRAND, OTHER, CALLS].map(with_caret);
    let names = ["base.lua", "child.lua", "grand.lua", "other.lua", "calls.lua"];
    let files: Vec<(&str, &str)> = names.into_iter().zip(texts.iter().map(String::as_str)).collect();
    renamed(&files)
}

#[test]
fn a_method_is_renamed_with_those_that_override_it_and_those_it_overrides() {
    // `RenameLuaMethodProcessor`
    let after = |text: &str| text.replace("run", "renamed");
    let calls = |kept: &str| after(CALLS).replace(&format!("{kept}:renamed()"), &format!("{kept}:run()"));
    assert_eq!(
        family("Base"),
        [
            format!("base.lua: {}", after(BASE)),
            format!("child.lua: {}", after(CHILD)),
            format!("grand.lua: {}", after(GRAND)),
            format!("other.lua: {}", after(OTHER)),
            format!("calls.lua: {}", after(CALLS)),
        ]
    );
    assert_eq!(
        family("Child"),
        [
            format!("base.lua: {}", after(BASE)),
            format!("child.lua: {}", after(CHILD)),
            format!("grand.lua: {}", after(GRAND)),
            format!("calls.lua: {}", calls("o")),
        ],
        "a method of another branch is neither one it overrides nor one that overrides it"
    );
}

#[test]
fn a_field_is_renamed_from_its_tag_to_its_uses() {
    let shop = "---@class Shop\n---@field label string\nShop = {}\nShop.label = 'x'\n";
    let uses = "---@type Shop\nlocal mine = Shop\nprint(mine.la<caret>bel, Shop.label)\nlocal other = { label = 1 }\n";
    assert_eq!(
        renamed(&[("shop.lua", shop), ("uses.lua", uses)]),
        [
            "shop.lua: ---@class Shop\n---@field renamed string\nShop = {}\nShop.renamed = 'x'\n",
            "uses.lua: ---@type Shop\nlocal mine = Shop\nprint(mine.renamed, Shop.renamed)\nlocal other = { label = 1 }\n",
        ]
    );
}

#[test]
fn a_field_of_a_table_is_renamed_where_another_name_reads_the_table() {
    let config = "Config = {\n    deb<caret>ug = false,\n    ['level'] = 1,\n}\nprint(Config.debug)\n";
    let uses = "if Config.debug then end\nlocal c = Config\nprint(c.debug, c['debug'], c.level)\n";
    assert_eq!(
        renamed(&[("config.lua", config), ("uses.lua", uses)]),
        [
            "config.lua: Config = {\n    renamed = false,\n    ['level'] = 1,\n}\nprint(Config.renamed)\n",
            "uses.lua: if Config.renamed then end\nlocal c = Config\nprint(c.renamed, c['renamed'], c.level)\n",
        ]
    );
}

#[test]
fn the_name_of_a_member_is_told_from_where_it_is_written() {
    let text = "---@field label string\nlocal t = { key = 1, ['quoted'] = 2 }\nfunction t:method() end\n\
                print(t.key, t['quoted'], name, 'text', t[index])\n";
    let src = Source::new(text);
    let named = |word: &str, nth: usize| {
        let at = text.match_indices(word).nth(nth).map(|(at, _)| at).unwrap_or_else(|| panic!("no {word}"));
        name_at(&src, at + 1).map(|(start, end)| &text[start..end])
    };
    assert_eq!(named("label", 0), Some("label"));
    assert_eq!((named("key", 0), named("key", 1)), (Some("key"), Some("key")));
    assert_eq!((named("quoted", 0), named("quoted", 1)), (Some("quoted"), Some("quoted")));
    assert_eq!(named("method", 0), Some("method"));
    assert_eq!((named("name", 0), named("text", 0), named("index", 0), named("string", 0)), (None, None, None, None));
    assert_eq!((named("t", 0), named("print", 0)), (None, None), "a local and a global are no members");
}

#[test]
fn what_could_not_be_told_in_full_is_not_told_at_all() {
    // What `make` returns is read in its body, in a file bigger than one question may read.
    let filler = "-- a line that says nothing\n".repeat(20_000);
    let maker = format!("function make()\n    return Shop\nend\n{filler}");
    let uses = "local made = make()\nmade:open()\n";
    let files = [MANIFEST, ("shop/shop.lua", SHOP), ("shop/maker.lua", maker.as_str()), ("shop/uses.lua", uses)];
    let fx = Fixture::new(&files);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let shop = fx.doc("shop/shop.lua");
    let at = SHOP.find("open").expect("the method");
    let member = feature.member_at(&fx.cx(&shop), at, TIME).expect("the method of the class");
    let (own, cost) = feature.member_uses(&fx.cx(&shop), &member, TIME).expect("told in its own file");
    assert_eq!((own.len(), cost.read, cost.short), (2, 0, false));
    let asked = fx.doc("shop/uses.lua");
    let little = Budget { read: 100_000, ..TIME };
    assert_eq!(feature.member_uses(&fx.cx(&asked), &member, little), None, "a file left unread");
    let (found, cost) = feature.member_uses(&fx.cx(&asked), &member, TIME).expect("told once it may read it");
    assert_eq!((found, cost.read), (vec![(25, 29)], maker.len() + SHOP.len()), "what was read is counted");
}
