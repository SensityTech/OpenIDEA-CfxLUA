//! The lists of the six tabs: search, filters, rows (what the page draws for
//! each item), counts and empty texts.

use super::data;
use super::search::{self, Query};
use crate::natives::{Native, NativeSet, Natives};
use crate::project::{Game, Side};
use crate::runtime::{Decl, Kind, Part, Runtime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Natives,
    Runtime,
    Events,
    Manifest,
    Syntax,
    Guides,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Natives, Tab::Runtime, Tab::Events, Tab::Manifest, Tab::Syntax, Tab::Guides];

    /// Protocol id: `natives`, `runtime`, `events`, `manifest`, `syntax`, `guides`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Natives => "natives",
            Self::Runtime => "runtime",
            Self::Events => "events",
            Self::Manifest => "manifest",
            Self::Syntax => "syntax",
            Self::Guides => "guides",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// The filter combos and toggles of a tab (natives: set, namespace, side,
/// aliases, unnamed; events: side, game). Empty strings mean "all".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    /// `project`, `all`, `GTA5`, `RDR3`, `SERVER`.
    pub set: String,
    pub namespace: String,
    /// `client`, `server`, `shared`.
    pub side: String,
    /// `gta5`, `rdr3`.
    pub game: String,
    pub aliases: bool,
    pub unnamed: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Self { set: "project".into(), namespace: String::new(), side: String::new(), game: String::new(), aliases: true, unnamed: true }
    }
}

impl Filters {
    /// The natives sets the set filter designates (`project`: `project`, or every set when empty).
    pub fn sets(&self, project: &[NativeSet]) -> Vec<NativeSet> {
        match self.set.as_str() {
            "all" => NativeSet::ALL.to_vec(),
            id => match NativeSet::parse(id) {
                Some(s) => vec![s],
                None if project.is_empty() => NativeSet::ALL.to_vec(),
                None => project.to_vec(),
            },
        }
    }
}

/// One list row: a bold or plain label, then text parts with CSS classes
/// (`muted`, `small`, `side client`…).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Row {
    pub id: String,
    pub label: String,
    /// `fn`, `event`, `prop`, `tpl`, `help`, `class`, `var`, `method`, `field`, `local`, `part`.
    #[serde(skip_serializing_if = "str::is_empty")]
    pub icon: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<(String, &'static str)>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tip: String,
    /// `category` (guides, not selectable for actions), `part` (runtime).
    #[serde(skip_serializing_if = "str::is_empty")]
    pub kind: &'static str,
    /// Runtime tree: depth, whether it has children, initially expanded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub children: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub expanded: bool,
}

/// The items of a search; natives rows are built when a page of them is asked.
pub enum Listing {
    Natives { keys: Vec<(NativeSet, String)>, multi: bool },
    Rows(Vec<Row>),
}

impl Listing {
    pub fn len(&self) -> usize {
        match self {
            Self::Natives { keys, .. } => keys.len(),
            Self::Rows(rows) => rows.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn id(&self, i: usize) -> Option<String> {
        match self {
            Self::Natives { keys, .. } => keys.get(i).map(|(s, n)| format!("{}:{n}", s.id())),
            Self::Rows(rows) => rows.get(i).map(|r| r.id.clone()),
        }
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        match self {
            Self::Natives { keys, .. } => {
                let (set, name) = id.split_once(':')?;
                let set = NativeSet::parse(set)?;
                keys.iter().position(|(s, n)| *s == set && n == name)
            }
            // Else the first row named `id` (runtime: `CreateThread` for `CreateThread(routine)`).
            Self::Rows(rows) => rows.iter().position(|r| r.id == id).or_else(|| {
                rows.iter().position(|r| r.label == id || r.label.strip_prefix(id).is_some_and(|rest| rest.starts_with('(')))
            }),
        }
    }

    /// The first row an initial selection may land on (not a guide category).
    pub fn first_selectable(&self) -> Option<usize> {
        match self {
            Self::Natives { .. } => (!self.is_empty()).then_some(0),
            Self::Rows(rows) => rows.iter().position(|r| r.kind != "category"),
        }
    }

    /// Rows `offset..offset + limit`.
    pub fn rows(&self, natives: &Natives, offset: usize, limit: usize) -> Vec<Row> {
        match self {
            Self::Natives { keys, multi } => keys
                .iter()
                .skip(offset)
                .take(limit)
                .filter_map(|(s, n)| natives.catalog(*s).get(n))
                .map(|n| native_row(n, *multi))
                .collect(),
            Self::Rows(rows) => rows.iter().skip(offset).take(limit).cloned().collect(),
        }
    }
}

/// A search's outcome.
pub struct Results {
    pub listing: Listing,
    /// `9 natives`; empty for the runtime tree.
    pub count: String,
    /// What the empty list says.
    pub empty: String,
    /// Natives: namespaces of the searched sets.
    pub namespaces: Option<Vec<String>>,
}

/// `12,345`.
pub fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn nothing(text: &str) -> String {
    if text.trim().is_empty() { "Nothing to show".into() } else { format!("Nothing matches '{text}'") }
}

/// Lower-case search words (split on spaces and tabs).
fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split([' ', '\t']).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

fn all_in(words: &[String], haystack: &str) -> bool {
    words.iter().all(|w| haystack.contains(w.as_str()))
}

/// The list of `tab` for search `text` and filters `f`. `project` are the
/// natives sets of the project.
pub fn list(tab: Tab, natives: &Natives, rt: &Runtime, project: &[NativeSet], text: &str, f: &Filters) -> Results {
    match tab {
        Tab::Natives => natives_list(natives, project, text, f),
        Tab::Runtime => {
            let rows = runtime_rows(rt, text);
            let empty = if text.trim().is_empty() { "The runtime stubs were not found".into() } else { nothing(text) };
            Results { listing: Listing::Rows(rows), count: String::new(), empty, namespaces: None }
        }
        Tab::Events => rows_result(event_rows(text, f), text, "events"),
        Tab::Manifest => rows_result(directive_rows(text), text, "directives"),
        Tab::Syntax => rows_result(syntax_rows(text), text, "features"),
        Tab::Guides => {
            let rows = guide_rows(text);
            let guides = rows.iter().filter(|r| r.kind != "category").count();
            Results { listing: Listing::Rows(rows), count: format!("{} guides", group(guides)), empty: nothing(text), namespaces: None }
        }
    }
}

fn rows_result(rows: Vec<Row>, text: &str, what: &str) -> Results {
    let count = format!("{} {what}", group(rows.len()));
    Results { listing: Listing::Rows(rows), count, empty: nothing(text), namespaces: None }
}

fn natives_list(natives: &Natives, project: &[NativeSet], text: &str, f: &Filters) -> Results {
    let sets = f.sets(project);
    let q = Query {
        text: text.to_owned(),
        namespace: (!f.namespace.is_empty()).then(|| f.namespace.clone()),
        sides: Side::parse(&f.side).map(|s| vec![s]),
        include_aliases: f.aliases,
        include_unnamed: f.unnamed,
        limit: usize::MAX,
    };
    let keys: Vec<(NativeSet, String)> =
        search::search(natives, &sets, &q).into_iter().map(|(n, _)| (n.set, n.lua_name.clone())).collect();
    let generated = sets.iter().any(|s| !natives.catalog(*s).is_empty());
    let empty = if generated { nothing(text) } else { "Natives not generated yet".into() };
    let count = format!("{} natives", group(keys.len()));
    Results { listing: Listing::Natives { keys, multi: sets.len() > 1 }, count, empty, namespaces: Some(search::namespaces(natives, &sets)) }
}

fn side_class(side: Side) -> &'static str {
    match side {
        Side::Client => "side client",
        Side::Server => "side server",
        _ => "side shared",
    }
}

/// `GetEntityCoords  ENTITY  Client  FiveM  0x3FEF770D40960D5A`.
pub fn native_row(n: &Native, multi: bool) -> Row {
    let mut parts = vec![(format!("  {}", n.namespace), "muted"), (format!("  {}", n.side_label()), side_class(n.apiset))];
    if multi && n.set != NativeSet::Server {
        parts.push((format!("  {}", n.set.short_name()), "muted small"));
    }
    parts.push((format!("  {}", n.hash_display()), "muted small"));
    Row {
        id: n.key(),
        label: n.lua_name.clone(),
        icon: "fn",
        bold: true,
        strike: n.is_alias(),
        parts,
        tip: n.typed_signature(),
        ..Row::default()
    }
}

fn event_rows(text: &str, f: &Filters) -> Vec<Row> {
    let words = words(text);
    let side = Side::parse(&f.side);
    let game = Game::from_manifest(&f.game);
    let mut found: Vec<(&data::BuiltinEvent, bool)> = data::events()
        .iter()
        .filter(|e| side.is_none_or(|s| e.side() == s || e.side() == Side::Shared))
        .filter(|e| game.is_none_or(|g| e.game().is_none_or(|eg| eg == g)))
        .filter_map(|e| {
            let (name, desc) = (e.name.to_lowercase(), e.description.to_lowercase());
            words.iter().all(|w| name.contains(w.as_str()) || desc.contains(w.as_str())).then(|| (e, all_in(&words, &name)))
        })
        .collect();
    found.sort_by_key(|(_, in_name)| !in_name);
    found
        .into_iter()
        .map(|(e, _)| {
            let mut parts = vec![(e.params_text(), "muted"), (format!("  {}", e.side().label()), side_class(e.side()))];
            if let Some(g) = e.game() {
                parts.push((format!("  {}", g.label()), "muted small"));
            }
            Row { id: e.name.clone(), label: e.name.clone(), icon: "event", bold: true, parts, ..Row::default() }
        })
        .collect()
}

fn directive_rows(text: &str) -> Vec<Row> {
    let words = words(text);
    let mut found: Vec<(&data::Directive, bool)> = data::directives()
        .iter()
        .filter_map(|d| {
            let desc = d.description.to_lowercase();
            words.iter().all(|w| d.name.contains(w.as_str()) || desc.contains(w.as_str())).then(|| (d, all_in(&words, &d.name)))
        })
        .collect();
    found.sort_by_key(|(d, in_name)| (d.deprecated, !in_name));
    found
        .into_iter()
        .map(|(d, _)| Row {
            id: d.name.clone(),
            label: d.name.clone(),
            icon: "prop",
            bold: true,
            strike: d.deprecated,
            parts: vec![(format!("  {}", d.value_type_text()), "muted")],
            ..Row::default()
        })
        .collect()
}

fn syntax_rows(text: &str) -> Vec<Row> {
    let words = words(text);
    data::syntax_features()
        .iter()
        .filter(|f| all_in(&words, &format!("{}\n{}\n{}\n{}", f.title, f.syntax, f.description, f.example).to_lowercase()))
        .map(|f| {
            let first = f.syntax.split(" | ").next().unwrap_or("");
            Row { id: f.id.clone(), label: f.title.clone(), icon: "tpl", bold: true, parts: vec![(format!("  {first}"), "muted")], ..Row::default() }
        })
        .collect()
}

fn guide_row(g: &data::Guide) -> Row {
    Row { id: g.id.clone(), label: g.title.clone(), icon: "help", tip: g.summary.clone(), ..Row::default() }
}

/// Categories (index order) and their guides; a search lists the matching guides only.
fn guide_rows(text: &str) -> Vec<Row> {
    let words = words(text);
    let guides = data::guides();
    if words.is_empty() {
        let mut categories: Vec<&str> = Vec::new();
        for g in guides {
            if !categories.contains(&g.category.as_str()) {
                categories.push(&g.category);
            }
        }
        let mut rows = Vec::new();
        for c in categories {
            rows.push(Row { id: format!("category:{c}"), label: c.to_uppercase(), bold: true, kind: "category", ..Row::default() });
            rows.extend(guides.iter().filter(|g| g.category == c).map(guide_row));
        }
        return rows;
    }
    let mut found: Vec<(&data::Guide, bool)> = guides
        .iter()
        .filter(|g| {
            let all = format!("{}\n{}\n{}\n{}", g.title, g.keywords.join(" "), g.summary, g.markdown).to_lowercase();
            all_in(&words, &all)
        })
        .map(|g| (g, all_in(&words, &g.title.to_lowercase())))
        .collect();
    found.sort_by_key(|(_, in_title)| !in_title);
    found
        .into_iter()
        .map(|(g, _)| {
            let mut row = guide_row(g);
            row.parts.push((format!("  {}", g.category), "muted small"));
            row
        })
        .collect()
}

/// What a runtime tree id designates.
pub enum RuntimeNode {
    Part(Part),
    Decl(usize),
}

/// The label of a tree node and the declaration whose children it shows: a
/// variable with a single variable child is shown merged (`a.b`).
fn compressed(rt: &Runtime, mut d: usize) -> (String, usize) {
    let mut label = rt.decl(d).label();
    loop {
        let decl = rt.decl(d);
        match decl.children.as_slice() {
            [only] if decl.is_var() && rt.decl(*only).is_var() => {
                label = format!("{label}.{}", rt.decl(*only).label());
                d = *only;
            }
            _ => return (label, d),
        }
    }
}

fn icon(d: &Decl) -> &'static str {
    match d.kind {
        Kind::Class => "class",
        Kind::Global => "var",
        Kind::Local => "local",
        Kind::Function => "fn",
        Kind::Method => "method",
        Kind::Field => "field",
    }
}

/// The runtime tree, filtered: a matching node keeps all its descendants, a
/// node with a matching descendant is kept; parts without matches are hidden.
pub fn runtime_rows(rt: &Runtime, text: &str) -> Vec<Row> {
    let needle = text.trim().to_lowercase();
    let mut rows = Vec::new();
    for part in Part::ALL {
        let tops = &rt.tops[part as usize];
        let mut nodes = Vec::new();
        let at = Level { path: part.label(), depth: 1, top: true, expand: !needle.is_empty() };
        for node in siblings(rt, tops) {
            add_node(rt, node, &at, &needle, &mut nodes);
        }
        if needle.is_empty() || !nodes.is_empty() {
            rows.push(Row {
                id: part.label().into(),
                label: part.label().into(),
                icon: "part",
                bold: true,
                kind: "part",
                depth: Some(0),
                children: !tops.is_empty(),
                expanded: !tops.is_empty(),
                ..Row::default()
            });
            rows.extend(nodes);
        }
    }
    rows
}

/// A tree node: its id segment (the label, `label#2` for a second sibling
/// with the same label), its label, its declaration and the declaration
/// whose children it shows (see [`compressed`]).
struct Node {
    segment: String,
    label: String,
    decl: usize,
    shown: usize,
}

fn siblings(rt: &Runtime, decls: &[usize]) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(decls.len());
    for &decl in decls {
        let (label, shown) = compressed(rt, decl);
        let same = out.iter().filter(|n| n.label == label).count();
        let segment = if same == 0 { label.clone() } else { format!("{label}#{}", same + 1) };
        out.push(Node { segment, label, decl, shown });
    }
    out
}

/// Where a tree node goes: its parent's id, its depth, whether it is top-level
/// (shows its file) and whether it starts expanded (a search expands everything).
struct Level<'a> {
    path: &'a str,
    depth: usize,
    top: bool,
    expand: bool,
}

/// Adds `node` (and its kept descendants) when it or a descendant matches `needle`.
fn add_node(rt: &Runtime, node: Node, at: &Level, needle: &str, out: &mut Vec<Row>) -> bool {
    let id = format!("{}/{}", at.path, node.segment);
    let matches = needle.is_empty() || node.label.to_lowercase().contains(needle);
    let mut children = Vec::new();
    let mut any = false;
    let below = Level { path: &id, depth: at.depth + 1, top: false, expand: at.expand };
    for child in siblings(rt, &rt.decl(node.shown).children) {
        any |= add_node(rt, child, &below, if matches { "" } else { needle }, &mut children);
    }
    if !matches && !any {
        return false;
    }
    let parts = if at.top { vec![(format!("  {}", rt.decl(node.decl).file), "muted small")] } else { Vec::new() };
    out.push(Row {
        icon: icon(rt.decl(node.shown)),
        parts,
        depth: Some(at.depth),
        children: !children.is_empty(),
        expanded: at.expand && !children.is_empty(),
        id,
        label: node.label,
        ..Row::default()
    });
    out.extend(children);
    true
}

/// The node of tree id `Part/Name/Name`, else the first declaration labelled `id`.
pub fn runtime_node(rt: &Runtime, id: &str) -> Option<RuntimeNode> {
    let mut segments = id.split('/');
    let first = segments.next()?;
    if let Some(part) = Part::ALL.into_iter().find(|p| p.label() == first) {
        let mut level: &[usize] = &rt.tops[part as usize];
        let mut node = None;
        for seg in segments {
            let shown = siblings(rt, level).into_iter().find(|n| n.segment == seg)?.shown;
            node = Some(shown);
            level = &rt.decl(shown).children;
        }
        return Some(node.map_or(RuntimeNode::Part(part), RuntimeNode::Decl));
    }
    (0..rt.decls.len()).find(|&d| rt.decl(d).label() == id || rt.decl(d).name == id).map(RuntimeNode::Decl)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::search::tests::fixture;
    use crate::runtime::runtime;

    fn ids(r: &Results, natives: &Natives) -> Vec<String> {
        r.listing.rows(natives, 0, usize::MAX).into_iter().map(|row| row.id).collect()
    }

    #[test]
    fn natives_tab_of_a_fivem_project() {
        let n = Natives::from_natives(fixture());
        let project = [NativeSet::Gta5, NativeSet::Server];
        let r = list(Tab::Natives, &n, runtime(), &project, "", &Filters::default());
        assert_eq!(r.listing.len(), 9, "GTA5 8 + DropPlayer, the shared native once");
        assert_eq!(r.count, "9 natives");
        assert_eq!(r.namespaces.as_deref().unwrap(), ["CFX", "ENTITY", "MISC", "PLAYER"]);
        let rows = r.listing.rows(&n, 0, 3);
        assert_eq!(rows.len(), 3);
        let row = r.listing.rows(&n, 0, 20).into_iter().find(|r| r.id == "GTA5:GetEntityCoords").unwrap();
        let parts: Vec<&str> = row.parts.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(parts, ["  ENTITY", "  Client", "  FiveM", "  0x3FEF770D40960D5A"]);
        assert_eq!(row.tip, "GetEntityCoords(entity: integer, alive: boolean): vector3");
        let all = Filters { set: "all".into(), ..Filters::default() };
        assert_eq!(list(Tab::Natives, &n, runtime(), &project, "", &all).listing.len(), 10);
        let server = Filters { set: "SERVER".into(), ..Filters::default() };
        let r = list(Tab::Natives, &n, runtime(), &project, "", &server);
        assert_eq!(ids(&r, &n), ["SERVER:DropPlayer", "SERVER:GetCurrentResourceName"]);
        let parts: Vec<String> = r.listing.rows(&n, 0, 1)[0].parts.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(parts, ["  CFX", "  Server", "  0xBA0613E1"], "one set: no set name");
        let r = list(Tab::Natives, &n, runtime(), &project, "zzz", &Filters::default());
        assert_eq!((r.listing.len(), r.empty.as_str()), (0, "Nothing matches 'zzz'"));
        assert_eq!(r.listing.position("GTA5:GetEntityCoords"), None);
        let empty = Natives::from_natives(Vec::new());
        assert_eq!(list(Tab::Natives, &empty, runtime(), &[], "", &Filters::default()).empty, "Natives not generated yet");
        assert_eq!(group(12_345_678), "12,345,678");
        assert_eq!(group(999), "999");
    }

    #[test]
    fn events_manifest_syntax_and_guides() {
        let n = Natives::from_natives(Vec::new());
        let rt = runtime();
        let all = list(Tab::Events, &n, rt, &[], "", &Filters::default());
        assert_eq!(all.listing.len(), data::events().len());
        let client = Filters { side: "client".into(), ..Filters::default() };
        let r = list(Tab::Events, &n, rt, &[], "", &client);
        assert!(r.listing.position("onClientResourceStart").is_some() && r.listing.position("onResourceStart").is_some());
        assert!(r.listing.position("playerConnecting").is_none());
        let redm = Filters { game: "rdr3".into(), ..Filters::default() };
        let redm = list(Tab::Events, &n, rt, &[], "", &redm);
        let fivem_only = data::events().iter().find(|e| e.game() == Some(Game::Gta5)).unwrap();
        assert!(redm.listing.position(&fivem_only.name).is_none() && redm.listing.position("onResourceStart").is_some());
        let r = list(Tab::Events, &n, rt, &[], "resource start", &Filters::default());
        assert!(r.listing.len() >= 2 && r.count.ends_with(" events"));
        let row = &r.listing.rows(&n, 0, 1)[0];
        assert!(row.label.to_lowercase().contains("resource") && row.label.to_lowercase().contains("start"), "{}", row.label);
        let m = list(Tab::Manifest, &n, rt, &[], "", &Filters::default());
        let rows = m.listing.rows(&n, 0, 100);
        let first_deprecated = rows.iter().position(|r| r.strike).unwrap();
        assert!(rows[first_deprecated..].iter().all(|r| r.strike), "deprecated directives last");
        assert_eq!(m.count, format!("{} directives", data::directives().len()));
        let s = list(Tab::Syntax, &n, rt, &[], "", &Filters::default());
        assert_eq!(s.listing.rows(&n, 0, 1)[0].parts[0].0, "  target += expr");
        assert_eq!(s.count, "12 features");
        let g = list(Tab::Guides, &n, rt, &[], "", &Filters::default());
        let rows = g.listing.rows(&n, 0, 100);
        assert_eq!(rows.len(), data::guides().len() + 6);
        assert_eq!((rows[0].id.as_str(), rows[0].label.as_str(), rows[0].kind), ("category:Getting started", "GETTING STARTED", "category"));
        assert_eq!(g.listing.first_selectable(), Some(1));
        assert_eq!(g.count, format!("{} guides", data::guides().len()));
        let found = list(Tab::Guides, &n, rt, &[], "state bags", &Filters::default());
        assert_eq!(found.listing.id(0).as_deref(), Some("state-bags"));
        let joaat = list(Tab::Guides, &n, rt, &[], "joaat", &Filters::default());
        assert!(joaat.listing.position("cfxlua-std-extensions").is_some());
    }

    #[test]
    fn runtime_tree() {
        let rt = runtime();
        let rows = runtime_rows(rt, "");
        let parts: Vec<&str> = rows.iter().filter(|r| r.kind == "part").map(|r| r.label.as_str()).collect();
        assert_eq!(parts, ["Shared", "Client", "Server", "Manifest"]);
        assert!(rows.iter().filter(|r| r.kind != "part").all(|r| !r.expanded), "a blank search expands the parts only");
        let citizen = rows.iter().find(|r| r.id == "Shared/Citizen").unwrap();
        assert_eq!((citizen.icon, citizen.parts[0].0.as_str(), citizen.children), ("var", "  citizen.lua", true));
        assert!(rows.iter().any(|r| r.id == "Shared/promise" && r.icon == "class"), "class of a local table");
        assert!(rows.iter().any(|r| r.id == "Shared/promise#2" && r.icon == "var"), "the global of the same name");
        let Some(RuntimeNode::Decl(d)) = runtime_node(rt, "Shared/promise#2") else { panic!() };
        assert_eq!(rt.decl(d).doc.ty.as_deref(), Some("PromiseLib"));
        assert!(rows.iter().any(|r| r.id == "Shared/Citizen/CreateThread(routine)" && r.depth == Some(2)));
        let found = runtime_rows(rt, "createthread");
        let labels: Vec<&str> = found.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Citizen") && labels.contains(&"CreateThread(routine)") && labels.contains(&"CreateThreadNow(routine, name)"));
        assert!(!labels.contains(&"Client") && !labels.contains(&"Wait(ms)"), "{labels:?}");
        let json = runtime_rows(rt, "json");
        assert!(json.iter().any(|r| r.label == "encode(value, options)"), "a matching node keeps its children");
        assert!(matches!(runtime_node(rt, "Shared"), Some(RuntimeNode::Part(Part::Shared))));
        let Some(RuntimeNode::Decl(d)) = runtime_node(rt, "Shared/Citizen/CreateThread(routine)") else { panic!() };
        assert_eq!(rt.decl(d).qualified(), "Citizen.CreateThread");
        let Some(RuntimeNode::Decl(d)) = runtime_node(rt, "PerformHttpRequest") else { panic!() };
        assert_eq!(rt.decl(d).part, Part::Server);
        assert!(runtime_node(rt, "Shared/Nope").is_none());
        let r = list(Tab::Runtime, &Natives::from_natives(Vec::new()), rt, &[], "zzz", &Filters::default());
        assert_eq!((r.listing.len(), r.empty.as_str()), (0, "Nothing matches 'zzz'"));
    }

    #[test]
    fn single_child_variables_are_merged() {
        let rt = Runtime::parse(&[(Part::Shared, "a.lua", "one = {}\none.two = {}\none.two.three = nil\n")]);
        let rows = runtime_rows(&rt, "");
        assert_eq!(rows[1].label, "one.two.three");
        assert!(matches!(runtime_node(&rt, "Shared/one.two.three"), Some(RuntimeNode::Decl(_))));
    }
}
