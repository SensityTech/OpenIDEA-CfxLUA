//! Completion of a name being typed: the locals in sight, the globals of the project, the keywords
//! (`LocalAndGlobalCompletionProvider`), and the methods of the object of the method the caret is written in
//! (`SuggestSelfMemberProvider`).

use super::members::{Mode, context_class};
use super::{Listing, expected};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::lua::scope::{DeclKind, Scopes};
use crate::lua::token::{T, is_id_part};
use crate::lua::tree::{N, Node};
use crate::matching::{Match, Query};
use crate::ty::context::Psi;
use crate::ty::members::{self, Listed};
use crate::ty::{FileId, G, SELF, Ty, display, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// How many globals of the project one list holds: the best matches of what is typed.
const MAX_GLOBALS: usize = 2000;

/// `if`, `for` and `elseif` are live templates in the JetBrains plugin: they are plain keywords here.
const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in", "local", "not", "or",
    "repeat", "return", "then", "until", "while",
];
const VALUES: &[&str] = &["false", "nil", "true", SELF];

/// The first character of a sort key: what is nearest to the caret comes first among equal matches.
const LOCAL: char = '2';
const OWN_METHOD: char = '3';
const GLOBAL: char = '4';
const KEYWORD: char = '8';

/// Where the name the caret stands in starts, or the caret itself where a name could start: not in a number,
/// nor right after a literal or a `..`, where the JetBrains plugin completes nothing.
pub fn start(src: &Source, at: usize) -> Option<usize> {
    let text = src.text.as_str();
    if at > text.len() || !text.is_char_boundary(at) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut start = at;
    while start > 0 && is_id_part(bytes[start - 1]) {
        start -= 1;
    }
    if start < at {
        return (!bytes[start].is_ascii_digit()).then_some(start);
    }
    let glued = src.tok_before(at).map(|i| src.toks[i]).filter(|token| token.end == at);
    let literal = |kind: Tok| matches!(kind, Tok::Number | Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. });
    let blocked = glued.is_some_and(|token| literal(token.kind) || token.text(text) == "..");
    (!blocked).then_some(at)
}

fn item(label: &str, kind: CompletionKind, detail: Option<String>, sort: String) -> CompletionItem {
    let mut item = CompletionItem::new(label, kind);
    item.detail = detail;
    item.sort_text = Some(sort);
    item
}

/// The locals, the parameters and the loop variables a name written at `here` could read: a function once
/// per signature, anything else with its type.
fn locals(listing: &mut Listing, here: Psi) {
    let root = here.src.root();
    for decl in Scopes::in_sight(root, here.node.id()) {
        if decl.kind == DeclKind::Global {
            continue;
        }
        let name = root.tree().tokens().text(root.source(), decl.name as usize);
        let ty = listing.cx.infer(here.at(root.tree().node(root.source(), decl.node)));
        let sort = format!("{LOCAL}{}", name.to_lowercase());
        let mut function = false;
        for signature in ty.each().iter().filter_map(Ty::as_function).flat_map(|function| function.signatures()) {
            function = true;
            let detail = listing.signature(signature, false);
            listing.push(item(name, CompletionKind::Function, Some(detail), sort.clone()));
        }
        if !function {
            listing.push(item(name, CompletionKind::Variable, Some(display(&ty)), sort));
        }
    }
}

/// The globals the project declares whose name answers what is typed: the members of `_G` and, in a file
/// that names its module, those of the module.
fn globals(listing: &mut Listing, here: Psi, typed: &str) {
    let cx = listing.cx;
    let query = Query::new(typed);
    let context = context_class(cx, here);
    let mode = Mode { colon: false, object: "", read: false, replaced: None };
    let module = members::module_of(here.src).map(|module| Ty::class(&module));
    for class in module.into_iter().chain([Ty::class(G)]) {
        let wanted = |name: &str| query.matches(name).is_some();
        let mut found: Vec<(Match, Listed)> = members::visible_named(cx, &class, &wanted)
            .into_iter()
            .filter_map(|listed| Some((query.matches(listed.member.name())?, listed)))
            .collect();
        found.sort_by(|(a_match, a), (b_match, b)| {
            let (a, b) = (a.member.name(), b.member.name());
            a_match.cmp(b_match).then(a.len().cmp(&b.len())).then_with(|| a.cmp(b))
        });
        found.truncate(MAX_GLOBALS);
        for (rank, (_, listed)) in found.iter().enumerate() {
            listing.member(listed, &class, &context, &mode, &format!("{GLOBAL}{rank:04}"));
        }
    }
}

/// Whether a name is the first thing of its statement: `function` written there starts a declaration, and
/// anywhere else a function that is a value.
fn starts_statement(name: Node) -> bool {
    name.parent().is_some_and(|stat| stat.is(N::ExprStat) && stat.code_range().0 == name.code_range().0)
}

fn keywords(listing: &mut Listing, name: Node) {
    for word in KEYWORDS {
        let mut keyword = item(word, CompletionKind::Keyword, None, format!("{KEYWORD}{word}"));
        if *word == "function" && !starts_statement(name) {
            keyword.insert_text = "function($1) $0 end".to_owned();
            keyword.snippet = true;
        }
        listing.push(keyword);
    }
    for word in VALUES {
        listing.push(item(word, CompletionKind::Keyword, None, format!("{KEYWORD}{word}")));
    }
}

/// The methods of the object of the method the caret is written in, as `self:name`.
fn own_methods(listing: &mut Listing, here: Psi) {
    let method = here.node.ancestors().find(|node| node.is(N::ClassMethodDef));
    let named = here.node.ancestors().any(|node| node.is(N::ClassMethodName));
    if named || !method.is_some_and(|method| !psi::is_static(method)) {
        return;
    }
    let class = context_class(listing.cx, here);
    let mode = Mode { colon: true, object: "self:", read: true, replaced: None };
    for listed in members::visible(listing.cx, &class) {
        let sort = format!("{OWN_METHOD}{}", listed.member.name().to_lowercase());
        listing.member(&listed, &class, &class, &mode, &sort);
    }
}

/// What a name that starts at `start` may be, `typed` being what is written of it.
pub(super) fn names(listing: &mut Listing, file: FileId, src: &Source, start: usize, typed: &str) {
    let leaf = src.root().tree().node_at(&src.text, start);
    let here = Psi { file, src, node: leaf };
    match leaf.kind() {
        N::NameExpr => {
            expected::values(listing, here);
            locals(listing, here);
            own_methods(listing, here);
            globals(listing, here, typed);
            keywords(listing, leaf);
        }
        // `function na`: the function is given to a variable, which may be one that exists.
        N::FuncDef if leaf.token(T::Id).is_some_and(|name| name.start() == start) => {
            locals(listing, here);
            globals(listing, here, typed);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed, labels, shown};

    const GLOBALS: (&str, &str) =
        ("shop/shared/globals.lua", "gVar1 = 1\n\ngVar2 = {\n    aaa = 2,\n    bbb = 3\n}\ngVar2.ccc = 4\n");

    /// The entries that are no keyword, as `label detail`.
    fn named(files: &[(&str, &str)]) -> Vec<String> {
        let entry = |item: &CompletionItem| format!("{} {}", item.label, item.detail.as_deref().unwrap_or_default());
        completed(files).iter().filter(|item| item.kind != Some(CompletionKind::Keyword)).map(entry).collect()
    }

    #[test]
    fn the_locals_in_sight() {
        // `TestCompletion.testLocalCompletion`
        let listed = named(&[MANIFEST, (MAIN, "local function func1()end\n\nlocal a = 2\nlocal b = 2\n<caret>\n")]);
        assert_eq!(listed, ["a number", "b number", "func1 (): void"]);
        // `TestCompletion.testParamCompletion`
        let listed = named(&[MANIFEST, (MAIN, "\nlocal function aaa(param1, param2)\n    local bbb = <caret>\nend\n")]);
        assert_eq!(listed, ["aaa (param1, param2): void", "param1 [local param1]", "param2 [local param2]"]);
    }

    #[test]
    fn a_name_being_typed_reads_what_is_declared_before_it() {
        let text = "local count = 1\nlocal function add(n) return count + n end\nfor index, value in pairs({}) do\n    \
                    local inner = 'x'\n    print(co<caret>)\nend\nlocal after = 2\n";
        let listed = named(&[MANIFEST, (MAIN, text)]);
        assert_eq!(
            listed,
            ["add (n): number", "count number", "index [local index]", "inner string", "value [local value]"],
            "what is declared after the caret, or in a block that is closed, is not in sight"
        );
        let shadowed = named(&[MANIFEST, (MAIN, "local a = 1\ndo\n    local a = 'x'\n    print(a<caret>)\nend\n")]);
        assert_eq!(shadowed, ["a string"], "the nearest declaration of a name");
    }

    #[test]
    fn the_globals_of_the_project_that_answer_what_is_typed() {
        // `TestCompletion.testGlobalCompletion`
        let all = named(&[MANIFEST, GLOBALS, (MAIN, "<caret>")]);
        assert_eq!(all, ["gVar1 number  [_G]", "gVar2 table  [_G]"]);
        let typed = named(&[MANIFEST, GLOBALS, (MAIN, "local x = gv<caret>")]);
        assert_eq!(typed, all, "without case");
        assert!(named(&[MANIFEST, GLOBALS, (MAIN, "local x = v<caret>")]).is_empty(), "by their start");
        let members = labels(&[MANIFEST, GLOBALS, (MAIN, "gVar2.<caret>")]);
        assert_eq!(members, ["aaa", "bbb", "ccc"]);

        let functions = (
            "shop/shared/api.lua",
            "---@param id number\n---@return string\nfunction GetShopName(id) end\nfunction GetShop(id, ...) return {} end\n\
             GetShopCount = function() return 3 end\nlocal function GetHidden() end\nShopName = GetShopName(1)\n",
        );
        let listed = named(&[MANIFEST, functions, (MAIN, "print(getsh<caret>)")]);
        assert_eq!(
            listed,
            ["GetShop (id, ...): table  [_G]", "GetShopName (id): string  [_G]", "GetShopCount (): number  [_G]"],
            "the shorter names first; what a function returns is read where it is not written"
        );
        let by_words = named(&[MANIFEST, functions, (MAIN, "print(gsc<caret>)")]);
        assert_eq!(by_words, ["GetShopCount (): number  [_G]"], "by the starts of its words");
        let variable = named(&[MANIFEST, functions, (MAIN, "print(ShopN<caret>)")]);
        assert_eq!(variable, ["ShopName "], "a type no statement tells is not read for a list");
    }

    #[test]
    fn the_keywords_and_a_function_written_as_a_value() {
        let items = completed(&[MANIFEST, (MAIN, "local f = fu<caret>")]);
        let keywords: Vec<&str> = items
            .iter()
            .filter(|item| item.kind == Some(CompletionKind::Keyword))
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            keywords,
            [
                "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local",
                "nil", "not", "or", "repeat", "return", "self", "then", "true", "until", "while"
            ]
        );
        let function = items.iter().find(|item| item.label == "function").expect("the keyword");
        assert_eq!((function.insert_text.as_str(), function.snippet), ("function($1) $0 end", true));
        let items = completed(&[MANIFEST, (MAIN, "fu<caret>")]);
        let function = items.iter().find(|item| item.label == "function").expect("the keyword");
        assert_eq!((function.insert_text.as_str(), function.snippet), ("function", false), "a declaration");
    }

    #[test]
    fn the_methods_of_the_object_of_a_method() {
        // `TestCompletion.testSelfCompletion`
        let text = "\nlocal s = {}\n\nfunction s:aaa()end\nfunction s:abb()end\nfunction s.static(a) end\ns.field = 1\n\
                    function s:ccc()\n    sa<caret>\nend\n";
        let listed = shown(&[MANIFEST, (MAIN, text)]);
        let own: Vec<&str> = listed.iter().map(String::as_str).filter(|entry| entry.starts_with("self:")).collect();
        assert_eq!(
            own,
            ["self:aaa (): void", "self:abb (): void", "self:ccc (): void"],
            "no field, no function without self"
        );
        let outside = labels(&[
            MANIFEST,
            (MAIN, "local s = {}\nfunction s:aaa() end\nfunction s.static()\n    sa<caret>\nend\n"),
        ]);
        assert!(
            !outside.iter().any(|label| label.starts_with("self:")),
            "a function declared with a dot has no object"
        );
    }

    #[test]
    fn a_local_shows_what_another_file_gives_it() {
        let config = (
            "shop/shared/config.lua",
            "---@class ShopItem\n---@field name string\n\nConfig = { debug = false }\n\n---@param name string\n\
             ---@return ShopItem?\nfunction Config.find(name)\nend\n",
        );
        let text = "local Shop = {}\nfunction Shop:open() end\nlocal found = Config.find('bread')\nfoun<caret>\n";
        let listed = named(&[MANIFEST, config, (MAIN, text)]);
        assert_eq!(listed, ["found ShopItem|[local found]", "Shop table|[local Shop]"]);
    }

    #[test]
    fn the_order_of_a_list() {
        let text = "function Shared() end\nlocal function shown() end\nlocal M = {}\nfunction M:send() end\n\
                    function M:run()\n    local shop = 1\n    s<caret>\nend\n";
        let listed = labels(&[MANIFEST, (MAIN, text)]);
        let at = |label: &str| listed.iter().position(|listed| listed == label).unwrap_or_else(|| panic!("{label}"));
        assert!(at("shop") < at("self:send") && at("self:send") < at("Shared") && at("Shared") < at("self"));
        assert!(at("shop") < at("shown"), "locals by name");
    }

    #[test]
    fn where_no_name_is_read_nothing_is_listed() {
        for text in [
            "local a = 12<caret>",
            "local a = 0x<caret>",
            "local a = 'text<caret>'",
            "local a = 'text'<caret>",
            "local a = `hash`<caret>",
            "local a = b ..<caret>",
            "-- comment <caret>",
            "local na<caret>",
            "local function na<caret>",
            "function f(pa<caret>) end",
            "for na<caret>",
            "goto na<caret>",
            "::na<caret>",
        ] {
            assert!(labels(&[MANIFEST, GLOBALS, (MAIN, text)]).is_empty(), "{text}");
        }
        let after_function = named(&[MANIFEST, GLOBALS, (MAIN, "local shop = {}\nfunction gv<caret>")]);
        assert_eq!(
            after_function,
            ["shop table|[local shop]", "gVar1 number  [_G]", "gVar2 table  [_G]"],
            "no keyword there"
        );
    }
}
