//! What hovering a name shows: how it is declared, then what its comment says (`documentation/` of the
//! JetBrains plugin: `LuaDocumentationProvider`, `DocRenderer`), written as Markdown.

use super::target::{self, Target};
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::token::{T, is_identifier};
use crate::lua::tree::{Element, N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::declarations;
use crate::ty::doc::Reader;
use crate::ty::{FileId, Function, Param, Ret, Signature, Ty, display, made_for_variable, plain, psi};

/// A declaration longer than this is written with one parameter per line.
const MAX_LINE: usize = 90;
/// How many entries one section lists.
const MAX_LISTED: usize = 40;
/// The IDE takes 64 KiB of Markdown at most.
const MAX_BYTES: usize = 48 << 10;
/// How deep inside a type a function has what it returns read in its body: a function that returns a
/// function that returns the first one has no end.
const BODIES_DEEP: u8 = 1;

pub struct Hover {
    /// Where the documented name is written.
    pub range: (usize, usize),
    pub markdown: String,
}

/// What is shown for the name at `at` in `src`, the file `file`.
pub fn hover(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Hover> {
    let named = target::at(cx, file, src, at)?;
    Some(Hover { range: named.range, markdown: document(cx, &named.target)? })
}

/// The documentation of what a name stands for.
pub fn document(cx: &Cx, target: &Target) -> Option<String> {
    let mut page = Page { cx, out: String::new() };
    match target {
        Target::Written(owner, field) => {
            let optional = if field.optional { "?" } else { "" };
            let code = format!("{}.{}{optional}: {}", page.shown(owner), field.name, page.shown(&field.ty));
            page.definition(&code);
        }
        Target::Elem(elem) if elem.doc => {
            let written = cx.read(elem.file, |src| {
                let docs = src.docs();
                let known = (elem.node as usize) < docs.tree().len();
                known.then(|| page.tag(elem.file, docs.tree().node(docs.source(), elem.node))).flatten()
            });
            written??;
        }
        Target::Elem(elem) => cx.at(elem.file, elem.node, |psi| page.declared(psi))??,
    }
    let mut markdown = page.out.trim_end().to_owned();
    if markdown.len() > MAX_BYTES {
        let end = (0..=MAX_BYTES).rev().find(|end| markdown.is_char_boundary(*end)).unwrap_or(0);
        markdown.truncate(end);
        markdown.push('…');
    }
    (!markdown.is_empty()).then_some(markdown)
}

/// The type with what its functions return, where they do not say it, read in their bodies: `depth`
/// functions deep, and `any` deeper.
fn settled(cx: &Cx, ty: &Ty, depth: u8) -> Ty {
    let settle = |signature: &Signature| Signature {
        params: signature
            .params
            .iter()
            .map(|param| Param { ty: settled(cx, &param.ty, depth), ..param.clone() })
            .collect(),
        ret: Ret::Ty(match &signature.ret {
            Ret::Ty(ty) => settled(cx, ty, depth),
            Ret::Body(_) if depth == 0 => Ty::Unknown,
            Ret::Body(_) => settled(cx, &cx.ret(&signature.ret), depth - 1),
        }),
        vararg: signature.vararg.as_ref().map(|ty| settled(cx, ty, depth)),
        ..signature.clone()
    };
    match ty {
        Ty::Function(function) => Ty::function(Function {
            main: settle(&function.main),
            overloads: function.overloads.iter().map(settle).collect(),
            flags: function.flags,
        }),
        Ty::Array(base) => Ty::array(settled(cx, base, depth)),
        Ty::Tuple(list) => Ty::tuple(list.iter().map(|ty| settled(cx, ty, depth)).collect()),
        Ty::Union(members) => members.iter().fold(Ty::Unknown, |all, member| all.union(&settled(cx, member, depth))),
        Ty::Generic(generic) => {
            let params = generic.params.iter().map(|param| settled(cx, param, depth)).collect();
            Ty::generic(settled(cx, &generic.base, depth), params)
        }
        _ => ty.clone(),
    }
}

/// A piece of code inside a line of Markdown.
fn code(text: &str) -> String {
    let ticks = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    if ticks == 0 {
        return format!("`{text}`");
    }
    let fence = "`".repeat(ticks + 1);
    format!("{fence} {text} {fence}")
}

/// `code — description`, or the one of the two there is.
fn described(code: &str, description: &str) -> String {
    match (code.is_empty(), description.is_empty()) {
        (_, true) => code.to_owned(),
        (true, false) => description.to_owned(),
        (false, false) => format!("{code} — {description}"),
    }
}

/// `class Name : Parent, Other`, as a `@class`, `@enum` or `@module` tag declares it.
fn class_head(tag: Node) -> Option<String> {
    let name = tags::name(tag)?;
    let kind = if tag.has(T::DocTagEnum) {
        "enum"
    } else if tag.has(T::DocTagModule) {
        "module"
    } else {
        "class"
    };
    let parents: Vec<&str> = tag.children_of(N::DocClassNameRef).map(Node::code).collect();
    Some(if parents.is_empty() { format!("{kind} {name}") } else { format!("{kind} {name} : {}", parents.join(", ")) })
}

/// The name a table constructor is given to by its statement: `Config` of `Config = {}`.
fn assigned_name(table: Node<'_>) -> Option<&str> {
    let stat = table.parent().filter(|list| list.is(N::ExprList))?.parent()?;
    let rank = psi::rank(table);
    match stat.kind() {
        N::LocalDef => stat.child(N::NameList)?.children_of(N::NameDef).nth(rank).map(Node::code),
        N::AssignStat => stat.child(N::VarList)?.exprs().nth(rank).map(Node::code),
        _ => None,
    }
}

/// `owner.name`, or `owner['name']` for a name that is no identifier.
fn member_path(owner: Option<&str>, sign: char, name: &str) -> String {
    match owner {
        Some(owner) if is_identifier(name) => format!("{owner}{sign}{name}"),
        Some(owner) => format!("{owner}['{name}']"),
        None => name.to_owned(),
    }
}

/// The Markdown of one hover, as it is written.
struct Page<'a, 'c> {
    cx: &'a Cx<'c>,
    out: String,
}

impl Page<'_, '_> {
    /// A type as a reader is shown it.
    fn shown(&self, ty: &Ty) -> String {
        display(&plain(&settled(self.cx, ty, BODIES_DEEP)))
    }

    /// The declaration, as a block of code: the IDE colours it, and shows it alone where it is brief.
    fn definition(&mut self, code: &str) {
        let ticks = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(ticks.max(2) + 1);
        self.out.push_str(&format!("{fence}lua\n{code}\n{fence}\n\n"));
    }

    fn paragraph(&mut self, text: &str) {
        let text = text.trim_matches('\n').trim_end();
        if !text.trim().is_empty() {
            self.out.push_str(text);
            self.out.push_str("\n\n");
        }
    }

    fn section(&mut self, title: &str, entries: Vec<String>) {
        if entries.is_empty() {
            return;
        }
        self.out.push_str(&format!("**{title}**\n"));
        for entry in entries.iter().take(MAX_LISTED) {
            self.out.push_str(&format!("- {entry}\n"));
        }
        if entries.len() > MAX_LISTED {
            self.out.push_str(&format!("- … {} more\n", entries.len() - MAX_LISTED));
        }
        self.out.push('\n');
    }

    /// `head(a: T, b?: U, ...: V): R`.
    fn signature(&self, head: &str, signature: &Signature) -> String {
        let mut params: Vec<String> = signature
            .params
            .iter()
            .map(|param| format!("{}: {}", param.display_name(), self.shown(&param.ty)))
            .collect();
        if let Some(extra) = &signature.vararg {
            params.push(format!("...: {}", self.shown(extra)));
        }
        let returned = self.shown(&self.cx.ret(&signature.ret));
        let line = format!("{head}({}): {returned}", params.join(", "));
        if line.chars().count() <= MAX_LINE || params.len() < 2 {
            return line;
        }
        format!("{head}(\n    {}\n): {returned}", params.join(",\n    "))
    }

    fn declared(&mut self, psi: Psi) -> Option<()> {
        match psi.node.kind() {
            N::ParamNameDef => self.param(psi),
            N::NameDef => self.local(psi),
            N::LocalFuncDef => self.local_function(psi),
            N::FuncDef | N::ClassMethodDef | N::IndexExpr | N::NameExpr | N::TableField => self.member(psi)?,
            _ => return None,
        }
        Some(())
    }

    /// A parameter, or a variable of a loop: that one is a local.
    fn param(&mut self, def: Psi) {
        let name = def.node.code();
        let reader = Reader::new(def.file);
        let looped = def.node.parent().is_some_and(|owner| matches!(owner.kind(), N::ForAStat | N::ForBStat));
        let kind = if looped { "local" } else { "param" };
        match declarations::comment(def).and_then(|comment| comment.param(name)) {
            Some(tag) => {
                let optional = if tags::is_optional(tag) { "?" } else { "" };
                self.definition(&format!("{kind} {name}{optional}: {}", self.shown(&reader.tag(tag))));
                self.paragraph(tags::description(tag));
                let values = self.values(reader, tag, "");
                self.paragraph(&values);
            }
            None => {
                let ty = self.shown(&self.cx.infer(def));
                self.definition(&format!("{kind} {name}: {ty}"));
            }
        }
    }

    fn local(&mut self, def: Psi) {
        let ty = self.shown(&self.cx.infer(def));
        self.definition(&format!("local {}: {ty}", def.node.code()));
        if let Some(comment) = declarations::comment(def) {
            self.comment(def.file, comment, None);
        }
    }

    fn local_function(&mut self, def: Psi) {
        let head = format!("local function {}", psi::function_name(def.node).unwrap_or_default());
        let code = match self.cx.infer(def).as_function() {
            Some(function) => self.signature(&head, &function.main),
            None => head,
        };
        self.definition(&code);
        if let Some(comment) = def.src.comment_of(def.node) {
            self.comment(def.file, comment, None);
        }
    }

    /// What a member is shown as a member of: the class of its object, under the name a reader knows it by.
    fn owner(&self, member: Psi) -> Option<String> {
        let node = member.node;
        let object = match node.kind() {
            N::ClassMethodDef => psi::method_owner(node)?,
            N::IndexExpr => psi::prefix(node)?,
            N::TableField => {
                let table = node.parent().filter(|table| table.is(N::TableExpr))?;
                return Some(assigned_name(table).unwrap_or("table").to_owned());
            }
            _ => return None,
        };
        let ty = self.cx.with_value(0, || self.cx.infer(member.at(object)));
        Some(match ty.perfect_class() {
            Some(Ty::Class(class)) if made_for_variable(class) && !class.var_name.is_empty() => {
                class.var_name.to_string()
            }
            Some(class) => display(class),
            None if object.code().contains('\n') => "…".to_owned(),
            None => object.code().to_owned(),
        })
    }

    /// A function statement, a field that is assigned, a global, or a field of a table constructor
    /// (`renderClassMember`).
    fn member(&mut self, member: Psi) -> Option<()> {
        let node = member.node;
        let name = match node.kind() {
            N::FuncDef | N::ClassMethodDef => psi::function_name(node)?.to_owned(),
            N::IndexExpr => psi::member_name(member.src, node)?.into_owned(),
            N::TableField => psi::field_name(member.src, node)?.into_owned(),
            _ => node.code().to_owned(),
        };
        let statement = matches!(node.kind(), N::FuncDef | N::ClassMethodDef);
        let assigned = psi::assign_stat(node);
        let ty = self.cx.with_value(0, || self.cx.infer(member));
        // A name nothing declares is a native, which its own feature documents, or is unknown; a field nothing
        // declares is shown only where its type is known.
        let read = assigned.is_none() && matches!(node.kind(), N::IndexExpr | N::NameExpr);
        if read && (node.is(N::NameExpr) || plain(&ty).is_invalid()) {
            return None;
        }
        let owner = self.owner(member);
        let code = match ty.as_function() {
            Some(function) => {
                let sign = if function.is_colon_call() { ':' } else { '.' };
                let path = member_path(owner.as_deref(), sign, &name);
                let head = if statement { format!("function {path}") } else { path };
                self.signature(&head, &function.main)
            }
            None => format!("{}: {}", member_path(owner.as_deref(), '.', &name), self.shown(&ty)),
        };
        self.definition(&code);
        let comment = match node.kind() {
            N::IndexExpr | N::NameExpr => assigned.and_then(|stat| member.src.comment_of(stat)),
            _ => member.src.comment_of(node),
        };
        if let Some(comment) = comment {
            self.comment(member.file, comment, None);
        }
        Some(())
    }

    /// A class, a field or an alias a comment declares.
    fn tag(&mut self, file: FileId, tag: Node) -> Option<()> {
        let comment = Comment(tag.ancestor(N::DocComment)?);
        let reader = Reader::new(file);
        match tag.kind() {
            N::DocTagClass => {
                self.definition(&class_head(tag)?);
                self.comment(file, comment, Some(tag));
            }
            N::DocTagField => {
                let name = tags::name(tag)?;
                let named = tag.child(N::DocClassNameRef).map(Node::code);
                let class = named.or_else(|| comment.tag(N::DocTagClass).and_then(tags::name));
                let ty = reader.tag(tag);
                let path = member_path(class, '.', name);
                let code = match ty.as_function() {
                    Some(function) => self.signature(&path, &function.main),
                    None => {
                        let optional = if tag.has(T::Question) { "?" } else { "" };
                        format!("{path}{optional}: {}", self.shown(&ty))
                    }
                };
                self.definition(&code);
                self.paragraph(tags::description(tag));
            }
            N::DocTagAlias => {
                self.definition(&format!("alias {} = {}", tags::name(tag)?, self.shown(&reader.alias(tag))));
                self.paragraph(tags::description(tag));
                let values = self.values(reader, tag, "");
                self.paragraph(&values);
            }
            _ => return None,
        }
        Some(())
    }

    /// The values a parameter or an alias lists, `---| value # what it is`, one per line.
    fn values(&self, reader: Reader, tag: Node, indent: &str) -> String {
        let mut out = String::new();
        for member in tag.children_of(N::DocAliasMember) {
            let Some(ty) = tags::ty(member) else { continue };
            let default = if tags::is_default(member) { " (default)" } else { "" };
            let value = format!("{}{default}", code(&self.shown(&reader.ty(ty))));
            out.push_str(&format!("{indent}- {}\n", described(&value, tags::description(member))));
        }
        out
    }

    /// `name: type`, each as code.
    fn typed(&self, reader: Reader, name: &str, ty: Option<Node>) -> String {
        match ty {
            Some(ty) => format!("{}: {}", code(name), code(&self.shown(&reader.ty(ty)))),
            None => code(name),
        }
    }

    fn field_entry(&self, reader: Reader, tag: Node) -> Option<String> {
        let name = match (tags::key_ty(tag), tags::name(tag)) {
            (Some(key), _) => format!("[{}]", self.shown(&reader.ty(key))),
            (None, Some(name)) => format!("{name}{}", if tags::is_optional(tag) { "?" } else { "" }),
            (None, None) => return None,
        };
        Some(described(&self.typed(reader, &name, tags::ty(tag)), tags::description(tag)))
    }

    fn param_entry(&self, reader: Reader, tag: Node) -> Option<String> {
        let name = tag.child(N::DocParamNameRef)?.code();
        let name = format!("{name}{}", if tags::is_optional(tag) { "?" } else { "" });
        let entry = described(&self.typed(reader, &name, tags::ty(tag)), tags::description(tag));
        Some(format!("{entry}\n{}", self.values(reader, tag, "  ")).trim_end().to_owned())
    }

    /// The values one `@return` gives, each with its name when it has one.
    fn return_entry(&self, reader: Reader, tag: Node) -> Option<String> {
        let list = tag.child(N::DocTypeList)?;
        let names = tags::value_names(list);
        let values: Vec<String> = tags::types(list)
            .enumerate()
            .map(|(rank, ty)| match names.get(rank).filter(|name| !name.is_empty()) {
                Some(name) => self.typed(reader, name, Some(ty)),
                None => code(&self.shown(&reader.ty(ty))),
            })
            .collect();
        let values = if values.len() > 1 { format!("({})", values.join(", ")) } else { values.concat() };
        Some(described(&values, tags::description(tag)))
    }

    /// What a comment says (`renderComment`): its text, then what its tags tell. `shown` is the class tag the
    /// declaration above already shows.
    fn comment(&mut self, file: FileId, comment: Comment, shown: Option<Node>) {
        let reader = Reader::new(file);
        let mut text = String::new();
        let mut after_text = false;
        for element in comment.0.elements() {
            match element {
                Element::Token(token) if token.is(T::DocString) => {
                    after_text = true;
                    text.push_str(token.text().strip_prefix(' ').unwrap_or(token.text()));
                }
                Element::Token(token) if token.is(T::DocDashes) && after_text => text.push('\n'),
                Element::Token(_) => {}
                Element::Node(tag) => {
                    after_text = false;
                    let head = match tag.kind() {
                        N::DocTagClass if Some(tag) != shown => class_head(tag).map(|head| code(&head)),
                        N::DocTagClass | N::DocTagType => None,
                        _ => continue,
                    };
                    self.paragraph(&std::mem::take(&mut text));
                    self.paragraph(&described(&head.unwrap_or_default(), tags::description(tag)));
                }
            }
        }
        self.paragraph(&text);

        for title in ["Version", "Author", "Since", "Deprecated"] {
            let named = |tag: &Node| tag.token(T::DocTagName).is_some_and(|name| name.text() == title.to_lowercase());
            for tag in comment.all(N::DocTagDef).filter(named) {
                self.paragraph(format!("**{title}** {}", tags::description(tag)).trim_end());
            }
        }
        let fields = comment.tags(N::DocTagField).filter_map(|tag| self.field_entry(reader, tag)).collect();
        self.section("Fields", fields);
        let params = comment.tags(N::DocTagParam).filter_map(|tag| self.param_entry(reader, tag)).collect();
        self.section("Parameters", params);
        let returns = comment.tags(N::DocTagReturn).filter_map(|tag| self.return_entry(reader, tag)).collect();
        self.section("Returns", returns);
        let overloads = comment
            .tags(N::DocTagOverload)
            .filter_map(|tag| Some(code(&self.shown(&reader.ty(tag.child(N::DocFunctionTy)?)))));
        let overloads = overloads.collect();
        self.section("Overloads", overloads);
        let seen = comment.tags(N::DocTagSee).filter_map(|tag| {
            let class = tag.child(N::DocClassNameRef)?.code();
            let named = tags::name(tag).map_or(class.to_owned(), |member| format!("{class}#{member}"));
            Some(described(&code(&named), tags::description(tag)))
        });
        let seen = seen.collect();
        self.section("See", seen);
    }
}

#[cfg(test)]
mod tests {
    use super::{Hover, code, plain};
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST};
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use crate::ty::{ANONYMOUS, GLOBAL, Ty, display};

    const SHOP_FILE: &str = "shop/shared/shop.lua";
    const SHOP: &str = "--- A shop of the map.
--- It sells *items*.
---@class Shop : Base, Named the base of every shop
---@field name string its name
---@field items? table<string, number>
---@field [number] string
---@field onOpen fun(item: string): boolean
local Shop = {}

--- Buys an item.
---
--- Returns nothing when the shop is closed.
---@param item string the name of the item
---@param count? number how many
---@param mode 'cash'|'card'
---| 'cash' # coins
---|> 'card' # the default
---@return boolean ok whether it was sold
---@return string? reason why not
---@overload fun(item: string): boolean
---@see Shop#open
---@deprecated use `Shop:sell`
---@since 1.2
function Shop:buy(item, count, mode, ...)
    return true
end

function Shop.open(self, door)
    self.door = door
    return self
end

Shop.rate = 2
--- The limits.
Shop.limits = { low = 1, ['high-end'] = 9 }

---@alias Mode
---| 'open' # doors open
---| 'closed'

---@type Shop
shop = nil

--- Counts things.
---@param n number
function count(n) return n * 2 end

return Shop
";

    /// What hovering the `<caret>` of the last file shows.
    fn hover(files: &[(&str, &str)]) -> Option<Hover> {
        let fx = Fixture::new(files);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(files.last().expect("a file").0);
        feature.hover(&fx.cx(&doc), at)
    }

    /// What is shown at the `<caret>` of `main`, a script of the resource of the shop.
    fn shown(main: &str) -> String {
        hover(&[MANIFEST, (SHOP_FILE, SHOP), (MAIN, main)]).map(|hover| hover.markdown).unwrap_or_default()
    }

    /// What is shown in the file of the shop, one character after the start of `needle`.
    fn shown_in_shop(needle: &str) -> String {
        let at = SHOP.find(needle).expect("the needle") + 1;
        let text = format!("{}<caret>{}", &SHOP[..at], &SHOP[at..]);
        hover(&[MANIFEST, (SHOP_FILE, &text)]).map(|hover| hover.markdown).unwrap_or_default()
    }

    const BUY: &str = r#"```lua
function Shop:buy(
    item: string,
    count?: number,
    mode: "cash"|"card",
    ...: any
): (boolean, string)
```

Buys an item.

Returns nothing when the shop is closed.

**Since** 1.2

**Deprecated** use `Shop:sell`

**Parameters**
- `item`: `string` — the name of the item
- `count?`: `number` — how many
- `mode`: `"cash"|"card"`
  - `"cash"` — coins
  - `"card"` (default) — the default

**Returns**
- `ok`: `boolean` — whether it was sold
- `reason`: `string` — why not

**Overloads**
- `fun(item: string): boolean`

**See**
- `Shop#open`"#;

    const CLASS: &str = r#"**Fields**
- `name`: `string` — its name
- `items?`: `table<string, number>`
- `[number]`: `string`
- `onOpen`: `fun(item: string): boolean`"#;

    #[test]
    fn a_method_shows_how_it_is_declared_and_what_its_comment_says() {
        assert_eq!(shown("shop:bu<caret>y('a')"), BUY, "where it is called");
        assert_eq!(shown_in_shop("buy(item"), BUY, "where it is declared");
        let hover = hover(&[MANIFEST, (SHOP_FILE, SHOP), (MAIN, "shop:bu<caret>y('a')")]).expect("a hover");
        assert_eq!(hover.range, (5, 8), "the name is what is documented");
        assert_eq!(shown("shop:buy<caret>('a')"), BUY, "the caret right after the name");
    }

    #[test]
    fn a_class_shows_its_parents_its_text_and_its_fields() {
        let text = "A shop of the map.\nIt sells *items*.";
        let class = format!("```lua\nclass Shop : Base, Named\n```\n\n{text}\n\nthe base of every shop\n\n{CLASS}");
        assert_eq!(shown("---@type Sh<caret>op\nlocal s"), class, "named by a type");
        assert_eq!(shown_in_shop("Shop : Base"), class, "where it is declared");
        assert_eq!(shown("---@class Stall : Sh<caret>op\nlocal Stall = {}"), class, "named as a parent");
        let variable = format!(
            "```lua\nlocal Shop: Shop\n```\n\n{text}\n\n`class Shop : Base, Named` — the base of every shop\n\n{CLASS}"
        );
        assert_eq!(shown_in_shop("Shop = {}"), variable, "the variable it annotates");
        assert_eq!(shown_in_shop("Shop:buy"), variable, "and where that variable is read");
        assert_eq!(shown("---@enum Sides\nlocal Si<caret>des = {}"), "```lua\nlocal Sides: Sides\n```\n\n`enum Sides`");
    }

    #[test]
    fn a_field_shows_the_class_it_belongs_to() {
        let named = "```lua\nShop.name: string\n```\n\nits name";
        assert_eq!(shown("local n = shop.na<caret>me"), named);
        assert_eq!(shown_in_shop("name string"), named, "where the comment declares it");
        assert_eq!(shown("shop.na<caret>me = 1"), named, "the annotation stands for an assignment of the same name");
        assert_eq!(shown("local r = shop.ra<caret>te"), "```lua\nShop.rate: number\n```");
        assert_eq!(shown("local l = shop.lim<caret>its"), "```lua\nShop.limits: table\n```\n\nThe limits.");
        assert_eq!(shown("local l = shop.limits.lo<caret>w"), "```lua\nShop.limits.low: number\n```");
        assert_eq!(shown_in_shop("low = 1"), "```lua\nShop.limits.low: number\n```", "in the table that writes it");
        assert_eq!(shown("local l = shop.limits['high<caret>-end']"), "```lua\nShop.limits['high-end']: number\n```");
        assert_eq!(shown("local o = shop.onO<caret>pen"), "```lua\nShop.onOpen(item: string): boolean\n```");
        assert_eq!(shown("local t = { al<caret>pha = 1 }"), "```lua\nt.alpha: number\n```");
        assert_eq!(shown("call({ al<caret>pha = 'a' })"), "```lua\ntable.alpha: string\n```");
        assert_eq!(
            shown("---@type { id: number, label?: string }\nlocal row\nlocal l = row.la<caret>bel"),
            "```lua\n{ id: number, label: string }.label?: string\n```"
        );
    }

    #[test]
    fn functions_show_their_parameters_and_what_they_return() {
        assert_eq!(
            shown("shop.op<caret>en()"),
            "```lua\nfunction Shop.open(self: any, door: any): any\n```",
            "declared with a dot: `self` is a parameter"
        );
        let counted =
            "```lua\nfunction count(n: number): number\n```\n\nCounts things.\n\n**Parameters**\n- `n`: `number`";
        assert_eq!(shown("cou<caret>nt(1)"), counted);
        assert_eq!(
            shown("--- Adds.\n---@return number\nlocal function ad<caret>d(a, b) return a + b end"),
            "```lua\nlocal function add(a: any, b: any): number\n```\n\nAdds.\n\n**Returns**\n- `number`"
        );
        assert_eq!(shown("local f\nfunction f<caret>() end"), "```lua\nlocal f: any\n```", "the local it is given to");
        assert_eq!(
            shown("handlers = {}\nhandlers.open = function(door) return 1 end\nhandlers.op<caret>en()"),
            "```lua\nhandlers.open(door: any): number\n```"
        );
    }

    #[test]
    fn what_a_function_returns_is_read_in_bodies_one_function_deep() {
        assert_eq!(
            shown("local function make() return function() return 1 end end\nlocal m = ma<caret>ke"),
            "```lua\nlocal function make(): fun(): number\n```"
        );
        assert_eq!(
            shown("function ping() return pong end\nfunction pong() return ping end\nlocal p = pi<caret>ng"),
            "```lua\nfunction ping(): fun(): fun(): any\n```",
            "two functions that return one another have no end"
        );
    }

    #[test]
    fn locals_parameters_and_loop_variables() {
        assert_eq!(shown("local a = 1\nlocal b = a<caret>"), "```lua\nlocal a: number\n```");
        assert_eq!(
            shown("--- How many.\n---@type number\nlocal cou<caret>nt"),
            "```lua\nlocal count: number\n```\n\nHow many."
        );
        assert_eq!(shown("local s<caret>s = {}"), "```lua\nlocal ss: table\n```", "the class made for it is not shown");
        let documented = "```lua\nparam item?: string\n```\n\nthe thing";
        assert_eq!(shown("---@param item? string the thing\nlocal function f(it<caret>em) end"), documented);
        assert_eq!(shown("---@param it<caret>em? string the thing\nlocal function f(item) end"), documented);
        assert_eq!(shown("local function f(it<caret>em) end"), "```lua\nparam item: any\n```");
        assert_eq!(
            shown("---@param mode Mode\n---| 'open' # doors open\n---|> 'closed'\nfunction set(mo<caret>de) end"),
            "```lua\nparam mode: Mode\n```\n\n- `\"open\"` — doors open\n- `\"closed\"` (default)"
        );
        assert_eq!(shown("for i<caret> = 1, 2 do end"), "```lua\nlocal i: number\n```");
        assert_eq!(
            shown("---@class Stall\nlocal Stall = {}\nfunction Stall:close()\n    return se<caret>lf\nend"),
            "```lua\nlocal Stall: Stall\n```\n\n`class Stall`",
            "`self` is what the method is declared on"
        );
    }

    #[test]
    fn globals_show_their_type_and_their_comment() {
        assert_eq!(shown("local s = sh<caret>op"), "```lua\nshop: Shop\n```");
        assert_eq!(
            shown("--- The settings.\nConfig = { debug = false }\nprint(Con<caret>fig)"),
            "```lua\nConfig: table\n```\n\nThe settings."
        );
    }

    #[test]
    fn an_alias_lists_its_values() {
        let alias = "```lua\nalias Mode = \"open\"|\"closed\"\n```\n\n- `\"open\"` — doors open\n- `\"closed\"`";
        assert_eq!(shown("---@type Mo<caret>de\nlocal mode"), alias);
        assert_eq!(shown_in_shop("Mode\n---| 'open'"), alias);
    }

    #[test]
    fn a_member_named_by_a_see_tag() {
        assert_eq!(shown_in_shop("open\n---@deprecated"), "```lua\nfunction Shop.open(self: any, door: any): any\n```");
    }

    #[test]
    fn nothing_is_shown_where_nothing_is_known() {
        for main in [
            "unknown<caret>Thing(1)",
            "local a = unknown.fie<caret>ld",
            "local a = 1<caret>2",
            "local a = 'te<caret>xt'",
            "-- a comm<caret>ent",
            "--- a descri<caret>ption\nlocal a",
            "---@ty<caret>pe Shop\nlocal a",
            "---@generic T\n---@param a T<caret>\nlocal function f(a) end",
            "loc<caret>al a = 1",
            "goto do<caret>ne\n::done::",
        ] {
            assert_eq!(shown(main), "", "{main}");
        }
    }

    #[test]
    fn a_name_is_code_whatever_it_holds() {
        assert_eq!(code("string"), "`string`");
        assert_eq!(code("`T`"), "`` `T` ``");
        assert_eq!(code("a``b"), "``` a``b ```");
        let made = Ty::flagged("12@3", "shop", ANONYMOUS).union(&Ty::TABLE);
        assert_eq!(display(&plain(&made)), "table", "the class made for a variable is left out");
        assert_eq!(display(&plain(&Ty::flagged("$shop", "shop", GLOBAL))), "any");
        assert_eq!(display(&plain(&Ty::array(Ty::class("Shop")))), "Shop[]");
    }
}
