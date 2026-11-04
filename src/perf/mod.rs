//! Performance linter: per-frame loops, expensive calls, distance checks.
//!
//! Six checks on the `while` / `repeat` loops and calls of a script (polling
//! loops waiting one frame, endless loops that never wait, natives called
//! repeatedly in a frame, distance natives, per-frame spam, registrations
//! repeated in handlers or loops), each with its quick fixes; a gutter marker
//! with the cost class of every long-running loop; and the "replace a
//! distance native with vector math" code action. The statement structure
//! comes from [`ast`], shared with the values and coordinates features.

pub mod ast;
pub mod checks;
pub mod classify;
pub mod distance;
pub mod edits;
pub mod loops;
pub mod repeated;
pub mod spec;

use crate::feature::{Ctx, Feature, Fix, Intention, Problem};
use crate::lua::Source;
use crate::project::{FileKind, Side};
use crate::workspace::Workspace;
use ast::{ExprKind, Node};
use classify::{FileFacts, Perf, ProjectIndex};
use loops::Cost;
use openidea_plugin::providers::LineMarker;
use std::cell::RefCell;

/// Settings keys (`perf.*`).
pub mod keys {
    pub const IDLE_SLEEP_MS: &str = "perf.idleSleepMs";
    pub const THROTTLE_MS: &str = "perf.throttleMs";
    pub const SUGGEST_NEAR_ONLY: &str = "perf.suggestNearOnly";
    pub const POLLING_FUNCTIONS: &str = "perf.pollingFunctions";
    pub const MIN_CALLS: &str = "perf.minCalls";
    pub const ALL_LOOPS: &str = "perf.allLoops";
    pub const SUGGEST_PLAYER_PED_ID: &str = "perf.suggestPlayerPedId";
}

#[derive(Default)]
pub struct PerfFeature {
    /// What the functions of every project file call.
    index: RefCell<ProjectIndex>,
}

impl PerfFeature {
    /// Whether the checks apply: a Lua script (not a manifest) with the plugin on.
    fn applies(cx: &Ctx) -> bool {
        cx.settings.enabled() && cx.doc.is_lua() && !cx.doc.is_manifest() && cx.doc.side != Side::Manifest
    }

    fn options(cx: &Ctx) -> checks::Options {
        let s = cx.settings;
        let d = checks::Options::default();
        checks::Options {
            idle_sleep_ms: s.int(keys::IDLE_SLEEP_MS, d.idle_sleep_ms).clamp(50, 60_000),
            throttle_ms: s.int(keys::THROTTLE_MS, d.throttle_ms).clamp(50, 60_000),
            suggest_near_only: s.bool(keys::SUGGEST_NEAR_ONLY, d.suggest_near_only),
            polling_functions: s.list(keys::POLLING_FUNCTIONS, ""),
            min_calls: s.int(keys::MIN_CALLS, d.min_calls as i64).clamp(2, 20) as usize,
            all_loops: s.bool(keys::ALL_LOOPS, d.all_loops),
            suggest_player_ped_id: s.bool(keys::SUGGEST_PLAYER_PED_ID, d.suggest_player_ped_id),
        }
    }
}

impl Feature for PerfFeature {
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut index = self.index.borrow_mut();
        if kind == FileKind::Lua {
            index.set(path, FileFacts::of(&ast::parse(src), src));
        } else {
            index.remove(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.index.borrow_mut().remove(path);
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let index = self.index.borrow();
        let p = Perf::new(cx, &index);
        checks::problems(&p, &Self::options(cx))
    }

    fn line_markers(&self, cx: &Ctx) -> Vec<LineMarker> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let index = self.index.borrow();
        let p = Perf::new(cx, &index);
        checks::markers(&p)
            .into_iter()
            .map(|(s, cost, tooltip)| {
                let keyword = cx.doc.src.toks[p.ast.stats[s].first];
                LineMarker {
                    line: cx.doc.position(keyword.start).line,
                    icon: Some(if cost == Cost::PerFrame { "run" } else { "navigate" }.to_owned()),
                    tooltip,
                    target: Some(cx.doc.location(keyword.start, keyword.end)),
                }
            })
            .collect()
    }

    fn intentions(&self, cx: &Ctx, start: usize, _end: usize) -> Vec<Intention> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let index = self.index.borrow();
        let p = Perf::new(cx, &index);
        distance_intention(&p, start).into_iter().map(|fix| Intention { fix, kind: "refactor" }).collect()
    }
}

/// «Replace GetDistanceBetweenCoords with #(a - b)» on the innermost distance call around `at`.
fn distance_intention(p: &Perf, at: usize) -> Option<Fix> {
    let ast = &p.ast;
    let toks = [p.src.tok_at(at), at.checked_sub(1).and_then(|a| p.src.tok_at(a))];
    for tok in toks.into_iter().flatten() {
        let mut n = ast.owner_of(tok);
        while let Some(node) = n {
            if matches!(node, Node::Stat(_)) {
                break;
            }
            if let Node::Expr(call) = node
                && matches!(ast.exprs[call].kind, ExprKind::Call { .. })
                && let Some(name) = ast.callee_name(call).filter(|n| distance::NATIVES.contains(n))
                && let Some(text) = distance::replacement(p, call)
            {
                return Some(Fix::new(format!("Replace {name} with #(a - b)")).replace(
                    ast.exprs[call].start,
                    ast.exprs[call].stop,
                    text,
                ));
            }
            n = ast.parent(node);
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests;
