use super::token::T;
use super::tree::{N, NONE, Node};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Local,
    Param,
    LoopVar,
    Function,
    /// A name that is assigned without a local of that name in sight: the first assignment declares it.
    Global,
}

#[derive(Clone, Copy, Debug)]
pub struct Decl {
    pub kind: DeclKind,
    /// `NAME_DEF`, `PARAM_NAME_DEF`, `LOCAL_FUNC_DEF`, or the `NAME_EXPR` a global is first assigned through.
    pub node: u32,
    /// The token of the name.
    pub name: u32,
    /// The `FUNC_BODY` the declaration lives in, `NONE` outside any function.
    pub func: u32,
}

/// Which declaration each name of a file refers to.
pub struct Scopes {
    decls: Vec<Decl>,
    target: Vec<u32>,
}

#[derive(PartialEq, Eq)]
enum Frame {
    Scope,
    /// The names of a `local` statement are visible after it, not in its values.
    Local,
    /// The variables of a `for` are visible in its body, not in its header.
    Loop,
}

struct Open {
    kind: Frame,
    end: u32,
    declared: usize,
    pending: Vec<u32>,
}

struct Walker<'a> {
    decls: Vec<Decl>,
    target: Vec<u32>,
    visible: HashMap<&'a str, Vec<u32>>,
    declared: Vec<&'a str>,
    open: Vec<Open>,
    funcs: Vec<(u32, u32)>,
}

fn name_token<'a>(node: Node<'a>) -> Option<(u32, &'a str)> {
    node.token(T::Id).map(|token| (token.index() as u32, token.text()))
}

impl<'a> Walker<'a> {
    fn push(&mut self, kind: Frame, end: u32) {
        self.open.push(Open { kind, end, declared: self.declared.len(), pending: Vec::new() });
    }

    fn create(&mut self, kind: DeclKind, node: Node<'a>, name: u32) -> u32 {
        let func = self.funcs.last().map_or(NONE, |(_, func)| *func);
        self.decls.push(Decl { kind, node: node.id(), name, func });
        let id = self.decls.len() as u32 - 1;
        self.target[node.id() as usize] = id;
        id
    }

    fn declare(&mut self, name: &'a str, decl: u32) {
        self.visible.entry(name).or_default().push(decl);
        self.declared.push(name);
    }

    fn activate(&mut self, pending: Vec<u32>, root: Node<'a>) {
        for decl in pending {
            let token = self.decls[decl as usize].name as usize;
            self.declare(root.tree().tokens().text(root.source(), token), decl);
        }
    }

    fn close(&mut self, at: u32, root: Node<'a>) {
        while self.open.last().is_some_and(|frame| frame.end <= at) {
            let Some(frame) = self.open.pop() else { break };
            if frame.kind == Frame::Local {
                self.activate(frame.pending, root);
                continue;
            }
            while self.declared.len() > frame.declared {
                if let Some(stack) = self.declared.pop().and_then(|name| self.visible.get_mut(name)) {
                    stack.pop();
                }
            }
        }
        while self.funcs.last().is_some_and(|(end, _)| *end <= at) {
            self.funcs.pop();
        }
    }

    fn lookup(&self, name: &str) -> Option<u32> {
        self.visible.get(name).and_then(|stack| stack.last()).copied()
    }

    fn visit(&mut self, node: Node<'a>, end: u32, root: Node<'a>) {
        let parent = node.parent().map(Node::kind);
        match node.kind() {
            N::File | N::RepeatStat => self.push(Frame::Scope, end),
            N::ForAStat | N::ForBStat => self.push(Frame::Loop, end),
            N::LocalDef => self.push(Frame::Local, end),
            N::FuncBody => {
                self.push(Frame::Scope, end);
                self.funcs.push((end, node.id()));
            }
            // `repeat local a until a`: the condition reads the locals of the body.
            N::Block if parent == Some(N::RepeatStat) => {}
            N::Block => {
                if matches!(parent, Some(N::ForAStat | N::ForBStat))
                    && let Some(frame) = self.open.last_mut()
                {
                    let pending = std::mem::take(&mut frame.pending);
                    self.activate(pending, root);
                }
                self.push(Frame::Scope, end);
            }
            N::NameDef | N::ParamNameDef => {
                let Some((token, name)) = name_token(node) else { return };
                let kind = match parent {
                    Some(N::FuncBody) => DeclKind::Param,
                    Some(N::ForAStat | N::ForBStat) => DeclKind::LoopVar,
                    _ => DeclKind::Local,
                };
                let decl = self.create(kind, node, token);
                match self.open.last_mut() {
                    Some(frame) if kind != DeclKind::Param => frame.pending.push(decl),
                    _ => self.declare(name, decl),
                }
            }
            N::LocalFuncDef => {
                if let Some((token, name)) = name_token(node) {
                    let decl = self.create(DeclKind::Function, node, token);
                    self.declare(name, decl);
                }
            }
            N::AssignStat => {
                for target in node.child(N::VarList).into_iter().flat_map(Node::children) {
                    if !target.is(N::NameExpr) {
                        continue;
                    }
                    let Some((token, name)) = name_token(target) else { continue };
                    if self.lookup(name).is_none() {
                        let decl = self.create(DeclKind::Global, target, token);
                        self.declare(name, decl);
                    }
                }
            }
            // `function f() end` assigns to a local `f` in sight, and to a global otherwise.
            N::NameExpr | N::FuncDef => {
                if let Some(decl) = name_token(node).and_then(|(_, name)| self.lookup(name)) {
                    self.target[node.id() as usize] = decl;
                }
            }
            _ => {}
        }
    }
}

impl Scopes {
    pub fn new(root: Node) -> Scopes {
        let count = root.tree().len();
        let mut walker = Walker {
            decls: Vec::new(),
            target: vec![NONE; count],
            visible: HashMap::new(),
            declared: Vec::new(),
            open: Vec::new(),
            funcs: Vec::new(),
        };
        for id in 0..count as u32 {
            let node = root.tree().node(root.source(), id);
            walker.close(id, root);
            walker.visit(node, node.next_id(), root);
        }
        Scopes { decls: walker.decls, target: walker.target }
    }

    pub fn decls(&self) -> &[Decl] {
        &self.decls
    }

    pub fn decl(&self, id: u32) -> &Decl {
        &self.decls[id as usize]
    }

    /// The declaration `node` refers to, or declares: a `NAME_EXPR`, or one of the nodes a `Decl` names.
    pub fn id_of(&self, node: Node) -> Option<u32> {
        self.target.get(node.id() as usize).copied().filter(|decl| *decl != NONE)
    }

    pub fn resolve(&self, node: Node) -> Option<&Decl> {
        self.id_of(node).map(|id| self.decl(id))
    }

    /// Whether `node` reads a local of a function that encloses its own.
    pub fn is_upvalue(&self, node: Node) -> bool {
        let Some(decl) = self.resolve(node) else { return false };
        decl.kind != DeclKind::Global && decl.func != node.ancestor(N::FuncBody).map_or(NONE, Node::id)
    }

    /// The nodes that refer to the declaration `id`, the declaring node included.
    pub fn uses(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        self.target.iter().enumerate().filter(move |(_, decl)| **decl == id).map(|(node, _)| node as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::tree::Tree;

    /// For each use of a name, `name@line -> kind@line`, or `name@line -> ?` when nothing declares it.
    fn resolved(src: &str) -> Vec<String> {
        let tree = Tree::parse(src);
        let root = tree.root(src);
        let scopes = Scopes::new(root);
        let line = |at: usize| src[..at].matches('\n').count() + 1;
        let mut out = Vec::new();
        for node in root.descendants().filter(|node| node.is(N::NameExpr)) {
            let from = format!("{}@{}", node.code(), line(node.code_range().0));
            let to = match scopes.resolve(node) {
                Some(decl) => {
                    let at = tree.tokens().start(decl.name as usize);
                    format!("{:?}@{}{}", decl.kind, line(at), if scopes.is_upvalue(node) { " up" } else { "" })
                }
                None => "?".to_owned(),
            };
            out.push(format!("{from} -> {to}"));
        }
        out
    }

    #[test]
    fn locals_shadow_and_see_the_outer_name_in_their_value() {
        assert_eq!(
            resolved("local a = 1\nlocal a = a\nprint(a)"),
            ["a@2 -> Local@1", "print@3 -> ?", "a@3 -> Local@2"]
        );
    }

    #[test]
    fn blocks_close_their_locals() {
        assert_eq!(resolved("do\n local a = 1\nend\nprint(a)")[1], "a@4 -> ?");
        assert_eq!(resolved("if x then\n local a\nelse\n a = 2\nend")[1], "a@4 -> Global@4");
    }

    #[test]
    fn parameters_and_upvalues() {
        let found = resolved("local n = 1\nfunction f(a, b)\n return function() return a + n end\nend");
        assert_eq!(found, ["a@3 -> Param@2 up", "n@3 -> Local@1 up"]);
    }

    #[test]
    fn a_local_function_sees_itself_a_local_closure_does_not() {
        assert_eq!(resolved("local function f() return f() end")[0], "f@1 -> Function@1 up");
        assert_eq!(resolved("local g = function() return g() end")[0], "g@1 -> ?");
    }

    #[test]
    fn loop_variables_live_in_the_body_only() {
        assert_eq!(resolved("local i = 9\nfor i = i, 10 do\n print(i)\nend")[0], "i@2 -> Local@1");
        assert_eq!(resolved("local i = 9\nfor i = i, 10 do\n print(i)\nend")[2], "i@3 -> LoopVar@2");
        let found = resolved("local v = {}\nfor k, v in pairs(v) do\n print(k, v)\nend\nprint(k)");
        assert_eq!(found[1], "v@2 -> Local@1");
        assert_eq!(found[3..], ["k@3 -> LoopVar@2", "v@3 -> LoopVar@2", "print@5 -> ?", "k@5 -> ?"]);
    }

    #[test]
    fn the_condition_of_repeat_reads_the_body() {
        assert_eq!(
            resolved("repeat\n local done = step()\nuntil done\nprint(done)")[1..],
            ["done@3 -> Local@2", "print@4 -> ?", "done@4 -> ?"]
        );
    }

    #[test]
    fn the_first_assignment_declares_a_global() {
        let found = resolved("count = 0\nfunction add()\n count = count + 1\n total = count\nend\nprint(count, total)");
        assert_eq!(
            found,
            [
                "count@1 -> Global@1",
                "count@3 -> Global@1",
                "count@3 -> Global@1",
                "total@4 -> Global@4",
                "count@4 -> Global@1",
                "print@6 -> ?",
                "count@6 -> Global@1",
                "total@6 -> ?",
            ]
        );
    }

    #[test]
    fn assignments_to_a_local_do_not_declare() {
        let src = "local a\na, b = 1, 2\nlocal t <const> = a";
        let tree = Tree::parse(src);
        let scopes = Scopes::new(tree.root(src));
        let kinds: Vec<DeclKind> = scopes.decls().iter().map(|decl| decl.kind).collect();
        assert_eq!(kinds, [DeclKind::Local, DeclKind::Global, DeclKind::Local]);
        assert_eq!(scopes.uses(0).count(), 3);
    }

    #[test]
    fn a_long_chain_of_operators_does_not_recurse() {
        let src = format!("local a = 1\nx = a{}", " + a".repeat(20_000));
        let tree = Tree::parse(&src);
        let scopes = Scopes::new(tree.root(&src));
        assert_eq!(scopes.uses(0).count(), 20_002);
    }
}
