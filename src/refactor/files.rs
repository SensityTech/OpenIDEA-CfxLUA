//! A Lua file renamed or moved with the `require` calls that load it (`LuaRequireReference`,
//! `LuaMoveFileHandler`).

use crate::editor::completion::requires::module;
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::lua::tree::{N, Node};
use crate::manifest::edits::Edit;
use crate::project::FileKind;
use crate::text::{file_name, parent};
use crate::ty::psi;
use crate::workspace::Workspace;

/// A `require` called with a literal string, and where the path is written inside its quotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Require {
    pub path: String,
    pub start: usize,
    pub end: usize,
}

/// The calls of `src` to one of `loaders` whose string can be rewritten as it is: no escape in it.
pub fn requires(src: &Source, loaders: &[String]) -> Vec<Require> {
    let calls = src.root().descendants().filter(|node| node.is(N::CallExpr));
    calls.filter_map(|call| require(src, loaders, call)).collect()
}

fn require(src: &Source, loaders: &[String], call: Node) -> Option<Require> {
    let callee = psi::prefix(call).filter(|callee| callee.is(N::NameExpr))?;
    if !loaders.iter().any(|name| name == callee.code()) {
        return None;
    }
    let literal = psi::first_string_arg(call)?;
    let token = *src.toks.get(src.tok_of(literal.first_token()?.index()))?;
    let closed = match token.kind {
        Tok::Str { closed } => closed,
        Tok::LongStr => token.text(&src.text).ends_with(']'),
        _ => false,
    };
    let (start, end) = token.content_range(&src.text);
    let path = token.string_value(&src.text);
    (closed && src.text.get(start..end) == Some(path.as_str())).then_some(Require { path, start, end })
}

/// The file `require(written)` loads from the file `from`, by the rule of `Files::required`.
pub fn required(ws: &Workspace, from: &str, written: &str) -> Option<String> {
    View { ws, moved: None }.required(from, written)
}

/// The word every `require` of `file` writes: its name, or the name of its folder for an `init.lua`.
pub fn searched_word(file: &str) -> &str {
    let name = file_name(file);
    match name {
        "init.lua" => file_name(parent(file)),
        _ => name.strip_suffix(".lua").unwrap_or(name),
    }
}

/// The edits of `src`, the file `from`, once `file` is renamed `name` in its folder: the last segment of each
/// path that loads it becomes the new name.
pub fn renamed(ws: &Workspace, (from, src): (&str, &Source), loaders: &[String], file: &str, name: &str) -> Vec<Edit> {
    let stem = name.strip_suffix(".lua").unwrap_or(name);
    let by_folder = file_name(file) == "init.lua";
    rewritten(ws, (from, src), loaders, file, |written| {
        let cut = written.rfind(['.', '/']).map_or(0, |at| at + 1);
        if by_folder && &written[cut..] != "init" {
            return Some(format!("{written}{}{stem}", separator(written)));
        }
        Some(format!("{}{stem}", &written[..cut]))
    })
}

/// The edits of `src`, the file `from`, once `file` is moved to `to`: each path that loads it is written again
/// from the root it was written from (the resource, the folder of `from` or the project) when the file is
/// still under it, or else becomes the shortest one that loads it again from `from`.
pub fn moved(ws: &Workspace, (from, src): (&str, &Source), loaders: &[String], file: &str, to: &str) -> Vec<Edit> {
    let (before, after) = (View { ws, moved: None }, View { ws, moved: Some((file, to)) });
    let shortest = after.shortest(from, to);
    rewritten(ws, (from, src), loaders, file, |written| {
        let kept = before.written_from(from, written, file).and_then(|root| after.under(from, root, to));
        let path = kept.or_else(|| shortest.clone())?;
        Some(if separator(written) == '/' { path.replace('.', "/") } else { path })
    })
}

/// A Lua file name from what was typed: `.lua` is added when missing.
pub fn lua_file_name(typed: &str) -> Result<String, &'static str> {
    let name = typed.trim();
    if name.contains(['/', '\\']) {
        return Err("A file name cannot contain '/' or '\\'");
    }
    if name.strip_suffix(".lua").unwrap_or(name).is_empty() {
        return Err("Enter a file name");
    }
    Ok(if name.ends_with(".lua") { name.to_owned() } else { format!("{name}.lua") })
}

fn separator(written: &str) -> char {
    if written.contains('/') && !written.contains('.') { '/' } else { '.' }
}

fn rewritten(
    ws: &Workspace,
    (from, src): (&str, &Source),
    loaders: &[String],
    file: &str,
    rewrite: impl Fn(&str) -> Option<String>,
) -> Vec<Edit> {
    let view = View { ws, moved: None };
    let loading = requires(src, loaders).into_iter().filter(|r| view.required(from, &r.path).as_deref() == Some(file));
    let mut edits: Vec<Edit> = loading
        .filter_map(|r| rewrite(&r.path).filter(|text| *text != r.path).map(|text| (r.start, r.end, text)))
        .collect();
    edits.sort();
    edits
}

/// The Lua files of a workspace, `moved` being read as already moved.
#[derive(Clone, Copy)]
struct View<'a> {
    ws: &'a Workspace,
    moved: Option<(&'a str, &'a str)>,
}

impl View<'_> {
    fn lua(&self, path: &str) -> Option<String> {
        match self.moved {
            Some((_, to)) if path == to => return Some(to.to_owned()),
            Some((from, _)) if path == from => return None,
            _ => {}
        }
        let file = self.ws.file(path).filter(|file| file.kind == FileKind::Lua)?;
        Some(file.path.clone())
    }

    fn roots<'r>(&'r self, from: &'r str) -> impl Iterator<Item = &'r str> {
        let resource = self.ws.resource_of(from).map(|resource| resource.dir.as_str());
        [resource, Some(parent(from)), Some(self.ws.root.as_str())].into_iter().flatten()
    }

    fn rooted(&self, from: &str, name: &str) -> Option<String> {
        let mut files =
            self.roots(from).flat_map(|root| [format!("{root}/{name}.lua"), format!("{root}/{name}/init.lua")]);
        files.find_map(|file| self.lua(&file))
    }

    fn required(&self, from: &str, written: &str) -> Option<String> {
        let name = written.replace('.', "/");
        if let Some(found) = self.rooted(from, &name) {
            return Some(found);
        }
        let (plain, init) = (format!("/{name}.lua"), format!("/{name}/init.lua"));
        let named = |path: &&str| path.ends_with(&plain) || path.ends_with(&init);
        let left = |path: &&str| self.moved.is_none_or(|(moved, _)| *path != moved);
        let kept = self.ws.files().map(|file| file.path.as_str()).filter(left).find(named);
        let arrived = self.moved.map(|(_, to)| to).filter(named);
        self.lua(kept.into_iter().chain(arrived).min()?)
    }

    /// The root a path was written from: the first one under which it names `file`.
    fn written_from<'r>(&'r self, from: &'r str, written: &str, file: &str) -> Option<&'r str> {
        let name = written.replace('.', "/");
        let names = |root: &&str| {
            [format!("{root}/{name}.lua"), format!("{root}/{name}/init.lua")].iter().any(|path| path == file)
        };
        self.roots(from).find(names)
    }

    /// The path that names `target` from `root`, when it loads it from `from`.
    fn under(&self, from: &str, root: &str, target: &str) -> Option<String> {
        let name = target.strip_prefix(root)?.strip_prefix('/').and_then(module)?;
        (self.rooted(from, &name.replace('.', "/")).as_deref() == Some(target)).then_some(name)
    }

    fn shortest(&self, from: &str, target: &str) -> Option<String> {
        let mut names: Vec<String> = self.roots(from).filter_map(|root| self.under(from, root, target)).collect();
        names.sort_by_key(String::len);
        names.into_iter().next()
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
