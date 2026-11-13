//! The index of each open project, and the files the engine reads through it.

use super::library::{self, Library};
use super::{Index, declare};
use crate::lua::Source;
use crate::project::FileKind;
use crate::runtime::stubs::Part;
use crate::ty::context::{Cx, Files, Options};
use crate::ty::{FileId, Ty};
use crate::workspace::{Stamp, Workspace};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// How much text one question may make the engine read anew: what is over is left unread, and the answer
/// says less instead of coming late.
const READ_BUDGET: usize = 400_000;
/// How many inferences one question may make, and how long it may take: past either, what is asked is unknown.
/// A question of this project takes hundreds of inferences at most; the IDE stops a plugin whose call runs
/// 500 ms.
const STEP_BUDGET: u32 = 50_000;
const TIME_BUDGET: Duration = Duration::from_millis(200);
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
    files: Vec<(FileId, Stamp, Rc<Source>)>,
}

impl Kept {
    /// The parsed file, when it was read from the text of that stamp.
    fn get(&mut self, file: FileId, stamp: Stamp) -> Option<Rc<Source>> {
        let at = self.files.iter().position(|(kept, _, _)| *kept == file)?;
        let entry = self.files.remove(at);
        if entry.1 != stamp {
            return None;
        }
        let src = entry.2.clone();
        self.files.insert(0, entry);
        Some(src)
    }

    fn keep(&mut self, file: FileId, stamp: Stamp, src: Rc<Source>) {
        self.files.retain(|(kept, _, _)| *kept != file);
        self.files.insert(0, (file, stamp, src));
        let (mut bytes, mut kept) = (0, 0);
        for (_, _, src) in self.files.iter().take(KEPT_FILES) {
            bytes += src.text.len();
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
    /// The files of the libraries, which the index holds as it holds those of the project.
    libraries: HashMap<FileId, (Library, Stamp)>,
}

/// No file: what a library declares is read in its own text.
struct Alone;

impl Files for Alone {
    fn source(&self, _: FileId) -> Option<Rc<Source>> {
        None
    }
}

impl Project {
    /// A project that knows the libraries, and no file yet.
    fn new(options: &Options) -> Project {
        let mut project = Project::default();
        for library in library::files() {
            let file = project.ids.id(&library.path());
            let src = Source::new(library.text);
            let declared = declare(&Cx::new(&Alone, &project.index, options).at_home(file, &src), file, &src);
            project.index.set(file, declared);
            project.libraries.insert(file, (library, Stamp::of(library.text)));
        }
        project
    }

    /// The part of the Cfx runtime a file declares, when it is one of its library.
    fn part_of(&self, file: FileId) -> Option<Part> {
        self.libraries.get(&file).and_then(|(library, _)| library.part)
    }
}

/// What a question sees beyond the files of its project: the parts of the Cfx runtime its file may use, and
/// the natives of its game and of its side.
pub struct Sight<'a> {
    pub parts: &'a [Part],
    pub native: &'a dyn Fn(&str) -> Option<Ty>,
}

impl Sight<'_> {
    /// What is seen while a file is declared: nothing but that file.
    const NONE: Sight<'static> = Sight { parts: &[], native: &|_| None };
}

/// The files of one project, read for one question.
struct Reading<'a> {
    ws: &'a Workspace,
    project: &'a Project,
    /// The file the question is asked from.
    from: &'a str,
    left: Cell<usize>,
    sight: &'a Sight<'a>,
}

impl Reading<'_> {
    fn lua(&self, path: &str) -> Option<FileId> {
        let file = self.ws.file(path).filter(|file| file.kind == FileKind::Lua)?;
        self.project.ids.ids.get(&file.path).copied()
    }
}

impl Files for Reading<'_> {
    fn source(&self, file: FileId) -> Option<Rc<Source>> {
        if let Some((library, stamp)) = self.project.libraries.get(&file) {
            if let Some(kept) = self.project.kept.borrow_mut().get(file, *stamp) {
                return Some(kept);
            }
            let src = Rc::new(Source::new(library.text));
            self.project.kept.borrow_mut().keep(file, *stamp, src.clone());
            return Some(src);
        }
        let path = self.project.ids.paths.get(file as usize)?;
        let known = self.ws.file(path)?;
        let stamp = known.stamp?;
        if let Some(kept) = self.project.kept.borrow_mut().get(file, stamp) {
            return Some(kept);
        }
        if stamp.size() > self.left.get() {
            return None;
        }
        self.left.set(self.left.get() - stamp.size());
        let text = self.ws.read(path)?;
        let src = Rc::new(crate::parse(known.kind, &text));
        self.project.kept.borrow_mut().keep(file, stamp, src.clone());
        Some(src)
    }

    /// A library has no file the IDE could open.
    fn path(&self, file: FileId) -> Option<String> {
        self.project.ids.paths.get(file as usize).filter(|_| !self.is_library(file)).cloned()
    }

    fn visible(&self, file: FileId) -> bool {
        self.project.part_of(file).is_none_or(|part| self.sight.parts.contains(&part))
    }

    fn is_library(&self, file: FileId) -> bool {
        self.project.libraries.contains_key(&file)
    }

    fn native(&self, name: &str) -> Option<Ty> {
        (self.sight.native)(name)
    }

    /// `require('a.b')` loads `a/b.lua` or `a/b/init.lua`: of the resource of the asking file, then of the
    /// project, then the first file of that name anywhere.
    fn required(&self, path: &str) -> Option<FileId> {
        crate::refactor::files::required(self.ws, self.from, path).and_then(|found| self.lua(&found))
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
        let project = projects.entry(ws.root.clone()).or_insert_with(|| Project::new(&self.options));
        let file = project.ids.id(path);
        let declared = {
            let reading = Reading { ws, project, from: path, left: Cell::new(0), sight: &Sight::NONE };
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

    /// Asks the engine about the file `path` of `ws`, whose text is `src`, with what that file sees of the
    /// libraries. `None` when the project knows nothing of that file yet.
    pub fn ask<R>(
        &self,
        asked: (&Workspace, &str, &Source),
        sight: &Sight,
        ask: impl FnOnce(&Cx, FileId) -> R,
    ) -> Option<R> {
        self.ask_within(TIME_BUDGET, asked, sight, ask)
    }

    /// `ask`, for a question that is given `time` to infer what it needs: past it, what is left reads as unknown.
    pub fn ask_within<R>(
        &self,
        time: Duration,
        (ws, path, src): (&Workspace, &str, &Source),
        sight: &Sight,
        ask: impl FnOnce(&Cx, FileId) -> R,
    ) -> Option<R> {
        let projects = self.by_root.borrow();
        let project = projects.get(&ws.root)?;
        let file = project.ids.ids.get(path).copied()?;
        let reading = Reading { ws, project, from: path, left: Cell::new(READ_BUDGET), sight };
        let cx = Cx::new(&reading, &project.index, &self.options).at_home(file, src);
        Some(ask(&cx.bounded(STEP_BUDGET, time), file))
    }
}
