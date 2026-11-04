//! Readers of the Cfx.re native databases: the docs.fivem.net JSON
//! (`natives.json`, `natives_cfx.json`), the RedM runtime DSL
//! (`natives_rdr3.lua`, read line by line and never executed) and alloc8or's
//! RDR3 NativeDB (descriptions only, joined to the DSL by hash).
//!
//! Strings borrow from the database text where they can: a catalog is built
//! from megabytes of JSON the first time a script needs it.

use super::convert::lua_name;
use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

/// Which database a [`RawNative`] was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// `natives.json`: GTA V game natives.
    Gta5Json,
    /// `natives_cfx.json`: CitizenFX natives (client / server / shared).
    CfxJson,
    /// `natives_rdr3.lua`: RDR3 game natives as the RedM runtime compiles them.
    Rdr3Dsl,
}

/// How the runtime marshals a type (case matters: `Object` is a handle, `object` a msgpack value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Bool,
    String,
    Hash,
    Any,
    Handle,
    Player,
    Vector3,
    Func,
    Object,
    Unknown,
}

/// Script handle types: integers at runtime, the name documents what they are.
const HANDLES: &[&str] = &[
    "Entity",
    "Ped",
    "Vehicle",
    "Object",
    "Cam",
    "Blip",
    "Pickup",
    "ScrHandle",
    "FireId",
    "Interior",
    "DecisionMaker",
    "CarGenerator",
    "Group",
    "Train",
    "Weapon",
    "Texture",
    "TextureDict",
    "CoverPoint",
    "Camera",
    "TaskSequence",
    "ColourIndex",
    "Sphere",
    "AnimScene",
    "ItemSet",
    "PersChar",
    "PopZone",
    "Prompt",
    "PropSet",
    "Volume",
];

impl Kind {
    pub fn of(base: &str) -> Self {
        match base {
            "int" | "uint" | "long" | "Void" => Self::Int,
            "float" => Self::Float,
            "BOOL" | "bool" => Self::Bool,
            "char" => Self::String,
            "Hash" => Self::Hash,
            "Any" => Self::Any,
            "Player" => Self::Player,
            "Vector3" => Self::Vector3,
            "func" => Self::Func,
            "object" => Self::Object,
            _ if HANDLES.contains(&base) => Self::Handle,
            _ => Self::Unknown,
        }
    }

    /// Passed and returned as an integer by the runtime.
    pub fn int_like(self) -> bool {
        matches!(self, Self::Int | Self::Hash | Self::Any | Self::Handle | Self::Player | Self::Unknown)
    }
}

/// A type token, normalised from the JSON (`Ped*`, `const char*`) or DSL (`PedPtr`, `charPtr`) spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeType {
    pub kind: Kind,
    /// A pointer argument (strings are not pointers).
    pub pointer: bool,
    pub vararg: bool,
    /// As documented: `Ped*`, `char*`, `BOOL`, `...`.
    pub display: String,
}

impl NativeType {
    fn new(base: &str, pointer: bool) -> Self {
        let display = if pointer { format!("{base}*") } else { base.to_owned() };
        Self { kind: Kind::of(base), pointer, vararg: false, display }
    }

    fn string() -> Self {
        Self { kind: Kind::String, pointer: false, vararg: false, display: "char*".into() }
    }

    /// A type of the JSON databases (`int`, `Ped*`, `const char*`; `""` / `...` for varargs).
    pub fn from_json(raw: &str) -> Self {
        let t = raw.trim();
        let t = t.strip_prefix("const ").unwrap_or(t).trim();
        if t.is_empty() || t == "..." {
            return Self { kind: Kind::Any, pointer: false, vararg: true, display: "...".into() };
        }
        let char_ptr =
            t.strip_prefix("char").and_then(|r| r.strip_suffix('*')).is_some_and(|mid| mid.bytes().all(is_ws));
        if char_ptr {
            return Self::string();
        }
        match t.strip_suffix('*') {
            Some(base) => Self::new(base.trim(), true),
            None => Self::new(t, false),
        }
    }

    /// A type token of the DSL; every `XPtr` is a pointer except `charPtr`.
    pub fn from_dsl(token: &str) -> Self {
        if token == "charPtr" {
            return Self::string();
        }
        match token.strip_suffix("Ptr").filter(|b| !b.is_empty()) {
            Some(base) => Self::new(base, true),
            None => Self::new(token, false),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawParam<'a> {
    /// As written in the database (sanitised by the converter).
    pub name: Cow<'a, str>,
    pub ty: NativeType,
    pub description: Option<Cow<'a, str>>,
}

/// A native as a database describes it, before the Lua conversion.
#[derive(Clone, Debug)]
pub struct RawNative<'a> {
    pub origin: Origin,
    pub hash: Cow<'a, str>,
    /// `None` for unnamed natives (the Lua name derives from the hash).
    pub name: Option<Cow<'a, str>>,
    pub namespace: Cow<'a, str>,
    pub params: Vec<RawParam<'a>>,
    pub result: Option<NativeType>,
    /// Markdown, not normalised yet.
    pub description: Cow<'a, str>,
    pub results_description: Option<Cow<'a, str>>,
    pub lua_example: Option<Cow<'a, str>>,
    pub aliases: Vec<Cow<'a, str>>,
    /// `client` for game natives.
    pub apiset: Cow<'a, str>,
    /// `gta5` / `rdr3` for game natives, the CFX `game` field otherwise.
    pub game: Option<Cow<'a, str>>,
    /// RDR3 NativeDB name, when it differs from the runtime name.
    pub alt_name: Option<String>,
}

/// A database whose content is not what it should be (an HTML error page, a truncated download).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Canonical join key of a hash: `0x` + upper-case hex digits.
pub fn normalize_hash(hash: &str) -> String {
    let h = hash.trim();
    let h = h.strip_prefix("0x").unwrap_or(h);
    let h = h.strip_prefix("0X").unwrap_or(h);
    format!("0x{}", h.to_uppercase())
}

/// Java's `\s`.
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_ascii() && is_ws(c as u8))
}

fn non_blank(s: &str) -> bool {
    !s.trim().is_empty()
}

// ---------------------------------------------------------------------------
// JSON databases

/// `natives.json` / `natives_cfx.json`: `{ "<NAMESPACE>": { "<hash>": { name?, params[], results, … } } }`, in document order.
pub fn parse_json(text: &str, origin: Origin) -> Result<Vec<RawNative<'_>>, FormatError> {
    let mut sink = JsonSink { origin, out: Vec::new() };
    read_namespaces(text, &mut sink)?;
    Ok(sink.out)
}

/// An RDR3 NativeDB entry: its name (when it has one) and comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdrDoc<'a> {
    pub name: Option<Cow<'a, str>>,
    pub comment: Cow<'a, str>,
}

/// `rdr3_nativedb.json`, keyed by [`normalize_hash`].
pub fn parse_nativedb(text: &str) -> Result<HashMap<String, RdrDoc<'_>>, FormatError> {
    let mut sink = DocsSink { out: HashMap::new() };
    read_namespaces(text, &mut sink)?;
    Ok(sink.out)
}

/// What a database does with each `namespace → key → object` entry.
trait Sink<'de> {
    type Entry: Visitor<'de>;
    fn entry(&self) -> Self::Entry;
    fn add(
        &mut self,
        namespace: Cow<'de, str>,
        key: Cow<'de, str>,
        value: <Self::Entry as Visitor<'de>>::Value,
    ) -> Result<(), String>;
    fn is_empty(&self) -> bool;
}

/// Walks `{ namespace: { key: object } }`, reporting the structural errors of the source.
fn read_namespaces<'de, S: Sink<'de>>(text: &'de str, sink: &mut S) -> Result<(), FormatError> {
    let mut error = None;
    let mut de = serde_json::Deserializer::from_str(text);
    let read = IfObject(Namespaces { sink: &mut *sink, error: &mut error }).deserialize(&mut de).and_then(|r| {
        de.end()?;
        Ok(r)
    });
    if let Some(e) = error {
        return Err(FormatError(e));
    }
    match read {
        Err(e) => Err(FormatError(format!("invalid JSON: {e}"))),
        Ok(None) => Err(FormatError("expected a JSON object of namespaces".into())),
        Ok(Some(())) if sink.is_empty() => Err(FormatError("no natives found".into())),
        Ok(Some(())) => Ok(()),
    }
}

/// A string that borrows from the source when it has no escapes.
struct Text<'a>(Cow<'a, str>);

impl<'de> Deserialize<'de> for Text<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Text<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Text(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Text(Cow::Owned(v.to_owned())))
            }
        }
        d.deserialize_str(V)
    }
}

/// A JSON primitive as text (numbers and booleans stringified); `None` for null, objects and arrays.
struct Prim<'a>(Option<Cow<'a, str>>);

impl<'de> Deserialize<'de> for Prim<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Prim<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Owned(v.to_string()))))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Owned(v.to_string()))))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Owned(v.to_string()))))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Owned(v.to_string()))))
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Prim(Some(Cow::Owned(v.to_owned()))))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Prim(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                skip_seq(seq)?;
                Ok(Prim(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                skip_map(map)?;
                Ok(Prim(None))
            }
        }
        d.deserialize_any(V)
    }
}

fn skip_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

fn skip_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

/// Scalars are none of the expected shapes.
macro_rules! scalars_are_none {
    () => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    };
}

/// `Some` when the value is a JSON object (read by the inner visitor), `None` for anything else.
struct IfObject<V>(V);

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for IfObject<V> {
    type Value = Option<V::Value>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for IfObject<V> {
    type Value = Option<V::Value>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(map).map(Some)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        skip_seq(seq)?;
        Ok(None)
    }
    scalars_are_none!();
}

/// `Some` when the value is a JSON array (read by the inner visitor), `None` for anything else.
struct IfArray<V>(V);

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for IfArray<V> {
    type Value = Option<V::Value>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for IfArray<V> {
    type Value = Option<V::Value>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(seq).map(Some)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        skip_map(map)?;
        Ok(None)
    }
    scalars_are_none!();
}

/// The root object: one entry per namespace.
struct Namespaces<'s, S> {
    sink: &'s mut S,
    error: &'s mut Option<String>,
}

impl<'de, S: Sink<'de>> Visitor<'de> for Namespaces<'_, S> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON object of namespaces")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(Text(ns)) = map.next_key()? {
            let entries = Entries { namespace: &ns, sink: &mut *self.sink, error: &mut *self.error };
            if map.next_value_seed(IfObject(entries))?.is_none() {
                *self.error = Some(format!("namespace {ns} is not an object"));
            }
            if self.error.is_some() {
                return Err(de::Error::custom("structure"));
            }
        }
        Ok(())
    }
}

/// The natives of one namespace.
struct Entries<'s, 'n, 'de, S> {
    namespace: &'n Cow<'de, str>,
    sink: &'s mut S,
    error: &'s mut Option<String>,
}

impl<'de, S: Sink<'de>> Visitor<'de> for Entries<'_, '_, 'de, S> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an object of natives")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(Text(key)) = map.next_key()? {
            let failure = match map.next_value_seed(IfObject(self.sink.entry()))? {
                Some(value) => self.sink.add(self.namespace.clone(), key, value).err(),
                None => Some(format!("native {key} is not an object")),
            };
            if failure.is_some() {
                *self.error = failure;
                return Err(de::Error::custom("structure"));
            }
        }
        Ok(())
    }
}

/// The fields of a docs.fivem.net native the converter uses.
#[derive(Default)]
struct NativeFields<'a> {
    name: Option<Cow<'a, str>>,
    params: Option<Vec<Option<ParamFields<'a>>>>,
    results: Option<Cow<'a, str>>,
    description: Option<Cow<'a, str>>,
    example: Option<Cow<'a, str>>,
    hash: Option<Cow<'a, str>>,
    ns: Option<Cow<'a, str>>,
    aliases: Option<Vec<Cow<'a, str>>>,
    results_description: Option<Cow<'a, str>>,
    apiset: Option<Cow<'a, str>>,
    game: Option<Cow<'a, str>>,
}

#[derive(Default)]
struct ParamFields<'a> {
    name: Option<Cow<'a, str>>,
    ty: Option<Cow<'a, str>>,
    description: Option<Cow<'a, str>>,
}

struct NativeVisitor;

impl<'de> Visitor<'de> for NativeVisitor {
    type Value = NativeFields<'de>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a native")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut f = NativeFields::default();
        while let Some(Text(key)) = map.next_key()? {
            match key.as_ref() {
                "name" => f.name = map.next_value::<Prim>()?.0,
                "params" => f.params = map.next_value_seed(IfArray(ParamsVisitor))?,
                "results" => f.results = map.next_value::<Prim>()?.0,
                "description" => f.description = map.next_value::<Prim>()?.0,
                "examples" => f.example = map.next_value_seed(IfArray(ExamplesVisitor))?.flatten(),
                "hash" => f.hash = map.next_value::<Prim>()?.0,
                "ns" => f.ns = map.next_value::<Prim>()?.0,
                "aliases" => f.aliases = map.next_value_seed(IfArray(AliasesVisitor))?,
                "resultsDescription" => f.results_description = map.next_value::<Prim>()?.0,
                "apiset" => f.apiset = map.next_value::<Prim>()?.0,
                "game" => f.game = map.next_value::<Prim>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(f)
    }
}

/// `params`: each element an object (`None` otherwise, reported by the caller).
struct ParamsVisitor;

impl<'de> Visitor<'de> for ParamsVisitor {
    type Value = Vec<Option<ParamFields<'de>>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("parameters")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(p) = seq.next_element_seed(IfObject(ParamVisitor))? {
            out.push(p);
        }
        Ok(out)
    }
}

struct ParamVisitor;

impl<'de> Visitor<'de> for ParamVisitor {
    type Value = ParamFields<'de>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a parameter")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut p = ParamFields::default();
        while let Some(Text(key)) = map.next_key()? {
            match key.as_ref() {
                "name" => p.name = map.next_value::<Prim>()?.0,
                "type" => p.ty = map.next_value::<Prim>()?.0,
                "description" => p.description = map.next_value::<Prim>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(p)
    }
}

/// `examples`: the code of the first example whose `lang` is Lua.
struct ExamplesVisitor;

impl<'de> Visitor<'de> for ExamplesVisitor {
    type Value = Option<Cow<'de, str>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("examples")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut lua: Option<Option<Cow<'de, str>>> = None;
        while let Some(example) = seq.next_element_seed(IfObject(ExampleVisitor))? {
            if let Some((Some(lang), code)) = example
                && lua.is_none()
                && lang.eq_ignore_ascii_case("lua")
            {
                lua = Some(code);
            }
        }
        Ok(lua.flatten())
    }
}

struct ExampleVisitor;

impl<'de> Visitor<'de> for ExampleVisitor {
    type Value = (Option<Cow<'de, str>>, Option<Cow<'de, str>>);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an example")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let (mut lang, mut code) = (None, None);
        while let Some(Text(key)) = map.next_key()? {
            match key.as_ref() {
                "lang" => lang = map.next_value::<Prim>()?.0,
                "code" => code = map.next_value::<Prim>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok((lang, code))
    }
}

/// `aliases`: the primitive items.
struct AliasesVisitor;

impl<'de> Visitor<'de> for AliasesVisitor {
    type Value = Vec<Cow<'de, str>>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("aliases")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(Prim(item)) = seq.next_element()? {
            out.extend(item);
        }
        Ok(out)
    }
}

struct JsonSink<'de> {
    origin: Origin,
    out: Vec<RawNative<'de>>,
}

impl<'de> Sink<'de> for JsonSink<'de> {
    type Entry = NativeVisitor;

    fn entry(&self) -> NativeVisitor {
        NativeVisitor
    }

    fn add(&mut self, namespace: Cow<'de, str>, key: Cow<'de, str>, f: NativeFields<'de>) -> Result<(), String> {
        let Some(params) = f.params else { return Err(format!("native {key} has no params")) };
        let params = params
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let p = p.ok_or_else(|| format!("bad parameter in {key}"))?;
                Ok(RawParam {
                    name: p.name.unwrap_or_else(|| Cow::Owned(format!("p{i}"))),
                    ty: NativeType::from_json(p.ty.as_deref().unwrap_or("")),
                    description: p.description,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let results = f.results.as_deref().unwrap_or("void");
        let cfx = self.origin == Origin::CfxJson;
        self.out.push(RawNative {
            origin: self.origin,
            result: (results != "void").then(|| NativeType::from_json(results)),
            hash: f.hash.unwrap_or(key),
            name: f.name.filter(|s| non_blank(s)),
            namespace: f.ns.unwrap_or(namespace),
            params,
            description: f.description.unwrap_or_default(),
            results_description: f.results_description.filter(|s| non_blank(s)),
            lua_example: f.example.filter(|s| non_blank(s)),
            aliases: f.aliases.unwrap_or_default(),
            apiset: if cfx { f.apiset.unwrap_or(Cow::Borrowed("client")) } else { Cow::Borrowed("client") },
            game: if cfx { f.game } else { Some(Cow::Borrowed("gta5")) },
            alt_name: None,
        });
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.out.is_empty()
    }
}

struct DocVisitor;

impl<'de> Visitor<'de> for DocVisitor {
    type Value = RdrDoc<'de>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a NativeDB entry")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let (mut name, mut comment) = (None, None);
        while let Some(Text(key)) = map.next_key()? {
            match key.as_ref() {
                "name" => name = map.next_value::<Prim>()?.0,
                "comment" => comment = map.next_value::<Prim>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(RdrDoc { name: name.filter(|s| non_blank(s)), comment: comment.unwrap_or_default() })
    }
}

struct DocsSink<'de> {
    out: HashMap<String, RdrDoc<'de>>,
}

impl<'de> Sink<'de> for DocsSink<'de> {
    type Entry = DocVisitor;

    fn entry(&self) -> DocVisitor {
        DocVisitor
    }

    fn add(&mut self, _: Cow<'de, str>, key: Cow<'de, str>, doc: RdrDoc<'de>) -> Result<(), String> {
        self.out.insert(normalize_hash(&key), doc);
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.out.is_empty()
    }
}

// ---------------------------------------------------------------------------
// RedM runtime DSL

/// One `native "…"` block of `natives_rdr3.lua`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DslNative<'a> {
    /// Native name, or `0x<hash>` when unnamed.
    pub name: &'a str,
    pub hash: Option<&'a str>,
    pub namespace: Option<&'a str>,
    /// `(type token, name)` pairs.
    pub arguments: Vec<(&'a str, &'a str)>,
    pub aliases: Vec<&'a str>,
    pub returns: Option<&'a str>,
    pub apisets: Vec<&'a str>,
    pub game: Option<&'a str>,
    /// The raw `doc [[! … ]]` content.
    pub doc: String,
}

/// Lines split at `\n`, `\r\n` and `\r`.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let s = rest?;
        match s.find(['\n', '\r']) {
            Some(i) => {
                let skip = if s[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = Some(&s[i + skip..]);
                Some(&s[..i])
            }
            None => {
                rest = None;
                Some(s)
            }
        }
    })
}

/// `keyword` followed by at least one space, at the start of the line (after spaces).
fn after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = skip_ws(line).strip_prefix(keyword)?;
    let t = skip_ws(rest);
    (t.len() < rest.len()).then_some(t)
}

/// `"text"` at the start of `s`: the text.
fn quoted(s: &str) -> Option<&str> {
    let t = s.strip_prefix('"')?;
    t.find('"').map(|end| &t[..end])
}

/// `native "NAME"`.
fn dsl_native(line: &str) -> Option<&str> {
    quoted(after_keyword(line, "native")?).filter(|n| !n.is_empty())
}

/// `hash "…"`, `alias '…'`, `ns`, `returns`, `apiset`, `game`.
fn dsl_directive(line: &str) -> Option<(&str, &str)> {
    let s = skip_ws(line);
    let end = s.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(s.len());
    let word = &s[..end];
    if !matches!(word, "hash" | "alias" | "ns" | "returns" | "apiset" | "game") {
        return None;
    }
    let value = after_keyword(s, word)?.strip_prefix(['"', '\''])?;
    value.find(['"', '\'']).map(|close| (word, &value[..close]))
}

/// `Type "name"` inside `arguments { … }`.
fn dsl_argument(line: &str) -> Option<(&str, &str)> {
    let s = skip_ws(line);
    if !s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len());
    let ty = &s[..end];
    Some((ty, quoted(after_keyword(s, ty)?)?))
}

/// The rest of a `doc [[` / `doc [[!` line.
fn dsl_doc(line: &str) -> Option<&str> {
    let s = skip_ws(skip_ws(line).strip_prefix("doc")?).strip_prefix("[[")?;
    Some(s.strip_prefix('!').unwrap_or(s))
}

/// Reads the DSL line by line; a `native` line inside a doc block is doc text.
pub fn parse_dsl(text: &str) -> Result<Vec<DslNative<'_>>, FormatError> {
    let mut out = Vec::new();
    let mut cur: Option<DslNative> = None;
    let (mut in_args, mut in_doc) = (false, false);
    for line in lines(text) {
        if in_doc && let Some(n) = cur.as_mut() {
            match line.find("]]") {
                Some(end) => {
                    n.doc.push_str(&line[..end]);
                    in_doc = false;
                }
                None => {
                    n.doc.push_str(line);
                    n.doc.push('\n');
                }
            }
            continue;
        }
        if let Some(name) = dsl_native(line) {
            out.extend(cur.replace(DslNative { name, ..DslNative::default() }));
            in_args = false;
            continue;
        }
        let Some(n) = cur.as_mut() else { continue };
        if in_args {
            if skip_ws(line).starts_with('}') {
                in_args = false;
            } else if let Some(arg) = dsl_argument(line) {
                n.arguments.push(arg);
            }
            continue;
        }
        if skip_ws(line).strip_prefix("arguments").is_some_and(|r| skip_ws(r).starts_with('{')) {
            // `arguments {}` on one line has no argument.
            in_args = !line.split_once('{').is_some_and(|(_, after)| after.contains('}'));
            continue;
        }
        if let Some(rest) = dsl_doc(line) {
            match rest.find("]]") {
                Some(end) => n.doc.push_str(&rest[..end]),
                None => {
                    n.doc.push_str(rest);
                    n.doc.push('\n');
                    in_doc = true;
                }
            }
            continue;
        }
        if let Some((directive, value)) = dsl_directive(line) {
            match directive {
                "hash" => n.hash = Some(value),
                "alias" => n.aliases.push(value),
                "ns" => n.namespace = Some(value),
                "returns" => n.returns = Some(value),
                "apiset" => n.apisets.push(value),
                _ => n.game = Some(value),
            }
        }
    }
    out.extend(cur);
    if out.is_empty() {
        return Err(FormatError("no native definitions found".into()));
    }
    Ok(out)
}

/// The text between the first `open` and the next `close`, trimmed.
fn tag<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    text[start..].find(close).map(|end| text[start..start + end].trim())
}

/// `<param name="x">text</param>` pairs of a DSL doc.
fn param_docs(doc: &str) -> Vec<(&str, &str)> {
    const OPEN: &str = "<param name=\"";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = doc[from..].find(OPEN).map(|i| from + i) {
        let rest = &doc[i + OPEN.len()..];
        let entry = rest.find('"').filter(|&q| q > 0).and_then(|q| {
            let body = rest[q..].strip_prefix("\">")?;
            let text_end = body.find('<')?;
            body[text_end..]
                .starts_with("</param>")
                .then(|| (&rest[..q], body[..text_end].trim(), q + 2 + text_end + 8))
        });
        match entry {
            Some((name, text, len)) => {
                out.push((name, text));
                from = i + OPEN.len() + len;
            }
            None => from = i + 1,
        }
    }
    out
}

/// NativeDB comments are plain text: fenced to keep their layout; else the DSL `<summary>`.
fn describe(doc: Option<&RdrDoc>, dsl_doc: &str) -> String {
    let comment = doc.map(|d| d.comment.replace("\r\n", "\n")).unwrap_or_default();
    let comment = comment.trim();
    if comment.is_empty() {
        return tag(dsl_doc, "<summary>", "</summary>").unwrap_or("").to_owned();
    }
    let fence = "`".repeat(longest_backtick_run(comment).max(2) + 1);
    format!("{fence}\n{comment}\n{fence}")
}

/// Length of the longest run of backticks in `text`.
pub fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// The RDR3 natives: the DSL's names and signatures (the only names RedM can
/// call) with the NativeDB descriptions.
pub fn join_rdr3<'a>(dsl: Vec<DslNative<'a>>, docs: &HashMap<String, RdrDoc<'_>>) -> Vec<RawNative<'a>> {
    dsl.into_iter()
        .filter_map(|n| {
            let hash = n.hash?;
            let unnamed = n.name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("0x"));
            let doc = docs.get(&normalize_hash(hash));
            let docs_of_params = param_docs(&n.doc);
            let params = n
                .arguments
                .iter()
                .map(|&(ty, name)| RawParam {
                    name: Cow::Borrowed(name),
                    ty: NativeType::from_dsl(ty),
                    description: docs_of_params
                        .iter()
                        .rev()
                        .find(|(p, _)| *p == name)
                        .map(|(_, text)| Cow::Owned((*text).to_owned())),
                })
                .collect();
            let alt_name =
                doc.and_then(|d| d.name.as_deref()).filter(|alt| lua_name(alt) != lua_name(n.name)).map(str::to_owned);
            Some(RawNative {
                origin: Origin::Rdr3Dsl,
                hash: Cow::Borrowed(hash),
                name: (!unnamed).then_some(Cow::Borrowed(n.name)),
                namespace: Cow::Borrowed(n.namespace.unwrap_or("UNKNOWN")),
                params,
                result: n.returns.filter(|r| *r != "void").map(NativeType::from_dsl),
                description: Cow::Owned(describe(doc, &n.doc)),
                results_description: tag(&n.doc, "<returns>", "</returns>")
                    .filter(|r| !r.is_empty())
                    .map(|r| Cow::Owned(r.to_owned())),
                lua_example: None,
                aliases: n.aliases.into_iter().map(Cow::Borrowed).collect(),
                apiset: Cow::Borrowed(n.apisets.first().copied().unwrap_or("client")),
                game: Some(Cow::Borrowed(n.game.unwrap_or("rdr3"))),
                alt_name,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DSL: &str = "-- Native definitions\n\nnative \"GET_CURRENT_PED_WEAPON\"\n    hash \"0x3A87E44BB9A01D54\"\n\
\targuments {\n\t\tPed \"ped\" [=[ {} ]=],\n\t\tHashPtr \"weaponHash\" [=[ {} ]=],\n\t\tcharPtr \"name\" [=[ {} ]=],\n\t}\n\
\talias \"_GET_CURRENT_PED_WEAPON_OLD\"\n\talias \"0x975BD6351648935F\"\n\tns \"WEAPON\"\n\treturns \"BOOL\"\n\tdoc [[!\n\
<summary>\nnative \"NOT_A_NATIVE\" (inside a doc)\n</summary>\n<param name=\"ped\">The ped</param>\n<returns>true when armed</returns>\n\t]]\n\n\
native \"0x733077295AB51304\"\n    hash \"0x733077295AB51304\"\n\targuments {\n\t\tAnyPtr \"args\" [=[ {} ]=],\n\t}\n\
\tns \"AICOVERPOINT\"\n\treturns \"void\"\n\tdoc [[! ]]\n\n\
native \"PLAYER_ID\"\r\n    hash \"0x217E9DC48139933D\"\r\n\targuments {}\r\n\tns \"PLAYER\"\r\n\tapiset \"client\"\r\n\tgame 'rdr3'\r\n\
\treturns \"Player\"\r\n\tdoc [[!\r\n\t]]\r\n";

    #[test]
    fn json_types() {
        let t = NativeType::from_json("Ped*");
        assert_eq!((t.kind, t.pointer, t.display.as_str()), (Kind::Handle, true, "Ped*"));
        for raw in ["char*", "const char*", "char *"] {
            assert_eq!(NativeType::from_json(raw), NativeType::string(), "{raw}");
        }
        let v = NativeType::from_json("Vector3");
        assert_eq!((v.kind, v.pointer, v.display.as_str()), (Kind::Vector3, false, "Vector3"));
        assert!(NativeType::from_json("").vararg);
        assert!(NativeType::from_json("...").vararg);
        assert_eq!(NativeType::from_json("Object").kind, Kind::Handle);
        assert_eq!(NativeType::from_json("object").kind, Kind::Object);
        assert_eq!(NativeType::from_json("Vector3 *").display, "Vector3*");
    }

    #[test]
    fn dsl_types() {
        let p = NativeType::from_dsl("PedPtr");
        assert_eq!((p.kind, p.pointer, p.display.as_str()), (Kind::Handle, true, "Ped*"));
        assert_eq!(NativeType::from_dsl("AnyPtr").display, "Any*");
        assert_eq!(NativeType::from_dsl("charPtr"), NativeType::string());
        assert!(!NativeType::from_dsl("Hash").pointer);
        assert!(!NativeType::from_dsl("Ptr").pointer);
        assert_eq!(NativeType::from_dsl("Volume").kind, Kind::Handle);
    }

    #[test]
    fn json_structure_errors() {
        let err = |text: &str| parse_json(text, Origin::Gta5Json).err().map(|e| e.0).unwrap_or_default();
        assert_eq!(err("[1, 2]"), "expected a JSON object of namespaces");
        assert_eq!(err("{}"), "no natives found");
        assert_eq!(err(r#"{"A": 1}"#), "namespace A is not an object");
        assert_eq!(err(r#"{"A": {"0x1": []}}"#), "native 0x1 is not an object");
        assert_eq!(err(r#"{"A": {"0x1": {"params": {}}}}"#), "native 0x1 has no params");
        assert_eq!(err(r#"{"A": {"0x1": {"params": [1]}}}"#), "bad parameter in 0x1");
        assert!(err("<html><body>404</body></html>").starts_with("invalid JSON: "));
        assert!(err(r#"{"A": {}} trailing"#).starts_with("invalid JSON: "));
    }

    #[test]
    fn json_fields() {
        let text = r#"{"MISC": {"0xAB": {"name": " ", "params": [{"type": "float*"}, {"name": "n", "description": "d"}],
            "results": "BOOL", "examples": [{"lang": "js", "code": "x"}, {"lang": "Lua", "code": "lua()"}, {"lang": "lua", "code": "no"}],
            "aliases": ["_OLD", null, 7, {"x": 1}], "ns": 12, "resultsDescription": "", "jhash": "0x1", "game": "rdr3"}},
            "CFX": {"0x1A": {"params": [], "description": "a\"b", "game": "rdr3"}}}"#;
        let n = &parse_json(text, Origin::Gta5Json).unwrap()[0];
        assert_eq!(n.name, None, "blank names are unnamed");
        assert_eq!((n.hash.as_ref(), n.namespace.as_ref()), ("0xAB", "12"));
        assert_eq!(n.params[0].name, "p0");
        assert!(n.params[0].ty.pointer);
        assert!(n.params[1].ty.vararg, "a missing type is a vararg");
        assert_eq!(n.params[1].description.as_deref(), Some("d"));
        assert_eq!(n.result.as_ref().map(|r| r.kind), Some(Kind::Bool));
        assert_eq!(n.lua_example.as_deref(), Some("lua()"));
        assert_eq!(n.aliases, ["_OLD", "7"]);
        assert_eq!(n.results_description, None);
        assert_eq!((n.apiset.as_ref(), n.game.as_deref()), ("client", Some("gta5")), "game natives ignore apiset/game");
        let cfx = parse_json(text, Origin::CfxJson).unwrap();
        assert_eq!(cfx[0].game.as_deref(), Some("rdr3"));
        assert_eq!((cfx[1].hash.as_ref(), cfx[1].description.as_ref()), ("0x1A", "a\"b"));
        assert_eq!(cfx[1].result, None);
    }

    #[test]
    fn dsl_without_running_it() {
        let natives = parse_dsl(DSL).unwrap();
        let names: Vec<&str> = natives.iter().map(|n| n.name).collect();
        assert_eq!(names, ["GET_CURRENT_PED_WEAPON", "0x733077295AB51304", "PLAYER_ID"]);
        let w = &natives[0];
        assert_eq!(w.hash, Some("0x3A87E44BB9A01D54"));
        assert_eq!(w.namespace, Some("WEAPON"));
        assert_eq!(w.arguments, [("Ped", "ped"), ("HashPtr", "weaponHash"), ("charPtr", "name")]);
        assert_eq!(w.aliases, ["_GET_CURRENT_PED_WEAPON_OLD", "0x975BD6351648935F"]);
        assert_eq!(w.returns, Some("BOOL"));
        assert!(w.doc.contains("NOT_A_NATIVE"));
        assert_eq!(natives[1].arguments, [("AnyPtr", "args")]);
        assert!(natives[2].arguments.is_empty());
        assert_eq!(natives[2].apisets, ["client"]);
        assert_eq!(natives[2].game, Some("rdr3"));
        assert_eq!(parse_dsl("<html><body>404</body></html>").unwrap_err().0, "no native definitions found");
    }

    #[test]
    fn rdr3_join() {
        let docs = HashMap::from([
            (
                "0x3A87E44BB9A01D54".to_owned(),
                RdrDoc { name: Some("GET_CURRENT_PED_WEAPON".into()), comment: "attachPoint: see X\r\n".into() },
            ),
            (
                "0x733077295AB51304".to_owned(),
                RdrDoc { name: Some("_ADD_COVER_BLOCKING_AREA".into()), comment: "uses ``` inside".into() },
            ),
        ]);
        let joined = join_rdr3(parse_dsl(DSL).unwrap(), &docs);
        let w = &joined[0];
        assert_eq!(w.description, "```\nattachPoint: see X\n```");
        assert_eq!(w.params[0].description.as_deref(), Some("The ped"));
        assert_eq!(w.results_description.as_deref(), Some("true when armed"));
        let types: Vec<(&str, bool)> = w.params.iter().map(|p| (p.ty.display.as_str(), p.ty.pointer)).collect();
        assert_eq!(types, [("Ped", false), ("Hash*", true), ("char*", false)]);
        assert_eq!((w.alt_name.as_deref(), w.game.as_deref()), (None, Some("rdr3")));
        let unnamed = &joined[1];
        assert_eq!(unnamed.name, None);
        assert_eq!(unnamed.alt_name.as_deref(), Some("_ADD_COVER_BLOCKING_AREA"));
        assert_eq!(unnamed.description, "````\nuses ``` inside\n````");
        assert_eq!(joined[2].description, "", "no NativeDB entry and no summary");
    }

    #[test]
    fn param_doc_tags() {
        let doc = "<param name=\"\">x</param><param name=\"a\">A <b></param><param name=\"b\"> B </param>";
        assert_eq!(param_docs(doc), [("b", "B")]);
        assert_eq!(normalize_hash(" 0xab12 "), "0xAB12");
        assert_eq!(normalize_hash("0XAB12"), "0xAB12");
    }
}
