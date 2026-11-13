use crate::coords::checks::{games_of, literal_position};
use crate::coords::expr::parse_number;
use crate::coords::zones::{Zone, ZoneMap};
use crate::editor::hints::{Hint, HintKind};
use crate::feature::Ctx;
use crate::lua::Source;
use crate::perf::ast::{Ast, ExprKind};
use crate::project::Game;

/// The zone of the literal position written by the call or table `e`.
pub(crate) fn zone_of<'z>(ast: &Ast, src: &Source, zones: &'z ZoneMap, e: usize) -> Option<&'z Zone> {
    let c = literal_position(ast, src, e)?;
    let values: Vec<Option<f64>> =
        c.number_texts(ast, src).into_iter().map(|t| t.as_deref().and_then(parse_number)).collect();
    let component = |i: usize| values.get(i).copied().flatten();
    zones.find(component(0)?, component(1)?, component(2))
}

pub fn hints(cx: &Ctx, ast: &Ast, zones: Option<&ZoneMap>, from: usize, to: usize) -> Vec<Hint> {
    let Some(zones) = zones.filter(|_| games_of(cx).contains(&Game::Gta5)) else { return Vec::new() };
    let mut out = Vec::new();
    for (e, expr) in ast.exprs.iter().enumerate() {
        if !matches!(expr.kind, ExprKind::Call { .. } | ExprKind::Table { .. }) || expr.stop < from || expr.stop > to {
            continue;
        }
        let Some(zone) = zone_of(ast, &cx.doc.src, zones, e) else { continue };
        let tooltip = format!("Approximate GTA V zone: {} ({}), from bounding boxes", zone.display, zone.name);
        out.push(Hint::new(expr.stop, format!("≈ {}", zone.display), HintKind::Value).tooltip(tooltip));
    }
    out.sort_by_key(|hint| hint.at);
    out
}

#[cfg(test)]
#[path = "hints_tests.rs"]
mod tests;
