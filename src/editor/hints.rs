//! What is written between the characters of a file (`LuaParameterHintsProvider`): the name of the parameter
//! an argument is given to and, where the settings ask for them, the type of a local or of a parameter and
//! what a function returns.

use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::{FileId, Ty, display, plain, psi};

pub const PARAMETER_NAMES: &str = "hints.parameterNames";
pub const LOCAL_TYPES: &str = "hints.localTypes";
pub const PARAMETER_TYPES: &str = "hints.parameterTypes";
pub const RETURN_TYPES: &str = "hints.returnTypes";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    /// `name:`, before an argument.
    Parameter,
    /// `: type`, after a name or the parameters of a function.
    Type,
    /// What a value stands for, after it.
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub at: usize,
    pub label: String,
    pub kind: HintKind,
    /// What the editor shows when the hint is hovered.
    pub tooltip: Option<String>,
}

impl Hint {
    pub fn new(at: usize, label: impl Into<String>, kind: HintKind) -> Self {
        Self { at, label: label.into(), kind, tooltip: None }
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// Which hints are asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub arguments: bool,
    pub locals: bool,
    pub parameters: bool,
    pub returns: bool,
}

impl Wanted {
    pub fn any(self) -> bool {
        self.arguments || self.locals || self.parameters || self.returns
    }
}

/// An argument that does not say what it is: a literal, a table, a function or an operation.
fn is_silent(expr: &Node) -> bool {
    matches!(expr.kind(), N::LiteralExpr | N::BinaryExpr | N::UnaryExpr | N::ClosureExpr | N::TableExpr)
}

/// The names of the parameters the arguments of a call are given to.
fn arguments(cx: &Cx, call: Psi, out: &mut Vec<Hint>) {
    let Some(args) = call.node.child(N::ListArgs) else { return };
    let Some(callee) = psi::prefix(call.node) else { return };
    let given: Vec<Node> = args.exprs().collect();
    let dot = callee.is(N::IndexExpr) && !psi::is_colon(callee);
    // The function is looked up only where an argument can take a hint.
    if !given.iter().any(is_silent) && !(dot && !given.is_empty()) {
        return;
    }
    let ty = cx.with_value(0, || cx.infer(call.at(callee)));
    let Some(function) = ty.each().iter().find_map(Ty::as_function) else { return };
    let colon = psi::is_colon_call(call.node);
    // A method called with a dot is given its object first.
    let object_given = function.is_colon_call() && dot;
    let counted = match (object_given, !function.is_colon_call() && colon) {
        (true, _) => given.len().saturating_sub(1),
        (_, true) => given.len() + 1,
        _ => given.len(),
    };
    let signature = function.perfect_signature(counted);
    for (rank, (param, expr)) in signature.args(None, colon).iter().zip(&given).enumerate() {
        if rank == 0 && object_given || is_silent(expr) {
            out.push(Hint::new(expr.code_range().0, format!("{}:", param.name), HintKind::Parameter));
        }
    }
}

/// The type of a local or of a parameter, after its name.
fn typed(cx: &Cx, def: Psi, out: &mut Vec<Hint>) {
    let ty = plain(&cx.infer(def));
    if !ty.is_invalid() {
        out.push(Hint::new(def.node.code_range().1, format!(": {}", display(&ty)), HintKind::Type));
    }
}

/// What a function returns, after its parameters.
fn returned(cx: &Cx, owner: Psi, out: &mut Vec<Hint>) {
    let close = owner.node.child(N::FuncBody).and_then(|body| body.token(T::RParen));
    let (Some(close), Some(function)) = (close, cx.infer(owner).as_function().cloned()) else { return };
    let ty = plain(&cx.ret(&function.main.ret));
    if !ty.is_invalid() {
        out.push(Hint::new(close.end(), format!(": {}", display(&ty)), HintKind::Type));
    }
}

/// The hints of what is written between `from` and `to` in `src`, the file `file`.
pub fn hints(cx: &Cx, file: FileId, src: &Source, (from, to): (usize, usize), wanted: Wanted) -> Vec<Hint> {
    let mut out = Vec::new();
    if !wanted.any() {
        return out;
    }
    // What is written before or after the range is not entered.
    let mut open = vec![src.root()];
    while let Some(parent) = open.pop() {
        let inside: Vec<Node> =
            parent.children().filter(|node| node.range().1 >= from && node.range().0 <= to).collect();
        for node in &inside {
            let psi = Psi { file, src, node: *node };
            match node.kind() {
                N::CallExpr if wanted.arguments => arguments(cx, psi, &mut out),
                N::ParamNameDef if wanted.parameters => typed(cx, psi, &mut out),
                N::NameDef if wanted.locals => typed(cx, psi, &mut out),
                _ if wanted.returns && psi::is_function(*node) => returned(cx, psi, &mut out),
                _ => {}
            }
        }
        open.extend(inside);
    }
    out.retain(|hint| from <= hint.at && hint.at <= to);
    out.sort_by_key(|hint| hint.at);
    out
}

#[cfg(test)]
mod tests {
    use super::{HintKind, Wanted};
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST};
    use crate::feature::Feature;
    use crate::settings::Settings;
    use crate::testing::Fixture;
    use serde_json::Value;

    const SHOP: (&str, &str) = (
        "shop/shared/shop.lua",
        "---@class Shop\nShop = {}\n\n---@param item string\n---@param count? number\nfunction Shop:buy(item, count) end\n\n\
         ---@param name string\n---@param ... number\nfunction Shop.open(name, ...) end\n\n---@type Shop\nshop = nil\n",
    );
    const OPEN: &str = "---@param door number\n---@param fast boolean\nlocal function open(door, fast) end\n";

    /// The text of `main` with each hint written where it stands, between `«` and `»`.
    fn hinted(main: &str, wanted: Wanted) -> String {
        let fx = Fixture::new(&[MANIFEST, SHOP, (MAIN, main)]);
        let settings = Settings::defaults()
            .with("hints.parameterNames", Value::from(wanted.arguments))
            .with("hints.localTypes", Value::from(wanted.locals))
            .with("hints.parameterTypes", Value::from(wanted.parameters))
            .with("hints.returnTypes", Value::from(wanted.returns));
        let fx = fx.with_settings(settings);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let doc = fx.doc(MAIN);
        let mut text = main.to_owned();
        let mut hints = feature.inlay_hints(&fx.cx(&doc), 0, main.len());
        hints.reverse();
        for hint in hints {
            assert_eq!(hint.kind == HintKind::Parameter, hint.label.ends_with(':'), "{}", hint.label);
            text.insert_str(hint.at, &format!("«{}»", hint.label));
        }
        text
    }

    const ARGUMENTS: Wanted = Wanted { arguments: true, locals: false, parameters: false, returns: false };
    const TYPES: Wanted = Wanted { arguments: false, locals: true, parameters: true, returns: true };

    /// What is written after the declaration of `open`.
    fn named(calls: &str) -> String {
        hinted(&format!("{OPEN}{calls}"), ARGUMENTS)[OPEN.len()..].to_owned()
    }

    #[test]
    fn an_argument_that_does_not_say_what_it_is_gets_the_name_of_its_parameter() {
        assert_eq!(named("open(1, true)"), "open(«door:»1, «fast:»true)");
        assert_eq!(named("local d, f = 1, true\nopen(d, f)"), "local d, f = 1, true\nopen(d, f)", "names say it");
        assert_eq!(named("open({}, function() end)"), "open(«door:»{}, «fast:»function() end)");
        assert_eq!(named("open(1 + 2, not x)"), "open(«door:»1 + 2, «fast:»not x)");
        assert_eq!(named("open(1, true, 3)"), "open(«door:»1, «fast:»true, 3)", "one argument too many");
        assert_eq!(named("open(open(1, 2), 3)"), "open(open(«door:»1, «fast:»2), «fast:»3)", "a call inside a call");
        assert_eq!(named("unknown(1, 2)\nopen 'x'"), "unknown(1, 2)\nopen 'x'");
    }

    #[test]
    fn a_method_called_with_a_dot_shows_where_its_object_goes() {
        assert_eq!(hinted("shop:buy('bread', 2)", ARGUMENTS), "shop:buy(«item:»'bread', «count:»2)");
        assert_eq!(hinted("shop.buy(shop, 'bread')", ARGUMENTS), "shop.buy(«self:»shop, «item:»'bread')");
        assert_eq!(
            hinted("Shop.open('north', 1, 2)", ARGUMENTS),
            "Shop.open(«name:»'north', 1, 2)",
            "no extra argument"
        );
    }

    #[test]
    fn types_are_shown_where_the_settings_ask_for_them() {
        let text = "local n, t = 1, {}\n---@param a string\nlocal function two(a, b) return 2 end\nfor i = 1, 2 do end";
        assert_eq!(
            hinted(text, TYPES),
            "local n«: number», t«: table» = 1, {}\n---@param a string\nlocal function two(a«: string», b)«: number» \
             return 2 end\nfor i«: number» = 1, 2 do end"
        );
        assert_eq!(hinted(text, ARGUMENTS), text, "none by default");
        let nothing = Wanted { arguments: false, ..ARGUMENTS };
        assert_eq!(hinted("local function f(a) end\nf(1)", nothing), "local function f(a) end\nf(1)");
    }

    #[test]
    fn only_what_is_written_in_the_range_is_hinted() {
        let fx =
            Fixture::new(&[MANIFEST, SHOP, (MAIN, &format!("{OPEN}open(1, true)\nopen(2, false)\nopen(3, true)"))]);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let doc = fx.doc(MAIN);
        let second = doc.text().find("open(2").expect("the second call");
        let hints = feature.inlay_hints(&fx.cx(&doc), second, second + 14);
        let labels: Vec<(usize, &str)> = hints.iter().map(|hint| (hint.at - second, hint.label.as_str())).collect();
        assert_eq!(labels, [(5, "door:"), (8, "fast:")]);
    }
}
