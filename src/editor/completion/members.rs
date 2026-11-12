//! Completion of the members of what stands before a `.` or a `:` (`ClassMemberCompletionProvider`).

use super::Listing;
use crate::lua::Source;
use crate::lua::doc::tags::Visibility;
use crate::lua::lexer::Tok;
use crate::lua::token::is_identifier;
use crate::lua::tree::N;
use crate::ty::context::{Cx, Psi};
use crate::ty::members::{self, Listed};
use crate::ty::{Detail, FileId, Function, SELF, Ty, display, psi};
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// The `.` or the `:` the caret names a member after, as an index in the code tokens: the caret stands
/// right after it, or in the name that follows it.
pub fn sign_before(src: &Source, at: usize) -> Option<usize> {
    let sign = |i: usize| src.is_op(i, ".") || src.is_op(i, ":");
    let before = src.tok_before(at)?;
    if sign(before) {
        return Some(before);
    }
    // A word that happens to be a keyword is still what is typed after the sign: `count.if`.
    let named = matches!(src.toks[before].kind, Tok::Name | Tok::Keyword) && at <= src.toks[before].end;
    (named && before > 0 && sign(before - 1)).then(|| before - 1)
}

/// The name of a class a member is shown with; a table or the type made for a variable has none worth it.
pub(super) fn class_name(class: &Ty) -> Option<&str> {
    match class {
        Ty::Class(named) if named.flags != 0 || matches!(named.detail, Detail::Table(_)) => None,
        other => other.class_name(),
    }
}

/// The class of the method the caret is written in: what its private members are visible from.
pub(super) fn context_class(cx: &Cx, from: Psi) -> Ty {
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

/// How the members of a class are listed.
pub(super) struct Mode<'m> {
    /// After a colon: what can take the object, and no field.
    pub colon: bool,
    /// What a label is written after: `self:` where the object of a method is not written.
    pub object: &'m str,
    /// Whether the type of a member its file did not keep, and what a function returns without saying it,
    /// are read in the file that declares them.
    pub read: bool,
    /// What a member that is no function is listed as: a field of its class, or a variable for a global.
    pub value: CompletionKind,
    /// From the sign to the caret: what an entry that is no name replaces, the sign included.
    pub replaced: Option<(usize, usize)>,
}

impl Listing<'_, '_> {
    /// Adds an entry unless the same one came from another class of the object: a global named as a class
    /// holds the members of that class too.
    fn entry(&mut self, mut item: CompletionItem, declaring: &Ty, sort: &str) {
        if !self.listed.insert((item.label.clone(), item.detail.clone().unwrap_or_default())) {
            return;
        }
        if let (Some(detail), Some(class)) = (&mut item.detail, class_name(declaring)) {
            detail.push_str(&format!("  [{class}]"));
        }
        item.sort_text = Some(sort.to_owned());
        self.items.push(item);
    }

    fn function(&mut self, name: &str, function: &Function, declaring: &Ty, asked: &Ty, mode: &Mode, sort: &str) {
        for signature in function.signatures() {
            if mode.colon {
                let first = signature.declared(Some(declaring)).into_iter().next();
                let fits = first.is_some_and(|first| members::is_subtype(self.cx, asked, &first.ty, true));
                if !fits {
                    continue;
                }
            }
            let kind = if mode.colon { CompletionKind::Method } else { CompletionKind::Function };
            let mut item = CompletionItem::new(&format!("{}{name}", mode.object), kind);
            item.detail = Some(self.signature(signature, mode.colon, mode.read));
            self.entry(item, declaring, sort);
        }
    }

    fn field(&mut self, name: &str, ty: Option<&Ty>, declaring: &Ty, mode: &Mode, sort: &str) {
        let mut item = CompletionItem::new(name, mode.value);
        item.detail = ty.map(display);
        if let Some(replaced) = mode.replaced.filter(|_| !is_identifier(name)) {
            item.label = format!("['{name}']");
            item.insert_text = item.label.clone();
            item.filter_text = Some(name.to_owned());
            item.replace = Some(self.doc.range(replaced.0, replaced.1));
        }
        self.entry(item, declaring, sort);
    }

    /// The entries of one member of the class `asked`, seen from a method of `context`: a field, or a
    /// function once per signature. `sort` is their sort key.
    pub(super) fn member(&mut self, listed: &Listed, asked: &Ty, context: &Ty, mode: &Mode, sort: &str) {
        let Listed { member, class: declaring } = listed;
        if !is_visible(self.cx, declaring, context, member.visibility()) {
            return;
        }
        let ty = if mode.read { Some(members::type_of(self.cx, member)) } else { member.kept_type() };
        let Some(ty @ Ty::Function(_)) = &ty else {
            if !mode.colon {
                self.field(member.name(), ty.as_ref(), declaring, mode, sort);
            }
            return;
        };
        // `self` in a signature is the class the member is asked on.
        let on_class = ty.substitute(&mut |named| (&*named.name == SELF).then(|| asked.clone()));
        if let Ty::Function(function) = &on_class {
            self.function(member.name(), function, declaring, asked, mode, sort);
        }
    }
}

/// The members of the expression before the sign, a `.` or a `:`, that precedes the caret.
pub(super) fn members(listing: &mut Listing, file: FileId, src: &Source, sign: usize, at: usize) {
    let sign = src.toks[sign];
    let index = src.root().tree().node_at(&src.text, sign.start);
    let Some(prefix) = psi::prefix(index).filter(|_| index.is(N::IndexExpr)) else { return };
    let from = Psi { file, src, node: index };
    let ty = listing.cx.infer(from.at(prefix));
    let context = context_class(listing.cx, from);
    let colon = sign.text(&src.text) == ":";
    let (value, replaced) = (CompletionKind::Field, Some((sign.start, at)));
    let mode = Mode { colon, object: "", read: true, value, replaced };
    for class in ty.top_classes() {
        for listed in members::visible(listing.cx, class) {
            let inherited = u8::from(listed.class != *class);
            let sort = format!("{inherited}{}", listed.member.name().to_lowercase());
            listing.member(&listed, class, &context, &mode, &sort);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST, completed, labels, shown};
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use openidea_plugin::providers::CompletionKind;

    const CONFIG: (&str, &str) = (
        "shop/shared/config.lua",
        "Config = { debug = false, ['max-items'] = 10 }\nConfig.rate = 2\n---@param name string\n---@return number\n\
         function Config.price(name, ...) end\nfunction Config:reload() return self end\n",
    );
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
        let members = items.iter().filter(|item| item.kind != Some(CompletionKind::Keyword));
        let labels: Vec<&str> = members.map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["first", "second"], "the postfix templates aside");
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
            "local a = 'Config.<caret>'",
            "function Config.<caret>",
            "local n = 12.<caret>",
            "Unknown.<caret>",
            "-- Config.<caret>",
        ] {
            assert!(labels(&[MANIFEST, CONFIG, (MAIN, text)]).is_empty(), "{text}");
        }
        for text in ["local a = 1 .. <caret>", "local a = Config<caret>"] {
            let listed = labels(&[MANIFEST, CONFIG, (MAIN, text)]);
            assert!(listed.contains(&"Config".to_owned()) && !listed.contains(&"rate".to_owned()), "{text}: names");
        }
    }
}
