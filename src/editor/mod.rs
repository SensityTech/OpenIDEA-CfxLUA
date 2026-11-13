pub mod completion;
pub mod definition;
pub mod folding;
pub mod formatting;
pub mod highlights;
pub mod hints;
pub mod hover;
pub mod injection;
pub mod markers;
pub mod selection;
pub mod semantic;
pub mod signature;
pub mod symbols;
pub mod target;

#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod markers_tests;
#[cfg(test)]
mod tests;

use crate::feature::{Ctx, Feature, Intention, Problem};
use crate::format;
use crate::index::library;
use crate::index::project::{Budget, Cost, Projects, Sight};
use crate::inspect::{self, Inspection};
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;
use crate::project::FileKind;
use crate::refactor::edges::{is_globals_table, is_local};
use crate::refactor::is_library;
use crate::refactor::members::{self, Member};
use crate::ty::context::Cx;
use crate::ty::{FileId, psi};
use crate::workspace::Workspace;
use openidea_plugin::Location;
use openidea_plugin::providers::{CompletionItem, LineMarker};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// What the inspections that ask the type engine may spend on one file: they are published by an editor
/// call, which the IDE drops after 50 ms.
const INSPECTION_TIME: Duration = Duration::from_millis(20);

/// Global names kept per file and counted over the files of the project: a name is kept once.
#[derive(Default)]
struct NamesByFile {
    by_file: HashMap<String, Vec<Rc<str>>>,
    files: HashMap<Rc<str>, u32>,
}

impl NamesByFile {
    /// `names` is sorted, each name once.
    fn set(&mut self, path: &str, names: &[impl AsRef<str>]) {
        self.remove(path);
        let mut kept = Vec::with_capacity(names.len());
        for name in names.iter().map(AsRef::as_ref) {
            let (name, count) = match self.files.remove_entry(name) {
                Some((name, count)) => (name, count + 1),
                None => (Rc::from(name), 1),
            };
            self.files.insert(name.clone(), count);
            kept.push(name);
        }
        if !kept.is_empty() {
            self.by_file.insert(path.to_owned(), kept);
        }
    }

    fn remove(&mut self, path: &str) {
        for name in self.by_file.remove(path).unwrap_or_default() {
            if let Some(count) = self.files.get_mut(&name) {
                *count -= 1;
                if *count == 0 {
                    self.files.remove(&name);
                }
            }
        }
    }

    fn holds(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    /// Whether a file other than `path` holds `name`.
    fn elsewhere(&self, path: &str, name: &str) -> bool {
        let own = self.by_file.get(path).is_some_and(|names| names.binary_search_by(|kept| (**kept).cmp(name)).is_ok());
        self.files.get(name).is_some_and(|count| *count > u32::from(own))
    }
}

/// What the editor shows of a Lua file beyond its text: colours, folds, outline, occurrences, and what the
/// type engine tells of it.
#[derive(Default)]
pub struct EditorFeature {
    /// The names `function name()` statements give to globals.
    functions: RefCell<NamesByFile>,
    /// The global names each file reads, assigns or defines.
    used: RefCell<NamesByFile>,
    projects: Projects,
}

impl EditorFeature {
    fn applies(cx: &Ctx) -> bool {
        cx.settings.enabled() && (cx.doc.is_lua() || matches!(cx.doc.kind, FileKind::Lua | FileKind::Manifest))
    }

    /// Asks the engine about the document, with what its file sees of the libraries: the parts of the Cfx
    /// runtime of its side, unless lua-language-server was given them, and the natives of its game.
    fn ask<R>(&self, cx: &Ctx, src: &Source, ask: impl FnOnce(&Cx, FileId) -> R) -> Option<R> {
        self.ask_within(None, cx, src, ask)
    }

    /// `ask`, within `time` when one is given.
    fn ask_within<R>(
        &self,
        time: Option<Duration>,
        cx: &Ctx,
        src: &Source,
        ask: impl FnOnce(&Cx, FileId) -> R,
    ) -> Option<R> {
        Self::seen(cx, src, |asked, sight| match time {
            Some(time) => self.projects.ask_within(time, asked, sight, ask),
            None => self.projects.ask(asked, sight, ask),
        })
    }

    /// `ask` with a budget of its own, and what the question cost; nothing when its answer tells less than
    /// there is.
    fn ask_whole<R>(
        &self,
        budget: Budget,
        cx: &Ctx,
        src: &Source,
        ask: impl FnOnce(&Cx, FileId) -> R,
    ) -> Option<(R, Cost)> {
        let counted = Self::seen(cx, src, |asked, sight| self.projects.ask_counted(budget, asked, sight, ask));
        counted.filter(|(_, cost)| !cost.short)
    }

    /// What a question about the document is asked with: the document, and what its file sees of the libraries.
    fn seen<R>(cx: &Ctx, src: &Source, ask: impl FnOnce((&Workspace, &str, &Source), &Sight) -> R) -> R {
        let mut parts = crate::runtime::visible_parts(cx.settings, cx.doc.side);
        parts.retain(|part| !cx.ws.luals.parts.contains(part));
        let games = cx.ws.resource_of(&cx.doc.path).and_then(|resource| resource.games.clone());
        let sets = crate::natives::visible_sets(cx.settings, &cx.ws.detected_games(), cx.doc.side, games.as_deref());
        let native = |name: &str| cx.natives.find(name, &sets).map(library::native_type);
        ask((cx.ws, cx.doc.path.as_str(), src), &Sight { parts: &parts, native: &native })
    }

    fn completed_place(cx: &Ctx, at: usize) -> Option<completion::Place> {
        let script = Self::applies(cx) && cx.doc.kind == FileKind::Lua;
        script.then(|| completion::place(&cx.doc.src, at)).flatten()
    }
}

/// The global functions a file defines, then every global name it uses, `_G.name` included: sorted, each
/// name once.
fn global_names(src: &Source) -> (Vec<&str>, Vec<Cow<'_, str>>) {
    let scopes = src.scopes();
    let (mut functions, mut used) = (Vec::new(), Vec::new());
    for node in src.root().descendants() {
        match node.kind() {
            N::FuncDef | N::NameExpr if !is_local(src, node) => {
                let Some(name) = node.token(T::Id) else { continue };
                used.push(Cow::Borrowed(name.text()));
                // A name that was assigned before its function statement stays a variable.
                if node.is(N::FuncDef) && scopes.id_of(node).is_none() {
                    functions.push(name.text());
                }
            }
            N::IndexExpr if psi::prefix(node).is_some_and(|table| is_globals_table(src, table)) => {
                used.extend(psi::member_name(src, node));
            }
            _ => {}
        }
    }
    functions.sort_unstable();
    functions.dedup();
    used.sort_unstable();
    used.dedup();
    (functions, used)
}

impl Feature for EditorFeature {
    fn index_file(&self, ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        if kind == FileKind::Lua {
            let (functions, used) = global_names(src);
            self.functions.borrow_mut().set(path, &functions);
            self.used.borrow_mut().set(path, &used);
            self.projects.declare(ws, path, src);
        } else {
            self.forget_file(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.functions.borrow_mut().remove(path);
        self.used.borrow_mut().remove(path);
        self.projects.forget(path);
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        // Nothing is analysed unless the tokens around the caret tell a place the engine completes.
        let Some(place) = Self::completed_place(cx, at) else { return Vec::new() };
        let typed = completion::Typed::new(&cx.doc.src, at, place);
        let src = typed.src(&cx.doc.src);
        let asked = completion::Asked { ws: cx.ws, doc: cx.doc, src, at };
        let listed = self.ask(cx, src, |engine, file| completion::complete(engine, file, &asked, place));
        listed.unwrap_or_default()
    }

    /// A member is read from what the document declares, typed a moment ago or not. A name is not worth
    /// indexing the document at every word: what it declares in sight is read in its tree.
    fn completes_from_index(&self, cx: &Ctx, at: usize) -> bool {
        matches!(Self::completed_place(cx, at), Some(completion::Place::Member(_)))
    }

    fn definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let found = self.ask(cx, src, |engine, file| definition::definition(engine, file, src, at));
        found.unwrap_or_default()
    }

    fn type_definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let found = self.ask(cx, src, |engine, file| definition::type_definition(engine, file, src, at));
        found.unwrap_or_default()
    }

    fn inlay_hints(&self, cx: &Ctx, from: usize, to: usize) -> Vec<hints::Hint> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let wanted = hints::Wanted {
            arguments: cx.settings.bool(hints::PARAMETER_NAMES, true),
            locals: cx.settings.bool(hints::LOCAL_TYPES, false),
            parameters: cx.settings.bool(hints::PARAMETER_TYPES, false),
            returns: cx.settings.bool(hints::RETURN_TYPES, false),
        };
        if !wanted.any() {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let found = self.ask(cx, src, |engine, file| hints::hints(engine, file, src, (from, to), wanted));
        found.unwrap_or_default()
    }

    fn hover(&self, cx: &Ctx, at: usize) -> Option<hover::Hover> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return None;
        }
        let src = &cx.doc.src;
        self.ask(cx, src, |engine, file| hover::hover(engine, file, src, at)).flatten()
    }

    fn signature_help(&self, cx: &Ctx, at: usize) -> Option<signature::Help> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return None;
        }
        let src = &cx.doc.src;
        self.ask(cx, src, |engine, file| signature::help(engine, file, src, at)).flatten()
    }

    fn line_markers(&self, cx: &Ctx) -> Vec<LineMarker> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let wanted = markers::Wanted {
            overrides: cx.settings.bool(markers::OVERRIDES, true),
            classes: cx.settings.bool(markers::CLASSES, true),
            recursive_calls: cx.settings.bool(markers::RECURSIVE_CALLS, true),
            tail_calls: cx.settings.bool(markers::TAIL_CALLS, true),
            project_read: !cx.ws.indexing,
        };
        let src = &cx.doc.src;
        let found =
            self.ask_within(Some(INSPECTION_TIME), cx, src, |engine, file| markers::markers(engine, file, src, wanted));
        let marker = |found: markers::Marker| LineMarker {
            line: cx.doc.position(found.at).line,
            icon: Some(found.icon.to_owned()),
            tooltip: found.tooltip,
            target: found.target,
        };
        found.unwrap_or_default().into_iter().map(marker).collect()
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let wanted = |inspection: Inspection| cx.settings.bool(inspection.setting, inspection.on_by_default());
        let mut found = inspect::problems(src, &wanted);
        // A project that is still being read declares less than it will: a type would be unknown for a while.
        let asked = inspect::INDEXED.into_iter().chain(inspect::TYPED).any(wanted);
        if cx.doc.kind == FileKind::Lua && !cx.ws.indexing && asked {
            let root = format!("{}/", cx.ws.root);
            let shown = |path: &str| path.strip_prefix(&root).unwrap_or(path).to_owned();
            let indexed = self.ask_within(Some(INSPECTION_TIME), cx, src, |engine, file| {
                let mut found = inspect::indexed::problems(engine, file, src, &wanted, &shown);
                found.extend(inspect::typed::problems(engine, file, src, &wanted));
                found
            });
            found.extend(indexed.unwrap_or_default());
            found.sort_by_key(|problem| problem.start);
        }
        // What another file uses is known once the project is read.
        if cx.doc.kind == FileKind::Lua && !cx.ws.indexing && wanted(inspect::GLOBAL_LOCAL) {
            let used = self.used.borrow();
            let elsewhere = |name: &str| is_library(cx.natives, name) || used.elsewhere(&cx.doc.path, name);
            found.extend(inspect::global_local::problems(src, &elsewhere));
            found.sort_by_key(|problem| problem.start);
        }
        found
    }

    fn intentions(&self, cx: &Ctx, start: usize, end: usize) -> Vec<Intention> {
        if !Self::applies(cx) || cx.doc.kind != FileKind::Lua {
            return Vec::new();
        }
        let src = &cx.doc.src;
        let mut found = crate::intent::rewrites(src, start);
        let annotations = self.ask_within(Some(INSPECTION_TIME), cx, src, |engine, file| {
            crate::intent::annotations(engine, file, src, start)
        });
        found.extend(annotations.unwrap_or_default());
        found.extend(crate::intent::surrounds(src, start, end));
        let rename = crate::refactor::rename::fix(src, start, |name| format!("Rename '{name}'…"));
        let library = |name: &str| crate::refactor::is_library(cx.natives, name);
        found.extend(rename.or_else(|| {
            crate::refactor::rename::shared_fix(src, start, &library, |name| format!("Rename '{name}' in the project…"))
        }));
        found.into_iter().map(|fix| Intention { fix, kind: "refactor" }).collect()
    }

    fn member_at(&self, cx: &Ctx, at: usize, budget: Budget) -> Option<Member> {
        let src = &cx.doc.src;
        let lua = Self::applies(cx) && cx.doc.kind == FileKind::Lua;
        let told = lua.then(|| self.ask_whole(budget, cx, src, |engine, file| members::at(engine, file, src, at)));
        told.flatten().and_then(|(member, _)| member)
    }

    fn member_uses(&self, cx: &Ctx, member: &Member, budget: Budget) -> Option<(Vec<(usize, usize)>, Cost)> {
        let src = &cx.doc.src;
        self.ask_whole(budget, cx, src, |engine, file| members::uses(engine, file, src, member))
    }

    fn semantic_tokens(&self, cx: &Ctx) -> Vec<semantic::Token> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let functions = self.functions.borrow();
        semantic::tokens(cx, &|name| functions.holds(name))
    }

    fn folding(&self, cx: &Ctx) -> Vec<folding::Fold> {
        if Self::applies(cx) { folding::folds(&cx.doc.src) } else { Vec::new() }
    }

    fn symbols(&self, cx: &Ctx) -> Vec<symbols::Symbol> {
        if Self::applies(cx) { symbols::symbols(&cx.doc.src) } else { Vec::new() }
    }

    fn selection_ranges(&self, cx: &Ctx, at: usize) -> Vec<(usize, usize)> {
        if Self::applies(cx) { selection::ranges(&cx.doc.src, at) } else { Vec::new() }
    }

    fn injections(&self, cx: &Ctx) -> Vec<injection::Injected> {
        if Self::applies(cx) { injection::injections(&cx.doc.src) } else { Vec::new() }
    }

    fn format(
        &self,
        cx: &Ctx,
        tab_size: u32,
        spaces: bool,
        range: Option<(usize, usize)>,
    ) -> Result<Vec<format::Edit>, format::Unsettled> {
        if !Self::applies(cx) {
            return Ok(Vec::new());
        }
        format::format(&cx.doc.src, &formatting::style(cx.settings, tab_size, spaces), range)
    }

    fn highlights(&self, cx: &Ctx, at: usize) -> Vec<highlights::Highlight> {
        if Self::applies(cx) { highlights::highlights(&cx.doc.src, at) } else { Vec::new() }
    }
}
