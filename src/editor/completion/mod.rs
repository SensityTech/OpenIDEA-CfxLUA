//! What the type engine completes in a Lua file (`editor/completion/` of the JetBrains plugin).

mod docs;
mod expected;
mod members;
mod names;
mod overrides;
mod tables;

use crate::feature::Doc;
use crate::lua::Source;
use crate::lua::tree::N;
use crate::ty::context::{Cx, Psi};
use crate::ty::{FileId, Ret, Signature, display};
use openidea_plugin::providers::CompletionItem;
use std::collections::HashSet;

/// How many functions without a `@return` have their body read to show what they return.
const BODIES_READ: usize = 60;

/// The name written at the caret where none is typed, as the JetBrains platform does: the tree then has a
/// name to tell the place from.
const CARET_NAME: &str = "__caret";

/// Where the caret stands, told from the tokens alone: nothing is analysed before a place is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// After a `.` or a `:`, the code token of that index: a member is named.
    Member(usize),
    /// In a name that starts at that offset, or where one could start.
    Name(usize),
    /// Inside the quoted string that is the code token of that index.
    String(usize),
    /// In a documentation comment, in a word that starts at that offset or where one could start.
    Doc(usize),
}

pub fn place(src: &Source, at: usize) -> Option<Place> {
    if let Some(string) = src.quoted_at(at) {
        return Some(Place::String(string));
    }
    if let Some(start) = docs::start(src, at) {
        return Some(Place::Doc(start));
    }
    if src.in_literal(at) {
        return None;
    }
    if let Some(sign) = members::sign_before(src, at) {
        return Some(Place::Member(sign));
    }
    names::start(src, at).map(Place::Name)
}

/// The text a completion reads: the document as it is, or with a name written at the caret where no name is
/// typed yet.
pub struct Typed {
    written: Option<Source>,
}

impl Typed {
    pub fn new(src: &Source, at: usize, place: Place) -> Self {
        let written = matches!(place, Place::Name(start) | Place::Doc(start) if start == at).then(|| {
            let mut text = String::with_capacity(src.text.len() + CARET_NAME.len());
            text.push_str(&src.text[..at]);
            text.push_str(CARET_NAME);
            text.push_str(&src.text[at..]);
            Source::new(&text)
        });
        Self { written }
    }

    pub fn src<'a>(&'a self, document: &'a Source) -> &'a Source {
        self.written.as_ref().unwrap_or(document)
    }
}

/// The entries of one completion, as they are built.
struct Listing<'a, 'c> {
    cx: &'a Cx<'c>,
    doc: &'a Doc,
    bodies: usize,
    /// The label and the detail of each entry listed so far.
    listed: HashSet<(String, String)>,
    items: Vec<CompletionItem>,
}

impl Listing<'_, '_> {
    /// Adds an entry, unless one that reads the same is listed.
    fn push(&mut self, item: CompletionItem) {
        if self.listed.insert((item.label.clone(), item.detail.clone().unwrap_or_default())) {
            self.items.push(item);
        }
    }

    /// `(a, b, ...): returned`, as far as what a function returns costs little to tell.
    fn signature(&mut self, signature: &Signature, colon: bool) -> String {
        let mut params: Vec<String> = signature.args(None, colon).iter().map(|param| param.name.to_string()).collect();
        if signature.vararg.is_some() {
            params.push("...".to_owned());
        }
        let returned = match &signature.ret {
            Ret::Ty(ty) => Some(ty.clone()),
            Ret::Body(_) if self.bodies < BODIES_READ => {
                self.bodies += 1;
                Some(self.cx.ret(&signature.ret))
            }
            Ret::Body(_) => None,
        };
        let returned = returned.map_or(String::new(), |ty| format!(": {}", display(&ty)));
        format!("({}){returned}", params.join(", "))
    }
}

/// What the engine lists at `place`. `src` is the text of `Typed`, `at` the caret in the document.
pub fn complete(cx: &Cx, file: FileId, doc: &Doc, src: &Source, at: usize, place: Place) -> Vec<CompletionItem> {
    let mut listing = Listing { cx, doc, bodies: 0, listed: HashSet::new(), items: Vec::new() };
    let typed = |start: usize| &doc.src.text[start..at];
    match place {
        Place::Member(sign) => {
            let named = Psi { file, src, node: src.root().tree().node_at(&src.text, src.toks[sign].start) };
            match named.node.kind() {
                // `function Class:name`: a method is declared, not read.
                N::ClassMethodName => overrides::methods(&mut listing, named),
                // The CfxLua set field `{ .name }`.
                N::TableField => {
                    if let Some(table) = tables::keyed_by(named.node) {
                        tables::keys(&mut listing, named.at(table), named.node);
                    }
                }
                _ => members::members(&mut listing, file, src, sign, at),
            }
            names::words(&mut listing, src, typed(src.toks[sign].end.min(at)));
        }
        Place::Name(start) => {
            if names::names(&mut listing, file, src, start, typed(start)) {
                names::words(&mut listing, src, typed(start));
            }
        }
        Place::String(token) => expected::strings(&mut listing, file, src, token, at),
        Place::Doc(start) => docs::complete(&mut listing, src, start, at),
    }
    listing.items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    listing.items
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorFeature;
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use openidea_plugin::providers::{CompletionItem, CompletionKind};

    pub const MANIFEST: (&str, &str) = (
        "shop/fxmanifest.lua",
        "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\nclient_script 'client/*.lua'\n",
    );
    pub const MAIN: &str = "shop/client/main.lua";

    /// Everything the editor feature lists at the `<caret>` of the last file.
    pub fn with_words(files: &[(&str, &str)]) -> Vec<CompletionItem> {
        let fx = Fixture::new(files);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(files.last().expect("a file").0);
        feature.completion(&fx.cx(&doc), at, None)
    }

    /// What it lists there, the words of the file left out.
    pub fn completed(files: &[(&str, &str)]) -> Vec<CompletionItem> {
        with_words(files).into_iter().filter(|item| item.kind != Some(CompletionKind::Text)).collect()
    }

    /// Each entry as `label detail`.
    pub fn shown(files: &[(&str, &str)]) -> Vec<String> {
        let entry = |item: &CompletionItem| format!("{} {}", item.label, item.detail.as_deref().unwrap_or_default());
        completed(files).iter().map(entry).collect()
    }

    pub fn labels(files: &[(&str, &str)]) -> Vec<String> {
        completed(files).into_iter().map(|item| item.label).collect()
    }
}
