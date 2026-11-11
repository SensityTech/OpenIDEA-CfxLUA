use super::{FileId, FuncRef, Ty};
use crate::index::Index;
use crate::lua::Source;
use crate::lua::tree::Node;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

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

/// What an answer is kept for: the type of a node, or what the body of a function returns.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Asked {
    Type(Elem),
    Returned(FuncRef),
}

struct Guard {
    elem: Elem,
    global: Option<Rc<str>>,
}

/// One question asked to the engine and what was inferred to answer it (`SearchContext`).
pub struct Cx<'a> {
    pub files: &'a dyn Files,
    pub index: &'a Index,
    pub options: &'a Options,
    value: Cell<i32>,
    stub: Cell<bool>,
    depth: Cell<u32>,
    cuts: Cell<u32>,
    guards: RefCell<Vec<Guard>>,
    busy: RefCell<Vec<Elem>>,
    cache: RefCell<HashMap<(Asked, i32, bool), Ty>>,
    sources: RefCell<HashMap<FileId, Option<Rc<Source>>>>,
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
            cuts: Cell::new(0),
            guards: RefCell::default(),
            busy: RefCell::default(),
            cache: RefCell::default(),
            sources: RefCell::default(),
        }
    }

    pub fn source(&self, file: FileId) -> Option<Rc<Source>> {
        if let Some(known) = self.sources.borrow().get(&file) {
            return known.clone();
        }
        let source = self.files.source(file);
        self.sources.borrow_mut().insert(file, source.clone());
        source
    }

    /// Runs `read` on the node `node` of the Lua tree of `file`; `None` when the file has no such node.
    pub fn at<R>(&self, file: FileId, node: u32, read: impl FnOnce(Psi) -> R) -> Option<R> {
        let src = self.source(file)?;
        let root = src.root();
        ((node as usize) < root.tree().len())
            .then(|| read(Psi { file, src: &src, node: root.tree().node(root.source(), node) }))
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

    /// An answer that a cycle or the depth limit cut short is not kept: asked from elsewhere, it may be
    /// complete.
    fn kept(&self, asked: Asked, infer: impl FnOnce() -> Ty) -> Ty {
        let key = (asked, self.value.get(), self.stub.get());
        if let Some(known) = self.cache.borrow().get(&key) {
            return known.clone();
        }
        if self.depth.get() >= MAX_DEPTH {
            self.cut();
            return Ty::Unknown;
        }
        let cuts = self.cuts.get();
        self.depth.set(self.depth.get() + 1);
        let ty = infer();
        self.depth.set(self.depth.get() - 1);
        if self.cuts.get() == cuts {
            self.cache.borrow_mut().insert(key, ty.clone());
        }
        ty
    }

    fn cut(&self) {
        self.cuts.set(self.cuts.get() + 1);
    }

    /// Runs `action` unless it is already running for `elem`, or for a global of the same name.
    pub fn guarded(&self, elem: Elem, global: Option<&str>, action: impl FnOnce() -> Ty) -> Ty {
        let same = |guard: &Guard| guard.elem == elem || global.is_some() && guard.global.as_deref() == global;
        if self.guards.borrow().iter().any(same) {
            self.cut();
            return Ty::Unknown;
        }
        self.guards.borrow_mut().push(Guard { elem, global: global.map(Rc::from) });
        let ty = action();
        self.guards.borrow_mut().pop();
        ty
    }

    /// Runs `action` unless it is already running for `elem`.
    pub fn once(&self, elem: Elem, action: impl FnOnce() -> Ty) -> Option<Ty> {
        if self.busy.borrow().contains(&elem) {
            self.cut();
            return None;
        }
        self.busy.borrow_mut().push(elem);
        let ty = action();
        self.busy.borrow_mut().pop();
        Some(ty)
    }
}
