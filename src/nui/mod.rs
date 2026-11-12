mod answer;
mod bridge;
mod build;
mod js;
mod lua;
mod manifest;
mod web;

pub use web::NUI_EVENT_PREFIX;

use crate::feature::{Ctx, Doc, Feature, Intention, Problem};
use crate::lua::Source;
use crate::project::FileKind;
use crate::settings::Settings;
use crate::text::{file_name, parent};
use crate::workspace::Workspace;
use bridge::{Bridge, Channel, Input, Keys, Role, Site, WrapKind};
use build::Configs;
use lua::{LuaCall, LuaKind};
use openidea_plugin::providers::{CompletionItem, CompletionKind, LineMarker};
use openidea_plugin::{Location, Severity};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub mod keys {
    pub const MESSAGE_KEYS: &str = "nui.messageKeys";
    pub const FETCH_FUNCTIONS: &str = "nui.fetchFunctions";
    pub const LISTENER_FUNCTIONS: &str = "nui.listenerFunctions";
    pub const DEV_SERVER_PORT: &str = "nui.devServerPort";
}

const DEFAULT_PORT: u16 = 5173;
const MAX_LOCATIONS: usize = 10;

fn read_keys(settings: &Settings) -> Keys {
    Keys {
        messages: settings.list(keys::MESSAGE_KEYS, "action, type"),
        fetch: settings.list(keys::FETCH_FUNCTIONS, "fetchNui"),
        listen: settings.list(keys::LISTENER_FUNCTIONS, "useNuiEvent"),
    }
}

fn dev_port(settings: &Settings) -> u16 {
    u16::try_from(settings.int(keys::DEV_SERVER_PORT, i64::from(DEFAULT_PORT)))
        .ok()
        .filter(|p| *p >= 1)
        .unwrap_or(DEFAULT_PORT)
}

pub struct NuiFeature {
    state: RefCell<State>,
}

struct State {
    lua: BTreeMap<String, lua::Facts>,
    web: BTreeMap<String, web::Facts>,
    configs: Configs,
    calls: HashMap<String, Rc<[LuaCall]>>,
    resources: HashMap<String, Option<String>>,
    bridge: Option<(Keys, Rc<Bridge>)>,
    keys: Keys,
    dirty: bool,
}

impl Default for NuiFeature {
    fn default() -> Self {
        Self {
            state: RefCell::new(State {
                lua: BTreeMap::new(),
                web: BTreeMap::new(),
                configs: Configs::default(),
                calls: HashMap::new(),
                resources: HashMap::new(),
                bridge: None,
                keys: read_keys(&Settings::defaults()),
                dirty: true,
            }),
        }
    }
}

fn skipped(path: &str) -> bool {
    parent(path).split('/').any(|s| matches!(s, "node_modules" | "bower_components" | ".git"))
}

fn extension(path: &str) -> &str {
    file_name(path).rsplit_once('.').map_or("", |(_, e)| e)
}

impl State {
    fn resource_of(&self, ws: &Workspace, path: &str) -> Option<String> {
        resource_of(ws, &self.configs, path)
    }

    fn rebuild(&mut self, ws: &Workspace, keys: Keys) -> Rc<Bridge> {
        let built = {
            let State { lua, web, configs, calls, resources, .. } = self;
            let configs: &Configs = configs;
            let mut resource =
                |path: &str| resources.entry(path.to_owned()).or_insert_with(|| resource_of(ws, configs, path)).clone();
            let mut calls_of = |path: &str| -> Rc<[LuaCall]> {
                if let Some(c) = calls.get(path) {
                    return c.clone();
                }
                let read: Rc<[LuaCall]> = match ws.read(path) {
                    Some(text) => {
                        let src = Source::new(&text);
                        lua::calls(&lua::Analysis::new(&src)).into()
                    }
                    None => Rc::from(Vec::new()),
                };
                calls.insert(path.to_owned(), read.clone());
                read
            };
            Bridge::build(Input { ws, keys: &keys, lua, web, resource_of: &mut resource, calls_of: &mut calls_of })
        };
        let built = Rc::new(built);
        self.bridge = Some((keys, built.clone()));
        self.dirty = false;
        built
    }
}

fn resource_of(ws: &Workspace, configs: &Configs, path: &str) -> Option<String> {
    if let Some(r) = ws.resource_of(path) {
        return Some(r.dir.clone());
    }
    (crate::project::file_kind(path) == FileKind::Web).then(|| configs.build_target(ws, path)).flatten()
}

fn display_path(ws: &Workspace, path: &str) -> String {
    if let Some(r) = ws.resource_of(path)
        && let Some(rel) = r.relative(path)
    {
        return format!("{}/{rel}", r.name);
    }
    if ws.contains(path) { ws.relative(path).to_owned() } else { file_name(path).to_owned() }
}

fn location_text(ws: &Workspace, s: &Site) -> String {
    format!("{}:{}", display_path(ws, &s.file), s.at.range.start.line + 1)
}

fn location(s: &Site) -> Location {
    Location { path: s.file.to_string(), range: s.at.range }
}

fn matches_prefix(prefix: &str, name: &str) -> bool {
    let prefix = prefix.to_ascii_lowercase();
    let lower = name.to_ascii_lowercase();
    let mut prev: Option<char> = None;
    name.char_indices().any(|(i, c)| {
        let word =
            prev.is_none_or(|p| (c.is_uppercase() && !p.is_uppercase()) || matches!(p, '_' | '-' | ':' | '.' | '/'));
        prev = Some(c);
        word && lower[i..].starts_with(&prefix)
    })
}

impl NuiFeature {
    fn bridge(&self, ws: &Workspace, settings: &Settings) -> Rc<Bridge> {
        let keys = read_keys(settings);
        let mut st = self.state.borrow_mut();
        // Rebuilt in `indexed` (heavy calls): a query keeps the bridge it has.
        if let Some((k, b)) = &st.bridge
            && *k == keys
        {
            return b.clone();
        }
        st.keys = keys.clone();
        st.rebuild(ws, keys)
    }

    fn lua_string(doc: &Doc, at: usize) -> Option<usize> {
        let k = doc.src.tok_at(at)?;
        doc.src.toks[k].is_string().then_some(k)
    }

    fn lua_completion(&self, cx: &Ctx, at: usize) -> Vec<CompletionItem> {
        let doc = cx.doc;
        let src = &doc.src;
        let Some(k) = src.quoted_at(at) else { return Vec::new() };
        let t = src.toks[k];
        let keys = read_keys(cx.settings);
        let an = lua::Analysis::new(src);
        let bridge = self.bridge(cx.ws, cx.settings);
        let role = match lua::slot(&an, k, &keys.messages) {
            Some(lua::Slot::Message) => Role::Listener,
            Some(lua::Slot::Callback) => Role::Fetch,
            Some(lua::Slot::Wrapper { path, colon, arg }) => match bridge.lua_wrapper_for(&doc.path, &path, colon, arg)
            {
                Some(LuaKind::Callback) => Role::Fetch,
                Some(LuaKind::Message) => Role::Listener,
                None => return Vec::new(),
            },
            None => return Vec::new(),
        };
        let current = lua::string_value(src, k).map(|(v, _)| v);
        let resource = cx.ws.resource_of(&doc.path).map(|r| r.dir.clone());
        items(doc, &bridge, role, resource.as_deref(), current.as_deref(), t.start + 1, at)
    }

    fn web_completion(&self, cx: &Ctx, at: usize) -> Vec<CompletionItem> {
        let doc = cx.doc;
        let keys = read_keys(cx.settings);
        let Some((slot, start)) = web::slot_at(doc.text(), extension(&doc.path), at, &keys.messages) else {
            return Vec::new();
        };
        let bridge = self.bridge(cx.ws, cx.settings);
        let own = self.state.borrow().resource_of(cx.ws, &doc.path);
        let (role, resource) = match slot {
            web::Slot::Url { host: None } => (Role::Registration, own),
            web::Slot::Url { host: Some(h) } => {
                let named = if h.to_ascii_lowercase().starts_with("cfx-nui-") {
                    None
                } else if own.as_deref().is_some_and(|o| file_name(o) == h) {
                    own
                } else {
                    cx.ws.resources().find(|r| r.name == h).map(|r| r.dir.clone())
                };
                match named {
                    Some(r) => (Role::Registration, Some(r)),
                    None => return Vec::new(),
                }
            }
            web::Slot::Message => (Role::Send, own),
            web::Slot::Argument { callee, receiver, arg } => {
                match bridge.web_wrapper_for(&doc.path, &callee, receiver.as_deref(), arg) {
                    Some(WrapKind::Fetch) => (Role::Registration, own),
                    Some(WrapKind::Listen) => (Role::Send, own),
                    None => return Vec::new(),
                }
            }
        };
        items(doc, &bridge, role, resource.as_deref(), None, start, at)
    }

    fn not_registered(&self, cx: &Ctx) -> Vec<Problem> {
        let bridge = self.bridge(cx.ws, cx.settings);
        let mut out: Vec<Problem> = Vec::new();
        for s in bridge.sites_in(&cx.doc.path).filter(|s| s.role == Role::Fetch) {
            let resource = s.resource.as_deref();
            if out.iter().any(|p| p.start == s.at.marked().0)
                || !bridge.has_registrations(resource)
                || bridge.registers_dynamically(resource)
                || !bridge.counterparts(s).is_empty()
            {
                continue;
            }
            let message = match resource {
                Some(r) => format!(
                    "NUI callback '{}' is not registered by the client scripts of '{}' (RegisterNUICallback): the request fails",
                    s.name,
                    file_name(r)
                ),
                None => format!(
                    "NUI callback '{}' is not registered by any client script (RegisterNUICallback): the request fails",
                    s.name
                ),
            };
            let (start, end) = s.at.marked();
            out.push(Problem::new("CfxNuiCallbackNotRegistered", Severity::Info, start, end, message));
        }
        out
    }
}

fn items(
    doc: &Doc,
    bridge: &Bridge,
    role: Role,
    resource: Option<&str>,
    current: Option<&str>,
    start: usize,
    at: usize,
) -> Vec<CompletionItem> {
    let prefix = doc.text().get(start..at).unwrap_or("");
    let replace = doc.range(start, at);
    let mut out = Vec::new();
    for (name, sites) in bridge.names(role, resource) {
        if Some(name) == current || name.trim().is_empty() || name.contains('\n') || !matches_prefix(prefix, name) {
            continue;
        }
        let mut files: Vec<&str> = sites.iter().map(|s| file_name(&s.file)).collect();
        files.sort_unstable();
        files.dedup();
        let place = if files.len() == 1 { files[0].to_owned() } else { format!("{} files", files.len()) };
        let mut item = CompletionItem::new(
            name,
            if role.channel() == Channel::Callback { CompletionKind::Function } else { CompletionKind::Event },
        );
        item.detail = Some(format!("{} ({place})", role.text()));
        item.replace = Some(replace);
        item.sort_text = Some(format!("0{name}"));
        out.push(item);
    }
    out
}

impl Feature for NuiFeature {
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut st = self.state.borrow_mut();
        match kind {
            FileKind::Lua => {
                let facts = lua::Facts::read(src);
                let called = st.calls.remove(path).is_some();
                if called || st.lua.get(path) != Some(&facts) {
                    st.lua.insert(path.to_owned(), facts);
                    st.dirty = true;
                }
            }
            FileKind::Web => {
                let read = !skipped(path) && !web::is_generated(file_name(path), &src.text);
                if read {
                    let facts = web::scan(src, extension(path));
                    if st.web.get(path) != Some(&facts) {
                        st.web.insert(path.to_owned(), facts);
                        st.dirty = true;
                    }
                } else if st.web.remove(path).is_some() {
                    st.dirty = true;
                }
            }
            FileKind::BundlerConfig => {
                st.configs.set(path, &src.text);
                st.resources.clear();
                st.dirty = true;
            }
            // Resources moved: every file may belong elsewhere.
            FileKind::Manifest => {
                st.resources.clear();
                st.dirty = true;
            }
            FileKind::Other => {}
        }
    }

    fn forget_file(&self, path: &str) {
        let mut st = self.state.borrow_mut();
        st.lua.remove(path);
        st.web.remove(path);
        st.calls.remove(path);
        st.configs.remove(path);
        st.resources.clear();
        st.dirty = true;
    }

    fn indexed(&self, ws: &Workspace) {
        let mut st = self.state.borrow_mut();
        if st.dirty {
            let keys = st.keys.clone();
            st.rebuild(ws, keys);
        }
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !cx.settings.enabled() {
            return Vec::new();
        }
        match cx.doc.kind {
            FileKind::Lua => self.lua_completion(cx, at),
            FileKind::Web => self.web_completion(cx, at),
            _ => Vec::new(),
        }
    }

    fn completes_from_index(&self, cx: &Ctx, at: usize) -> bool {
        cx.doc.kind != FileKind::Lua || cx.doc.src.quoted_at(at).is_some()
    }

    fn definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        if !cx.settings.enabled() || !matches!(cx.doc.kind, FileKind::Lua | FileKind::Web) {
            return Vec::new();
        }
        let bridge = self.bridge(cx.ws, cx.settings);
        let site = match cx.doc.kind {
            FileKind::Lua => {
                Self::lua_string(cx.doc, at).and_then(|k| bridge.lua_site_at(&cx.doc.path, cx.doc.src.toks[k].start))
            }
            _ => bridge.web_site_at(&cx.doc.path, at),
        };
        site.map(|s| bridge.counterparts(s).into_iter().map(location).collect()).unwrap_or_default()
    }

    fn line_markers(&self, cx: &Ctx) -> Vec<LineMarker> {
        if !cx.settings.enabled() || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let bridge = self.bridge(cx.ws, cx.settings);
        let mut by_line: BTreeMap<u32, LineMarker> = BTreeMap::new();
        for s in bridge.sites_in(&cx.doc.path).filter(|s| s.lua) {
            let counterparts = bridge.counterparts(s);
            let Some(first) = counterparts.first() else { continue };
            let n = counterparts.len();
            let (text, icon) = match (s.role.channel(), n) {
                (Channel::Callback, 1) => ("1 call from the page".to_owned(), "implemented"),
                (Channel::Callback, _) => (format!("{n} calls from the page"), "implemented"),
                (Channel::Message, 1) => ("1 listener in the page".to_owned(), "implementing"),
                (Channel::Message, _) => (format!("{n} listeners in the page"), "implementing"),
            };
            let mut tooltip = format!("{text} '{}'", s.name);
            for c in counterparts.iter().take(MAX_LOCATIONS) {
                tooltip.push('\n');
                tooltip.push_str(&location_text(cx.ws, c));
            }
            if n > MAX_LOCATIONS {
                tooltip.push_str("\n…");
            }
            let line = cx.doc.position(s.literal).line;
            match by_line.get_mut(&line) {
                // One marker per line: the first site's target, every site's text.
                Some(m) => {
                    m.tooltip.push_str("\n\n");
                    m.tooltip.push_str(&tooltip);
                }
                None => {
                    by_line.insert(
                        line,
                        LineMarker { line, icon: Some(icon.to_owned()), tooltip, target: Some(location(first)) },
                    );
                }
            }
        }
        by_line.into_values().collect()
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !cx.settings.enabled() {
            return Vec::new();
        }
        match cx.doc.kind {
            FileKind::Lua => answer::check(&lua::Analysis::new(&cx.doc.src)),
            FileKind::Web => self.not_registered(cx),
            FileKind::Manifest => manifest::files_missing(cx.doc, cx.ws, &self.state.borrow().configs),
            _ => Vec::new(),
        }
    }

    fn intentions(&self, cx: &Ctx, start: usize, _end: usize) -> Vec<Intention> {
        if !cx.settings.enabled() || !cx.doc.is_manifest() {
            return Vec::new();
        }
        manifest::dev_mode(cx.doc, &self.state.borrow().configs, dev_port(cx.settings), start).into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
