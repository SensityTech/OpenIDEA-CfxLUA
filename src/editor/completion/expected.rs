//! Completion of the values the place of an expression asks for: the strings of a literal type
//! (`SmartCompletionContributor`), the members of an enum and the keys of an enum of keys
//! (`LuaCatsCompletionContributor`).

use super::Listing;
use crate::index::EnumKind;
use crate::lua::Source;
use crate::lua::tree::N;
use crate::ty::context::{Cx, Psi};
use crate::ty::declarations::should_be;
use crate::ty::members;
use crate::ty::{ANONYMOUS, FileId, GLOBAL, Ty, literal_value};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::rc::Rc;

/// The first character of the sort key: nothing answers a place better than what it asks for.
const EXPECTED: char = '0';

/// What a string of a literal type is shown with, where an enum shows its name.
const LITERAL: &str = "literal";

struct Enum {
    name: Rc<str>,
    /// `---@enum (key)`: its keys are the values, as strings.
    keys: bool,
    /// What its members are read through: what its table is given to, or else its own name.
    access: Rc<str>,
    members: Vec<Rc<str>>,
}

/// The enums among the classes of a type, each with the members its own table writes.
fn enums(cx: &Cx, expected: &Ty) -> Vec<Enum> {
    let mut found = Vec::new();
    for ty in expected.each() {
        let Ty::Class(class) = ty else { continue };
        if class.flags & (ANONYMOUS | GLOBAL) != 0 {
            continue;
        }
        let Some(def) = cx.index.class(&class.name).filter(|def| def.enum_kind != EnumKind::No) else { continue };
        let own = members::visible(cx, ty).into_iter().filter(|listed| listed.class == *ty);
        found.push(Enum {
            name: class.name.clone(),
            keys: def.enum_kind == EnumKind::Keys,
            access: def.access.clone().unwrap_or_else(|| class.name.clone()),
            members: own.map(|listed| listed.member.name().into()).collect(),
        });
    }
    found
}

/// The strings a literal type lists, in the order it lists them.
fn literals(expected: &Ty) -> impl Iterator<Item = &str> {
    expected.each().iter().filter_map(|ty| match ty {
        Ty::StringLiteral(content) => Some(literal_value(content)),
        _ => None,
    })
}

/// Where a name is typed: the strings of an expected literal type, written with their quotes, the members of
/// an expected enum read through its table, the keys of an enum of keys as strings.
pub(super) fn values(listing: &mut Listing, here: Psi) {
    let expected = should_be(listing.cx, here);
    let mut entries: Vec<CompletionItem> = Vec::new();
    let quoted = |value: &str, of: Option<&str>| {
        let kind = if of.is_some() { CompletionKind::EnumMember } else { CompletionKind::Value };
        let mut item = CompletionItem::new(&format!("\"{value}\""), kind);
        item.filter_text = Some(value.to_owned());
        item.detail = of.map(str::to_owned);
        item
    };
    entries.extend(literals(&expected).map(|value| quoted(value, None)));
    for found in enums(listing.cx, &expected) {
        for member in &found.members {
            if found.keys {
                entries.push(quoted(member, Some(&found.name)));
                continue;
            }
            let mut item = CompletionItem::new(&format!("{}.{member}", found.access), CompletionKind::EnumMember);
            item.detail = Some(found.name.to_string());
            entries.push(item);
        }
    }
    for (rank, mut item) in entries.into_iter().enumerate() {
        item.sort_text = Some(format!("{EXPECTED}{rank:03}"));
        listing.push(item);
    }
}

/// Inside the quoted string of the code token `token`: the strings of the literal type its place asks for,
/// and the keys of an expected enum of keys. They replace what is typed of the string.
pub(super) fn strings(listing: &mut Listing, file: FileId, src: &Source, token: usize, at: usize) {
    let quote = src.toks[token];
    if src.text[quote.start + 1..at].contains(['\n', '\\']) {
        return;
    }
    let literal = src.root().tree().node_at(&src.text, quote.start);
    if !literal.is(N::LiteralExpr) {
        return;
    }
    let expected = should_be(listing.cx, Psi { file, src, node: literal });
    let mut values: Vec<(Rc<str>, Option<Rc<str>>)> = literals(&expected).map(|value| (value.into(), None)).collect();
    for found in enums(listing.cx, &expected).into_iter().filter(|found| found.keys) {
        values.extend(found.members.iter().map(|member| (member.clone(), Some(found.name.clone()))));
    }
    let replace = listing.doc.range(quote.start + 1, at);
    let mut seen: Vec<Rc<str>> = Vec::new();
    for (value, of) in values {
        if seen.contains(&value) {
            continue;
        }
        seen.push(value.clone());
        let kind = if of.is_some() { CompletionKind::EnumMember } else { CompletionKind::Value };
        let mut item = CompletionItem::new(&value, kind);
        item.detail = Some(of.as_deref().unwrap_or(LITERAL).to_owned());
        item.replace = Some(replace);
        item.sort_text = Some(format!("{EXPECTED}{:03}", seen.len()));
        listing.push(item);
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed, labels};
    use openidea_plugin::providers::CompletionKind;

    /// The synthetic library the JetBrains plugin tests its LuaCATS completion on (`LuaCatsCompletionTest`).
    const LIBRARY: (&str, &str) = ("shop/shared/synthlib.lua", include_str!("../../lua/doc/testdata/synthlib.lua"));
    const POSITIONS: [&str; 6] = ["top", "top-right", "top-left", "bottom", "bottom-right", "bottom-left"];

    fn at(text: &str) -> Vec<String> {
        labels(&[MANIFEST, LIBRARY, (MAIN, text)])
    }

    /// The expected values listed: what sorts before every name.
    fn expected(text: &str) -> Vec<String> {
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, text)]);
        items
            .into_iter()
            .filter(|item| item.sort_text.as_deref().is_some_and(|key| key.starts_with('0')))
            .map(|item| item.label)
            .collect()
    }

    #[test]
    fn the_strings_of_a_literal_type_inside_a_string() {
        assert_eq!(at("lib.showTextUI('Press E', '<caret>')"), POSITIONS);
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.showTextUI('Press E', \"top-<caret>\")")]);
        assert_eq!(items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), POSITIONS, "the editor filters");
        let range = items[1].replace.expect("a range");
        assert_eq!((range.start.character, range.end.character), (27, 31), "what is typed of the string");
        assert_eq!((items[1].insert_text.as_str(), items[1].detail.as_deref()), ("top-right", Some("literal")));
        assert_eq!(at("lib.notify({ title = 'Saved', type = '<caret>' })"), ["inform", "error", "success", "warning"]);
        assert_eq!(at("lib.notify({ position = '<caret>' })"), POSITIONS);
        assert_eq!(at("lib.count('<caret>')"), ["ped", "vehicle", "object"]);
        assert_eq!(at("lib.showTextUI('Press E', 'to<caret>"), POSITIONS, "a string that is not closed yet");
    }

    #[test]
    fn the_keys_of_an_enum_of_keys_inside_a_string() {
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.setMode(SynthKeybindMode.PRESS, '<caret>')")]);
        let shown: Vec<(&str, Option<&str>)> =
            items.iter().map(|item| (item.label.as_str(), item.detail.as_deref())).collect();
        assert_eq!(shown, [("CLEAR", Some("SynthWeather")), ("RAIN", Some("SynthWeather"))]);
        assert_eq!(items[0].kind, Some(CompletionKind::EnumMember));
        assert!(at("lib.setMode('<caret>')").is_empty(), "the members of an enum of values are no strings");
    }

    #[test]
    fn nothing_in_a_string_no_type_asks_for() {
        for text in [
            "local a = '<caret>'",
            "lib.showTextUI('<caret>')",
            "lib.showTextUI('a', 'top\\\\<caret>')",
            "lib.unknown('<caret>')",
            "lib.showTextUI('a', [[<caret>]])",
        ] {
            assert!(at(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn the_members_of_an_expected_enum() {
        let listed = expected("lib.setMode(<caret>)");
        assert_eq!(listed, ["SynthKeybindMode.PRESS", "SynthKeybindMode.HOLD", "SynthKeybindMode.TOGGLE"]);
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.setMode(TOGG<caret>)")]);
        let toggle = items.iter().find(|item| item.label == "SynthKeybindMode.TOGGLE").expect("the member");
        assert_eq!(
            (toggle.insert_text.as_str(), toggle.detail.as_deref()),
            ("SynthKeybindMode.TOGGLE", Some("SynthKeybindMode"))
        );
        assert_eq!(at("local m = SynthKeybindMode.<caret>"), ["HOLD", "PRESS", "TOGGLE"]);

        let keys = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.setMode(SynthKeybindMode.PRESS, <caret>)")]);
        let clear = keys.iter().find(|item| item.label == "\"CLEAR\"").expect("the key, as a string");
        assert_eq!((clear.insert_text.as_str(), clear.filter_text.as_deref()), ("\"CLEAR\"", Some("CLEAR")));

        let local = "---@enum Mode\nlocal Modes = { ON = 1, OFF = 2 }\n---@param mode Mode\nlocal function set(mode) end\nset(<caret>)";
        assert_eq!(expected(local), ["Modes.ON", "Modes.OFF"], "read through the name the table is given to");
    }

    #[test]
    fn a_string_of_a_literal_type_where_no_quote_is_typed() {
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.showTextUI('Press E', bot<caret>)")]);
        let quoted: Vec<&str> = items
            .iter()
            .filter(|item| item.kind == Some(CompletionKind::Value))
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            quoted,
            ["\"top\"", "\"top-right\"", "\"top-left\"", "\"bottom\"", "\"bottom-right\"", "\"bottom-left\""]
        );
        let left = items.iter().find(|item| item.label == "\"bottom-left\"").expect("the value");
        assert_eq!((left.insert_text.as_str(), left.filter_text.as_deref()), ("\"bottom-left\"", Some("bottom-left")));
        assert!(expected("local a = <caret>").is_empty(), "nothing is asked for there");
    }
}
