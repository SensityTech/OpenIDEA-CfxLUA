use super::ast::{self, ExprKind};
use super::db::Db;
use super::flow::{Analysis, Arg, FileFacts};
use super::model::{Dirs, EventFn, EventKey, FileInfo, Role, Seg, Site, side_text};
use super::{count, location, md};
use crate::docs::data::BuiltinEvent;
use crate::feature::Ctx;
use crate::project::{Game, Side};
use openidea_plugin::providers::{CompletionItem, CompletionKind, LineMarker};
use openidea_plugin::{Location, Range};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

const MAX_LOCATIONS: usize = 10;

struct Slot {
    function: Rc<EventFn>,
    role: Role,
    component: usize,
    logical: bool,
    arity: Option<usize>,
    prefix: String,
    suffix: String,
    fixed: HashMap<usize, String>,
}

impl Slot {
    fn accepts_network_names(&self) -> bool {
        !self.logical && self.prefix.is_empty() && self.suffix.is_empty()
    }

    fn target(&self) -> Option<Side> {
        let dirs = if self.role == Role::Trigger { self.function.directions } else { Dirs::NONE };
        match dirs {
            Dirs::SERVER => Some(Side::Server),
            Dirs::CLIENT => Some(Side::Client),
            _ => (self.function.required_side == Some(Side::Server)).then_some(Side::Server),
        }
    }
}

fn value_of(component: &[Seg], args: &[Arg]) -> Option<String> {
    let mut out = String::new();
    for seg in component {
        match seg {
            Seg::Lit(t) => out.push_str(t),
            Seg::Param(i) => match args.get(*i) {
                Some(Arg::Str { value, .. }) => out.push_str(value),
                _ => return None,
            },
        }
    }
    Some(out)
}

fn slot(function: Rc<EventFn>, args: &[Arg], index: usize) -> Option<Slot> {
    let role = function.role_for(args.len());
    let network = |function| Slot {
        function,
        role,
        component: 0,
        logical: false,
        arity: Some(1),
        prefix: String::new(),
        suffix: String::new(),
        fixed: HashMap::new(),
    };
    if role == Role::Alias && function.key.components.is_empty() {
        let wire = function.wire.unwrap_or(0);
        if index == wire {
            return Some(network(function));
        }
        let shifted = |i: usize| if i > wire { i - 1 } else { i };
        let fixed = args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != wire && *i < index)
            .filter_map(|(i, a)| match a {
                Arg::Str { value, .. } => Some((shifted(i), value.clone())),
                _ => None,
            })
            .collect();
        return Some(Slot {
            function,
            role,
            component: shifted(index),
            logical: true,
            arity: None,
            prefix: String::new(),
            suffix: String::new(),
            fixed,
        });
    }
    if role == Role::Alias && Some(index) == function.wire {
        return Some(network(function));
    }
    let components = &function.key.components;
    for (j, component) in components.iter().enumerate() {
        let params: Vec<usize> =
            component.iter().filter_map(|s| if let Seg::Param(i) = s { Some(*i) } else { None }).collect();
        if !params.contains(&index) {
            continue;
        }
        if params.len() != 1 {
            return None;
        }
        let position = component.iter().position(|s| matches!(s, Seg::Param(_)))?;
        let lits = |segs: &[Seg]| {
            segs.iter().filter_map(|s| if let Seg::Lit(t) = s { Some(t.as_str()) } else { None }).collect::<String>()
        };
        let (prefix, suffix) = (lits(&component[..position]), lits(&component[position + 1..]));
        let fixed = components[..j].iter().enumerate().filter_map(|(k, c)| value_of(c, args).map(|v| (k, v))).collect();
        let logical = function.key.logical;
        let arity = Some(components.len());
        return Some(Slot { function: function.clone(), role, component: j, logical, arity, prefix, suffix, fixed });
    }
    None
}

fn values(db: &Db, slot: &Slot) -> BTreeMap<String, Vec<EventKey>> {
    let mut out: BTreeMap<String, Vec<EventKey>> = BTreeMap::new();
    for key in db.keys() {
        if key.logical != slot.logical
            || slot.arity.is_some_and(|a| key.parts.len() != a)
            || slot.component >= key.parts.len()
        {
            continue;
        }
        if slot.fixed.iter().any(|(k, v)| key.parts.get(*k) != Some(v)) {
            continue;
        }
        let part = &key.parts[slot.component];
        if part.len() < slot.prefix.len() + slot.suffix.len()
            || !part.starts_with(&slot.prefix)
            || !part.ends_with(&slot.suffix)
        {
            continue;
        }
        let value = &part[slot.prefix.len()..part.len() - slot.suffix.len()];
        if value.trim().is_empty() || value.contains('\n') {
            continue;
        }
        out.entry(value.to_owned()).or_default().push(key.clone());
    }
    for keys in out.values_mut() {
        keys.sort_by_key(EventKey::order);
    }
    out
}

fn handled_text(db: &Db, keys: &[EventKey]) -> String {
    let handlers: Vec<&Site> = keys.iter().flat_map(|k| db.handlers(k)).collect();
    if handlers.is_empty() {
        return "not handled".into();
    }
    let mut sides: Vec<String> = handlers.iter().filter_map(|h| side_text(h.side())).map(str::to_lowercase).collect();
    sides.sort();
    sides.dedup();
    if sides.is_empty() { "handled".into() } else { format!("handled on {}", sides.join(", ")) }
}

fn standard_target(name: &str) -> Option<Option<Side>> {
    match name {
        "AddEventHandler" | "RegisterNetEvent" | "TriggerEvent" => Some(None),
        "RegisterServerEvent" | "TriggerServerEvent" | "TriggerLatentServerEvent" => Some(Some(Side::Server)),
        "TriggerClientEvent" | "TriggerLatentClientEvent" => Some(Some(Side::Client)),
        _ => None,
    }
}

fn project_games(cx: &Ctx) -> Vec<Game> {
    match cx.settings.string(crate::settings::keys::GAME, "auto").as_str() {
        "fivem" => vec![Game::Gta5],
        "redm" => vec![Game::Rdr3],
        "both" => Game::ALL.to_vec(),
        _ => {
            let detected = cx.ws.detected_games();
            if detected.is_empty() { Game::ALL.to_vec() } else { detected }
        }
    }
}

fn builtin_relevant(e: &BuiltinEvent, target: Option<Side>, side: Side, games: &[Game]) -> bool {
    let side_ok = match target {
        Some(t) => e.side() == t || e.side() == Side::Shared,
        None => e.side().visible_from(side),
    };
    side_ok && !e.is_pattern() && e.game().is_none_or(|g| games.contains(&g))
}

pub fn builtin_doc(e: &BuiltinEvent) -> String {
    let mut out = format!("event **{}**{}\n\n", md(&e.name), md(&e.params_text()));
    if !e.description.trim().is_empty() {
        out.push_str(e.description.trim());
        out.push_str("\n\n");
    }
    if !e.params.is_empty() {
        out.push_str("**Params:**  \n");
        let lines: Vec<String> = e
            .params
            .iter()
            .map(|p| {
                let d = p.description.trim();
                if d.is_empty() { format!("`{}: {}`", p.name, p.ty) } else { format!("`{}: {}` – {d}", p.name, p.ty) }
            })
            .collect();
        out.push_str(&lines.join("  \n"));
        out.push_str("\n\n");
    }
    let game = e.game().map_or("All games", Game::label);
    out.push_str(&format!(
        "**Side:** {}  \n**Game:** {game}  \n**Cancelable:** {}",
        e.side().label(),
        if e.cancelable { "Yes" } else { "No" }
    ));
    if !e.doc_url.is_empty() {
        out.push_str(&format!("\n\n[Online documentation]({})", e.doc_url));
    }
    out
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn builtin_item(e: &BuiltinEvent, replace: Range) -> CompletionItem {
    let mut item = CompletionItem::new(&e.name, CompletionKind::Event);
    item.detail = Some(format!("{} {}", e.side().label(), e.params_text()));
    item.replace = Some(replace);
    item.sort_text = Some(format!("0{}", e.name));
    item.documentation = Some(builtin_doc(e));
    item
}

fn project_item(label: &str, detail: String, replace: Range, documentation: Option<String>) -> CompletionItem {
    let mut item = CompletionItem::new(label, CompletionKind::Event);
    item.detail = Some(detail);
    item.replace = Some(replace);
    item.sort_text = Some(format!("1{label}"));
    item.documentation = documentation;
    item
}

pub fn completion(
    cx: &Ctx,
    db: Option<&Db>,
    files: &BTreeMap<String, FileFacts>,
    i: usize,
    at: usize,
) -> Vec<CompletionItem> {
    let src = &cx.doc.src;
    let tok = src.toks[i];
    let prefix = &src.text[tok.start + 1..at];
    let current = ast::raw_string(src, i);
    let replace = cx.doc.range(tok.start + 1, at);
    let tree = ast::parse(src);
    let Some(literal) = tree.exprs.iter().position(|e| e.kind == ExprKind::Str && e.first as usize == i) else {
        return Vec::new();
    };
    let found = tree.exprs.iter().enumerate().find_map(|(id, e)| match &e.kind {
        ExprKind::Call(callee, args) => {
            args.iter().position(|&a| a as usize == literal).map(|n| (id as u32, *callee, n))
        }
        _ => None,
    });
    let Some((call, callee, arg_index)) = found else { return Vec::new() };
    let games = project_games(cx);
    let mut offered: HashSet<String> = HashSet::new();
    let mut items = Vec::new();

    // the event analysis: helpers, redefinitions and the runtime functions
    if let Some(db) = db {
        let analysis = Analysis::new(src, &tree);
        if let Some(record) = analysis.record(call, false) {
            let info = FileInfo { side: cx.doc.side, resource: cx.doc.resource.as_deref().map(Rc::from) };
            let self_class = record.function.and_then(|f| analysis.functions()[f as usize].class_name());
            let resolved = db.resolve(cx.ws, &record, &cx.doc.path, &info, self_class);
            let slot = resolved.and_then(|(function, shift)| {
                let index = arg_index.checked_sub(shift)?;
                slot(function, &record.args[shift.min(record.args.len())..], index)
            });
            if let Some(slot) = slot {
                if slot.accepts_network_names() {
                    for e in crate::docs::data::events() {
                        if builtin_relevant(e, slot.target(), cx.doc.side, &games)
                            && starts_with_ci(&e.name, prefix)
                            && offered.insert(e.name.clone())
                        {
                            items.push(builtin_item(e, replace));
                        }
                    }
                }
                for (value, keys) in values(db, &slot) {
                    if value == current || !starts_with_ci(&value, prefix) || !offered.insert(value.clone()) {
                        continue;
                    }
                    let docs: Vec<String> = keys.iter().take(3).map(|k| key_doc(cx, db, k)).collect();
                    items.push(project_item(&value, handled_text(db, &keys), replace, Some(docs.join("\n\n---\n\n"))));
                }
                return items;
            }
        }
    }

    // no slot: the first argument of a plain call to a runtime function
    let Some(name) = tree.global(src, callee) else { return Vec::new() };
    let Some(target) = standard_target(name).filter(|_| arg_index == 0) else { return Vec::new() };
    for e in crate::docs::data::events() {
        if builtin_relevant(e, target, cx.doc.side, &games)
            && starts_with_ci(&e.name, prefix)
            && offered.insert(e.name.clone())
        {
            items.push(builtin_item(e, replace));
        }
    }
    let mut names: Vec<&str> = Vec::new();
    for facts in files.values() {
        for name in super::model::STANDARD_NAMES {
            for c in facts.calls_named(name) {
                if let Some(Arg::Str { value, lit: Some(_) }) = c.args.first() {
                    names.push(value);
                }
            }
        }
    }
    names.sort_unstable();
    for value in names {
        let len = value.chars().count();
        if (2..=128).contains(&len)
            && !value.contains('\n')
            && !value.trim().is_empty()
            && value != current
            && starts_with_ci(value, prefix)
            && offered.insert(value.to_owned())
        {
            items.push(project_item(value, "project event".into(), replace, None));
        }
    }
    items
}

fn targets_of<'a>(db: &'a Db, site: &Site) -> Vec<&'a Site> {
    let found = match site.role {
        Role::Trigger => {
            let reached = db.handlers_reached_by(site);
            if reached.is_empty() { db.handlers(&site.key) } else { reached }
        }
        Role::Handler => {
            let reaching = db.triggers_reaching(site);
            if reaching.is_empty() { db.triggers(&site.key) } else { reaching }
        }
        Role::Register | Role::Alias => db
            .sites_of(&site.key, true)
            .into_iter()
            .filter(|s| matches!(s.role, Role::Handler | Role::Trigger) && !s.same_call(site))
            .collect(),
    };
    distinct(found)
}

fn distinct(sites: Vec<&Site>) -> Vec<&Site> {
    let mut seen: HashSet<(&str, u32)> = HashSet::new();
    sites.into_iter().filter(|s| seen.insert((&s.file, s.call_offset))).collect()
}

fn anchor_location(site: &Site) -> Location {
    Location { path: site.file.to_string(), range: site.anchor().range }
}

fn site_at_caret<'a>(cx: &Ctx, db: &'a Db, at: usize) -> Option<(&'a Site, usize)> {
    let src = &cx.doc.src;
    let i = src.tok_at(at)?;
    let tok = src.toks[i];
    if !tok.is_string() {
        return None;
    }
    db.site_at(&cx.doc.path, tok.start as u32)
}

pub fn definition(cx: &Ctx, db: &Db, at: usize) -> Vec<Location> {
    let Some((site, _)) = site_at_caret(cx, db, at) else { return Vec::new() };
    // The NUI bridge answers on `AddEventHandler('__cfx_nui:…')`, with the page.
    if matches!(site.role, Role::Handler) && site.key.is_nui_callback() {
        return Vec::new();
    }
    targets_of(db, site).into_iter().map(anchor_location).collect()
}

fn located(cx: &Ctx, site: &Site) -> String {
    let loc = location(cx.ws, &site.file, site.line);
    match side_text(site.side()) {
        Some(side) => format!("{loc} ({side})"),
        None => loc,
    }
}

pub fn line_markers(cx: &Ctx, db: &Db) -> Vec<LineMarker> {
    let mut out: Vec<LineMarker> = Vec::new();
    for site in db.sites_in(&cx.doc.path) {
        let trigger = match site.role {
            Role::Trigger => true,
            Role::Handler => false,
            _ => continue,
        };
        let targets = if trigger { db.handlers_reached_by(site) } else { db.triggers_reaching(site) };
        let Some(first) = targets.first() else { continue };
        let line = site.anchor().range.start.line;
        if out.iter().any(|m| m.line == line) {
            continue;
        }
        let n = distinct(targets.clone()).len();
        let text = if trigger { count(n, "handler", "handlers", None) } else { count(n, "trigger", "triggers", None) };
        let mut lines = vec![format!("{text} {}", site.key.quoted())];
        lines.extend(targets.iter().take(MAX_LOCATIONS).map(|t| located(cx, t)));
        if targets.len() > MAX_LOCATIONS {
            lines.push("…".into());
        }
        out.push(LineMarker {
            line,
            icon: Some(if trigger { "implemented" } else { "implementing" }.into()),
            tooltip: lines.join("\n"),
            target: Some(anchor_location(first)),
        });
    }
    out
}

fn direction(site: &Site) -> Option<&'static str> {
    match site.directions {
        Dirs::SERVER => Some("to the server"),
        Dirs::CLIENT => Some("to clients"),
        Dirs::LOCAL => Some("local"),
        _ => None,
    }
}

fn locations(cx: &Ctx, sites: &[&Site], details: impl Fn(&Site) -> Option<String>) -> Option<String> {
    if sites.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = sites
        .iter()
        .take(MAX_LOCATIONS)
        .map(|s| {
            let via = (!s.function.is_standard()).then(|| format!("via {}", s.function.path));
            let parts: Vec<String> =
                side_text(s.side()).map(str::to_lowercase).into_iter().chain(details(s)).chain(via).collect();
            let loc = location(cx.ws, &s.file, s.line);
            md(&if parts.is_empty() { loc } else { format!("{loc} ({})", parts.join(", ")) })
        })
        .collect();
    if sites.len() > MAX_LOCATIONS {
        lines.push(format!("… {} more", sites.len() - MAX_LOCATIONS));
    }
    Some(lines.join("  \n"))
}

fn sides(sites: &[&Site]) -> Option<String> {
    let mut s: Vec<&str> = sites.iter().filter_map(|x| side_text(x.side())).collect();
    s.sort_unstable();
    s.dedup();
    (!s.is_empty()).then(|| s.join(", "))
}

pub fn key_doc(cx: &Ctx, db: &Db, key: &EventKey) -> String {
    let mut seen: HashSet<(&str, u32, &EventKey)> = HashSet::new();
    let sites: Vec<&Site> =
        db.sites_of(key, true).into_iter().filter(|s| seen.insert((&s.file, s.call_offset, &s.key))).collect();
    let handlers: Vec<&Site> = sites.iter().copied().filter(|s| s.role == Role::Handler).collect();
    let triggers: Vec<&Site> = sites.iter().copied().filter(|s| s.role == Role::Trigger).collect();
    let registrations: Vec<&Site> =
        sites.iter().copied().filter(|s| s.role == Role::Register || (s.role == Role::Handler && s.net)).collect();
    let mut aliases: Vec<EventKey> = db.related(key).into_iter().filter(|k| k != key).collect();
    aliases.sort_by_key(EventKey::order);

    let kind = if key.logical { "event" } else { "network event" };
    let mut out = format!("{kind} **{}**\n\n", md(&key.display()));
    out.push_str(&match (handlers.len(), triggers.len()) {
        (0, 0) => "Not handled nor triggered in this project.".to_owned(),
        (0, 1) => "Triggered once but handled nowhere in this project.".to_owned(),
        (0, n) => format!("Triggered {n} times but handled nowhere in this project."),
        (_, 0) => "Handled but never triggered in this project (maybe by the game, a framework or another resource)."
            .to_owned(),
        (h, t) => format!(
            "{}, {} in this project.",
            count(h, "handler", "handlers", None),
            count(t, "trigger", "triggers", None)
        ),
    });
    if key.logical {
        out.push_str("\n\nKey made of helper arguments: the helper computes the network name from it.");
    }
    let mut sections: Vec<(&str, String)> = Vec::new();
    if !aliases.is_empty() {
        sections.push(("Aliases:", md(&aliases.iter().map(EventKey::quoted).collect::<Vec<_>>().join(", "))));
    }
    if let Some(s) = sides(&handlers) {
        sections.push(("Handled on:", s));
    }
    if !registrations.is_empty() {
        sections.push(("Safe for net:", sides(&registrations).unwrap_or_else(|| "yes".into())));
    }
    let takes = |s: &Site| s.callback_arity.map(|n| format!("takes {}", count(n, "value", "values", Some("no value"))));
    if let Some(l) = locations(cx, &handlers, takes) {
        sections.push(("Handlers:", l));
    }
    let sends = |s: &Site| {
        let parts: Vec<String> = direction(s)
            .map(str::to_owned)
            .into_iter()
            .chain(s.payload_arity.map(|n| format!("sends {}", count(n, "value", "values", Some("no value")))))
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    };
    if let Some(l) = locations(cx, &triggers, sends) {
        sections.push(("Triggers:", l));
    }
    let registers: Vec<&Site> = sites.iter().copied().filter(|s| s.role == Role::Register).collect();
    if let Some(l) = locations(cx, &registers, |_| None) {
        sections.push(("Registrations:", l));
    }
    for (title, content) in sections {
        let sep = if content.contains('\n') { "  \n" } else { " " };
        out.push_str(&format!("\n\n**{title}**{sep}{content}"));
    }
    out
}
