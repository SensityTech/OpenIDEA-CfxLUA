//! What a documentation comment says, read from its tree: the accessors of the JetBrains plugin
//! (`LuaCommentImpl`, `LuaDocPsiImplUtil`).

use crate::lua::token::T;
use crate::lua::tree::{Element, N, Node};

/// The types written directly in `node`, in source order.
pub fn types(node: Node) -> impl Iterator<Item = Node> {
    node.children().filter(|child| child.kind().is_doc_type())
}

/// The last type written before the `]` of an index signature, or the first one after it.
fn around_bracket(node: Node, after: bool) -> Option<Node> {
    let mut closed = false;
    let mut before = None;
    for element in node.elements() {
        match element {
            Element::Token(token) if token.is(T::RBrack) => {
                if !after {
                    return before;
                }
                closed = true;
            }
            Element::Node(child) if child.kind().is_doc_type() => {
                if after && closed {
                    return Some(child);
                }
                before = Some(child);
            }
            _ => {}
        }
    }
    if after { None } else { before }
}

/// The type a tag, a field or a parameter gives: the value type of an index signature `[K] V`.
pub fn ty(node: Node) -> Option<Node> {
    if node.has(T::LBrack) { around_bracket(node, true) } else { types(node).next() }
}

/// The key type of an index signature `[K] V`; a named field has none.
pub fn key_ty(node: Node) -> Option<Node> {
    if node.has(T::LBrack) { around_bracket(node, false) } else { None }
}

pub fn name(node: Node<'_>) -> Option<&str> {
    node.token(T::Id).map(|id| id.text())
}

/// The text after the type of a tag, without the sign that starts it.
pub fn description(node: Node<'_>) -> &str {
    let text = node.child(N::DocCommentString).and_then(|comment| comment.token(T::DocString));
    text.map_or("", |text| text.text().trim())
}

/// Whether the type admits nil: `T?`, `T|nil`, `nil`.
pub fn accepts_nil(ty: Node) -> bool {
    match ty.kind() {
        N::DocNullableTy => true,
        N::DocGeneralTy => ty.code() == "nil",
        N::DocUnionTy => types(ty).any(accepts_nil),
        N::DocParTy => types(ty).next().is_some_and(accepts_nil),
        _ => false,
    }
}

/// A parameter or a field that may be left out: `name?`, or a type that admits nil.
pub fn is_optional(node: Node) -> bool {
    let nil = |node: Node| ty(node).is_some_and(accepts_nil);
    node.has(T::Question) || nil(node) || node.children_of(N::DocAliasMember).any(nil)
}

/// The names of the returned values of a type list, an empty one for a value that has none.
pub fn value_names(list: Node<'_>) -> Vec<&str> {
    let mut out = Vec::new();
    let mut unnamed = false;
    for element in list.elements() {
        match element {
            Element::Node(node) if node.kind().is_doc_type() => {
                if unnamed {
                    out.push("");
                }
                unnamed = true;
            }
            Element::Token(token) if unnamed && token.is(T::Id) => {
                out.push(token.text());
                unnamed = false;
            }
            _ => {}
        }
    }
    if unnamed {
        out.push("");
    }
    out
}

/// `---|>` marks the default value of an alias.
pub fn is_default(member: Node) -> bool {
    member.first_token().is_some_and(|bar| bar.text().starts_with("|>"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

impl Visibility {
    pub fn of(text: &str) -> Visibility {
        match text {
            "private" => Visibility::Private,
            "protected" => Visibility::Protected,
            _ => Visibility::Public,
        }
    }
}

/// The visibility a field tag states, public when it states none.
pub fn visibility(node: Node) -> Visibility {
    node.child(N::DocAccessModifier).map_or(Visibility::Public, |modifier| Visibility::of(modifier.code()))
}

/// One documentation comment: a `DocComment` of `Source::docs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment<'a>(pub Node<'a>);

impl<'a> Comment<'a> {
    pub fn tag(self, kind: N) -> Option<Node<'a>> {
        self.0.child(kind)
    }

    pub fn tags(self, kind: N) -> impl Iterator<Item = Node<'a>> {
        self.0.children_of(kind)
    }

    /// The nodes of a kind anywhere in the comment, those written inside a type included.
    pub fn all(self, kind: N) -> impl Iterator<Item = Node<'a>> {
        self.0.descendants().filter(move |node| node.is(kind))
    }

    pub fn param(self, name: &str) -> Option<Node<'a>> {
        let named = |tag: &Node| tag.child(N::DocParamNameRef).is_some_and(|reference| reference.code() == name);
        self.tags(N::DocTagParam).find(named)
    }

    pub fn field(self, wanted: &str) -> Option<Node<'a>> {
        self.tags(N::DocTagField).find(|tag| name(*tag) == Some(wanted))
    }

    /// A tag the grammar has no rule for, as `@deprecated` or `@async`.
    pub fn has_tag_named(self, wanted: &str) -> bool {
        let named = |tag: Node| tag.token(T::DocTagName).is_some_and(|name| name.text() == wanted);
        self.all(N::DocTagDef).any(named)
    }

    pub fn is_deprecated(self) -> bool {
        self.has_tag_named("deprecated")
    }

    /// The name a `@module` tag gives to the file.
    pub fn module_name(self) -> Option<&'a str> {
        self.tag(N::DocTagClass).filter(|class| class.has(T::DocTagModule)).and_then(name)
    }
}
