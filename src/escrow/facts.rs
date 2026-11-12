use super::check::syntax_error;
use super::rules::{self, Load};
use crate::lua::Source;
use crate::workspace::{Stamp, Workspace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const LEXED_PER_CALL: usize = 256 << 10;

pub struct Facts {
    stamp: Stamp,
    /// The file is already encrypted: nothing else is read of it.
    pub protected: bool,
    pub syntax: Option<(usize, String)>,
    pub loads: Vec<Load>,
}

impl Facts {
    fn new(src: &Source) -> Self {
        let stamp = Stamp::of(&src.text);
        if rules::is_protected(&src.text) {
            return Self { stamp, protected: true, syntax: None, loads: Vec::new() };
        }
        let syntax = syntax_error(src).map(|(at, error)| (src.lines.line_of(at) + 1, error));
        Self { stamp, protected: false, syntax, loads: rules::loads(src) }
    }
}

// Valid while the workspace was last given the very text they were derived from.
thread_local! {
    static FACTS: RefCell<HashMap<String, Rc<Facts>>> = RefCell::new(HashMap::new());
}

pub fn record(path: &str, src: &Source) {
    FACTS.with_borrow_mut(|m| m.insert(path.to_owned(), Rc::new(Facts::new(src))));
}

pub fn forget(path: &str) {
    FACTS.with_borrow_mut(|m| m.remove(path));
}

/// Forgets every file: a test starts from a project that nothing indexed.
#[cfg(test)]
pub fn reset() {
    FACTS.with_borrow_mut(|m| m.clear());
}

pub fn indexed(ws: &Workspace, path: &str) -> Option<Rc<Facts>> {
    let stamp = ws.stamp(path)?;
    FACTS.with_borrow(|m| m.get(path).filter(|f| f.stamp == stamp).cloned())
}

/// The indexed facts, else lexes the file while `budget` (bytes) lasts.
pub fn of(ws: &Workspace, path: &str, budget: &mut usize) -> Option<Rc<Facts>> {
    if let Some(facts) = indexed(ws, path) {
        return Some(facts);
    }
    if *budget == 0 {
        return None;
    }
    let text = ws.read(path)?;
    *budget = budget.saturating_sub(text.len().max(1));
    let facts = Rc::new(Facts::new(&crate::parse(crate::project::FileKind::Lua, &text)));
    FACTS.with_borrow_mut(|m| m.insert(path.to_owned(), facts.clone()));
    Some(facts)
}
