//! Completion of the name of a method being declared: the methods of the classes its class inherits from,
//! which it may define again (`OverrideCompletionProvider`).

use super::Listing;
use super::members::context_class;
use crate::index::MemberKind;
use crate::ty::context::Psi;
use crate::ty::{Ty, members};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::HashSet;
use std::rc::Rc;

/// The first character of the sort key: nothing else is named where a method is declared.
const OVERRIDE: char = '0';

/// The methods the parents of the class of the method `name` belongs to declare, and that the class does not
/// define yet. An entry writes the parameters and closes the function.
pub(super) fn methods(listing: &mut Listing, name: Psi) {
    let cx = listing.cx;
    let class = context_class(cx, name);
    let own = members::visible(cx, &class).into_iter().filter(|listed| listed.class == class);
    let mut known: HashSet<Rc<str>> = own.map(|listed| listed.member.name().into()).collect();
    for parent in members::supers(cx, &class) {
        let others = cx.index.other_names(&parent, cx.options.global_name_as_type);
        for kept_under in std::iter::once(parent).chain(others) {
            for found in cx.index.own(&kept_under) {
                let member = found.member;
                let Some(Ty::Function(function)) = &member.ty else { continue };
                if member.kind != MemberKind::Method || !known.insert(member.name.clone()) {
                    continue;
                }
                let mut params: Vec<&str> = function.main.params.iter().map(|param| &*param.name).collect();
                let stops: Vec<String> =
                    params.iter().enumerate().map(|(rank, param)| format!("${{{}:{param}}}", rank + 1)).collect();
                if function.main.vararg.is_some() {
                    params.push("...");
                }
                let rest = if function.main.vararg.is_some() { vec!["...".to_owned()] } else { Vec::new() };
                let written: Vec<String> = stops.into_iter().chain(rest).collect();
                let mut item = CompletionItem::new(&member.name, CompletionKind::Method);
                item.detail = Some(format!("({})", params.join(", ")));
                item.insert_text = format!("{}({})$0\nend", member.name, written.join(", "));
                item.snippet = true;
                item.sort_text = Some(format!("{OVERRIDE}{}", member.name.to_lowercase()));
                listing.push(item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed};

    const CLASSES: (&str, &str) = (
        "shop/shared/classes.lua",
        "---@class Base\nBase = {}\nfunction Base:open(door, ...) end\nfunction Base:close() end\nfunction Base.create(name) end\n\
         Base.count = 1\n\n---@class Mid : Base\nMid = {}\nfunction Mid:open(door) end\nfunction Mid:lock(key) end\n\n\
         ---@class Shop : Mid\nShop = {}\nfunction Shop:close() end\n",
    );

    /// Each entry as `label detail -> inserted`.
    fn offered(text: &str) -> Vec<String> {
        let items = completed(&[MANIFEST, CLASSES, (MAIN, text)]);
        let entry = |item: &openidea_plugin::providers::CompletionItem| {
            format!("{} {} -> {}", item.label, item.detail.as_deref().unwrap_or_default(), item.insert_text)
        };
        items.iter().map(entry).collect()
    }

    #[test]
    fn the_methods_of_the_parents_the_class_does_not_define() {
        let listed = offered("function Shop:<caret>");
        assert_eq!(
            listed,
            [
                "create (name) -> create(${1:name})$0\nend",
                "lock (key) -> lock(${1:key})$0\nend",
                "open (door) -> open(${1:door})$0\nend",
            ],
            "the nearest parent that declares a method gives it; `close` is defined, `count` is no method"
        );
        assert!(completed(&[MANIFEST, CLASSES, (MAIN, "function Shop:<caret>")]).iter().all(|item| item.snippet));
        assert_eq!(offered("function Shop:lo<caret>"), listed, "the editor filters what is typed");
        assert_eq!(offered("function Shop.<caret>"), listed, "after a dot too");
        let varargs = offered("function Mid:<caret>");
        assert_eq!(
            varargs,
            ["close () -> close()$0\nend", "create (name) -> create(${1:name})$0\nend"],
            "what a class defines is not offered to it again"
        );
        assert!(offered("function Base:<caret>").is_empty(), "a class without a parent");
        assert!(offered("function Unknown:<caret>").is_empty());
    }

    #[test]
    fn the_extra_arguments_of_a_method_are_written_as_they_are() {
        let classes = (
            "shop/shared/classes.lua",
            "---@class Base\nBase = {}\nfunction Base:open(door, ...) end\n---@class Shop : Base\nShop = {}\n",
        );
        let items = completed(&[MANIFEST, classes, (MAIN, "function Shop:op<caret>")]);
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].detail.as_deref(), items[0].insert_text.as_str()),
            (Some("(door, ...)"), "open(${1:door}, ...)$0\nend")
        );
    }
}
