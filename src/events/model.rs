//! What the event analysis talks about: keys (a network name or a tuple of
//! helper arguments), the functions registering / triggering / aliasing
//! events (the runtime API and the helpers found in the project), and the
//! sites of the project calling them.

use crate::project::Side;
use openidea_plugin::Range;
use std::collections::BTreeSet;
use std::rc::Rc;

/// Source id of the enclosing function's `...` in parameter sets.
pub const VARARG: i32 = -1;

/// Parameters (0-based, `self` excluded) and [`VARARG`] a value is computed from.
pub type Sources = BTreeSet<i32>;

/// A piece of a key component: literal text or a parameter.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Seg {
    Lit(String),
    Param(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Registers a handler.
    Handler,
    Trigger,
    /// Makes the event safe for the network without handling it (`RegisterNetEvent(name)`).
    Register,
    /// Declares two keys the same event (`RegisterEventAlias(wire, eType, eName)`).
    Alias,
}

/// Where the handlers of a trigger run: a set of [`Dirs::LOCAL`], [`Dirs::SERVER`], [`Dirs::CLIENT`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dirs(u8);

impl Dirs {
    pub const NONE: Dirs = Dirs(0);
    /// The caller's side (`TriggerEvent`).
    pub const LOCAL: Dirs = Dirs(1);
    pub const SERVER: Dirs = Dirs(2);
    pub const CLIENT: Dirs = Dirs(4);

    pub fn has(self, d: Dirs) -> bool {
        self.0 & d.0 != 0
    }

    pub fn with(self, d: Dirs) -> Dirs {
        Dirs(self.0 | d.0)
    }

    /// The side a trigger must be called from.
    pub fn required_side(self) -> Option<Side> {
        match self {
            Dirs::SERVER => Some(Side::Client),
            Dirs::CLIENT => Some(Side::Server),
            _ => None,
        }
    }
}

/// Where an event function comes from, by increasing priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Standard,
    Detected,
    Annotation,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    /// `function Name()` / `Name = function()`.
    Global,
    /// `function A.b()` / `A.b = function()` / `A = { b = function() }`.
    Field,
    /// `function A:b()`.
    Method,
    /// `local function f()` / `local f = function()` at the top level.
    Local,
}

impl FnKind {
    /// The kind a path names (helpers of the settings).
    pub fn of_path(path: &str) -> Self {
        if path.contains(':') {
            Self::Method
        } else if path.contains('.') {
            Self::Field
        } else {
            Self::Global
        }
    }
}

/// How the arguments of a call form the key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeySpec {
    /// `false`: one component, the network name; `true`: a tuple of helper arguments.
    pub logical: bool,
    pub components: Vec<Vec<Seg>>,
}

impl KeySpec {
    /// The network name is argument `index`.
    pub fn wire(index: usize) -> Self {
        Self { logical: false, components: vec![vec![Seg::Param(index)]] }
    }

    /// A tuple of arguments.
    pub fn logical(indices: &[usize]) -> Self {
        Self { logical: true, components: indices.iter().map(|&i| vec![Seg::Param(i)]).collect() }
    }
}

/// A network event name, or a tuple of helper arguments the helper computes the name from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventKey {
    pub parts: Vec<String>,
    pub logical: bool,
}

impl EventKey {
    pub fn wire(name: &str) -> Self {
        Self { parts: vec![name.to_owned()], logical: false }
    }

    #[cfg(test)]
    pub fn tuple(parts: &[&str]) -> Self {
        Self { parts: parts.iter().map(|p| (*p).to_owned()).collect(), logical: true }
    }

    /// `shop:buy`, `(Gamemode, RequestModel)`.
    pub fn display(&self) -> String {
        if self.logical { format!("({})", self.parts.join(", ")) } else { self.parts[0].clone() }
    }

    /// `'shop:buy'`, `('Gamemode', 'SpawnPlayer')`.
    pub fn quoted(&self) -> String {
        if self.logical {
            format!("({})", self.parts.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", "))
        } else {
            format!("'{}'", self.parts[0])
        }
    }

    /// The part before the first `:` of a name, the first part of a tuple.
    pub fn namespace(&self) -> &str {
        if self.logical { &self.parts[0] } else { self.parts[0].split_once(':').map_or("", |(ns, _)| ns) }
    }

    /// Sorting order: display name, case-insensitive first.
    pub fn order(&self) -> (String, String) {
        let d = self.display();
        (d.to_lowercase(), d)
    }

    /// Events of the runtime and of the standard resources (`__cfx_*`, the catalog).
    pub fn is_builtin(&self) -> bool {
        !self.logical && (self.parts[0].starts_with("__cfx_") || crate::docs::data::event(&self.parts[0]).is_some())
    }
}

/// A runtime function or a helper. Parameter indices are 0-based, `self` excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventFn {
    pub path: String,
    pub role: Role,
    pub key: KeySpec,
    pub origin: Origin,
    /// Handler: parameter receiving the handler function.
    pub callback: Option<usize>,
    /// Handler: without the callback the call only registers (`RegisterNetEvent(name)`).
    pub callback_optional: bool,
    /// Handler: values the helper passes to the handler before the payload.
    pub injected: usize,
    /// Handler: registers the event as safe for the network.
    pub net: bool,
    pub directions: Dirs,
    /// Trigger: parameter of the target player.
    pub target: Option<usize>,
    /// Trigger: first payload parameter.
    pub payload_start: Option<usize>,
    /// Trigger: number of payload values when always the same.
    pub payload_fixed: Option<usize>,
    /// Alias: parameter holding the network name.
    pub wire: Option<usize>,
    /// Definition file and offset of the name.
    pub file: Option<Rc<str>>,
    pub offset: Option<u32>,
    pub kind: FnKind,
    pub defined_side: Side,
    /// A project function replacing a runtime function.
    pub redefinition: bool,
    pub required_side: Option<Side>,
    /// Named parameters, `None` when unknown (settings).
    pub param_count: Option<usize>,
    pub vararg: bool,
}

impl EventFn {
    /// A function with nothing but its path, role, key and origin.
    pub fn base(path: &str, role: Role, key: KeySpec, origin: Origin) -> Self {
        Self {
            path: path.to_owned(),
            role,
            key,
            origin,
            callback: None,
            callback_optional: false,
            injected: 0,
            net: false,
            directions: Dirs::NONE,
            target: None,
            payload_start: None,
            payload_fixed: None,
            wire: None,
            file: None,
            offset: None,
            kind: FnKind::Global,
            defined_side: Side::Unknown,
            redefinition: false,
            required_side: None,
            param_count: None,
            vararg: false,
        }
    }

    /// An alias function of the settings: every argument but the network name is a key part.
    pub fn alias(path: &str) -> Self {
        let mut f = Self::base(path, Role::Alias, KeySpec { logical: true, components: Vec::new() }, Origin::Settings);
        f.wire = Some(0);
        f.kind = FnKind::of_path(path);
        f
    }

    pub fn simple_name(&self) -> &str {
        simple_name(&self.path)
    }

    pub fn is_method(&self) -> bool {
        self.kind == FnKind::Method
    }

    /// `Player` for `Player:TriggerEvent`.
    pub fn class_name(&self) -> Option<&str> {
        if self.is_method() { self.path.rsplit_once(':').map(|(c, _)| c) } else { None }
    }

    pub fn is_standard(&self) -> bool {
        self.origin == Origin::Standard
    }

    /// The role of a call passing `args` arguments.
    pub fn role_for(&self, args: usize) -> Role {
        match self.callback {
            Some(cb) if self.role == Role::Handler && self.callback_optional && args <= cb => Role::Register,
            _ => self.role,
        }
    }
}

/// `TriggerEvent` for `player:TriggerEvent` and `MyHelper.TriggerEvent`.
pub fn simple_name(path: &str) -> &str {
    path.rfind(['.', ':']).map_or(path, |i| &path[i + 1..])
}

/// The runtime functions naming an event in their first argument (`RemoveEventHandler` takes a handle: not one).
pub const STANDARD_NAMES: [&str; 8] = [
    "AddEventHandler",
    "RegisterNetEvent",
    "RegisterServerEvent",
    "TriggerEvent",
    "TriggerServerEvent",
    "TriggerLatentServerEvent",
    "TriggerClientEvent",
    "TriggerLatentClientEvent",
];

/// The functions of [`STANDARD_NAMES`], in that order.
pub fn standard() -> Vec<EventFn> {
    let handler = |name: &str, net: bool, optional: bool, required: Option<Side>| {
        let mut f = EventFn::base(name, Role::Handler, KeySpec::wire(0), Origin::Standard);
        f.callback = Some(1);
        f.callback_optional = optional;
        f.net = net;
        f.required_side = required;
        f
    };
    let trigger = |name: &str, dirs: Dirs, payload: usize, target: Option<usize>| {
        let mut f = EventFn::base(name, Role::Trigger, KeySpec::wire(0), Origin::Standard);
        f.directions = dirs;
        f.target = target;
        f.payload_start = Some(payload);
        f.required_side = dirs.required_side();
        f
    };
    vec![
        handler("AddEventHandler", false, false, None),
        handler("RegisterNetEvent", true, true, None),
        handler("RegisterServerEvent", true, true, Some(Side::Server)),
        trigger("TriggerEvent", Dirs::LOCAL, 1, None),
        trigger("TriggerServerEvent", Dirs::SERVER, 1, None),
        trigger("TriggerLatentServerEvent", Dirs::SERVER, 2, None),
        trigger("TriggerClientEvent", Dirs::CLIENT, 2, Some(1)),
        trigger("TriggerLatentClientEvent", Dirs::CLIENT, 3, Some(1)),
    ]
}

pub fn is_standard_name(name: &str) -> bool {
    STANDARD_NAMES.contains(&name)
}

/// Client / server compatibility: everything is compatible except client with server.
pub fn compatible(a: Side, b: Side) -> bool {
    !matches!((a, b), (Side::Client, Side::Server) | (Side::Server, Side::Client))
}

/// `Client` / `Server` / `Shared`, `None` for the other sides.
pub fn side_text(side: Side) -> Option<&'static str> {
    match side {
        Side::Client | Side::Server | Side::Shared => Some(side.label()),
        _ => None,
    }
}

/// A token of a file: byte span and editor range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub range: Range,
}

impl Span {
    pub fn of(src: &crate::lua::Source, start: usize, end: usize) -> Self {
        Self { start: start as u32, end: end as u32, range: src.lines.range(&src.text, start, end) }
    }
}

/// A key component written as a string literal at a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// The literal, quotes included.
    pub span: Span,
    /// The literal is the whole component (no text added by a helper).
    pub whole: bool,
}

/// Where a file runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub side: Side,
    /// Directory of the enclosing resource.
    pub resource: Option<Rc<str>>,
}

/// A call registering, triggering or aliasing an event whose key is known statically.
#[derive(Clone, Debug)]
pub struct Site {
    pub file: Rc<str>,
    pub role: Role,
    /// Shared by the sites of the same key.
    pub key: Rc<EventKey>,
    /// For each key component, the literal written at the call.
    pub occurrences: Vec<Option<Occurrence>>,
    pub call_offset: u32,
    /// 0-based line of the call.
    pub line: u32,
    /// The callee's last name.
    pub callee_span: Span,
    pub function: Rc<EventFn>,
    pub net: bool,
    pub directions: Dirs,
    /// Handler: values the callback takes.
    pub callback_arity: Option<usize>,
    /// Trigger: values sent.
    pub payload_arity: Option<usize>,
    pub info: FileInfo,
}

impl Site {
    pub fn side(&self) -> Side {
        self.info.side
    }

    /// The element marking the site: the first key literal, else the callee's last name.
    pub fn anchor(&self) -> Span {
        self.occurrences.iter().flatten().next().map_or(self.callee_span, |o| o.span)
    }

    /// Same call (an alias call has two sites).
    pub fn same_call(&self, other: &Site) -> bool {
        self.file == other.file && self.call_offset == other.call_offset
    }
}

/// A call whose key is computed at runtime: it may stand for any key matching its known parts.
#[derive(Clone, Debug)]
pub struct DynamicSite {
    pub file: Rc<str>,
    pub role: Role,
    pub logical: bool,
    /// Known components, `None` for the computed ones.
    pub parts: Vec<Option<String>>,
    /// Known start of the first computed component.
    pub prefix: String,
    pub net: bool,
    pub info: FileInfo,
}

impl DynamicSite {
    pub fn matches(&self, key: &EventKey) -> bool {
        if key.logical != self.logical || key.parts.len() != self.parts.len() {
            return false;
        }
        let mut prefix_checked = false;
        for (part, known) in key.parts.iter().zip(&self.parts) {
            match known {
                Some(k) if k != part => return false,
                Some(_) => {}
                None if !prefix_checked => {
                    if !part.starts_with(&self.prefix) {
                        return false;
                    }
                    prefix_checked = true;
                }
                None => {}
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let k = EventKey::tuple(&["Gamemode", "SpawnPlayer"]);
        assert_eq!(k.display(), "(Gamemode, SpawnPlayer)");
        assert_eq!(k.quoted(), "('Gamemode', 'SpawnPlayer')");
        assert_eq!(k.namespace(), "Gamemode");
        let w = EventKey::wire("shop:buy");
        assert_eq!((w.display().as_str(), w.quoted().as_str(), w.namespace()), ("shop:buy", "'shop:buy'", "shop"));
        assert_eq!(EventKey::wire("noNamespace").namespace(), "");
        assert!(EventKey::wire("onResourceStart").is_builtin());
        assert!(EventKey::wire("__cfx_nui:close").is_builtin());
        assert!(!EventKey::tuple(&["onResourceStart"]).is_builtin());
        assert_eq!(simple_name("Player:TriggerEvent"), "TriggerEvent");
        assert_eq!(simple_name("a.b.c"), "c");
    }

    #[test]
    fn standard_api() {
        let all = standard();
        let f = |name: &str| all.iter().find(|f| f.path == name);
        let reg = f("RegisterNetEvent").unwrap();
        assert_eq!(reg.role_for(1), Role::Register);
        assert_eq!(reg.role_for(2), Role::Handler);
        assert_eq!(f("TriggerClientEvent").unwrap().required_side, Some(Side::Server));
        assert_eq!(f("TriggerServerEvent").unwrap().required_side, Some(Side::Client));
        assert!(!is_standard_name("RemoveEventHandler"));
        assert_eq!(all.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), STANDARD_NAMES);
    }

    #[test]
    fn dynamic_matching() {
        let info = FileInfo { side: Side::Server, resource: None };
        let d = DynamicSite {
            file: "/p/a.lua".into(),
            role: Role::Handler,
            logical: false,
            parts: vec![None],
            prefix: "dyn:".into(),
            net: true,
            info,
        };
        assert!(d.matches(&EventKey::wire("dyn:open")));
        assert!(!d.matches(&EventKey::wire("other")));
        assert!(!d.matches(&EventKey::tuple(&["dyn:open"])));
    }
}
