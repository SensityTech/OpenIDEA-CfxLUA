use crate::editor::hints::{Hint, HintKind};
use crate::feature::Ctx;
use crate::perf::ast::{self, ExprKind, Node};
use crate::values::decode::Decoded;
use crate::values::sites::Scope;
use crate::values::{ValuesFeature, is_annotated, keys};
use std::collections::HashMap;

/// The decoded numbers of the file that end between `from` and `to`, each with its span.
pub(crate) fn decoded_values(values: &ValuesFeature, cx: &Ctx, from: usize, to: usize) -> Vec<(usize, usize, Decoded)> {
    if !ValuesFeature::applies(cx) || !cx.settings.bool(keys::HINTS, true) {
        return Vec::new();
    }
    let tree = ast::parse(&cx.doc.src);
    let scope = Scope::new(cx, &tree);
    let mut enabled: HashMap<&str, bool> = HashMap::new();
    let mut out = Vec::new();
    for (e, expr) in tree.exprs.iter().enumerate() {
        if !matches!(expr.kind, ExprKind::Number) || expr.stop < from || expr.stop > to {
            continue;
        }
        let Some(site) = scope.site_at(e) else { continue };
        let key = site.kind.setting();
        if !*enabled.entry(key).or_insert_with(|| cx.settings.bool(key, true)) {
            continue;
        }
        let Some(decoded) = values.decode(&site) else { continue };
        let (start, end) = tree.span(Node::Expr(site.expr));
        if !is_annotated(&cx.doc.src, end, &decoded.name) {
            out.push((start, end, decoded));
        }
    }
    out
}

pub fn hints(values: &ValuesFeature, cx: &Ctx, from: usize, to: usize) -> Vec<Hint> {
    let mut out: Vec<Hint> = decoded_values(values, cx, from, to)
        .into_iter()
        .map(|(_, end, decoded)| {
            let tooltip = decoded.tooltip();
            Hint::new(end, decoded.label, HintKind::Value).tooltip(tooltip)
        })
        .collect();
    out.sort_by_key(|hint| hint.at);
    out
}

#[cfg(test)]
#[path = "hints_tests.rs"]
mod tests;
