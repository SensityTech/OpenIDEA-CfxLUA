//! The parameters of the function a call is written for, and the one the caret stands at
//! (`LuaParameterInfoHandler`).

use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::{ELLIPSIS, FileId, Signature, Ty, display, psi};

/// One signature of the called function, written as the parameters a call of that form gives values to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub label: String,
    /// Where each parameter is written in the label, in bytes.
    pub params: Vec<(usize, usize)>,
    /// Whether the last parameter takes every argument from its rank on.
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Help {
    pub signatures: Vec<Shown>,
    pub active_signature: usize,
    pub active_parameter: usize,
}

/// The arguments the caret stands in: between the parentheses of a call, or in the string or the table a call
/// is given without them. Arguments left open hold what follows them, up to the next token.
fn arguments(src: &Source, at: usize) -> Option<Node<'_>> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let holds = |args: &Node| {
        let (start, end) = args.code_range();
        if args.is(N::SingleArg) {
            return start <= at && at < end;
        }
        let closed = args.token(T::RParen).map(|close| close.start());
        let limit = closed.unwrap_or_else(|| {
            let next = (args.token_span().1..tokens.len()).find(|index| !tokens.kind(*index).is_trivia());
            next.map_or(src.text.len(), |index| tokens.start(index))
        });
        start < at && at <= limit
    };
    let innermost = |index: usize| {
        let node = tree.node(root.source(), tree.node_of_token(index));
        let mut around = std::iter::once(node).chain(node.ancestors());
        around.find(|node| matches!(node.kind(), N::ListArgs | N::SingleArg) && holds(node))
    };
    let here = tokens.at(at)?;
    let before = (0..=here).rev().find(|index| !tokens.kind(*index).is_trivia() && tokens.start(*index) < at);
    let found = [Some(here), before].into_iter().flatten().filter_map(innermost);
    found.max_by_key(|args| args.code_range().0)
}

/// `a: T, b?: U, ...: V`: the parameters a call gives values to, written with a colon or with a dot.
fn shown(signature: &Signature, colon: bool) -> Shown {
    let mut label = String::new();
    let mut params = Vec::new();
    let mut write = |name: &str, ty: &Ty| {
        if !label.is_empty() {
            label.push_str(", ");
        }
        let start = label.len();
        label.push_str(&format!("{name}: {}", display(ty)));
        params.push((start, label.len()));
    };
    for param in signature.args(None, colon) {
        write(&param.display_name(), &param.ty);
    }
    if let Some(extra) = &signature.vararg {
        write(ELLIPSIS, extra);
    }
    Shown { label, params, open: signature.vararg.is_some() }
}

/// What a call shows while its arguments are typed at `at` in `src`, the file `file`.
pub fn help(cx: &Cx, file: FileId, src: &Source, at: usize) -> Option<Help> {
    let args = arguments(src, at)?;
    let call = args.parent().filter(|call| call.is(N::CallExpr))?;
    let psi = Psi { file, src, node: call };
    let colon = psi::is_colon_call(call);
    let callee = psi.at(psi::prefix(call)?);
    let callee = cx.with_value(0, || cx.infer(callee));
    let mut signatures: Vec<Shown> = Vec::new();
    for function in callee.each().iter().filter_map(Ty::as_function) {
        for signature in function.signatures() {
            let shown = shown(signature, colon);
            if !shown.params.is_empty() && !signatures.contains(&shown) {
                signatures.push(shown);
            }
        }
    }
    if signatures.is_empty() {
        return None;
    }
    let given = args.tokens().filter(|token| token.is(T::Comma) && token.end() <= at).count();
    let takes = |shown: &Shown| given < shown.params.len() || shown.open;
    let active_signature = signatures.iter().position(takes).unwrap_or(0);
    let last = signatures[active_signature].params.len() - 1;
    let active_parameter = if signatures[active_signature].open { given.min(last) } else { given };
    Some(Help { signatures, active_signature, active_parameter })
}

#[cfg(test)]
mod tests {
    use super::Help;
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST};
    use crate::feature::Feature;
    use crate::testing::Fixture;

    const SHOP: (&str, &str) = (
        "shop/shared/shop.lua",
        "---@class Shop\nShop = {}\n\n---@param item string\n---@param count? number\n---@overload fun(item: string)\n\
         function Shop:buy(item, count, ...) end\n\n---@param name string\n---@param ... number\n\
         function Shop.open(name, ...) end\n\n---@type Shop\nshop = nil\n",
    );

    fn help(main: &str) -> Option<Help> {
        let fx = Fixture::new(&[MANIFEST, SHOP, (MAIN, main)]);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(MAIN);
        feature.signature_help(&fx.cx(&doc), at)
    }

    /// The labels, the active one between brackets around its active parameter: `a: T, [b: U]`.
    fn shown(main: &str) -> Vec<String> {
        let Some(help) = help(main) else { return Vec::new() };
        let marked = |(rank, shown): (usize, &super::Shown)| match shown.params.get(help.active_parameter) {
            Some(&(start, end)) if rank == help.active_signature => {
                format!("{}[{}]{}", &shown.label[..start], &shown.label[start..end], &shown.label[end..])
            }
            _ => shown.label.clone(),
        };
        help.signatures.iter().enumerate().map(marked).collect()
    }

    const ADD: &str = "---@param a number\n---@param b string\nlocal function add(a, b) end\n";

    #[test]
    fn the_parameter_the_caret_stands_at_is_the_active_one() {
        assert_eq!(shown(&format!("{ADD}add(<caret>)")), ["[a: number], b: string"]);
        assert_eq!(shown(&format!("{ADD}add(1<caret>, 2)")), ["[a: number], b: string"], "before the comma");
        assert_eq!(shown(&format!("{ADD}add(1,<caret> 2)")), ["a: number, [b: string]"], "after it");
        assert_eq!(shown(&format!("{ADD}add(1, 2<caret>)")), ["a: number, [b: string]"]);
        assert_eq!(shown(&format!("{ADD}add(1, 2, <caret>)")), ["a: number, b: string"], "one argument too many");
        assert_eq!(shown(&format!("{ADD}add({{ 1, 2 }}, <caret>)")), ["a: number, [b: string]"], "commas of a table");
        assert_eq!(shown(&format!("{ADD}add(add(1, 2), <caret>)")), ["a: number, [b: string]"], "and of a call");
        assert_eq!(shown(&format!("{ADD}add(1, add(<caret>))")), ["[a: number], b: string"], "the innermost call");
    }

    #[test]
    fn arguments_left_open_hold_what_is_typed_after_them() {
        assert_eq!(shown(&format!("{ADD}add(<caret>")), ["[a: number], b: string"]);
        assert_eq!(shown(&format!("{ADD}add(1,   <caret>\n")), ["a: number, [b: string]"]);
        assert_eq!(shown(&format!("{ADD}add(1, <caret>\nlocal after = 1")), ["a: number, [b: string]"]);
        assert_eq!(shown(&format!("{ADD}add(1,\nlocal after<caret> = 1")), Vec::<String>::new(), "the next statement");
    }

    #[test]
    fn a_method_takes_its_object_when_it_is_called_with_a_dot() {
        let colon = ["[item: string], count?: number, ...: any", "item: string"];
        assert_eq!(shown("shop:buy(<caret>)"), colon, "with its overload");
        assert_eq!(shown("shop:buy('a', 1, 2, <caret>)"), ["item: string, count?: number, [...: any]", "item: string"]);
        assert_eq!(
            shown("shop.buy(<caret>)"),
            ["[self: any], item: string, count?: number, ...: any", "self: any, item: string"]
        );
        assert_eq!(shown("shop.open(<caret>)"), ["[name: string], ...: number"]);
        assert_eq!(shown("shop:open(<caret>)"), ["[...: number]"], "the object is the first parameter");
    }

    #[test]
    fn the_first_signature_that_takes_the_argument_is_the_active_one() {
        let overloads =
            "---@param a number\n---@overload fun(a: number, b: string, c: boolean)\nlocal function f(a) end\n";
        assert_eq!(shown(&format!("{overloads}f(<caret>)")), ["[a: number]", "a: number, b: string, c: boolean"]);
        assert_eq!(shown(&format!("{overloads}f(1, <caret>)")), ["a: number", "a: number, [b: string], c: boolean"]);
    }

    #[test]
    fn a_string_or_a_table_given_without_parentheses() {
        assert_eq!(shown(&format!("{ADD}add 'te<caret>xt'")), ["[a: number], b: string"]);
        assert_eq!(shown(&format!("{ADD}add {{ <caret> }}")), ["[a: number], b: string"]);
    }

    #[test]
    fn nothing_is_shown_outside_the_arguments_of_a_known_function() {
        for main in [
            format!("{ADD}add<caret>(1)"),
            format!("{ADD}add(1)<caret>"),
            format!("{ADD}local n = <caret>1"),
            "unknown(<caret>)".to_owned(),
            "local function none() end\nnone(<caret>)".to_owned(),
            "local s = ('x'):rep(<caret>)".to_owned(),
        ] {
            assert_eq!(shown(&main), Vec::<String>::new(), "{main}");
        }
    }
}
