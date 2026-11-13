use crate::docs::target::{HASH_DESCRIPTION, HASH_INVALID, hash_value};
use crate::editor::hover::Hover;
use crate::events::md;
use crate::feature::Ctx;
use crate::lua::Source;
use crate::lua::joaat::hex;
use crate::lua::lexer::{Tok, Token};
use crate::perf::ast::{self, Node};
use crate::values::ValuesFeature;
use crate::values::decode::argument;
use crate::values::sites::Scope;

/// The page of the number passed to a native, or of the hash literal, under `at` or ending there.
pub fn hover(values: &ValuesFeature, cx: &Ctx, at: usize) -> Option<Hover> {
    if !ValuesFeature::applies(cx) {
        return None;
    }
    let src = &cx.doc.src;
    // `tok_at(x)` holds the byte before `x`: the token under `at`, then the one that ends there.
    let tok = [at + 1, at]
        .into_iter()
        .filter_map(|x| src.tok_at(x))
        .map(|i| src.toks[i])
        .find(|t| matches!(t.kind, Tok::Number | Tok::Hash { .. }))?;
    match tok.kind {
        Tok::Hash { closed } => Some(hash(src, tok, closed)),
        _ => number(values, cx, tok),
    }
}

fn number(values: &ValuesFeature, cx: &Ctx, tok: Token) -> Option<Hover> {
    let tree = ast::parse(&cx.doc.src);
    let scope = Scope::new(cx, &tree);
    let site = ValuesFeature::site_near(&scope, tok.start + 1)?;
    let decoded = values.decode(&site)?;
    let node = Node::Expr(site.expr);
    let argument = site.call.as_ref().zip(site.index).map(|(call, i)| argument(i, call.param(i), &call.name));
    let markdown = decoded.markdown(tree.text(&cx.doc.src, node), argument.as_deref());
    Some(Hover { range: tree.span(node), markdown })
}

fn hash(src: &Source, tok: Token, closed: bool) -> Hover {
    let (start, end) = tok.content_range(&src.text);
    let value = if closed { hash_value(&src.text[start..end]) } else { None };
    Hover { range: (tok.start, tok.end), markdown: hash_page(tok.text(&src.text), value) }
}

fn hash_page(literal: &str, value: Option<i32>) -> String {
    let mut out = format!("hash **{}**\n\n", md(literal));
    match value {
        Some(v) => out.push_str(&format!(
            "{HASH_DESCRIPTION}\n\n**Signed (int32):** `{v}`  \n**Unsigned (uint32):** `{}`  \n**Hex:** `{}`",
            v as u32,
            hex(v)
        )),
        None => out.push_str(HASH_INVALID),
    }
    out
}

#[cfg(test)]
#[path = "hover_tests.rs"]
mod tests;
