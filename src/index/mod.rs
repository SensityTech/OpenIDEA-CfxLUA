//! What the files of a project declare, by name: classes, members, aliases (`stubs/index/`,
//! `LuaShortNamesManager`). A member is kept under each class the declaring file alone can tell it belongs to.
//! The fields a table constructor writes are not kept: most of what a project declares, they are read in
//! their file when the members of that table are asked for.

pub mod declare;
pub mod project;

pub use declare::declare;

use crate::lua::doc::tags::Visibility;
use crate::ty::{FileId, Ty, global_type_name};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    /// `---@field name T` of a class.
    DocField,
    /// `function a.b:name()`.
    Method,
    /// `function name()`.
    Function,
    /// `a.name = v`.
    Field,
    /// `name = v`, a global.
    Global,
}

impl MemberKind {
    /// Which declaration of a name stands for the others: an annotation, then a function.
    pub fn worth(self) -> u32 {
        match self {
            MemberKind::DocField => 1000,
            MemberKind::Method | MemberKind::Function => 900,
            MemberKind::Field | MemberKind::Global => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Member {
    /// The classes it belongs to. The members a file declares on the same classes share the list.
    pub owners: Rc<[Rc<str>]>,
    pub name: Rc<str>,
    pub kind: MemberKind,
    /// The node that declares it: of the documentation tree for a `DocField`, of the Lua tree otherwise.
    pub node: u32,
    /// Its type, when the statement that declares it tells it: an annotation, a function, a literal, a
    /// table. Without it the type is inferred in the declaring file, when asked for.
    pub ty: Option<Ty>,
    pub is_static: bool,
    pub deprecated: bool,
    pub visibility: Visibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumKind {
    No,
    /// `---@enum Name`: the values of the table are the values of the type.
    Values,
    /// `---@enum (key) Name`: its keys are.
    Keys,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub name: Rc<str>,
    pub super_name: Option<Rc<str>>,
    /// The parents after the first one of `---@class A: B, C`.
    pub extra_supers: Vec<Rc<str>>,
    /// The type of the table the class annotates: its fields are members of the class.
    pub alias: Option<Rc<str>>,
    pub deprecated: bool,
    pub enum_kind: EnumKind,
    /// What the table of an enum is given to, as it is written: its members are read through that name.
    pub access: Option<Rc<str>>,
    /// The index signatures `---@field [K] V`: key type, value type.
    pub signatures: Vec<(Ty, Ty)>,
    /// The `DocTagClass` of the documentation tree.
    pub node: u32,
}

#[derive(Clone, Debug)]
pub struct AliasDef {
    pub name: Rc<str>,
    pub ty: Ty,
    /// The `DocTagAlias` of the documentation tree.
    pub node: u32,
}

/// What one file declares.
#[derive(Clone, Debug, Default)]
pub struct Declared {
    /// The name `---@module Name` gives to the file: its globals are members of that class, not of `_G`.
    pub module: Option<Rc<str>>,
    pub members: Vec<Member>,
    pub classes: Vec<ClassDef>,
    pub aliases: Vec<AliasDef>,
}

/// A member and the file that declares it.
#[derive(Clone, Copy, Debug)]
pub struct Found<'a> {
    pub file: FileId,
    pub member: &'a Member,
}

type Places = Vec<(FileId, u32)>;

/// Where each name is declared; sorted, so that what is listed comes out in the same order every time.
type Names = BTreeMap<Rc<str>, Places>;

/// Where a member is declared, under one of the classes it belongs to. Sorted by class, then by name, then
/// by file: the members of a class are listed by name, and those of one name always in the same order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    class: Rc<str>,
    name: Rc<str>,
    file: FileId,
    at: u32,
}

/// A place as it is searched for: its names are not owned.
trait Placed {
    fn key(&self) -> (&str, &str, FileId, u32);
}

impl Placed for Place {
    fn key(&self) -> (&str, &str, FileId, u32) {
        (&self.class, &self.name, self.file, self.at)
    }
}

impl Placed for (&str, &str, FileId, u32) {
    fn key(&self) -> (&str, &str, FileId, u32) {
        *self
    }
}

impl<'a> Borrow<dyn Placed + 'a> for Place {
    fn borrow(&self) -> &(dyn Placed + 'a) {
        self
    }
}

impl PartialEq for dyn Placed + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for dyn Placed + '_ {}

impl PartialOrd for dyn Placed + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for dyn Placed + '_ {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

#[derive(Default)]
pub struct Index {
    files: HashMap<FileId, Declared>,
    members: BTreeSet<Place>,
    classes: Names,
    aliases: Names,
}

fn forget(places: &mut Names, name: &str, file: FileId) {
    let Some(list) = places.get_mut(name) else { return };
    list.retain(|(declaring, _)| *declaring != file);
    if list.is_empty() {
        places.remove(name);
    }
}

impl Index {
    pub fn file(&self, file: FileId) -> Option<&Declared> {
        self.files.get(&file)
    }

    /// Replaces what `file` declares.
    pub fn set(&mut self, file: FileId, declared: Declared) {
        self.remove(file);
        for (at, member) in declared.members.iter().enumerate() {
            for owner in member.owners.iter() {
                let (class, name) = (owner.clone(), member.name.clone());
                self.members.insert(Place { class, name, file, at: at as u32 });
            }
        }
        for (at, class) in declared.classes.iter().enumerate() {
            self.classes.entry(class.name.clone()).or_default().push((file, at as u32));
        }
        for (at, alias) in declared.aliases.iter().enumerate() {
            self.aliases.entry(alias.name.clone()).or_default().push((file, at as u32));
        }
        self.files.insert(file, declared);
    }

    pub fn remove(&mut self, file: FileId) {
        let Some(declared) = self.files.remove(&file) else { return };
        for (at, member) in declared.members.iter().enumerate() {
            for owner in member.owners.iter() {
                let place: &dyn Placed = &(&**owner, &*member.name, file, at as u32);
                self.members.remove(place);
            }
        }
        for class in &declared.classes {
            forget(&mut self.classes, &class.name, file);
        }
        for alias in &declared.aliases {
            forget(&mut self.aliases, &alias.name, file);
        }
    }

    fn found(&self, place: &Place) -> Option<Found<'_>> {
        let member = self.files.get(&place.file)?.members.get(place.at as usize)?;
        Some(Found { file: place.file, member })
    }

    /// The places kept under `class`, from the name `name` on.
    fn from(&self, class: &str, name: &str) -> impl Iterator<Item = &Place> {
        let first: &dyn Placed = &(class, name, 0, 0);
        let places = self.members.range::<dyn Placed, _>((Bound::Included(first), Bound::Unbounded));
        places.take_while(move |place| *place.class == *class)
    }

    pub fn class(&self, name: &str) -> Option<&ClassDef> {
        let (file, at) = self.classes.get(name)?.first()?;
        self.files.get(file)?.classes.get(*at as usize)
    }

    pub fn alias(&self, name: &str) -> Option<&AliasDef> {
        let (file, at) = self.aliases.get(name)?.first()?;
        self.files.get(file)?.aliases.get(*at as usize)
    }

    /// The names of the classes the project declares, sorted.
    pub fn class_names(&self) -> impl Iterator<Item = &str> {
        self.classes.keys().map(|name| &**name)
    }

    /// The names of its aliases, sorted.
    pub fn alias_names(&self) -> impl Iterator<Item = &str> {
        self.aliases.keys().map(|name| &**name)
    }

    /// The members declared under the very name `class`.
    pub fn own(&self, class: &str) -> impl Iterator<Item = Found<'_>> {
        self.from(class, "").filter_map(|place| self.found(place))
    }

    /// The members `name` declared under the very name `class`.
    pub fn own_named(&self, class: &str, name: &str) -> impl Iterator<Item = Found<'_>> {
        let named = self.from(class, name).take_while(move |place| *place.name == *name);
        named.filter_map(|place| self.found(place))
    }

    /// The other names the members of a declared class are kept under: the table it annotates and, when a
    /// global of that name is taken for the class, what is assigned to the fields of that global.
    pub fn other_names(&self, class: &str, global_name_as_type: bool) -> Vec<Rc<str>> {
        let Some(alias) = self.class(class).and_then(|def| def.alias.clone()).filter(|alias| **alias != *class) else {
            return Vec::new();
        };
        let mut names = vec![alias];
        if global_name_as_type {
            names.push(global_type_name(class).into());
        }
        names
    }

    /// The class `name` extends, when it is one whose members can be read: a declared class, or `string`.
    /// `declared` is the parent the type itself states, as a bounded generic parameter does.
    pub fn parent(&self, name: &str, declared: Option<&str>) -> Option<Rc<str>> {
        let parent = match declared {
            Some(parent) => Some(Rc::from(parent)),
            None => self.class(name).and_then(|def| def.super_name.clone()),
        };
        parent.filter(|parent| **parent != *name && self.is_class(parent))
    }

    fn is_class(&self, name: &str) -> bool {
        match Ty::builtin(name) {
            Some(builtin) => builtin.is_class(),
            None => self.classes.contains_key(name),
        }
    }

    /// The classes `class` inherits from, the nearest first: its parents one after the other, then the extra
    /// parents of each class met and their own parents (`TyClass.processSuperClass`). `base` is the parent
    /// the type itself states, as a bounded generic parameter does.
    pub fn supers(&self, class: &str, base: Option<&str>) -> Vec<Rc<str>> {
        let mut out: Vec<Rc<str>> = Vec::new();
        let mut current = self.parent(class, base);
        while let Some(parent) = current.filter(|parent| !out.contains(parent)) {
            current = self.parent(&parent, None);
            out.push(parent);
        }
        let mut seen = out.clone();
        let mut visited: Vec<Rc<str>> = std::iter::once(Rc::from(class)).chain(out.iter().cloned()).collect();
        let mut next = 0;
        while let Some(owner) = visited.get(next).cloned() {
            next += 1;
            let extras = self.class(&owner).map(|def| def.extra_supers.clone()).unwrap_or_default();
            for extra in extras {
                if *extra == *class || seen.contains(&extra) {
                    continue;
                }
                seen.push(extra.clone());
                if !self.is_class(&extra) {
                    continue;
                }
                out.push(extra.clone());
                visited.push(extra.clone());
                let mut current = self.parent(&extra, None);
                while let Some(parent) = current.filter(|parent| **parent != *class && !seen.contains(parent)) {
                    current = self.parent(&parent, None);
                    seen.push(parent.clone());
                    out.push(parent.clone());
                    visited.push(parent);
                }
            }
        }
        out
    }

    /// Every name the members of a class are kept under: its own and, for a declared class, its other
    /// names, then the classes it inherits from and theirs (`LuaClassMemberIndex.process`).
    pub fn names(&self, class: &str, global_name_as_type: bool) -> Vec<Rc<str>> {
        let mut names: Vec<Rc<str>> = vec![class.into()];
        if self.class(class).is_none() {
            return names;
        }
        names.extend(self.other_names(class, global_name_as_type));
        for parent in self.supers(class, None) {
            let others = self.other_names(&parent, global_name_as_type);
            names.push(parent);
            names.extend(others);
        }
        names
    }

    /// The members `name` the index keeps for a class, under each of its names.
    pub fn named(&self, class: &str, name: &str, global_name_as_type: bool) -> Vec<Found<'_>> {
        let names = self.names(class, global_name_as_type);
        names.iter().flat_map(|kept_under| self.own_named(kept_under, name)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str, parents: &[&str], alias: Option<&str>) -> ClassDef {
        let mut parents = parents.iter().map(|parent| Rc::from(*parent));
        ClassDef {
            name: name.into(),
            super_name: parents.next(),
            extra_supers: parents.collect(),
            alias: alias.map(Rc::from),
            deprecated: false,
            enum_kind: EnumKind::No,
            access: None,
            signatures: Vec::new(),
            node: 0,
        }
    }

    fn member(owner: &str, name: &str, kind: MemberKind) -> Member {
        Member {
            owners: [Rc::from(owner)].into(),
            name: name.into(),
            kind,
            node: 0,
            ty: None,
            is_static: false,
            deprecated: false,
            visibility: Visibility::Public,
        }
    }

    fn names(found: Vec<Found>) -> Vec<String> {
        found.iter().map(|found| format!("{}.{}", found.member.owners[0], found.member.name)).collect()
    }

    #[test]
    fn parents_come_nearest_first_then_the_extra_ones_and_theirs() {
        let mut index = Index::default();
        let classes = vec![
            class("A", &["B", "X", "Y"], None),
            class("B", &["C"], None),
            class("C", &["table"], None),
            class("X", &["XBase"], None),
            class("XBase", &["C"], None),
            class("Y", &["Undeclared"], None),
            class("Loop", &["Loop2"], None),
            class("Loop2", &["Loop"], None),
            class("Text", &["string"], None),
        ];
        index.set(1, Declared { classes, ..Declared::default() });
        let supers = |class: &str| index.supers(class, None).iter().map(|name| name.to_string()).collect::<Vec<_>>();
        assert_eq!(supers("A"), ["B", "C", "X", "XBase", "Y"], "`table` and an undeclared class are no parents");
        assert_eq!(supers("C"), Vec::<String>::new());
        assert_eq!(supers("Loop"), ["Loop2", "Loop"], "a cycle is walked once");
        assert_eq!(supers("Text"), ["string"]);
        assert_eq!(supers("Nowhere"), Vec::<String>::new());
        assert_eq!(index.supers("T", Some("B")), [Rc::from("B"), Rc::from("C")], "a bounded generic parameter");
        assert_eq!(index.parent("A", None).as_deref(), Some("B"));
        assert_eq!(index.parent("C", None), None);
    }

    #[test]
    fn the_members_of_a_class_are_also_those_of_its_table_and_of_its_parents() {
        let mut index = Index::default();
        let declared = Declared {
            classes: vec![class("Shop", &["Base"], Some("1@(9)table")), class("Base", &[], Some("1@(40)table"))],
            members: vec![
                member("Shop", "open", MemberKind::Method),
                member("1@(9)table", "open", MemberKind::Field),
                member("$Shop", "open", MemberKind::Field),
                member("Base", "open", MemberKind::DocField),
                member("1@(40)table", "open", MemberKind::Field),
                member("Elsewhere", "open", MemberKind::Field),
                member("Shop", "close", MemberKind::Method),
            ],
            ..Declared::default()
        };
        index.set(1, declared);
        assert_eq!(
            names(index.named("Shop", "open", true)),
            ["Shop.open", "1@(9)table.open", "$Shop.open", "Base.open", "1@(40)table.open"]
        );
        assert_eq!(names(index.named("Shop", "open", false)).len(), 4, "the global of that name is not the class");
        assert_eq!(names(index.named("Elsewhere", "open", true)), ["Elsewhere.open"], "no class is declared");
        assert_eq!(names(index.own("Shop").collect()), ["Shop.close", "Shop.open"], "by name");
        assert_eq!(index.other_names("Base", true), [Rc::from("1@(40)table"), Rc::from("$Base")]);
        assert!(index.other_names("Elsewhere", true).is_empty());
    }

    #[test]
    fn a_file_replaces_what_it_declared() {
        let mut index = Index::default();
        let first = Declared {
            classes: vec![class("Shop", &[], None)],
            members: vec![member("Shop", "open", MemberKind::Method), member("_G", "Shop", MemberKind::Global)],
            ..Declared::default()
        };
        index.set(1, first);
        index.set(2, Declared { members: vec![member("Shop", "open", MemberKind::Field)], ..Declared::default() });
        assert_eq!(index.own_named("Shop", "open").map(|found| found.file).collect::<Vec<_>>(), [1, 2]);
        index.set(1, Declared { members: vec![member("Shop", "close", MemberKind::Method)], ..Declared::default() });
        assert!(index.class("Shop").is_none());
        assert_eq!(names(index.own("Shop").collect()), ["Shop.close", "Shop.open"]);
        assert_eq!(index.own_named("Shop", "open").map(|found| found.file).collect::<Vec<_>>(), [2]);
        assert!(index.own("_G").next().is_none());

        // A file that declares again keeps its place among the others: by file, not by who came last.
        index.set(1, Declared { members: vec![member("Shop", "open", MemberKind::Method)], ..Declared::default() });
        assert_eq!(index.own_named("Shop", "open").map(|found| found.file).collect::<Vec<_>>(), [1, 2]);
        let twice = vec![member("Shop", "open", MemberKind::Field), member("Shop", "open", MemberKind::Method)];
        index.set(2, Declared { members: twice, ..Declared::default() });
        let kinds: Vec<MemberKind> = index.own_named("Shop", "open").map(|found| found.member.kind).collect();
        assert_eq!(kinds, [MemberKind::Method, MemberKind::Field, MemberKind::Method], "in the order of the file");
        assert!(index.own_named("Shop", "ope").next().is_none() && index.own_named("Sho", "open").next().is_none());
        assert!(index.own("Sho").next().is_none() && index.own("Shop2").next().is_none());

        index.remove(1);
        index.remove(2);
        assert!(index.own("Shop").next().is_none() && index.file(1).is_none());
        assert!(index.members.is_empty());
    }
}
