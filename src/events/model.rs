use crate::project::Side;
use openidea_plugin::Range;
use std::collections::BTreeSet;
use std::rc::Rc;

pub const VARARG: i32 = -1;

pub type Sources = BTreeSet<i32>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Seg {
    Lit(String),
    Param(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Handler,
    Trigger,
    Register,
    Alias,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dirs(u8);

impl Dirs {
    pub const NONE: Dirs = Dirs(0);
    pub const LOCAL: Dirs = Dirs(1);
    pub const SERVER: Dirs = Dirs(2);
    pub const CLIENT: Dirs = Dirs(4);

    pub fn has(self, d: Dirs) -> bool {
        self.0 & d.0 != 0
    }

    pub fn with(self, d: Dirs) -> Dirs {
        Dirs(self.0 | d.0)
    }

    pub fn required_side(self) -> Option<Side> {
        match self {
            Dirs::SERVER => Some(Side::Client),
            Dirs::CLIENT => Some(Side::Server),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Standard,
    Detected,
    Annotation,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    Global,
    Field,
    Method,
    Local,
}

impl FnKind {
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeySpec {
    pub logical: bool,
    pub components: Vec<Vec<Seg>>,
}

impl KeySpec {
    pub fn wire(index: usize) -> Self {
        Self { logical: false, components: vec![vec![Seg::Param(index)]] }
    }

    pub fn logical(indices: &[usize]) -> Self {
        Self { logical: true, components: indices.iter().map(|&i| vec![Seg::Param(i)]).collect() }
    }
}

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

    pub fn display(&self) -> String {
        if self.logical { format!("({})", self.parts.join(", ")) } else { self.parts[0].clone() }
    }

    pub fn quoted(&self) -> String {
        if self.logical {
            format!("({})", self.parts.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", "))
        } else {
            format!("'{}'", self.parts[0])
        }
    }

    pub fn namespace(&self) -> &str {
        if self.logical { &self.parts[0] } else { self.parts[0].split_once(':').map_or("", |(ns, _)| ns) }
    }

    pub fn order(&self) -> (String, String) {
        let d = self.display();
        (d.to_lowercase(), d)
    }

    pub fn is_builtin(&self) -> bool {
        !self.logical && (self.parts[0].starts_with("__cfx_") || crate::docs::data::event(&self.parts[0]).is_some())
    }

    pub fn is_nui_callback(&self) -> bool {
        !self.logical && self.parts[0].starts_with(crate::nui::NUI_EVENT_PREFIX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventFn {
    pub path: String,
    pub role: Role,
    pub key: KeySpec,
    pub origin: Origin,
    pub callback: Option<usize>,
    pub callback_optional: bool,
    pub injected: usize,
    pub net: bool,
    pub directions: Dirs,
    pub target: Option<usize>,
    pub payload_start: Option<usize>,
    pub payload_fixed: Option<usize>,
    pub wire: Option<usize>,
    pub file: Option<Rc<str>>,
    /// Which definition of `path` in `file`, from 0: an edit above it leaves the function the same.
    pub nth: u32,
    pub kind: FnKind,
    pub defined_side: Side,
    pub redefinition: bool,
    pub required_side: Option<Side>,
    pub param_count: Option<usize>,
    pub vararg: bool,
}

impl EventFn {
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
            nth: 0,
            kind: FnKind::Global,
            defined_side: Side::Unknown,
            redefinition: false,
            required_side: None,
            param_count: None,
            vararg: false,
        }
    }

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

    pub fn class_name(&self) -> Option<&str> {
        if self.is_method() { self.path.rsplit_once(':').map(|(c, _)| c) } else { None }
    }

    pub fn is_standard(&self) -> bool {
        self.origin == Origin::Standard
    }

    pub fn role_for(&self, args: usize) -> Role {
        match self.callback {
            Some(cb) if self.role == Role::Handler && self.callback_optional && args <= cb => Role::Register,
            _ => self.role,
        }
    }
}

pub fn simple_name(path: &str) -> &str {
    path.rfind(['.', ':']).map_or(path, |i| &path[i + 1..])
}

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

pub fn compatible(a: Side, b: Side) -> bool {
    !matches!((a, b), (Side::Client, Side::Server) | (Side::Server, Side::Client))
}

pub fn side_text(side: Side) -> Option<&'static str> {
    match side {
        Side::Client | Side::Server | Side::Shared => Some(side.label()),
        _ => None,
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub span: Span,
    pub whole: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub side: Side,
    pub resource: Option<Rc<str>>,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub file: Rc<str>,
    pub role: Role,
    pub key: Rc<EventKey>,
    pub occurrences: Vec<Option<Occurrence>>,
    pub call_offset: u32,
    pub line: u32,
    pub callee_span: Span,
    pub function: Rc<EventFn>,
    pub net: bool,
    pub directions: Dirs,
    pub callback_arity: Option<usize>,
    pub payload_arity: Option<usize>,
    pub info: FileInfo,
}

impl Site {
    pub fn side(&self) -> Side {
        self.info.side
    }

    pub fn anchor(&self) -> Span {
        self.occurrences.iter().flatten().next().map_or(self.callee_span, |o| o.span)
    }

    pub fn same_call(&self, other: &Site) -> bool {
        self.file == other.file && self.call_offset == other.call_offset
    }
}

#[derive(Clone, Debug)]
pub struct DynamicSite {
    pub file: Rc<str>,
    pub role: Role,
    pub logical: bool,
    pub parts: Vec<Option<String>>,
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
