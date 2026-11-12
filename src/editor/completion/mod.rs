//! What the type engine completes in a Lua file (`editor/completion/` of the JetBrains plugin).

mod members;

pub use members::sign_before;

use crate::feature::Doc;
use crate::ty::context::Cx;
use crate::ty::{FileId, Ret, Signature, display};
use openidea_plugin::providers::CompletionItem;
use std::collections::HashSet;

/// How many functions without a `@return` have their body read to show what they return.
const BODIES_READ: usize = 60;

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

/// The members of what stands before the `.` or the `:` that precedes the caret.
pub fn complete(cx: &Cx, file: FileId, doc: &Doc, at: usize) -> Vec<CompletionItem> {
    let mut listing = Listing { cx, doc, bodies: 0, listed: HashSet::new(), items: Vec::new() };
    if let Some(sign) = members::sign_before(&doc.src, at) {
        members::members(&mut listing, file, &doc.src, sign, at);
    }
    listing.items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    listing.items
}

#[cfg(test)]
mod tests {
    use crate::editor::EditorFeature;
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use openidea_plugin::providers::CompletionItem;

    pub const MANIFEST: (&str, &str) = (
        "shop/fxmanifest.lua",
        "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\nclient_script 'client/*.lua'\n",
    );
    pub const MAIN: &str = "shop/client/main.lua";

    /// What the editor feature lists at the `<caret>` of the last file.
    pub fn completed(files: &[(&str, &str)]) -> Vec<CompletionItem> {
        let fx = Fixture::new(files);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(files.last().expect("a file").0);
        feature.completion(&fx.cx(&doc), at, None)
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
