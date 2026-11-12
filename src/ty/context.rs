use super::{FileId, FuncRef, Ty};
use crate::index::Index;
use crate::lua::Source;
use crate::lua::tree::Node;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A node of a file the engine reads: of its Lua tree, or of its documentation tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Elem {
    pub file: FileId,
    pub node: u32,
    pub doc: bool,
}

/// A node of the Lua tree of a file, with what it takes to read around it.
#[derive(Clone, Copy)]
pub struct Psi<'s> {
    pub file: FileId,
    pub src: &'s Source,
    pub node: Node<'s>,
}

impl<'s> Psi<'s> {
    pub fn elem(self) -> Elem {
        Elem { file: self.file, node: self.node.id(), doc: false }
    }

    pub fn at(self, node: Node<'s>) -> Psi<'s> {
        Psi { node, ..self }
    }

    /// Where the node starts in its file, its comment left out.
    pub fn start(self) -> usize {
        self.node.code_range().0
    }
}

/// The files of the project, as the engine reads them.
pub trait Files {
    fn source(&self, file: FileId) -> Option<Rc<Source>>;

    /// The file `require(path)` loads.
    fn required(&self, _path: &str) -> Option<FileId> {
        None
    }

    /// Where the file is, as the IDE names it.
    fn path(&self, _file: FileId) -> Option<String> {
        None
    }

    /// Whether what `file` declares may be used where the question is asked: a library of the other side of
    /// the network may not.
    fn visible(&self, _file: FileId) -> bool {
        true
    }

    /// Whether `file` is one of a library, not of the project.
    fn is_library(&self, _file: FileId) -> bool {
        false
    }

    /// The function the native `name` is, where the question is asked.
    fn native(&self, _name: &str) -> Option<Ty> {
        None
    }
}

/// The settings of the JetBrains plugin the engine depends on, with their defaults.
#[derive(Clone, Debug)]
pub struct Options {
    /// The functions whose call gives what the file they name returns.
    pub require_like: Vec<String>,
    /// The functions whose call gives the class they are a field of: `Class.new()`.
    pub constructors: Vec<String>,
    /// Whether a global named `Foo` has the members of the class `Foo`.
    pub global_name_as_type: bool,
    /// Whether a parameter typed with a bounded generic has the members of its bound.
    pub bounded_generics: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            require_like: vec!["require".to_owned()],
            constructors: vec!["new".to_owned(), "get".to_owned()],
            global_name_as_type: true,
            bounded_generics: false,
        }
    }
}

const MAX_DEPTH: u32 = 96;
/// How many inferences are made between two looks at the clock.
const TIMED_EVERY: u32 = 16;

/// The rank of a guarded question among those opened so far: the questions that are open at one time have
/// growing ranks, and the youngest is closed first.
type Serial = u32;

/// What cuts an answer when no open question does: such an answer is never kept.
const UNBOUND: Serial = Serial::MAX;

/// What an answer is kept for: the type of a node, or what the body of a function returns.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Asked {
    Type(Elem),
    Returned(FuncRef),
}

struct Guard {
    elem: Elem,
    global: Option<Rc<str>>,
    serial: Serial,
}

type Key = (Asked, i32, bool);

/// An answer that questions still open cut short, and those questions, the oldest first: it stands as long
/// as that one is open, so that each answer of a circle is inferred once while the circle is walked. Once it
/// is closed the answer may be more complete, and is inferred again.
struct Provisional {
    ty: Ty,
    cut_by: Rc<[Serial]>,
}

/// One question asked to the engine and what was inferred to answer it (`SearchContext`).
pub struct Cx<'a> {
    pub files: &'a dyn Files,
    pub index: &'a Index,
    pub options: &'a Options,
    value: Cell<i32>,
    stub: Cell<bool>,
    depth: Cell<u32>,
    serials: Cell<Serial>,
    /// How many answers were inferred so far, how many may be, and until when.
    steps: Cell<u32>,
    budget: u32,
    until: Option<Instant>,
    /// The open questions that cut the answer being inferred, each one once.
    cut_by: RefCell<Vec<Serial>>,
    guards: RefCell<Vec<Guard>>,
    busy: RefCell<Vec<(Elem, Serial)>>,
    cache: RefCell<HashMap<Key, Ty>>,
    provisional: RefCell<HashMap<Key, Provisional>>,
    sources: RefCell<HashMap<FileId, Option<Rc<Source>>>>,
    home: Option<(FileId, &'a Source)>,
}

impl<'a> Cx<'a> {
    pub fn new(files: &'a dyn Files, index: &'a Index, options: &'a Options) -> Cx<'a> {
        Cx {
            files,
            index,
            options,
            value: Cell::new(-1),
            stub: Cell::new(false),
            depth: Cell::new(0),
            serials: Cell::new(0),
            steps: Cell::new(0),
            budget: u32::MAX,
            until: None,
            cut_by: RefCell::default(),
            guards: RefCell::default(),
            busy: RefCell::default(),
            cache: RefCell::default(),
            provisional: RefCell::default(),
            sources: RefCell::default(),
            home: None,
        }
    }

    /// The same question, asked about the file `file` whose text is `src`: the document being edited or the
    /// file being declared, read as it is given and not through `files`.
    pub fn at_home(mut self, file: FileId, src: &'a Source) -> Cx<'a> {
        self.home = Some((file, src));
        self
    }

    /// The same question, answered with `steps` inferences at most and within `time`: past either, what is
    /// asked is unknown, and the answer says less instead of coming late.
    pub fn bounded(mut self, steps: u32, time: Duration) -> Cx<'a> {
        self.budget = steps;
        self.until = Some(Instant::now() + time);
        self
    }

    /// How many answers were inferred so far.
    pub fn steps(&self) -> u32 {
        self.steps.get()
    }

    fn source(&self, file: FileId) -> Option<Rc<Source>> {
        if let Some(known) = self.sources.borrow().get(&file) {
            return known.clone();
        }
        let source = self.files.source(file);
        self.sources.borrow_mut().insert(file, source.clone());
        source
    }

    /// Runs `read` on the text of `file`; `None` when the file cannot be read.
    pub fn read<R>(&self, file: FileId, read: impl FnOnce(&Source) -> R) -> Option<R> {
        match self.home {
            Some((home, src)) if home == file => Some(read(src)),
            _ => self.source(file).map(|src| read(&src)),
        }
    }

    /// Runs `read` on the node `node` of the Lua tree of `file`; `None` when the file has no such node.
    pub fn at<R>(&self, file: FileId, node: u32, read: impl FnOnce(Psi) -> R) -> Option<R> {
        let found = self.read(file, |src| {
            let root = src.root();
            let known = (node as usize) < root.tree().len();
            known.then(|| read(Psi { file, src, node: root.tree().node(root.source(), node) }))
        });
        found.flatten()
    }

    /// Which value of a multiple result is asked for: its rank, or a negative number for all of them.
    pub fn value(&self) -> i32 {
        self.value.get()
    }

    pub fn wants_tuple(&self) -> bool {
        self.value.get() < 0
    }

    pub fn with_value<R>(&self, value: i32, action: impl FnOnce() -> R) -> R {
        let saved = self.value.replace(value);
        let result = action();
        self.value.set(saved);
        result
    }

    /// Whether the index is left out: what a file declares is read from the file alone.
    pub fn alone(&self) -> bool {
        self.stub.get()
    }

    pub fn in_file_alone<R>(&self, action: impl FnOnce() -> R) -> R {
        let saved = self.stub.replace(true);
        let result = action();
        self.stub.set(saved);
        result
    }

    /// What `infer` gives for a node, kept for the next time it is asked with the same value rank.
    pub fn cached(&self, elem: Elem, infer: impl FnOnce() -> Ty) -> Ty {
        self.kept(Asked::Type(elem), infer)
    }

    /// What `infer` gives for the body of a function, kept for the next time it is asked.
    pub fn returned(&self, func: FuncRef, infer: impl FnOnce() -> Ty) -> Ty {
        self.kept(Asked::Returned(func), infer)
    }

    /// An answer that a cycle or the depth limit cut short is not kept for good: asked from elsewhere, it may
    /// be complete. It is kept while the oldest question that cut it is open, so that a cycle is walked once
    /// and not once per path that leads into it.
    fn kept(&self, asked: Asked, infer: impl FnOnce() -> Ty) -> Ty {
        let key = (asked, self.value.get(), self.stub.get());
        if let Some(known) = self.cache.borrow().get(&key) {
            return known.clone();
        }
        let standing = self.provisional.borrow().get(&key).map(|kept| (kept.ty.clone(), kept.cut_by.clone()));
        if let Some((ty, cut_by)) = standing {
            if cut_by.first().is_some_and(|oldest| self.is_open(*oldest)) {
                cut_by.iter().for_each(|serial| self.cut(*serial));
                return ty;
            }
            self.provisional.borrow_mut().remove(&key);
        }
        if self.depth.get() >= MAX_DEPTH {
            self.cut(self.youngest().unwrap_or(UNBOUND));
            return Ty::Unknown;
        }
        if self.steps.get() >= self.budget {
            self.cut(UNBOUND);
            return Ty::Unknown;
        }
        self.steps.set(self.steps.get() + 1);
        if self.steps.get().is_multiple_of(TIMED_EVERY) && self.until.is_some_and(|until| Instant::now() >= until) {
            self.steps.set(self.budget);
        }
        let first = self.serials.get();
        let outer = std::mem::take(&mut *self.cut_by.borrow_mut());
        self.depth.set(self.depth.get() + 1);
        let ty = infer();
        self.depth.set(self.depth.get() - 1);
        let mut cut_by = std::mem::replace(&mut *self.cut_by.borrow_mut(), outer);
        // The questions opened to infer this answer are closed: it holds what they found.
        cut_by.retain(|serial| *serial < first || *serial == UNBOUND);
        if cut_by.is_empty() {
            self.cache.borrow_mut().insert(key, ty.clone());
            return ty;
        }
        cut_by.sort_unstable();
        cut_by.iter().for_each(|serial| self.cut(*serial));
        if cut_by.last() != Some(&UNBOUND) {
            self.provisional.borrow_mut().insert(key, Provisional { ty: ty.clone(), cut_by: cut_by.into() });
        }
        ty
    }

    fn open(&self) -> Serial {
        let serial = self.serials.get();
        self.serials.set(serial + 1);
        serial
    }

    fn is_open(&self, serial: Serial) -> bool {
        self.guards.borrow().iter().any(|guard| guard.serial == serial)
            || self.busy.borrow().iter().any(|(_, open)| *open == serial)
    }

    /// The question that was opened last among those that are open.
    fn youngest(&self) -> Option<Serial> {
        let guarded = self.guards.borrow().last().map(|guard| guard.serial);
        guarded.max(self.busy.borrow().last().map(|(_, serial)| *serial))
    }

    /// The answer being inferred is cut short by the open question `by`.
    fn cut(&self, by: Serial) {
        let mut cut_by = self.cut_by.borrow_mut();
        if !cut_by.contains(&by) {
            cut_by.push(by);
        }
    }

    /// Runs `action` unless it is already running for `elem`, or for a global of the same name.
    pub fn guarded(&self, elem: Elem, global: Option<&str>, action: impl FnOnce() -> Ty) -> Ty {
        let same = |guard: &&Guard| guard.elem == elem || global.is_some() && guard.global.as_deref() == global;
        let running = self.guards.borrow().iter().rev().find(same).map(|guard| guard.serial);
        if let Some(running) = running {
            self.cut(running);
            return Ty::Unknown;
        }
        let serial = self.open();
        self.guards.borrow_mut().push(Guard { elem, global: global.map(Rc::from), serial });
        let ty = action();
        self.guards.borrow_mut().pop();
        ty
    }

    /// Runs `action` unless it is already running for `elem`.
    pub fn once(&self, elem: Elem, action: impl FnOnce() -> Ty) -> Option<Ty> {
        let running = self.busy.borrow().iter().rev().find(|(busy, _)| *busy == elem).map(|(_, serial)| *serial);
        if let Some(running) = running {
            self.cut(running);
            return None;
        }
        let serial = self.open();
        self.busy.borrow_mut().push((elem, serial));
        let ty = action();
        self.busy.borrow_mut().pop();
        Some(ty)
    }
}
