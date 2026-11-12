use super::flow::{Arg, CallRecord, CallbackCall, FileFacts, FunctionInfo, merge_literals};
use super::model::{
    Dirs, DynamicSite, EventFn, EventKey, FileInfo, FnKind, KeySpec, Occurrence, Origin, Role, STANDARD_NAMES, Seg,
    Site, Sources, VARARG, compatible, is_standard_name, standard,
};
use super::spec::{Spec, settings_helper};
use crate::project::{CASE_SENSITIVE, Side, glob::Glob};
use crate::workspace::Workspace;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;

const MAX_ROUNDS: usize = 12;
const MAX_DERIVATIONS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub detect: bool,
    pub helpers: Vec<String>,
    pub aliases: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { detect: true, helpers: Vec::new(), aliases: vec!["RegisterEventAlias".into()] }
    }
}

type Importers = Rc<Vec<(Rc<str>, Side)>>;

pub struct Scopes<'a> {
    ws: &'a Workspace,
    infos: RefCell<HashMap<String, FileInfo>>,
    importers: RefCell<HashMap<String, Importers>>,
}

impl<'a> Scopes<'a> {
    pub fn new(ws: &'a Workspace) -> Self {
        Self { ws, infos: RefCell::default(), importers: RefCell::default() }
    }

    pub fn info(&self, path: &str) -> FileInfo {
        if let Some(i) = self.infos.borrow().get(path) {
            return i.clone();
        }
        let info = FileInfo {
            side: self.ws.side_of(path),
            resource: self.ws.resource_of(path).map(|r| Rc::from(r.dir.as_str())),
        };
        self.infos.borrow_mut().insert(path.to_owned(), info.clone());
        info
    }

    fn importers(&self, path: &str) -> Importers {
        if let Some(i) = self.importers.borrow().get(path) {
            return i.clone();
        }
        let mut out = Vec::new();
        if let Some(home) = self.ws.resource_of(path)
            && let Some(rel) = home.relative(path)
        {
            for other in self.ws.resources().filter(|r| r.dir != home.dir) {
                for e in &other.entries {
                    let side = match e.name.as_str() {
                        "client_script" | "client_scripts" => Side::Client,
                        "server_script" | "server_scripts" => Side::Server,
                        "shared_script" | "shared_scripts" => Side::Shared,
                        _ => continue,
                    };
                    for v in &e.values {
                        let Some((name, pattern)) = v.text.trim().strip_prefix('@').and_then(|r| r.split_once('/'))
                        else {
                            continue;
                        };
                        if name == home.name && Glob::new(pattern, CASE_SENSITIVE).is_some_and(|g| g.matches(rel)) {
                            out.push((Rc::from(other.dir.as_str()), side));
                        }
                    }
                }
            }
        }
        let out = Rc::new(out);
        self.importers.borrow_mut().insert(path.to_owned(), out.clone());
        out
    }

    fn usable(&self, f: &EventFn, file: &str, info: &FileInfo) -> bool {
        if f.is_standard() {
            return true;
        }
        if f.kind == FnKind::Local && f.file.as_deref() != Some(file) {
            return false;
        }
        if f.redefinition {
            return self.applies_to(f, info);
        }
        compatible(f.defined_side, info.side)
    }

    fn applies_to(&self, f: &EventFn, info: &FileInfo) -> bool {
        if !compatible(f.defined_side, info.side) {
            return false;
        }
        let Some(def) = f.file.as_deref() else { return true };
        if self.info(def).resource == info.resource {
            return true;
        }
        self.importers(def).iter().any(|(r, side)| Some(r) == info.resource.as_ref() && compatible(*side, info.side))
    }

    fn pick(&self, candidates: Vec<Rc<EventFn>>, file: &str, info: &FileInfo) -> Option<Rc<EventFn>> {
        if candidates.len() <= 1 {
            return candidates.into_iter().next();
        }
        let same_file = candidates.iter().find(|c| c.file.as_deref() == Some(file));
        let same_resource = || {
            candidates.iter().find(|c| {
                info.resource.is_some() && c.file.as_deref().is_some_and(|f| self.info(f).resource == info.resource)
            })
        };
        same_file.or_else(same_resource).or(candidates.first()).cloned()
    }

    fn resolve(
        &self,
        functions: &HashMap<String, Vec<Rc<EventFn>>>,
        standard: &HashMap<String, Rc<EventFn>>,
        record: &CallRecord,
        file: &str,
        info: &FileInfo,
        self_class: Option<&str>,
    ) -> Option<(Rc<EventFn>, usize)> {
        let by_name = |n: &str| standard.get(n).cloned().map(|f| (f, 0));
        if record.original {
            return by_name(&record.callee);
        }
        let name = record.simple_name();
        let Some(named) = functions.get(name) else {
            // no helper of that name: only a plain call of a runtime function resolves
            return if record.colon || record.callee.contains('.') { None } else { by_name(&record.callee) };
        };
        let candidates: Vec<Rc<EventFn>> = named.iter().filter(|f| self.usable(f, file, info)).cloned().collect();
        if record.colon {
            let methods: Vec<Rc<EventFn>> = candidates.into_iter().filter(|f| f.is_method()).collect();
            if methods.is_empty() {
                return None;
            }
            let receiver = record.receiver().unwrap_or("?");
            let by_class: Vec<Rc<EventFn>> = methods
                .iter()
                .filter(|f| {
                    let class = f.class_name();
                    class == Some(receiver)
                        || (class.is_some() && class == record.receiver_class.as_deref())
                        || (receiver == "self" && class.is_some() && class == self_class)
                })
                .cloned()
                .collect();
            if let Some(f) = self.pick(by_class, file, info) {
                return Some((f, 0));
            }
            if methods.len() == 1 {
                return Some((methods[0].clone(), 0));
            }
            if receiver == "?" {
                return None;
            }
            // `zone:TriggerEvent(…)` with several `X:TriggerEvent`: a receiver named after the class
            let receiver_name = receiver.rsplit('.').next().unwrap_or(receiver).to_lowercase();
            let by_name: Vec<&Rc<EventFn>> = methods
                .iter()
                .filter(|f| {
                    let class = f.class_name().unwrap_or("");
                    receiver_name.contains(&class.rsplit('.').next().unwrap_or(class).to_lowercase())
                })
                .collect();
            return match by_name.as_slice() {
                [f] => Some(((*f).clone(), 0)),
                _ => None,
            };
        }
        if let Some((prefix, _)) = record.callee.rsplit_once('.') {
            let same: Vec<Rc<EventFn>> =
                candidates.iter().filter(|f| !f.is_method() && f.path == record.callee).cloned().collect();
            if let Some(f) = self.pick(same, file, info) {
                return Some((f, 0));
            }
            let as_method = format!("{prefix}:{name}");
            let methods: Vec<Rc<EventFn>> =
                candidates.iter().filter(|f| f.is_method() && f.path == as_method).cloned().collect();
            if let Some(f) = self.pick(methods, file, info) {
                return Some((f, 1));
            }
            // `local H = MyHelper; H.Trigger(…)`
            if record.local_receiver {
                let fields: Vec<&Rc<EventFn>> = candidates.iter().filter(|f| f.kind == FnKind::Field).collect();
                if let [f] = fields.as_slice() {
                    return Some(((*f).clone(), 0));
                }
            }
            return None;
        }
        let same: Vec<&Rc<EventFn>> = candidates.iter().filter(|f| f.path == record.callee).collect();
        let subset =
            |pred: &dyn Fn(&EventFn) -> bool| same.iter().filter(|f| pred(f)).map(|f| (*f).clone()).collect::<Vec<_>>();
        if let Some(f) = self.pick(subset(&|f| f.kind == FnKind::Local), file, info) {
            return Some((f, 0));
        }
        if let Some(f) = self.pick(subset(&|f| f.redefinition), file, info) {
            return Some((f, 0));
        }
        if let Some(f) = self.pick(subset(&|f| f.kind == FnKind::Global && !f.is_standard()), file, info) {
            return Some((f, 0));
        }
        by_name(&record.callee)
    }
}

#[derive(Default)]
pub struct Db {
    pub helpers: Vec<Rc<EventFn>>,
    pub sites: Vec<Site>,
    pub dynamic: Vec<DynamicSite>,
    by_key: HashMap<Rc<EventKey>, Vec<usize>>,
    by_file: HashMap<Rc<str>, Vec<usize>>,
    aliases: HashMap<Rc<EventKey>, Vec<Rc<EventKey>>>,
    handled_namespaces: HashSet<String>,
    functions: HashMap<String, Vec<Rc<EventFn>>>,
    standard: HashMap<String, Rc<EventFn>>,
    /// The names of the functions whose calls the detection of the event functions read.
    fetched: BTreeSet<String>,
}

pub fn reaches(t: &Site, h: &Site) -> bool {
    (t.directions.has(Dirs::LOCAL) && compatible(t.side(), h.side()))
        || (t.directions.has(Dirs::SERVER) && h.side() != Side::Client)
        || (t.directions.has(Dirs::CLIENT) && h.side() != Side::Server)
}

impl Db {
    pub fn build(ws: &Workspace, files: &BTreeMap<String, FileFacts>, config: &Config) -> Db {
        Builder::new(ws, files, config).build()
    }

    pub fn keys(&self) -> impl Iterator<Item = &EventKey> {
        self.by_key.keys().map(|k| &**k)
    }

    pub fn related(&self, key: &EventKey) -> Vec<EventKey> {
        let mut out = vec![key.clone()];
        let mut queue = VecDeque::from([key.clone()]);
        while let Some(k) = queue.pop_front() {
            for other in self.aliases.get(&k).into_iter().flatten() {
                if !out.contains(other) {
                    out.push((**other).clone());
                    queue.push_back((**other).clone());
                }
            }
        }
        out
    }

    fn sites_keyed(&self, key: &EventKey) -> impl Iterator<Item = &Site> {
        self.by_key.get(key).into_iter().flatten().map(|&i| &self.sites[i])
    }

    pub fn sites_of(&self, key: &EventKey, aliases: bool) -> Vec<&Site> {
        let mut out: Vec<&Site> = self.sites_keyed(key).collect();
        if aliases && self.aliases.contains_key(key) {
            for k in self.related(key).iter().skip(1) {
                out.extend(self.sites_keyed(k));
            }
        }
        out
    }

    pub fn handlers(&self, key: &EventKey) -> Vec<&Site> {
        self.sites_of(key, true).into_iter().filter(|s| s.role == Role::Handler).collect()
    }

    pub fn triggers(&self, key: &EventKey) -> Vec<&Site> {
        self.sites_of(key, true).into_iter().filter(|s| s.role == Role::Trigger).collect()
    }

    pub fn handlers_reached_by(&self, trigger: &Site) -> Vec<&Site> {
        self.handlers(&trigger.key).into_iter().filter(|h| reaches(trigger, h)).collect()
    }

    pub fn triggers_reaching(&self, handler: &Site) -> Vec<&Site> {
        self.triggers(&handler.key).into_iter().filter(|t| reaches(t, handler)).collect()
    }

    pub fn is_handled_namespace(&self, ns: &str) -> bool {
        self.handled_namespaces.contains(ns)
    }

    pub fn may_be_handled_dynamically(&self, key: &EventKey, net: bool) -> bool {
        !self.dynamic.is_empty()
            && self
                .related(key)
                .iter()
                .any(|k| self.dynamic.iter().any(|d| d.matches(k) && if net { d.net } else { d.role == Role::Handler }))
    }

    pub fn sites_in<'a>(&'a self, file: &str) -> impl Iterator<Item = &'a Site> + 'a {
        self.by_file.get(file).into_iter().flatten().map(|&i| &self.sites[i])
    }

    pub fn site_at(&self, file: &str, start: u32) -> Option<(&Site, usize)> {
        self.sites_in(file)
            .find_map(|s| s.occurrences.iter().position(|o| o.is_some_and(|o| o.span.start == start)).map(|n| (s, n)))
    }

    pub fn resolve(
        &self,
        ws: &Workspace,
        record: &CallRecord,
        file: &str,
        info: &FileInfo,
        self_class: Option<&str>,
    ) -> Option<(Rc<EventFn>, usize)> {
        Scopes::new(ws).resolve(&self.functions, &self.standard, record, file, info, self_class)
    }
}

/// A file and the place of a function among those of its facts.
type FnId<'a> = (&'a str, u32);

/// A call read from the facts of its file.
struct Ref<'a> {
    file: &'a str,
    record: CallRecord,
    facts: &'a FileFacts,
}

impl<'a> Ref<'a> {
    fn function(&self) -> Option<&'a FunctionInfo> {
        self.facts.function_of(&self.record)
    }

    fn function_id(&self) -> Option<FnId<'a>> {
        self.record.function.map(|i| (self.file, i))
    }
}

struct Contribution<'a> {
    target: Rc<EventFn>,
    role: Role,
    key: KeySpec,
    args: &'a [Arg],
}

struct Builder<'a> {
    files: &'a BTreeMap<String, FileFacts>,
    scopes: Scopes<'a>,
    config: &'a Config,
    standard: HashMap<String, Rc<EventFn>>,
    paths: RefCell<HashMap<&'a str, Rc<str>>>,
    explicit: Vec<Rc<EventFn>>,
    explicit_ids: HashSet<FnId<'a>>,
    settings_paths: HashSet<String>,
    detected: Vec<Option<(FnId<'a>, Rc<EventFn>)>>,
    detected_index: HashMap<FnId<'a>, usize>,
    helper_ids: HashSet<FnId<'a>>,
    functions: HashMap<String, Vec<Rc<EventFn>>>,
    keys: RefCell<HashSet<Rc<EventKey>>>,
}

impl<'a> Builder<'a> {
    fn new(ws: &'a Workspace, files: &'a BTreeMap<String, FileFacts>, config: &'a Config) -> Self {
        Self {
            files,
            scopes: Scopes::new(ws),
            config,
            standard: standard().into_iter().map(|f| (f.path.clone(), Rc::new(f))).collect(),
            paths: RefCell::default(),
            explicit: Vec::new(),
            explicit_ids: HashSet::new(),
            settings_paths: HashSet::new(),
            detected: Vec::new(),
            detected_index: HashMap::new(),
            helper_ids: HashSet::new(),
            functions: HashMap::new(),
            keys: RefCell::default(),
        }
    }

    fn path(&self, file: &'a str) -> Rc<str> {
        self.paths.borrow_mut().entry(file).or_insert_with(|| Rc::from(file)).clone()
    }

    fn rebuild_lookup(&mut self) {
        let mut functions: HashMap<String, Vec<Rc<EventFn>>> = HashMap::new();
        let detected =
            self.detected.iter().flatten().map(|(_, f)| f).filter(|f| !self.settings_paths.contains(&f.path));
        for f in self.explicit.iter().chain(detected) {
            functions.entry(f.simple_name().to_owned()).or_default().push(f.clone());
        }
        self.functions = functions;
    }

    /// The calls of a name, read from the facts: they are kept no longer than what reads them.
    fn calls(name: &str, files: impl Iterator<Item = (&'a String, &'a FileFacts)>) -> Vec<Ref<'a>> {
        files
            .flat_map(|(file, facts)| {
                facts.calls.named(name).into_iter().map(move |record| Ref { file: file.as_str(), record, facts })
            })
            .collect()
    }

    fn resolve(&self, r: &Ref, info: &FileInfo) -> Option<(Rc<EventFn>, usize)> {
        let self_class = r.function().and_then(FunctionInfo::class_name);
        self.scopes.resolve(&self.functions, &self.standard, &r.record, r.file, info, self_class)
    }

    fn declare(&mut self) {
        for line in &self.config.helpers {
            if let Some(f) = settings_helper(line) {
                self.settings_paths.insert(f.path.clone());
                self.explicit.push(Rc::new(f));
            }
        }
        let mut aliases: Vec<&str> = Vec::new();
        for a in self.config.aliases.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
            if !aliases.contains(&a) {
                aliases.push(a);
            }
        }
        for a in aliases {
            if self.settings_paths.insert(a.to_owned()) {
                self.explicit.push(Rc::new(EventFn::alias(a)));
            }
        }
    }

    /// The explicit and detected event functions; returns the names of every function whose calls make sites.
    fn detect(&mut self) -> BTreeSet<String> {
        self.declare();
        let files = self.files;
        for (file, facts) in files {
            for (i, info) in facts.functions.iter().enumerate() {
                let Some(annotation) = &info.annotation else { continue };
                if self.settings_paths.contains(&info.path) {
                    continue;
                }
                let Some(spec) = Spec::parse(annotation, Some(&info.params), info.vararg) else { continue };
                let side = self.scopes.info(file).side;
                let f = spec.to_function(
                    &info.path,
                    info.kind,
                    Origin::Annotation,
                    Some(self.path(file)),
                    info.nth,
                    side,
                    Some(info.params.len()),
                    info.vararg,
                );
                self.explicit.push(Rc::new(f));
                self.explicit_ids.insert((file.as_str(), i as u32));
            }
        }
        self.explicit.sort_by(|a, b| {
            (a.file.as_deref().unwrap_or(""), a.nth, &a.path).cmp(&(b.file.as_deref().unwrap_or(""), b.nth, &b.path))
        });
        self.rebuild_lookup();

        let mut fetched: BTreeSet<String> = BTreeSet::new();
        let mut inner: Vec<(FnId<'a>, &'a FunctionInfo, Vec<Ref<'a>>)> = Vec::new();
        let mut inner_index: HashMap<FnId<'a>, usize> = HashMap::new();
        let mut pending: BTreeSet<String> =
            STANDARD_NAMES.iter().map(|n| (*n).to_owned()).chain(self.functions.keys().cloned()).collect();
        for _ in 0..MAX_ROUNDS {
            if pending.is_empty() {
                break;
            }
            for name in &pending {
                fetched.insert(name.clone());
                for r in Self::calls(name, files.iter()) {
                    let (Some(id), Some(info)) = (r.function_id(), r.function()) else { continue };
                    // only arguments computed from parameters may form a helper's key
                    if !r.record.forwards_params() {
                        continue;
                    }
                    let k = *inner_index.entry(id).or_insert_with(|| {
                        inner.push((id, info, Vec::new()));
                        inner.len() - 1
                    });
                    inner[k].2.push(r);
                }
            }
            if self.config.detect {
                self.derive_helpers(&inner);
            }
            pending = self.functions.keys().filter(|k| !fetched.contains(*k)).cloned().collect();
        }
        self.helper_ids = self.explicit_ids.iter().chain(self.detected_index.keys()).copied().collect();
        fetched
    }

    /// Takes the event functions of `db` for what `detect` would find: no edit touched what it reads.
    fn resume(&mut self, db: &Db, changed: &BTreeSet<String>) {
        self.declare();
        self.functions = db.functions.clone();
        self.standard = db.standard.clone();
        let files = self.files;
        for h in &db.helpers {
            let edited = h.file.as_deref().filter(|f| changed.contains(*f));
            let Some((file, facts)) = edited.and_then(|f| files.get_key_value(f)) else { continue };
            if let Some(i) = facts.functions.iter().position(|f| f.path == h.path && f.nth == h.nth) {
                self.helper_ids.insert((file.as_str(), i as u32));
            }
        }
    }

    /// Grouped by file, in call order within a file, so that the sites of one file can be replaced alone.
    fn sites(&mut self, fetched: &BTreeSet<String>, only: Option<&BTreeSet<String>>) -> (Vec<Site>, Vec<DynamicSite>) {
        let mut sites = Vec::new();
        let mut dynamic = Vec::new();
        let files = self.files;
        for name in fetched {
            let refs = match only {
                Some(only) => Self::calls(name, only.iter().filter_map(|f| files.get_key_value(f))),
                None => Self::calls(name, files.iter()),
            };
            let mut last: Option<(&str, FileInfo)> = None;
            for r in refs.iter() {
                let info = match &last {
                    Some((file, info)) if *file == r.file => info.clone(),
                    _ => self.scopes.info(r.file),
                };
                if let Some((f, shift)) = self.resolve(r, &info) {
                    self.add_sites(r, f, shift, info.clone(), &mut sites, &mut dynamic);
                }
                last = Some((r.file, info));
            }
        }
        sites.sort_by(|a, b| a.file.cmp(&b.file));
        dynamic.sort_by(|a, b| a.file.cmp(&b.file));
        (sites, dynamic)
    }

    fn helper_list(&self) -> Vec<Rc<EventFn>> {
        let mut helpers: Vec<Rc<EventFn>> = self.explicit.clone();
        helpers.extend(
            self.detected.iter().flatten().map(|(_, f)| f.clone()).filter(|f| !self.settings_paths.contains(&f.path)),
        );
        helpers.sort_by(|a, b| {
            (&a.path, a.file.as_deref().unwrap_or(""), a.nth).cmp(&(&b.path, b.file.as_deref().unwrap_or(""), b.nth))
        });
        helpers
    }

    fn build(mut self) -> Db {
        let fetched = self.detect();
        let (sites, dynamic) = self.sites(&fetched, None);
        let helpers = self.helper_list();
        let (functions, standard) = (self.functions, self.standard);
        let mut db = Db { helpers, sites, dynamic, functions, standard, fetched, ..Db::default() };
        db.index_keys();
        db.index_derived();
        db
    }

    fn derive_helpers(&mut self, inner: &[(FnId<'a>, &'a FunctionInfo, Vec<Ref<'a>>)]) {
        for _ in 0..MAX_DERIVATIONS {
            let mut changed = false;
            for (id, info, calls) in inner {
                if self.explicit_ids.contains(id) || self.settings_paths.contains(&info.path) {
                    continue;
                }
                let derived = self.derive(*id, info, calls);
                let current =
                    self.detected_index.get(id).and_then(|&i| self.detected[i].as_ref()).map(|(_, f)| f.as_ref());
                if current == derived.as_ref() {
                    continue;
                }
                changed = true;
                match derived {
                    Some(f) => {
                        let f = Rc::new(f);
                        match self.detected_index.get(id) {
                            Some(&i) => self.detected[i] = Some((*id, f)),
                            None => {
                                self.detected_index.insert(*id, self.detected.len());
                                self.detected.push(Some((*id, f)));
                            }
                        }
                    }
                    None => {
                        if let Some(i) = self.detected_index.remove(id) {
                            self.detected[i] = None;
                        }
                    }
                }
            }
            if !changed {
                return;
            }
            self.rebuild_lookup();
        }
    }

    fn derive(&self, id: FnId<'a>, function: &FunctionInfo, calls: &[Ref<'a>]) -> Option<EventFn> {
        let info = self.scopes.info(id.0);
        let mut contributions = Vec::new();
        for r in calls {
            let Some((target, shift)) = self.resolve(r, &info) else { continue };
            if target.file.as_deref() == Some(id.0) && target.path == function.path && target.nth == function.nth {
                continue;
            }
            let args = &r.record.args[shift.min(r.record.args.len())..];
            let role = target.role_for(args.len());
            if role == Role::Alias {
                continue;
            }
            let Some(key) = compose_key(&target.key, args, function) else { continue };
            contributions.push(Contribution { target, role, key, args });
        }
        let first = contributions.first()?;
        let mut base = EventFn::base(&function.path, Role::Register, first.key.clone(), Origin::Detected);
        base.file = Some(self.path(id.0));
        base.nth = function.nth;
        base.kind = function.kind;
        base.defined_side = info.side;
        base.redefinition = function.kind == FnKind::Global && is_standard_name(&function.path);
        base.param_count = Some(function.params.len());
        base.vararg = function.vararg;

        if let Some(h) = contributions.iter().find(|c| c.role == Role::Handler) {
            let (callback, injected) = callback_of(h);
            let net = h.target.net
                || contributions
                    .iter()
                    .any(|c| c.key == h.key && (c.role == Role::Register || (c.role == Role::Handler && c.target.net)));
            return Some(EventFn {
                role: Role::Handler,
                key: h.key.clone(),
                callback,
                injected,
                net,
                required_side: h.target.required_side,
                ..base
            });
        }
        let triggers: Vec<&Contribution> = contributions.iter().filter(|c| c.role == Role::Trigger).collect();
        if let Some(t) = triggers.first() {
            let directions = triggers.iter().fold(Dirs::NONE, |d, c| d.with(c.target.directions));
            let (payload_start, payload_fixed) = payload_of(t, function);
            let target = t.target.target.and_then(|i| t.args.get(i)).and_then(Arg::single_param);
            return Some(EventFn {
                role: Role::Trigger,
                key: t.key.clone(),
                directions,
                target,
                payload_start,
                payload_fixed,
                required_side: directions.required_side(),
                ..base
            });
        }
        let r = contributions.iter().find(|c| c.role == Role::Register)?;
        Some(EventFn { key: r.key.clone(), net: true, required_side: r.target.required_side, ..base })
    }

    fn is_helper(&self, r: &Ref<'a>) -> bool {
        r.function_id().is_some_and(|id| self.helper_ids.contains(&id))
            || r.function().is_some_and(|f| self.settings_paths.contains(&f.path))
    }

    fn site(
        &self,
        r: &Ref<'a>,
        f: &Rc<EventFn>,
        role: Role,
        key: EventKey,
        occurrences: Vec<Option<Occurrence>>,
        info: &FileInfo,
    ) -> Site {
        let mut keys = self.keys.borrow_mut();
        let key = match keys.get(&key) {
            Some(k) => k.clone(),
            None => {
                let k = Rc::new(key);
                keys.insert(k.clone());
                k
            }
        };
        Site {
            file: self.path(r.file),
            role,
            key,
            occurrences,
            call_offset: r.record.offset,
            line: r.record.line,
            callee_span: r.record.callee_span,
            function: f.clone(),
            net: false,
            directions: Dirs::NONE,
            callback_arity: None,
            payload_arity: None,
            info: info.clone(),
        }
    }

    fn add_sites(
        &self,
        r: &Ref<'a>,
        f: Rc<EventFn>,
        shift: usize,
        info: FileInfo,
        sites: &mut Vec<Site>,
        dynamic: &mut Vec<DynamicSite>,
    ) {
        let record = &r.record;
        let args = &record.args[shift.min(record.args.len())..];
        let role = f.role_for(args.len());
        if role == Role::Alias {
            self.add_alias_sites(r, &f, args, &info, sites);
            return;
        }
        let mut parts: Vec<Option<String>> = Vec::new();
        let mut occurrences: Vec<Option<Occurrence>> = Vec::new();
        let mut prefix: Option<String> = None;
        let mut parameter_dependent = false;
        for component in &f.key.components {
            let mut text = String::new();
            let mut occurrence = None;
            let mut known = true;
            for seg in component {
                match seg {
                    Seg::Lit(t) => text.push_str(t),
                    Seg::Param(i) => match args.get(*i) {
                        Some(Arg::Str { value, lit }) => {
                            if occurrence.is_none()
                                && let Some(span) = lit
                            {
                                occurrence = Some(Occurrence { span: *span, whole: component.len() == 1 });
                            }
                            text.push_str(value);
                        }
                        Some(Arg::Template(_) | Arg::Tainted(_) | Arg::Spread(_)) => {
                            parameter_dependent = true;
                            known = false;
                            break;
                        }
                        Some(Arg::Dynamic(p)) => {
                            known = false;
                            prefix.get_or_insert_with(|| format!("{text}{p}"));
                            break;
                        }
                        _ => {
                            known = false;
                            break;
                        }
                    },
                }
            }
            if known {
                parts.push(Some(text));
                occurrences.push(occurrence);
            } else {
                parts.push(None);
                occurrences.push(None);
                prefix.get_or_insert(text);
            }
        }
        let net = role == Role::Register || (role == Role::Handler && f.net);
        if parts.iter().any(Option::is_none) {
            // the plumbing of a helper is described by the helper itself
            if parameter_dependent && self.is_helper(r) {
                return;
            }
            dynamic.push(DynamicSite {
                file: self.path(r.file),
                role,
                logical: f.key.logical,
                parts,
                prefix: prefix.unwrap_or_default(),
                net,
                info,
            });
            return;
        }
        let key = EventKey { parts: parts.into_iter().flatten().collect(), logical: f.key.logical };
        let mut site = self.site(r, &f, role, key, occurrences, &info);
        site.net = net;
        if role == Role::Handler {
            site.callback_arity =
                f.callback.and_then(|i| args.get(i)).and_then(arity_of).map(|a| a.saturating_sub(f.injected));
        }
        if role == Role::Trigger {
            site.directions = f.directions;
            site.payload_arity = payload_arity(&f, args, record);
        }
        sites.push(site);
    }

    fn add_alias_sites(&self, r: &Ref<'a>, f: &Rc<EventFn>, args: &[Arg], info: &FileInfo, sites: &mut Vec<Site>) {
        let wire_index = f.wire.unwrap_or(0);
        let Some(Arg::Str { value: wire, lit: wire_lit }) = args.get(wire_index) else { return };
        let key_args: Vec<Option<&Arg>> = if f.key.components.is_empty() {
            args.iter().enumerate().filter(|(i, _)| *i != wire_index).map(|(_, a)| Some(a)).collect()
        } else {
            f.key
                .components
                .iter()
                .map(|c| match c.as_slice() {
                    [Seg::Param(i)] => args.get(*i),
                    _ => None,
                })
                .collect()
        };
        let mut strings = Vec::new();
        for a in &key_args {
            match a {
                Some(Arg::Str { value, lit }) => strings.push((value.clone(), *lit)),
                _ => return,
            }
        }
        if strings.is_empty() {
            return;
        }
        let occurrence = |lit: &Option<super::model::Span>| lit.map(|span| Occurrence { span, whole: true });
        sites.push(self.site(r, f, Role::Alias, EventKey::wire(wire), vec![occurrence(wire_lit)], info));
        let key = EventKey { parts: strings.iter().map(|(v, _)| v.clone()).collect(), logical: true };
        let occurrences = strings.iter().map(|(_, lit)| occurrence(lit)).collect();
        sites.push(self.site(r, f, Role::Alias, key, occurrences, info));
    }
}

fn compose_key(spec: &KeySpec, args: &[Arg], function: &FunctionInfo) -> Option<KeySpec> {
    // some key argument must come from the helper's parameters
    let from_params =
        |s: &Seg| matches!(s, Seg::Param(i) if matches!(args.get(*i), Some(Arg::Template(_) | Arg::Tainted(_))));
    if !spec.components.iter().flatten().any(from_params) {
        return None;
    }
    let mut components: Vec<Option<Vec<Seg>>> = Vec::new();
    let mut opaque: Vec<Sources> = Vec::new();
    for component in &spec.components {
        let mut segs = Vec::new();
        let mut computed: Option<Sources> = None;
        for seg in component {
            match seg {
                Seg::Lit(_) => segs.push(seg.clone()),
                Seg::Param(i) => match args.get(*i) {
                    Some(Arg::Str { value, .. }) => segs.push(Seg::Lit(value.clone())),
                    Some(Arg::Template(t)) => segs.extend(t.iter().cloned()),
                    Some(Arg::Tainted(s)) => computed.get_or_insert_with(Sources::new).extend(s.iter().copied()),
                    _ => return None,
                },
            }
        }
        match computed {
            Some(s) => {
                components.push(None);
                opaque.push(s);
            }
            None => components.push(Some(merge_literals(segs))),
        }
    }
    let mut used: Sources = Sources::new();
    for c in components.iter().flatten() {
        used.extend(c.iter().filter_map(|s| if let Seg::Param(i) = s { Some(*i as i32) } else { None }));
    }
    opaque.iter().for_each(|s| used.extend(s.iter().copied()));
    if used.is_empty() || used.contains(&VARARG) || used.iter().any(|&i| i as usize >= function.params.len()) {
        return None;
    }
    if !spec.logical {
        // a name computed from the parameters (obfuscated, prefixed by a runtime value): the parameters are the key
        return Some(match components.pop().flatten() {
            Some(single) => KeySpec { logical: false, components: vec![single] },
            None => KeySpec::logical(&used.iter().map(|&i| i as usize).collect::<Vec<_>>()),
        });
    }
    let mut opaque = opaque.into_iter();
    let mut result = Vec::new();
    for c in components {
        match c {
            Some(c) => result.push(c),
            None => result.extend(opaque.next().into_iter().flatten().map(|i| vec![Seg::Param(i as usize)])),
        }
    }
    Some(KeySpec { logical: true, components: result })
}

fn callback_of(h: &Contribution) -> (Option<usize>, usize) {
    let Some(index) = h.target.callback else { return (None, 0) };
    match h.args.get(index) {
        Some(a @ Arg::Template(_)) => a.single_param().map_or((None, 0), |p| (Some(p), h.target.injected)),
        Some(Arg::Closure { callback: Some(c), .. }) => (Some(c.param), injected_through(c, h.target.injected)),
        _ => (None, 0),
    }
}

fn injected_through(c: &CallbackCall, received: usize) -> usize {
    let mut payload: Sources = (received..c.closure_params).map(|i| i as i32).collect();
    if c.closure_vararg {
        payload.insert(VARARG);
    }
    let Some(first) = c.inputs.iter().position(|inputs| inputs.iter().any(|i| payload.contains(i))) else {
        return c.inputs.len();
    };
    // `function(...) callback(...) end` receiving injected values: they are in the vararg too
    if c.inputs[first].contains(&VARARG) && c.closure_params < received {
        first + received - c.closure_params
    } else {
        first
    }
}

fn is_vararg_spread(a: &Arg) -> bool {
    matches!(a, Arg::Spread(s) if s.len() == 1 && s.contains(&VARARG))
}

fn payload_of(t: &Contribution, function: &FunctionInfo) -> (Option<usize>, Option<usize>) {
    let Some(start) = t.target.payload_start else { return (None, None) };
    let payload = &t.args[start.min(t.args.len())..];
    let Some(first) = payload.first() else { return (None, Some(0)) };
    let vararg_position = function.params.len();
    if is_vararg_spread(first) {
        return (function.vararg.then_some(vararg_position), None);
    }
    let Some(p0) = first.single_param() else { return (None, None) };
    for (j, a) in payload.iter().enumerate() {
        if j + 1 == payload.len() && is_vararg_spread(a) && p0 + j == vararg_position {
            return (Some(p0), None);
        }
        if a.single_param() != Some(p0 + j) {
            return (None, None);
        }
    }
    (Some(p0), Some(payload.len()))
}

fn arity_of(a: &Arg) -> Option<usize> {
    match a {
        Arg::Closure { params, vararg: false, .. } | Arg::FunctionRef { params, vararg: false } => Some(*params),
        _ => None,
    }
}

fn payload_arity(f: &EventFn, args: &[Arg], record: &CallRecord) -> Option<usize> {
    if let Some(n) = f.payload_fixed {
        return Some(n);
    }
    let start = f.payload_start?;
    let payload = &args[start.min(args.len())..];
    if payload.iter().any(|a| matches!(a, Arg::Spread(_))) || (record.last_arg_multi && !payload.is_empty()) {
        return None;
    }
    Some(payload.len())
}

fn annotated(facts: &FileFacts) -> impl Iterator<Item = &FunctionInfo> {
    facts.functions.iter().filter(|f| f.annotation.is_some())
}

impl Db {
    fn forwarding<'a>(&self, facts: &'a FileFacts) -> Vec<(CallRecord, &'a FunctionInfo)> {
        let read = facts.calls.of(|name| self.fetched.contains(name)).into_iter().filter(CallRecord::forwards_params);
        read.filter_map(|call| facts.function_of(&call).map(|function| (call, function))).collect()
    }

    /// Whether the detection of the event functions reads the same in two versions of a file, wherever the code
    /// stands in each: the annotated functions, and the calls of event functions that forward parameters.
    pub fn detects_alike(&self, old: &FileFacts, new: &FileFacts) -> bool {
        if !annotated(old).eq(annotated(new)) {
            return false;
        }
        let (a, b) = (self.forwarding(old), self.forwarding(new));
        let alike =
            |((call, function), (other, same)): (&(CallRecord, &FunctionInfo), &(CallRecord, &FunctionInfo))| {
                call.alike(other) && function == same
            };
        a.len() == b.len() && a.iter().zip(&b).all(alike)
    }

    /// The database after the Lua files `changed` were edited, re-resolving only their calls. The event functions,
    /// which every file depends on, are detected again when `redetect` says an edit touched what the detection
    /// reads: `None` when they changed, build it again.
    pub fn update(
        mut self,
        ws: &Workspace,
        files: &BTreeMap<String, FileFacts>,
        config: &Config,
        changed: &BTreeSet<String>,
        redetect: bool,
    ) -> Option<Db> {
        let mut builder = Builder::new(ws, files, config);
        if redetect {
            let fetched = builder.detect();
            if builder.helper_list() != self.helpers {
                return None;
            }
            self.fetched = fetched;
        } else {
            builder.resume(&self, changed);
        }
        let (fresh, fresh_dynamic) = builder.sites(&self.fetched, Some(changed));
        (self.functions, self.standard) = (builder.functions, builder.standard);
        self.replace_sites(changed, fresh);
        self.dynamic.retain(|d| !changed.contains(&*d.file));
        self.dynamic.extend(fresh_dynamic);
        self.dynamic.sort_by(|a, b| a.file.cmp(&b.file));
        self.index_derived();
        Some(self)
    }

    fn replace_sites(&mut self, changed: &BTreeSet<String>, fresh: Vec<Site>) {
        let old = std::mem::take(&mut self.sites);
        let mut moved: Vec<Option<usize>> = Vec::with_capacity(old.len());
        let mut sites = Vec::with_capacity(old.len() + fresh.len());
        let mut added = Vec::with_capacity(fresh.len());
        let mut fresh = fresh.into_iter().peekable();
        for s in old {
            while let Some(f) = fresh.next_if(|f| f.file < s.file) {
                added.push(sites.len());
                sites.push(f);
            }
            if changed.contains(&*s.file) {
                moved.push(None);
            } else {
                moved.push(Some(sites.len()));
                sites.push(s);
            }
        }
        for f in fresh {
            added.push(sites.len());
            sites.push(f);
        }
        self.sites = sites;
        self.by_key.retain(|_, list| {
            list.retain_mut(|i| moved[*i].map(|n| *i = n).is_some());
            !list.is_empty()
        });
        let mut touched: HashSet<Rc<EventKey>> = HashSet::new();
        for &i in &added {
            let key = &self.sites[i].key;
            self.by_key.entry(key.clone()).or_default().push(i);
            touched.insert(key.clone());
        }
        for key in touched {
            if let Some(list) = self.by_key.get_mut(&key) {
                list.sort_unstable();
            }
        }
    }

    fn index_keys(&mut self) {
        for (i, s) in self.sites.iter().enumerate() {
            self.by_key.entry(s.key.clone()).or_default().push(i);
        }
    }

    fn index_derived(&mut self) {
        self.by_file.clear();
        let mut start = 0;
        while start < self.sites.len() {
            let file = &self.sites[start].file;
            let end = start + self.sites[start..].iter().take_while(|s| s.file == *file).count();
            self.by_file.insert(file.clone(), (start..end).collect());
            start = end;
        }
        self.aliases.clear();
        // the two sites of an alias call are built together
        let mut calls: Vec<Vec<Rc<EventKey>>> = Vec::new();
        let mut previous: Option<&Site> = None;
        for s in self.sites.iter().filter(|s| s.role == Role::Alias) {
            match calls.last_mut() {
                Some(keys) if previous.is_some_and(|p| p.same_call(s)) => {
                    if !keys.contains(&s.key) {
                        keys.push(s.key.clone());
                    }
                }
                _ => calls.push(vec![s.key.clone()]),
            }
            previous = Some(s);
        }
        for keys in calls {
            for a in &keys {
                for b in keys.iter().filter(|b| *b != a) {
                    let list = self.aliases.entry(a.clone()).or_default();
                    if !list.contains(b) {
                        list.push(b.clone());
                    }
                }
            }
        }
        let mut namespaces: HashSet<String> = HashSet::new();
        let mut add = |k: &EventKey| {
            let ns = k.namespace();
            if !ns.is_empty() && !namespaces.contains(ns) {
                namespaces.insert(ns.to_owned());
            }
        };
        for (key, sites) in &self.by_key {
            if !sites.iter().any(|&i| self.sites[i].role == Role::Handler) {
                continue;
            }
            if self.aliases.contains_key(key) {
                self.related(key).iter().for_each(&mut add);
            } else {
                add(key);
            }
        }
        self.handled_namespaces = namespaces;
    }

    #[cfg(test)]
    pub fn dump(&self) -> String {
        let mut keys: Vec<String> = self.by_key.iter().map(|(k, v)| format!("{k:?} {v:?}")).collect();
        let mut files: Vec<String> = self.by_file.iter().map(|(k, v)| format!("{k} {v:?}")).collect();
        let mut aliases: Vec<String> = self.aliases.iter().map(|(k, v)| format!("{k:?} {v:?}")).collect();
        let mut namespaces: Vec<&String> = self.handled_namespaces.iter().collect();
        keys.sort();
        files.sort();
        aliases.sort();
        namespaces.sort();
        format!(
            "helpers {:?}\nsites {:#?}\ndynamic {:#?}\nkeys {keys:#?}\nfiles {files:#?}\naliases {aliases:#?}\nnamespaces {namespaces:?}",
            self.helpers, self.sites, self.dynamic
        )
    }
}
