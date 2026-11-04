use super::ast::{Ast, DeclKind, ExprKind, Node, StatKind};
use super::edits::{self, Edit};
use super::loops::{LoopFacts, assigned_names, is_wait_loop};
use super::spec::{Work, Yield};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stability {
    Frame,
    Ped,
    State,
}

const CACHEABLE: &[(&str, &str, Stability)] = &[
    ("PlayerPedId", "ped", Stability::Ped),
    ("PlayerId", "playerId", Stability::Frame),
    ("GetPlayerServerId", "serverId", Stability::Frame),
    ("GetGameTimer", "now", Stability::Frame),
    ("GetHashKey", "hash", Stability::Frame),
    ("GetCurrentResourceName", "resourceName", Stability::Frame),
    ("GetEntityCoords", "coords", Stability::State),
    ("GetEntityHeading", "heading", Stability::State),
    ("GetEntityModel", "model", Stability::State),
    ("GetEntityHealth", "health", Stability::State),
    ("GetEntitySpeed", "speed", Stability::State),
    ("GetVehiclePedIsIn", "vehicle", Stability::State),
    ("IsPedInAnyVehicle", "inVehicle", Stability::State),
    ("GetSelectedPedWeapon", "weapon", Stability::State),
];

const MODEL_CHANGES: &[&str] = &["SetPlayerModel", "ChangePlayerPed", "NetworkResurrectLocalPlayer", "ResurrectPed"];

pub const PLAYER_PED_ID: &str = "PlayerPedId()";

pub struct Group {
    pub key: String,
    pub local: &'static str,
    pub stability: Stability,
    pub calls: Vec<usize>,
}

pub struct Cache {
    pub name: String,
    pub reuse: bool,
    pub edits: Vec<Edit>,
}

struct Shape {
    key: String,
    local: &'static str,
    stability: Stability,
}

fn sequence(ast: &Ast, n: Node, out: &mut Vec<Option<usize>>) {
    ast.children(n, |c| {
        if is_wait_loop(ast, c) {
            out.push(None);
        } else if !ast.is_function(c) {
            sequence(ast, c, out);
        }
    });
    if let Node::Expr(e) = n
        && matches!(ast.exprs[e].kind, ExprKind::Call { .. })
    {
        out.push(Some(e));
    }
}

pub fn groups(lp: &LoopFacts, min: usize) -> Vec<Group> {
    let ast = &lp.p.ast;
    let assigned = assigned_names(ast, Node::Block(lp.body));
    let mut items = Vec::new();
    sequence(ast, Node::Block(lp.body), &mut items);
    let mut open: Vec<Group> = Vec::new();
    let mut done: Vec<Group> = Vec::new();
    let close = |open: &mut Vec<Group>, done: &mut Vec<Group>, pred: &dyn Fn(&Group) -> bool| {
        let (closed, kept): (Vec<Group>, Vec<Group>) = open.drain(..).partition(|g| pred(g));
        done.extend(closed);
        *open = kept;
    };
    for item in items {
        let Some(call) = item else {
            close(&mut open, &mut done, &|_| true);
            continue;
        };
        let info = lp.p.info(call);
        if info.yields() != Yield::None {
            close(&mut open, &mut done, &|_| true);
            continue;
        }
        if info.path.as_deref().is_some_and(|p| MODEL_CHANGES.contains(&p)) {
            close(&mut open, &mut done, &|g| g.stability != Stability::Frame);
        }
        if matches!(info.work(), Work::Action | Work::Effect | Work::Unknown) {
            close(&mut open, &mut done, &|g| g.stability == Stability::State);
        }
        let Some(shape) = shape(lp, call, &assigned) else { continue };
        match open.iter_mut().find(|g| g.key == shape.key) {
            Some(g) => g.calls.push(call),
            None => {
                open.push(Group { key: shape.key, local: shape.local, stability: shape.stability, calls: vec![call] })
            }
        }
    }
    close(&mut open, &mut done, &|_| true);
    done.retain(|g| max_on_one_path(ast, g) >= min);
    done.sort_by_key(|g| ast.exprs[g.calls[0]].start);
    done
}

fn max_on_one_path(ast: &Ast, g: &Group) -> usize {
    g.calls.iter().map(|&a| 1 + g.calls.iter().filter(|&&b| b != a && !exclusive(ast, a, b)).count()).max().unwrap_or(0)
}

fn exclusive(ast: &Ast, a: usize, b: usize) -> bool {
    let Some(common @ Node::Stat(s)) = ast.common(Node::Expr(a), Node::Expr(b)) else { return false };
    if !matches!(ast.stats[s].kind, StatKind::If { .. }) {
        return false;
    }
    let first = if ast.exprs[a].start <= ast.exprs[b].start { a } else { b };
    let mut part = Node::Expr(first);
    while let Some(parent) = ast.parent(part) {
        if parent == common {
            return matches!(part, Node::Block(_));
        }
        part = parent;
    }
    false
}

fn shape(lp: &LoopFacts, call: usize, assigned: &HashSet<String>) -> Option<Shape> {
    let p = lp.p;
    let ast = &p.ast;
    let ExprKind::Call { callee, method: None, args } = &ast.exprs[call].kind else { return None };
    let ExprKind::Name { name, decl } = &ast.exprs[*callee].kind else { return None };
    if !p.may_run_on_client() && !matches!(name.as_str(), "GetGameTimer" | "GetHashKey" | "GetCurrentResourceName") {
        return None;
    }
    if is_player_ped_minus_one(ast, p.src, call) {
        return decl.is_none().then(|| Shape { key: PLAYER_PED_ID.into(), local: "ped", stability: Stability::Ped });
    }
    let &(_, local, mut stability) = CACHEABLE.iter().find(|(n, _, _)| n == name)?;
    if !matches!(p.info(call).work(), Work::Query | Work::Pure | Work::PerFrame) {
        return None;
    }
    let mut parts = Vec::new();
    for &arg in args {
        let e = ast.unwrap(arg);
        let part = match &ast.exprs[e].kind {
            ExprKind::Number
            | ExprKind::Str
            | ExprKind::Hash
            | ExprKind::Nil
            | ExprKind::True
            | ExprKind::False
            | ExprKind::Vararg => ast.text(p.src, Node::Expr(e)).to_owned(),
            ExprKind::Unary { op: "-", operand } if is_literal(ast, ast.unwrap(*operand)) => {
                ast.text(p.src, Node::Expr(e)).chars().filter(|c| !c.is_whitespace()).collect()
            }
            ExprKind::Call { .. } => {
                let inner = shape(lp, e, assigned)?;
                stability = stability.max(inner.stability);
                inner.key
            }
            ExprKind::Name { name, decl } => {
                if assigned.contains(name) {
                    return None;
                }
                match decl {
                    Some(d) => format!("{name}@{}", p.src.toks[ast.decls[*d].tok].start),
                    None => name.clone(),
                }
            }
            _ => return None,
        };
        parts.push(part);
    }
    Some(Shape { key: format!("{name}({})", parts.join(",")), local, stability })
}

fn is_literal(ast: &Ast, e: usize) -> bool {
    matches!(
        ast.exprs[e].kind,
        ExprKind::Number
            | ExprKind::Str
            | ExprKind::Hash
            | ExprKind::Nil
            | ExprKind::True
            | ExprKind::False
            | ExprKind::Vararg
    )
}

pub fn is_player_ped_minus_one(ast: &Ast, src: &crate::lua::Source, call: usize) -> bool {
    if ast.callee_name(call) != Some("GetPlayerPed") {
        return false;
    }
    let [arg] = ast.call_args(call) else { return false };
    let text: String = ast.text(src, Node::Expr(ast.unwrap(*arg))).chars().filter(|c| !c.is_whitespace()).collect();
    text == "-1"
}

pub fn display(ast: &Ast, src: &crate::lua::Source, g: &Group) -> String {
    if g.key == PLAYER_PED_ID {
        return PLAYER_PED_ID.into();
    }
    ast.text(src, Node::Expr(g.calls[0])).split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn cache_edits(lp: &LoopFacts, g: &Group) -> Option<Cache> {
    let p = lp.p;
    let ast = &p.ast;
    let src = p.src;
    let first = g.calls[0];
    let assigned = assigned_names(ast, Node::Block(lp.body));
    let span = |c: usize| (ast.exprs[c].start, ast.exprs[c].stop);

    if let Some(Node::Stat(local)) = ast.parent(Node::Expr(first))
        && let StatKind::Local { names, values } = &ast.stats[local].kind
        && let ([d], [v]) = (names.as_slice(), values.as_slice())
        && *v == first
        && !assigned.contains(&ast.decls[*d].name)
    {
        let scope = Node::Block(ast.stats[local].parent);
        let end = ast.stats[local].stop;
        let others = &g.calls[1..];
        if others.iter().all(|&c| ast.is_ancestor(scope, Node::Expr(c), true) && ast.exprs[c].start > end) {
            let name = ast.decls[*d].name.clone();
            let edits = others.iter().map(|&c| (span(c).0, span(c).1, name.clone())).collect();
            return Some(Cache { name, reuse: true, edits });
        }
    }

    let common = g.calls[1..].iter().try_fold(Node::Expr(first), |acc, &c| ast.common(acc, Node::Expr(c)))?;
    let mut block = common;
    while !matches!(block, Node::Block(_)) {
        block = ast.parent(block)?;
    }
    if !ast.is_ancestor(Node::Block(lp.body), block, false) {
        return None;
    }
    let mut anchor = Node::Expr(first);
    while ast.parent(anchor) != Some(block) {
        anchor = ast.parent(anchor)?;
    }
    let Node::Stat(anchor) = anchor else { return None };

    // Nothing between the new local and the first call may change the value.
    let (from, to) = (ast.stats[anchor].start, ast.exprs[first].start);
    for c in ast.calls(Node::Stat(anchor), &|_| false) {
        let start = ast.exprs[c].start;
        if start < from || start >= to || ast.is_ancestor(Node::Expr(first), Node::Expr(c), false) {
            continue;
        }
        let info = p.info(c);
        if info.yields() != Yield::None {
            return None;
        }
        if g.stability == Stability::State && matches!(info.work(), Work::Action | Work::Effect | Work::Unknown) {
            return None;
        }
    }
    // Locals used as arguments must be visible before the new local.
    let mut visible = true;
    ast.walk_all(Node::Expr(first), &mut |n| {
        let Node::Expr(e) = n else { return };
        let Some(d) = ast.decl_of(e) else { return };
        let is_callee =
            matches!(ast.parent(n), Some(Node::Expr(c)) if matches!(ast.exprs[c].kind, ExprKind::Call { callee, .. } if callee == e));
        if !is_callee && !visible_before(ast, d, anchor) {
            visible = false;
        }
    });
    if !visible {
        return None;
    }
    let name = edits::fresh_name(ast, lp.stat, g.local);
    let init =
        if g.key == PLAYER_PED_ID { PLAYER_PED_ID.to_owned() } else { ast.text(src, Node::Expr(first)).to_owned() };
    let mut out = vec![edits::insert_before(src, ast.stats[anchor].start, &format!("local {name} = {init}"))];
    out.extend(g.calls.iter().map(|&c| (span(c).0, span(c).1, name.clone())));
    Some(Cache { name, reuse: false, edits: out })
}

fn visible_before(ast: &Ast, d: usize, anchor: usize) -> bool {
    let a = Node::Stat(anchor);
    match ast.decls[d].kind {
        DeclKind::ForVar { stat } => ast.is_ancestor(Node::Stat(stat), a, true),
        DeclKind::Param { func } => ast.is_ancestor(Node::Expr(func), a, true),
        DeclKind::Local { stat, .. } | DeclKind::LocalFunction { stat } => {
            ast.is_ancestor(Node::Block(ast.stats[stat].parent), a, true)
                && ast.stats[stat].stop <= ast.stats[anchor].start
        }
    }
}
