//! The paths of a manifest that follow a file renamed or moved (`CfxManifestPathReference`).

use super::catalog::{self, Role};
use super::edits::Edit;
use super::files::{Tree, is_glob, is_url, split_resource_path};
use super::view::View;
use crate::lua::Source;
use crate::text::{file_name, parent};
use crate::workspace::Workspace;

/// The edits of the manifest `manifest`, read in `src`, once the file `file` is `to`: each plain path that
/// names it, a glob staying as it is. A file renamed in its folder takes its new name after what is written
/// before it. A moved one is written again from the resource its path is read from, `@resource/` kept, and
/// stays as it is written when it left that resource.
pub fn followed(ws: &Workspace, manifest: &str, src: &Source, file: &str, to: &str) -> Vec<Edit> {
    let renamed = parent(file) == parent(to);
    let mut edits = Vec::new();
    for decl in &View::new(src).decls {
        for (arg, value) in decl.args() {
            let Role::Path(kind) = catalog::role(decl.name(), arg) else { continue };
            let written = value.text.as_str();
            if is_glob(written) || (kind.urls() && is_url(written)) {
                continue;
            }
            let other = split_resource_path(written);
            let bases: Vec<&str> = match other {
                Some(_) if !kind.other_resources() => continue,
                Some((name, _)) => ws.resources().filter(|r| r.name == name).map(|r| r.dir.as_str()).collect(),
                None => vec![parent(manifest)],
            };
            let path = other.map_or(written, |(_, path)| path);
            let named = |base: &&str| {
                // The files of a resource are listed only for a path that may name the file.
                file.strip_prefix(*base).is_some_and(|rest| rest.starts_with('/'))
                    && Tree::cached(ws, base)
                        .matches(path, kind)
                        .iter()
                        .any(|m| !m.dir && format!("{base}/{}", m.rel) == file)
            };
            if !bases.iter().any(named) {
                continue;
            }
            let new = if renamed {
                let cut = written.rfind('/').map_or(0, |at| at + 1);
                format!("{}{}", &written[..cut], file_name(to))
            } else {
                let from_base = bases.first().and_then(|base| to.strip_prefix(*base)?.strip_prefix('/'));
                let Some(relative) = from_base else { continue };
                match other {
                    Some((name, _)) => format!("@{name}/{relative}"),
                    None => relative.to_owned(),
                }
            };
            let Some(token) = src.toks.get(value.tok) else { continue };
            let (start, end) = token.content_range(&src.text);
            // A string with an escape is left alone: its text is not what is written.
            if src.text.get(start..end) == Some(written) && new != written {
                edits.push((start, end, new));
            }
        }
    }
    edits.sort();
    edits
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
