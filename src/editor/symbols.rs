use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};

/// An entry of the structure view; `parent` indexes the list and comes before its children.
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

impl Outline {
    fn create(&mut self, kind: Kind, name: &str, detail: String, node: Node) -> usize {
        self.elements.push(Element {
            name: name.to_owned(),
            detail,
            kind,
            range: node.code_range(),
            selection: selection(node),
            children: Vec::new(),
        });
        self.elements.len() - 1
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
                    self.add(created);
                    created
                }
                (None, Some(owner)) => {
                    let created = self.create(Kind::LocalVar, name, String::new(), part);
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
            match owner {
                Some(owner) => self.elements[owner].children.push(child),
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

    fn list(&self, element: usize, parent: Option<u32>, nested: bool, out: &mut Vec<Symbol>) {
        let e = &self.elements[element];
        let kind = match e.kind {
            Kind::GlobalFunction | Kind::LocalFunction => "function",
            Kind::Method => "method",
            Kind::Class => "class",
            Kind::LocalVar if nested => "field",
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
        let nested = !matches!(e.kind, Kind::GlobalFunction | Kind::LocalFunction | Kind::Method);
        for child in &e.children {
            self.list(*child, Some(index), nested, out);
        }
    }
}

/// The structure view of the JetBrains plugin: what the file declares, functions holding what
/// their body declares, tables their named fields.
pub fn symbols(src: &Source) -> Vec<Symbol> {
    let mut outline = Outline { elements: Vec::new(), contexts: vec![Context { element: None, names: Vec::new() }] };
    outline.block(src.root());
    let top: Vec<usize> = outline.contexts[0].names.iter().map(|(_, element)| *element).collect();
    let mut out = Vec::new();
    for element in top {
        outline.compress(element);
        outline.list(element, None, false, &mut out);
    }
    out
}
