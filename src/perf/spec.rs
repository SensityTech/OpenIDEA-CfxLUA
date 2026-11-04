#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Work {
    Pure,
    Query,
    Action,
    Effect,
    PerFrame,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Yield {
    None,
    Wait,
    Await,
    Maybe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spam {
    Log,
    Serialization,
    NetworkToServer,
    NetworkToClients,
    Storage,
    Http,
    Database,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    Thread,
    Timeout,
    Handler,
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub work: Work,
    pub yields: Yield,
    pub spam: Option<Spam>,
    pub registration: Option<Registration>,
    pub function_arg: usize,
    pub name_arg: usize,
    pub exits: bool,
}

impl Spec {
    pub const fn of(work: Work) -> Self {
        Self {
            work,
            yields: Yield::None,
            spam: None,
            registration: None,
            function_arg: usize::MAX,
            name_arg: usize::MAX,
            exits: false,
        }
    }

    pub const PURE: Spec = Spec::of(Work::Pure);
    pub const QUERY: Spec = Spec::of(Work::Query);
    pub const ACTION: Spec = Spec::of(Work::Action);
    pub const EFFECT: Spec = Spec::of(Work::Effect);
    pub const PER_FRAME: Spec = Spec::of(Work::PerFrame);
    pub const UNKNOWN: Spec = Spec { yields: Yield::Maybe, ..Spec::of(Work::Unknown) };

    const fn yielding(self, yields: Yield) -> Self {
        Self { yields, ..self }
    }

    const fn spamming(self, spam: Spam) -> Self {
        Self { spam: Some(spam), ..self }
    }

    const fn registering(self, registration: Registration, function_arg: usize, name_arg: usize) -> Self {
        Self { registration: Some(registration), function_arg, name_arg, ..self }
    }
}

const RUNTIME: &[(&[&str], Spec)] = &[
    (&["Wait", "Citizen.Wait"], Spec::PURE.yielding(Yield::Wait)),
    (&["Citizen.Await", "coroutine.yield", "PerformHttpRequestAwait"], Spec::PURE.yielding(Yield::Await)),
    (
        &[
            "type",
            "tostring",
            "tonumber",
            "select",
            "next",
            "pairs",
            "ipairs",
            "rawget",
            "rawequal",
            "rawlen",
            "unpack",
            "assert",
            "setmetatable",
            "getmetatable",
            "joaat",
            "each",
            "scrub",
            "vector2",
            "vector3",
            "vector4",
            "vec",
            "vec2",
            "vec3",
            "vec4",
            "vector",
            "vector1",
            "vec1",
            "quat",
            "mat3",
            "mat4",
            "os.time",
            "os.clock",
            "os.date",
            "os.difftime",
            "os.microtime",
            "os.nanotime",
            "os.deltatime",
            "os.rdtsc",
            "os.rdtscp",
            "coroutine.create",
            "coroutine.wrap",
            "coroutine.running",
            "coroutine.status",
            "coroutine.isyieldable",
            "promise.new",
            "promise.all",
            "promise.allSettled",
            "promise.any",
            "promise.first",
            "promise.race",
            "promise.resolve",
            "promise.reject",
        ],
        Spec::PURE,
    ),
    (
        &[
            "rawset",
            "collectgarbage",
            "Citizen.ClearTimeout",
            "ClearTimeout",
            "RemoveEventHandler",
            "RemoveStateBagChangeHandler",
            "SendNUIMessage",
        ],
        Spec::ACTION,
    ),
    // Local handlers run right away and may do anything, per-frame work included.
    (&["TriggerEvent"], Spec::of(Work::Unknown)),
    (&["print", "Citizen.Trace", "RconPrint", "RconLog"], Spec::ACTION.spamming(Spam::Log)),
    (&["json.encode", "json.decode", "msgpack.pack", "msgpack.unpack"], Spec::PURE.spamming(Spam::Serialization)),
    (&["TriggerServerEvent", "TriggerLatentServerEvent"], Spec::ACTION.spamming(Spam::NetworkToServer)),
    (&["TriggerClientEvent", "TriggerLatentClientEvent"], Spec::ACTION.spamming(Spam::NetworkToClients)),
    (
        &["ESX.TriggerServerCallback", "QBCore.Functions.TriggerCallback", "lib.callback", "lib.callback.await"],
        Spec::UNKNOWN.spamming(Spam::NetworkToServer),
    ),
    (&["LoadResourceFile"], Spec::QUERY.spamming(Spam::Storage)),
    (
        &[
            "SaveResourceFile",
            "SetResourceKvp",
            "SetResourceKvpInt",
            "SetResourceKvpFloat",
            "SetResourceKvpNoSync",
            "SetResourceKvpIntNoSync",
            "SetResourceKvpFloatNoSync",
            "DeleteResourceKvp",
            "DeleteResourceKvpNoSync",
        ],
        Spec::EFFECT.spamming(Spam::Storage),
    ),
    (&["PerformHttpRequest"], Spec::ACTION.spamming(Spam::Http)),
    (
        &[
            "GetPlayers",
            "GetPlayerIdentifiers",
            "GetPlayerTokens",
            "GetPlayerEP",
            "Player",
            "Entity",
            "GetGameTimer",
            "GetCurrentResourceName",
            "GetInvokingResource",
            "IsDuplicityVersion",
            "PlayerPedId",
            "PlayerId",
        ],
        Spec::QUERY,
    ),
    (
        &[
            "Citizen.InvokeNative",
            "pcall",
            "xpcall",
            "require",
            "load",
            "loadstring",
            "dofile",
            "coroutine.resume",
            "exports",
        ],
        Spec::UNKNOWN,
    ),
    (&["error"], Spec { exits: true, ..Spec::ACTION }),
    (
        &["CreateThread", "Citizen.CreateThread", "CreateThreadNow", "Citizen.CreateThreadNow"],
        Spec::ACTION.registering(Registration::Thread, 0, usize::MAX),
    ),
    (&["SetTimeout", "Citizen.SetTimeout"], Spec::ACTION.registering(Registration::Timeout, 1, usize::MAX)),
    (
        &["AddEventHandler", "RegisterNetEvent", "RegisterServerEvent", "RegisterNUICallback"],
        Spec::ACTION.registering(Registration::Handler, 1, 0),
    ),
    (&["AddStateBagChangeHandler"], Spec::ACTION.registering(Registration::Handler, 2, 0)),
    (&["RegisterCommand"], Spec::ACTION.registering(Registration::Command, 1, 0)),
];

const PURE_LIBRARIES: &[&str] = &["math", "string", "table", "utf8", "bit32"];

pub fn runtime(path: &str) -> Option<Spec> {
    if let Some((_, spec)) = RUNTIME.iter().find(|(names, _)| names.contains(&path)) {
        return Some(*spec);
    }
    let (library, rest) = path.split_once('.')?;
    if PURE_LIBRARIES.contains(&library) && !rest.contains('.') {
        return Some(Spec::PURE);
    }
    (library == "MySQL").then_some(Spec::UNKNOWN.spamming(Spam::Database))
}

const PER_FRAME_PREFIXES: &[&str] = &[
    "Draw",
    "IsControl",
    "IsDisabledControl",
    "GetControl",
    "GetDisabledControl",
    "DisableControl",
    "DisableAllControl",
    "EnableControl",
    "EnableAllControl",
    "SetText",
    "BeginTextCommand",
    "EndTextCommand",
    "AddTextComponent",
    "SetDrawOrigin",
    "ClearDrawOrigin",
    "SetScriptGfx",
    "ResetScriptGfx",
    "HideHud",
    "ShowHud",
    "HideHelpText",
    "DisplayHelpText",
    "Disable",
    "SetPedResetFlag",
    "SetMouseCursor",
    "SetInputExclusive",
    "SetCam",
    "PointCam",
    "SetGameplayCam",
    "SetUiLayer",
];

const PER_FRAME_PARTS: &[&str] = &["ThisFrame", "ThisUpdate", "NextFrame", "Prompt"];

pub fn is_per_frame_name(name: &str) -> bool {
    PER_FRAME_PREFIXES.iter().any(|p| name.starts_with(p)) || PER_FRAME_PARTS.iter().any(|p| name.contains(p))
}

const QUERY_PREFIXES: &[&str] = &[
    "Get",
    "Is",
    "Has",
    "Does",
    "Are",
    "Can",
    "Was",
    "NetworkGet",
    "NetworkIs",
    "NetworkHas",
    "NetworkDoes",
    "NetworkAre",
    "NetworkCan",
    "Vdist",
    "Vmag",
];

pub fn is_query_name(name: &str) -> bool {
    name == "PlayerPedId"
        || name == "PlayerId"
        || QUERY_PREFIXES
            .iter()
            .any(|p| name.strip_prefix(p).is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_lowercase())))
}

const TRANSIENT_WORDS: &[&str] = &[
    "Control",
    "Just",
    "Pressed",
    "Released",
    "Shooting",
    "Shoot",
    "Shot",
    "Bullet",
    "Explosion",
    "Projectile",
    "Impact",
    "Collided",
    "Collision",
    "Damaged",
    "Damage",
    "Hit",
    "Melee",
    "Stealth",
    "Jumping",
    "Jump",
    "Climbing",
    "Vaulting",
    "Ragdoll",
    "Falling",
    "Aiming",
    "Aim",
    "Targetting",
    "Targeting",
    "Target",
    "Firing",
    "Reloading",
    "Input",
    "Mouse",
    "Cursor",
    "Keyboard",
    "Frame",
    "Scaleform",
    "Talking",
    "Speaking",
    "Voice",
    "Prompt",
    "Event",
    "Sound",
    "Audio",
    "Cam",
    "Camera",
    "Cutscene",
    "Help",
    "Text",
    "Hud",
    "Radar",
    "Minimap",
    "Ptfx",
    "Particle",
    "Anim",
    "Playing",
    "Task",
    "Phone",
    "Pause",
    "Menu",
    "Nui",
    "Weapon",
];

pub fn is_transient_query_name(name: &str) -> bool {
    words(name).iter().any(|w| TRANSIENT_WORDS.contains(w))
}

pub fn words(name: &str) -> Vec<&str> {
    let bytes = name.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'_' {
            if start < i {
                out.push(&name[start..i]);
            }
            start = i + 1;
        } else if i > start
            && bytes[i].is_ascii_uppercase()
            && (bytes[i - 1].is_ascii_lowercase() || bytes[i - 1].is_ascii_digit())
        {
            out.push(&name[start..i]);
            start = i;
        }
    }
    if start < bytes.len() {
        out.push(&name[start..]);
    }
    out
}

pub fn looks_like_native(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

pub fn is_per_frame_description(description: &str) -> bool {
    let text = description.to_ascii_lowercase();
    let words: Vec<(usize, &str)> = word_spans(&text);
    for (i, &(at, w)) in words.iter().enumerate() {
        match w {
            "continuously" | "ontick" => return true,
            "in" if words.get(i + 1).is_some_and(|n| n.1 == "a") && words.get(i + 2).is_some_and(|n| n.1 == "loop") => {
                // `in a loop` written with single spaces.
                let end = words[i + 2].0 + 4;
                if text[at..end] == *"in a loop" {
                    return true;
                }
            }
            "every" | "each" | "per" | "this" | "next" | "single" | "one" => {
                let rest = &text[at + w.len()..];
                if let Some(after) = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('-'))
                    && ["frame", "tick"].iter().any(|u| {
                        after
                            .strip_prefix(u)
                            .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
                    })
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn word_spans(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let word = c.is_alphanumeric() || c == '_';
        match (word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_table() {
        assert_eq!(runtime("Citizen.Wait").map(|s| s.yields), Some(Yield::Wait));
        assert_eq!(runtime("string.format"), Some(Spec::PURE));
        assert_eq!(runtime("string.a.b"), None);
        assert_eq!(runtime("MySQL.query.await").map(|s| s.spam), Some(Some(Spam::Database)));
        assert_eq!(runtime("AddStateBagChangeHandler").map(|s| s.function_arg), Some(2));
        assert!(runtime("error").is_some_and(|s| s.exits));
        assert_eq!(runtime("TriggerEvent").map(|s| (s.work, s.yields)), Some((Work::Unknown, Yield::None)));
        assert_eq!(runtime("GetEntityCoords"), None);
    }

    #[test]
    fn names() {
        assert!(is_per_frame_name("DrawMarker"));
        assert!(is_per_frame_name("HideHudComponentThisFrame"));
        assert!(is_per_frame_name("IsDisabledControlJustPressed"));
        assert!(!is_per_frame_name("GetEntityCoords"));
        assert!(is_query_name("GetEntityCoords"));
        assert!(is_query_name("IsPedInAnyVehicle"));
        assert!(is_query_name("Vdist2"));
        assert!(!is_query_name("Getaway"));
        assert!(!is_query_name("SetEntityCoords"));
        assert!(is_query_name("PlayerPedId"));
        assert_eq!(words("IsPedShooting"), ["Is", "Ped", "Shooting"]);
        assert_eq!(words("GET_ENTITY_COORDS"), ["GET", "ENTITY", "COORDS"]);
        assert_eq!(words("Vdist2Of"), ["Vdist2", "Of"]);
        assert!(is_transient_query_name("IsPedShooting"));
        assert!(!is_transient_query_name("GetEntityCoords"));
        assert!(looks_like_native("SetEntityHealth"));
        assert!(!looks_like_native("isNear"));
    }

    #[test]
    fn descriptions() {
        assert!(is_per_frame_description("Only returns true for one frame after the pulse."));
        assert!(is_per_frame_description("Needs to be called every-tick."));
        assert!(is_per_frame_description("Call it in a loop."));
        assert!(is_per_frame_description("Draws continuously"));
        assert!(!is_per_frame_description("Returns the frames of the animation."));
        assert!(!is_per_frame_description("Every frames"));
    }
}
