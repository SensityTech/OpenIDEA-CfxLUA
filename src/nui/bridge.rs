use super::lua::{self, Arg, FnKind, LuaCall, LuaKind, simple_name};
use super::web;
use crate::lua::Source;
use crate::text::file_name;
use crate::workspace::Workspace;
use openidea_plugin::Range;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct At {
    pub start: usize,
    pub len: usize,
    pub exact: bool,
    pub end: usize,
    pub range: Range,
}

impl At {
    pub fn new(src: &Source, start: usize, len: usize, exact: bool, end: usize) -> Self {
        let range = src.lines.range(&src.text, start, if exact { start + len } else { start });
        Self { start, len, exact, end, range }
    }

    pub fn marked(&self) -> (usize, usize) {
        if self.exact { (self.start, self.start + self.len) } else { (self.start, self.end.max(self.start)) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Callback,
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Registration,
    Fetch,
    Send,
    Listener,
}

impl Role {
    pub fn channel(self) -> Channel {
        match self {
            Self::Registration | Self::Fetch => Channel::Callback,
            Self::Send | Self::Listener => Channel::Message,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Registration => Self::Fetch,
            Self::Fetch => Self::Registration,
            Self::Send => Self::Listener,
            Self::Listener => Self::Send,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Registration => "callback registration",
            Self::Fetch => "callback call",
            Self::Send => "message sent",
            Self::Listener => "message handler",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WrapKind {
    Fetch,
    Listen,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub file: Rc<str>,
    pub role: Role,
    pub name: String,
    pub lua: bool,
    pub at: At,
    pub literal: usize,
    pub resource: Option<Rc<str>>,
}

pub fn compatible(a: Option<&str>, b: Option<&str>) -> bool {
    a.is_none() || b.is_none() || a == b
}

#[derive(Clone, Debug)]
struct LuaWrapper {
    file: Rc<str>,
    kind: LuaKind,
    path: String,
    fkind: FnKind,
    param: usize,
    field: Option<String>,
}

impl LuaWrapper {
    fn class(&self) -> Option<&str> {
        (self.fkind == FnKind::Method).then(|| self.path.rsplit_once(':').map_or("", |(c, _)| c))
    }

    fn called_by(&self, call: &LuaCall, file: &str) -> bool {
        match self.fkind {
            FnKind::Global | FnKind::Field => !call.colon && call.callee == self.path,
            FnKind::Local => *self.file == *file && !call.colon && call.callee == self.path,
            FnKind::Method => {
                let class = self.class();
                call.colon
                    && simple_name(&call.callee) == simple_name(&self.path)
                    && (call.receiver_class.as_deref() == class
                        || call.function.as_ref().and_then(|f| f.class()) == class
                        || call.receiver() == class)
            }
        }
    }
}

#[derive(Clone, Debug)]
struct JsWrapper {
    kind: WrapKind,
    name: String,
    container: Option<String>,
    param: usize,
    file: Option<Rc<str>>,
}

struct Info {
    module: bool,
    imports: Vec<(String, String)>,
    resource: Option<Rc<str>>,
}

impl Info {
    fn import(&self, local: &str) -> Option<&str> {
        self.imports.iter().find(|(l, _)| l == local).map(|(_, i)| i.as_str())
    }
}

struct Resolver {
    wrappers: Vec<JsWrapper>,
    infos: HashMap<Rc<str>, Info>,
}

impl Resolver {
    fn import(&self, file: &str, local: &str) -> Option<&str> {
        self.infos.get(file).and_then(|i| i.import(local))
    }

    fn module(&self, file: &str) -> bool {
        self.infos.get(file).is_some_and(|i| i.module)
    }

    fn resource(&self, file: &str) -> Option<&str> {
        self.infos.get(file).and_then(|i| i.resource.as_deref())
    }

    fn wrapper_for(&self, file: &str, callee: &str, receiver: Option<&str>, arg: usize) -> Option<WrapKind> {
        let name = match self.import(file, callee) {
            Some(i) if receiver.is_none() && i != "default" && i != "*" => i,
            _ => callee,
        };
        self.wrappers
            .iter()
            .filter(|w| w.name == name && w.param == arg)
            .find(|w| self.matches(w, file, callee, receiver))
            .map(|w| w.kind)
    }

    fn matches(&self, w: &JsWrapper, file: &str, callee: &str, receiver: Option<&str>) -> bool {
        let Some(home) = &w.file else { return true };
        let same_scope = **home == *file
            || (!self.module(file) && !self.module(home) && compatible(self.resource(file), self.resource(home)));
        match (&w.container, receiver) {
            (None, Some(r)) => self.import(file, r) == Some("*"),
            (None, None) => same_scope || self.import(file, callee).is_some(),
            (Some(_), None) => false,
            (Some(_), Some("this")) => **home == *file,
            (Some(c), Some(r)) => c == r && (same_scope || self.import(file, r).is_some()),
        }
    }

    fn callee_names(&self) -> HashSet<String> {
        let mut names: HashSet<String> = self.wrappers.iter().map(|w| w.name.clone()).collect();
        let aliases: Vec<String> = self
            .infos
            .values()
            .flat_map(|i| &i.imports)
            .filter(|(_, imported)| names.contains(imported))
            .map(|(l, _)| l.clone())
            .collect();
        names.extend(aliases);
        names
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys {
    pub messages: Vec<String>,
    pub fetch: Vec<String>,
    pub listen: Vec<String>,
}

impl Keys {
    fn message(&self, field: Option<&str>) -> bool {
        field.is_some_and(|f| self.messages.iter().any(|k| k == f))
    }
}

/// The calls a file makes of the wrappers named like the hash given; a file that has to be read gives at once
/// the calls of every name of the sorted list.
pub type CallsOf<'a> = dyn FnMut(&str, u64, &[u64]) -> Rc<[LuaCall]> + 'a;

pub struct Input<'a> {
    pub ws: &'a Workspace,
    pub keys: &'a Keys,
    pub lua: &'a BTreeMap<String, lua::Facts>,
    pub web: &'a BTreeMap<String, web::Facts>,
    pub resource_of: &'a mut dyn FnMut(&str) -> Option<String>,
    pub calls_of: &'a mut CallsOf<'a>,
}

const MAX_WRAPPERS: usize = 200;
const MAX_FORWARD_ROUNDS: usize = 6;

pub struct Bridge {
    pub sites: Vec<Site>,
    by_file: HashMap<Rc<str>, Vec<usize>>,
    by_name: HashMap<(Channel, String), Vec<usize>>,
    dynamic: Vec<Option<Rc<str>>>,
    resolver: Resolver,
    lua_wrappers: Vec<LuaWrapper>,
}

impl Bridge {
    pub fn build(input: Input) -> Self {
        let mut b = Builder { input, resources: HashMap::new(), sites: Vec::new(), dynamic: Vec::new() };
        let lua_wrappers = b.lua();
        let resolver = b.web();
        let Builder { sites, dynamic, .. } = b;
        let mut seen = HashSet::new();
        let sites: Vec<Site> =
            sites.into_iter().filter(|s| seen.insert((s.file.clone(), s.at.start, s.role))).collect();
        let mut by_file: HashMap<Rc<str>, Vec<usize>> = HashMap::new();
        let mut by_name: HashMap<(Channel, String), Vec<usize>> = HashMap::new();
        for (i, s) in sites.iter().enumerate() {
            by_file.entry(s.file.clone()).or_default().push(i);
            by_name.entry((s.role.channel(), s.name.clone())).or_default().push(i);
        }
        Self { sites, by_file, by_name, dynamic, resolver, lua_wrappers }
    }

    pub fn sites_in<'a>(&'a self, file: &str) -> impl Iterator<Item = &'a Site> + 'a {
        self.by_file.get(file).into_iter().flatten().map(|&i| &self.sites[i])
    }

    pub fn lua_site_at(&self, file: &str, literal: usize) -> Option<&Site> {
        self.sites_in(file).find(|s| s.lua && s.literal == literal)
    }

    pub fn web_site_at(&self, file: &str, offset: usize) -> Option<&Site> {
        self.sites_in(file).find(|s| !s.lua && s.at.start <= offset && offset <= s.at.start + s.at.len)
    }

    pub fn sites(&self, channel: Channel, name: &str, resource: Option<&str>) -> impl Iterator<Item = &Site> + '_ {
        let resource = resource.map(str::to_owned);
        self.by_name
            .get(&(channel, name.to_owned()))
            .into_iter()
            .flatten()
            .map(|&i| &self.sites[i])
            .filter(move |s| compatible(s.resource.as_deref(), resource.as_deref()))
    }

    pub fn counterparts(&self, site: &Site) -> Vec<&Site> {
        let mut seen = HashSet::new();
        self.sites(site.role.channel(), &site.name, site.resource.as_deref())
            .filter(|s| s.role == site.role.opposite() && seen.insert((s.file.clone(), s.at.start)))
            .collect()
    }

    pub fn registers_dynamically(&self, resource: Option<&str>) -> bool {
        self.dynamic.iter().any(|d| compatible(d.as_deref(), resource))
    }

    pub fn has_registrations(&self, resource: Option<&str>) -> bool {
        self.sites.iter().any(|s| s.role == Role::Registration && compatible(s.resource.as_deref(), resource))
    }

    pub fn names(&self, role: Role, resource: Option<&str>) -> BTreeMap<&str, Vec<&Site>> {
        let mut out: BTreeMap<&str, Vec<&Site>> = BTreeMap::new();
        for s in self.sites.iter().filter(|s| s.role == role && compatible(s.resource.as_deref(), resource)) {
            out.entry(s.name.as_str()).or_default().push(s);
        }
        out
    }

    pub fn web_wrapper_for(&self, file: &str, callee: &str, receiver: Option<&str>, arg: usize) -> Option<WrapKind> {
        self.resolver.wrapper_for(file, callee, receiver, arg)
    }

    pub fn lua_wrapper_for(&self, file: &str, path: &str, colon: bool, arg: usize) -> Option<LuaKind> {
        self.lua_wrappers
            .iter()
            .find(|w| {
                w.param == arg
                    && match w.fkind {
                        FnKind::Method => colon && simple_name(path) == simple_name(&w.path),
                        FnKind::Local => !colon && path == w.path && *w.file == *file,
                        _ => !colon && path == w.path,
                    }
            })
            .map(|w| w.kind)
    }
}

struct Builder<'a> {
    input: Input<'a>,
    resources: HashMap<String, Option<Rc<str>>>,
    sites: Vec<Site>,
    dynamic: Vec<Option<Rc<str>>>,
}

impl Builder<'_> {
    fn resource(&mut self, file: &str) -> Option<Rc<str>> {
        if let Some(r) = self.resources.get(file) {
            return r.clone();
        }
        let r: Option<Rc<str>> = (self.input.resource_of)(file).map(Rc::from);
        self.resources.insert(file.to_owned(), r.clone());
        r
    }

    fn site(&mut self, file: &Rc<str>, role: Role, name: &str, lua: bool, at: &At, literal: usize) {
        let resource = self.resource(file);
        self.sites.push(Site {
            file: file.clone(),
            role,
            name: name.to_owned(),
            lua,
            at: at.clone(),
            literal,
            resource,
        });
    }

    fn mark_dynamic(&mut self, file: &str) {
        let r = self.resource(file);
        if !self.dynamic.contains(&r) {
            self.dynamic.push(r);
        }
    }

    fn lua(&mut self) -> Vec<LuaWrapper> {
        let input_lua = self.input.lua;
        let keys = self.input.keys;
        let mut wrappers = Vec::new();
        for (path, facts) in input_lua {
            let file: Rc<str> = Rc::from(path.as_str());
            for c in &facts.callbacks {
                self.site(&file, Role::Registration, &c.name, true, &c.at, c.literal);
            }
            for (field, m) in &facts.messages {
                if keys.message(Some(field)) {
                    self.site(&file, Role::Send, &m.name, true, &m.at, m.literal);
                }
            }
            for w in &facts.wrappers {
                if w.kind == LuaKind::Callback || keys.message(w.field.as_deref()) {
                    wrappers.push(LuaWrapper {
                        file: file.clone(),
                        kind: w.kind,
                        path: w.function.path.clone(),
                        fkind: w.function.kind,
                        param: w.param,
                        field: w.field.clone(),
                    });
                }
            }
            if facts.dynamic {
                self.mark_dynamic(path);
            }
        }
        self.resolve_lua_wrappers(&mut wrappers);
        wrappers
    }

    fn resolve_lua_wrappers(&mut self, wrappers: &mut Vec<LuaWrapper>) {
        let input_lua = self.input.lua;
        let mut known: HashSet<(String, usize, LuaKind)> =
            wrappers.iter().map(|w| (w.path.clone(), w.param, w.kind)).collect();
        let mut names: Vec<u64> = wrappers.iter().map(|w| lua::name_hash(simple_name(&w.path))).collect();
        names.sort_unstable();
        names.dedup();
        let mut i = 0;
        while i < wrappers.len() && i < MAX_WRAPPERS {
            let w = wrappers[i].clone();
            i += 1;
            let hash = lua::name_hash(simple_name(&w.path));
            let role = if w.kind == LuaKind::Callback { Role::Registration } else { Role::Send };
            for (path, facts) in input_lua {
                if facts.call_names.binary_search(&hash).is_err() {
                    continue;
                }
                let file: Rc<str> = Rc::from(path.as_str());
                let calls = (self.input.calls_of)(path, hash, &names);
                for call in calls.iter().filter(|c| w.called_by(c, path)) {
                    match call.args.get(w.param) {
                        Some(Arg::Str(n)) => self.site(&file, role, &n.name, true, &n.at, n.literal),
                        Some(Arg::Const(value)) => {
                            let at = At { len: value.len(), ..call.at.clone() };
                            self.site(&file, role, value, true, &at, call.at.start);
                        }
                        Some(Arg::Param(index)) => {
                            if let Some(f) = &call.function
                                && known.insert((f.path.clone(), *index, w.kind))
                            {
                                wrappers.push(LuaWrapper {
                                    file: file.clone(),
                                    kind: w.kind,
                                    path: f.path.clone(),
                                    fkind: f.kind,
                                    param: *index,
                                    field: w.field.clone(),
                                });
                                let found = lua::name_hash(simple_name(&f.path));
                                if let Err(at) = names.binary_search(&found) {
                                    names.insert(at, found);
                                }
                            }
                        }
                        Some(Arg::Other) if w.kind == LuaKind::Callback => self.mark_dynamic(path),
                        _ => {}
                    }
                }
            }
        }
    }

    fn web(&mut self) -> Resolver {
        let input_web = self.input.web;
        let keys = self.input.keys;
        let mut wrappers: Vec<JsWrapper> = Vec::new();
        for name in &keys.fetch {
            wrappers.push(JsWrapper {
                kind: WrapKind::Fetch,
                name: name.clone(),
                container: None,
                param: 0,
                file: None,
            });
        }
        for name in &keys.listen {
            wrappers.push(JsWrapper {
                kind: WrapKind::Listen,
                name: name.clone(),
                container: None,
                param: 0,
                file: None,
            });
        }
        let mut infos = HashMap::new();
        let mut forwards = Vec::new();
        for (path, facts) in input_web {
            let file: Rc<str> = Rc::from(path.as_str());
            for u in &facts.urls {
                self.url_site(&file, u);
            }
            for l in &facts.listens {
                if keys.message(Some(&l.field)) {
                    self.site(&file, Role::Listener, &l.name, false, &l.at, l.at.start);
                }
            }
            for r in &facts.registrations {
                self.site(&file, Role::Registration, &r.name, false, &r.at, r.at.start);
            }
            // A hook comparing another field to its parameter (`e.data.requestId === id`) is no message hook.
            for w in &facts.wrappers {
                if w.kind == WrapKind::Fetch || keys.message(w.field.as_deref()) {
                    wrappers.push(JsWrapper {
                        kind: w.kind,
                        name: w.name.clone(),
                        container: w.container.clone(),
                        param: w.param,
                        file: Some(file.clone()),
                    });
                }
            }
            for f in &facts.forwards {
                forwards.push((file.clone(), f));
            }
            if facts.dynamic > 0 {
                self.mark_dynamic(path);
            }
            let resource = self.resource(path);
            infos.insert(file, Info { module: facts.module, imports: facts.imports.clone(), resource });
        }
        let mut resolver = Resolver { wrappers, infos };
        // Functions forwarding their parameter to a wrapper are wrappers too.
        for _ in 0..MAX_FORWARD_ROUNDS {
            let mut added: Vec<JsWrapper> = Vec::new();
            for (file, f) in &forwards {
                let Some(kind) = resolver.wrapper_for(file, &f.callee, f.receiver.as_deref(), f.arg) else { continue };
                let exists = resolver.wrappers.iter().chain(&added).any(|w| {
                    w.file.as_deref() == Some(&**file)
                        && w.name == f.name
                        && w.container == f.container
                        && w.param == f.param
                });
                if !exists {
                    added.push(JsWrapper {
                        kind,
                        name: f.name.clone(),
                        container: f.container.clone(),
                        param: f.param,
                        file: Some(file.clone()),
                    });
                }
            }
            if added.is_empty() {
                break;
            }
            resolver.wrappers.extend(added);
        }
        let names = resolver.callee_names();
        for (path, facts) in input_web {
            let file: Rc<str> = Rc::from(path.as_str());
            for c in facts.calls.iter().filter(|c| names.contains(&c.callee)) {
                let role = match resolver.wrapper_for(path, &c.callee, c.receiver.as_deref(), c.arg) {
                    Some(WrapKind::Fetch) => Role::Fetch,
                    Some(WrapKind::Listen) => Role::Listener,
                    None => continue,
                };
                self.site(&file, role, &c.value, false, &c.at, c.at.start);
            }
        }
        resolver
    }

    fn url_site(&mut self, file: &Rc<str>, u: &web::Url) {
        let own = self.resource(file);
        let resource = match &u.host {
            None => own,
            Some(h) if own.as_deref().is_some_and(|o| file_name(o) == h) => own,
            Some(h) => match self.input.ws.resources().find(|r| r.name == *h) {
                Some(r) => Some(Rc::from(r.dir.as_str())),
                None => return,
            },
        };
        self.sites.push(Site {
            file: file.clone(),
            role: Role::Fetch,
            name: u.name.clone(),
            lua: false,
            at: u.at.clone(),
            literal: u.at.start,
            resource,
        });
    }
}
