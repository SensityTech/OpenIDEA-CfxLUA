use super::expr::{Coord, Form, analyze, name_context, parse_number};
use super::zones::ZoneMap;
use crate::feature::{Ctx, Problem};
use crate::lua::Source;
use crate::perf::ast::{Ast, ExprKind, Node, StatKind};
use crate::project::{Game, describe_games};
use openidea_plugin::Severity;
use std::collections::HashMap;

pub const ZERO_Z: &str = "CfxCoordsZeroZ";
pub const OUT_OF_BOUNDS: &str = "CfxCoordsOutOfBounds";
pub const DUPLICATE: &str = "CfxCoordsDuplicate";
pub const ZONE: &str = "CfxCoordsZone";

const WEAK: Severity = Severity::Info;

const NON_POSITION_WORDS: &[&str] = &[
    "rot",
    "offset",
    "scale",
    "size",
    "dimension",
    "color",
    "colour",
    "rgb",
    "vel",
    "force",
    "dir",
    "normal",
    "speed",
];

pub fn literal_position(ast: &Ast, src: &Source, e: usize) -> Option<Coord> {
    let c = analyze(ast, src, e)?;
    if c.form == Form::Array {
        return None;
    }
    let name = name_context(ast, src, e).map(|n| n.to_ascii_lowercase());
    if name.is_some_and(|n| NON_POSITION_WORDS.iter().any(|w| n.contains(w))) {
        return None;
    }
    Some(c)
}

fn is_colour_like(texts: &[Option<String>]) -> bool {
    texts.iter().all(|t| {
        t.as_ref().is_some_and(|t| {
            !t.contains(['.', 'e', 'E']) && parse_number(t).is_some_and(|v| (0.0..=255.0).contains(&v))
        })
    })
}

fn format_number(v: f64) -> String {
    if v == v.round() && v.abs() < 1e15 { format!("{v:.0}") } else { v.to_string() }
}

pub fn games_of(cx: &Ctx) -> Vec<Game> {
    let auto = cx.settings.string(crate::settings::keys::GAME, "auto") == "auto";
    let resource = cx.ws.resource_of(&cx.doc.path).and_then(|r| r.games.clone()).filter(|g| !g.is_empty());
    match resource {
        Some(g) if auto => g,
        _ => crate::natives::project_games(cx.settings, &cx.ws.detected_games()),
    }
}

fn bounds(game: Game) -> [(f64, f64); 3] {
    match game {
        Game::Gta5 => [(-8000.0, 8000.0), (-8000.0, 10000.0), (-1000.0, 3000.0)],
        Game::Rdr3 => [(-8000.0, 6000.0), (-8000.0, 5000.0), (-1000.0, 3000.0)],
    }
}

pub fn problems(cx: &Ctx, ast: &Ast, config_roots: &[String], zones: Option<&ZoneMap>) -> Vec<Problem> {
    let src = &cx.doc.src;
    let games = games_of(cx);
    let names = describe_games(&games);
    let zones = zones.filter(|_| games.contains(&Game::Gta5));
    let mut out = Vec::new();
    for e in 0..ast.exprs.len() {
        if !matches!(ast.exprs[e].kind, ExprKind::Call { .. } | ExprKind::Table { .. }) {
            continue;
        }
        let Some(c) = literal_position(ast, src, e) else { continue };
        let texts = c.number_texts(ast, src);
        let values: Vec<Option<f64>> = texts.iter().map(|t| t.as_deref().and_then(parse_number)).collect();
        if let (Some(x), Some(y), Some(z)) = (values[0], values[1], values[2])
            && z == 0.0
            && x.abs().max(y.abs()) >= 100.0
            && !is_colour_like(&texts[..3])
        {
            let (s, t) = ast.span(Node::Expr(c.components[2]));
            out.push(Problem::new(ZERO_Z, WEAK, s, t, "z is 0.0: was the height forgotten?"));
        }
        if !games.is_empty() {
            for (axis, value) in values.iter().take(3).enumerate() {
                let Some(v) = *value else { continue };
                let min = games.iter().map(|&g| bounds(g)[axis].0).fold(f64::INFINITY, f64::min);
                let max = games.iter().map(|&g| bounds(g)[axis].1).fold(f64::NEG_INFINITY, f64::max);
                if (min..=max).contains(&v) {
                    continue;
                }
                let (s, t) = ast.span(Node::Expr(c.components[axis]));
                let message = format!(
                    "{} is outside of the {names} map ({} to {})",
                    ["x", "y", "z"][axis],
                    format_number(min),
                    format_number(max)
                );
                out.push(Problem::new(OUT_OF_BOUNDS, WEAK, s, t, message));
                break;
            }
        }
        if let Some(zones) = zones
            && let (Some(x), Some(y)) = (values[0], values[1])
            && let Some(zone) = zones.find(x, y, values[2])
        {
            let (s, t) = ast.span(Node::Expr(e));
            let message = format!(
                "≈ {} — Approximate GTA V zone: {} ({}), from bounding boxes",
                zone.display, zone.display, zone.name
            );
            out.push(Problem::new(ZONE, Severity::Hint, s, t, message));
        }
    }
    if !config_roots.is_empty() {
        duplicates(ast, src, config_roots, &mut out);
    }
    out
}

fn duplicates(ast: &Ast, src: &Source, roots: &[String], out: &mut Vec<Problem>) {
    let is_root = |name: &str| roots.iter().any(|r| r.eq_ignore_ascii_case(name));
    let table = |e: usize| matches!(ast.exprs[e].kind, ExprKind::Table { .. }).then_some(e);
    for s in &ast.stats {
        match &s.kind {
            StatKind::Assign { targets, values } => {
                for (&target, &value) in targets.iter().zip(values) {
                    let Some(t) = table(value) else { continue };
                    let Some(root) = ast.root_name(target).and_then(|r| ast.name_of(r)) else { continue };
                    if is_root(root) {
                        config(ast, src, t, ast.name_of(target).is_some(), out);
                    }
                }
            }
            StatKind::Local { names, values } => {
                for (&d, &value) in names.iter().zip(values) {
                    if let Some(t) = table(value)
                        && is_root(&ast.decls[d].name)
                    {
                        config(ast, src, t, true, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn config(ast: &Ast, src: &Source, table: usize, is_root: bool, out: &mut Vec<Problem>) {
    if !is_root {
        return scope(ast, src, table, out);
    }
    let ExprKind::Table { fields } = &ast.exprs[table].kind else { return };
    for v in fields.iter().filter_map(|f| f.value) {
        if matches!(ast.exprs[v].kind, ExprKind::Table { .. }) {
            scope(ast, src, v, out);
        }
    }
}

fn scope(ast: &Ast, src: &Source, table: usize, out: &mut Vec<Problem>) {
    let mut seen: HashMap<(String, Vec<u64>), usize> = HashMap::new();
    ast.walk_all(Node::Expr(table), &mut |n| {
        let Node::Expr(e) = n else { return };
        if e == table || !matches!(ast.exprs[e].kind, ExprKind::Call { .. } | ExprKind::Table { .. }) {
            return;
        }
        let Some(c) = analyze(ast, src, e).filter(|c| c.form != Form::Array) else { return };
        let Some(values) = c.values(ast, src) else { return };
        let role = match ast.parent(n) {
            Some(Node::Expr(t)) if matches!(ast.exprs[t].kind, ExprKind::Table { .. }) => {
                name_context(ast, src, e).unwrap_or_else(|| "[]".into())
            }
            _ => "[]".into(),
        };
        let key = (role, values.iter().map(|v| v.to_bits()).collect());
        match seen.get(&key) {
            Some(&first) => {
                let line = src.lines.line_of(ast.exprs[first].start) + 1;
                let (s, t) = ast.span(n);
                out.push(Problem::new(DUPLICATE, WEAK, s, t, format!("Same coordinates as line {line}")));
            }
            None => {
                seen.insert(key, e);
            }
        }
    });
}
