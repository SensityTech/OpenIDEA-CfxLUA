//! Completion inside a documentation comment: the names of the tags, the parameters of the function it
//! documents, the classes and the aliases where a type is written (`LuaDocCompletionContributor`).

use super::{LIBRARY, Listing};
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::lexer::Tok;
use crate::lua::token::{T, is_id_part};
use crate::lua::tree::{N, Node};
use crate::ty::{FileId, Ty, members, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// The tags the grammar reads, then those it only names (`LuaParserDefinition.DOC_TAG_TOKENS`, and the
/// additional tags of the contributor).
const TAGS: &[&str] = &[
    "param",
    "return",
    "class",
    "module",
    "type",
    "field",
    "language",
    "overload",
    "private",
    "protected",
    "public",
    "see",
    "generic",
    "vararg",
    "alias",
    "enum",
    "cast",
    "deprecated",
    "author",
    "version",
    "since",
    "async",
    "nodiscard",
    "meta",
    "diagnostic",
    "operator",
    "package",
    "source",
];

/// The types of the language: the JetBrains plugin reads them from its standard library, which declares a
/// class for each.
const BUILTIN: &[&str] =
    &["any", "boolean", "function", "integer", "nil", "number", "string", "table", "thread", "userdata", "void"];

/// Where the word the caret stands in starts, when the caret is in a documentation comment, `---`.
pub fn start(src: &Source, at: usize) -> Option<usize> {
    let before = src.all.partition_point(|token| token.start < at).checked_sub(1)?;
    let comment = src.all[before];
    let text = src.text.as_str();
    if comment.kind != Tok::LineComment || at > comment.end || !comment.text(text).starts_with("---") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut start = at;
    while start > comment.start + 3 && is_id_part(bytes[start - 1]) {
        start -= 1;
    }
    Some(start)
}

fn item(label: &str, kind: CompletionKind) -> CompletionItem {
    let mut item = CompletionItem::new(label, kind);
    item.sort_text = Some(format!("0{}", label.to_lowercase()));
    item
}

/// The parameters of the function a comment documents: the statement it is bound to, or the function that
/// statement gives a name to.
fn documented_params<'a>(src: &'a Source, comment: Node) -> Vec<&'a str> {
    let owner = src.root().tree().node_at(&src.text, comment.range().0);
    let function = if psi::is_function(owner) {
        Some(owner)
    } else {
        let values = owner.child(N::ExprList).filter(|_| matches!(owner.kind(), N::LocalDef | N::AssignStat));
        values.and_then(Node::expr).filter(|value| value.is(N::ClosureExpr))
    };
    function.map_or(Vec::new(), |function| psi::params(function).map(Node::code).collect())
}

/// The classes and the aliases of the project, and the types of the language. A name with a dot is written
/// from the start of the reference: the editor would only replace what follows the last dot.
fn types(listing: &mut Listing, reference: Node, at: usize) {
    let index = listing.cx.index;
    let replace = listing.doc.range(reference.code_range().0, at);
    let dotted = |mut entry: CompletionItem| {
        if entry.label.contains('.') {
            entry.replace = Some(replace);
        }
        entry
    };
    // The types of the language first: the library declares them as classes too, and what is listed under a
    // name is listed once.
    for name in BUILTIN {
        listing.push(item(name, CompletionKind::Keyword));
    }
    // A type of a library comes after those of the project, and one of the other side is left out.
    let files = listing.cx.files;
    let placed = |mut entry: CompletionItem, file: FileId| {
        if files.is_library(file) {
            entry.sort_text = Some(format!("{LIBRARY}{}", entry.label.to_lowercase()));
        }
        dotted(entry)
    };
    for (name, file) in index.class_names().filter(|(_, file)| files.visible(*file)) {
        listing.push(placed(item(name, CompletionKind::Class), file));
    }
    for (name, file) in index.alias_names().filter(|(_, file)| files.visible(*file)) {
        listing.push(placed(item(name, CompletionKind::Interface), file));
    }
}

/// The members of a class, by name; `typed` is the word being written, which the document may already
/// declare as one of them.
fn members_of(listing: &mut Listing, class: &str, fields_only: bool, typed: &str) {
    let cx = listing.cx;
    for listed in members::visible(cx, &Ty::class(class)) {
        if fields_only && listed.member.is_function() || listed.member.name() == typed {
            continue;
        }
        let kind = if listed.member.is_function() { CompletionKind::Method } else { CompletionKind::Field };
        listing.push(item(listed.member.name(), kind));
    }
}

/// What a word that starts at `start`, in a documentation comment, may be.
pub(super) fn complete(listing: &mut Listing, src: &Source, start: usize, at: usize) {
    let text = src.text.as_str();
    if start > 0 && text.as_bytes()[start - 1] == b'@' {
        for tag in TAGS {
            listing.push(item(tag, CompletionKind::Keyword));
        }
        return;
    }
    let docs = src.docs();
    let leaf = docs.tree().node_at(docs.source(), start);
    let comment = std::iter::once(leaf).chain(leaf.ancestors()).find(|node| node.is(N::DocComment));
    let named_here = |node: Node| node.token(T::Id).is_some_and(|name| name.start() == start);
    match leaf.kind() {
        N::DocClassNameRef => types(listing, leaf, at),
        N::DocParamNameRef => {
            for param in comment.map_or(Vec::new(), |comment| documented_params(src, comment)) {
                listing.push(item(param, CompletionKind::Variable));
            }
            listing.push(item("optional", CompletionKind::Keyword));
        }
        // `---@field name`: a visibility, or the name of a field the class already has.
        N::DocTagField if named_here(leaf) => {
            if leaf.child(N::DocAccessModifier).is_none() {
                listing.push(item("protected", CompletionKind::Keyword));
                listing.push(item("public", CompletionKind::Keyword));
            }
            let class = comment.and_then(|comment| Comment(comment).tag(N::DocTagClass)).and_then(tags::name);
            if let Some(class) = class {
                members_of(listing, class, true, &text[start..at]);
            }
        }
        // `---@see Class#member`
        N::DocTagSee if named_here(leaf) => {
            if let Some(class) = leaf.child(N::DocClassNameRef) {
                members_of(listing, class.code(), false, &text[start..at]);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed, labels};
    use openidea_plugin::providers::CompletionKind;

    const CLASSES: (&str, &str) = (
        "shop/shared/classes.lua",
        "---@class Shop\n---@field name string\n---@field private stock number\nShop = {}\nfunction Shop:open() end\n\n\
         ---@class ui.Panel : Shop\n\n---@alias ShopId string|number\n",
    );

    fn at(text: &str) -> Vec<String> {
        labels(&[MANIFEST, CLASSES, (MAIN, text)])
    }

    #[test]
    fn the_names_of_the_tags_after_the_sign() {
        let all = at("---@<caret>\nlocal a");
        assert_eq!(all.len(), 28);
        for tag in ["param", "return", "class", "field", "type", "alias", "enum", "cast", "deprecated", "nodiscard"] {
            assert!(all.contains(&tag.to_owned()), "{tag}");
        }
        assert_eq!(at("---@par<caret>\nlocal a"), all, "the editor filters what is typed");
        assert_eq!(at("--- @param<caret>\nlocal a"), all);
        assert_eq!(at("---Text.\n---@re<caret>\nlocal function f() end"), all, "on any line of the comment");
        assert!(at("-- @par<caret>\nlocal a").is_empty(), "a plain comment documents nothing");
        assert!(at("--[[ @par<caret> ]]\nlocal a").is_empty());
        assert!(at("---plain te<caret>\nlocal a").is_empty(), "the text of a comment");
    }

    #[test]
    fn the_parameters_of_the_documented_function() {
        assert_eq!(
            at("---@param <caret>\nlocal function add(first, second, ...) end"),
            ["first", "optional", "second"]
        );
        assert_eq!(at("---@param fi<caret>\nfunction Shop:add(first, second) end"), ["first", "optional", "second"]);
        assert_eq!(at("---@param <caret>\nlocal add = function(first) end"), ["first", "optional"]);
        assert_eq!(at("---@param <caret>\nShop.add = function(first) end"), ["first", "optional"]);
        assert_eq!(at("---@param <caret>\nlocal a = 1"), ["optional"]);
    }

    #[test]
    fn the_classes_and_the_aliases_where_a_type_is_written() {
        let types = [
            "any", "boolean", "function", "integer", "nil", "number", "Shop", "ShopId", "string", "table", "thread",
            "ui.Panel", "userdata", "void",
        ];
        for text in [
            "---@type <caret>\nlocal a",
            "---@type Sh<caret>\nlocal a",
            "---@param item Sh<caret>\nlocal function f(item) end",
            "---@return Sh<caret>\nlocal function f() end",
            "---@field shop Sh<caret>\nlocal a",
            "---@type table<string, Sh<caret>>\nlocal a",
            "---@type fun(item: Sh<caret>)\nlocal a",
            "---@type string|Sh<caret>\nlocal a",
            "---@see Sh<caret>\nlocal a",
        ] {
            assert_eq!(at(text), types, "{text}");
        }
        let items = completed(&[MANIFEST, CLASSES, (MAIN, "---@type ui.Pa<caret>\nlocal a")]);
        let panel = items.iter().find(|item| item.label == "ui.Panel").expect("the class");
        let range = panel.replace.expect("a range");
        assert_eq!((range.start.character, range.end.character), (9, 14), "from the start of the name");
        assert_eq!(panel.kind, Some(CompletionKind::Class));
        let alias = items.iter().find(|item| item.label == "ShopId").expect("the alias");
        assert_eq!((alias.kind, alias.replace), (Some(CompletionKind::Interface), None));
        let parents = at("---@class Late : Sh<caret>\nlocal a");
        assert!(parents.contains(&"Shop".to_owned()) && parents.contains(&"Late".to_owned()), "{parents:?}");
        assert!(at("---@class Sh<caret>\nlocal a").is_empty(), "the name of a class being declared");
        assert!(at("---@alias Sh<caret>\nlocal a").is_empty());
    }

    #[test]
    fn the_fields_of_a_class_and_the_members_a_see_names() {
        assert_eq!(at("---@class Late : Shop\n---@field <caret>\nlocal a"), ["name", "protected", "public", "stock"]);
        assert_eq!(at("---@field na<caret>\nlocal a"), ["protected", "public"], "no class in the comment");
        assert_eq!(at("---@class Late : Shop\n---@field public na<caret>\nlocal a"), ["name", "stock"]);
        assert_eq!(at("---@see Shop#<caret>\nlocal a"), ["name", "open", "stock"]);
        assert_eq!(at("---@see Shop#op<caret>\nlocal a"), ["name", "open", "stock"]);
        assert!(at("---@see Unknown#<caret>\nlocal a").is_empty());
    }
}
