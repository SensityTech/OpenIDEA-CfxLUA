use super::convert::{self, Group};
use super::model::NativeSet;
use super::sources::{self, Database, Inputs};
use super::{Natives, project_games};
use crate::feature::{CommandCx, CommandResult};
use crate::project::Game;
use crate::workspace::Workspace;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use openidea_plugin::{HttpRequest, LogLevel, NotificationLevel, host};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, BufWriter, Read, Write};
use std::rc::Rc;

pub const UPDATE: &str = "natives.update";
pub const SCHEDULED_UPDATE: &str = "natives.scheduled-update";
pub const AUTO_UPDATE: &str = "natives.autoUpdate";

const STATE_KEY: &str = "natives.state";
const DELTA_FORMAT: u32 = 1;
pub const MAX_STORED_DELTA: usize = 112 * 1024;
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const TIMEOUT_MS: u32 = 15_000;
const TASK_TITLE: &str = "CfxLua: updating natives";
// Fuel, not time: a call converts at most this much database text (about 0.3·10⁹ per MiB), or one set's.
const BYTES_PER_CALL: usize = 5 << 20;

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    checked: u64,
    #[serde(default)]
    databases: BTreeMap<String, Validators>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Validators {
    etag: Option<String>,
    last_modified: Option<String>,
    fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    format: u32,
    base: String,
    inputs: Inputs,
    functions: usize,
    removed: Vec<String>,
    groups: Vec<Group>,
}

thread_local! {
    static IN_EFFECT: RefCell<[Option<Option<Rc<Delta>>>; 3]> = const { RefCell::new([None, None, None]) };
}

thread_local! {
    static CHANGED: RefCell<Vec<NativeSet>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
}

/// The sets updates changed since the last call.
pub fn take_changed() -> Vec<NativeSet> {
    CHANGED.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

fn known(set: NativeSet) -> Option<Option<Rc<Delta>>> {
    IN_EFFECT.with(|m| m.borrow()[set as usize].clone())
}

fn remember(set: NativeSet, delta: Option<Rc<Delta>>) {
    IN_EFFECT.with(|m| m.borrow_mut()[set as usize] = Some(delta));
}

fn delta_key(set: NativeSet) -> String {
    format!("natives.delta.{}", set.id().to_ascii_lowercase())
}

fn read_stored(set: NativeSet) -> Option<Delta> {
    let key = delta_key(set);
    let raw = host::storage::get(&key).ok()??;
    let delta = decode_delta(&raw).filter(|d| d.format == DELTA_FORMAT && d.base == sources::snapshot_id(set));
    if delta.is_none() {
        let _ = host::storage::delete(&key);
    }
    delta
}

fn effective(set: NativeSet) -> Option<Rc<Delta>> {
    if let Some(delta) = known(set) {
        return delta;
    }
    let stored = read_stored(set).map(Rc::new);
    remember(set, stored.clone());
    stored
}

pub fn current_delta(set: NativeSet) -> Option<Rc<Delta>> {
    if cfg!(test) { known(set).flatten() } else { effective(set) }
}

pub fn apply(groups: Vec<Group>, delta: &Delta) -> Vec<Group> {
    let replaced: HashSet<&str> = delta
        .removed
        .iter()
        .map(String::as_str)
        .chain(delta.groups.iter().map(|g| g.native.lua_name.as_str()))
        .collect();
    let mut out: Vec<Group> = groups.into_iter().filter(|g| !replaced.contains(g.native.lua_name.as_str())).collect();
    out.extend(delta.groups.iter().cloned());
    convert::sort_groups(&mut out);
    out
}

fn diff(set: NativeSet, bundled: &[Group], new: Vec<Group>, inputs: Inputs) -> Delta {
    let removed = {
        let names: HashSet<&str> = new.iter().map(|g| g.native.lua_name.as_str()).collect();
        bundled.iter().map(|g| g.native.lua_name.as_str()).filter(|n| !names.contains(n)).map(str::to_owned).collect()
    };
    let old: HashMap<&str, &Group> = bundled.iter().map(|g| (g.native.lua_name.as_str(), g)).collect();
    let functions = convert::functions(&new);
    let groups = new.into_iter().filter(|g| old.get(g.native.lua_name.as_str()) != Some(&g)).collect();
    Delta { format: DELTA_FORMAT, base: sources::snapshot_id(set), inputs, functions, removed, groups }
}

struct Bounded {
    bytes: Vec<u8>,
    max: usize,
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes.len() + buf.len() > self.max {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Gives up as soon as the text would exceed `max`: a delta too large to be stored costs no more than one that fits.
fn encode_delta(delta: &Delta, max: usize) -> Option<String> {
    let mut gz = GzEncoder::new(Bounded { bytes: Vec::new(), max: max / 4 * 3 }, Compression::best());
    let mut json = BufWriter::new(&mut gz);
    serde_json::to_writer(&mut json, delta).ok()?;
    json.into_inner().ok()?;
    Some(base64_encode(&gz.finish().ok()?.bytes))
}

fn decode_delta(text: &str) -> Option<Delta> {
    let gz = base64_decode(text)?;
    let mut json = Vec::new();
    GzDecoder::new(gz.as_slice()).read_to_end(&mut json).ok()?;
    serde_json::from_slice(&json).ok()
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = byte(0) << 16 | byte(1) << 8 | byte(2);
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(BASE64[(n >> (18 - 6 * i)) as usize & 63]) } else { '=' });
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| -> Option<u32> {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        Some(u32::from(v))
    };
    let last = bytes.len() / 4;
    let mut out = Vec::with_capacity(last * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = n << 6 | value(c)?;
        }
        n <<= 6 * pad as u32;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8][..3 - pad]);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// The update

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Remote,
    Cache,
    Bundled,
}

#[derive(Debug)]
struct Outcome {
    changed: Vec<NativeSet>,
    counts: [Option<usize>; 3],
    origin: Origin,
    error: Option<String>,
    unstored: Vec<NativeSet>,
    storage: bool,
}

enum Content {
    Failed,
    Known(String),
    Fetched { text: String, validators: Validators, parsed: bool },
}

impl Content {
    fn fingerprint(&self) -> Option<&str> {
        match self {
            Self::Failed => None,
            Self::Known(fp) => Some(fp),
            Self::Fetched { validators, .. } => Some(&validators.fingerprint),
        }
    }
}

enum Answer {
    NotModified,
    Body { bytes: Vec<u8>, etag: Option<String>, last_modified: Option<String> },
}

fn download(db: Database, validators: Option<&Validators>) -> Result<Answer, String> {
    let mut request = HttpRequest::get(db.url());
    request.timeout_ms = Some(TIMEOUT_MS);
    if let Some(v) = validators {
        // static.cfx.re ignores ETags, GitHub raw has no Last-Modified.
        if let Some(date) = &v.last_modified {
            request = request.header("If-Modified-Since", date);
        }
        if let Some(etag) = &v.etag {
            request = request.header("If-None-Match", etag);
        }
    }
    let response = host::fetch(&request).map_err(|e| e.to_string())?;
    let header =
        |name: &str| response.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    match response.status {
        304 => Ok(Answer::NotModified),
        200..=299 => {
            let (etag, last_modified) = (header("etag"), header("last-modified"));
            Ok(Answer::Body { bytes: response.body, etag, last_modified })
        }
        status => Err(format!("HTTP {status}")),
    }
}

fn in_effect(set: NativeSet) -> Inputs {
    effective(set).map_or_else(|| sources::bundled_inputs(set), |d| d.inputs.clone())
}

fn wanted(set: NativeSet, content: &HashMap<Database, Content>, in_effect: &Inputs) -> Inputs {
    sources::databases(set)
        .iter()
        .filter_map(|&db| {
            let fp = content
                .get(&db)
                .and_then(Content::fingerprint)
                .or_else(|| in_effect.get(db.id()).map(String::as_str))?;
            Some((db.id().to_owned(), fp.to_owned()))
        })
        .collect()
}

fn weight(inputs: &Inputs) -> usize {
    inputs.values().map(|fp| sources::fingerprint_len(fp)).sum()
}

fn save(set: NativeSet, delta: Option<&Delta>, budget: usize) -> bool {
    let key = delta_key(set);
    match delta {
        None => {
            let _ = host::storage::delete(&key);
            true
        }
        Some(d) => encode_delta(d, budget).is_some_and(|text| host::storage::set(&key, &text).is_ok()),
    }
}

fn save_state(state: &State) {
    if let Ok(json) = serde_json::to_string(state)
        && host::storage::set(STATE_KEY, &json).is_err()
    {
        host::log(LogLevel::Debug, "natives: the update state cannot be stored");
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

enum Piece {
    Convert(NativeSet, Inputs),
    Diff(NativeSet),
    Count(NativeSet),
}

impl Piece {
    fn set(&self) -> NativeSet {
        match self {
            Self::Convert(set, _) | Self::Diff(set) | Self::Count(set) => *set,
        }
    }

    fn weight(&self) -> usize {
        match self {
            Self::Convert(_, wanted) => weight(wanted),
            Self::Diff(set) | Self::Count(set) => weight(&sources::bundled_inputs(*set)),
        }
    }
}

struct Converted {
    set: NativeSet,
    inputs: Inputs,
    groups: Vec<Group>,
}

// Spread over several calls and lost with the instance: what it saved stays, the rest waits for the next check.
struct Job {
    manual: bool,
    budget: usize,
    state: State,
    content: HashMap<Database, Content>,
    sets: Vec<NativeSet>,
    todo: Vec<NativeSet>,
    converted: Option<Converted>,
    counted: Vec<NativeSet>,
    errors: Vec<String>,
    outcome: Outcome,
    progress: u32,
}

impl Job {
    fn start(task: &host::Task, games: &[Game], manual: bool, budget: usize) -> Option<Job> {
        let (mut state, storage) = match host::storage::get(STATE_KEY) {
            Ok(raw) => (raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default(), true),
            Err(_) => (State::default(), false),
        };
        state.checked = now_secs();
        save_state(&state);
        let mut sets: Vec<NativeSet> =
            games.iter().map(|&g| NativeSet::for_game(g)).chain([NativeSet::Server]).collect();
        sets.sort_by_key(|&s| (weight(&sources::bundled_inputs(s)), s));
        sets.dedup();
        let databases: Vec<Database> =
            Database::ALL.into_iter().filter(|db| sets.iter().any(|&s| sources::databases(s).contains(db))).collect();
        let outcome = Outcome {
            changed: Vec::new(),
            counts: [None; 3],
            origin: Origin::Remote,
            error: None,
            unstored: Vec::new(),
            storage,
        };
        let mut job = Job {
            manual,
            budget,
            state,
            content: HashMap::new(),
            todo: sets.clone(),
            sets,
            converted: None,
            counted: Vec::new(),
            errors: Vec::new(),
            outcome,
            progress: 0,
        };
        for db in databases {
            if task.is_cancelled() {
                return None;
            }
            job.report(task, db.file_name());
            let content = job.obtain(db, true);
            job.content.insert(db, content);
        }
        for set in job.sets.clone() {
            let in_effect = in_effect(set);
            if wanted(set, &job.content, &in_effect) == in_effect {
                continue;
            }
            // A 304 does not carry the content: download it again unless it is the bundled one.
            for &db in sources::databases(set) {
                if matches!(job.content.get(&db), Some(Content::Known(fp)) if *fp != db.bundled_fingerprint()) {
                    if task.is_cancelled() {
                        return None;
                    }
                    let content = job.obtain(db, false);
                    job.content.insert(db, content);
                }
            }
        }
        Some(job)
    }

    fn report(&mut self, task: &host::Task, step: &str) {
        let steps = (Database::ALL.len() + 2 * self.sets.len()) as u32;
        task.report(self.progress, Some(steps.max(self.progress + 1)), Some(step));
        self.progress += 1;
    }

    fn obtain(&mut self, db: Database, conditional: bool) -> Content {
        let last = self.state.databases.get(db.id()).cloned();
        match download(db, last.as_ref().filter(|_| conditional)) {
            Ok(Answer::NotModified) => match last {
                Some(v) if conditional => Content::Known(v.fingerprint),
                _ => {
                    self.errors.push(format!("{}: HTTP 304 without a valid cached copy", db.id()));
                    Content::Failed
                }
            },
            Ok(Answer::Body { bytes, etag, last_modified }) => {
                let fingerprint = sources::fingerprint(&bytes);
                // Known to parse (last download, snapshot, input of an update in effect): no conversion to wait for.
                let parsed = last.is_some_and(|v| v.fingerprint == fingerprint)
                    || fingerprint == db.bundled_fingerprint()
                    || self.sets.iter().any(|&s| in_effect(s).get(db.id()) == Some(&fingerprint));
                let validators = Validators { etag, last_modified, fingerprint };
                if parsed {
                    self.state.databases.insert(db.id().to_owned(), validators.clone());
                }
                Content::Fetched { text: sources::decode(bytes), validators, parsed }
            }
            Err(e) => {
                self.errors.push(format!("{}: {e}", db.id()));
                Content::Failed
            }
        }
    }

    fn fetched(&self, db: Database, fp: &str) -> Option<&str> {
        match self.content.get(&db) {
            Some(Content::Fetched { text, validators, .. }) if validators.fingerprint == fp => Some(text),
            _ => None,
        }
    }

    fn next(&mut self, natives: &Natives) -> Option<Piece> {
        while let Some(&set) = self.todo.first() {
            if self.converted.as_ref().is_some_and(|c| c.set == set) {
                return Some(Piece::Diff(set));
            }
            let in_effect = in_effect(set);
            let wanted = wanted(set, &self.content, &in_effect);
            let available = sources::databases(set).iter().all(|&db| {
                wanted.get(db.id()).is_some_and(|fp| self.fetched(db, fp).is_some() || *fp == db.bundled_fingerprint())
            });
            if wanted == in_effect || !available {
                // Unchanged, or a database could not be obtained (reported).
                self.todo.remove(0);
                continue;
            }
            return Some(Piece::Convert(set, wanted));
        }
        if !self.manual || !self.outcome.changed.is_empty() || !self.errors.is_empty() {
            return None;
        }
        // "N functions from …": loading a catalog is a conversion too.
        let uncounted = |s: &NativeSet| !self.counted.contains(s) && effective(*s).is_none() && !natives.is_loaded(*s);
        self.sets.iter().copied().find(uncounted).map(Piece::Count)
    }

    fn work(&mut self, task: &host::Task, natives: &Natives) -> Option<bool> {
        let mut converted = 0;
        while let Some(piece) = self.next(natives) {
            let weight = piece.weight();
            if converted > 0 && converted + weight > BYTES_PER_CALL {
                return Some(true);
            }
            if task.is_cancelled() {
                return None;
            }
            self.report(task, piece.set().display_name());
            match piece {
                Piece::Convert(set, wanted) => self.convert(set, wanted),
                Piece::Diff(set) => self.compare(set),
                Piece::Count(set) => {
                    natives.catalog(set);
                    self.counted.push(set);
                }
            }
            converted += weight;
        }
        Some(false)
    }

    fn convert(&mut self, set: NativeSet, wanted: Inputs) {
        let fp = |db: Database| wanted.get(db.id()).map_or("", String::as_str);
        let bundled: HashMap<Database, String> = sources::databases(set)
            .iter()
            .filter(|&&db| self.fetched(db, fp(db)).is_none())
            .filter_map(|&db| Some((db, db.bundled_text().ok()?)))
            .collect();
        let groups =
            sources::groups(set, |db| self.fetched(db, fp(db)).or_else(|| bundled.get(&db).map(String::as_str)));
        let groups = match groups {
            Ok(groups) => groups,
            Err((db, e)) => {
                self.errors.push(format!("{}: invalid content ({e})", db.id()));
                // A download is set aside and the set tried again without it; the snapshot has no fallback.
                if !matches!(self.content.insert(db, Content::Failed), Some(Content::Fetched { .. })) {
                    self.todo.remove(0);
                }
                return;
            }
        };
        for &db in sources::databases(set) {
            if let Some(Content::Fetched { validators, parsed, .. }) = self.content.get_mut(&db)
                && !*parsed
                && validators.fingerprint == fp(db)
            {
                *parsed = true;
                self.state.databases.insert(db.id().to_owned(), validators.clone());
            }
        }
        if wanted == sources::bundled_inputs(set) {
            let functions = convert::functions(&groups);
            self.complete(set, None, functions);
        } else {
            self.converted = Some(Converted { set, inputs: wanted, groups });
        }
    }

    fn compare(&mut self, set: NativeSet) {
        let Some(Converted { inputs, groups, .. }) = self.converted.take() else { return };
        let delta = diff(set, &sources::bundled_groups(set), groups, inputs);
        let functions = delta.functions;
        self.complete(set, Some(Rc::new(delta)), functions);
    }

    fn complete(&mut self, set: NativeSet, delta: Option<Rc<Delta>>, functions: usize) {
        if !save(set, delta.as_deref(), self.budget) {
            self.outcome.unstored.push(set);
        }
        remember(set, delta);
        CHANGED.with(|c| c.borrow_mut().push(set));
        self.outcome.changed.push(set);
        self.outcome.counts[set as usize] = Some(functions);
        self.todo.remove(0);
    }

    fn finish(mut self, natives: &Natives) {
        save_state(&self.state);
        let o = &mut self.outcome;
        for db in Database::ALL {
            if matches!(self.content.get(&db), Some(Content::Failed)) {
                let downloaded =
                    self.state.databases.get(db.id()).is_some_and(|v| v.fingerprint != db.bundled_fingerprint());
                o.origin = o.origin.max(if downloaded { Origin::Cache } else { Origin::Bundled });
            }
        }
        o.error = (!self.errors.is_empty()).then(|| self.errors.join("; "));
        if self.manual && o.changed.is_empty() && o.error.is_none() {
            // "N functions from …": the sets of the project count.
            for &set in &self.sets {
                o.counts[set as usize] =
                    Some(effective(set).map_or_else(|| natives.catalog(set).len(), |d| d.functions));
            }
        }
        o.changed.sort();
        o.unstored.sort();
        let changed: Vec<&str> = o.changed.iter().map(|s| s.id()).collect();
        host::log(
            LogLevel::Info,
            &format!(
                "natives: origin={:?}, changed={changed:?}, error={}",
                o.origin,
                o.error.as_deref().unwrap_or("none")
            ),
        );
        if let Some((level, title, body)) = notification(o, self.manual) {
            host::notify(level, title, &body);
        }
    }
}

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn describe_fallback(o: &Outcome) -> String {
    let fallback = match o.origin {
        Origin::Cache => "the previously downloaded natives are used",
        Origin::Bundled => "the natives bundled with the plugin are used",
        Origin::Remote => "the natives may be outdated",
    };
    format!("{}: {fallback}.", o.error.as_deref().unwrap_or_default())
}

fn describe(o: &Outcome) -> String {
    let total: usize = o.counts.iter().flatten().sum();
    let source = match o.origin {
        Origin::Remote => "the Cfx.re native database",
        Origin::Cache => "the cached native database (offline)",
        Origin::Bundled => "the snapshot bundled with the plugin (offline)",
    };
    let changes = if o.changed.is_empty() {
        "already up to date".to_owned()
    } else {
        let names: Vec<&str> = o.changed.iter().map(|s| s.display_name()).collect();
        format!("updated {}", names.join(", "))
    };
    let error = o.error.as_ref().map(|e| format!(" Error: {e}")).unwrap_or_default();
    format!("{} functions from {source}, {changes}.{error}", grouped(total))
}

fn notification(o: &Outcome, manual: bool) -> Option<(NotificationLevel, &'static str, String)> {
    let level = if o.error.is_some() { NotificationLevel::Warning } else { NotificationLevel::Info };
    if !o.changed.is_empty() {
        let mut lines: Vec<String> = o
            .changed
            .iter()
            .map(|&s| match o.counts[s as usize] {
                Some(n) => format!("{}: {} functions", s.display_name(), grouped(n)),
                None => s.display_name().to_owned(),
            })
            .collect();
        let why = if o.storage { "too large to be stored" } else { "the plugin's storage is not granted" };
        lines.extend(
            o.unstored.iter().map(|s| format!("{}: {why}, used until the plugin is restarted.", s.display_name())),
        );
        if o.error.is_some() {
            lines.push(describe_fallback(o));
        }
        return Some((level, "CfxLua: natives updated", lines.join("\n")));
    }
    if !manual {
        // Unchanged automatic updates are silent: an offline IDE never nags.
        return None;
    }
    if o.error.is_some() {
        return Some((level, "CfxLua: natives could not be updated", describe_fallback(o)));
    }
    Some((level, "CfxLua: natives are up to date", describe(o)))
}

fn proceed(mut job: Job, task: &host::Task, natives: &Natives) {
    match job.work(task, natives) {
        Some(true) => JOB.with(|j| *j.borrow_mut() = Some(job)),
        Some(false) => job.finish(natives),
        None => host::log(LogLevel::Info, "natives: update cancelled"),
    }
}

fn start(natives: &Natives, games: &[Game], manual: bool, budget: usize) {
    JOB.with(|j| j.borrow_mut().take());
    let task = host::begin_task(TASK_TITLE, true);
    match Job::start(&task, games, manual, budget) {
        Some(job) => proceed(job, &task, natives),
        None => host::log(LogLevel::Info, "natives: update cancelled"),
    }
}

/// The next share of the update in progress: true when there was one, and it took this call's budget.
/// At most one of run, scheduled, resume per call.
pub fn resume(natives: &Natives) -> bool {
    let Some(job) = JOB.with(|j| j.borrow_mut().take()) else { return false };
    let task = host::begin_task(TASK_TITLE, true);
    proceed(job, &task, natives);
    true
}

// Nothing detected yet (manifests unread, or none declares a game): either game.
fn games(cx: &CommandCx) -> Vec<Game> {
    let detected = cx.ws.map(Workspace::detected_games).filter(|g| !g.is_empty()).unwrap_or_else(|| Game::ALL.to_vec());
    project_games(cx.settings, &detected)
}

pub fn run(cx: &CommandCx) -> CommandResult {
    start(cx.natives, &games(cx), true, MAX_STORED_DELTA);
    Ok(None)
}

pub fn scheduled(cx: &CommandCx) -> CommandResult {
    if !cx.settings.enabled() || !cx.settings.bool(AUTO_UPDATE, true) {
        return Ok(None);
    }
    if JOB.with(|j| j.borrow().is_some()) {
        resume(cx.natives);
        return Ok(None);
    }
    let checked = match host::storage::get(STATE_KEY) {
        Ok(raw) => raw.and_then(|r| serde_json::from_str::<State>(&r).ok()).map_or(0, |s| s.checked),
        Err(e) => {
            // Without storage nothing would be remembered between checks.
            host::log(LogLevel::Debug, &format!("natives: no automatic update ({e})"));
            return Ok(None);
        }
    };
    // Written when a check starts: one that could not finish waits for the next interval.
    if now_secs().checked_sub(checked).is_some_and(|elapsed| elapsed < CHECK_INTERVAL_SECS) {
        return Ok(None);
    }
    start(cx.natives, &games(cx), false, MAX_STORED_DELTA);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use openidea_plugin::HttpResponse;
    use openidea_plugin::mock::{MockHost, Notification};
    use serde_json::Value;

    const NET: &[&str] = &["storage", "net:static.cfx.re", "net:raw.githubusercontent.com"];
    const LAST_MODIFIED: &str = "Mon, 22 Jun 2026 13:09:01 GMT";

    fn databases(edit: impl Fn(Database, String) -> String) -> HashMap<String, String> {
        Database::ALL.into_iter().map(|d| (d.url().to_owned(), edit(d, d.bundled_text().unwrap_or_default()))).collect()
    }

    fn serve(mock: &MockHost, bodies: HashMap<String, String>) {
        mock.on_fetch(move |r| {
            let body = bodies.get(&r.url).ok_or_else(|| openidea_plugin::Error::Failed("connection refused".into()))?;
            let etag = format!("\"{}\"", sources::fingerprint(body.as_bytes()));
            let matches = r.headers.iter().any(|(n, v)| n == "If-None-Match" && *v == etag);
            let headers = vec![("etag".to_owned(), etag), ("last-modified".to_owned(), LAST_MODIFIED.to_owned())];
            let body = if matches { Vec::new() } else { body.clone().into_bytes() };
            Ok(HttpResponse { status: if matches { 304 } else { 200 }, headers, body })
        });
    }

    fn last(mock: &MockHost) -> Notification {
        mock.notifications().pop().expect("a notification")
    }

    fn finish(natives: &Natives) -> usize {
        let mut calls = 1;
        while pending() {
            resume(natives);
            calls += 1;
        }
        calls
    }

    fn update(natives: &Natives) -> usize {
        start(natives, &Game::ALL, true, MAX_STORED_DELTA);
        finish(natives)
    }

    fn pending() -> bool {
        JOB.with(|j| j.borrow().is_some())
    }

    fn edited_gta5(d: Database, text: String) -> String {
        if d != Database::Gta5 {
            return text;
        }
        let text = text.replacen("This native gets the ground level", "UPDATED: this native gets the ground level", 1);
        text.replacen('{', r#"{"ZZZ":{"0x1234":{"name":"ZZ_NEW_NATIVE","params":[],"results":"void"}},"#, 1)
    }

    fn edited_everywhere(d: Database, text: String) -> String {
        match d {
            Database::Gta5 => edited_gta5(d, text),
            Database::Cfx => text.replacen(
                r#"{"CFX":{"#,
                r#"{"CFX":{"0xAAAAAAAA":{"name":"ZZ_SHARED_NATIVE","params":[],"results":"void","description":"Added.","hash":"0xAAAAAAAA","ns":"CFX","apiset":"shared"},"#,
                1,
            ),
            Database::Rdr3 => text.replacen(r#"Ped "ped""#, r#"Ped "targetPed""#, 1),
            Database::Rdr3Docs => text.replacen(r#""comment": "Counts up."#, r#""comment": "UPDATED: counts up."#, 1),
        }
    }

    fn command<R>(mock: &MockHost, natives: &Natives, ws: Option<&Workspace>, f: impl FnOnce(&CommandCx) -> R) -> R {
        let inv = mock.invocation();
        let settings = Settings::defaults();
        f(&CommandCx { ws, settings: &settings, natives, doc: None, inv: &inv })
    }

    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0xff, 0, 0x80]] {
            let text = base64_encode(data);
            assert_eq!(base64_decode(&text).as_deref(), Some(data), "{text}");
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_decode("Zm8"), None);
        assert_eq!(base64_decode("Zm8=Zm8="), None);
        assert_eq!(base64_decode("Z!8="), None);
    }

    #[test]
    fn deltas_apply_to_the_snapshot() {
        let set = NativeSet::Server;
        let bundled = sources::bundled_groups(set);
        let mut new = bundled.clone();
        new.retain(|g| g.native.lua_name != "GetPlayerName");
        let command = new.iter_mut().find(|g| g.native.lua_name == "RegisterCommand").unwrap();
        command.native.description = "Changed.".into();
        let mut added = new[0].clone();
        added.native.lua_name = "AaNewNative".into();
        new.push(added);
        convert::sort_groups(&mut new);
        let delta = diff(set, &bundled, new.clone(), Inputs::new());
        assert_eq!(delta.removed, ["GetPlayerName"]);
        let changed: Vec<&str> = delta.groups.iter().map(|g| g.native.lua_name.as_str()).collect();
        assert_eq!(changed, ["AaNewNative", "RegisterCommand"]);
        assert_eq!(delta.functions, convert::functions(&new));
        let encoded = encode_delta(&delta, MAX_STORED_DELTA).unwrap();
        assert!(encoded.len() < 8 * 1024, "{} bytes", encoded.len());
        assert_eq!(encode_delta(&delta, encoded.len()).as_ref(), Some(&encoded));
        assert_eq!(encode_delta(&delta, encoded.len() - 1), None, "stops at the limit");
        let decoded = decode_delta(&encoded).unwrap();
        assert_eq!(decoded, delta);
        assert_eq!(apply(bundled, &decoded), new);
    }

    #[test]
    fn notification_texts() {
        let mut o = Outcome {
            changed: vec![NativeSet::Server],
            counts: [None, None, Some(390)],
            origin: Origin::Remote,
            error: None,
            unstored: Vec::new(),
            storage: true,
        };
        let text = |o: &Outcome, manual| notification(o, manual).map(|(l, t, b)| (l, t.to_owned(), b));
        assert_eq!(
            text(&o, false),
            Some((
                NotificationLevel::Info,
                "CfxLua: natives updated".into(),
                "Server natives (FXServer): 390 functions".into()
            ))
        );
        o.error = Some("gta5: Connection refused <timeout>".into());
        o.origin = Origin::Cache;
        o.unstored = vec![NativeSet::Server];
        assert_eq!(
            text(&o, false).map(|(_, _, b)| b).as_deref(),
            Some(
                "Server natives (FXServer): 390 functions\n\
                 Server natives (FXServer): too large to be stored, used until the plugin is restarted.\n\
                 gta5: Connection refused <timeout>: the previously downloaded natives are used."
            )
        );
        assert_eq!(
            describe(&o),
            "390 functions from the cached native database (offline), updated Server natives (FXServer). Error: gta5: Connection refused <timeout>"
        );
        o.changed.clear();
        assert_eq!(text(&o, false), None, "unchanged automatic updates are silent");
        o.origin = Origin::Bundled;
        assert_eq!(
            text(&o, true),
            Some((
                NotificationLevel::Warning,
                "CfxLua: natives could not be updated".into(),
                "gta5: Connection refused <timeout>: the natives bundled with the plugin are used.".into()
            ))
        );
        o.error = None;
        o.origin = Origin::Remote;
        o.counts = [Some(9930), Some(10110), Some(394)];
        assert_eq!(
            text(&o, true),
            Some((
                NotificationLevel::Info,
                "CfxLua: natives are up to date".into(),
                "20,434 functions from the Cfx.re native database, already up to date.".into()
            ))
        );
        assert_eq!(
            (grouped(0), grouped(999), grouped(1000), grouped(1234567)),
            ("0".into(), "999".into(), "1,000".into(), "1,234,567".into())
        );
    }

    #[test]
    fn without_network_the_bundled_natives_stay() {
        let mock = MockHost::install();
        let natives = Natives::bundled();
        assert_eq!(update(&natives), 1);
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Warning, "CfxLua: natives could not be updated"));
        assert_eq!(
            n.body,
            "gta5: permission denied: net:static.cfx.re is not granted; cfx: permission denied: net:static.cfx.re is not granted; \
             rdr3: permission denied: net:static.cfx.re is not granted; \
             rdr3_docs: permission denied: net:raw.githubusercontent.com is not granted: the natives bundled with the plugin are used."
        );
        let tasks = mock.tasks();
        assert_eq!((tasks.len(), tasks[0].title.as_str(), tasks[0].ended), (1, TASK_TITLE, true));
        assert!(mock.storage().keys().all(|k| k == STATE_KEY), "no update stored");
        assert!(!natives.is_loaded(NativeSet::Gta5), "nothing converted");
    }

    #[test]
    fn up_to_date_then_not_modified() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(|_, t| t));
        let natives = Natives::bundled();
        update(&natives);
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Info, "CfxLua: natives are up to date"));
        assert_eq!(n.body, "20,434 functions from the Cfx.re native database, already up to date.");
        assert!(mock.requests().iter().all(|r| r.headers.is_empty()), "nothing to validate yet");
        let state: State = serde_json::from_str(&mock.storage()[STATE_KEY]).unwrap();
        assert_eq!(state.databases.len(), 4);
        assert!(now_secs() - state.checked < 60);

        assert_eq!(update(&natives), 1, "the catalogs are loaded: nothing to convert");
        let second = &mock.requests()[4..];
        assert_eq!(second.len(), 4, "a 304 for the bundled content needs no second download");
        for r in second {
            assert!(r.headers.iter().any(|(n, v)| n == "If-Modified-Since" && v == LAST_MODIFIED), "{r:?}");
            assert!(r.headers.iter().any(|(n, _)| n == "If-None-Match"), "{r:?}");
        }
        assert_eq!(last(&mock).title, "CfxLua: natives are up to date");
        assert!(!mock.storage().keys().any(|k| k.starts_with("natives.delta.")));
    }

    #[test]
    fn a_changed_database_updates_its_sets() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_gta5));
        let natives = Natives::bundled();
        update(&natives);
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Info, "CfxLua: natives updated"));
        assert_eq!(n.body, "FiveM client natives (GTA V): 9,931 functions", "only the set of natives.json");
        assert_eq!(take_changed(), [NativeSet::Gta5]);
        let stored = mock.storage();
        assert_eq!(
            stored.keys().filter(|k| k.starts_with("natives.delta.")).collect::<Vec<_>>(),
            ["natives.delta.gta5"]
        );
        let delta = read_stored(NativeSet::Gta5).expect("stored update");
        let changed: Vec<&str> = delta.groups.iter().map(|g| g.native.lua_name.as_str()).collect();
        assert_eq!(changed, ["GetGroundZFor_3dCoord", "ZzNewNative"]);
        assert!(delta.removed.is_empty());
        assert_eq!(current_delta(NativeSet::Gta5).as_deref(), Some(&delta));

        // Catalogs loaded from now on apply it.
        let fresh = Natives::bundled();
        let gta5 = fresh.catalog(NativeSet::Gta5);
        assert_eq!(gta5.len(), 9931);
        assert!(gta5.get("ZzNewNative").is_some_and(|n| n.namespace == "ZZZ"));
        assert!(gta5.get("GetGroundZFor_3dCoord").is_some_and(|n| n.description.starts_with("UPDATED: ")));
        assert!(fresh.catalog(NativeSet::Server).get("ZzNewNative").is_none());

        // The database goes back to the bundled content: so does the set, in one conversion.
        serve(&mock, databases(|_, t| t));
        assert_eq!(update(&natives), 1);
        assert_eq!(last(&mock).body, "FiveM client natives (GTA V): 9,930 functions");
        assert!(!mock.storage().contains_key("natives.delta.gta5"));
        assert_eq!(current_delta(NativeSet::Gta5), None);
    }

    #[test]
    fn an_update_split_over_calls_stores_what_one_pass_would() {
        let mock = MockHost::install().with_capabilities(NET);
        let bodies = databases(edited_everywhere);
        serve(&mock, bodies.clone());
        let natives = Natives::bundled();
        start(&natives, &Game::ALL, true, MAX_STORED_DELTA);
        assert!(pending() && mock.notifications().is_empty(), "one summary, at the end");
        let calls = finish(&natives);
        assert!(calls >= 3, "{calls} calls");
        assert_eq!(mock.tasks().len(), calls);
        assert_eq!(mock.notifications().len(), 1);
        assert_eq!(
            last(&mock).body,
            "FiveM client natives (GTA V): 9,932 functions\nRedM client natives (RDR3): 10,111 functions\n\
             Server natives (FXServer): 395 functions"
        );
        let mut changed = take_changed();
        changed.sort();
        assert_eq!(changed, NativeSet::ALL);
        let texts: HashMap<Database, &str> = Database::ALL.into_iter().map(|d| (d, bodies[d.url()].as_str())).collect();
        for set in NativeSet::ALL {
            let inputs = sources::databases(set)
                .iter()
                .map(|d| (d.id().to_owned(), sources::fingerprint(texts[d].as_bytes())))
                .collect();
            let new = sources::groups(set, |d| texts.get(&d).copied()).unwrap();
            let expected = diff(set, &sources::bundled_groups(set), new, inputs);
            assert!(!expected.groups.is_empty(), "{set:?}");
            assert_eq!(read_stored(set).as_ref(), Some(&expected), "{set:?}");
        }
    }

    #[test]
    fn only_the_sets_of_the_project_are_updated() {
        let mock = MockHost::install();
        let natives = Natives::bundled();
        let mut ws = Workspace::new("/srv");
        let games_of = |ws: Option<&Workspace>| command(&mock, &natives, ws, games);
        assert_eq!(games_of(None), Game::ALL);
        assert_eq!(games_of(Some(&ws)), Game::ALL, "no manifest read yet");
        ws.set("/srv/res/fxmanifest.lua", Some("fx_version 'cerulean'\ngame 'rdr3'\n"));
        assert_eq!(games_of(Some(&ws)), [Game::Rdr3]);

        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_everywhere));
        ws.set("/srv/res/fxmanifest.lua", Some("fx_version 'cerulean'\ngame 'gta5'\n"));
        command(&mock, &natives, Some(&ws), run).unwrap();
        finish(&natives);
        let urls: Vec<String> = mock.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(urls, [Database::Gta5.url(), Database::Cfx.url()]);
        assert_eq!(
            last(&mock).body,
            "FiveM client natives (GTA V): 9,932 functions\nServer natives (FXServer): 395 functions"
        );
        assert!(!mock.storage().contains_key("natives.delta.rdr3") && known(NativeSet::Rdr3).is_none());
    }

    #[test]
    fn the_update_goes_on_in_later_calls() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_gta5));
        let natives = Natives::bundled();
        command(&mock, &natives, None, scheduled).unwrap();
        assert!(pending() && mock.notifications().is_empty());
        let state: State = serde_json::from_str(&mock.storage()[STATE_KEY]).unwrap();
        assert!(now_secs() - state.checked < 60, "the check is recorded before the work");
        command(&mock, &natives, None, scheduled).unwrap();
        assert!(!pending());
        assert_eq!(mock.requests().len(), 4, "continued, not started again");
        assert_eq!(last(&mock).body, "FiveM client natives (GTA V): 9,931 functions");
        assert!(mock.storage().contains_key("natives.delta.gta5"));
    }

    #[test]
    fn an_interrupted_update_waits_for_the_next_check() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_gta5));
        let natives = Natives::bundled();
        command(&mock, &natives, None, scheduled).unwrap();
        assert!(pending());
        // The instance is reclaimed in between.
        JOB.with(|j| j.borrow_mut().take());
        command(&mock, &natives, None, scheduled).unwrap();
        assert!(!resume(&natives));
        assert_eq!(mock.requests().len(), 4, "no new check before the interval");
        assert!(mock.notifications().is_empty() && !mock.storage().contains_key("natives.delta.gta5"));

        update(&natives);
        assert_eq!(last(&mock).body, "FiveM client natives (GTA V): 9,931 functions");
    }

    #[test]
    fn an_update_too_large_to_store_is_kept_in_memory() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_gta5));
        let natives = Natives::bundled();
        start(&natives, &Game::ALL, true, 16);
        finish(&natives);
        assert_eq!(
            last(&mock).body,
            "FiveM client natives (GTA V): 9,931 functions\n\
             FiveM client natives (GTA V): too large to be stored, used until the plugin is restarted."
        );
        assert!(!mock.storage().contains_key("natives.delta.gta5"));
        assert!(Natives::bundled().catalog(NativeSet::Gta5).get("ZzNewNative").is_some());
    }

    #[test]
    fn invalid_content_is_not_used() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(|d, t| if d == Database::Cfx { "<html><body>502</body></html>".into() } else { t }));
        let natives = Natives::bundled();
        update(&natives);
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Warning, "CfxLua: natives could not be updated"));
        assert!(n.body.starts_with("cfx: invalid content (invalid JSON: "), "{}", n.body);
        assert!(n.body.ends_with(": the natives bundled with the plugin are used."), "{}", n.body);
        assert_eq!(n.body.matches("cfx:").count(), 1, "reported once: {}", n.body);
        let state: State = serde_json::from_str(&mock.storage()[STATE_KEY]).unwrap();
        assert!(!state.databases.contains_key("cfx") && state.databases.contains_key("gta5"));
    }

    #[test]
    fn scheduled_checks_at_most_daily() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(|_, t| t));
        let natives = Natives::bundled();
        let inv = mock.invocation();
        let scheduled_with = |settings: &Settings| {
            let cx = CommandCx { ws: None, settings, natives: &natives, doc: None, inv: &inv };
            scheduled(&cx).unwrap();
        };
        scheduled_with(&Settings::defaults().with(AUTO_UPDATE, Value::from(false)));
        assert!(mock.requests().is_empty(), "automatic updates off");
        scheduled_with(&Settings::defaults());
        assert_eq!(mock.requests().len(), 4);
        assert!(mock.notifications().is_empty(), "unchanged: silent");
        assert!(!pending());
        scheduled_with(&Settings::defaults());
        assert_eq!(mock.requests().len(), 4, "checked less than a day ago");

        let mock = MockHost::install().with_capabilities(&["net:static.cfx.re"]);
        let inv = mock.invocation();
        let cx = CommandCx { ws: None, settings: &Settings::defaults(), natives: &natives, doc: None, inv: &inv };
        scheduled(&cx).unwrap();
        assert!(mock.requests().is_empty() && mock.tasks().is_empty(), "nothing remembered without storage");
    }
}
