use super::ast::{self, Ast, ExprKind, StmtKind};
use super::db::Db;
use super::flow::{doc_block, event_tag};
use super::model::{Dirs, Role, Site, compatible, side_text};
use super::spec::{ERROR_FUNCTION, ERROR_KIND, Spec};
use super::{count, keys, location};
use crate::feature::{Ctx, Fix, Problem};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use openidea_plugin::Severity;

fn key_range(site: &Site) -> (usize, usize) {
    let mut lits = site.occurrences.iter().flatten();
    match lits.next() {
        Some(first) => {
            let (s, e) =
                lits.fold((first.span.start, first.span.end), |(s, e), o| (s.min(o.span.start), e.max(o.span.end)));
            (s as usize, e as usize)
        }
        None => callee_range(site),
    }
}

fn callee_range(site: &Site) -> (usize, usize) {
    (site.callee_span.start as usize, site.callee_span.end as usize)
}

fn problem(code: &'static str, severity: Severity, (start, end): (usize, usize), message: String) -> Problem {
    Problem::new(code, severity, start, end, message)
}

pub fn sites(cx: &Ctx, db: &Db) -> Vec<Problem> {
    let report_ignored = cx.settings.bool(keys::REPORT_IGNORED_VALUES, true);
    let sites: Vec<&Site> = db.sites_in(&cx.doc.path).collect();
    let mut out = Vec::new();
    for site in &sites {
        match site.role {
            Role::Trigger => {
                out.extend(no_handler(db, site));
                out.extend(argument_count(cx, db, site, report_ignored));
            }
            Role::Handler => {
                out.extend(not_safe_for_net(cx, db, site));
                out.extend(duplicate_handler(cx, db, site));
            }
            _ => {}
        }
        out.extend(wrong_side(&sites, site));
    }
    out
}

fn no_handler(db: &Db, site: &Site) -> Option<Problem> {
    let key = &site.key;
    if key.is_builtin() || db.may_be_handled_dynamically(key, false) {
        return None;
    }
    let message = if db.handlers(key).is_empty() {
        if key.namespace().is_empty() || !db.is_handled_namespace(key.namespace()) {
            return None;
        }
        format!("Event {} has no handler in this project", key.quoted())
    } else {
        if !db.handlers_reached_by(site).is_empty() {
            return None;
        }
        let target = match site.directions {
            Dirs::SERVER => crate::project::Side::Server,
            Dirs::CLIENT => crate::project::Side::Client,
            _ => site.side(),
        };
        format!("Event {} has no {} handler in this project", key.quoted(), side_text(target)?.to_lowercase())
    };
    Some(problem("CfxEventNoHandler", Severity::Info, key_range(site), message))
}

fn not_safe_for_net(cx: &Ctx, db: &Db, site: &Site) -> Option<Problem> {
    if !site.function.is_standard() || site.net {
        return None;
    }
    let key = &site.key;
    let remote: Vec<&Site> =
        db.triggers_reaching(site).into_iter().filter(|t| !t.directions.has(Dirs::LOCAL)).collect();
    let first = remote.first()?;
    let registered = db
        .sites_of(key, false)
        .iter()
        .any(|o| o.net && o.info.resource == site.info.resource && compatible(o.side(), site.side()));
    let dynamic = db
        .related(key)
        .iter()
        .any(|k| db.dynamic.iter().any(|d| d.net && d.info.resource == site.info.resource && d.matches(k)));
    if registered || dynamic {
        return None;
    }
    let message = format!(
        "Event {} is triggered from the other side ({}) but not registered with RegisterNetEvent in this resource: the runtime drops it (\"event was not safe for net\")",
        key.quoted(),
        location(cx.ws, &first.file, first.line)
    );
    let mut p = problem("CfxEventNotSafeForNet", Severity::Warning, key_range(site), message);
    let (s, e) = callee_range(site);
    // a plain `AddEventHandler(…)` call (not one through a local saving it)
    if site.function.path == "AddEventHandler" && cx.doc.text().get(s..e) == Some("AddEventHandler") {
        p = p.fix(
            Fix::new("Use RegisterNetEvent (registers the event for the network)")
                .replace(s, e, "RegisterNetEvent")
                .preferred(),
        );
    }
    Some(p)
}

fn wrong_side(sites: &[&Site], site: &Site) -> Option<Problem> {
    let required = site.function.required_side?;
    let side = site.side();
    if !matches!(side, crate::project::Side::Client | crate::project::Side::Server) || compatible(required, side) {
        return None;
    }
    let first = sites.iter().find(|s| s.same_call(site) && s.function == site.function)?;
    if !std::ptr::eq(*first, site) {
        return None;
    }
    let message = format!(
        "'{}' must be called from a {} script, this is a {} script",
        site.function.path,
        side_text(required)?.to_lowercase(),
        side_text(side)?.to_lowercase()
    );
    Some(problem("CfxEventWrongSide", Severity::Warning, callee_range(site), message))
}

fn argument_count(cx: &Ctx, db: &Db, site: &Site, report_ignored: bool) -> Option<Problem> {
    let sent = site.payload_arity?;
    let handlers = db.handlers_reached_by(site);
    if handlers.is_empty() || handlers.iter().any(|h| h.callback_arity.is_none_or(|n| n == sent)) {
        return None;
    }
    let handler = handlers.iter().find(|h| h.callback_arity.is_some_and(|taken| taken > sent) || report_ignored)?;
    let message = format!(
        "Event {} sends {} but its handler at {} takes {}",
        site.key.quoted(),
        count(sent, "value", "values", Some("no value")),
        location(cx.ws, &handler.file, handler.line),
        handler.callback_arity?
    );
    Some(problem("CfxEventArgumentCount", Severity::Info, key_range(site), message))
}

fn duplicate_handler(cx: &Ctx, db: &Db, site: &Site) -> Option<Problem> {
    site.info.resource.as_ref()?;
    let key = &site.key;
    if key.is_builtin() || db.triggers(key).is_empty() {
        return None;
    }
    let first = db
        .sites_of(key, false)
        .into_iter()
        .filter(|s| {
            s.role == Role::Handler && s.info.resource == site.info.resource && compatible(s.side(), site.side())
        })
        .min_by(|a, b| (&a.file, a.call_offset).cmp(&(&b.file, b.call_offset)))?;
    if first.same_call(site) {
        return None;
    }
    let message = format!(
        "Event {} already has a handler in this resource and side ({})",
        key.quoted(),
        location(cx.ws, &first.file, first.line)
    );
    Some(problem("CfxEventDuplicateHandler", Severity::Info, key_range(site), message))
}

fn documented_function(src: &Source, tree: &Ast, comment: usize) -> Option<u32> {
    // the last comment of the block holding `comment`
    let mut last = comment;
    while let Some(next) = src.all.get(last + 1) {
        let cur = src.all[last];
        if next.kind != Tok::LineComment
            || !next.text(&src.text).starts_with("---")
            || src.lines.line_of(next.start) != src.lines.line_of(cur.start) + 1
        {
            break;
        }
        last += 1;
    }
    let end = src.all[last].end;
    let owner = src.toks.partition_point(|t| t.start < end);
    if owner >= src.toks.len() || !doc_block(src, owner).contains(&comment) {
        return None;
    }
    let owner = owner as u32;
    let first_closure = |first: u32, last: u32| {
        tree.exprs
            .iter()
            .filter(|e| matches!(e.kind, ExprKind::Func(_)) && e.first >= first && e.last <= last)
            .min_by_key(|e| e.first)
            .and_then(|e| if let ExprKind::Func(f) = e.kind { Some(f) } else { None })
    };
    for s in tree.stmts.iter().filter(|s| s.first == owner) {
        match &s.kind {
            StmtKind::Function { func, .. } | StmtKind::LocalFunction { func, .. } => return Some(*func),
            StmtKind::Local { .. } | StmtKind::Assign { .. } => return first_closure(s.first, s.last),
            _ => {}
        }
    }
    for e in &tree.exprs {
        if let ExprKind::Table(fields) = &e.kind
            && let Some(f) = fields.iter().find(|f| f.first == owner)
        {
            return first_closure(f.first, tree.expr(f.value).last);
        }
    }
    None
}

pub fn annotations(cx: &Ctx) -> Vec<Problem> {
    let src = &cx.doc.src;
    let mut out = Vec::new();
    let mut tree = None;
    for i in 0..src.all.len() {
        if src.all[i].kind != Tok::LineComment {
            continue;
        }
        let Some((start, end, text)) = event_tag(src, i) else { continue };
        let tree = tree.get_or_insert_with(|| ast::parse(src));
        let messages: Vec<String> = match documented_function(src, tree, i) {
            None => vec![ERROR_FUNCTION.into()],
            Some(f) => {
                let func = tree.func(f);
                let params: Vec<String> =
                    func.params.iter().map(|&d| src.tok_text(tree.decl(d).tok as usize).to_owned()).collect();
                match Spec::parse(&text, Some(&params), func.vararg) {
                    None => vec![ERROR_KIND.into()],
                    Some(spec) => spec.errors,
                }
            }
        };
        out.extend(messages.into_iter().map(|m| Problem::new("CfxEventAnnotation", Severity::Warning, start, end, m)));
    }
    out
}
