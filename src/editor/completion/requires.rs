//! Completion of the file a `require` loads (`RequirePathCompletionProvider`).

use super::Listing;
use crate::lua::Source;
use crate::lua::tree::{N, Node};
use crate::project::FileKind;
use crate::ty::psi;
use crate::workspace::Workspace;
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// How many files one list holds.
const MAX_FILES: usize = 2000;

/// Whether `literal` is the string a function that loads a file is called with: `require 'a.b'`.
fn loads(listing: &Listing, literal: Node) -> bool {
    let args = literal.parent().filter(|args| matches!(args.kind(), N::ListArgs | N::SingleArg));
    let callee = args.and_then(Node::parent).filter(|call| call.is(N::CallExpr)).and_then(psi::prefix);
    callee.is_some_and(|callee| {
        callee.is(N::NameExpr) && listing.cx.options.require_like.iter().any(|name| name == callee.code())
    })
}

/// The name a file is required by, from the folder its path is read from: `a/b.lua` is `a.b`, and
/// `a/b/init.lua` is `a.b` too.
pub(crate) fn module(rel: &str) -> Option<String> {
    let path = rel.strip_suffix(".lua")?;
    let path = path.strip_suffix("/init").unwrap_or(path);
    (!path.is_empty() && path != "init").then(|| path.replace('/', "."))
}

/// Inside the quoted string `token`: when a `require` is called with it, the Lua files of the resource of the
/// document, or of the project for a file that belongs to none, as the names they are required by. Tells
/// whether the string names a file: nothing else is offered there.
pub(super) fn paths(listing: &mut Listing, ws: &Workspace, src: &Source, token: usize, at: usize) -> bool {
    let quote = src.toks[token];
    let literal = src.root().tree().node_at(&src.text, quote.start);
    if !literal.is(N::LiteralExpr) || !loads(listing, literal) {
        return false;
    }
    let document = listing.doc.path.as_str();
    let root = ws.resource_of(document).map_or(ws.root.as_str(), |resource| resource.dir.as_str());
    let replace = listing.doc.range(quote.start + 1, at);
    let scripts = ws.files_under(root).filter(|file| file.kind == FileKind::Lua && file.path != document);
    for file in scripts.take(MAX_FILES) {
        let rel = &file.path[root.len() + 1..];
        let Some(name) = module(rel) else { continue };
        let mut item = CompletionItem::new(&name, CompletionKind::File);
        item.detail = Some(rel.to_owned());
        item.replace = Some(replace);
        item.sort_text = Some(format!("0{name}"));
        listing.push(item);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::module;
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed};

    const FILES: [(&str, &str); 5] = [
        MANIFEST,
        ("shop/shared/config.lua", "Config = {}"),
        ("shop/modules/cart/init.lua", "return {}"),
        ("shop/modules/cart/items.lua", "return {}"),
        ("other/server/main.lua", "print(1)"),
    ];

    fn required(text: &str) -> Vec<String> {
        let mut files = FILES.to_vec();
        files.push((MAIN, text));
        let entry = |item: &openidea_plugin::providers::CompletionItem| {
            format!("{} ({})", item.label, item.detail.as_deref().unwrap_or_default())
        };
        completed(&files).iter().map(entry).collect()
    }

    #[test]
    fn the_files_of_the_resource_as_they_are_required() {
        let listed = required("local cart = require('<caret>')");
        assert_eq!(
            listed,
            [
                "modules.cart (modules/cart/init.lua)",
                "modules.cart.items (modules/cart/items.lua)",
                "shared.config (shared/config.lua)",
            ],
            "not the document itself, not the manifest, not another resource"
        );
        assert_eq!(required("local cart = require 'modules.ca<caret>'"), listed, "the editor filters");
        assert_eq!(required("local cart = require('modules/ca<caret>')"), listed, "a path written with slashes");
        let mut files = FILES.to_vec();
        files.push((MAIN, "require('modules.ca<caret>')"));
        let items = completed(&files);
        let range = items[0].replace.expect("a range");
        assert_eq!((range.start.character, range.end.character), (9, 19), "what is typed of the string");
        assert!(required("local cart = load('<caret>')").is_empty(), "not a function that loads a file");
        assert!(required("local cart = lib.require('<caret>')").is_empty());
    }

    #[test]
    fn the_name_a_file_is_required_by() {
        assert_eq!(module("a/b.lua").as_deref(), Some("a.b"));
        assert_eq!(module("a/b/init.lua").as_deref(), Some("a.b"));
        assert_eq!(module("main.lua").as_deref(), Some("main"));
        assert_eq!(module("init.lua"), None);
        assert_eq!(module("a/b.txt"), None);
    }
}
