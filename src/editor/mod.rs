pub mod completion;
pub mod definition;
pub mod folding;
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
use crate::index::library;
use crate::index::project::{Projects, Sight};
use crate::inspect::{self, Inspection};
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;
use crate::project::FileKind;
use crate::ty::FileId;
use crate::ty::context::Cx;
use crate::workspace::Workspace;
use openidea_plugin::Location;
use openidea_plugin::providers::{CompletionItem, LineMarker};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

/// What the inspections that ask the type engine may spend on one file: they are published by an editor
/// call, which the IDE drops after 50 ms.
const INSPECTION_TIME: Duration = Duration::from_millis(20);

/// The names `function name()` statements give to globals, per file and counted over the project.
#[derive(Default)]
struct GlobalFunctions {
    by_file: HashMap<String, Vec<Box<str>>>,
    defined: HashMap<Box<str>, u32>,
}

impl GlobalFunctions {
    fn set(&mut self, path: &str, names: Vec<Box<str>>) {
        self.remove(path);
        for name in &names {
            *self.defined.entry(name.clone()).or_default() += 1;
        }
        if !names.is_empty() {
            self.by_file.insert(path.to_owned(), names);
        }
    }

    fn remove(&mut self, path: &str) {
        for name in self.by_file.remove(path).unwrap_or_default() {
            if let Some(count) = self.defined.get_mut(&name) {
                *count -= 1;
                if *count == 0 {
                    self.defined.remove(&name);
                }
            }
        }
    }
}

/// What the editor shows of a Lua file beyond its text: colours, folds, outline, occurrences, and what the
/// type engine tells of it.
#[derive(Default)]
pub struct EditorFeature {
    functions: RefCell<GlobalFunctions>,
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
        let mut parts = crate::runtime::visible_parts(cx.settings, cx.doc.side);
        parts.retain(|part| !cx.ws.luals.parts.contains(part));
        let games = cx.ws.resource_of(&cx.doc.path).and_then(|resource| resource.games.clone());
        let sets = crate::natives::visible_sets(cx.settings, &cx.ws.detected_games(), cx.doc.side, games.as_deref());
        let native = |name: &str| cx.natives.find(name, &sets).map(library::native_type);
        let (asked, sight) = ((cx.ws, cx.doc.path.as_str(), src), Sight { parts: &parts, native: &native });
        match time {
            Some(time) => self.projects.ask_within(time, asked, &sight, ask),
            None => self.projects.ask(asked, &sight, ask),
        }
    }

    fn completed_place(cx: &Ctx, at: usize) -> Option<completion::Place> {
        let script = Self::applies(cx) && cx.doc.kind == FileKind::Lua;
        script.then(|| completion::place(&cx.doc.src, at)).flatten()
    }
}

fn global_functions(src: &Source) -> Vec<Box<str>> {
    let scopes = src.scopes();
    let mut names: Vec<Box<str>> = src
        .root()
        .descendants()
        .filter(|node| node.is(N::FuncDef) && scopes.id_of(*node).is_none())
        .filter_map(|node| node.token(T::Id))
        .map(|name| name.text().into())
        .collect();
    names.sort();
    names.dedup();
    names
}

impl Feature for EditorFeature {
    fn index_file(&self, ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut functions = self.functions.borrow_mut();
        if kind == FileKind::Lua {
            functions.set(path, global_functions(src));
            self.projects.declare(ws, path, src);
        } else {
            functions.remove(path);
            self.projects.forget(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.functions.borrow_mut().remove(path);
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
        found.into_iter().map(|fix| Intention { fix, kind: "refactor" }).collect()
    }

    fn semantic_tokens(&self, cx: &Ctx) -> Vec<semantic::Token> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let functions = self.functions.borrow();
        semantic::tokens(cx, &|name| functions.defined.contains_key(name))
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

    fn highlights(&self, cx: &Ctx, at: usize) -> Vec<highlights::Highlight> {
        if Self::applies(cx) { highlights::highlights(&cx.doc.src, at) } else { Vec::new() }
    }
}
