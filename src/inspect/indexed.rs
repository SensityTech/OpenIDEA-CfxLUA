//! The inspections of the JetBrains plugin that read what the project declares: a class declared twice, a
//! deprecated name, and in comments a type or a parameter nothing declares (`DuplicateClassDeclaration`,
//! `LuaDeprecationInspection`, `inspection/doc/`).

use super::{DEPRECATED, DUPLICATE_CLASS, Inspection, UNRESOLVED_PARAM, UNRESOLVED_TYPE};
use crate::editor::target;
use crate::feature::{Fix, Problem};
use crate::lua::Source;
use crate::lua::doc::tags::Comment;
use crate::lua::scope::DeclKind;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::{ELLIPSIS, FileId, Ty, psi};
use openidea_plugin::Severity;

/// What the inspections `wanted` keeps find in `src`, the file `file`, in the order of the text. `shown` is how
/// the path of another file is written in a message.
pub fn problems(
    cx: &Cx,
    file: FileId,
    src: &Source,
    wanted: &dyn Fn(Inspection) -> bool,
    shown: &dyn Fn(&str) -> String,
) -> Vec<Problem> {
    let mut out = Vec::new();
    if wanted(DUPLICATE_CLASS) {
        duplicate_classes(cx, file, src, shown, &mut out);
    }
    if wanted(UNRESOLVED_TYPE) {
        unresolved_types(cx, src, &mut out);
    }
    if wanted(UNRESOLVED_PARAM) {
        unresolved_params(src, &mut out);
    }
    if wanted(DEPRECATED) {
        deprecated_names(cx, file, src, &mut out);
    }
    out.sort_by_key(|problem| problem.start);
    out
}

/// A `---@class` another one of the project already declares (`DuplicateClassDeclaration`).
fn duplicate_classes(cx: &Cx, file: FileId, src: &Source, shown: &dyn Fn(&str) -> String, out: &mut Vec<Problem>) {
    let Some(declared) = cx.index.file(file) else { return };
    let docs = src.docs();
    for class in &declared.classes {
        if class.node as usize >= docs.tree().len() {
            continue;
        }
        let tag = docs.tree().node(docs.source(), class.node);
        let Some(name) = tag.token(T::Id).filter(|name| tag.is(N::DocTagClass) && name.text() == &*class.name) else {
            continue;
        };
        for (other, again) in cx.index.classes_named(&class.name) {
            if (other == file && again.node == class.node) || !cx.files.visible(other) {
                continue;
            }
            let message = match cx.files.path(other) {
                Some(path) => format!("Duplicate class found in the file: {}", shown(&path)),
                None => "Duplicate class found in a bundled library".to_owned(),
            };
            out.push(Problem::new(DUPLICATE_CLASS.code, Severity::Error, name.start(), name.end(), message));
        }
    }
}

/// A type of a comment that names no class, no alias and no generic parameter (`UnresolvedClassInspection`).
fn unresolved_types(cx: &Cx, src: &Source, out: &mut Vec<Problem>) {
    for ty in src.docs().descendants().filter(|node| node.is(N::DocGeneralTy)) {
        let Some(reference) = ty.child(N::DocClassNameRef) else { continue };
        let name = reference.code().trim_matches('`');
        let known = Ty::builtin(name).is_some() || cx.index.class(name).is_some() || cx.index.alias(name).is_some();
        if name.is_empty() || known || target::is_generic(reference) {
            continue;
        }
        let (start, end) = ty.code_range();
        let message = format!("Unresolved type \"{name}\"");
        out.push(Problem::new(UNRESOLVED_TYPE.code, Severity::Error, start, end, message));
    }
}

/// Whether a `@param ...` of the comment bound to `owner` documents the variable arguments of a function of it.
fn has_varargs(owner: Node) -> bool {
    std::iter::once(owner).chain(owner.descendants()).any(|node| psi::is_function(node) && psi::has_varargs(node))
}

/// The bytes a tag leaves with: its line when nothing else is written on it.
fn line_of(src: &Source, tag: Node) -> (usize, usize) {
    let (start, end) = tag.code_range();
    let text = src.text.as_str();
    let line = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let next = text[end..].find('\n').map_or(text.len(), |at| end + at + 1);
    // Before the name of the tag: the dashes of the comment and its `@`.
    let opened = text[line..start].trim().trim_start_matches('-').trim_start() == "@";
    if opened && text[end..next].trim().is_empty() { (line, next) } else { (start, end) }
}

/// A `@param name` that names no parameter of what its comment documents
/// (`UnresolvedSymbolInEmmyDocInspection`).
fn unresolved_params(src: &Source, out: &mut Vec<Problem>) {
    for tag in src.docs().descendants().filter(|node| node.is(N::DocTagParam)) {
        let Some(reference) = tag.child(N::DocParamNameRef) else { continue };
        let name = reference.code();
        let owner = tag.ancestor(N::DocComment).and_then(|comment| src.documented_by(Comment(comment)));
        let resolved = match owner {
            Some(owner) if name == ELLIPSIS => has_varargs(owner),
            _ => target::documented_param(src, reference).is_some(),
        };
        if resolved {
            continue;
        }
        let (start, end) = reference.code_range();
        let (from, to) = line_of(src, tag);
        let fix = Fix::new(format!("Remove '{name}'")).replace(from, to, "");
        let message = format!("Cant resolve symbol '{name}'");
        out.push(Problem::new(UNRESOLVED_PARAM.code, Severity::Info, start, end, message).fix(fix));
    }
}

/// The names that stand for something marked `@deprecated` (`LuaDeprecationInspection`). A name is only
/// resolved when something deprecated bears it in the project.
fn deprecated_names(cx: &Cx, file: FileId, src: &Source, out: &mut Vec<Problem>) {
    let scopes = src.scopes();
    let found = |start: usize, end: usize, name: &str| {
        Problem::new(DEPRECATED.code, Severity::Hint, start, end, format!("'{name}' is deprecated")).deprecated()
    };
    for node in src.root().descendants() {
        let local = |node: Node| scopes.resolve(node).is_some_and(|decl| decl.kind != DeclKind::Global);
        let name = match node.kind() {
            N::NameExpr if !local(node) => node.token(T::Id),
            N::IndexExpr => node.token(T::Id),
            _ => None,
        };
        let Some(name) = name.filter(|name| cx.index.has_deprecated(name.text())) else { continue };
        if target::named(cx, Psi { file, src, node }).is_some_and(|target| target.is_deprecated(cx)) {
            out.push(found(name.start(), name.end(), name.text()));
        }
    }
    for reference in src.docs().descendants().filter(|node| node.is(N::DocClassNameRef)) {
        let name = reference.code().trim_matches('`');
        if !cx.index.has_deprecated(name) {
            continue;
        }
        if target::type_named(cx, reference).is_some_and(|target| target.is_deprecated(cx)) {
            let (start, end) = reference.code_range();
            out.push(found(start, end, name));
        }
    }
}
