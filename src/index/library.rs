//! What the engine knows without a project writing it: the standard library of Lua 5.4 and the Cfx runtime,
//! as annotated stubs (`std/` of the JetBrains plugin), and the natives, typed from their catalogs.

use crate::lua::Source;
use crate::lua::doc::tags::Comment;
use crate::lua::tree::N;
use crate::natives::Native;
use crate::runtime::stubs::{self, Part};
use crate::ty::doc::Reader;
use crate::ty::{FileId, Function, GLOBAL, Param, Signature, Ty};
use std::cell::RefCell;
use std::collections::HashMap;

/// One file of a library.
#[derive(Clone, Copy, Debug)]
pub struct Library {
    pub name: &'static str,
    pub text: &'static str,
    /// The part of the Cfx runtime it declares; the standard library of Lua has none, and is seen from
    /// everywhere.
    pub part: Option<Part>,
}

impl Library {
    /// The path the index knows the file by: no file of a project has one like it.
    pub fn path(&self) -> String {
        match self.part {
            Some(part) => format!("library:/cfx/{}/{}", part.label().to_lowercase(), self.name),
            None => format!("library:/lua/{}", self.name),
        }
    }
}

const STD: &[(&str, &str)] = &[
    ("builtin.lua", include_str!("../../data/std/builtin.lua")),
    ("global.lua", include_str!("../../data/std/global.lua")),
    ("coroutine.lua", include_str!("../../data/std/coroutine.lua")),
    ("debug.lua", include_str!("../../data/std/debug.lua")),
    ("io.lua", include_str!("../../data/std/io.lua")),
    ("math.lua", include_str!("../../data/std/math.lua")),
    ("os.lua", include_str!("../../data/std/os.lua")),
    ("package.lua", include_str!("../../data/std/package.lua")),
    ("string.lua", include_str!("../../data/std/string.lua")),
    ("table.lua", include_str!("../../data/std/table.lua")),
    ("utf8.lua", include_str!("../../data/std/utf8.lua")),
];

/// The files of the libraries: Lua first, then the Cfx runtime. What a manifest may write is no script's
/// business.
pub fn files() -> impl Iterator<Item = Library> {
    let std = STD.iter().map(|(name, text)| Library { name, text, part: None });
    let runtime = stubs::FILES.iter().filter(|(part, _, _)| *part != Part::Manifest);
    std.chain(runtime.map(|(part, name, text)| Library { name, text, part: Some(*part) }))
}

thread_local! {
    /// The types the catalogs write, each read once: a few dozen texts for thousands of natives.
    static WRITTEN: RefCell<HashMap<String, Ty>> = RefCell::default();
}

/// The type a catalog writes as a comment would: `integer[]|nil`, `fun(source: integer)`.
fn written(text: &str) -> Ty {
    if let Some(known) = WRITTEN.with(|written| written.borrow().get(text).cloned()) {
        return known;
    }
    let src = Source::new(&format!("---@type {text}"));
    let comment = src.docs().child(N::DocComment).map(Comment);
    let ty = comment.and_then(|comment| Reader::new(FileId::MAX).typed(comment)).unwrap_or(Ty::Unknown);
    WRITTEN.with(|written| written.borrow_mut().insert(text.to_owned(), ty.clone()));
    ty
}

/// The function a native is, as its catalog describes it.
pub fn native_type(native: &Native) -> Ty {
    let mut params = Vec::new();
    let mut vararg = None;
    for param in native.lua_params() {
        if param.is_vararg() {
            vararg = Some(Ty::Unknown);
            continue;
        }
        let optional = param.lua_type.split('|').any(|member| member == "nil");
        params.push(Param { name: param.name.as_str().into(), ty: written(&param.lua_type), optional });
    }
    let mut returned: Vec<Ty> = native.returns.iter().map(|value| written(&value.lua_type)).collect();
    let ret = match returned.len() {
        0 => Ty::Void,
        1 => returned.remove(0),
        _ => Ty::tuple(returned),
    };
    let main = Signature { vararg, ..Signature::new(false, params, ret) };
    Ty::function(Function { main, overloads: Vec::new(), flags: GLOBAL })
}
