//! Where what the name under the caret stands for is declared (`reference/` and `comment/reference/` of the
//! JetBrains plugin), and where the classes of its type are.

use super::target::{self, Target};
use crate::lua::Source;
use crate::lua::doc::tags;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Elem, Psi};
use crate::ty::{ANONYMOUS, Detail, FileId, GLOBAL, Ty, psi};
use openidea_plugin::Location;

/// Where the name of a declaration is written in its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Named {
    file: FileId,
    start: usize,
    end: usize,
}

/// Where the name a declaration of the Lua tree gives is written: its identifier, or the string that names a
/// field between brackets.
fn declared_name(node: Node) -> (usize, usize) {
    let named = match node.kind() {
        N::ClassMethodDef => node.child(N::ClassMethodName).unwrap_or(node),
        _ => node,
    };
    let keyed = || match named.kind() {
        N::TableField => named.expr().filter(|_| named.has(T::LBrack)),
        _ => psi::key_expr(named),
    };
    match named.token(T::Id) {
        Some(id) => id.range(),
        None => keyed().map_or(node.code_range(), Node::code_range),
    }
}

/// The `{ name: T }` a class of that name was written as, in the comments of `src`.
fn written_field(src: &Source, table: usize, name: &str) -> Option<(usize, usize)> {
    let docs = src.docs();
    let inner = docs.tree().node_at(docs.source(), table);
    let def = std::iter::once(inner).chain(inner.ancestors()).find(|node| node.is(N::DocTableDef))?;
    let field = def.children_of(N::DocTableField).find(|field| tags::name(*field) == Some(name))?;
    field.token(T::Id).map(|id| id.range())
}

/// The file and the offset a table written in a comment is named after: `10|<file>|<offset>`.
fn written_table(name: &str) -> Option<(FileId, usize)> {
    let (file, offset) = name.strip_prefix("10|")?.split_once('|')?;
    Some((file.parse().ok()?, offset.parse().ok()?))
}

fn name_of(cx: &Cx, target: &Target) -> Option<Named> {
    match target {
        Target::Elem(Elem { file, node, doc: false }) => {
            let (start, end) = cx.at(*file, *node, |psi| declared_name(psi.node))?;
            Some(Named { file: *file, start, end })
        }
        Target::Elem(Elem { file, node, .. }) => {
            let named = cx.read(*file, |src| {
                let docs = src.docs();
                let known = (*node as usize) < docs.tree().len();
                let tag = known.then(|| docs.tree().node(docs.source(), *node))?;
                tag.token(T::Id).map(|id| id.range())
            });
            let (start, end) = named.flatten()?;
            Some(Named { file: *file, start, end })
        }
        Target::Written(owner, field) => {
            let Ty::Class(class) = owner else { return None };
            let (file, table) = written_table(&class.name)?;
            let (start, end) = cx.read(file, |src| written_field(src, table, &field.name)).flatten()?;
            Some(Named { file, start, end })
        }
    }
}

/// Where what a name stands for is written.
pub fn place(cx: &Cx, target: &Target) -> Option<Location> {
    name_of(cx, target).and_then(|named| location(cx, named))
}

fn location(cx: &Cx, named: Named) -> Option<Location> {
    let path = cx.files.path(named.file)?;
    let range = cx.read(named.file, |src| src.lines.range(&src.text, named.start, named.end))?;
    Some(Location { path, range })
}

/// The label a `goto` jumps to: the one of that name in the blocks around it, inside its function.
pub(crate) fn label<'a>(goto: Node<'a>, name: &str) -> Option<Node<'a>> {
    let blocks = goto.ancestors().take_while(|node| !node.is(N::FuncBody));
    let mut labels = blocks.flat_map(|block| block.children_of(N::LabelStat));
    labels.find(|label| label.token(T::Id).is_some_and(|id| id.text() == name))
}

/// What a token that is no name of a declaration leads to: the label of a `goto`, or the file a `require`
/// loads.
fn other(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Named> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let index = target::token_at(tokens, at, |kind| matches!(kind, T::Id | T::String))?;
    let node = tree.node(root.source(), tree.node_of_token(index));
    match tokens.kind(index) {
        T::Id if node.is(N::GotoStat) => {
            let (start, end) = label(node, tokens.text(root.source(), index))?.token(T::Id)?.range();
            Some(Named { file, start, end })
        }
        T::String => {
            let call = node.ancestors().find(|node| node.is(N::CallExpr))?;
            let callee = psi::prefix(call).filter(|callee| callee.is(N::NameExpr))?;
            let named = psi::first_string_arg(call).filter(|arg| *arg == node)?;
            if !cx.options.require_like.iter().any(|name| name == callee.code()) {
                return None;
            }
            let required = cx.files.required(&psi::string_value(src, named)?)?;
            Some(Named { file: required, start: 0, end: 0 })
        }
        _ => None,
    }
}

/// Where the name at `at` in `src`, the file `file`, is declared; nothing on the name of the declaration
/// itself.
pub fn definition(cx: &Cx, file: FileId, src: &Source, at: usize) -> Vec<Location> {
    let found = match target::at(cx, file, src, at) {
        Some(named) => {
            let declared = name_of(cx, &named.target);
            declared.filter(|declared| {
                (declared.file, declared.start, declared.end) != (file, named.range.0, named.range.1)
            })
        }
        None => other(cx, file, src, at),
    };
    found.and_then(|named| location(cx, named)).into_iter().collect()
}

/// The names of the classes a value of this type is one of: those of a union, of the elements of an array,
/// of the first value of a tuple.
fn classes(ty: &Ty, names: &mut Vec<String>) {
    match ty {
        Ty::Class(class) if class.flags & (ANONYMOUS | GLOBAL) == 0 && matches!(class.detail, Detail::Named) => {
            if !names.iter().any(|name| **name == *class.name) {
                names.push(class.name.to_string());
            }
        }
        Ty::Array(base) => classes(base, names),
        Ty::Union(members) => members.iter().for_each(|member| classes(member, names)),
        Ty::Tuple(list) => list.first().into_iter().for_each(|first| classes(first, names)),
        _ => {}
    }
}

/// Where the classes and the aliases of the type of the name at `at` are declared.
pub fn type_definition(cx: &Cx, file: FileId, src: &Source, at: usize) -> Vec<Location> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let Some(index) = target::token_at(tokens, at, |kind| kind == T::Id) else { return Vec::new() };
    let node = tree.node(root.source(), tree.node_of_token(index));
    let typed = match node.kind() {
        N::NameExpr | N::IndexExpr | N::NameDef | N::ParamNameDef | N::TableField => node,
        _ => return Vec::new(),
    };
    let ty = cx.with_value(0, || cx.infer(Psi { file, src, node: typed }));
    let mut names = Vec::new();
    classes(&ty, &mut names);
    let declared = |name: &String| {
        let class = cx.index.class_in(name).map(|(file, class)| (file, class.node));
        let (file, node) = class.or_else(|| cx.index.alias_in(name).map(|(file, alias)| (file, alias.node)))?;
        name_of(cx, &Target::Elem(Elem { file, node, doc: true })).and_then(|named| location(cx, named))
    };
    names.iter().filter_map(declared).collect()
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST};
    use crate::feature::Feature;
    use crate::testing::Fixture;

    const SHOP_FILE: &str = "shop/shared/shop.lua";
    const SHOP: &str = "---@class Shop
---@field name string
Shop = {}

function Shop:buy(item) end

Shop.rate = 2
Shop.limits = { low = 1, ['high-end'] = 9 }

---@alias Mode 'open'|'closed'

---@type Shop
shop = nil

function count(n) return n end

return Shop
";

    /// Where the `<caret>` of `main` leads, as `file:line:from-to`, lines and columns counted from one.
    fn found(main: &str, of_type: bool) -> Vec<String> {
        let fx = Fixture::new(&[MANIFEST, (SHOP_FILE, SHOP), (MAIN, main)]);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(MAIN);
        let cx = fx.cx(&doc);
        let locations = if of_type { feature.type_definition(&cx, at) } else { feature.definition(&cx, at) };
        let shown = |location: &openidea_plugin::Location| {
            let (start, end) = (location.range.start, location.range.end);
            let file = location.path.rsplit('/').next().unwrap_or_default();
            format!("{file}:{}:{}-{}", start.line + 1, start.character + 1, end.character + 1)
        };
        locations.iter().map(shown).collect()
    }

    fn declared(main: &str) -> Vec<String> {
        found(main, false)
    }

    #[test]
    fn a_name_leads_to_what_declares_it() {
        assert_eq!(declared("local a = 1\nlocal b = a<caret>"), ["main.lua:1:7-8"], "a local");
        assert_eq!(
            declared("local function f(item)\n    return it<caret>em\nend"),
            ["main.lua:1:18-22"],
            "a parameter"
        );
        assert_eq!(declared("local function f() end\nf<caret>()"), ["main.lua:1:16-17"], "a local function");
        assert_eq!(
            declared("local f\nfunction f<caret>() end"),
            ["main.lua:1:7-8"],
            "the local a function is given to"
        );
        assert_eq!(declared("local s = sh<caret>op"), ["shop.lua:13:1-5"], "a global of another file");
        assert_eq!(declared("cou<caret>nt(1)"), ["shop.lua:15:10-15"], "a global function");
        assert_eq!(declared("function Shop:close()\n    return se<caret>lf\nend"), ["shop.lua:3:1-5"], "`self`");
    }

    #[test]
    fn a_field_leads_to_the_member_of_its_class() {
        assert_eq!(declared("shop:bu<caret>y()"), ["shop.lua:5:15-18"], "a method");
        assert_eq!(declared("local n = shop.na<caret>me"), ["shop.lua:2:11-15"], "a field a comment declares");
        assert_eq!(declared("local r = shop.ra<caret>te"), ["shop.lua:7:6-10"], "a field that is assigned");
        assert_eq!(declared("local l = shop.limits.lo<caret>w"), ["shop.lua:8:17-20"], "a field of a table");
        assert_eq!(declared("local h = shop.limits['high<caret>-end']"), ["shop.lua:8:27-37"], "named by a string");
        assert_eq!(
            declared("local t = {}\nt.count = 1\nprint(t.cou<caret>nt)"),
            ["main.lua:2:3-8"],
            "of a local table"
        );
    }

    #[test]
    fn names_of_comments_lead_to_what_they_name() {
        assert_eq!(declared("---@type Sh<caret>op\nlocal s"), ["shop.lua:1:11-15"], "a class");
        assert_eq!(declared("---@type Mo<caret>de\nlocal m"), ["shop.lua:10:11-15"], "an alias");
        assert_eq!(declared("---@param it<caret>em string\nlocal function f(item) end"), ["main.lua:2:18-22"]);
        assert_eq!(declared("---@see Shop#bu<caret>y\nlocal function f() end"), ["shop.lua:5:15-18"]);
        assert_eq!(
            declared("---@type { id: number, label: string }\nlocal row\nprint(row.la<caret>bel)"),
            ["main.lua:1:24-29"],
            "a field of a table written in a type"
        );
    }

    #[test]
    fn a_label_and_a_required_file() {
        let jump = "for i = 1, 2 do\n    if i == 1 then goto cont<caret>inue end\n    ::continue::\nend";
        assert_eq!(declared(jump), ["main.lua:3:7-15"], "a label written after its goto");
        assert_eq!(declared("local m = require('shared.sh<caret>op')"), ["shop.lua:1:1-1"]);
        assert_eq!(declared("local m = print('shared.sh<caret>op')"), Vec::<String>::new(), "no require");
    }

    #[test]
    fn nothing_on_a_declaration_or_on_what_nothing_declares() {
        for main in [
            "local a<caret> = 1",
            "unkn<caret>own()",
            "loc<caret>al a = 1",
            "local a = 'te<caret>xt'",
            "goto no<caret>where",
        ] {
            assert_eq!(declared(main), Vec::<String>::new(), "{main}");
        }
    }

    #[test]
    fn the_type_of_a_name_leads_to_its_classes() {
        assert_eq!(found("local s = sh<caret>op", true), ["shop.lua:1:11-15"]);
        assert_eq!(found("local s<caret> = shop", true), ["shop.lua:1:11-15"], "on the declaration too");
        assert_eq!(found("---@type Shop[]\nlocal li<caret>st", true), ["shop.lua:1:11-15"], "the elements of an array");
        assert_eq!(found("---@type Mode\nlocal mo<caret>de", true), ["shop.lua:10:11-15"], "an alias");
        assert_eq!(found("---@class Stall\nlocal Stall = {}\n---@type Shop|Stall\nlocal on<caret>e", true).len(), 2);
        assert_eq!(found("local n<caret> = 1", true), Vec::<String>::new(), "a number has no class");
    }
}
