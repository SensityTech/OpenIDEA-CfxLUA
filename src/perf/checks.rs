use super::ast::{Ast, ExprKind, Node, StatKind};
use super::classify::{CallInfo, Perf, WaitValue};
use super::edits::{self, Edit};
use super::loops::{Cost, LoopFacts, Polling, assigned_names, has_exit_in, is_long_running, is_wait_loop, read_names};
use super::repeated;
use super::spec::{Registration, Spam, Work, Yield};
use super::{distance, spec};
use crate::feature::{Fix, Problem};
use openidea_plugin::Severity;

pub struct Options {
    pub idle_sleep_ms: i64,
    pub throttle_ms: i64,
    pub suggest_near_only: bool,
    pub polling_functions: Vec<String>,
    pub min_calls: usize,
    pub all_loops: bool,
    pub suggest_player_ped_id: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            idle_sleep_ms: 1000,
            throttle_ms: 500,
            suggest_near_only: true,
            polling_functions: Vec::new(),
            min_calls: 2,
            all_loops: false,
            suggest_player_ped_id: true,
        }
    }
}

pub const PER_FRAME_LOOP: &str = "CfxPerFrameLoop";
pub const LOOP_WITHOUT_WAIT: &str = "CfxLoopWithoutWait";
pub const REPEATED_NATIVE: &str = "CfxRepeatedNativeCall";
pub const DISTANCE_NATIVE: &str = "CfxDistanceNative";
pub const PER_FRAME_SPAM: &str = "CfxPerFrameSpam";
pub const REPEATED_REGISTRATION: &str = "CfxRepeatedRegistration";

const WEAK: Severity = Severity::Info;

fn fix(title: String, edits: Vec<Edit>) -> Fix {
    let mut f = Fix::new(title);
    f.edits = edits::normalize(edits);
    f
}

pub fn problems(p: &Perf, opts: &Options) -> Vec<Problem> {
    let ast = &p.ast;
    let mut out = Vec::new();
    for s in 0..ast.stats.len() {
        let Some(lp) = LoopFacts::new(p, s) else { continue };
        per_frame_loop(&lp, opts, &mut out);
        loop_without_wait(&lp, &mut out);
        repeated_natives(&lp, opts, &mut out);
        per_frame_spam(&lp, &mut out);
    }
    for (call, expr) in ast.exprs.iter().enumerate() {
        if !matches!(expr.kind, ExprKind::Call { .. }) {
            continue;
        }
        player_ped_minus_one(p, opts, call, &mut out);
        distance_native(p, call, &mut out);
        repeated_registration(p, call, &mut out);
    }
    out
}

// 1. per-frame polling

fn per_frame_loop(lp: &LoopFacts, opts: &Options, out: &mut Vec<Problem>) {
    let Some(polling) = lp.polling(&opts.polling_functions) else { return };
    if polling.near_only && !opts.suggest_near_only {
        return;
    }
    let p = lp.p;
    let ast = &p.ast;
    let wait = polling.wait.call;
    let (start, stop) = (ast.exprs[wait].start, ast.exprs[wait].stop);
    let mut fixes = Vec::new();
    if let Some(edits) = adaptive_sleep(lp, &polling, opts.idle_sleep_ms) {
        fixes.push(fix(
            format!("Use adaptive sleep ({} ms when far, every frame when near)", opts.idle_sleep_ms),
            edits,
        ));
    }
    if !polling.near_only {
        let ExprKind::Call { callee, .. } = ast.exprs[wait].kind else { return };
        let open = p.src.toks[ast.exprs[callee].end].start;
        fixes.push(fix(
            format!("Wait {} ms between iterations", opts.throttle_ms),
            vec![(open, stop, format!("({})", opts.throttle_ms))],
        ));
        let text = ast.text(p.src, Node::Expr(wait));
        let message = format!(
            "'{text}' makes this loop run every frame, but it only reads state (no drawing, no control input): it can sleep between checks"
        );
        let mut problem = Problem::new(PER_FRAME_LOOP, WEAK, start, stop, message);
        problem.fixes = fixes;
        out.push(problem);
    } else if !fixes.is_empty() {
        let message = "The per-frame work of this loop only runs near a position: the loop can sleep while the player is far away";
        let mut problem = Problem::new(PER_FRAME_LOOP, Severity::Hint, start, stop, message);
        problem.fixes = fixes;
        out.push(problem);
    }
}

fn adaptive_sleep(lp: &LoopFacts, polling: &Polling, idle: i64) -> Option<Vec<Edit>> {
    let p = lp.p;
    let (ast, src) = (&p.ast, p.src);
    let near = lp.near_blocks();
    let stats = &ast.blocks[lp.body].stats;
    if near.is_empty() || stats.len() < 2 {
        return None;
    }
    let name = edits::fresh_name(ast, lp.stat, "sleep");
    let ws = polling.wait_stat;
    let ExprKind::Call { callee, .. } = ast.exprs[polling.wait.call].kind else { return None };
    let wait = format!("{}({name})", ast.text(src, Node::Expr(callee)));
    let (ws_start, ws_stop) = (ast.stats[ws].start, ast.stats[ws].stop);
    // A comment on the line of the Wait(0) moves with it.
    let comment = edits::trailing_comment(src, ws_stop).filter(|c| edits::end_of_line(src, ws_stop) == c.1);
    let moved = match comment {
        Some((a, b)) => format!("{wait} {}", &src.text[a..b]),
        None => wait.clone(),
    };
    let declaration = format!("local {name} = {idle}");
    let owner = Node::Stat(lp.stat);
    let mut out = Vec::new();
    if stats.first() == Some(&ws) {
        out.push((ws_start, edits::end_of_line(src, ws_stop), declaration));
        out.push(edits::insert_last(ast, src, owner, lp.body, &moved, Some(ws)));
    } else if stats.last() == Some(&ws) {
        out.push(edits::insert_first(ast, src, owner, lp.body, &declaration));
        out.push((ws_start, ws_stop, wait));
    } else {
        out.push(edits::insert_first(ast, src, owner, lp.body, &declaration));
        out.push(edits::delete(src, (ws_start, ws_stop)));
        let line = if edits::starts_line(&src.text, ws_start) { moved } else { wait };
        out.push(edits::insert_last(ast, src, owner, lp.body, &line, Some(ws)));
    }
    for &block in near {
        let Some(owner) = ast.blocks[block].parent else { continue };
        out.push(edits::insert_first(ast, src, owner, block, &format!("{name} = 0")));
    }
    Some(out)
}

// 2. endless loops that never wait

fn loop_without_wait(lp: &LoopFacts, out: &mut Vec<Problem>) {
    if lp.has_exit() {
        return;
    }
    let p = lp.p;
    let message = if lp.endless {
        if !lp.can_spin() {
            return;
        }
        let place = match p.side {
            crate::project::Side::Client => "game",
            crate::project::Side::Server => "server",
            _ => "game or the server",
        };
        if lp.never_yields() {
            format!("Endless loop that never waits: it blocks its thread and freezes the {place}")
        } else {
            format!(
                "An iteration of this endless loop can finish without waiting: when it does, it freezes the {place}"
            )
        }
    } else {
        let Some(loading) = loading_check(lp) else { return };
        format!(
            "'{}' only changes between frames: this loop never waits, so it never ends and freezes the game",
            loading.display
        )
    };
    let keyword = p.src.toks[p.ast.stats[lp.stat].first];
    let wait = edits::wait_function(&p.src.text);
    let edit = edits::insert_last(&p.ast, p.src, Node::Stat(lp.stat), lp.body, &format!("{wait}(0)"), None);
    let mut problem = Problem::new(LOOP_WITHOUT_WAIT, Severity::Warning, keyword.start, keyword.end, message);
    problem.fixes.push(fix(format!("Insert {wait}(0) at the end of the loop"), vec![edit]));
    out.push(problem);
}

fn loading_check(lp: &LoopFacts) -> Option<std::rc::Rc<CallInfo>> {
    if !lp.p.may_run_on_client() || !lp.never_yields() {
        return None;
    }
    lp.p.ast.calls(Node::Expr(lp.cond), &|_| false).into_iter().map(|c| lp.p.info(c)).find(|c| {
        matches!(c.work(), Work::Query | Work::PerFrame)
            && c.path.as_deref().is_some_and(|p| spec::words(p).contains(&"Loaded"))
    })
}

// 3. repeated natives

fn repeated_natives(lp: &LoopFacts, opts: &Options, out: &mut Vec<Problem>) {
    let Some(cost) = lp.cost() else { return };
    if !opts.all_loops && !matches!(cost, Cost::PerFrame | Cost::Adaptive) {
        return;
    }
    let p = lp.p;
    let ast = &p.ast;
    for g in repeated::groups(lp, opts.min_calls.max(2)) {
        let display = repeated::display(ast, p.src, &g);
        let cache = repeated::cache_edits(lp, &g);
        let message =
            format!("'{display}' is called {} times in the same frame: cache the value in a local", g.calls.len());
        for &c in &g.calls {
            let mut problem =
                Problem::new(REPEATED_NATIVE, WEAK, ast.exprs[c].start, ast.exprs[c].stop, message.clone());
            if let Some(cache) = &cache {
                let title = if cache.reuse {
                    format!("Use local '{}' instead of calling '{display}' again", cache.name)
                } else {
                    format!("Cache '{display}' in local '{}'", cache.name)
                };
                problem.fixes.push(fix(title, cache.edits.clone()));
            }
            out.push(problem);
        }
    }
}

fn player_ped_minus_one(p: &Perf, opts: &Options, call: usize, out: &mut Vec<Problem>) {
    let ast = &p.ast;
    if !opts.suggest_player_ped_id || !p.may_run_on_client() || !repeated::is_player_ped_minus_one(ast, p.src, call) {
        return;
    }
    let ExprKind::Call { callee, .. } = ast.exprs[call].kind else { return };
    if ast.decl_of(callee).is_some() {
        return;
    }
    let (start, stop) = (ast.exprs[call].start, ast.exprs[call].stop);
    let message = "'GetPlayerPed(-1)' is a slower way to get the player's ped: use 'PlayerPedId()'";
    out.push(
        Problem::new(REPEATED_NATIVE, WEAK, start, stop, message)
            .fix(fix("Replace with PlayerPedId()".into(), vec![(start, stop, repeated::PLAYER_PED_ID.into())])),
    );
}

// 4. distance natives

fn distance_native(p: &Perf, call: usize, out: &mut Vec<Problem>) {
    let ast = &p.ast;
    let Some(name) = ast.callee_name(call).filter(|n| distance::NATIVES.contains(n)) else { return };
    if !p.may_run_on_client() || p.info(call).work() != Work::Query {
        return;
    }
    let ExprKind::Call { callee, .. } = ast.exprs[call].kind else { return };
    let (start, stop) = (ast.exprs[callee].start, ast.exprs[callee].stop);
    let mut problem = Problem::new(
        DISTANCE_NATIVE,
        WEAK,
        start,
        stop,
        format!("'{name}' calls into the game for simple math: compute the distance with vectors, #(a - b)"),
    );
    if let Some(replacement) = distance::replacement(p, call) {
        let title = if replacement.chars().count() <= 60 {
            format!("Replace with '{replacement}'")
        } else {
            "Replace with vector math #(a - b)".to_owned()
        };
        problem.fixes.push(fix(title, vec![(ast.exprs[call].start, ast.exprs[call].stop, replacement)]));
    }
    out.push(problem);
}

// 5. per-frame spam

fn per_frame_spam(lp: &LoopFacts, out: &mut Vec<Problem>) {
    if !lp.long_running || lp.cost() != Some(Cost::PerFrame) {
        return;
    }
    let p = lp.p;
    let ast = &p.ast;
    for call in ast.calls(Node::Block(lp.body), &|n| is_wait_loop(ast, n)) {
        let info = p.info(call);
        let Some(spam) = info.spec.spam else { continue };
        if (spam == Spam::NetworkToServer && !p.may_run_on_client())
            || (spam == Spam::NetworkToClients && !p.may_run_on_server())
        {
            continue;
        }
        if !runs_every_frame(lp, call) {
            continue;
        }
        let what = match spam {
            Spam::Log => "it floods the console and costs time on each frame",
            Spam::Serialization => "serializing on each frame wastes CPU time",
            Spam::NetworkToServer => "it floods the server with network events",
            Spam::NetworkToClients => "it floods the clients with network traffic",
            Spam::Storage => "it reads or writes the disk on each frame",
            Spam::Http => "it sends an HTTP request on each frame",
            Spam::Database => "it queries the database on each frame",
        };
        let (start, stop) = ast.callee_span(p.src, call);
        let mut problem = Problem::new(
            PER_FRAME_SPAM,
            WEAK,
            start,
            stop,
            format!("'{}' runs every frame here: {what}", info.display),
        );
        if spam == Spam::Log
            && let Some(Node::Stat(s)) = ast.parent(Node::Expr(call))
            && matches!(ast.stats[s].kind, StatKind::Expr(_))
        {
            let edit = edits::delete(p.src, (ast.stats[s].start, ast.stats[s].stop));
            problem.fixes.push(fix(format!("Remove the '{}' call", info.display), vec![edit]));
        }
        out.push(problem);
    }
}

fn runs_every_frame(lp: &LoopFacts, call: usize) -> bool {
    let ast = &lp.p.ast;
    let mut n = Node::Expr(call);
    while n != Node::Block(lp.body) {
        let Some(parent) = ast.parent(n) else { return false };
        if let (Node::Block(b), Node::Stat(s)) = (n, parent)
            && matches!(ast.stats[s].kind, StatKind::If { .. })
            && !is_level_branch(lp, s, b)
        {
            return false;
        }
        n = parent;
    }
    true
}

fn branch(ast: &Ast, s: usize, block: usize) -> Option<(Option<usize>, Vec<usize>)> {
    let StatKind::If { conds, blocks, otherwise } = &ast.stats[s].kind else { return None };
    if let Some(i) = blocks.iter().position(|&b| b == block) {
        return Some((Some(conds[i]), conds.clone()));
    }
    (*otherwise == Some(block)).then(|| (None, conds.clone()))
}

fn is_level_branch(lp: &LoopFacts, s: usize, block: usize) -> bool {
    let p = lp.p;
    let ast = &p.ast;
    let Some((own, all)) = branch(ast, s, block) else { return false };
    let conditions = match own {
        Some(c) => vec![c],
        None => all,
    };
    // `if Config.Debug then print(...) end`: output wanted while debugging.
    if own.is_some_and(|c| ast.text(p.src, Node::Expr(c)).to_ascii_lowercase().contains("debug")) {
        return false;
    }
    for &c in &conditions {
        for call in ast.calls(Node::Expr(c), &|_| false) {
            let info = p.info(call);
            if info.work() == Work::Unknown {
                return false;
            }
            let Some(path) = &info.path else { return false };
            let last = path.rsplit('.').next().unwrap_or(path);
            if spec::words(last).iter().any(|w| *w == "Just" || *w == "Completed") {
                return false;
            }
        }
    }
    // A branch flipping what its condition reads runs once per change.
    let read: std::collections::HashSet<String> =
        conditions.iter().flat_map(|&c| read_names(ast, Node::Expr(c))).collect();
    if assigned_names(ast, Node::Block(block)).iter().any(|n| read.contains(n)) {
        return false;
    }
    if ast.calls(Node::Block(block), &|_| false).into_iter().any(|c| p.info(c).yields() != Yield::None) {
        return false;
    }
    !has_exit_in(ast, Node::Block(block), lp.stat)
}

// 6. repeated registrations

enum Place {
    Loop,
    Handler(Option<String>),
    Command(Option<String>),
}

const RESOURCE_START_EVENTS: &[&str] = &["onResourceStart", "onClientResourceStart", "onServerResourceStart"];

fn repeated_registration(p: &Perf, call: usize, out: &mut Vec<Problem>) {
    let ast = &p.ast;
    let ExprKind::Call { callee, method: None, args } = &ast.exprs[call].kind else { return };
    let Some(path) = ast.path(*callee) else { return };
    if spec::runtime(&path).and_then(|s| s.registration).is_none() {
        return;
    }
    let info = p.info(call);
    let Some(registration) = info.spec.registration else { return };
    match registration {
        Registration::Handler => {
            let statement = matches!(ast.parent(Node::Expr(call)), Some(Node::Stat(s)) if matches!(ast.stats[s].kind, StatKind::Expr(_)));
            if !statement || args.len() <= info.spec.function_arg {
                return;
            }
        }
        Registration::Thread => {
            let Some(&f) = args.get(info.spec.function_arg) else { return };
            if !has_endless_loop(ast, f) {
                return;
            }
        }
        Registration::Timeout | Registration::Command => return,
    }
    let Some(place) = repeated_place(p, call) else { return };
    let name = &info.display;
    let thread = registration == Registration::Thread;
    let message = match (place, thread) {
        (Place::Loop, false) => {
            format!("'{name}' inside a loop: each iteration adds one more handler that is never removed")
        }
        (Place::Loop, true) => {
            format!("'{name}' starts an endless thread inside a loop: each iteration starts one more")
        }
        (Place::Command(cmd), false) => format!(
            "'{name}' inside the '{}' command: each use of the command adds one more handler that is never removed",
            cmd.as_deref().unwrap_or("?")
        ),
        (Place::Command(cmd), true) => format!(
            "'{name}' starts an endless thread inside the '{}' command: each use of the command starts one more",
            cmd.as_deref().unwrap_or("?")
        ),
        (Place::Handler(Some(event)), false) => {
            format!(
                "'{name}' inside the '{event}' handler: each '{event}' event adds one more handler that is never removed"
            )
        }
        (Place::Handler(Some(event)), true) => {
            format!(
                "'{name}' starts an endless thread inside the '{event}' handler: each '{event}' event starts one more"
            )
        }
        (Place::Handler(None), false) => {
            format!("'{name}' inside an event handler: each event adds one more handler that is never removed")
        }
        (Place::Handler(None), true) => {
            format!("'{name}' starts an endless thread inside an event handler: each event starts one more")
        }
    };
    let (start, stop) = (ast.exprs[*callee].start, ast.exprs[*callee].stop);
    out.push(Problem::new(REPEATED_REGISTRATION, Severity::Warning, start, stop, message));
}

fn repeated_place(p: &Perf, call: usize) -> Option<Place> {
    let ast = &p.ast;
    let mut n = Node::Expr(call);
    loop {
        let parent = ast.parent(n)?;
        if let (Node::Block(b), Node::Stat(s)) = (n, parent)
            && is_one_shot_guard(ast, s, b)
        {
            return None;
        }
        if let Node::Stat(s) = parent
            && is_wait_loop(ast, parent)
            && is_long_running(ast, s)
        {
            return Some(Place::Loop);
        }
        if let Node::Expr(f) = parent
            && matches!(ast.exprs[f].kind, ExprKind::Function { .. })
        {
            // Only callbacks of a call: named functions run wherever they are called.
            let Some(Node::Expr(owner)) = ast.parent(parent) else { return None };
            let ExprKind::Call { args, .. } = &ast.exprs[owner].kind else { return None };
            let spec = p.info(owner).spec;
            if args.get(spec.function_arg) != Some(&f) {
                return None;
            }
            let literal = |i: usize| {
                args.get(i).and_then(|&a| matches!(ast.exprs[a].kind, ExprKind::Str).then(|| string_value(p, a)))
            };
            match spec.registration {
                Some(Registration::Thread | Registration::Timeout) => {}
                Some(Registration::Handler) => {
                    let event = literal(spec.name_arg);
                    if event.as_deref().is_some_and(|e| RESOURCE_START_EVENTS.contains(&e))
                        && ast.text(p.src, parent).contains("GetCurrentResourceName")
                    {
                        return None;
                    }
                    return Some(Place::Handler(event));
                }
                Some(Registration::Command) => return Some(Place::Command(literal(spec.name_arg))),
                None => return None,
            }
        }
        n = parent;
    }
}

fn string_value(p: &Perf, e: usize) -> String {
    p.src.toks[p.ast.exprs[e].first].string_value(&p.src.text)
}

fn is_one_shot_guard(ast: &Ast, s: usize, block: usize) -> bool {
    let Some((Some(cond), _)) = branch(ast, s, block) else { return false };
    let read = read_names(ast, Node::Expr(cond));
    assigned_names(ast, Node::Block(block)).iter().any(|n| read.contains(n))
}

fn has_endless_loop(ast: &Ast, f: usize) -> bool {
    let ExprKind::Function { body, .. } = ast.exprs[ast.unwrap(f)].kind else { return false };
    let mut found = false;
    ast.walk(Node::Block(body), &|_| false, &mut |n| {
        if let Node::Stat(s) = n
            && let Some((_, b, true)) = super::loops::loop_parts(ast, s)
            && !has_exit_in(ast, Node::Block(b), s)
        {
            found = true;
        }
    });
    found
}

// Loop cost markers

pub fn markers(p: &Perf) -> Vec<(usize, Cost, String)> {
    let mut out = Vec::new();
    for s in 0..p.ast.stats.len() {
        let Some(lp) = LoopFacts::new(p, s) else { continue };
        if !lp.long_running {
            continue;
        }
        let Some(cost) = lp.cost() else { continue };
        out.push((s, cost, tooltip(&lp, cost)));
    }
    out
}

fn tooltip(lp: &LoopFacts, cost: Cost) -> String {
    let p = lp.p;
    let own = lp.own_calls();
    let waits: Vec<&std::rc::Rc<CallInfo>> = own.iter().filter(|c| c.yields() == Yield::Wait).collect();
    let values: Vec<&WaitValue> = waits.iter().filter_map(|c| c.wait.as_ref()).collect();
    let text = |c: &CallInfo| p.ast.text(p.src, Node::Expr(c.call)).to_owned();
    match cost {
        Cost::PerFrame => {
            let first = waits.first().map(|c| text(c)).unwrap_or_default();
            format!("Runs every frame ({}) — {}", shorten(&first), per_frame_reason(lp))
        }
        Cost::Adaptive => {
            let idle = values.iter().filter_map(|v| match v {
                WaitValue::Fixed(ms) | WaitValue::Adaptive(ms) => Some(*ms),
                _ => None,
            });
            match idle.max() {
                Some(ms) => format!("Adaptive: every frame or every {ms} ms depending on the state"),
                None => "Adaptive: every frame or slower depending on the state".into(),
            }
        }
        Cost::Throttled => {
            let fixed: Vec<i64> =
                values.iter().filter_map(|v| if let WaitValue::Fixed(ms) = v { Some(*ms) } else { None }).collect();
            match fixed.iter().min() {
                Some(min) if fixed.len() == values.len() => format!("Throttled: runs every {min} ms"),
                _ => {
                    let texts: Vec<String> = waits.iter().map(|c| text(c)).collect();
                    format!("Throttled: waits {} ms between iterations", shorten(&texts.join(", ")))
                }
            }
        }
        Cost::EventDriven => {
            let name = own.iter().find(|c| c.yields() == Yield::Await).map(|c| c.display.clone()).unwrap_or_default();
            format!("Event-driven: resumes when '{}' returns", shorten(&name))
        }
    }
}

fn per_frame_reason(lp: &LoopFacts) -> String {
    let own = lp.own_calls();
    let calm: Vec<&std::rc::Rc<CallInfo>> = own.iter().filter(|c| c.yields() == Yield::None).collect();
    let per_frame =
        names(calm.iter().filter(|c| matches!(c.work(), Work::PerFrame | Work::Effect)).map(|c| c.display.as_str()));
    if !per_frame.is_empty() {
        return format!("Per-frame work: {per_frame}");
    }
    let unknown = names(
        own.iter().filter(|c| c.work() == Work::Unknown && c.yields() != Yield::Wait).map(|c| c.display.as_str()),
    );
    if !unknown.is_empty() {
        return format!("Calls code the plugin can't follow: {unknown}");
    }
    "No per-frame work found: the loop could wait longer between iterations".into()
}

fn names<'s>(all: impl Iterator<Item = &'s str>) -> String {
    let mut distinct: Vec<&str> = Vec::new();
    for n in all {
        if !distinct.contains(&n) {
            distinct.push(n);
        }
    }
    let shown: Vec<String> = distinct.iter().take(4).map(|n| shorten(n)).collect();
    let mut out = shown.join(", ");
    if distinct.len() > 4 {
        out.push_str(", …");
    }
    out
}

fn shorten(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 60 { format!("{}…", one.chars().take(59).collect::<String>()) } else { one }
}
