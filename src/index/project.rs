//! The index of each open project, and the files the engine reads through it.

use super::{Index, declare};
use crate::lua::Source;
use crate::project::FileKind;
use crate::text::parent;
use crate::ty::FileId;
use crate::ty::context::{Cx, Files, Options};
use crate::workspace::Workspace;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// How much text one question may make the engine read anew: what is over is left unread, and the answer
/// says less instead of coming late.
const READ_BUDGET: usize = 400_000;
/// How many files, and how much of their text, stay parsed between two questions.
const KEPT_FILES: usize = 48;
const KEPT_BYTES: usize = 2_000_000;

/// The number each file of a project is known by. A number is never given to another file.
#[derive(Default)]
struct FileIds {
    ids: HashMap<String, FileId>,
    paths: Vec<String>,
}

impl FileIds {
    fn id(&mut self, path: &str) -> FileId {
        if let Some(id) = self.ids.get(path) {
            return *id;
        }
        let id = self.paths.len() as FileId;
        self.paths.push(path.to_owned());
        self.ids.insert(path.to_owned(), id);
        id
    }
}

/// The files read lately, parsed, the latest first.
#[derive(Default)]
struct Kept {
    files: Vec<(FileId, Arc<str>, Rc<Source>)>,
}

impl Kept {
    /// The parsed file, when it was read from this very text.
    fn get(&mut self, file: FileId, text: &Arc<str>) -> Option<Rc<Source>> {
        let at = self.files.iter().position(|(kept, _, _)| *kept == file)?;
        let entry = self.files.remove(at);
        if !Arc::ptr_eq(&entry.1, text) {
            return None;
        }
        let src = entry.2.clone();
        self.files.insert(0, entry);
        Some(src)
    }

    fn keep(&mut self, file: FileId, text: Arc<str>, src: Rc<Source>) {
        self.files.retain(|(kept, _, _)| *kept != file);
        self.files.insert(0, (file, text, src));
        let (mut bytes, mut kept) = (0, 0);
        for (_, text, _) in self.files.iter().take(KEPT_FILES) {
            bytes += text.len();
            if bytes > KEPT_BYTES && kept > 0 {
                break;
            }
            kept += 1;
        }
        self.files.truncate(kept);
    }
}

#[derive(Default)]
struct Project {
    ids: FileIds,
    index: Index,
    kept: RefCell<Kept>,
}

/// The files of one project, read for one question.
struct Reading<'a> {
    ws: &'a Workspace,
    project: &'a Project,
    /// The file the question is asked from.
    from: &'a str,
    left: Cell<usize>,
}

impl Reading<'_> {
    fn lua(&self, path: &str) -> Option<FileId> {
        let file = self.ws.file(path).filter(|file| file.kind == FileKind::Lua)?;
        self.project.ids.ids.get(&file.path).copied()
    }
}

impl Files for Reading<'_> {
    fn source(&self, file: FileId) -> Option<Rc<Source>> {
        let path = self.project.ids.paths.get(file as usize)?;
        let known = self.ws.file(path)?;
        let text = known.text.clone()?;
        if let Some(kept) = self.project.kept.borrow_mut().get(file, &text) {
            return Some(kept);
        }
        if text.len() > self.left.get() {
            return None;
        }
        self.left.set(self.left.get() - text.len());
        let src = Rc::new(crate::parse(known.kind, &text));
        self.project.kept.borrow_mut().keep(file, text, src.clone());
        Some(src)
    }

    /// `require('a.b')` loads `a/b.lua` or `a/b/init.lua`: of the resource of the asking file, then of the
    /// project, then the first file of that name anywhere.
    fn required(&self, path: &str) -> Option<FileId> {
        let name = path.replace('.', "/");
        let resource = self.ws.resource_of(self.from).map(|resource| resource.dir.clone());
        let roots = [resource, Some(parent(self.from).to_owned()), Some(self.ws.root.clone())];
        for root in roots.into_iter().flatten() {
            for file in [format!("{root}/{name}.lua"), format!("{root}/{name}/init.lua")] {
                if let Some(found) = self.lua(&file) {
                    return Some(found);
                }
            }
        }
        let (plain, init) = (format!("/{name}.lua"), format!("/{name}/init.lua"));
        let named = self.ws.files().find(|file| file.path.ends_with(&plain) || file.path.ends_with(&init))?;
        self.lua(&named.path)
    }
}

/// What the files of the open projects declare, kept up to date as they are indexed.
#[derive(Default)]
pub struct Projects {
    by_root: RefCell<HashMap<String, Project>>,
    options: Options,
}

impl Projects {
    /// Replaces what the Lua file `path` declares by what `src`, its text, declares.
    pub fn declare(&self, ws: &Workspace, path: &str, src: &Source) {
        let mut projects = self.by_root.borrow_mut();
        let project = projects.entry(ws.root.clone()).or_default();
        let file = project.ids.id(path);
        let declared = {
            let reading = Reading { ws, project, from: path, left: Cell::new(0) };
            declare(&Cx::new(&reading, &project.index, &self.options).at_home(file, src), file, src)
        };
        project.index.set(file, declared);
    }

    pub fn forget(&self, path: &str) {
        for project in self.by_root.borrow_mut().values_mut() {
            if let Some(file) = project.ids.ids.get(path).copied() {
                project.index.remove(file);
            }
        }
    }

    /// Asks the engine about the file `path` of `ws`, whose text is `src`. `None` when the project knows
    /// nothing of that file yet.
    pub fn ask<R>(&self, ws: &Workspace, path: &str, src: &Source, ask: impl FnOnce(&Cx, FileId) -> R) -> Option<R> {
        let projects = self.by_root.borrow();
        let project = projects.get(&ws.root)?;
        let file = project.ids.ids.get(path).copied()?;
        let reading = Reading { ws, project, from: path, left: Cell::new(READ_BUDGET) };
        Some(ask(&Cx::new(&reading, &project.index, &self.options).at_home(file, src), file))
    }
}
