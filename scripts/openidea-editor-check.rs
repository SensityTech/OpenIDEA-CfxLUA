// Asks a plugin package what the editor would receive for a file, through OpenIDEA's own plugin host and without
// the editor: semantic tokens, folds, structure, the occurrences of a word, what hovering it shows, where it is
// declared and the parameters shown after its parenthesis, what those questions, the code actions and the ranges
// of Extend Selection cost all over the file, the strings it gives to another language, the hints of each screen
// of it, then what the editor asks after an edit, at its own delays. The calls
// run under the host's limits for editor calls, so a provider that is too slow answers nothing here as it would in
// the IDE. The host then stops waiting at its deadline and the time printed is its wait, not what the provider
// costs: `--patient` lifts the deadline to show that cost. An entry of the structure that its parent does not hold
// fails the check: the IDE would list it at the top.
// Without `--project`, no project is open in this host and the plugin loads no native catalog: natives come back as
// unknown globals and completion lists none. With it, the host opens that folder as a project and events are posted
// until the plugin has read it, as they are while a user moves between files: the file is then asked about as one
// of an indexed project, and the problems the plugin publishes for it after the edit are counted. `--log` prints
// on stderr what the host logs, the lines of the plugin included.
// `--type <text>` is what the edit types on a new last line, `GetEnt` by default: the completion is asked at its
// end and its first entries are printed.
// `--index`, with `--project`, gives each Lua file of the project to the document indexer of the plugin, as the
// IDE does once it has listed the project, then asks at places of the file what Find Usages would gather there.
// `--set <key>=<value>`, with `--project` and as many times as needed, gives the plugin the value of one of its
// settings as the settings page of the IDE would: it is read as JSON, or as a string when it is none.
// `--command <id>`, with `--project`, runs that command of the plugin once the project is read, as from the window
// of the project with no editor focused, and prints how long it took and what it notified.
// `--rename <n>=<name>`, with `--project`, chooses the action of the file that renames in the project, the one
// offered at the n-th name that has one (0 is the first), answers `<name>` when the plugin asks the new name, and
// prints how long the action took to resolve and how many files and edits it gave. `<n>x<count>` resolves that
// one and the `count - 1` after it, each name once, and ends with how many resolved and the slowest. The text
// search the plugin asks the IDE for is played here: the Lua files of the project that write the word, whole
// and in its case.
// `--fuel <n>` gives each call of the plugin that many instructions instead of the five billion of the IDE: a
// call that passes with a quarter of them is far from the limit.
// Built like scripts/openidea-start-check.rs, as an example of OpenIDEA's `ide-host` crate:
//
//   cp scripts/openidea-editor-check.rs <openidea>/crates/ide-host/examples/cfxlua_editor_check.rs
//   cargo run --release --locked -p ide-host --example cfxlua_editor_check -- \
//       [--patient] [--log] [--project <folder>] [--index] [--set <key>=<value>] [--command <id>] \
//       [--rename <n>[x<count>]=<name>] [--fuel <n>] [--type <text>] <package.oiplugin> <file.lua> [word]

use ide_core::{CancellationToken, Pos, Range, TextEdit};
use ide_host::plugins::navigation::{MAX_DOCUMENT_BYTES, Snapshot};
use ide_host::plugins::{Consent, PluginEvent, PluginHost, PluginWorkspace};
use ide_plugin::{
    HostError, Invocation, Limits, Location, ProviderDocument, TextMatch, TextQuery, TextSearch, WorkbenchEvent,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: cfxlua_editor_check [--patient] [--log] [--project <folder>] [--index] \
                     [--set <key>=<value>] [--command <id>] [--rename <n>[x<count>]=<name>] [--fuel <n>] \
                     [--type <text>] <package.oiplugin> <file.lua> [word]";
/// How many entries of the completion are printed.
const SHOWN: usize = 12;
/// More event batches than any project needs: a scan that never ends stops the check instead of hanging it.
const MAX_BATCHES: usize = 5_000;
/// How many places of the file one question is asked at.
const SAMPLED: usize = 200;
/// How many lines the editor asks the hints of at once.
const SCREEN: u32 = 60;
const KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// How many problems the plugin last published: errors, warnings, infos, hints, and those shown faded.
type Published = Arc<Mutex<Option<[usize; 5]>>>;

/// The values the user gave to settings of the plugin.
type Settings = serde_json::Map<String, serde_json::Value>;

/// One open project, with no editor on it: what the plugin reads comes from the disk. Its Lua files are read
/// once for the searches the plugin asks for.
struct Opened(PathBuf, Published, Settings, OnceLock<Vec<(PathBuf, String)>>);

impl PluginWorkspace for Opened {
    fn projects(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
    fn buffer_text(&self, _: &Path) -> Option<String> {
        None
    }
    fn document(&self, _: &Path) -> Option<(u64, String)> {
        None
    }
    fn apply_edits(&self, _: &Path, _: u64, _: &[TextEdit]) -> Result<u64, HostError> {
        Err(HostError::Stale)
    }
    fn trusted(&self, _: &Path) -> bool {
        true
    }
    fn project_of(&self, _: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn publish_diagnostics(&self, _: &str, _: &Path, found: Vec<ide_core::Diagnostic>) -> Result<(), HostError> {
        let mut counts = [0; 5];
        for problem in &found {
            counts[problem.severity as usize] += 1;
            counts[4] += usize::from(problem.unnecessary);
        }
        if let Ok(mut published) = self.1.lock() {
            *published = Some(counts);
        }
        Ok(())
    }
    fn plugin_settings(&self, _: &str, _: Option<&Path>) -> Settings {
        self.2.clone()
    }
    /// Stands for Find in Files, for what the plugin asks of it: the Lua files of the project that write a
    /// word, whole and in its case.
    fn search_text(
        &self,
        _: Option<&str>,
        root: &Path,
        query: TextQuery,
        limit: u32,
        _: &CancellationToken,
    ) -> Result<TextSearch, HostError> {
        let texts = self.3.get_or_init(|| {
            let mut files = Vec::new();
            lua_files(root, &mut files);
            files.sort();
            files.into_iter().filter_map(|path| Some((std::fs::read_to_string(&path).ok()?, path))).map(|(text, path)| (path, text)).collect()
        });
        let word = |near: Option<char>| near.is_some_and(|near| near.is_alphanumeric() || near == '_');
        let mut out = TextSearch::default();
        for (path, text) in texts {
            for (line, written) in text.lines().enumerate() {
                for (at, found) in written.match_indices(query.pattern.as_str()) {
                    let inside = word(written[..at].chars().next_back()) || word(written[at + found.len()..].chars().next());
                    if query.whole_word && inside {
                        continue;
                    }
                    if out.matches.len() >= limit as usize {
                        out.limit_hit = true;
                        return Ok(out);
                    }
                    let column = written[..at].encode_utf16().count() as u32;
                    let end = Pos::new(line as u32, column + found.encode_utf16().count() as u32);
                    let location = Location { path: path.clone(), range: Range::new(Pos::new(line as u32, column), end) };
                    out.matches.push(TextMatch { location, line: written.to_owned() });
                }
            }
        }
        Ok(out)
    }
}

/// What the plugin tells the IDE while it reads a project.
#[derive(Default)]
struct Scan {
    slices: AtomicUsize,
    ended: AtomicBool,
    /// The title and the body of each notification.
    notified: Mutex<Vec<(String, String)>>,
    /// What is answered to the question the plugin asks next, and the host that takes the answer.
    answer: Mutex<Option<String>>,
    host: OnceLock<Weak<PluginHost>>,
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let patient = args.iter().position(|a| a == "--patient").map(|at| args.remove(at)).is_some();
    if args.iter().position(|a| a == "--log").map(|at| args.remove(at)).is_some() {
        let logs = tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).with_target(false);
        let _ = logs.with_max_level(tracing::Level::INFO).try_init();
    }
    let indexed = args.iter().position(|a| a == "--index").map(|at| args.remove(at)).is_some();
    let mut settings = Settings::new();
    while let Some(at) = args.iter().position(|a| a == "--set") {
        args.remove(at);
        let pair = (at < args.len()).then(|| args.remove(at));
        let Some((key, value)) = pair.as_deref().and_then(|pair| pair.split_once('=')) else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        let value = serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_owned()));
        settings.insert(key.to_owned(), value);
    }
    let command = match args.iter().position(|a| a == "--command") {
        Some(at) if at + 1 < args.len() => {
            args.remove(at);
            Some(args.remove(at))
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let rename = match args.iter().position(|a| a == "--rename") {
        Some(at) if at + 1 < args.len() => {
            args.remove(at);
            let asked = args.remove(at);
            let parsed = asked.split_once('=').and_then(|(nth, name)| {
                let (first, count) = nth.split_once('x').unwrap_or((nth, "1"));
                Some((first.parse::<usize>().ok()?, count.parse::<usize>().ok()?, name.to_owned()))
            });
            let Some(parsed) = parsed else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            Some(parsed)
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let fuel = match args.iter().position(|a| a == "--fuel") {
        Some(at) if at + 1 < args.len() => {
            args.remove(at);
            let Ok(fuel) = args.remove(at).parse::<u64>() else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            Some(fuel)
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let typing = match args.iter().position(|a| a == "--type") {
        Some(at) if at + 1 < args.len() => {
            args.remove(at);
            args.remove(at)
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => "GetEnt".to_owned(),
    };
    let project = match args.iter().position(|a| a == "--project") {
        Some(at) if at + 1 < args.len() => {
            args.remove(at);
            Some(PathBuf::from(args.remove(at)))
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let (Some(package), Some(file)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let limits = if patient {
        Limits { edit_soft: Duration::from_secs(30), edit_hard: Duration::from_secs(30), ..Limits::default() }
    } else {
        Limits::default()
    };
    let limits = Limits { fuel: fuel.unwrap_or(limits.fuel), ..limits };
    let dir = std::env::temp_dir().join(format!("cfxlua-editor-check-{}", std::process::id()));
    let word = args.get(2).map(String::as_str);
    let rename = rename.as_ref().map(|(first, count, name)| (*first, *count, name.as_str()));
    let command = command.as_deref();
    let project = project.as_deref();
    let asked = Asked { project, indexed, settings: &settings, command, rename, word, typing: &typing };
    let result = check(&dir, limits, Path::new(package), Path::new(file), &asked);
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// The text a range of one line covers; columns count UTF-16 units.
fn slice(lines: &[&str], line: u32, from: u32, to: u32) -> String {
    let units: Vec<u16> = lines.get(line as usize).map(|l| l.encode_utf16().collect()).unwrap_or_default();
    let (from, to) = ((from as usize).min(units.len()), (to as usize).min(units.len()));
    String::from_utf16_lossy(&units[from..to.max(from)])
}

/// Where the words of the text start, keywords and numbers left out, in comments and strings too: a mouse
/// stops anywhere.
fn names(lines: &[&str]) -> Vec<Pos> {
    let mut out = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            if at == start {
                at += 1;
            } else if !bytes[start].is_ascii_digit() && !KEYWORDS.contains(&&text[start..at]) {
                out.push(Pos::new(line as u32, text[..start].encode_utf16().count() as u32));
            }
        }
    }
    out
}

/// Where the arguments of the calls of the text start: after each parenthesis that opens.
fn calls(lines: &[&str]) -> Vec<Pos> {
    let mut out = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let mut column = 0;
        for character in text.chars() {
            column += character.len_utf16() as u32;
            if character == '(' {
                out.push(Pos::new(line as u32, column));
            }
        }
    }
    out
}

/// Asks `call` at places spread over `places`: how many answer, how many come after the deadline of the host,
/// which shows nothing of them, the slowest, and the biggest answer.
fn sweep(what: &str, unit: &str, places: &[Pos], deadline: Duration, call: impl Fn(Pos) -> Option<usize>) {
    let (mut count, mut answered, mut late, mut most) = (0, 0, 0, 0);
    let (mut slowest, mut total) = (Duration::ZERO, Duration::ZERO);
    for at in places.iter().step_by((places.len() / SAMPLED).max(1)) {
        let start = Instant::now();
        let size = call(*at);
        let took = start.elapsed();
        count += 1;
        answered += usize::from(size.is_some());
        late += usize::from(took >= deadline);
        most = most.max(size.unwrap_or(0));
        slowest = slowest.max(took);
        total += took;
    }
    let timed = format!("slowest {slowest:?}, {total:?} in all, {most} {unit} at most");
    println!("{what}: {count} asked, {answered} answered, {late} late ({timed})");
}

/// Starts the plugin on the open project and posts events until its first scan ends: it reads a slice per call.
fn read_project(host: &PluginHost, plugin: &str, scan: &Scan) -> Result<(), String> {
    let start = Instant::now();
    host.run_command(plugin, "show-docs", None, Invocation::default()).map_err(|e| format!("show-docs: {e}"))?;
    let mut batches = 0;
    while !scan.ended.load(Ordering::Relaxed) && batches < MAX_BATCHES {
        host.post_events(Some("main"), vec![WorkbenchEvent::EditorFocused(None)]);
        host.flush_events();
        host.wait_events(Duration::from_secs(60));
        batches += 1;
    }
    let slices = scan.slices.load(Ordering::Relaxed);
    println!("project: read in {:?}, {batches} event batches, {slices} indexing slices", start.elapsed());
    if scan.ended.load(Ordering::Relaxed) {
        Ok(())
    } else {
        Err("the plugin did not finish reading the project".into())
    }
}

/// Runs a command of the plugin as from the window of the project, no editor focused: how long it takes, then
/// what it notifies.
fn run_command(host: &PluginHost, plugin: &str, root: &Path, command: &str, scan: &Scan) -> Result<(), String> {
    let root = dunce::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let invocation = Invocation { client: Some("main".into()), workspace_root: Some(root), ..Default::default() };
    let before = scan.notified.lock().map(|notified| notified.len()).unwrap_or(0);
    let start = Instant::now();
    let result = host.run_command(plugin, command, None, invocation);
    let took = start.elapsed();
    let notified = scan.notified.lock().map(|notified| notified[before..].to_vec()).unwrap_or_default();
    let bytes: usize = notified.iter().map(|(title, body)| title.len() + body.len()).sum();
    println!("command: {command} in {took:?}, {} notifications, {bytes} bytes", notified.len());
    for (title, body) in &notified {
        println!("  {title}");
        for line in body.lines() {
            println!("    {line}");
        }
    }
    result.map(|_| ()).map_err(|e| format!("{command}: {e}"))
}

/// Why a rename was refused, in the words of the plugin that name nothing of the project, or of the host.
fn refusal(error: &str, notified: &[(String, String)]) -> &'static str {
    const SAID: [&str; 5] = ["too many places", "too many files", "cannot tell", "could not tell", "does not read"];
    let said = |reason: &str| notified.iter().any(|(_, body)| body.contains(reason));
    if let Some(reason) = SAID.into_iter().find(|reason| said(reason)) {
        return reason;
    }
    let error = error.to_lowercase();
    match () {
        () if error.contains("budget") || error.contains("fuel") => "cpu budget",
        () if error.contains("time") || error.contains("deadline") => "timeout",
        () if error.contains("cancel") => "cancelled",
        () => "another error",
    }
}

/// Chooses the actions that rename in the project at the names of the file that offer one, each name once:
/// `count` of them from the `first`. Answers `name` to the question each asks and tells how it resolved: how
/// long, how many files and edits. What is renamed is printed under each line.
fn rename_in_project(
    host: &PluginHost,
    root: &Path,
    doc: &ProviderDocument,
    lines: &[&str],
    (first, count, name): (usize, usize, &str),
    scan: &Scan,
) -> Result<(), String> {
    let root = dunce::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut titles = std::collections::BTreeSet::new();
    let offered = |at: &Pos| {
        let actions = host.provide_code_actions(doc, Range::point(*at));
        actions.into_iter().find(|(_, action)| action.data.is_some() && action.title.ends_with(" in the project…"))
    };
    let positions = names(lines);
    let distinct = positions.iter().filter_map(offered).filter(|(_, action)| titles.insert(action.title.clone()));
    let chosen: Vec<_> = distinct.skip(first).take(count).collect();
    if chosen.is_empty() {
        println!("rename: no action that renames in the project, {first} skipped");
        return Ok(());
    }
    let (mut resolved_count, mut slowest) = (0, Duration::ZERO);
    for (plugin, action) in &chosen {
        if let Ok(mut answer) = scan.answer.lock() {
            *answer = Some(name.to_owned());
        }
        let before = scan.notified.lock().map(|notified| notified.len()).unwrap_or(0);
        let invocation =
            Invocation { client: Some("main".into()), workspace_root: Some(root.clone()), ..Default::default() };
        let start = Instant::now();
        let resolved = host.resolve_code_action(plugin, doc, action.clone(), invocation);
        let took = start.elapsed();
        slowest = slowest.max(took);
        match resolved {
            Ok(resolved) => {
                resolved_count += 1;
                let edits: usize = resolved.edits.iter().map(|(_, edits)| edits.len()).sum();
                println!("rename: resolved in {took:?}, {} files, {edits} edits", resolved.edits.len());
                println!("  {}", action.title);
                for (path, edits) in &resolved.edits {
                    println!("  {}: {}", path.display(), edits.len());
                }
            }
            Err(e) => {
                let notified = scan.notified.lock().map(|notified| notified[before..].to_vec()).unwrap_or_default();
                let why = refusal(&e.to_string(), &notified);
                println!("rename: refused in {took:?}, {} notifications ({why})", notified.len());
                println!("  {}: {e}", action.title);
                for (title, body) in &notified {
                    println!("  {title}: {body}");
                }
            }
        }
    }
    if chosen.len() > 1 {
        let refused = chosen.len() - resolved_count;
        println!("renames: {resolved_count} resolved, {refused} refused, slowest {slowest:?}");
    }
    Ok(())
}

/// The Lua files under a folder, hidden folders and `node_modules` left out.
fn lua_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            lua_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("lua")) {
            out.push(path);
        }
    }
}

/// Gives each Lua file of the project to the document indexer, then asks at places of the file what Find Usages
/// would gather there. Numbers only, and what the host answers when it refuses a file.
fn index_project(host: &PluginHost, root: &Path, file: &Path, lines: &[&str]) -> Result<(), String> {
    let root = dunce::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let file = dunce::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut files = Vec::new();
    lua_files(&root, &mut files);
    files.sort();
    let (mut given, mut slowest, start) = (0, Duration::ZERO, Instant::now());
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.len() > MAX_DOCUMENT_BYTES {
            continue;
        }
        let invocation = Invocation { workspace_root: Some(root.clone()), ..Default::default() };
        let snapshot = Snapshot { path, version: None, text: &text, symbols: &[], scope: "" };
        let asked = Instant::now();
        match host.index_navigation(&root, snapshot, invocation) {
            Ok(()) => given += 1,
            Err(e) => *refused.entry(e.to_string()).or_default() += 1,
        }
        slowest = slowest.max(asked.elapsed());
    }
    println!("index: {} files, {given} indexed in {:?} (slowest {slowest:?})", files.len(), start.elapsed());
    for (why, count) in &refused {
        println!("  refused {count}: {why}");
    }
    let most_files = std::cell::Cell::new(0);
    sweep("usages", "places", &names(lines), Duration::MAX, |at| {
        let found = host.indexed_symbol_at(&root, &file, at, None)?;
        most_files.set(most_files.get().max(found.files.len()));
        Some(found.files.iter().map(|anchors| anchors.ranges.len()).sum())
    });
    println!("  in {} files at most", most_files.get());
    Ok(())
}

/// What the check is asked for besides the package and the file.
struct Asked<'a> {
    project: Option<&'a Path>,
    indexed: bool,
    settings: &'a Settings,
    command: Option<&'a str>,
    rename: Option<(usize, usize, &'a str)>,
    word: Option<&'a str>,
    typing: &'a str,
}

fn check(dir: &Path, limits: Limits, package: &Path, file: &Path, asked: &Asked) -> Result<(), String> {
    let Asked { project, indexed, settings, command, rename, word, typing } = *asked;
    let scan = Arc::new(Scan::default());
    let seen = scan.clone();
    let deadline = limits.edit_soft;
    let sink = move |_: Option<&str>, event: PluginEvent| match event {
        PluginEvent::TaskStarted { .. } => {
            seen.slices.fetch_add(1, Ordering::Relaxed);
        }
        // The plugin tells its tool window to list again what the first scan found.
        PluginEvent::ViewMessage { message, .. } if message.contains("\"refresh\"") => {
            seen.ended.store(true, Ordering::Relaxed);
        }
        PluginEvent::Notification { title, body, .. } => {
            if let Ok(mut notified) = seen.notified.lock() {
                notified.push((title, body));
            }
        }
        // The call that asks waits for its answer: it is given from another thread, as a window gives it.
        PluginEvent::PromptRequested { prompt_id, .. } => {
            let answer = seen.answer.lock().ok().and_then(|mut answer| answer.take());
            let host = seen.host.get().and_then(Weak::upgrade);
            if let Some(host) = host {
                std::thread::spawn(move || {
                    let answer = answer.map(|answer| vec![answer]);
                    for _ in 0..400 {
                        if host.answer_prompt(&prompt_id, answer.clone()) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
            }
        }
        _ => {}
    };
    let host = Arc::new(PluginHost::new(dir.to_path_buf(), Arc::new(sink), limits).map_err(|e| e.to_string())?);
    let _ = scan.host.set(Arc::downgrade(&host));
    let published = Published::default();
    if indexed {
        // An indexer runs in a trusted project only.
        host.set_project_policy(Arc::new(|_| true));
    }
    if let Some(root) = project {
        let opened = Opened(root.to_path_buf(), published.clone(), settings.clone(), OnceLock::new());
        host.set_workspace(Some(Arc::new(opened)));
    }
    let unpacked = host.unpack_package(package).map_err(|e| e.to_string())?;
    let installed = host.install_from_dir(&unpacked, &Consent::all()).map_err(|e| e.to_string())?;
    if let Some(root) = project {
        read_project(&host, &installed.id, &scan)?;
        if let Some(command) = command {
            run_command(&host, &installed.id, root, command, &scan)?;
        }
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let doc = ProviderDocument { path: file.to_path_buf(), language: "lua".into(), version: 1, text: text.clone() };

    // The first call starts the plugin and reads the file; the calls after it find the file read. A first call past
    // the deadline runs on after the host gave up: it is left to end, so that the next times are each provider's own.
    let start = Instant::now();
    let first = host.provide_semantic_tokens(&doc).len();
    let cold = start.elapsed();
    if first == 0 {
        std::thread::sleep(Limits::default().edit_hard);
    }
    let start = Instant::now();
    let tokens = host.provide_semantic_tokens(&doc);
    println!("tokens: {} ({first} on the first call, {cold:?}; then {:?})", tokens.len(), start.elapsed());
    let mut styles: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for token in &tokens {
        let style = std::iter::once(token.kind.as_str()).chain(token.modifiers.iter().map(String::as_str));
        let entry = styles.entry(style.collect::<Vec<_>>().join(" ")).or_default();
        entry.0 += 1;
        if entry.1.is_empty() {
            entry.1 = slice(&lines, token.range.start.line, token.range.start.col, token.range.end.col);
        }
    }
    for (style, (count, example)) in &styles {
        println!("  {style}: {count}, first `{example}`");
    }

    let start = Instant::now();
    let folds = host.language_folding(&doc);
    let listed: Vec<String> =
        folds.iter().map(|f| format!("{}..{} {}", f.start_line + 1, f.end_line + 1, f.kind)).collect();
    println!("folds: {} ({:?}): {}", folds.len(), start.elapsed(), listed.join(", "));

    let start = Instant::now();
    let symbols = host.language_document_symbols(&doc);
    // The IDE nests an entry only under a parent that comes before it and whose range holds its own.
    let held = |index: usize, parent: u32| {
        let (entry, holder) = (&symbols[index], symbols.get(parent as usize));
        holder.is_some_and(|holder| {
            (parent as usize) < index && holder.range.start <= entry.range.start && entry.range.end <= holder.range.end
        })
    };
    let loose =
        symbols.iter().enumerate().filter(|(index, entry)| entry.parent.is_some_and(|p| !held(*index, p))).count();
    println!("structure: {} ({:?}), {loose} outside their parent", symbols.len(), start.elapsed());
    for symbol in &symbols {
        let mut depth = 0;
        let mut parent = symbol.parent;
        while let Some(index) = parent {
            depth += 1;
            parent = symbols.get(index as usize).and_then(|s| s.parent);
        }
        println!("  {}{}{} {}", "  ".repeat(depth), symbol.name, symbol.detail, symbol.kind);
    }

    if let Some(word) = word {
        let at = lines.iter().enumerate().find_map(|(line, text)| {
            text.find(word).map(|byte| Pos::new(line as u32, text[..byte].encode_utf16().count() as u32))
        });
        let at = at.ok_or_else(|| format!("`{word}` is not in the file"))?;
        let start = Instant::now();
        let highlights = host.language_document_highlights(&doc, at);
        let listed: Vec<String> = highlights
            .iter()
            .map(|h| format!("{}:{} {}", h.range.start.line + 1, h.range.start.col + 1, h.kind))
            .collect();
        println!("occurrences of `{word}`: {} ({:?}): {}", highlights.len(), start.elapsed(), listed.join(", "));
        let start = Instant::now();
        let hovers = host.provide_hover(&doc, at);
        println!("hover on `{word}`: {} ({:?})", hovers.len(), start.elapsed());
        for line in hovers.iter().flat_map(|hover| hover.markdown.lines()) {
            println!("    {line}");
        }
        let places = |found: Vec<ide_plugin::Location>| -> Vec<String> {
            let place = |l: &ide_plugin::Location| {
                let file = l.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                format!("{file}:{}:{}", l.range.start.line + 1, l.range.start.col + 1)
            };
            found.iter().map(place).collect()
        };
        println!("definition of `{word}`: {}", places(host.provide_definition(&doc, at)).join(", "));
        println!("type of `{word}`: {}", places(host.language_type_definition(&doc, at)).join(", "));
        if let Some(open) = word.find('(') {
            let inside = Pos::new(at.line, at.col + word[..=open].encode_utf16().count() as u32);
            let start = Instant::now();
            let help = host.language_signature_help(&doc, inside);
            let labels: Vec<&str> = help.iter().flat_map(|help| &help.signatures).map(|s| s.label.as_str()).collect();
            let active = help.as_ref().map_or((0, 0), |help| (help.active_signature, help.active_parameter));
            let took = start.elapsed();
            println!("signatures in `{word}`: {} ({took:?}), active {active:?}: {}", labels.len(), labels.join(" | "));
        }
    }

    sweep("hovers", "bytes", &names(&lines), deadline, |at| {
        let hovers = host.provide_hover(&doc, at);
        (!hovers.is_empty()).then(|| hovers.iter().map(|hover| hover.markdown.len()).sum())
    });
    sweep("definitions", "places", &names(&lines), deadline, |at| {
        Some(host.provide_definition(&doc, at).len()).filter(|found| *found > 0)
    });
    sweep("types", "places", &names(&lines), deadline, |at| {
        Some(host.language_type_definition(&doc, at).len()).filter(|found| *found > 0)
    });
    sweep("signatures", "signatures", &calls(&lines), deadline, |at| {
        host.language_signature_help(&doc, at).map(|help| help.signatures.len())
    });
    sweep("actions", "actions", &names(&lines), deadline, |at| {
        Some(host.provide_code_actions(&doc, Range::point(at)).len()).filter(|found| *found > 0)
    });
    sweep("selections", "ranges", &names(&lines), deadline, |at| {
        Some(host.language_selection_ranges(&doc, at).len()).filter(|found| *found > 0)
    });
    let start = Instant::now();
    let injected = host.provide_injections(&doc);
    let languages: std::collections::BTreeSet<&str> = injected.iter().map(|found| found.language.as_str()).collect();
    let named = languages.into_iter().collect::<Vec<_>>().join(", ");
    let named = if named.is_empty() { named } else { format!(": {named}") };
    println!("injections: {} ({:?}){named}", injected.len(), start.elapsed());
    let screens: Vec<Pos> = (0..lines.len() as u32).step_by(SCREEN as usize).map(|line| Pos::new(line, 0)).collect();
    let screen = |from: Pos| Range::new(from, Pos::new(from.line + SCREEN, 0));
    let first: Vec<String> = host
        .provide_inlay_hints(&doc, screen(Pos::new(0, 0)))
        .iter()
        .take(SHOWN)
        .map(|hint| format!("{}:{} {}", hint.position.line + 1, hint.position.col + 1, hint.label))
        .collect();
    println!("hints of the first screen: {}", first.join(", "));
    sweep("hints", "hints", &screens, deadline, |from| {
        Some(host.provide_inlay_hints(&doc, screen(from)).len()).filter(|found| *found > 0)
    });

    // What the editor asks after an edit, at the delays of OpenIDEA 0.1.40: completion while typing, occurrences
    // 300 ms after the edit, line markers at 400, code actions at 500, semantic tokens at 600. Each call finds the
    // plugin as the one before left it, still busy when that one was late.
    let typed = ProviderDocument { version: 2, text: format!("{text}\n{typing}"), ..doc.clone() };
    let at = Pos::new(lines.len() as u32, typing.encode_utf16().count() as u32);
    let edit = Instant::now();
    let ask = |delay: u64, what: &str, call: &dyn Fn() -> usize| {
        std::thread::sleep(Duration::from_millis(delay).saturating_sub(edit.elapsed()));
        let start = Instant::now();
        let count = call();
        println!("  +{delay} ms {what}: {count} ({:?})", start.elapsed());
        count
    };
    println!("after an edit:");
    let completed = std::cell::RefCell::new(Vec::new());
    ask(100, "completion", &|| {
        let items = host.provide_completion(&typed, at, None);
        *completed.borrow_mut() = items.iter().take(SHOWN).map(|(_, item)| item.label.clone()).collect();
        items.len()
    });
    println!("    after `{typing}`: {}", completed.borrow().join(", "));
    ask(300, "occurrences", &|| host.language_document_highlights(&typed, at).len());
    ask(400, "line markers", &|| host.provide_line_markers(&typed).len());
    if let Some([errors, warnings, infos, hints, faded]) = published.lock().ok().and_then(|counts| *counts) {
        let all = errors + warnings + infos + hints;
        println!(
            "  problems published: {all} ({errors} errors, {warnings} warnings, {infos} infos, {hints} hints, \
             {faded} faded)"
        );
    }
    ask(500, "code actions", &|| host.provide_code_actions(&typed, Range::point(at)).len());
    let coloured = ask(600, "tokens", &|| host.provide_semantic_tokens(&typed).len());

    if let (true, Some(root)) = (indexed, project) {
        index_project(&host, root, file, &lines)?;
    }
    if let (Some(asked), Some(root)) = (rename, project) {
        rename_in_project(&host, root, &doc, &lines, asked, &scan)?;
    }
    let info = host.info_of(&installed.id).map_err(|e| e.to_string())?;
    println!("{} {}: {:?}, {:?}", info.id, info.version, info.status, info.stats);
    if tokens.is_empty() {
        return Err("no semantic token came back".to_owned());
    }
    if coloured == 0 {
        return Err("no semantic token came back after the edit".to_owned());
    }
    if loose > 0 {
        return Err(format!("{loose} entries of the structure are not inside their parent: the IDE lists them flat"));
    }
    Ok(())
}
