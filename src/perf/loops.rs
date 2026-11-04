use super::ast::{Ast, ExprKind, Node, StatKind};
use super::classify::{CallInfo, Perf, WaitValue};
use super::spec::{Work, Yield};
use std::cell::OnceCell;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cost {
    PerFrame,
    Adaptive,
    Throttled,
    EventDriven,
}

pub struct Polling {
    pub wait: Rc<CallInfo>,
    pub wait_stat: usize,
    pub near_only: bool,
}

pub struct LoopFacts<'p, 'a> {
    pub p: &'p Perf<'a>,
    pub stat: usize,
    pub cond: usize,
    pub body: usize,
    pub endless: bool,
    pub long_running: bool,
    calls: OnceCell<Vec<Rc<CallInfo>>>,
    own_calls: OnceCell<Vec<Rc<CallInfo>>>,
    near: OnceCell<Vec<usize>>,
}

pub fn is_wait_loop(ast: &Ast, n: Node) -> bool {
    matches!(n, Node::Stat(s) if matches!(ast.stats[s].kind, StatKind::While { .. } | StatKind::Repeat { .. }))
}

fn truthy(ast: &Ast, e: usize) -> bool {
    let e = ast.unwrap(e);
    match &ast.exprs[e].kind {
        ExprKind::True | ExprKind::Number | ExprKind::Str | ExprKind::Hash => true,
        ExprKind::Unary { op: "not", operand } => falsy(ast, *operand),
        _ => false,
    }
}

fn falsy(ast: &Ast, e: usize) -> bool {
    let e = ast.unwrap(e);
    match &ast.exprs[e].kind {
        ExprKind::Nil | ExprKind::False => true,
        ExprKind::Unary { op: "not", operand } => truthy(ast, *operand),
        _ => false,
    }
}

pub fn loop_parts(ast: &Ast, s: usize) -> Option<(usize, usize, bool)> {
    match ast.stats[s].kind {
        StatKind::While { cond, body } => Some((cond, body, truthy(ast, cond))),
        StatKind::Repeat { body, cond } => Some((cond, body, falsy(ast, cond))),
        _ => None,
    }
}

pub fn is_long_running(ast: &Ast, s: usize) -> bool {
    let Some((cond, _, endless)) = loop_parts(ast, s) else { return false };
    if endless {
        return true;
    }
    let mut e = ast.unwrap(cond);
    if let ExprKind::Unary { op: "not", operand } = ast.exprs[e].kind {
        e = ast.unwrap(operand);
    }
    matches!(ast.exprs[e].kind, ExprKind::Name { .. } | ExprKind::Field { .. } | ExprKind::Index { .. })
        && ast.calls(Node::Expr(e), &|_| false).is_empty()
}

fn loop_of(ast: &Ast, brk: usize) -> Option<usize> {
    let mut n = ast.parent(Node::Stat(brk));
    while let Some(x) = n {
        if ast.is_function(x) {
            return None;
        }
        if let Node::Stat(s) = x
            && matches!(
                ast.stats[s].kind,
                StatKind::While { .. } | StatKind::Repeat { .. } | StatKind::NumFor { .. } | StatKind::GenFor { .. }
            )
        {
            return Some(s);
        }
        n = ast.parent(x);
    }
    None
}

pub fn has_exit_in(ast: &Ast, root: Node, lp: usize) -> bool {
    let mut found = false;
    ast.walk(root, &|_| false, &mut |n| {
        if let Node::Stat(s) = n {
            match ast.stats[s].kind {
                StatKind::Return(_) | StatKind::Goto => found = true,
                StatKind::Break if loop_of(ast, s) == Some(lp) => found = true,
                _ => {}
            }
        }
    });
    found
}

const DISTANCE_NATIVES: &[&str] = &["GetDistanceBetweenCoords", "Vdist", "Vdist2"];

fn is_distance(ast: &Ast, e: usize, depth: usize) -> bool {
    let e = ast.unwrap(e);
    match &ast.exprs[e].kind {
        ExprKind::Unary { op: "#", operand } => {
            matches!(ast.exprs[ast.unwrap(*operand)].kind, ExprKind::Binary { op: "-", .. })
        }
        ExprKind::Call { .. } => ast.callee_name(e).is_some_and(|n| DISTANCE_NATIVES.contains(&n)),
        ExprKind::Name { .. } if depth == 0 => ast.local_initializer(e).is_some_and(|i| is_distance(ast, i, 1)),
        _ => false,
    }
}

fn is_near(ast: &Ast, e: usize, depth: usize) -> bool {
    let e = ast.unwrap(e);
    match &ast.exprs[e].kind {
        ExprKind::Name { .. } if depth == 0 => ast.local_initializer(e).is_some_and(|i| is_near(ast, i, 1)),
        ExprKind::Binary { op: "and", lhs, rhs } => is_near(ast, *lhs, depth) || is_near(ast, *rhs, depth),
        ExprKind::Binary { op: "<" | "<=", lhs, .. } => is_distance(ast, *lhs, 0),
        ExprKind::Binary { op: ">" | ">=", rhs, .. } => is_distance(ast, *rhs, 0),
        _ => false,
    }
}

fn is_far(ast: &Ast, e: usize) -> bool {
    match &ast.exprs[ast.unwrap(e)].kind {
        ExprKind::Binary { op: ">" | ">=", lhs, .. } => is_distance(ast, *lhs, 0),
        ExprKind::Binary { op: "<" | "<=", rhs, .. } => is_distance(ast, *rhs, 0),
        _ => false,
    }
}

pub fn assigned_names(ast: &Ast, root: Node) -> HashSet<String> {
    let mut out = HashSet::new();
    ast.walk(root, &|_| false, &mut |n| {
        let Node::Stat(s) = n else { return };
        let targets: &[usize] = match &ast.stats[s].kind {
            StatKind::Assign { targets, .. } => targets,
            StatKind::Compound { target, .. } => std::slice::from_ref(target),
            _ => return,
        };
        out.extend(targets.iter().filter_map(|&t| ast.name_of(t)).map(str::to_owned));
    });
    out
}

pub fn read_names(ast: &Ast, root: Node) -> HashSet<String> {
    let mut out = HashSet::new();
    ast.walk(root, &|_| false, &mut |n| {
        if let Node::Expr(e) = n
            && let Some(name) = ast.name_of(e)
        {
            out.insert(name.to_owned());
        }
    });
    out
}

#[derive(Clone, Copy)]
struct Flow {
    falls: bool,
    breaks: bool,
}

const STOP: Flow = Flow { falls: false, breaks: false };
const PASS: Flow = Flow { falls: true, breaks: false };
const BREAK: Flow = Flow { falls: false, breaks: true };

impl<'p, 'a> LoopFacts<'p, 'a> {
    pub fn new(p: &'p Perf<'a>, stat: usize) -> Option<Self> {
        let (cond, body, endless) = loop_parts(&p.ast, stat)?;
        let long_running = is_long_running(&p.ast, stat);
        Some(Self {
            p,
            stat,
            cond,
            body,
            endless,
            long_running,
            calls: OnceCell::new(),
            own_calls: OnceCell::new(),
            near: OnceCell::new(),
        })
    }

    fn ast(&self) -> &Ast {
        &self.p.ast
    }

    pub fn has_exit(&self) -> bool {
        has_exit_in(self.ast(), Node::Block(self.body), self.stat)
    }

    pub fn calls(&self) -> &[Rc<CallInfo>] {
        self.calls.get_or_init(|| {
            self.ast().calls(Node::Stat(self.stat), &|_| false).into_iter().map(|c| self.p.info(c)).collect()
        })
    }

    pub fn own_calls(&self) -> &[Rc<CallInfo>] {
        self.own_calls.get_or_init(|| {
            let ast = self.ast();
            let me = Node::Stat(self.stat);
            ast.calls(me, &|n| n != me && is_wait_loop(ast, n)).into_iter().map(|c| self.p.info(c)).collect()
        })
    }

    pub fn near_blocks(&self) -> &[usize] {
        self.near.get_or_init(|| {
            let mut out = Vec::new();
            self.visit_near(self.body, &mut out);
            out
        })
    }

    fn visit_near(&self, block: usize, out: &mut Vec<usize>) {
        let ast = self.ast();
        for &s in &ast.blocks[block].stats {
            match &ast.stats[s].kind {
                StatKind::If { conds, blocks, otherwise } => {
                    for (&c, &b) in conds.iter().zip(blocks) {
                        if is_near(ast, c, 0) { out.push(b) } else { self.visit_near(b, out) }
                    }
                    if let Some(b) = *otherwise {
                        if !conds.is_empty() && conds.iter().all(|&c| is_far(ast, c)) {
                            out.push(b)
                        } else {
                            self.visit_near(b, out)
                        }
                    }
                }
                StatKind::NumFor { body, .. } | StatKind::GenFor { body, .. } | StatKind::Do(body) => {
                    self.visit_near(*body, out)
                }
                _ => {}
            }
        }
    }

    pub fn in_near_block(&self, n: Node) -> bool {
        self.near_blocks().iter().any(|&b| self.ast().is_ancestor(Node::Block(b), n, false))
    }

    pub fn cost(&self) -> Option<Cost> {
        let own = self.own_calls();
        let waits: Vec<&WaitValue> =
            own.iter().filter(|c| c.yields() == Yield::Wait).filter_map(|c| c.wait.as_ref()).collect();
        if waits.is_empty() {
            return own.iter().any(|c| c.yields() == Yield::Await).then_some(Cost::EventDriven);
        }
        let frame = waits.iter().any(|w| **w == WaitValue::Frame);
        let slower = waits.iter().any(|w| **w != WaitValue::Frame);
        Some(if waits.iter().any(|w| matches!(w, WaitValue::Adaptive(_))) || (frame && slower) {
            Cost::Adaptive
        } else if frame {
            Cost::PerFrame
        } else {
            Cost::Throttled
        })
    }

    pub fn never_yields(&self) -> bool {
        self.calls().iter().all(|c| c.yields() == Yield::None)
    }

    pub fn can_spin(&self) -> bool {
        self.flow_block(self.body).falls
    }

    fn stops(&self, n: Node) -> bool {
        self.ast().calls(n, &|_| false).into_iter().any(|c| {
            let i = self.p.info(c);
            i.yields() != Yield::None || i.spec.exits
        })
    }

    fn flow_block(&self, block: usize) -> Flow {
        let mut falls = true;
        let mut breaks = false;
        for &s in &self.ast().blocks[block].stats {
            if !falls {
                break;
            }
            let f = self.flow(s);
            breaks |= f.breaks;
            falls = f.falls;
        }
        Flow { falls, breaks }
    }

    fn flow(&self, s: usize) -> Flow {
        let ast = self.ast();
        match &ast.stats[s].kind {
            StatKind::Break => BREAK,
            StatKind::Return(_) | StatKind::Goto => STOP,
            StatKind::Do(b) => self.flow_block(*b),
            StatKind::If { conds, blocks, otherwise } => {
                let mut falls = false;
                let mut breaks = false;
                let mut reachable = true;
                let branches =
                    conds.iter().map(|&c| Some(c)).zip(blocks.iter().copied()).chain(otherwise.map(|b| (None, b)));
                for (cond, block) in branches {
                    if cond.is_some_and(|c| self.stops(Node::Expr(c))) {
                        reachable = false;
                        break;
                    }
                    let f = self.flow_block(block);
                    falls |= f.falls;
                    breaks |= f.breaks;
                }
                if reachable && otherwise.is_none() {
                    falls = true;
                }
                Flow { falls, breaks }
            }
            StatKind::While { cond, body } => {
                if self.stops(Node::Expr(*cond)) {
                    STOP
                } else if truthy(ast, *cond) {
                    Flow { falls: self.flow_block(*body).breaks, breaks: false }
                } else {
                    PASS
                }
            }
            StatKind::Repeat { body, cond } => {
                let f = self.flow_block(*body);
                let falls =
                    if falsy(ast, *cond) { f.breaks } else { (f.falls && !self.stops(Node::Expr(*cond))) || f.breaks };
                Flow { falls, breaks: false }
            }
            StatKind::NumFor { exprs, body, .. } => {
                if exprs.iter().any(|&e| self.stops(Node::Expr(e))) {
                    STOP
                } else if self.runs_at_least_once(exprs) {
                    let f = self.flow_block(*body);
                    Flow { falls: f.falls || f.breaks, breaks: false }
                } else {
                    PASS
                }
            }
            StatKind::GenFor { exprs, .. } => {
                if exprs.iter().any(|&e| self.stops(Node::Expr(e))) {
                    STOP
                } else {
                    PASS
                }
            }
            StatKind::Function { .. } | StatKind::LocalFunction { .. } | StatKind::Label | StatKind::Empty => PASS,
            _ => {
                if self.stops(Node::Stat(s)) {
                    STOP
                } else {
                    PASS
                }
            }
        }
    }

    fn runs_at_least_once(&self, exprs: &[usize]) -> bool {
        let ast = self.ast();
        if exprs.len() < 2 {
            return false;
        }
        let mut values = Vec::new();
        for &e in exprs {
            let e = ast.unwrap(e);
            if !ast.is_number(e) {
                return false;
            }
            match super::classify::lua_number(ast.text(self.p.src, Node::Expr(e))) {
                Some(v) => values.push(v),
                None => return false,
            }
        }
        let step = values.get(2).copied().unwrap_or(1.0);
        step > 0.0 && values[0] <= values[1]
    }

    pub fn polling(&self, declared: &[String]) -> Option<Polling> {
        if !self.long_running || self.has_exit() {
            return None;
        }
        let ast = self.ast();
        let is_declared = |c: &CallInfo| c.path.as_ref().is_some_and(|p| declared.contains(p));
        let calls = self.calls();
        let mut outside = calls
            .iter()
            .filter(|c| c.yields() != Yield::None && !is_declared(c) && !self.in_near_block(Node::Expr(c.call)));
        let wait = outside.next()?;
        if outside.next().is_some() || wait.yields() != Yield::Wait || wait.wait != Some(WaitValue::Frame) {
            return None;
        }
        let Some(Node::Stat(wait_stat)) = ast.parent(Node::Expr(wait.call)) else { return None };
        if !matches!(ast.stats[wait_stat].kind, StatKind::Expr(_)) || ast.stats[wait_stat].parent != self.body {
            return None;
        }
        let polling_work =
            |c: &CallInfo| c.yields() == Yield::None && matches!(c.work(), Work::Pure | Work::Query | Work::Action);
        let blocking: Vec<&Rc<CallInfo>> =
            calls.iter().filter(|c| c.call != wait.call && !is_declared(c) && !polling_work(c)).collect();
        let near_only = if blocking.is_empty() {
            false
        } else if !self.near_blocks().is_empty() && blocking.iter().all(|c| self.in_near_block(Node::Expr(c.call))) {
            true
        } else {
            return None;
        };
        Some(Polling { wait: wait.clone(), wait_stat, near_only })
    }
}
