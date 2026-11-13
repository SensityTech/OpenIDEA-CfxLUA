use crate::lua::Source;
use crate::manifest::catalog::FX_VERSION_LINE;
use crate::manifest::edits::{Edit, add_directive, apply, remove_statement, replace_statement};
use crate::manifest::view::{Decl, View};

pub const TITLE: &str = "Convert to fxmanifest.lua";

/// The text of a `__resource.lua` once it becomes a `fxmanifest.lua`; the rename is the caller's.
pub fn convert(text: &str) -> String {
    let src = Source::new(text);
    let view = View::new(&src);
    let has_game = view.decls.iter().any(|d| d.metadata_key() == "game");
    let versions: Vec<&Decl> = view.named("resource_manifest_version").collect();
    let mut edits: Vec<Edit> = Vec::new();
    if let Some(first) = versions.first().filter(|d| d.statement) {
        edits.extend(versions[1..].iter().filter(|d| d.statement).map(|d| remove_statement(text, d)));
        edits.push(replace_statement(first, FX_VERSION_LINE));
    } else if !view.has("fx_version") {
        edits.push(add_directive(&src, &view, FX_VERSION_LINE, &[]));
    }
    let text = apply(text, &edits);
    if has_game {
        return text;
    }
    let src = Source::new(&text);
    let game = add_directive(&src, &View::new(&src), "game 'gta5'", &["fx_version"]);
    apply(&text, &[game])
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
