//! The natives search engine: a typed text (Lua name, `NATIVE_NAME`, camel
//! humps, hash) against one or several natives sets, best matches first.
//!
//! Matching is allocation-free per native (case-insensitive byte comparisons,
//! reused word buffers), so a search over a full catalog (~15k natives) is one
//! cheap pass.

use crate::natives::{Native, NativeSet, Natives};
use crate::project::Side;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};

/// How well a native matches the text, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    /// Lua name, native name or hash equal to the text (case-insensitive).
    Exact,
    /// Lua name, native name or hash starting with the text.
    Prefix,
    /// The text picks the start of consecutive words of the Lua name (`ggz` → `GetGroundZFor_3dCoord`).
    CamelHump,
    /// Lua name, native name or hash containing the text.
    Substring,
    /// Blank text: every native passing the filters.
    Any,
}

/// Search criteria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// `GetEntityCoords`, `GET_ENTITY_COORDS`, `GEC`, `0x3FEF770D40960D5A` (with or without `0x`); blank: everything.
    pub text: String,
    /// Only this namespace (`ENTITY`).
    pub namespace: Option<String>,
    /// Only these sides (apisets).
    pub sides: Option<Vec<Side>>,
    pub include_aliases: bool,
    /// Natives known by their hash only (`N_0x…`).
    pub include_unnamed: bool,
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self { text: String::new(), namespace: None, sides: None, include_aliases: true, include_unnamed: true, limit: usize::MAX }
    }
}

impl Query {
    pub fn text(text: &str) -> Self {
        Self { text: text.to_owned(), ..Self::default() }
    }
}

/// The natives of `sets` matching `q`: by [`MatchKind`], then primaries before
/// aliases, named before unnamed, then by name (case-insensitive) and set. A
/// native present in several sets (CFX shared natives) is listed once, from
/// the first set in GTA5, RDR3, SERVER order.
pub fn search<'a>(natives: &'a Natives, sets: &[NativeSet], q: &Query) -> Vec<(&'a Native, MatchKind)> {
    let pattern = Pattern::new(&q.text);
    let sets: Vec<NativeSet> = NativeSet::ALL.into_iter().filter(|s| sets.contains(s)).collect();
    let mut seen: HashSet<(&str, &str)> = HashSet::new();
    let mut words = Vec::new();
    let mut compact = Vec::new();
    let mut out = Vec::new();
    for &set in &sets {
        for n in &natives.catalog(set).natives {
            if !accept(n, q) {
                continue;
            }
            let kind = match &pattern {
                None => MatchKind::Any,
                Some(p) => match p.matches(n, &mut words, &mut compact) {
                    Some(k) => k,
                    None => continue,
                },
            };
            if sets.len() > 1 && !seen.insert((n.lua_name.as_str(), n.hash.as_str())) {
                continue;
            }
            out.push((n, kind));
        }
    }
    if out.len() > q.limit {
        if q.limit == 0 {
            return Vec::new();
        }
        out.select_nth_unstable_by(q.limit - 1, order);
        out.truncate(q.limit);
    }
    out.sort_by(order);
    out
}

/// Namespaces of the natives of `sets`, sorted.
pub fn namespaces(natives: &Natives, sets: &[NativeSet]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for &set in sets {
        out.extend(natives.catalog(set).natives.iter().map(|n| n.namespace.as_str()));
    }
    out.into_iter().map(str::to_owned).collect()
}

fn accept(n: &Native, q: &Query) -> bool {
    (q.include_aliases || !n.is_alias())
        && (q.include_unnamed || !n.is_unnamed())
        && q.namespace.as_ref().is_none_or(|ns| *ns == n.namespace)
        && q.sides.as_ref().is_none_or(|s| s.contains(&n.apiset))
}

fn order(a: &(&Native, MatchKind), b: &(&Native, MatchKind)) -> Ordering {
    a.1.cmp(&b.1)
        .then(a.0.is_alias().cmp(&b.0.is_alias()))
        .then(a.0.is_unnamed().cmp(&b.0.is_unnamed()))
        .then_with(|| lc_cmp(a.0.lua_name.as_bytes(), b.0.lua_name.as_bytes()))
        .then_with(|| a.0.lua_name.cmp(&b.0.lua_name))
        .then(a.0.set.cmp(&b.0.set))
}

fn lc_cmp(a: &[u8], b: &[u8]) -> Ordering {
    a.iter().map(u8::to_ascii_lowercase).cmp(b.iter().map(u8::to_ascii_lowercase))
}

/// `0xC906A7DA` / `C906A7DA` → `C906A7DA` (case kept: compared case-insensitively).
fn hash_digits(hash: &str) -> &str {
    let h = hash.trim();
    h.strip_prefix("0x").or_else(|| h.strip_prefix("0X")).unwrap_or(h)
}

/// A search text, pre-processed once per search.
struct Pattern {
    /// Lower-cased, trimmed text.
    lower: Vec<u8>,
    /// `lower` without `_` and whitespace.
    compact: Vec<u8>,
    /// Hex digits when the text looks like a hash.
    hash: Option<Vec<u8>>,
    /// Whether hashes may also match by prefix / substring (explicit `0x`, or 6 digits and more).
    hash_prefix: bool,
}

impl Pattern {
    fn new(text: &str) -> Option<Self> {
        let lower = text.trim().to_lowercase();
        if lower.is_empty() {
            return None;
        }
        let compact: Vec<u8> = lower.bytes().filter(|b| *b != b'_' && !b.is_ascii_whitespace()).collect();
        let digits = lower.strip_prefix("0x").unwrap_or(&lower);
        let hex = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit());
        let hash = hex.then(|| digits.as_bytes().to_vec());
        // Short words such as `add` are valid hex: hashes need an explicit `0x` or a long text.
        let hash_prefix = hash.is_some() && (lower.starts_with("0x") || digits.len() >= 6);
        Some(Self { lower: lower.into_bytes(), compact, hash, hash_prefix })
    }

    fn matches(&self, n: &Native, words: &mut Vec<(usize, usize)>, compact: &mut Vec<u8>) -> Option<MatchKind> {
        let name = n.lua_name.as_bytes();
        let original = n.original_name.as_deref().map(str::as_bytes);
        let hash = hash_digits(&n.hash).as_bytes();
        let q = self.lower.as_slice();
        let qh = self.hash.as_deref();
        if lc_eq(name, q) || original.is_some_and(|o| lc_eq(o, q)) || qh.is_some_and(|h| lc_eq(hash, h)) {
            return Some(MatchKind::Exact);
        }
        compact.clear();
        compact.extend(name.iter().filter(|b| **b != b'_').map(u8::to_ascii_lowercase));
        let hashed = |f: fn(&[u8], &[u8]) -> bool| self.hash_prefix && qh.is_some_and(|h| f(hash, h));
        let has_compact = !self.compact.is_empty();
        if lc_starts(name, q)
            || original.is_some_and(|o| lc_starts(o, q))
            || (has_compact && compact.starts_with(&self.compact))
            || hashed(lc_starts)
        {
            return Some(MatchKind::Prefix);
        }
        if has_compact {
            split_words(name, words);
            if camel_humps(name, words, &self.compact, 0, 0, true) {
                return Some(MatchKind::CamelHump);
            }
        }
        if lc_contains(name, q)
            || original.is_some_and(|o| lc_contains(o, q))
            || (has_compact && lc_contains(compact, &self.compact))
            || hashed(lc_contains)
        {
            return Some(MatchKind::Substring);
        }
        None
    }
}

/// `a` equals the lower-case `q`, ignoring ASCII case.
fn lc_eq(a: &[u8], q: &[u8]) -> bool {
    a.len() == q.len() && lc_starts(a, q)
}

fn lc_starts(a: &[u8], q: &[u8]) -> bool {
    a.len() >= q.len() && a.iter().zip(q).all(|(x, y)| x.to_ascii_lowercase() == *y)
}

fn lc_contains(a: &[u8], q: &[u8]) -> bool {
    q.is_empty() || (a.len() >= q.len() && (0..=a.len() - q.len()).any(|i| lc_starts(&a[i..], q)))
}

/// Word ranges of a Lua name: `GetGroundZFor_3dCoord` → `Get`, `Ground`, `ZFor`, `3d`, `Coord`.
fn split_words(name: &[u8], out: &mut Vec<(usize, usize)>) {
    out.clear();
    let mut start = 0;
    for (i, &c) in name.iter().enumerate() {
        if c == b'_' {
            if i > start {
                out.push((start, i));
            }
            start = i + 1;
            continue;
        }
        if i > start {
            let p = name[i - 1];
            let lower_to_upper = c.is_ascii_uppercase() && (p.is_ascii_lowercase() || p.is_ascii_digit());
            let letter_to_digit = c.is_ascii_digit() && p.is_ascii_alphabetic();
            if lower_to_upper || letter_to_digit {
                out.push((start, i));
                start = i;
            }
        }
    }
    if start < name.len() {
        out.push((start, name.len()));
    }
}

/// Whether `q[qi..]` is a sequence of word prefixes of `words[wi..]`, in
/// order: words may be skipped, except the first one when `anchored`.
fn camel_humps(name: &[u8], words: &[(usize, usize)], q: &[u8], qi: usize, wi: usize, anchored: bool) -> bool {
    if qi == q.len() {
        return true;
    }
    let Some(max) = words.len().checked_sub(1) else { return false };
    let last = if anchored { wi.min(max) } else { max };
    for w in wi..=last {
        let (s, e) = words[w];
        let word = &name[s..e];
        if word[0].to_ascii_lowercase() != q[qi] {
            continue;
        }
        let mut common = 1;
        while common < word.len() && qi + common < q.len() && word[common].to_ascii_lowercase() == q[qi + common] {
            common += 1;
        }
        if (1..=common).rev().any(|len| camel_humps(name, words, q, qi + len, w + 1, false)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::natives::{Param, Return};
    use crate::project::Game;
    use crate::testing::native;

    fn param(name: &str, lua: &str, nat: &str, desc: Option<&str>, out: bool) -> Param {
        Param {
            name: name.into(),
            lua_type: lua.into(),
            native_type: nat.into(),
            description: desc.map(Into::into),
            out_pointer: out,
            kept_trailing_pointer: false,
        }
    }

    fn ret(name: &str, lua: &str, nat: &str, desc: Option<&str>) -> Return {
        Return { name: name.into(), lua_type: lua.into(), native_type: nat.into(), description: desc.map(Into::into) }
    }

    fn named(set: NativeSet, lua: &str, original: Option<&str>, hash: &str, ns: &str, side: Side) -> Native {
        let mut n = native(set, lua, side);
        n.original_name = original.map(Into::into);
        n.hash = hash.into();
        n.namespace = ns.into();
        n.game = set.game();
        n.doc_url = format!("https://docs.fivem.net/natives/?_{hash}");
        n
    }

    /// The fixture catalogs of the original test suite.
    pub(crate) fn fixture() -> Vec<Native> {
        let g = NativeSet::Gta5;
        let mut ground = named(g, "GetGroundZFor_3dCoord", Some("GET_GROUND_Z_FOR_3D_COORD"), "0xC906A7DAB05C8D2B", "MISC", Side::Client);
        ground.params = vec![
            param("x", "number", "float", None, false),
            param("y", "number", "float", None, false),
            param("z", "number", "float", None, false),
            param("groundZ", "number", "float*", None, true),
            param("ignoreWater", "boolean", "BOOL", Some("Ignore water surfaces"), false),
        ];
        ground.returns =
            vec![ret("retval", "boolean", "BOOL", Some("Whether the ground was found")), ret("groundZ", "number", "float*", None)];
        ground.description = "Gets the ground elevation at the specified position.\n\n**Note**: the area must be loaded.".into();
        ground.lua_example = Some("local found, z = GetGroundZFor_3dCoord(0.0, 0.0, 1000.0, false)".into());
        let mut coords = named(g, "GetEntityCoords", Some("GET_ENTITY_COORDS"), "0x3FEF770D40960D5A", "ENTITY", Side::Client);
        coords.params = vec![param("entity", "integer", "Entity", Some("The entity"), false), param("alive", "boolean", "BOOL", None, false)];
        coords.returns = vec![ret("retval", "vector3", "Vector3", None)];
        coords.aliases = vec!["GetEntityCoordinates".into()];
        let mut alias = coords.clone();
        alias.lua_name = "GetEntityCoordinates".into();
        alias.original_name = Some("_GET_ENTITY_COORDINATES".into());
        alias.aliases.clear();
        alias.alias_of = Some("GetEntityCoords".into());
        let mut heading = named(g, "GetEntityHeading", Some("GET_ENTITY_HEADING"), "0x00000001", "ENTITY", Side::Client);
        heading.params = vec![param("entity", "integer", "Entity", None, false)];
        heading.returns = vec![ret("retval", "number", "float", None)];
        let mut ped = named(g, "PlayerPedId", Some("PLAYER_PED_ID"), "0x00000002", "PLAYER", Side::Client);
        ped.returns = vec![ret("retval", "integer", "Ped", None)];
        let mut set_coords = named(g, "SetEntityCoords", Some("SET_ENTITY_COORDS"), "0x00000003", "ENTITY", Side::Client);
        set_coords.params = vec![param("entity", "integer", "Entity", None, false), param("x", "number", "float", None, false)];
        let unnamed = named(g, "N_0x0b40ed49d7d6ff84", None, "0x0B40ED49D7D6FF84", "MISC", Side::Client);
        let mut resource = named(g, "GetCurrentResourceName", Some("GET_CURRENT_RESOURCE_NAME"), "0xE5E9EBBB", "CFX", Side::Shared);
        resource.returns = vec![ret("retval", "string", "char*", None)];
        resource.game = None;
        let mut server_resource = resource.clone();
        server_resource.set = NativeSet::Server;
        let mut drop = named(NativeSet::Server, "DropPlayer", Some("DROP_PLAYER"), "0xBA0613E1", "CFX", Side::Server);
        drop.params = vec![param("playerSrc", "string", "char*", None, false), param("reason", "string", "char*", None, false)];
        let mut rdr = named(NativeSet::Rdr3, "GetEntityCoordsRdr", Some("GET_ENTITY_COORDS_RDR"), "0xA86D5F069399F44D", "ENTITY", Side::Client);
        rdr.game = Some(Game::Rdr3);
        vec![ground, coords, alias, heading, ped, set_coords, unnamed, resource, server_resource, drop, rdr]
    }

    fn natives() -> Natives {
        Natives::from_natives(fixture())
    }

    fn names(n: &Natives, q: &Query) -> Vec<String> {
        search(n, &[NativeSet::Gta5], q).into_iter().map(|(n, _)| n.lua_name.clone()).collect()
    }

    fn text(n: &Natives, t: &str) -> Vec<String> {
        names(n, &Query::text(t))
    }

    fn kinds(n: &Natives, t: &str) -> Vec<MatchKind> {
        search(n, &[NativeSet::Gta5], &Query::text(t)).into_iter().map(|(_, k)| k).collect()
    }

    #[test]
    fn exact_before_prefix_and_aliases_after_primaries() {
        let n = natives();
        assert_eq!(text(&n, "GetEntityCoords"), ["GetEntityCoords"]);
        assert_eq!(kinds(&n, "GetEntityCoords"), [MatchKind::Exact]);
        assert_eq!(text(&n, "GetEntityCoord"), ["GetEntityCoords", "GetEntityCoordinates"]);
        assert!(kinds(&n, "GetEntityCoord").iter().all(|k| *k == MatchKind::Prefix));
        assert_eq!(text(&n, "getentity"), ["GetEntityCoords", "GetEntityHeading", "GetEntityCoordinates"]);
        assert!(kinds(&n, "getentity").iter().all(|k| *k == MatchKind::Prefix));
    }

    #[test]
    fn camel_humps_anchor_the_first_word() {
        let n = natives();
        assert_eq!(text(&n, "ggz"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(text(&n, "GGZF3C"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(text(&n, "GetGrZ"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(kinds(&n, "gec")[0], MatchKind::CamelHump);
        assert_eq!(text(&n, "gcoords"), ["GetEntityCoords"]);
        assert!(!kinds(&n, "ec").contains(&MatchKind::CamelHump));
        let mut words = Vec::new();
        split_words(b"GetGroundZFor_3dCoord", &mut words);
        let w: Vec<&str> = words.iter().map(|&(s, e)| &"GetGroundZFor_3dCoord"[s..e]).collect();
        assert_eq!(w, ["Get", "Ground", "ZFor", "3d", "Coord"]);
    }

    #[test]
    fn substring_comes_last() {
        let n = natives();
        assert_eq!(text(&n, "coords"), ["GetEntityCoords", "SetEntityCoords"]);
        assert!(kinds(&n, "coords").iter().all(|k| *k == MatchKind::Substring));
    }

    #[test]
    fn native_names_with_or_without_underscores() {
        let n = natives();
        assert_eq!(kinds(&n, "GET_ENTITY_COORDS")[0], MatchKind::Exact);
        assert_eq!(text(&n, "GET_GROUND_Z"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(text(&n, "get entity heading"), ["GetEntityHeading"]);
    }

    #[test]
    fn hashes_with_or_without_0x_in_any_case() {
        let n = natives();
        assert_eq!(text(&n, "0xC906A7DAB05C8D2B"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(text(&n, "c906a7dab05c8d2b"), ["GetGroundZFor_3dCoord"]);
        assert_eq!(kinds(&n, "C906A7DAB05C8D2B"), [MatchKind::Exact]);
        assert_eq!(text(&n, "0xc906"), ["GetGroundZFor_3dCoord"]);
        assert!(text(&n, "c906").is_empty(), "short hex words are names");
        assert_eq!(text(&n, "0x0B40ED49D7D6FF84"), ["N_0x0b40ed49d7d6ff84"]);
    }

    #[test]
    fn filters() {
        let n = natives();
        assert_eq!(names(&n, &Query { namespace: Some("PLAYER".into()), ..Query::default() }), ["PlayerPedId"]);
        assert!(!names(&n, &Query { include_aliases: false, ..Query::default() }).contains(&"GetEntityCoordinates".to_owned()));
        assert!(!names(&n, &Query { include_unnamed: false, ..Query::default() }).contains(&"N_0x0b40ed49d7d6ff84".to_owned()));
        assert_eq!(names(&n, &Query { sides: Some(vec![Side::Shared]), ..Query::default() }), ["GetCurrentResourceName"]);
        assert_eq!(names(&n, &Query { limit: 3, ..Query::default() }).len(), 3);
        assert_eq!(namespaces(&n, &[NativeSet::Gta5]), ["CFX", "ENTITY", "MISC", "PLAYER"]);
    }

    #[test]
    fn blank_text_lists_everything_unnamed_then_aliases_last() {
        let n = natives();
        let all = text(&n, "");
        assert_eq!(all.len(), n.catalog(NativeSet::Gta5).len());
        assert_eq!(all[all.len() - 2], "N_0x0b40ed49d7d6ff84");
        assert_eq!(all[all.len() - 1], "GetEntityCoordinates");
    }

    #[test]
    fn several_sets_deduplicate_shared_natives() {
        let n = natives();
        let both = search(&n, &[NativeSet::Server, NativeSet::Gta5], &Query::text("GetCurrentResourceName"));
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].0.set, NativeSet::Gta5);
        let server = search(&n, &[NativeSet::Server], &Query::text("GetCurrentResourceName"));
        assert_eq!(server[0].0.set, NativeSet::Server);
        assert!(search(&n, &[NativeSet::Gta5], &Query::text("DropPlayer")).is_empty());
        assert_eq!(n.find("DropPlayer", &NativeSet::ALL).map(|n| n.set), Some(NativeSet::Server));
        assert!(n.find("DropPlayer", &[NativeSet::Gta5, NativeSet::Rdr3]).is_none());
        assert_eq!(search(&n, &NativeSet::ALL, &Query::default()).len(), 10, "11 natives, one shared");
    }

    #[test]
    fn fast_on_a_full_size_catalog() {
        let namespaces = ["ENTITY", "PED", "VEHICLE", "PLAYER", "MISC", "GRAPHICS", "HUD", "STREAMING"];
        let verbs = ["Get", "Set", "Is", "Has", "Clear", "Request", "Network", "Task"];
        let nouns = ["Entity", "Ped", "Vehicle", "Player", "Ground", "Coord", "Heading", "Model", "Weapon", "Blip", "Cam", "Object"];
        let big: Vec<Native> = (0..15_000usize)
            .map(|i| {
                let name = format!("{}{}{}Value{i}", verbs[i % 8], nouns[(i / 8) % 12], nouns[(i / 96) % 12]);
                let hash = format!("0x{:016X}", (i as u64).wrapping_mul(2_654_435_761));
                let upper = name.to_uppercase();
                named(NativeSet::Gta5, &name, Some(&upper), &hash, namespaces[i % 8], Side::Client)
            })
            .collect();
        let n = Natives::from_natives(big);
        let queries = ["getentity", "GEGC", "coord", "0x5", "value1234", "GET_PED", "xyz"];
        let q = |t: &str| Query { limit: 500, ..Query::text(t) };
        for t in queries {
            search(&n, &[NativeSet::Gta5], &q(t));
        }
        let slowest = queries
            .iter()
            .map(|t| {
                (0..3)
                    .map(|_| {
                        let start = std::time::Instant::now();
                        search(&n, &[NativeSet::Gta5], &q(t));
                        start.elapsed()
                    })
                    .min()
                    .unwrap_or_default()
            })
            .max()
            .unwrap_or_default();
        // Debug builds are several times slower than the plugin's release build.
        let budget = if cfg!(debug_assertions) { 500 } else { 50 };
        assert!(slowest.as_millis() < budget, "slowest query took {} ms", slowest.as_millis());
        assert_eq!(search(&n, &[NativeSet::Gta5], &Query::text("GetEntityEntityValue0"))[0].0.lua_name, "GetEntityEntityValue0");
    }
}
