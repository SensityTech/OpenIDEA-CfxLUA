//! Natives updates ("Update Natives"): the four databases are downloaded
//! conditionally (validators kept in the plugin's storage) and each set whose
//! content changed is converted again.
//!
//! The plugin's storage (1 MiB) cannot hold the databases: what is kept per
//! set is its difference with the bundled snapshot (groups added or changed,
//! primaries removed), gzip + base64, with the fingerprints of the content it
//! was made from. A difference too large to store is used by this instance
//! only. Catalogs apply the difference when they load ([`super::load`]).

use super::Natives;
use super::convert::{self, Group};
use super::model::NativeSet;
use super::sources::{self, Database, Inputs};
use crate::feature::{CommandCx, CommandResult};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use openidea_plugin::{HttpRequest, LogLevel, NotificationLevel, host};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::rc::Rc;

pub const UPDATE: &str = "natives.update";
pub const SCHEDULED_UPDATE: &str = "natives.scheduled-update";
/// Setting: check for new natives automatically (at most once a day).
pub const AUTO_UPDATE: &str = "natives.autoUpdate";

const STATE_KEY: &str = "natives.state";
const DELTA_FORMAT: u32 = 1;
/// Largest stored difference of a set (base64): the three fit in a third of
/// the storage at most, leaving room for the other features.
pub const MAX_STORED_DELTA: usize = 112 * 1024;
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const TIMEOUT_MS: u32 = 15_000;
const TASK_TITLE: &str = "CfxLua: updating natives";

/// What the plugin remembers between sessions.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// When the databases were last checked (seconds since the epoch).
    #[serde(default)]
    checked: u64,
    /// The last download of each database, by id.
    #[serde(default)]
    databases: BTreeMap<String, Validators>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Validators {
    etag: Option<String>,
    last_modified: Option<String>,
    /// Fingerprint of the content they validate.
    fingerprint: String,
}

/// The difference between a set and the bundled snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    format: u32,
    /// The snapshot it applies to ([`sources::snapshot_id`]).
    base: String,
    /// What the set was converted from.
    inputs: Inputs,
    /// Functions of the set once applied.
    functions: usize,
    /// Primaries of the snapshot that are gone.
    removed: Vec<String>,
    /// Groups added or changed.
    groups: Vec<Group>,
}

thread_local! {
    /// The update in effect per set, once this instance knows it (`Some(None)`: none).
    static IN_EFFECT: RefCell<[Option<Option<Rc<Delta>>>; 3]> = const { RefCell::new([None, None, None]) };
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

/// The stored update of `set`; one made for another snapshot or unreadable is deleted.
fn read_stored(set: NativeSet) -> Option<Delta> {
    let key = delta_key(set);
    let raw = host::storage::get(&key).ok()??;
    let delta = decode_delta(&raw).filter(|d| d.format == DELTA_FORMAT && d.base == sources::snapshot_id(set));
    if delta.is_none() {
        let _ = host::storage::delete(&key);
    }
    delta
}

/// The update in effect for `set`, read from the storage once per instance.
fn effective(set: NativeSet) -> Option<Rc<Delta>> {
    if let Some(delta) = known(set) {
        return delta;
    }
    let stored = read_stored(set).map(Rc::new);
    remember(set, stored.clone());
    stored
}

/// The update catalogs of `set` apply. Unit tests of every area build catalogs
/// without a host: there, only updates made in the test count.
pub fn current_delta(set: NativeSet) -> Option<Rc<Delta>> {
    if cfg!(test) { known(set).flatten() } else { effective(set) }
}

/// The bundled groups of a set with an update applied, sorted.
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

/// The update that turns the `bundled` groups of `set` into `new`.
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

fn encode_delta(delta: &Delta) -> Option<String> {
    let json = serde_json::to_vec(delta).ok()?;
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    gz.write_all(&json).ok()?;
    Some(base64_encode(&gz.finish().ok()?))
}

fn decode_delta(text: &str) -> Option<Delta> {
    let gz = base64_decode(text)?;
    let mut json = Vec::new();
    GzDecoder::new(gz.as_slice()).read_to_end(&mut json).ok()?;
    serde_json::from_slice(&json).ok()
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (the storage keeps text).
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

/// Where the natives in effect come from (the worst database is reported).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    /// Downloaded now, or confirmed by the server.
    Remote,
    /// The server could not be reached: the previous download is used.
    Cache,
    /// Nothing downloaded: the snapshot bundled with the plugin.
    Bundled,
}

/// What an update did.
#[derive(Debug)]
struct Outcome {
    changed: Vec<NativeSet>,
    /// Functions per set, for the sets the notification names.
    counts: [Option<usize>; 3],
    origin: Origin,
    error: Option<String>,
    /// Changed sets whose update is kept in memory only.
    unstored: Vec<NativeSet>,
    /// Whether the plugin may use its storage.
    storage: bool,
}

/// A database during an update.
enum Content {
    /// Not obtained (the error is reported).
    Failed,
    /// Unchanged since the last download (HTTP 304): its fingerprint.
    Known(String),
    /// Downloaded by this update.
    Fetched { fingerprint: String, text: String },
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

/// Downloads `db` (`conditional`: with the validators of the last download);
/// new content is checked before its validators are kept.
fn obtain(db: Database, state: &mut State, errors: &mut Vec<String>, conditional: bool) -> Content {
    let last = state.databases.get(db.id()).cloned();
    match download(db, last.as_ref().filter(|_| conditional)) {
        Ok(Answer::NotModified) => match last {
            Some(v) if conditional => Content::Known(v.fingerprint),
            _ => {
                errors.push(format!("{}: HTTP 304 without a valid cached copy", db.id()));
                Content::Failed
            }
        },
        Ok(Answer::Body { bytes, etag, last_modified }) => {
            let fingerprint = sources::fingerprint(&bytes);
            let text = sources::decode(bytes);
            let checked = last.is_some_and(|v| v.fingerprint == fingerprint) || fingerprint == db.bundled_fingerprint();
            if !checked && let Err(e) = db.validate(&text) {
                errors.push(format!("{}: invalid content ({e})", db.id()));
                return Content::Failed;
            }
            let validators = Validators { etag, last_modified, fingerprint: fingerprint.clone() };
            state.databases.insert(db.id().to_owned(), validators);
            Content::Fetched { fingerprint, text }
        }
        Err(e) => {
            errors.push(format!("{}: {e}", db.id()));
            Content::Failed
        }
    }
}

/// The content `set` should be built from: what the databases have now, or
/// what is in effect for those that could not be checked.
fn wanted(set: NativeSet, content: &[Content], in_effect: &Inputs) -> Inputs {
    sources::databases(set)
        .iter()
        .filter_map(|&db| {
            let fp = match &content[db as usize] {
                Content::Known(fp) | Content::Fetched { fingerprint: fp, .. } => fp.clone(),
                Content::Failed => in_effect.get(db.id())?.clone(),
            };
            Some((db.id().to_owned(), fp))
        })
        .collect()
}

/// Stores the update of `set` (`None`: back to the snapshot); false when it does not fit.
fn save(set: NativeSet, delta: Option<&Delta>, budget: usize) -> bool {
    let key = delta_key(set);
    match delta {
        None => {
            let _ = host::storage::delete(&key);
            true
        }
        Some(d) => encode_delta(d).is_some_and(|text| text.len() <= budget && host::storage::set(&key, &text).is_ok()),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Checks the four databases and converts the sets whose content changed.
/// `None` when the user cancelled it.
fn update(task: &host::Task, natives: &Natives, manual: bool, budget: usize) -> Option<Outcome> {
    let (mut state, storage) = match host::storage::get(STATE_KEY) {
        Ok(raw) => (raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default(), true),
        Err(_) => (State::default(), false),
    };
    let steps = (Database::ALL.len() + NativeSet::ALL.len()) as u32;
    let mut errors = Vec::new();
    let mut content = Vec::with_capacity(Database::ALL.len());
    for (i, db) in Database::ALL.into_iter().enumerate() {
        if task.is_cancelled() {
            return None;
        }
        task.report(i as u32, Some(steps), Some(db.file_name()));
        content.push(obtain(db, &mut state, &mut errors, true));
    }

    let mut outcome = Outcome {
        changed: Vec::new(),
        counts: [None; 3],
        origin: Origin::Remote,
        error: None,
        unstored: Vec::new(),
        storage,
    };
    for (i, set) in NativeSet::ALL.into_iter().enumerate() {
        if task.is_cancelled() {
            return None;
        }
        let in_effect = effective(set).map_or_else(|| sources::bundled_inputs(set), |d| d.inputs.clone());
        if wanted(set, &content, &in_effect) == in_effect {
            continue;
        }
        task.report((Database::ALL.len() + i) as u32, Some(steps), Some(set.display_name()));
        // A 304 does not carry the content: download it again unless it is the bundled one.
        for &db in sources::databases(set) {
            if matches!(&content[db as usize], Content::Known(fp) if *fp != db.bundled_fingerprint()) {
                content[db as usize] = obtain(db, &mut state, &mut errors, false);
            }
        }
        let wanted = wanted(set, &content, &in_effect);
        let texts: HashMap<Database, String> = sources::databases(set)
            .iter()
            .filter_map(|&db| {
                let fp = wanted.get(db.id())?;
                let text = match &content[db as usize] {
                    Content::Fetched { fingerprint, text } if fingerprint == fp => text.clone(),
                    _ if *fp == db.bundled_fingerprint() => db.bundled_text().ok()?,
                    _ => return None,
                };
                Some((db, text))
            })
            .collect();
        if wanted == in_effect || texts.len() < sources::databases(set).len() {
            // Unchanged after all, or a database could not be obtained (reported).
            continue;
        }
        let groups = match sources::groups(set, |db| texts.get(&db).cloned()) {
            Ok(groups) => groups,
            Err((db, e)) => {
                errors.push(format!("{}: invalid content ({e})", db.id()));
                continue;
            }
        };
        let functions = convert::functions(&groups);
        let delta = (wanted != sources::bundled_inputs(set))
            .then(|| Rc::new(diff(set, &sources::bundled_groups(set), groups, wanted)));
        if !save(set, delta.as_deref(), budget) {
            outcome.unstored.push(set);
        }
        remember(set, delta);
        outcome.changed.push(set);
        outcome.counts[set as usize] = Some(functions);
    }

    state.checked = now_secs();
    if let Ok(json) = serde_json::to_string(&state)
        && host::storage::set(STATE_KEY, &json).is_err()
    {
        host::log(LogLevel::Debug, "natives: the update state cannot be stored");
    }
    for (db, c) in Database::ALL.into_iter().zip(&content) {
        if matches!(c, Content::Failed) {
            let downloaded = state.databases.get(db.id()).is_some_and(|v| v.fingerprint != db.bundled_fingerprint());
            outcome.origin = outcome.origin.max(if downloaded { Origin::Cache } else { Origin::Bundled });
        }
    }
    outcome.error = (!errors.is_empty()).then(|| errors.join("; "));
    if manual && outcome.changed.is_empty() && outcome.error.is_none() {
        // "N functions from …": every set counts.
        for set in NativeSet::ALL {
            let functions = effective(set).map_or_else(|| natives.catalog(set).len(), |d| d.functions);
            outcome.counts[set as usize] = Some(functions);
        }
    }
    Some(outcome)
}

/// `7,088`.
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

/// The notification of an update: changes always, the rest when asked for.
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

fn run_with(natives: &Natives, manual: bool, budget: usize) -> CommandResult {
    let outcome = {
        let task = host::begin_task(TASK_TITLE, true);
        update(&task, natives, manual, budget)
    };
    let Some(outcome) = outcome else {
        host::log(LogLevel::Info, "natives: update cancelled");
        return Ok(None);
    };
    let changed: Vec<&str> = outcome.changed.iter().map(|s| s.id()).collect();
    host::log(
        LogLevel::Info,
        &format!(
            "natives: origin={:?}, changed={changed:?}, error={}",
            outcome.origin,
            outcome.error.as_deref().unwrap_or("none")
        ),
    );
    if let Some((level, title, body)) = notification(&outcome, manual) {
        host::notify(level, title, &body);
    }
    Ok(None)
}

/// `natives.update`: checks now and always reports.
pub fn run(natives: &Natives) -> CommandResult {
    run_with(natives, true, MAX_STORED_DELTA)
}

/// `natives.scheduled-update` (the plugin's scheduled event): at most one
/// silent check a day, when automatic updates are on.
pub fn scheduled(cx: &CommandCx) -> CommandResult {
    if !cx.settings.enabled() || !cx.settings.bool(AUTO_UPDATE, true) {
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
    if now_secs().checked_sub(checked).is_some_and(|elapsed| elapsed < CHECK_INTERVAL_SECS) {
        return Ok(None);
    }
    run_with(cx.natives, false, MAX_STORED_DELTA)
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

    /// The bundled databases, with `edit` applied to some.
    fn databases(edit: impl Fn(Database, String) -> String) -> HashMap<String, String> {
        Database::ALL.into_iter().map(|d| (d.url().to_owned(), edit(d, d.bundled_text().unwrap_or_default()))).collect()
    }

    /// Serves `bodies` by URL, with an ETag (the content's fingerprint) and a Last-Modified; 304 when it matches.
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

    /// GTA V natives.json with one description changed and one native added.
    fn edited_gta5(d: Database, text: String) -> String {
        if d != Database::Gta5 {
            return text;
        }
        let text = text.replacen("This native gets the ground level", "UPDATED: this native gets the ground level", 1);
        text.replacen('{', r#"{"ZZZ":{"0x1234":{"name":"ZZ_NEW_NATIVE","params":[],"results":"void"}},"#, 1)
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
        let encoded = encode_delta(&delta).unwrap();
        assert!(encoded.len() < 8 * 1024, "{} bytes", encoded.len());
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
        run(&natives).unwrap();
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
        run(&natives).unwrap();
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Info, "CfxLua: natives are up to date"));
        assert_eq!(n.body, "20,434 functions from the Cfx.re native database, already up to date.");
        assert!(mock.requests().iter().all(|r| r.headers.is_empty()), "nothing to validate yet");
        let state: State = serde_json::from_str(&mock.storage()[STATE_KEY]).unwrap();
        assert_eq!(state.databases.len(), 4);
        assert!(now_secs() - state.checked < 60);

        run(&natives).unwrap();
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
        run(&natives).unwrap();
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Info, "CfxLua: natives updated"));
        assert_eq!(n.body, "FiveM client natives (GTA V): 9,931 functions", "only the set of natives.json");
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

        // The database goes back to the bundled content: so does the set.
        serve(&mock, databases(|_, t| t));
        run(&natives).unwrap();
        assert_eq!(last(&mock).body, "FiveM client natives (GTA V): 9,930 functions");
        assert!(!mock.storage().contains_key("natives.delta.gta5"));
        assert_eq!(current_delta(NativeSet::Gta5), None);
    }

    #[test]
    fn an_update_too_large_to_store_is_kept_in_memory() {
        let mock = MockHost::install().with_capabilities(NET);
        serve(&mock, databases(edited_gta5));
        let natives = Natives::bundled();
        run_with(&natives, true, 16).unwrap();
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
        run(&natives).unwrap();
        let n = last(&mock);
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Warning, "CfxLua: natives could not be updated"));
        assert!(n.body.starts_with("cfx: invalid content (invalid JSON: "), "{}", n.body);
        assert!(n.body.ends_with(": the natives bundled with the plugin are used."), "{}", n.body);
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
        scheduled_with(&Settings::defaults());
        assert_eq!(mock.requests().len(), 4, "checked less than a day ago");

        let mock = MockHost::install().with_capabilities(&["net:static.cfx.re"]);
        let inv = mock.invocation();
        let cx = CommandCx { ws: None, settings: &Settings::defaults(), natives: &natives, doc: None, inv: &inv };
        scheduled(&cx).unwrap();
        assert!(mock.requests().is_empty() && mock.tasks().is_empty(), "nothing remembered without storage");
    }
}
