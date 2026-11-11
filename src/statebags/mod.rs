pub mod sites;

use crate::events::{display_path, location, md};
use crate::feature::{Ctx, Feature, Fix, Problem};
use crate::lua::Source;
use crate::project::{FileKind, Side};
use crate::text::edit_distance;
use crate::workspace::Workspace;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use openidea_plugin::{Location, Range, Severity};
use sites::{ANY, Access, Analyzer, DYNAMIC, Element, Kind, NIL, Replication, Site};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

const MAX_LOCATIONS: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    key: String,
    kind: Kind,
    access: Access,
    replication: Replication,
    value_type: &'static str,
    range: Range,
}

#[derive(Clone, Debug)]
struct Occurrence {
    path: Rc<str>,
    kind: Kind,
    access: Access,
    replication: Replication,
    value_type: &'static str,
    range: Range,
}

#[derive(Default)]
struct Project {
    files: BTreeMap<String, Vec<Entry>>,
    all: BTreeMap<String, Vec<Occurrence>>,
    dirty: bool,
}

impl Project {
    fn rebuild(&mut self) {
        let mut all: BTreeMap<String, Vec<Occurrence>> = BTreeMap::new();
        for (path, entries) in &self.files {
            let path: Rc<str> = Rc::from(path.as_str());
            for e in entries {
                all.entry(e.key.clone()).or_default().push(Occurrence {
                    path: path.clone(),
                    kind: e.kind,
                    access: e.access,
                    replication: e.replication,
                    value_type: e.value_type,
                    range: e.range,
                });
            }
        }
        self.all = all;
        self.dirty = false;
    }

    fn occurrences(&self, key: &str, kind: Kind) -> Vec<&Occurrence> {
        self.all.get(key).into_iter().flatten().filter(|o| kind.matches(o.kind)).collect()
    }

    fn known_keys(&self, kind: Kind) -> Vec<(&str, Vec<&Occurrence>)> {
        self.all
            .iter()
            .filter(|(k, _)| k.as_str() != DYNAMIC)
            .filter_map(|(k, occs)| {
                let matching: Vec<&Occurrence> = occs.iter().filter(|o| kind.matches(o.kind)).collect();
                matching.iter().any(|o| o.access != Access::Read).then_some((k.as_str(), matching))
            })
            .collect()
    }

    fn known_key_names(&self, kind: Kind) -> impl Iterator<Item = &str> {
        self.all
            .iter()
            .filter(move |(k, occs)| {
                k.as_str() != DYNAMIC && occs.iter().any(|o| kind.matches(o.kind) && o.access != Access::Read)
            })
            .map(|(k, _)| k.as_str())
    }

    fn has_dynamic_writes(&self, kind: Kind) -> bool {
        self.occurrences(DYNAMIC, kind).iter().any(|o| o.access == Access::Write)
    }

    fn is_set(&self, key: &str) -> bool {
        self.all.get(key).is_some_and(|occs| occs.iter().any(|o| o.access != Access::Read))
    }
}

fn writes<'a>(occurrences: &[&'a Occurrence]) -> Vec<&'a Occurrence> {
    occurrences.iter().copied().filter(|o| o.access == Access::Write).collect()
}

fn value_type_names(writes: &[&Occurrence]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for w in writes {
        if !w.value_type.is_empty() && !names.contains(&w.value_type) {
            names.push(w.value_type);
        }
    }
    let values: Vec<&'static str> = names.iter().copied().filter(|n| *n != NIL).collect();
    if values.is_empty() { names } else { values }
}

fn type_text(writes: &[&Occurrence]) -> Option<String> {
    let names = value_type_names(writes);
    if names.is_empty() {
        return None;
    }
    let known: Vec<&str> = names.iter().copied().filter(|n| *n != ANY).collect();
    Some(if known.is_empty() { names.join("|") } else { known.join("|") })
}

fn is_replicated(ws: &Workspace, w: &Occurrence) -> Option<bool> {
    match w.replication {
        Replication::Replicated => Some(true),
        Replication::Local => Some(false),
        Replication::Unknown => None,
        Replication::BySide => match ws.side_of(&w.path) {
            Side::Server => Some(true),
            Side::Client => Some(false),
            _ => None,
        },
    }
}

fn tail_text(ws: &Workspace, occurrences: &[&Occurrence]) -> String {
    let writes = writes(occurrences);
    let shown: Vec<&Occurrence> = if writes.is_empty() {
        occurrences.iter().copied().filter(|o| o.access == Access::Handler).collect()
    } else {
        writes.clone()
    };
    let Some(first) = shown.first() else { return String::new() };
    let verb = if writes.is_empty() { "watched" } else { "set" };
    let mut files: Vec<&str> = shown.iter().map(|o| &*o.path).collect();
    files.dedup();
    let more = if files.len() > 1 { format!(" +{}", files.len() - 1) } else { String::new() };
    let replicated = if writes.is_empty() {
        ""
    } else if writes.iter().any(|w| is_replicated(ws, w) == Some(true)) {
        " (replicated)"
    } else if writes.iter().all(|w| is_replicated(ws, w) == Some(false)) {
        " (local)"
    } else {
        ""
    };
    format!(" {verb} in {}{more}{replicated}", display_path(ws, &first.path))
}

fn bag_sentence(kind: Kind) -> &'static str {
    match kind {
        Kind::Player => "Player state bag key, shared by `LocalPlayer.state` and `Player(id).state`.",
        Kind::Entity => "Entity state bag key: `Entity(handle).state`.",
        Kind::Global => "Global state bag key: `GlobalState`.",
        Kind::Unknown => "State bag key, in any kind of bag.",
    }
}

fn location_lines(ws: &Workspace, occurrences: &[&Occurrence], details: bool) -> Option<String> {
    if occurrences.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = occurrences
        .iter()
        .take(MAX_LOCATIONS)
        .map(|o| {
            let mut parts: Vec<String> = Vec::new();
            if matches!(ws.side_of(&o.path), Side::Client | Side::Server | Side::Shared) {
                parts.push(ws.side_of(&o.path).id().to_owned());
            }
            if o.kind != Kind::Unknown {
                parts.push(o.kind.name().to_owned());
            }
            if details {
                if !o.value_type.is_empty() && o.value_type != ANY {
                    parts.push(o.value_type.to_owned());
                }
                if let Some(r) = is_replicated(ws, o) {
                    parts.push(if r { "replicated" } else { "local" }.to_owned());
                }
            }
            let loc = location(ws, &o.path, o.range.start.line);
            md(&if parts.is_empty() { loc } else { format!("{loc} ({})", parts.join(", ")) })
        })
        .collect();
    if occurrences.len() > MAX_LOCATIONS {
        lines.push(format!("… {} more", occurrences.len() - MAX_LOCATIONS));
    }
    Some(lines.join("  \n"))
}

fn key_doc(ws: &Workspace, project: &Project, key: &str, kind: Kind) -> String {
    let occurrences = project.occurrences(key, kind);
    let writes = writes(&occurrences);
    let by = |a: Access| occurrences.iter().copied().filter(|o| o.access == a).collect::<Vec<_>>();
    let mut out = format!("{} key **{}**\n\n{}", kind.description(), md(key), bag_sentence(kind));
    if writes.is_empty() {
        out.push_str("\n\nNever set in this project.");
    }
    let mut sections: Vec<(&str, String)> = Vec::new();
    let types = value_type_names(&writes);
    if !types.is_empty() {
        sections.push(("Type:", types.join(" | ")));
    }
    if !writes.is_empty() {
        let mut values: Vec<Option<bool>> = Vec::new();
        for w in &writes {
            let r = is_replicated(ws, w);
            if !values.contains(&r) {
                values.push(r);
            }
        }
        let text = match values.as_slice() {
            [Some(true)] => "yes",
            [Some(false)] => "no (local to the side that sets it)",
            [None] => "unknown",
            _ => "depends on the write",
        };
        sections.push(("Replicated:", text.to_owned()));
    }
    for (title, occs, details) in [
        ("Set in:", writes.clone(), true),
        ("Change handlers:", by(Access::Handler), false),
        ("Read in:", by(Access::Read), false),
    ] {
        if let Some(lines) = location_lines(ws, &occs, details) {
            sections.push((title, lines));
        }
    }
    for (title, content) in sections {
        let sep = if content.contains('\n') { "  \n" } else { " " };
        out.push_str(&format!("\n\n**{title}**{sep}{content}"));
    }
    out
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[derive(Default)]
pub struct StateBagsFeature {
    projects: RefCell<HashMap<String, Project>>,
}

impl StateBagsFeature {
    fn with_project<R>(&self, cx: &Ctx, f: impl FnOnce(&Project) -> R) -> Option<R> {
        if !cx.settings.enabled() || !cx.ws.scanned || !cx.doc.is_lua() {
            return None;
        }
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(cx.ws.root.clone()).or_default();
        if p.dirty {
            p.rebuild();
        }
        Some(f(p))
    }

    /// The keys of the bag whose field or string holds `at`.
    fn keys_at(&self, cx: &Ctx, at: usize) -> Vec<CompletionItem> {
        self.with_project(cx, |p| {
            let src = &cx.doc.src;
            let ast = crate::events::ast::parse(src);
            let analyzer = Analyzer::new(src, &ast);
            let (kind, prefix_start, in_string) = match analyzer.field_at(at) {
                Some((kind, start)) => (kind, start, false),
                None => {
                    let Some(i) = src.quoted_at(at) else { return Vec::new() };
                    let tok = src.toks[i];
                    let sites = analyzer.sites();
                    let Some(site) =
                        sites.iter().find(|s| s.start == tok.start && matches!(s.element, Element::Str(_)))
                    else {
                        return Vec::new();
                    };
                    (site.kind, tok.start + 1, true)
                }
            };
            let prefix = &src.text[prefix_start..at];
            p.known_keys(kind)
                .into_iter()
                .filter(|(key, _)| (in_string || sites::is_identifier(key)) && starts_with_ci(key, prefix))
                .map(|(key, occurrences)| {
                    let mut item = CompletionItem::new(key, CompletionKind::Property);
                    let ty = type_text(&writes(&occurrences)).unwrap_or_else(|| "state bag key".into());
                    let tail = tail_text(cx.ws, &occurrences);
                    item.detail = Some(if tail.is_empty() { ty } else { format!("{ty} ·{tail}") });
                    item.sort_text = Some(format!("0{key}"));
                    item.documentation = Some(key_doc(cx.ws, p, key, kind));
                    if in_string {
                        item.replace = Some(cx.doc.range(prefix_start, at));
                    }
                    item
                })
                .collect()
        })
        .unwrap_or_default()
    }
}

fn site_at(sites: &[Site], at: usize) -> Option<&Site> {
    sites.iter().find(|s| s.key.is_some() && s.start <= at && at <= s.end)
}

/// Where a key is completed, read from the tokens: the name of a field or the content of a string, in a file
/// that may hold a bag.
fn completes_at(src: &Source, at: usize) -> bool {
    (src.names_field(at) || src.quoted_at(at).is_some()) && sites::may_hold_bags(&src.text)
}

fn entries(src: &Source) -> Vec<Entry> {
    if !sites::may_hold_bags(&src.text) {
        return Vec::new();
    }
    sites::analyze(src)
        .into_iter()
        .map(|s| Entry {
            key: s.key.unwrap_or_else(|| DYNAMIC.to_owned()),
            kind: s.kind,
            access: s.access,
            replication: s.replication,
            value_type: s.value_type,
            range: src.lines.range(&src.text, s.start, s.end),
        })
        .collect()
}

impl Feature for StateBagsFeature {
    fn index_file(&self, ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let entries = if kind == FileKind::Lua { entries(src) } else { Vec::new() };
        let mut projects = self.projects.borrow_mut();
        let p = projects.entry(ws.root.clone()).or_default();
        let old = p.files.get(path);
        if old.map_or(entries.is_empty(), |o| *o == entries) {
            return;
        }
        if entries.is_empty() {
            p.files.remove(path);
        } else {
            p.files.insert(path.to_owned(), entries);
        }
        p.dirty = true;
    }

    fn forget_file(&self, path: &str) {
        for p in self.projects.borrow_mut().values_mut() {
            if p.files.remove(path).is_some() {
                p.dirty = true;
            }
        }
    }

    fn indexed(&self, ws: &Workspace) {
        if let Some(p) = self.projects.borrow_mut().get_mut(&ws.root)
            && p.dirty
        {
            p.rebuild();
        }
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        // Asked at every keystroke: the file is only analysed where a key can be named.
        if completes_at(&cx.doc.src, at) { self.keys_at(cx, at) } else { Vec::new() }
    }

    fn completes_from_index(&self, cx: &Ctx, at: usize) -> bool {
        completes_at(&cx.doc.src, at)
    }

    fn definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        self.with_project(cx, |p| {
            let sites = sites::analyze(&cx.doc.src);
            let Some(site) = site_at(&sites, at) else { return Vec::new() };
            let key = site.key.as_deref().unwrap_or_default();
            let occurrences = p.occurrences(key, site.kind);
            let mut targets: Vec<&Occurrence> = writes(&occurrences);
            if targets.is_empty() {
                targets = occurrences.into_iter().filter(|o| o.access == Access::Handler).collect();
            }
            targets.into_iter().map(|o| Location { path: o.path.to_string(), range: o.range }).collect()
        })
        .unwrap_or_default()
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !cx.ws.contains(&cx.doc.path) {
            return Vec::new();
        }
        self.with_project(cx, |p| {
            // a resource only reading keys set by a framework is left alone
            if !p.all.iter().any(|(k, occs)| k != DYNAMIC && occs.iter().any(|o| o.access != Access::Read)) {
                return Vec::new();
            }
            let mut out = Vec::new();
            for site in sites::analyze(&cx.doc.src) {
                let Some(key) = site.key.as_deref() else { continue };
                if site.access != Access::Read || p.is_set(key) || p.has_dynamic_writes(site.kind) {
                    continue;
                }
                let mut problem = Problem::new(
                    "CfxStateBagUnknownKey",
                    Severity::Info,
                    site.start,
                    site.end,
                    format!("State bag key '{key}' is never set in this project"),
                );
                for candidate in suggestions(p, &site, key) {
                    let text = match site.element {
                        Element::Str(q) => {
                            let escaped = candidate.replace('\\', "\\\\").replace(q, &format!("\\{q}"));
                            format!("{q}{escaped}{q}")
                        }
                        _ => candidate.to_owned(),
                    };
                    problem = problem
                        .fix(Fix::new(format!("Replace with '{candidate}'")).replace(site.start, site.end, text));
                }
                out.push(problem);
            }
            out
        })
        .unwrap_or_default()
    }
}

fn suggestions<'a>(p: &'a Project, site: &Site, key: &str) -> Vec<&'a str> {
    let max = (key.chars().count() / 3).max(2);
    let mut found: Vec<(usize, &str)> = p
        .known_key_names(site.kind)
        .map(|c| (edit_distance(key, c, max), c))
        .filter(|(d, c)| *c != key && *d <= max && (site.element != Element::Ident || sites::is_identifier(c)))
        .collect();
    found.sort();
    found.into_iter().take(3).map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests;
