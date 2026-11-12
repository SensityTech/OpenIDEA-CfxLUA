//! Completion of the members of what stands before a `.` or a `:` (`ClassMemberCompletionProvider`).

use crate::feature::Doc;
use crate::lua::Source;
use crate::lua::doc::tags::Visibility;
use crate::lua::lexer::Tok;
use crate::lua::token::is_id_part;
use crate::lua::tree::N;
use crate::ty::context::{Cx, Psi};
use crate::ty::members::{self, Listed, Member};
use crate::ty::{Detail, FileId, Function, Ret, SELF, Signature, Ty, display, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// How many functions without a `@return` have their body read to show what they return.
const BODIES_READ: usize = 60;

/// The `.` or the `:` the caret names a member after, as an index in the code tokens: the caret stands
/// right after it, or in the name that follows it.
pub fn sign_before(src: &Source, at: usize) -> Option<usize> {
    let sign = |i: usize| src.is_op(i, ".") || src.is_op(i, ":");
    let before = src.tok_before(at)?;
    if sign(before) {
        return Some(before);
    }
    let named = src.toks[before].kind == Tok::Name && at <= src.toks[before].end;
    (named && before > 0 && sign(before - 1)).then(|| before - 1)
}

fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| is_id_part(first) && !first.is_ascii_digit()) && bytes.all(is_id_part)
}

/// The name of a class a member is shown with; a table or the type made for a variable has none worth it.
fn class_name(class: &Ty) -> Option<&str> {
    match class {
        Ty::Class(named) if named.flags != 0 || matches!(named.detail, Detail::Table(_)) => None,
        other => other.class_name(),
    }
}

/// The class of the method the caret is written in: what its private members are visible from.
fn context_class(cx: &Cx, from: Psi) -> Ty {
    let method = from.node.ancestors().find(|node| node.is(N::ClassMethodDef));
    let owner = method.and_then(psi::method_owner);
    let ty = owner.map_or(Ty::Unknown, |owner| cx.with_value(0, || cx.infer(from.at(owner))));
    ty.perfect_class().cloned().unwrap_or(Ty::Unknown)
}

/// Whether a member of `class` may be named from a method of `context`: a private one from its own class
/// only, a protected one from the classes that inherit from it too.
fn is_visible(cx: &Cx, class: &Ty, context: &Ty, visibility: Visibility) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Private => context.each().iter().any(|from| from == class),
        Visibility::Protected => {
            context.each().iter().any(|from| from.is_class() && members::is_subtype(cx, from, class, true))
        }
    }
}

struct Listing<'a, 'c> {
    cx: &'a Cx<'c>,
    doc: &'a Doc,
    colon: bool,
    /// From the sign to the caret: what an entry that is no name replaces, the sign included.
    replaced: (usize, usize),
    bodies: usize,
    /// The label and the type of each entry listed so far.
    listed: Vec<(String, String)>,
    items: Vec<CompletionItem>,
}

impl Listing<'_, '_> {
    /// Adds an entry unless the same one came from another class of the object: a global named as a class
    /// holds the members of that class too.
    fn push(&mut self, mut item: CompletionItem, class: &Ty, own: bool) {
        let entry = (item.label.clone(), item.detail.clone().unwrap_or_default());
        if self.listed.contains(&entry) {
            return;
        }
        self.listed.push(entry);
        if let (Some(detail), Some(class)) = (&mut item.detail, class_name(class)) {
            detail.push_str(&format!("  [{class}]"));
        }
        let name = item.filter_text.as_deref().unwrap_or(&item.label).to_lowercase();
        item.sort_text = Some(format!("{}{name}", if own { 0 } else { 1 }));
        self.items.push(item);
    }

    /// What a signature returns, as far as it costs little to tell.
    fn returned(&mut self, signature: &Signature) -> Option<Ty> {
        match &signature.ret {
            Ret::Ty(ty) => Some(ty.clone()),
            Ret::Body(_) if self.bodies < BODIES_READ => {
                self.bodies += 1;
                Some(self.cx.ret(&signature.ret))
            }
            Ret::Body(_) => None,
        }
    }

    fn function(&mut self, name: &str, function: &Function, declaring: &Ty, asked: &Ty) {
        for signature in function.signatures() {
            if self.colon {
                let first = signature.declared(Some(declaring)).into_iter().next();
                let fits = first.is_some_and(|first| members::is_subtype(self.cx, asked, &first.ty, true));
                if !fits {
                    continue;
                }
            }
            let mut params: Vec<String> =
                signature.args(None, self.colon).iter().map(|param| param.name.to_string()).collect();
            if signature.vararg.is_some() {
                params.push("...".to_owned());
            }
            let returned = self.returned(signature).map_or(String::new(), |ty| format!(": {}", display(&ty)));
            let kind = if self.colon { CompletionKind::Method } else { CompletionKind::Function };
            let mut item = CompletionItem::new(name, kind);
            item.detail = Some(format!("({}){returned}", params.join(", ")));
            self.push(item, declaring, declaring == asked);
        }
    }

    fn field(&mut self, name: &str, ty: &Ty, declaring: &Ty, asked: &Ty) {
        let mut item = CompletionItem::new(name, CompletionKind::Field);
        item.detail = Some(display(ty));
        if !is_identifier(name) {
            item.label = format!("['{name}']");
            item.insert_text = item.label.clone();
            item.filter_text = Some(name.to_owned());
            item.replace = Some(self.doc.range(self.replaced.0, self.replaced.1));
        }
        self.push(item, declaring, declaring == asked);
    }

    fn class(&mut self, asked: &Ty, context: &Ty) {
        for Listed { member, class } in members::visible(self.cx, asked) {
            let visibility = match &member {
                Member::Declared(found) => found.member.visibility,
                Member::Written(_) => Visibility::Public,
            };
            if !is_visible(self.cx, &class, context, visibility) {
                continue;
            }
            let ty = members::type_of(self.cx, &member);
            if !matches!(ty, Ty::Function(_)) {
                if !self.colon {
                    self.field(member.name(), &ty, &class, asked);
                }
                continue;
            }
            // `self` in a signature is the class the member is asked on.
            let on_class = ty.substitute(&mut |named| (&*named.name == SELF).then(|| asked.clone()));
            if let Ty::Function(function) = &on_class {
                self.function(member.name(), function, &class, asked);
            }
        }
    }
}

/// The members of the expression before the `.` or the `:` that precedes the caret.
pub fn members(cx: &Cx, file: FileId, doc: &Doc, at: usize) -> Vec<CompletionItem> {
    let src = &doc.src;
    let Some(sign) = sign_before(src, at) else { return Vec::new() };
    let sign = src.toks[sign];
    let index = src.root().tree().node_at(&src.text, sign.start);
    let Some(prefix) = psi::prefix(index).filter(|_| index.is(N::IndexExpr)) else { return Vec::new() };
    let from = Psi { file, src, node: index };
    let ty = cx.infer(from.at(prefix));
    let context = context_class(cx, from);
    let colon = sign.text(&src.text) == ":";
    let mut listing =
        Listing { cx, doc, colon, replaced: (sign.start, at), bodies: 0, listed: Vec::new(), items: Vec::new() };
    for class in ty.top_classes() {
        listing.class(class, &context);
    }
    listing.items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    listing.items
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorFeature;
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use openidea_plugin::providers::{CompletionItem, CompletionKind};

    const MANIFEST: (&str, &str) = (
        "shop/fxmanifest.lua",
        "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\nclient_script 'client/*.lua'\n",
    );
    const CONFIG: (&str, &str) = (
        "shop/shared/config.lua",
        "Config = { debug = false, ['max-items'] = 10 }\nConfig.rate = 2\n---@param name string\n---@return number\n\
         function Config.price(name, ...) end\nfunction Config:reload() return self end\n",
    );
    const MAIN: &str = "shop/client/main.lua";

    fn completed(files: &[(&str, &str)]) -> Vec<CompletionItem> {
        let fx = Fixture::new(files);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(files.last().expect("a file").0);
        feature.completion(&fx.cx(&doc), at, None)
    }

    /// Each entry as `label detail`.
    fn shown(files: &[(&str, &str)]) -> Vec<String> {
        let entry = |item: &CompletionItem| format!("{} {}", item.label, item.detail.as_deref().unwrap_or_default());
        completed(files).iter().map(entry).collect()
    }

    fn labels(files: &[(&str, &str)]) -> Vec<String> {
        completed(files).into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn the_members_of_a_global_declared_in_another_file() {
        let listed = shown(&[MANIFEST, CONFIG, (MAIN, "local rate = Config.<caret>")]);
        assert_eq!(
            listed,
            [
                "debug boolean",
                "['max-items'] number",
                "price (name, ...): number",
                "rate number",
                "reload (self): [global Config]|table",
            ]
        );
        let typed = shown(&[MANIFEST, CONFIG, (MAIN, "local rate = Config.ra<caret>")]);
        assert_eq!(typed, listed, "the editor filters what is typed");
    }

    #[test]
    fn a_colon_lists_what_can_take_the_object() {
        let listed = shown(&[MANIFEST, CONFIG, (MAIN, "Config:<caret>")]);
        assert_eq!(listed, ["reload (): [global Config]|table"], "no field, no function that takes a name first");
        let items = completed(&[MANIFEST, CONFIG, (MAIN, "Config:<caret>")]);
        assert_eq!(items[0].kind, Some(CompletionKind::Method));
        assert_eq!(items[0].insert_text, "reload");
    }

    #[test]
    fn a_name_that_is_no_identifier_replaces_the_dot() {
        let items = completed(&[MANIFEST, CONFIG, (MAIN, "local n = Config.ma<caret>")]);
        let item = items.iter().find(|item| item.label == "['max-items']").expect("the field");
        assert_eq!(item.insert_text, "['max-items']");
        assert_eq!(item.filter_text.as_deref(), Some("max-items"));
        let range = item.replace.expect("a range");
        assert_eq!((range.start.character, range.end.character), (16, 19), "from the dot to the caret");
    }

    #[test]
    fn the_members_of_an_annotated_class_and_of_its_parents() {
        let class = (
            "shop/shared/shop.lua",
            "---@class Base\n---@field id number\n---@field protected owner string\n---@field private secret string\n\
             local Base = {}\nfunction Base:open() end\n\n---@class Shop : Base\n---@field name string\nShop = {}\n\
             function Shop:buy(item) end\nfunction Shop:peek()\n    return self.<caret>\nend\n",
        );
        let inside = labels(&[MANIFEST, class]);
        assert_eq!(inside, ["buy", "name", "peek", "id", "open", "owner"], "a private member stays in its class");
        let outside = shown(&[
            MANIFEST,
            (class.0, &class.1.replace("<caret>", "name")),
            (MAIN, "---@type Shop\nlocal shop\nshop.<caret>"),
        ]);
        assert_eq!(
            outside,
            [
                "buy (self, item): void  [Shop]",
                "name string  [Shop]",
                "peek (self): string  [Shop]",
                "id number  [Base]",
                "open (self): void  [Base]",
            ],
            "a protected member is not seen from outside"
        );
    }

    #[test]
    fn what_the_document_declares_before_it_is_saved() {
        let fx = Fixture::new(&[MANIFEST, CONFIG, (MAIN, "local M = {}\nM.first = 1\n")]);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let typed = "local M = {}\nM.first = 1\nfunction M.second() end\nM.";
        let doc = crate::feature::Doc::new(&fx.ws, &Fixture::path(MAIN), "lua", 2, typed);
        feature.index_file(&fx.ws, &doc.path, doc.kind, &doc.src);
        let items = feature.completion(&fx.cx(&doc), typed.len(), None);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["first", "second"]);
    }

    #[test]
    fn a_required_file_gives_what_it_returns() {
        let module =
            ("shop/shared/money.lua", "local Money = {}\nfunction Money.format(amount) return '' end\nreturn Money\n");
        let listed = labels(&[MANIFEST, module, (MAIN, "local money = require('shared.money')\nmoney.<caret>")]);
        assert_eq!(listed, ["format"]);
    }

    #[test]
    fn nothing_is_listed_where_no_member_is_named() {
        for text in [
            "local a = 1 .. <caret>",
            "local a = 'Config.<caret>'",
            "local a = Config<caret>",
            "function Config.<caret>",
            "local n = 12.<caret>",
            "Unknown.<caret>",
            "-- Config.<caret>",
        ] {
            assert!(labels(&[MANIFEST, CONFIG, (MAIN, text)]).is_empty(), "{text}");
        }
    }
}
