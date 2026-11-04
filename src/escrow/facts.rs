use super::check::syntax_error;
use super::rules::{self, Load};
use crate::lua::Source;
use crate::workspace::Workspace;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub const LEXED_PER_CALL: usize = 256 << 10;

pub struct Facts {
    text: Arc<str>,
    pub syntax: Option<(usize, String)>,
    pub loads: Vec<Load>,
}

impl Facts {
    fn new(text: Arc<str>, src: &Source) -> Self {
        let syntax = syntax_error(src).map(|(at, error)| (src.lines.line_of(at) + 1, error));
        Self { text, syntax, loads: rules::loads(src) }
    }
}

// Valid while the workspace still holds the very text they were derived from.
thread_local! {
    static FACTS: RefCell<HashMap<String, Rc<Facts>>> = RefCell::new(HashMap::new());
}

pub fn record(ws: &Workspace, path: &str, src: &Source) {
    let text = ws.file(path).and_then(|f| f.text.clone()).filter(|t| **t == *src.text);
    match text.filter(|t| !rules::is_protected(t)) {
        Some(text) => FACTS.with_borrow_mut(|m| m.insert(path.to_owned(), Rc::new(Facts::new(text, src)))),
        None => FACTS.with_borrow_mut(|m| m.remove(path)),
    };
}

pub fn forget(path: &str) {
    FACTS.with_borrow_mut(|m| m.remove(path));
}

pub fn indexed(ws: &Workspace, path: &str) -> Option<Rc<Facts>> {
    let text = ws.file(path)?.text.as_ref()?;
    FACTS.with_borrow(|m| m.get(path).filter(|f| Arc::ptr_eq(&f.text, text)).cloned())
}

/// The indexed facts, else lexes the file while `budget` (bytes) lasts.
pub fn of(ws: &Workspace, path: &str, budget: &mut usize) -> Option<Rc<Facts>> {
    if let Some(facts) = indexed(ws, path) {
        return Some(facts);
    }
    let text = ws.file(path)?.text.clone()?;
    if *budget == 0 {
        return None;
    }
    *budget = budget.saturating_sub(text.len().max(1));
    let facts = Rc::new(Facts::new(text.clone(), &Source::new(&text)));
    FACTS.with_borrow_mut(|m| m.insert(path.to_owned(), facts.clone()));
    Some(facts)
}
