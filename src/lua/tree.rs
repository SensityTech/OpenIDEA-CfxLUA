use super::token::{T, Tokens};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum N {
    File,
    Block,
    ShebangLine,
    LocalDef,
    NameList,
    NameDef,
    Attribute,
    ExprList,
    AssignStat,
    VarList,
    CompoundAssignStat,
    ExprStat,
    DoStat,
    IfStat,
    WhileStat,
    RepeatStat,
    ForAStat,
    ForBStat,
    LabelStat,
    GotoStat,
    BreakStat,
    ReturnStat,
    EmptyStat,
    FuncDef,
    ClassMethodDef,
    ClassMethodName,
    LocalFuncDef,
    FuncBody,
    ParamNameDef,
    LiteralExpr,
    NameExpr,
    IndexExpr,
    CallExpr,
    ListArgs,
    SingleArg,
    TableExpr,
    TableField,
    TableFieldSep,
    BinaryExpr,
    BinaryOp,
    UnaryExpr,
    UnaryOp,
    ParenExpr,
    ClosureExpr,
    DocFile,
    DocComment,
    DocTagParam,
    DocTagAlias,
    DocTagSuppress,
    DocTagVararg,
    DocTagReturn,
    DocTagClass,
    DocTagField,
    DocTagType,
    DocTagLan,
    DocTagOverload,
    DocTagSee,
    DocTagCast,
    DocTagDef,
    DocAccessModifier,
    DocTagGenericList,
    DocGenericDef,
    DocParamNameRef,
    DocClassNameRef,
    DocClassAttr,
    DocCommentString,
    DocTypeList,
    DocAliasMember,
    DocCastItem,
    DocLanId,
    DocUnionTy,
    DocFunctionTy,
    DocFunctionParam,
    DocVarargParam,
    DocTableTy,
    DocTableDef,
    DocTableField,
    DocTableIndexField,
    DocGenericTy,
    DocArrTy,
    DocNullableTy,
    DocGeneralTy,
    DocParTy,
    DocStringLiteralTy,
    DocNumberLiteralTy,
    DocTupleTy,
    Error,
}

impl N {
    pub fn is_statement(self) -> bool {
        matches!(
            self,
            N::LocalDef
                | N::AssignStat
                | N::CompoundAssignStat
                | N::ExprStat
                | N::DoStat
                | N::IfStat
                | N::WhileStat
                | N::RepeatStat
                | N::ForAStat
                | N::ForBStat
                | N::LabelStat
                | N::GotoStat
                | N::BreakStat
                | N::ReturnStat
                | N::EmptyStat
                | N::FuncDef
                | N::ClassMethodDef
                | N::LocalFuncDef
        )
    }

    pub fn is_expr(self) -> bool {
        matches!(
            self,
            N::LiteralExpr
                | N::NameExpr
                | N::IndexExpr
                | N::CallExpr
                | N::TableExpr
                | N::BinaryExpr
                | N::UnaryExpr
                | N::ParenExpr
                | N::ClosureExpr
        )
    }

    /// A type of a documentation comment.
    pub fn is_doc_type(self) -> bool {
        matches!(
            self,
            N::DocUnionTy
                | N::DocFunctionTy
                | N::DocTableTy
                | N::DocGenericTy
                | N::DocArrTy
                | N::DocNullableTy
                | N::DocGeneralTy
                | N::DocParTy
                | N::DocStringLiteralTy
                | N::DocNumberLiteralTy
                | N::DocTupleTy
        )
    }

    /// The name IntelliJ prints for this element type.
    pub fn name(self) -> &'static str {
        match self {
            N::File => "FILE",
            N::Block => "LuaBlock",
            N::ShebangLine => "SHEBANG_LINE",
            N::LocalDef => "LOCAL_DEF",
            N::NameList => "NAME_LIST",
            N::NameDef => "NAME_DEF",
            N::Attribute => "ATTRIBUTE",
            N::ExprList => "EXPR_LIST",
            N::AssignStat => "ASSIGN_STAT",
            N::VarList => "VAR_LIST",
            N::CompoundAssignStat => "COMPOUND_ASSIGN_STAT",
            N::ExprStat => "CALL_STAT",
            N::DoStat => "DO_STAT",
            N::IfStat => "IF_STAT",
            N::WhileStat => "WHILE_STAT",
            N::RepeatStat => "REPEAT_STAT",
            N::ForAStat => "FOR_A_STAT",
            N::ForBStat => "FOR_B_STAT",
            N::LabelStat => "LABEL_STAT",
            N::GotoStat => "GOTO_STAT",
            N::BreakStat => "BREAK_STAT",
            N::ReturnStat => "RETURN_STAT",
            N::EmptyStat => "EMPTY_STAT",
            N::FuncDef => "Global Function",
            N::ClassMethodDef => "Class Method",
            N::ClassMethodName => "CLASS_METHOD_NAME",
            N::LocalFuncDef => "LOCAL_FUNC_DEF",
            N::FuncBody => "FUNC_BODY",
            N::ParamNameDef => "PARAM_NAME_DEF",
            N::LiteralExpr => "LITERAL_EXPR",
            N::NameExpr => "NAME_EXPR",
            N::IndexExpr => "INDEX_EXPR",
            N::CallExpr => "CALL_EXPR",
            N::ListArgs => "LIST_ARGS",
            N::SingleArg => "SINGLE_ARG",
            N::TableExpr => "TABLE_EXPR",
            N::TableField => "TABLE_FIELD",
            N::TableFieldSep => "TABLE_FIELD_SEP",
            N::BinaryExpr => "BINARY_EXPR",
            N::BinaryOp => "BINARY_OP",
            N::UnaryExpr => "UNARY_EXPR",
            N::UnaryOp => "UNARY_OP",
            N::ParenExpr => "PAREN_EXPR",
            N::ClosureExpr => "CLOSURE_EXPR",
            N::DocFile => "DOC_FILE",
            N::DocComment => "DOC_COMMENT",
            N::DocTagParam => "TAG_PARAM",
            N::DocTagAlias => "TAG_ALIAS",
            N::DocTagSuppress => "TAG_SUPPRESS",
            N::DocTagVararg => "TAG_VARARG",
            N::DocTagReturn => "TAG_RETURN",
            N::DocTagClass => "TAG_CLASS",
            N::DocTagField => "TAG_FIELD",
            N::DocTagType => "TAG_TYPE",
            N::DocTagLan => "TAG_LAN",
            N::DocTagOverload => "TAG_OVERLOAD",
            N::DocTagSee => "TAG_SEE",
            N::DocTagCast => "TAG_CAST",
            N::DocTagDef => "TAG_DEF",
            N::DocAccessModifier => "ACCESS_MODIFIER",
            N::DocTagGenericList => "TAG_GENERIC_LIST",
            N::DocGenericDef => "GENERIC_DEF",
            N::DocParamNameRef => "PARAM_NAME_REF",
            N::DocClassNameRef => "CLASS_NAME_REF",
            N::DocClassAttr => "CLASS_ATTR",
            N::DocCommentString => "COMMENT_STRING",
            N::DocTypeList => "TYPE_LIST",
            N::DocAliasMember => "ALIAS_MEMBER",
            N::DocCastItem => "CAST_ITEM",
            N::DocLanId => "LAN_ID",
            N::DocUnionTy => "UNION_TY",
            N::DocFunctionTy => "FUNCTION_TY",
            N::DocFunctionParam => "FUNCTION_PARAM",
            N::DocVarargParam => "VARARG_PARAM",
            N::DocTableTy => "TABLE_TY",
            N::DocTableDef => "TABLE_DEF",
            N::DocTableField => "TABLE_FIELD",
            N::DocTableIndexField => "TABLE_INDEX_FIELD",
            N::DocGenericTy => "GENERIC_TY",
            N::DocArrTy => "ARR_TY",
            N::DocNullableTy => "NULLABLE_TY",
            N::DocGeneralTy => "GENERAL_TY",
            N::DocParTy => "PAR_TY",
            N::DocStringLiteralTy => "STRING_LITERAL_TY",
            N::DocNumberLiteralTy => "NUMBER_LITERAL_TY",
            N::DocTupleTy => "TUPLE_TY",
            N::Error => "ERROR_ELEMENT",
        }
    }
}

pub(super) const NONE: u32 = u32::MAX;

/// Nodes are stored in pre-order: the subtree of `i` is `i + 1 .. next`, its tokens `first .. end`.
#[derive(Clone, Copy, Debug)]
pub(super) struct NodeData {
    pub kind: N,
    pub parent: u32,
    pub first: u32,
    pub end: u32,
    pub next: u32,
}

pub struct Tree {
    pub(super) tokens: Rc<Tokens>,
    pub(super) nodes: Vec<NodeData>,
    pub(super) errors: Vec<(u32, Box<str>)>,
}

impl Tree {
    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn root<'a>(&'a self, src: &'a str) -> Node<'a> {
        Node { tree: self, src, id: 0 }
    }

    pub fn node<'a>(&'a self, src: &'a str, id: u32) -> Node<'a> {
        Node { tree: self, src, id }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// How many nodes stand around the deepest one, itself included. The parser bounds what is nested, not
    /// what is chained: a sum or a chain of calls is as deep as it is long.
    pub fn depth(&self) -> u32 {
        let mut depths: Vec<u32> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            depths.push(if node.parent == NONE { 1 } else { depths[node.parent as usize] + 1 });
        }
        depths.into_iter().max().unwrap_or(0)
    }

    pub fn errors<'a>(&'a self, src: &'a str) -> impl Iterator<Item = (Node<'a>, &'a str)> {
        self.errors.iter().map(move |(id, message)| (self.node(src, *id), &**message))
    }

    /// The deepest node that holds the token `index`.
    pub fn node_of_token(&self, index: usize) -> u32 {
        let index = index as u32;
        let mut id = self.nodes.partition_point(|n| n.first <= index).saturating_sub(1) as u32;
        while id != 0 && self.nodes[id as usize].end <= index {
            id = self.nodes[id as usize].parent;
        }
        id
    }

    /// The deepest node whose text holds the byte `at`.
    pub fn node_at<'a>(&'a self, src: &'a str, at: usize) -> Node<'a> {
        let id = self.tokens.at(at).map_or(0, |token| self.node_of_token(token));
        self.node(src, id)
    }
}

#[derive(Clone, Copy)]
pub struct Node<'a> {
    tree: &'a Tree,
    src: &'a str,
    id: u32,
}

#[derive(Clone, Copy)]
pub struct Token<'a> {
    tree: &'a Tree,
    src: &'a str,
    index: u32,
}

#[derive(Clone, Copy)]
pub enum Element<'a> {
    Node(Node<'a>),
    Token(Token<'a>),
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.tree, other.tree)
    }
}

impl Eq for Node<'_> {}

impl std::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{:?}", self.kind().name(), self.range())
    }
}

impl std::fmt::Debug for Token<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.kind().name(), self.text())
    }
}

impl<'a> Node<'a> {
    fn data(self) -> &'a NodeData {
        &self.tree.nodes[self.id as usize]
    }

    fn at(self, id: u32) -> Node<'a> {
        Node { id, ..self }
    }

    fn token_at(self, index: u32) -> Token<'a> {
        Token { tree: self.tree, src: self.src, index }
    }

    pub fn id(self) -> u32 {
        self.id
    }

    /// The id of the first node after this one and its descendants.
    pub fn next_id(self) -> u32 {
        self.data().next
    }

    pub fn tree(self) -> &'a Tree {
        self.tree
    }

    pub fn source(self) -> &'a str {
        self.src
    }

    pub fn kind(self) -> N {
        self.data().kind
    }

    pub fn is(self, kind: N) -> bool {
        self.kind() == kind
    }

    pub fn parent(self) -> Option<Node<'a>> {
        let parent = self.data().parent;
        (parent != NONE).then(|| self.at(parent))
    }

    pub fn ancestors(self) -> impl Iterator<Item = Node<'a>> {
        std::iter::successors(self.parent(), |node| node.parent())
    }

    pub fn ancestor(self, kind: N) -> Option<Node<'a>> {
        self.ancestors().find(|node| node.is(kind))
    }

    pub fn children(self) -> impl Iterator<Item = Node<'a>> {
        let end = self.data().next;
        let mut next = self.id + 1;
        std::iter::from_fn(move || {
            (next < end).then(|| {
                let child = self.at(next);
                next = child.data().next;
                child
            })
        })
    }

    /// The nodes below this one, in source order, this node excluded.
    pub fn descendants(self) -> impl Iterator<Item = Node<'a>> {
        (self.id + 1..self.data().next).map(move |id| self.at(id))
    }

    pub fn child(self, kind: N) -> Option<Node<'a>> {
        self.children().find(|child| child.is(kind))
    }

    pub fn children_of(self, kind: N) -> impl Iterator<Item = Node<'a>> {
        self.children().filter(move |child| child.is(kind))
    }

    pub fn exprs(self) -> impl Iterator<Item = Node<'a>> {
        self.children().filter(|child| child.kind().is_expr())
    }

    pub fn expr(self) -> Option<Node<'a>> {
        self.exprs().next()
    }

    pub fn next_sibling(self) -> Option<Node<'a>> {
        let parent = self.parent()?;
        let next = self.data().next;
        (next < parent.data().next).then(|| self.at(next))
    }

    pub fn prev_sibling(self) -> Option<Node<'a>> {
        self.parent()?.children().take_while(|child| child.id < self.id).last()
    }

    /// Child nodes and tokens in source order, white space and comments included.
    pub fn elements(self) -> impl Iterator<Item = Element<'a>> {
        let data = self.data();
        let mut child = self.id + 1;
        let mut token = data.first;
        std::iter::from_fn(move || {
            if child < data.next {
                let node = self.at(child);
                if node.data().first <= token {
                    child = node.data().next;
                    token = token.max(node.data().end);
                    return Some(Element::Node(node));
                }
            }
            (token < data.end).then(|| {
                token += 1;
                Element::Token(self.token_at(token - 1))
            })
        })
    }

    /// The tokens that are direct children, white space and comments excluded.
    pub fn tokens(self) -> impl Iterator<Item = Token<'a>> {
        self.elements().filter_map(|element| match element {
            Element::Token(token) if !token.kind().is_trivia() => Some(token),
            _ => None,
        })
    }

    pub fn token(self, kind: T) -> Option<Token<'a>> {
        self.tokens().find(|token| token.kind() == kind)
    }

    pub fn has(self, kind: T) -> bool {
        self.token(kind).is_some()
    }

    /// Every token of the subtree, white space and comments included.
    pub fn all_tokens(self) -> impl Iterator<Item = Token<'a>> {
        (self.data().first..self.data().end).map(move |index| self.token_at(index))
    }

    pub fn first_token(self) -> Option<Token<'a>> {
        self.all_tokens().find(|token| !token.kind().is_trivia())
    }

    pub fn last_token(self) -> Option<Token<'a>> {
        (self.data().first..self.data().end)
            .rev()
            .map(|index| self.token_at(index))
            .find(|token| !token.kind().is_trivia())
    }

    pub fn token_span(self) -> (usize, usize) {
        (self.data().first as usize, self.data().end as usize)
    }

    /// Byte range of the node, bound comments included; an empty node sits where the next token starts.
    pub fn range(self) -> (usize, usize) {
        let data = self.data();
        let tokens = &self.tree.tokens;
        if data.first >= data.end {
            let at =
                if (data.first as usize) < tokens.len() { tokens.start(data.first as usize) } else { self.src.len() };
            return (at, at);
        }
        (tokens.start(data.first as usize), tokens.end(data.end as usize - 1))
    }

    /// Byte range from the first to the last token that is not white space or a comment.
    pub fn code_range(self) -> (usize, usize) {
        match (self.first_token(), self.last_token()) {
            (Some(first), Some(last)) => (first.start(), last.end()),
            _ => self.range(),
        }
    }

    pub fn text(self) -> &'a str {
        let (start, end) = self.range();
        &self.src[start..end]
    }

    pub fn code(self) -> &'a str {
        let (start, end) = self.code_range();
        &self.src[start..end]
    }

    pub fn error(self) -> Option<&'a str> {
        if !self.is(N::Error) {
            return None;
        }
        let errors = &self.tree.errors;
        errors.binary_search_by_key(&self.id, |(id, _)| *id).ok().map(|i| &*errors[i].1)
    }
}

impl<'a> Token<'a> {
    pub fn index(self) -> usize {
        self.index as usize
    }

    pub fn kind(self) -> T {
        self.tree.tokens.kind(self.index as usize)
    }

    pub fn is(self, kind: T) -> bool {
        self.kind() == kind
    }

    pub fn start(self) -> usize {
        self.tree.tokens.start(self.index as usize)
    }

    pub fn end(self) -> usize {
        self.tree.tokens.end(self.index as usize)
    }

    pub fn range(self) -> (usize, usize) {
        (self.start(), self.end())
    }

    pub fn text(self) -> &'a str {
        &self.src[self.start()..self.end()]
    }

    pub fn parent(self) -> Node<'a> {
        Node { tree: self.tree, src: self.src, id: self.tree.node_of_token(self.index as usize) }
    }
}

impl<'a> Element<'a> {
    pub fn node(self) -> Option<Node<'a>> {
        match self {
            Element::Node(node) => Some(node),
            Element::Token(_) => None,
        }
    }

    pub fn token(self) -> Option<Token<'a>> {
        match self {
            Element::Token(token) => Some(token),
            Element::Node(_) => None,
        }
    }
}

/// The tree as IntelliJ's parser tests print it: element types, leaves with their text, white space skipped.
#[cfg(test)]
pub fn dump(root: Node) -> String {
    fn print(out: &mut String, element: Element, depth: usize) {
        match element {
            Element::Token(token) if token.is(T::Whitespace) => {}
            Element::Token(token) => {
                let text = token.text().replace('\n', "\\n").replace('\r', "\\r");
                out.push_str(&format!("{}{} '{}'\n", "  ".repeat(depth), token.kind().name(), text));
            }
            Element::Node(node) => {
                out.push_str(&format!("{}{}\n", "  ".repeat(depth), node.kind().name()));
                for child in node.elements() {
                    print(out, child, depth + 1);
                }
            }
        }
    }
    let mut out = String::new();
    print(&mut out, Element::Node(root), 0);
    out
}
