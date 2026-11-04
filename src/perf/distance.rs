//! `GetDistanceBetweenCoords(x1, y1, z1, x2, y2, z2, useZ)` / `Vdist(…)` and
//! their vector math equivalent `#(a - b)`: the fix of the distance check
//! and the standalone code action.

use super::ast::{Ast, ExprKind, Node};
use super::classify::Perf;
use super::spec::Work;
use crate::lua::Source;
use crate::natives::{NativeSet, Natives};

/// Distance natives replaced by vector math (`Vdist2` is a squared distance).
pub const NATIVES: &[&str] = &["GetDistanceBetweenCoords", "Vdist"];

/// `#(a - b)` computing the same distance as `call`; `None` when it can't be
/// written safely (`useZ` not a literal, unexpected arguments).
pub fn replacement(p: &Perf, call: usize) -> Option<String> {
    let ast = &p.ast;
    let name = ast.callee_name(call)?;
    let args = ast.call_args(call);
    let use_z = match (name, args.len()) {
        ("Vdist", 6) => true,
        ("GetDistanceBetweenCoords", 6) => false,
        ("GetDistanceBetweenCoords", 7) => literal_bool(ast, p.src, args[6])?,
        _ => return None,
    };
    if args.iter().any(|&a| matches!(ast.exprs[a].kind, ExprKind::Vararg)) {
        return None;
    }
    let a = operand(p, &args[0..3], use_z)?;
    let b = operand(p, &args[3..6], use_z)?;
    let text = format!("#({a} - {b})");
    // `^` binds tighter than `#`.
    let left_of_pow = matches!(ast.parent(Node::Expr(call)), Some(Node::Expr(e)) if matches!(ast.exprs[e].kind, ExprKind::Binary { op: "^", lhs, .. } if lhs == call));
    Some(if left_of_pow { format!("({text})") } else { text })
}

fn literal_bool(ast: &Ast, src: &Source, e: usize) -> Option<bool> {
    let e = ast.unwrap(e);
    match ast.exprs[e].kind {
        ExprKind::True => Some(true),
        ExprKind::False | ExprKind::Nil => Some(false),
        // Natives receive numbers as integers: 0 is false.
        ExprKind::Number => super::classify::lua_number(ast.text(src, Node::Expr(e))).map(|v| v != 0.0),
        _ => None,
    }
}

/// `v` (`v.xy` in 2D, `v.xyz` for a vector4) when the coordinates are `v.x, v.y, v.z` of a vector, else `vector3(x, y, z)` / `vector2(x, y)`.
fn operand(p: &Perf, coords: &[usize], use_z: bool) -> Option<String> {
    let ast = &p.ast;
    let text = |e: usize| ast.text(p.src, Node::Expr(e));
    if let Some((base, class)) = common_vector(p, coords) {
        return Some(match (use_z, class) {
            (false, _) => format!("{base}.xy"),
            (true, "vector4") => format!("{base}.xyz"),
            _ => base,
        });
    }
    if use_z {
        Some(format!("vector3({}, {}, {})", text(coords[0]), text(coords[1]), text(coords[2])))
    } else {
        // The z coordinate disappears: only when evaluating it has no effect.
        droppable(p, coords[2]).then(|| format!("vector2({}, {})", text(coords[0]), text(coords[1])))
    }
}

fn droppable(p: &Perf, e: usize) -> bool {
    p.ast.calls(Node::Expr(e), &|_| false).into_iter().all(|c| matches!(p.info(c).work(), Work::Pure | Work::Query))
}

fn common_vector(p: &Perf, coords: &[usize]) -> Option<(String, &'static str)> {
    let ast = &p.ast;
    let mut prefix = None;
    for (&e, axis) in coords.iter().zip(["x", "y", "z"]) {
        let ExprKind::Field { obj, name, safe: false } = &ast.exprs[ast.unwrap(e)].kind else { return None };
        let text = ast.text(p.src, Node::Expr(*obj));
        if name != axis || prefix.is_some_and(|(_, t)| t != text) {
            return None;
        }
        prefix = Some((*obj, text));
    }
    let (obj, text) = prefix?;
    // Evaluating `v` once instead of three times must not change anything.
    if !droppable(p, obj) {
        return None;
    }
    Some((text.to_owned(), created_vector(ast, p.cx.natives, p.sets(), obj)?))
}

/// `vector3` / `vector4` when `e` is created by a vector constructor or a
/// native returning a vector, directly or through a local never assigned again.
pub fn created_vector(ast: &Ast, natives: &Natives, sets: &[NativeSet], e: usize) -> Option<&'static str> {
    let e = ast.unwrap(e);
    let value = if matches!(ast.exprs[e].kind, ExprKind::Name { .. }) { ast.constant_initializer(e)? } else { e };
    let call = ast.unwrap(value);
    let ExprKind::Call { callee, method: None, .. } = ast.exprs[call].kind else { return None };
    let ExprKind::Name { name, decl: None } = &ast.exprs[callee].kind else { return None };
    match name.as_str() {
        "vector3" | "vec3" => Some("vector3"),
        "vector4" | "vec4" => Some("vector4"),
        _ => match natives.find(name, sets)?.returns.first()?.lua_type.as_str() {
            "vector3" => Some("vector3"),
            "vector4" => Some("vector4"),
            _ => None,
        },
    }
}
