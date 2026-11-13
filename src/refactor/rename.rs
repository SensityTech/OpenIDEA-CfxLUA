use crate::editor::definition;
use crate::editor::target::{documented_param, token_at};
use crate::feature::Fix;
use crate::lua::Source;
use crate::lua::scope::DeclKind;
use crate::lua::token::{self, T};
use crate::lua::tree::{N, Node};

/// A name the file alone can rename, and where it is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renamed {
    pub name: String,
    /// Sorted and disjoint.
    pub ranges: Vec<(usize, usize)>,
}

enum Declared<'a> {
    Local(u32),
    Label(Node<'a>),
}

/// The local, parameter, loop variable, local function or label named at `at`, from its declaration, one of its
/// uses or the `@param` that documents it; nothing for a global, a field or a method.
pub fn at(src: &Source, at: usize) -> Option<Renamed> {
    let root = src.root();
    let (tree, tokens) = (root.tree(), root.tree().tokens());
    let index = token_at(tokens, at, |kind| matches!(kind, T::Id | T::DocComment))?;
    let declared = match tokens.kind(index) {
        T::DocComment => Declared::Local(local(src, param_named(src, at)?)?),
        _ => {
            let node = tree.node(root.source(), tree.node_of_token(index));
            match node.kind() {
                N::GotoStat => Declared::Label(definition::label(node, tokens.text(root.source(), index))?),
                N::LabelStat => Declared::Label(node),
                _ => Declared::Local(local(src, node)?),
            }
        }
    };
    let (name, mut ranges) = match declared {
        Declared::Local(decl) => local_ranges(src, decl),
        Declared::Label(label) => label_ranges(root, label)?,
    };
    ranges.sort_unstable();
    ranges.dedup();
    Some(Renamed { name, ranges })
}

/// Why `new` cannot replace `old`, in a few words.
pub fn valid(old: &str, new: &str) -> Result<(), String> {
    if new.is_empty() {
        return Err("Enter a new name".to_owned());
    }
    if new == old {
        return Err("The name is unchanged".to_owned());
    }
    if T::keyword(new).is_some() {
        return Err(format!("'{new}' is a keyword"));
    }
    if !token::is_identifier(new) {
        return Err(format!("'{new}' is not a valid Lua identifier"));
    }
    Ok(())
}

pub fn edits(found: &Renamed, new: &str) -> Vec<(usize, usize, String)> {
    found.ranges.iter().map(|&(start, end)| (start, end, new.to_owned())).collect()
}

const ASKED: &str = "rename:";

/// The fix that renames what is named at `at`: its edits wait for the name, asked once it is chosen.
pub fn fix(src: &Source, at: usize, title: impl FnOnce(&str) -> String) -> Option<Fix> {
    let found = self::at(src, at)?;
    Some(Fix::new(title(&found.name)).asking(format!("{ASKED}{at}:{}", found.name)))
}

/// The edits of a fix `fix` gave, once `ask` has given a name: it shows a question and a text to change, and
/// answers nothing when the question is dismissed. A name that cannot be is asked again, with the reason.
pub fn asked(
    src: &Source,
    data: &str,
    ask: &mut dyn FnMut(&str, &str) -> Option<String>,
) -> Option<Vec<(usize, usize, String)>> {
    let (at, name) = data.strip_prefix(ASKED)?.split_once(':')?;
    // The document may have changed since the fix was offered.
    let found = self::at(src, at.parse().ok()?).filter(|found| found.name == name)?;
    let mut question = format!("Rename '{name}' to:");
    let mut entered = name.to_owned();
    loop {
        entered = ask(&question, &entered)?.trim().to_owned();
        match valid(name, &entered) {
            Ok(()) => return Some(edits(&found, &entered)),
            Err(_) if entered == name => return None,
            Err(why) => question = format!("{why}. Rename '{name}' to:"),
        }
    }
}

fn param_named(src: &Source, at: usize) -> Option<Node<'_>> {
    let docs = src.docs();
    let (tree, tokens) = (docs.tree(), docs.tree().tokens());
    let index = token_at(tokens, at, |kind| kind == T::Id)?;
    let reference = tree.node(docs.source(), tree.node_of_token(index));
    reference.is(N::DocParamNameRef).then(|| documented_param(src, reference)).flatten()
}

fn local(src: &Source, node: Node) -> Option<u32> {
    let scopes = src.scopes();
    scopes.id_of(node).filter(|decl| scopes.decl(*decl).kind != DeclKind::Global)
}

fn name_range(node: Node) -> Option<(usize, usize)> {
    node.token(T::Id).map(|id| id.range())
}

fn local_ranges(src: &Source, decl: u32) -> (String, Vec<(usize, usize)>) {
    let (root, scopes) = (src.root(), src.scopes());
    let declared = scopes.decl(decl);
    let name = root.tree().tokens().text(root.source(), declared.name as usize).to_owned();
    let uses = scopes.uses(decl).filter_map(|id| name_range(root.tree().node(root.source(), id)));
    let documents = |reference: &Node| documented_param(src, *reference).is_some_and(|p| p.id() == declared.node);
    let tags = src.docs().descendants().filter(|node| node.is(N::DocParamNameRef)).filter(documents);
    (name, uses.chain(tags.filter_map(name_range)).collect())
}

/// A label and every `goto` that jumps to it, a label written after its `goto` included.
fn label_ranges<'a>(root: Node<'a>, label: Node<'a>) -> Option<(String, Vec<(usize, usize)>)> {
    let id = label.token(T::Id)?;
    let name = id.text();
    let within = label.ancestor(N::FuncBody).unwrap_or(root);
    let jumps = within.descendants().filter(|node| node.is(N::GotoStat)).filter(|goto| {
        goto.token(T::Id).is_some_and(|id| id.text() == name) && definition::label(*goto, name) == Some(label)
    });
    Some((name.to_owned(), std::iter::once(id.range()).chain(jumps.filter_map(name_range)).collect()))
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
