use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};

/// An entry of the structure view; `parent` indexes the list, comes before its children and is written
/// around them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub detail: String,
    pub kind: &'static str,
    pub range: (usize, usize),
    pub selection: (usize, usize),
    pub parent: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    GlobalVar,
    LocalVar,
    /// A local that a `---@class` comment names: it is listed under the name of the class.
    Class,
    GlobalFunction,
    LocalFunction,
    Method,
}

struct Element {
    name: String,
    detail: String,
    kind: Kind,
    range: (usize, usize),
    selection: (usize, usize),
    children: Vec<usize>,
    /// The name as the code writes it, for an element declared away from what holds it.
    written: Option<String>,
    /// Named on the way to a member (`a` and `b` of `a.b.c = 1`), never declared here.
    implied: bool,
    /// A variable that another variable or a class holds.
    field: bool,
}

/// The names visible where the walk stands: those of an element, on top of its parents'.
struct Context {
    element: Option<usize>,
    names: Vec<(String, usize)>,
}

struct Outline {
    elements: Vec<Element>,
    contexts: Vec<Context>,
}

fn params(owner: Node) -> String {
    let Some(body) = owner.child(N::FuncBody) else { return "()".to_owned() };
    let mut names: Vec<&str> = body.children_of(N::ParamNameDef).map(Node::code).collect();
    if body.has(T::Ellipsis) {
        names.push("...");
    }
    format!("({})", names.join(", "))
}

/// The name an expression gives when it is assigned to: a name, or the member of an index.
fn name_of<'a>(expr: Node<'a>) -> Option<&'a str> {
    match expr.kind() {
        N::NameExpr | N::IndexExpr => expr.token(T::Id).map(|id| id.text()),
        _ => None,
    }
}

/// The classes the documentation comment above `statement` declares with `---@class`, in order.
fn classes<'a>(statement: Node<'a>) -> Vec<&'a str> {
    let leading = statement.all_tokens().take_while(|token| token.kind().is_trivia());
    let Some(doc) = leading.filter(|token| token.is(T::DocComment)).last() else { return Vec::new() };
    let named = |line: &'a str| {
        let rest = line.trim_start().trim_start_matches('-').trim_start().strip_prefix("@class")?.trim_start();
        let rest = if rest.starts_with('(') { rest.split_once(')')?.1.trim_start() } else { rest };
        let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    };
    doc.text().lines().filter_map(named).collect()
}

fn selection(node: Node) -> (usize, usize) {
    node.token(T::Id).map_or(node.code_range(), |id| id.range())
}

/// The text of `node` without its spaces and comments.
fn written(node: Node) -> String {
    node.all_tokens().filter(|token| !token.kind().is_trivia()).map(|token| token.text()).collect()
}

impl Outline {
    fn create(&mut self, kind: Kind, name: &str, detail: String, node: Node) -> usize {
        self.elements.push(Element {
            name: name.to_owned(),
            detail,
            kind,
            range: node.code_range(),
            selection: selection(node),
            children: Vec::new(),
            written: None,
            implied: false,
            field: false,
        });
        self.elements.len() - 1
    }

    /// A variable reaches to the end of its value: the fields of a table are written inside it.
    fn reach(&mut self, element: usize, value: Option<Node>) {
        if let Some(value) = value {
            let end = value.code_range().1;
            let range = &mut self.elements[element].range;
            range.1 = range.1.max(end);
        }
    }

    /// Adds `element` where the walk stands; a name already listed there is replaced in place.
    fn add(&mut self, element: usize) {
        let name = self.elements[element].name.clone();
        self.add_as(element, name);
    }

    /// Adds `element` under the name the code knows it by.
    fn add_as(&mut self, element: usize, name: String) {
        let Some(context) = self.contexts.last_mut() else { return };
        match context.names.iter_mut().find(|(known, _)| *known == name) {
            Some(slot) => slot.1 = element,
            None => context.names.push((name, element)),
        }
        if let Some(owner) = context.element {
            self.elements[owner].children.push(element);
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.contexts
            .iter()
            .rev()
            .find_map(|context| context.names.iter().find(|(known, _)| known == name).map(|(_, element)| *element))
    }

    fn child_named(&self, owner: usize, name: &str) -> Option<usize> {
        self.elements[owner].children.iter().copied().find(|child| self.elements[*child].name == name)
    }

    fn inside(&mut self, element: usize, body: Option<Node>) {
        self.contexts.push(Context { element: Some(element), names: Vec::new() });
        if let Some(block) = body.and_then(|body| body.child(N::Block)) {
            self.block(block);
        }
        self.contexts.pop();
    }

    /// The element `a.b.c` stands for: the first name is looked up, or becomes a global.
    fn compound(&mut self, expr: Node) -> Option<usize> {
        let mut parts = vec![expr];
        while let Some(inner) = parts.last().and_then(|part| part.expr()).filter(|_| parts.len() < 64) {
            parts.push(inner);
        }
        let mut element: Option<usize> = None;
        for part in parts.into_iter().rev() {
            let name = name_of(part)?;
            let found = match element {
                None => self.find(name),
                Some(owner) => self.child_named(owner, name),
            };
            let next = match (found, element) {
                (Some(found), _) => found,
                (None, None) => {
                    let created = self.create(Kind::GlobalVar, name, String::new(), part);
                    self.elements[created].implied = true;
                    self.add(created);
                    created
                }
                (None, Some(owner)) => {
                    let created = self.create(Kind::LocalVar, name, String::new(), part);
                    self.elements[created].implied = true;
                    self.elements[owner].children.push(created);
                    created
                }
            };
            element = Some(next);
        }
        element
    }

    fn table(&mut self, table: Node, owner: usize) {
        for field in table.children_of(N::TableField) {
            let Some(name) = field.token(T::Id) else { continue };
            let value = field.expr();
            let closure = value.filter(|value| value.is(N::ClosureExpr));
            let class = self.elements[owner].kind == Kind::Class;
            let child = match closure {
                Some(closure) if class => self.create(Kind::Method, name.text(), params(closure), field),
                Some(closure) => self.create(Kind::LocalFunction, name.text(), params(closure), field),
                None => self.create(Kind::LocalVar, name.text(), String::new(), field),
            };
            self.elements[owner].children.push(child);
        }
    }

    fn assign(&mut self, node: Node) {
        let targets: Vec<Node> = node.child(N::VarList).into_iter().flat_map(Node::exprs).collect();
        let values: Vec<Node> = node.child(N::ExprList).into_iter().flat_map(Node::exprs).collect();
        for (i, target) in targets.into_iter().enumerate() {
            let Some(name) = name_of(target) else { return };
            // Assigning to something already listed declares nothing.
            if target.is(N::NameExpr) && self.find(name).is_some() {
                return;
            }
            let owner = match target.is(N::IndexExpr).then(|| target.expr()).flatten() {
                Some(object) => match self.compound(object) {
                    Some(owner) => Some(owner),
                    None => return,
                },
                None => None,
            };
            let held = owner.map(|owner| self.elements[owner].kind);
            let value = values.get(i).copied();
            let closure = value.filter(|value| value.is(N::ClosureExpr));
            let child = match (closure, held) {
                (Some(closure), Some(Kind::Class)) => self.create(Kind::Method, name, params(closure), target),
                (Some(closure), Some(Kind::LocalVar)) => {
                    self.create(Kind::LocalFunction, name, params(closure), target)
                }
                (Some(closure), _) => self.create(Kind::GlobalFunction, name, params(closure), target),
                (None, Some(Kind::Class | Kind::LocalVar)) => self.create(Kind::LocalVar, name, String::new(), target),
                (None, _) => self.create(Kind::GlobalVar, name, String::new(), target),
            };
            self.reach(child, value);
            match owner {
                Some(owner) => {
                    self.elements[child].written = Some(written(target));
                    self.elements[owner].children.push(child);
                }
                None => self.add(child),
            }
            if let Some(closure) = closure {
                self.inside(child, closure.child(N::FuncBody));
            } else if let Some(table) = value.filter(|value| value.is(N::TableExpr)) {
                self.table(table, child);
            }
        }
    }

    fn statement(&mut self, node: Node) {
        match node.kind() {
            N::LocalDef => {
                let values: Vec<Node> = node.child(N::ExprList).into_iter().flat_map(Node::exprs).collect();
                let names = node.child(N::NameList).into_iter().flat_map(|list| list.children_of(N::NameDef));
                let declared = classes(node);
                for (i, name) in names.enumerate() {
                    let element = match declared.get(i) {
                        Some(class) => self.create(Kind::Class, class, String::new(), name),
                        None => self.create(Kind::LocalVar, name.code(), String::new(), name),
                    };
                    self.reach(element, values.get(i).copied());
                    self.add_as(element, name.code().to_owned());
                    if let Some(table) = values.get(i).filter(|value| value.is(N::TableExpr)) {
                        self.table(*table, element);
                    }
                }
            }
            N::LocalFuncDef => {
                let Some(name) = node.token(T::Id) else { return };
                let element = self.create(Kind::LocalFunction, name.text(), params(node), node);
                self.add(element);
                self.inside(element, node.child(N::FuncBody));
            }
            N::FuncDef => {
                if let Some(name) = node.token(T::Id) {
                    let element = self.create(Kind::GlobalFunction, name.text(), params(node), node);
                    self.add(element);
                }
            }
            N::ClassMethodDef => {
                let Some(path) = node.child(N::ClassMethodName) else { return };
                let (Some(name), Some(object)) = (path.token(T::Id), path.expr()) else { return };
                let Some(owner) = self.compound(object) else { return };
                let element = self.create(Kind::Method, name.text(), params(node), node);
                self.elements[element].selection = name.range();
                self.elements[element].written = Some(written(path));
                self.elements[owner].children.push(element);
                self.inside(element, node.child(N::FuncBody));
            }
            N::AssignStat => self.assign(node),
            // The functions passed to a call: handlers, threads, callbacks.
            N::ExprStat => {
                let args = node.expr().filter(|call| call.is(N::CallExpr)).and_then(|call| call.child(N::ListArgs));
                for closure in args.into_iter().flat_map(Node::exprs).filter(|arg| arg.is(N::ClosureExpr)) {
                    let element = self.create(Kind::LocalFunction, "<anonymous>", params(closure), closure);
                    self.add(element);
                    self.inside(element, closure.child(N::FuncBody));
                }
            }
            _ => {}
        }
    }

    fn block(&mut self, block: Node) {
        for statement in block.children() {
            self.statement(statement);
        }
    }

    /// `a` holding only `b` holding only `c` is shown as `a.b.c`.
    fn compress(&mut self, element: usize) {
        loop {
            let variable = |kind: Kind| matches!(kind, Kind::GlobalVar | Kind::LocalVar | Kind::Class);
            let only = match self.elements[element].children.as_slice() {
                [only] if variable(self.elements[element].kind) && variable(self.elements[*only].kind) => *only,
                _ => break,
            };
            let name = format!("{}.{}", self.elements[element].name, self.elements[only].name);
            self.elements[element].name = name;
            self.elements[element].children = std::mem::take(&mut self.elements[only].children);
        }
        for child in self.elements[element].children.clone() {
            self.compress(child);
        }
    }

    /// Marks the variables that another variable or a class holds.
    fn mark_fields(&mut self, element: usize, nested: bool) {
        self.elements[element].field = nested;
        let nested = !matches!(self.elements[element].kind, Kind::GlobalFunction | Kind::LocalFunction | Kind::Method);
        for i in 0..self.elements[element].children.len() {
            self.mark_fields(self.elements[element].children[i], nested);
        }
    }

    /// OpenIDEA nests an entry only inside what is written around it. An element declared away from what
    /// holds it (`function a:b()` after `a`) moves to what is written around it, under the name the code gives
    /// it, and what was only named on the way to a member is not listed. Gives the elements left at the top.
    fn rehome(&mut self, top: Vec<usize>) -> Vec<usize> {
        let mut found: Vec<(usize, Option<usize>)> = Vec::new();
        let mut pending: Vec<(usize, Option<usize>)> = top.into_iter().rev().map(|element| (element, None)).collect();
        while let Some((element, holder)) = pending.pop() {
            found.push((element, holder));
            pending.extend(self.elements[element].children.iter().rev().map(|child| (*child, Some(element))));
        }
        let inside = |outer: (usize, usize), inner: (usize, usize)| outer.0 <= inner.0 && inner.1 <= outer.1;
        found.retain(|(element, _)| !self.elements[*element].implied);
        found.sort_by_key(|(element, _)| {
            let (start, end) = self.elements[*element].range;
            (start, std::cmp::Reverse(end))
        });
        for (element, _) in &found {
            self.elements[*element].children.clear();
        }
        let mut top = Vec::new();
        let mut around: Vec<usize> = Vec::new();
        for (element, holder) in found {
            let range = self.elements[element].range;
            while around.last().is_some_and(|outer| !inside(self.elements[*outer].range, range)) {
                around.pop();
            }
            let stays = holder.filter(|h| !self.elements[*h].implied && inside(self.elements[*h].range, range));
            let renamed = self.elements[element].written.take().filter(|_| stays.is_none());
            if let Some(name) = renamed {
                self.elements[element].name = name;
            }
            match stays.or(around.last().copied()) {
                Some(holder) => self.elements[holder].children.push(element),
                None => top.push(element),
            }
            around.push(element);
        }
        top
    }

    fn list(&self, element: usize, parent: Option<u32>, out: &mut Vec<Symbol>) {
        let e = &self.elements[element];
        let kind = match e.kind {
            Kind::GlobalFunction | Kind::LocalFunction => "function",
            Kind::Method => "method",
            Kind::Class => "class",
            Kind::LocalVar if e.field => "field",
            Kind::GlobalVar | Kind::LocalVar => "variable",
        };
        let index = out.len() as u32;
        out.push(Symbol {
            name: e.name.clone(),
            detail: e.detail.clone(),
            kind,
            range: e.range,
            selection: e.selection,
            parent,
        });
        for child in &e.children {
            self.list(*child, Some(index), out);
        }
    }

    /// What the file declares, as the JetBrains plugin holds it, and the elements at its top.
    fn of(src: &Source) -> (Outline, Vec<usize>) {
        let mut outline =
            Outline { elements: Vec::new(), contexts: vec![Context { element: None, names: Vec::new() }] };
        outline.block(src.root());
        let top: Vec<usize> = outline.contexts[0].names.iter().map(|(_, element)| *element).collect();
        for element in &top {
            outline.mark_fields(*element, false);
        }
        (outline, top)
    }

    fn listed(mut self, top: Vec<usize>) -> Vec<Symbol> {
        let mut out = Vec::new();
        for element in top {
            self.compress(element);
            self.list(element, None, &mut out);
        }
        out
    }
}

/// The structure view of the JetBrains plugin: what the file declares, functions holding what
/// their body declares, tables their named fields.
#[cfg(test)]
pub fn as_intellij(src: &Source) -> Vec<Symbol> {
    let (outline, top) = Outline::of(src);
    outline.listed(top)
}

/// The structure view: the entries of the JetBrains plugin, each under what is written around it.
pub fn symbols(src: &Source) -> Vec<Symbol> {
    let (mut outline, top) = Outline::of(src);
    let top = outline.rehome(top);
    outline.listed(top)
}
