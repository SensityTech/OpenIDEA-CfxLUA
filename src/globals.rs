//! Which files define a global function: what a feature asks before it reads a function of another file.

use std::collections::HashMap;
use std::rc::Rc;

/// The files that define each name, sorted: one file, most often. A file is the one copy of its path that the
/// index it belongs to keeps.
#[derive(Default)]
pub struct Definers {
    by_name: HashMap<String, Vec<Rc<str>>>,
}

impl Definers {
    pub fn add(&mut self, name: &str, file: &Rc<str>) {
        let Some(files) = self.by_name.get_mut(name) else {
            self.by_name.insert(name.to_owned(), vec![file.clone()]);
            return;
        };
        if let Err(at) = files.binary_search(file) {
            files.insert(at, file.clone());
        }
    }

    pub fn remove(&mut self, name: &str, file: &str) {
        let Some(files) = self.by_name.get_mut(name) else { return };
        files.retain(|other| **other != *file);
        if files.is_empty() {
            self.by_name.remove(name);
        }
    }

    /// The first file that defines `name`, `except` left out.
    pub fn other(&self, name: &str, except: &str) -> Option<&Rc<str>> {
        self.by_name.get(name)?.iter().find(|file| ***file != *except)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_other_file_that_defines_a_name() {
        let (a, b, c): (Rc<str>, Rc<str>, Rc<str>) = ("/a.lua".into(), "/b.lua".into(), "/c.lua".into());
        let mut definers = Definers::default();
        definers.add("Draw", &c);
        definers.add("Draw", &a);
        definers.add("Draw", &b);
        definers.add("Draw", &a);
        definers.add("Tick", &b);
        let other =
            |definers: &Definers, name: &str, except: &str| definers.other(name, except).map(|file| file.to_string());
        assert_eq!(other(&definers, "Draw", "/x.lua").as_deref(), Some("/a.lua"), "sorted, whatever the order");
        assert_eq!(other(&definers, "Draw", "/a.lua").as_deref(), Some("/b.lua"));
        assert_eq!(other(&definers, "Tick", "/b.lua"), None, "its only file is left out");
        assert_eq!(other(&definers, "Nothing", "/a.lua"), None);
        assert_eq!(Rc::strong_count(&a), 2, "a file is kept once per name");

        definers.remove("Draw", "/a.lua");
        definers.remove("Draw", "/a.lua");
        definers.remove("Nothing", "/a.lua");
        assert_eq!(other(&definers, "Draw", "/x.lua").as_deref(), Some("/b.lua"));
        definers.remove("Draw", "/b.lua");
        definers.remove("Draw", "/c.lua");
        definers.remove("Tick", "/b.lua");
        assert!(definers.by_name.is_empty());
    }
}
