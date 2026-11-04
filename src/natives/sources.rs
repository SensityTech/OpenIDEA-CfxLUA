//! The four Cfx.re databases: where they are downloaded from, the snapshot
//! bundled with the plugin (gzip, as downloaded), and which ones each natives
//! set is built from.

use super::convert::{self, Group, Overrides};
use super::model::NativeSet;
use super::parse::{self, FormatError, Origin, RawNative};
use flate2::read::GzDecoder;
use std::collections::BTreeMap;
use std::io::Read;

/// Bumped when the conversion changes: updates made by an older one are dropped.
const CONVERTER_VERSION: u32 = 1;

pub const OVERRIDES: &str = include_str!("../../data/natives/overrides.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Database {
    /// docs.fivem.net: GTA V game natives.
    Gta5,
    /// docs.fivem.net: CitizenFX natives (client / server / shared, every game).
    Cfx,
    /// RedM runtime codegen input: the only callable RedM names and signatures.
    Rdr3,
    /// alloc8or's RDR3 NativeDB: descriptions, joined by hash.
    Rdr3Docs,
}

impl Database {
    pub const ALL: [Database; 4] = [Database::Gta5, Database::Cfx, Database::Rdr3, Database::Rdr3Docs];

    pub fn id(self) -> &'static str {
        match self {
            Self::Gta5 => "gta5",
            Self::Cfx => "cfx",
            Self::Rdr3 => "rdr3",
            Self::Rdr3Docs => "rdr3_docs",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Gta5 => "natives.json",
            Self::Cfx => "natives_cfx.json",
            Self::Rdr3 => "natives_rdr3.lua",
            Self::Rdr3Docs => "rdr3_nativedb.json",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Gta5 => "https://static.cfx.re/natives/natives.json",
            Self::Cfx => "https://static.cfx.re/natives/natives_cfx.json",
            Self::Rdr3 => "https://static.cfx.re/natives/natives_rdr3.lua",
            Self::Rdr3Docs => "https://raw.githubusercontent.com/alloc8or/rdr3-nativedb-data/master/natives.json",
        }
    }

    /// The bundled snapshot, gzip-compressed.
    fn bundled_gz(self) -> &'static [u8] {
        match self {
            Self::Gta5 => include_bytes!("../../data/natives/natives.json.gz"),
            Self::Cfx => include_bytes!("../../data/natives/natives_cfx.json.gz"),
            Self::Rdr3 => include_bytes!("../../data/natives/natives_rdr3.lua.gz"),
            Self::Rdr3Docs => include_bytes!("../../data/natives/rdr3_nativedb.json.gz"),
        }
    }

    /// Fingerprint of the bundled content, read from the gzip trailer (CRC-32 and size).
    pub fn bundled_fingerprint(self) -> String {
        let gz = self.bundled_gz();
        let word = |at: usize| gz.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let n = gz.len().saturating_sub(8);
        format!("{:08x}:{}", word(n), word(n + 4))
    }

    /// The bundled content as text.
    pub fn bundled_text(self) -> Result<String, String> {
        let mut bytes = Vec::with_capacity(4 << 20);
        GzDecoder::new(self.bundled_gz()).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", self.file_name()))?;
        Ok(decode(bytes))
    }

    /// Checks that `text` is a database of this kind.
    pub fn validate(self, text: &str) -> Result<(), FormatError> {
        match self {
            Self::Gta5 => parse::parse_json(text, Origin::Gta5Json).map(drop),
            Self::Cfx => parse::parse_json(text, Origin::CfxJson).map(drop),
            Self::Rdr3 => parse::parse_dsl(text).map(drop),
            Self::Rdr3Docs => parse::parse_nativedb(text).map(drop),
        }
    }
}

/// Fingerprint of downloaded content (same form as [`Database::bundled_fingerprint`]).
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    format!("{:08x}:{}", crc.sum(), bytes.len() as u32)
}

/// UTF-8 text of a download, without its byte order mark.
pub fn decode(bytes: Vec<u8>) -> String {
    let text = String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

/// The databases a set is built from.
pub fn databases(set: NativeSet) -> &'static [Database] {
    match set {
        NativeSet::Gta5 => &[Database::Gta5, Database::Cfx],
        NativeSet::Rdr3 => &[Database::Rdr3, Database::Rdr3Docs, Database::Cfx],
        NativeSet::Server => &[Database::Cfx],
    }
}

/// Fingerprints of the content a set is built from, by database id.
pub type Inputs = BTreeMap<String, String>;

/// The inputs of `set` in the bundled snapshot.
pub fn bundled_inputs(set: NativeSet) -> Inputs {
    databases(set).iter().map(|d| (d.id().to_owned(), d.bundled_fingerprint())).collect()
}

/// Identifies the bundled snapshot and conversion of `set`: updates are made against it.
pub fn snapshot_id(set: NativeSet) -> String {
    let inputs: Vec<String> = bundled_inputs(set).into_iter().map(|(id, fp)| format!("{id}={fp}")).collect();
    format!("v{CONVERTER_VERSION};overrides={};{}", fingerprint(OVERRIDES.as_bytes()), inputs.join(";"))
}

fn json(d: Database, text: &str, origin: Origin) -> Result<Vec<RawNative<'_>>, (Database, FormatError)> {
    parse::parse_json(text, origin).map_err(|e| (d, e))
}

/// Converts the natives of `set` from the texts of its databases.
pub fn groups(
    set: NativeSet,
    text: impl Fn(Database) -> Option<String>,
) -> Result<Vec<Group>, (Database, FormatError)> {
    let get = |d: Database| text(d).ok_or_else(|| (d, FormatError("not available".into())));
    let cfx = get(Database::Cfx)?;
    let overrides = Overrides::bundled();
    match set {
        NativeSet::Gta5 => {
            let gta5 = get(Database::Gta5)?;
            let mut raw = json(Database::Gta5, &gta5, Origin::Gta5Json)?;
            raw.extend(json(Database::Cfx, &cfx, Origin::CfxJson)?);
            Ok(convert::convert(set, raw, &overrides))
        }
        NativeSet::Rdr3 => {
            let (dsl, docs) = (get(Database::Rdr3)?, get(Database::Rdr3Docs)?);
            let dsl = parse::parse_dsl(&dsl).map_err(|e| (Database::Rdr3, e))?;
            let docs = parse::parse_nativedb(&docs).map_err(|e| (Database::Rdr3Docs, e))?;
            let mut raw = parse::join_rdr3(dsl, &docs);
            raw.extend(json(Database::Cfx, &cfx, Origin::CfxJson)?);
            Ok(convert::convert(set, raw, &overrides))
        }
        NativeSet::Server => Ok(convert::convert(set, json(Database::Cfx, &cfx, Origin::CfxJson)?, &overrides)),
    }
}

/// The natives of `set` in the bundled snapshot (empty if it cannot be read: a build defect the tests catch).
pub fn bundled_groups(set: NativeSet) -> Vec<Group> {
    groups(set, |d| d.bundled_text().ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn text(d: Database) -> String {
        d.bundled_text().unwrap()
    }

    #[test]
    fn snapshot_inventory() {
        let gta5 = text(Database::Gta5);
        let natives = parse::parse_json(&gta5, Origin::Gta5Json).unwrap();
        let namespaces: HashSet<&str> = natives.iter().map(|n| n.namespace.as_ref()).collect();
        assert_eq!((natives.len(), namespaces.len()), (6436, 44));
        assert_eq!(natives.iter().filter(|n| n.name.is_none()).count(), 1001);
        assert_eq!(natives.iter().filter(|n| !n.aliases.is_empty()).count(), 2308);
        assert_eq!(natives.iter().filter(|n| n.lua_example.is_some()).count(), 293);

        let cfx = text(Database::Cfx);
        let natives = parse::parse_json(&cfx, Origin::CfxJson).unwrap();
        let count = |f: &dyn Fn(&RawNative) -> bool| natives.iter().filter(|n| f(n)).count();
        assert_eq!(natives.len(), 973);
        assert_eq!(
            [count(&|n| n.apiset == "client"), count(&|n| n.apiset == "server"), count(&|n| n.apiset == "shared")],
            [583, 292, 98]
        );
        assert_eq!([count(&|n| n.game.is_none()), count(&|n| n.game.as_deref() == Some("ny"))], [466, 2]);

        let dsl_text = text(Database::Rdr3);
        let dsl = parse::parse_dsl(&dsl_text).unwrap();
        assert_eq!(dsl.len(), 7154);
        assert!(dsl.iter().all(|n| n.hash.is_some() && n.namespace.is_some() && n.returns.is_some()));
        assert_eq!(dsl.iter().filter_map(|n| n.hash).collect::<HashSet<_>>().len(), 7154, "unique hashes");
        assert_eq!(dsl.iter().filter(|n| n.name.starts_with("0x")).count(), 2221);
        assert_eq!(dsl.iter().map(|n| n.aliases.len()).sum::<usize>(), 2746);
        assert_eq!(dsl.iter().filter(|n| n.aliases.len() > 1).count(), 36);

        let docs_text = text(Database::Rdr3Docs);
        let docs = parse::parse_nativedb(&docs_text).unwrap();
        assert_eq!(docs.len(), 7132);
        let missing =
            dsl.iter().filter_map(|n| n.hash).filter(|h| !docs.contains_key(&parse::normalize_hash(h))).count();
        assert_eq!(missing, 22);
        let commented = dsl
            .iter()
            .filter_map(|n| docs.get(&parse::normalize_hash(n.hash?)))
            .filter(|d| !d.comment.trim().is_empty())
            .count();
        assert_eq!(commented, 2161);
        let joined = parse::join_rdr3(dsl, &docs);
        assert_eq!(joined.iter().filter(|n| n.alt_name.is_some()).count(), 354);
    }

    #[test]
    fn fingerprints_and_snapshot() {
        for d in Database::ALL {
            let mut raw = Vec::new();
            GzDecoder::new(d.bundled_gz()).read_to_end(&mut raw).unwrap();
            assert_eq!(fingerprint(&raw), d.bundled_fingerprint(), "{}", d.file_name());
            assert!(d.validate(&decode(raw)).is_ok());
        }
        assert_eq!(decode(b"\xef\xbb\xbf{}".to_vec()), "{}");
        assert_eq!(decode(b"a\xffb".to_vec()), "a\u{fffd}b");
        let id = snapshot_id(NativeSet::Rdr3);
        assert!(id.starts_with("v1;overrides=") && id.contains(";cfx=") && id.contains(";rdr3_docs="), "{id}");
        assert!(!snapshot_id(NativeSet::Server).contains("gta5="));
        assert_eq!(Database::Cfx.validate("<html></html>").map_err(|e| e.0.starts_with("invalid JSON")), Err(true));
        let overrides = Overrides::bundled();
        assert_eq!(overrides.return_types.len(), 17);
        assert_eq!(overrides.param_types["REGISTER_RAW_KEYMAP"]["onKeyUp"], "fun()|nil");
        assert_eq!(overrides.string_param_types["playerSrc"], "string|integer");
    }
}
