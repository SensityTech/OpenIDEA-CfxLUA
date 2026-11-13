use crate::lua::Source;
use crate::refactor::edges::{Anchor, Family, in_order, literal_name};
use crate::statebags::sites::{Element, Kind, analyze, may_hold_bags};

const KNOWN: [Kind; 3] = [Kind::Player, Kind::Entity, Kind::Global];

fn kinds(kind: &Kind) -> &[Kind] {
    if *kind == Kind::Unknown { &KNOWN } else { std::slice::from_ref(kind) }
}

/// The state bag keys a file writes, one anchor for each kind of bag the site may stand for.
pub fn anchors(src: &Source) -> Vec<Anchor> {
    if !may_hold_bags(&src.text) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for site in analyze(src).iter().filter(|site| site.key.is_some()) {
        let range = match site.element {
            Element::Ident => Some((site.start, site.end)),
            Element::Str(_) => literal_name(src, site.start),
            Element::Expr => None,
        };
        let Some((start, end)) = range.filter(|(start, end)| start < end) else { continue };
        let name = &src.text[start..end];
        for kind in kinds(&site.kind) {
            let key = format!("cfx.statebag:{}#{name}", kind.name());
            out.push(Anchor { family: Family::Cfx, key, start, end });
        }
    }
    in_order(out)
}

#[cfg(test)]
#[path = "anchors_tests.rs"]
mod tests;
