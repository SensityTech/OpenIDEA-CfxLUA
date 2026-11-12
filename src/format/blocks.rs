//! The blocks of a file (`editor/formatter/blocks/`): one for each element of the tree that holds text, a
//! `LuaBlock` giving its children to the block around it, with the indent and the alignment its parent
//! gives it.

use super::Style;
use crate::lua::token::T;
use crate::lua::tree::{Element, N, Node, Tree};

pub(super) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Indent {
    None,
    Normal,
    Continuation,
    /// The continuation indent, for every child but the first one.
    ContinuationWithoutFirst,
}

/// Which block class of the JetBrains plugin a block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Script,
    IndentRange,
    FuncBody,
    Table,
    TableField,
    Assign,
    Binary,
    Unary,
    Paren,
    Call,
    ListArgs,
    Index,
}

/// The element type a spacing rule reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ty {
    N(N),
    T(T),
}

#[derive(Clone, Debug)]
pub(super) struct Block {
    pub parent: u32,
    pub prev: u32,
    pub next: u32,
    pub kind: Kind,
    pub ty: Ty,
    /// The node of a composite block, `NONE` for a token.
    pub node: u32,
    pub indent: Indent,
    pub align: u32,
    /// Its leaves: `first..end`.
    pub first: u32,
    pub end: u32,
    /// The alignment its children share: fields of a table, parameters, arguments, the dots of a chain.
    child_align: u32,
    /// The alignment of `=`: among the fields of a table, or along consecutive assignments.
    assign_align: u32,
    /// The first leaf of the first child that starts a line, once the layout has met it.
    pub line_child: u32,
}

pub(super) struct Align {
    /// Whether a block further right moves the blocks before it (`Alignment.createAlignment(true)`).
    pub backward: bool,
    /// The leaves that start a block aligned by it, in the order of the text.
    pub registered: Vec<u32>,
}

pub(super) struct Blocks {
    pub blocks: Vec<Block>,
    /// The token of each leaf, and its block.
    pub leaves: Vec<(u32, u32)>,
    pub aligns: Vec<Align>,
}

struct Builder<'a> {
    style: &'a Style,
    root: Node<'a>,
    /// The tokens that are not part of the formatted text: the `;` taken out.
    removed: &'a [u32],
    out: Blocks,
    /// The last child each block was given so far.
    last: Vec<u32>,
}

/// What is left of the walk of the tree: a child to give its block, with the indent a `LuaBlock` passes on,
/// or a block that has all its children.
enum Step<'a> {
    Child(u32, Element<'a>, Option<Indent>),
    Close(u32),
}

fn kind_of(kind: N) -> Kind {
    match kind {
        N::UnaryExpr => Kind::Unary,
        N::BinaryExpr => Kind::Binary,
        N::ParenExpr => Kind::Paren,
        N::ListArgs => Kind::ListArgs,
        N::FuncBody => Kind::FuncBody,
        N::TableExpr => Kind::Table,
        N::CallExpr => Kind::Call,
        N::DoStat | N::WhileStat | N::RepeatStat | N::IfStat | N::ForAStat | N::ForBStat => Kind::IndentRange,
        N::IndexExpr => Kind::Index,
        N::AssignStat | N::CompoundAssignStat | N::LocalDef => Kind::Assign,
        _ => Kind::Script,
    }
}

impl<'a> Builder<'a> {
    fn is_removed(&self, token: usize) -> bool {
        self.removed.binary_search(&(token as u32)).is_ok()
    }

    fn holds_text(&self, node: Node) -> bool {
        node.all_tokens().any(|token| !token.is(T::Whitespace) && !self.is_removed(token.index()))
    }

    /// Whether the index a block stands for is a safe navigation: `a?.b`.
    fn asks(&self, block: u32) -> bool {
        let node = self.out.node(self.root.tree(), self.root.source(), block);
        node.is_some_and(|node| node.has(T::Question))
    }

    fn align(&mut self, backward: bool) -> u32 {
        self.out.aligns.push(Align { backward, registered: Vec::new() });
        self.out.aligns.len() as u32 - 1
    }

    fn child_align(&mut self, block: u32, backward: bool) -> u32 {
        if self.out.blocks[block as usize].child_align == NONE {
            self.out.blocks[block as usize].child_align = self.align(backward);
        }
        self.out.blocks[block as usize].child_align
    }

    /// The alignment of the `=` of an assignment: the one of the assignment before it, comments skipped,
    /// or its own (`LuaAssignBlock.assignAlign`). The assignments passed on the way back take it too.
    fn assign_align(&mut self, block: u32) -> u32 {
        let mut passed = Vec::new();
        let mut at = block;
        let found = loop {
            let known = self.out.blocks[at as usize].assign_align;
            if known != NONE {
                break known;
            }
            passed.push(at);
            let mut prev = self.out.blocks[at as usize].prev;
            while prev != NONE && matches!(self.out.blocks[prev as usize].ty, Ty::T(kind) if kind.is_comment()) {
                prev = self.out.blocks[prev as usize].prev;
            }
            if prev == NONE || self.out.blocks[prev as usize].kind != Kind::Assign {
                break NONE;
            }
            at = prev;
        };
        let align = if found == NONE { self.align(true) } else { found };
        for assignment in passed {
            self.out.blocks[assignment as usize].assign_align = align;
        }
        align
    }

    /// The alignment of the dots of a chain: the one of the index around it, directly or through a call
    /// (`LuaIndexExprBlock.align`). The indexes passed on the way up take it too: a chain is walked once.
    fn dot_align(&mut self, block: u32) -> u32 {
        let mut passed = Vec::new();
        let mut at = block;
        let align = loop {
            let known = self.out.blocks[at as usize].child_align;
            if known != NONE {
                break known;
            }
            passed.push(at);
            let parent = self.out.blocks[at as usize].parent;
            let outer = match parent {
                NONE => NONE,
                parent if self.out.blocks[parent as usize].kind == Kind::Index => parent,
                parent if self.out.blocks[parent as usize].ty == Ty::N(N::CallExpr) => {
                    let above = self.out.blocks[parent as usize].parent;
                    if above != NONE && self.out.blocks[above as usize].kind == Kind::Index { above } else { NONE }
                }
                _ => NONE,
            };
            if outer == NONE {
                break self.align(true);
            }
            at = outer;
        };
        for index in passed {
            self.out.blocks[index as usize].child_align = align;
        }
        align
    }

    /// The indent, the alignment and the class a parent gives a child (`buildChild`).
    fn given(&mut self, parent: u32, child: Element, passed: Option<Indent>) -> (Indent, u32, Option<Kind>) {
        let kind = self.out.blocks[parent as usize].kind;
        let token = child.token().map(|token| token.kind());
        let inner = child.node().map(|node| node.kind());
        match kind {
            Kind::FuncBody if inner == Some(N::ParamNameDef) => {
                let align = if self.style.align_parameters { self.child_align(parent, false) } else { NONE };
                (Indent::Continuation, align, None)
            }
            Kind::Table if inner == Some(N::TableField) => {
                (Indent::Normal, self.child_align(parent, false), Some(Kind::TableField))
            }
            Kind::Table if token.is_some_and(T::is_comment) => (Indent::Normal, NONE, None),
            Kind::TableField if self.style.align_table_field_assign && token == Some(T::Assign) => {
                let table = self.out.blocks[parent as usize].parent;
                if self.out.blocks[table as usize].assign_align == NONE {
                    self.out.blocks[table as usize].assign_align = self.align(true);
                }
                (Indent::None, self.out.blocks[table as usize].assign_align, None)
            }
            Kind::Assign
                if self.style.align_consecutive_assignments
                    && token.is_some_and(|token| token == T::Assign || token.is_compound_assign()) =>
            {
                (Indent::Continuation, self.assign_align(parent), None)
            }
            Kind::Binary => (Indent::ContinuationWithoutFirst, NONE, None),
            Kind::Paren if !matches!(token, Some(T::LParen | T::RParen)) => (Indent::Continuation, NONE, None),
            Kind::ListArgs if inner.is_some_and(N::is_expr) => {
                let align = if self.style.align_arguments { self.child_align(parent, false) } else { NONE };
                (Indent::Continuation, align, None)
            }
            // The suffix of an index starts at its `?` where it has one: `a?.b`.
            Kind::Index
                if match self.asks(parent) {
                    true => token == Some(T::Question),
                    false => matches!(token, Some(T::Dot | T::Colon)),
                } =>
            {
                (Indent::Continuation, self.dot_align(parent), None)
            }
            _ => (passed.unwrap_or(Indent::None), NONE, None),
        }
    }

    /// Gives a child its block, after the children its parent has. Returns the block and the node of a
    /// child that has children of its own.
    fn child(&mut self, parent: u32, child: Element<'a>, passed: Option<Indent>) -> Option<(u32, Node<'a>)> {
        let held = match child {
            Element::Token(token) => !token.is(T::Whitespace) && !self.is_removed(token.index()),
            Element::Node(inner) => self.holds_text(inner),
        };
        if !held {
            return None;
        }
        let (indent, align, class) = self.given(parent, child, passed);
        let id = self.out.blocks.len() as u32;
        let prev = self.last[parent as usize];
        let (kind, ty, element) = match child {
            Element::Token(token) => (Kind::Script, Ty::T(token.kind()), NONE),
            Element::Node(inner) => (class.unwrap_or_else(|| kind_of(inner.kind())), Ty::N(inner.kind()), inner.id()),
        };
        self.out.blocks.push(Block {
            parent,
            prev,
            next: NONE,
            kind,
            ty,
            node: element,
            indent,
            align,
            first: self.out.leaves.len() as u32,
            end: 0,
            child_align: NONE,
            assign_align: NONE,
            line_child: NONE,
        });
        self.last.push(NONE);
        if prev != NONE {
            self.out.blocks[prev as usize].next = id;
        }
        self.last[parent as usize] = id;
        match child {
            Element::Token(token) => {
                self.out.leaves.push((token.index() as u32, id));
                self.out.blocks[id as usize].end = self.out.leaves.len() as u32;
                None
            }
            Element::Node(inner) => Some((id, inner)),
        }
    }

    /// Leaves the children of a block to be visited, the first one last: those of its node, and for a
    /// `LuaBlock` among them the statements it holds, indented (`LuaScriptBlock.buildChildren`).
    fn open(&self, block: u32, node: Node<'a>, left: &mut Vec<Step<'a>>) {
        left.push(Step::Close(block));
        let from = left.len();
        for element in node.elements() {
            match element {
                Element::Node(body) if body.is(N::Block) => {
                    left.extend(body.elements().map(|inner| Step::Child(block, inner, Some(Indent::Normal))));
                }
                other => left.push(Step::Child(block, other, None)),
            }
        }
        left[from..].reverse();
    }
}

/// The blocks of the file `root`, without the tokens `removed`, which are sorted. The tree is walked
/// without recursion: a long sum or a long chain of calls is as deep as it is long.
pub(super) fn build(root: Node, style: &Style, removed: &[u32]) -> Blocks {
    let out = Blocks { blocks: Vec::new(), leaves: Vec::new(), aligns: Vec::new() };
    let mut builder = Builder { style, root, removed, out, last: vec![NONE] };
    builder.out.blocks.push(Block {
        parent: NONE,
        prev: NONE,
        next: NONE,
        kind: Kind::Script,
        ty: Ty::N(N::File),
        node: root.id(),
        indent: Indent::None,
        align: NONE,
        first: 0,
        end: 0,
        child_align: NONE,
        assign_align: NONE,
        line_child: NONE,
    });
    let mut left = Vec::new();
    builder.open(0, root, &mut left);
    while let Some(step) = left.pop() {
        match step {
            Step::Close(block) => builder.out.blocks[block as usize].end = builder.out.leaves.len() as u32,
            Step::Child(parent, child, passed) => {
                if let Some((block, node)) = builder.child(parent, child, passed) {
                    builder.open(block, node, &mut left);
                }
            }
        }
    }
    builder.out
}

impl Blocks {
    pub fn node<'a>(&self, tree: &'a Tree, src: &'a str, block: u32) -> Option<Node<'a>> {
        let node = self.blocks[block as usize].node;
        (node != NONE).then(|| tree.node(src, node))
    }

    /// Whether a block is a statement (`psi is LuaStatement`).
    pub fn is_statement(&self, block: u32) -> bool {
        matches!(self.blocks[block as usize].ty, Ty::N(kind) if kind.is_statement())
    }

    /// The alignment of the blocks that start where `block` does, the innermost first.
    pub fn align_at_start(&self, block: u32) -> u32 {
        let first = self.blocks[block as usize].first;
        let mut at = block;
        while at != NONE && self.blocks[at as usize].first == first {
            if self.blocks[at as usize].align != NONE {
                return self.blocks[at as usize].align;
            }
            at = self.blocks[at as usize].parent;
        }
        NONE
    }
}
