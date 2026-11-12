//! Completion of a name being typed: the locals in sight, the globals of the project, the keywords
//! (`LocalAndGlobalCompletionProvider`), and the methods of the object of the method the caret is written in
//! (`SuggestSelfMemberProvider`).

use super::members::{Mode, context_class};
use super::{CARET_NAME, Listing, expected, tables};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::lua::scope::{DeclKind, Scopes};
use crate::lua::token::{T, is_id_part, is_identifier};
use crate::lua::tree::{N, Node};
use crate::matching::{self, Match, Query};
use crate::ty::context::Psi;
use crate::ty::members::{self, Listed};
use crate::ty::{FileId, G, SELF, Ty, display, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::HashSet;

/// How many globals of the project one list holds: the best matches of what is typed.
const MAX_GLOBALS: usize = 2000;
/// How many words of the document one list holds.
const MAX_WORDS: usize = 2000;

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
const SUGGESTED: char = '7';
const KEYWORD: char = '8';
const WORD: char = '9';

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
            let detail = listing.signature(signature, false, true);
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
    let mode = Mode { colon: false, object: "", read: false, value: CompletionKind::Variable, replaced: None };
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
    let mode = Mode { colon: true, object: "self:", read: true, value: CompletionKind::Field, replaced: None };
    for listed in members::visible(listing.cx, &class) {
        let sort = format!("{OWN_METHOD}{}", listed.member.name().to_lowercase());
        listing.member(&listed, &class, &class, &mode, &sort);
    }
}

/// The names the classes of the project suggest for a local being declared: `data` and `playerData` for
/// `PlayerData` (`SuggestLocalNameProvider`).
fn suggested(listing: &mut Listing) {
    let mut names: Vec<String> = Vec::new();
    for class in listing.cx.index.class_names() {
        let words = matching::words(class.rsplit('.').next().unwrap_or(class));
        for from in (0..words.len()).rev() {
            let first = words[from];
            let shouted = first.len() > 1 && !first.bytes().any(|b| b.is_ascii_lowercase());
            let lowered = if shouted { first.len() } else { 1 };
            let name =
                format!("{}{}{}", first[..lowered].to_ascii_lowercase(), &first[lowered..], words[from + 1..].concat());
            if is_identifier(&name) && !KEYWORDS.contains(&name.as_str()) && !VALUES.contains(&name.as_str()) {
                names.push(name);
            }
        }
    }
    for name in names {
        let sort = format!("{SUGGESTED}{}", name.to_lowercase());
        listing.push(item(&name, CompletionKind::Variable, None, sort));
    }
}

/// The labels of the blocks a `goto` is written in, up to its function.
fn labels(listing: &mut Listing, goto: Node) {
    for block in goto.ancestors().take_while(|node| !node.is(N::FuncBody)) {
        for label in block.children_of(N::LabelStat).filter_map(|label| label.token(T::Id)) {
            listing.push(item(label.text(), CompletionKind::Reference, None, format!("{LOCAL}{}", label.text())));
        }
    }
}

/// The identifiers of the document, and its strings that read as one, that nothing else offers: those of
/// more than two letters (`suggestWordsInFile`). `typed` is the word at the caret.
pub(super) fn words(listing: &mut Listing, src: &Source, typed: &str) {
    let mut found: Vec<CompletionItem> = Vec::new();
    {
        let mut known: HashSet<&str> = listing.items.iter().map(|item| item.label.as_str()).collect();
        known.extend([typed, CARET_NAME]);
        for token in &src.toks {
            let text = token.text(&src.text);
            let word = match token.kind {
                Tok::Name => text,
                Tok::Str { closed: true } => &text[1..text.len() - 1],
                _ => continue,
            };
            if word.len() > 2 && is_identifier(word) && known.insert(word) {
                found.push(item(word, CompletionKind::Text, None, format!("{WORD}{}", word.to_lowercase())));
                if found.len() == MAX_WORDS {
                    break;
                }
            }
        }
    }
    listing.items.extend(found);
}

/// What a name that starts at `start` may be, `typed` being what is written of it. Tells whether the words
/// of the document may be offered too: not where only a few names can stand.
pub(super) fn names(listing: &mut Listing, file: FileId, src: &Source, start: usize, typed: &str) -> bool {
    let leaf = src.root().tree().node_at(&src.text, start);
    let here = Psi { file, src, node: leaf };
    match leaf.kind() {
        N::NameExpr => {
            expected::values(listing, here);
            if let (Some(table), Some(field)) = (tables::keyed_by(leaf), leaf.parent()) {
                tables::keys(listing, here.at(table), field);
            }
            locals(listing, here);
            own_methods(listing, here);
            globals(listing, here, typed);
            keywords(listing, leaf);
        }
        // `{ na = value }`: the name of a field.
        N::TableField if leaf.token(T::Id).is_some_and(|name| name.start() == start) => {
            if let Some(table) = tables::keyed_by(leaf) {
                tables::keys(listing, here.at(table), leaf);
            }
        }
        // `function na`: the function is given to a variable, which may be one that exists.
        N::FuncDef if leaf.token(T::Id).is_some_and(|name| name.start() == start) => {
            locals(listing, here);
            globals(listing, here, typed);
        }
        // `local na`: a local is being named.
        N::NameDef => suggested(listing),
        N::GotoStat => {
            labels(listing, leaf);
            return false;
        }
        // `local a <co`
        N::Attribute => {
            for attribute in ["const", "close"] {
                listing.push(item(attribute, CompletionKind::Keyword, None, format!("{KEYWORD}{attribute}")));
            }
            return false;
        }
        N::LabelStat => return false,
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed, labels, shown, with_words};

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
            ["GetShop (id, ...)  [_G]", "GetShopName (id): string  [_G]", "GetShopCount ()  [_G]"],
            "the shorter names first; what a function returns is shown where it is written: no file is read"
        );
        let by_words = named(&[MANIFEST, functions, (MAIN, "print(gsc<caret>)")]);
        assert_eq!(by_words, ["GetShopCount ()  [_G]"], "by the starts of its words");
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

    /// The words of the file offered at the caret.
    fn words_at(text: &str) -> Vec<String> {
        let items = with_words(&[MANIFEST, (MAIN, text)]);
        items.into_iter().filter(|item| item.kind == Some(CompletionKind::Text)).map(|item| item.label).collect()
    }

    fn every_label(text: &str) -> Vec<String> {
        with_words(&[MANIFEST, (MAIN, text)]).into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn the_words_of_the_file_nothing_else_offers() {
        let text = concat!(
            "local shop = { title = 'welcome', ['max-items'] = 3, id = 1 }\n",
            "print(shop.title, unknownName)\n",
            "-- inComment\n",
            "local other = sho<caret>"
        );
        assert_eq!(
            words_at(text),
            ["other", "print", "title", "unknownName", "welcome"],
            "no local in sight, no keyword, nothing of two letters, no comment, not what is being typed"
        );
        assert_eq!(words_at("local shop = {}\nshop.title = 1\nUnknown.<caret>"), ["shop", "title", "Unknown"]);
        assert_eq!(words_at("local shop = {}\nshop.title = 1\nshop.ti<caret>"), ["shop"], "`title` is a member");
        assert!(words_at("local title = 'in a string <caret>'").is_empty());
        assert!(words_at("local title = 1\n---@type <caret>\nlocal a").is_empty());
    }

    #[test]
    fn the_labels_a_goto_may_name_and_the_attributes_of_a_local() {
        let text = concat!(
            "::top::\n",
            "local function f()\n",
            "    ::start::\n",
            "    do\n",
            "        ::inner::\n",
            "        goto <caret>\n",
            "    end\n",
            "    ::after::\n",
            "end\n",
            "::bottom::\n"
        );
        assert_eq!(every_label(text), ["after", "inner", "start"], "the labels of its function, and no word");
        assert_eq!(every_label(&text.replace("goto <caret>", "goto st<caret>")), ["after", "inner", "start"]);
        assert_eq!(every_label("local name <co<caret>> = 1"), ["close", "const"]);
    }

    #[test]
    fn the_names_the_classes_suggest_for_a_local() {
        let classes = (
            "shop/shared/classes.lua",
            "---@class PlayerData\n---@class ui.Panel\n---@class shop_item\n---@class End\n",
        );
        let listed = labels(&[MANIFEST, classes, (MAIN, "local da<caret>")]);
        assert_eq!(listed, ["data", "item", "panel", "playerData", "shopitem"], "`end` is a keyword");
        assert_eq!(labels(&[MANIFEST, classes, (MAIN, "local first, da<caret>")]), listed);
        assert!(labels(&[MANIFEST, classes, (MAIN, "local function f(da<caret>) end")]).is_empty(), "a parameter");
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
