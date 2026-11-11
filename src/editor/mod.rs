pub mod folding;
pub mod semantic;

#[cfg(test)]
mod tests;

use crate::feature::{Ctx, Feature};
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::N;
use crate::project::FileKind;
use crate::workspace::Workspace;
use std::cell::RefCell;
use std::collections::HashMap;

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

/// What the editor shows of a Lua file beyond its text: colours, folds, outline, occurrences.
#[derive(Default)]
pub struct EditorFeature {
    functions: RefCell<GlobalFunctions>,
}

impl EditorFeature {
    fn applies(cx: &Ctx) -> bool {
        cx.settings.enabled() && (cx.doc.is_lua() || matches!(cx.doc.kind, FileKind::Lua | FileKind::Manifest))
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
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut functions = self.functions.borrow_mut();
        if kind == FileKind::Lua {
            functions.set(path, global_functions(src));
        } else {
            functions.remove(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.functions.borrow_mut().remove(path);
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
}
