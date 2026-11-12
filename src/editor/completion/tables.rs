//! Completion of the keys of a table constructor: the fields of the class its place asks for, and the names
//! of the metamethods (`TableCompletionProvider`).

use super::Listing;
use super::members::class_name;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::Psi;
use crate::ty::declarations::should_be;
use crate::ty::members::{self, Listed};
use crate::ty::{display, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// The first character of the sort key of a field: after what the place asks for, before the names.
const KEY: char = '1';
/// The metamethods come last: a table is rarely a metatable.
const META: char = '9';

const METAMETHODS: &[(&str, &str)] = &[
    ("__add", "a + b"),
    ("__sub", "a - b"),
    ("__mul", "a * b"),
    ("__div", "a / b"),
    ("__mod", "a % b"),
    ("__pow", "a ^ b"),
    ("__unm", "-a"),
    ("__concat", "a .. b"),
    ("__len", "#a"),
    ("__eq", "a == a"),
    ("__lt", "a < b"),
    ("__le", "a <= b"),
    ("__index", "Meta method"),
    ("__newindex", "Meta method"),
    ("__call", "Meta method"),
    ("__tostring", "Meta method"),
    ("__metatable", "Meta method"),
];

/// The table a field is written in, when the name typed in `field` may be its key: `{ name }`, `{ name = v }`
/// or the CfxLua set field `{ .name }`. `name` is the node the typed name belongs to.
pub(super) fn keyed_by(name: Node) -> Option<Node> {
    let field = match name.kind() {
        N::TableField => name,
        // A name alone in a field: nothing tells yet whether it is a value or the start of `key = value`.
        N::NameExpr => {
            name.parent().filter(|field| field.is(N::TableField) && psi::field_value(*field) == Some(name))?
        }
        _ => return None,
    };
    let keyless = name.is(N::TableField) || !field.has(T::Assign);
    field.parent().filter(|table| keyless && table.is(N::TableExpr))
}

/// The keys the table `table` may be given, where the name of the field `field` is typed.
pub(super) fn keys(listing: &mut Listing, table: Psi, field: Node) {
    let cx = listing.cx;
    // A set field stores true, and a field that has its value needs no second `=`.
    let assign = if field.has(T::Dot) || field.has(T::Assign) { "" } else { " = " };
    let present: Vec<String> = table
        .node
        .children_of(N::TableField)
        .filter(|other| *other != field)
        .filter_map(|other| psi::field_name(table.src, other).map(|name| name.into_owned()))
        .collect();
    let expected = should_be(cx, table);
    for class in expected.top_classes() {
        for Listed { member, class: declaring } in members::visible(cx, class) {
            let name = member.name();
            // A method is declared with `function`, not written in a table.
            if present.iter().any(|key| key == name) || member.is_function() {
                continue;
            }
            let mut item = CompletionItem::new(&format!("{name}{assign}"), CompletionKind::Field);
            let owner = class_name(&declaring).map_or(String::new(), |class| format!("  [{class}]"));
            item.detail = Some(format!("{}{owner}", display(&members::type_of(cx, &member))));
            let inherited = u8::from(declaring != *class);
            item.sort_text = Some(format!("{KEY}{inherited}{}", name.to_lowercase()));
            listing.push(item);
        }
    }
    for (name, means) in METAMETHODS {
        let mut item = CompletionItem::new(name, CompletionKind::Property);
        item.detail = Some((*means).to_owned());
        item.sort_text = Some(format!("{META}{name}"));
        listing.push(item);
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed};
    use openidea_plugin::providers::{CompletionItem, CompletionKind};

    const LIBRARY: (&str, &str) = ("shop/shared/synthlib.lua", include_str!("../../lua/doc/testdata/synthlib.lua"));

    /// The keys offered: the fields, as `label detail`.
    fn fields(text: &str) -> Vec<String> {
        let entry = |item: &CompletionItem| format!("{}{}", item.label, item.detail.as_deref().unwrap_or_default());
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, text)]);
        items.iter().filter(|item| item.kind == Some(CompletionKind::Field)).map(entry).collect()
    }

    #[test]
    fn the_fields_of_the_class_a_table_is_expected_to_be() {
        // `LuaCatsCompletionTest`: option table keys
        let listed = fields("lib.notify({ <caret> })");
        assert_eq!(
            listed,
            [
                "description = string  [SynthNotifyProps]",
                "duration = number  [SynthNotifyProps]",
                "icon = string|string[]  [SynthNotifyProps]",
                "id = SynthId  [SynthNotifyProps]",
                "position = SynthPosition  [SynthNotifyProps]",
                "style = table<string, string|number>  [SynthNotifyProps]",
                "title = string  [SynthNotifyProps]",
                "type = SynthNotifyType  [SynthNotifyProps]",
            ]
        );
        let set = fields("lib.notify({ title = 'Saved', <caret> })");
        assert!(
            set.iter().any(|key| key.starts_with("description = ")) && !set.iter().any(|key| key.starts_with("title"))
        );
        let typed = fields("lib.notify({ title = 'Saved', de<caret> })");
        assert_eq!(typed, set, "the editor filters what is typed");
        let named = fields("lib.notify({ ti<caret> = 'Saved' })");
        assert_eq!(named.len(), 8, "the name of a field that has its value: {named:?}");
        let local = fields("---@param point SynthPoint\nlocal function add(point) end\nadd({ <caret> })");
        assert_eq!(
            local,
            [
                "coords = vector3  [SynthPoint]",
                "distance = number  [SynthPoint]",
                "onEnter = fun(self: SynthPoint): void  [SynthPoint]"
            ]
        );
        let assigned = fields("---@type SynthPoint\nlocal point = { <caret> }");
        assert_eq!(assigned.len(), 3, "what a table is assigned to tells its class");
    }

    #[test]
    fn a_set_field_takes_the_name_alone() {
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.notify({ .ti<caret> })")]);
        let title = items.iter().find(|item| item.label == "title").expect("the field");
        assert_eq!(title.insert_text, "title");
    }

    #[test]
    fn the_metamethods_and_what_else_a_value_may_be() {
        let items = completed(&[MANIFEST, LIBRARY, (MAIN, "local shop = 1\nlocal meta = { __<caret> }")]);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"__index") && labels.contains(&"__tostring"), "{labels:?}");
        let index = items.iter().find(|item| item.label == "__index").expect("the metamethod");
        assert_eq!((index.detail.as_deref(), index.kind), (Some("Meta method"), Some(CompletionKind::Property)));
        assert!(labels.contains(&"shop") && labels.contains(&"function"), "a name alone may be a value: {labels:?}");
        let value = completed(&[MANIFEST, LIBRARY, (MAIN, "lib.notify({ title = ti<caret> })")]);
        assert!(
            !value.iter().any(|item| item.label.ends_with(" = ") || item.label == "__index"),
            "no key where a value is typed"
        );
        let nowhere = completed(&[MANIFEST, LIBRARY, (MAIN, "local t = { [1] = <caret> }")]);
        assert!(!nowhere.iter().any(|item| item.label == "__index"));
    }
}
