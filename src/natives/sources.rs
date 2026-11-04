use super::convert::{self, Group, Overrides};
use super::model::NativeSet;
use super::parse::{self, FormatError, Origin, RawNative};
use flate2::read::GzDecoder;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

const CONVERTER_VERSION: u32 = 1;

pub const OVERRIDES: &str = include_str!("../../data/natives/overrides.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Database {
    Gta5,
    Cfx,
    Rdr3,
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

    fn bundled_gz(self) -> &'static [u8] {
        match self {
            Self::Gta5 => include_bytes!("../../data/natives/natives.json.gz"),
            Self::Cfx => include_bytes!("../../data/natives/natives_cfx.json.gz"),
            Self::Rdr3 => include_bytes!("../../data/natives/natives_rdr3.lua.gz"),
            Self::Rdr3Docs => include_bytes!("../../data/natives/rdr3_nativedb.json.gz"),
        }
    }

    pub fn bundled_fingerprint(self) -> String {
        let gz = self.bundled_gz();
        let word = |at: usize| gz.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let n = gz.len().saturating_sub(8);
        format!("{:08x}:{}", word(n), word(n + 4))
    }

    pub fn bundled_text(self) -> Result<String, String> {
        let mut bytes = Vec::with_capacity(4 << 20);
        GzDecoder::new(self.bundled_gz()).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", self.file_name()))?;
        Ok(decode(bytes))
    }
}

pub fn fingerprint(bytes: &[u8]) -> String {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    format!("{:08x}:{}", crc.sum(), bytes.len() as u32)
}

pub fn fingerprint_len(fingerprint: &str) -> usize {
    fingerprint.rsplit_once(':').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

pub fn decode(bytes: Vec<u8>) -> String {
    let text = String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

pub fn databases(set: NativeSet) -> &'static [Database] {
    match set {
        NativeSet::Gta5 => &[Database::Gta5, Database::Cfx],
        NativeSet::Rdr3 => &[Database::Rdr3, Database::Rdr3Docs, Database::Cfx],
        NativeSet::Server => &[Database::Cfx],
    }
}

pub type Inputs = BTreeMap<String, String>;

pub fn bundled_inputs(set: NativeSet) -> Inputs {
    databases(set).iter().map(|d| (d.id().to_owned(), d.bundled_fingerprint())).collect()
}

pub fn snapshot_id(set: NativeSet) -> String {
    let inputs: Vec<String> = bundled_inputs(set).into_iter().map(|(id, fp)| format!("{id}={fp}")).collect();
    format!("v{CONVERTER_VERSION};overrides={};{}", fingerprint(OVERRIDES.as_bytes()), inputs.join(";"))
}

fn json(d: Database, text: &str, origin: Origin) -> Result<Vec<RawNative<'_>>, (Database, FormatError)> {
    parse::parse_json(text, origin).map_err(|e| (d, e))
}

pub fn groups<'a>(
    set: NativeSet,
    text: impl Fn(Database) -> Option<&'a str>,
) -> Result<Vec<Group>, (Database, FormatError)> {
    let get = |d: Database| text(d).ok_or_else(|| (d, FormatError("not available".into())));
    let cfx = get(Database::Cfx)?;
    let overrides = Overrides::bundled();
    match set {
        NativeSet::Gta5 => {
            let mut raw = json(Database::Gta5, get(Database::Gta5)?, Origin::Gta5Json)?;
            raw.extend(json(Database::Cfx, cfx, Origin::CfxJson)?);
            Ok(convert::convert(set, raw, &overrides))
        }
        NativeSet::Rdr3 => {
            let dsl = parse::parse_dsl(get(Database::Rdr3)?).map_err(|e| (Database::Rdr3, e))?;
            let docs = parse::parse_nativedb(get(Database::Rdr3Docs)?).map_err(|e| (Database::Rdr3Docs, e))?;
            let mut raw = parse::join_rdr3(dsl, &docs);
            raw.extend(json(Database::Cfx, cfx, Origin::CfxJson)?);
            Ok(convert::convert(set, raw, &overrides))
        }
        NativeSet::Server => Ok(convert::convert(set, json(Database::Cfx, cfx, Origin::CfxJson)?, &overrides)),
    }
}

pub fn bundled_groups(set: NativeSet) -> Vec<Group> {
    let texts: HashMap<Database, String> =
        databases(set).iter().filter_map(|&d| Some((d, d.bundled_text().ok()?))).collect();
    groups(set, |d| texts.get(&d).map(String::as_str)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn text(d: Database) -> String {
        d.bundled_text().unwrap()
    }

    #[test]
    #[ignore = "lists the databases for scripts/update-natives.sh"]
    fn database_list() {
        for d in Database::ALL {
            println!("natives-db {} {}", d.file_name(), d.url());
        }
    }

    #[test]
    fn snapshot_inventory() {
        let gta5 = text(Database::Gta5);
        let natives = parse::parse_json(&gta5, Origin::Gta5Json).unwrap();
        let namespaces: HashSet<&str> = natives.iter().map(|n| n.namespace.as_ref()).collect();
        assert!(
            natives.len() > 6000 && namespaces.len() > 40,
            "{} natives, {} namespaces",
            natives.len(),
            namespaces.len()
        );
        assert!(natives.iter().filter(|n| n.name.is_none()).count() > 500);
        assert!(natives.iter().filter(|n| !n.aliases.is_empty()).count() > 2000);
        assert!(natives.iter().filter(|n| n.lua_example.is_some()).count() > 200);

        let cfx = text(Database::Cfx);
        let natives = parse::parse_json(&cfx, Origin::CfxJson).unwrap();
        let count = |f: &dyn Fn(&RawNative) -> bool| natives.iter().filter(|n| f(n)).count();
        assert!(natives.len() > 900, "{}", natives.len());
        let apisets =
            [count(&|n| n.apiset == "client"), count(&|n| n.apiset == "server"), count(&|n| n.apiset == "shared")];
        assert!(apisets.iter().all(|&n| n > 50) && apisets.iter().sum::<usize>() == natives.len(), "{apisets:?}");
        assert!(count(&|n| n.game.is_none()) > 400);

        let dsl_text = text(Database::Rdr3);
        let dsl = parse::parse_dsl(&dsl_text).unwrap();
        assert!(dsl.len() > 7000, "{}", dsl.len());
        assert!(dsl.iter().all(|n| n.hash.is_some() && n.namespace.is_some() && n.returns.is_some()));
        assert_eq!(dsl.iter().filter_map(|n| n.hash).collect::<HashSet<_>>().len(), dsl.len(), "unique hashes");
        assert!(dsl.iter().filter(|n| n.name.starts_with("0x")).count() > 1000);
        assert!(dsl.iter().map(|n| n.aliases.len()).sum::<usize>() > 2000);
        assert!(dsl.iter().any(|n| n.aliases.len() > 1));

        let docs_text = text(Database::Rdr3Docs);
        let docs = parse::parse_nativedb(&docs_text).unwrap();
        assert!(docs.len() > 7000, "{}", docs.len());
        let missing =
            dsl.iter().filter_map(|n| n.hash).filter(|h| !docs.contains_key(&parse::normalize_hash(h))).count();
        assert!(missing < dsl.len() / 50, "{missing} natives without documentation");
        let commented = dsl
            .iter()
            .filter_map(|n| docs.get(&parse::normalize_hash(n.hash?)))
            .filter(|d| !d.comment.trim().is_empty())
            .count();
        assert!(commented > 2000, "{commented}");
        let joined = parse::join_rdr3(dsl, &docs);
        assert!(joined.iter().filter(|n| n.alt_name.is_some()).count() > 200);
    }

    #[test]
    fn fingerprints_and_snapshot() {
        for d in Database::ALL {
            let mut raw = Vec::new();
            GzDecoder::new(d.bundled_gz()).read_to_end(&mut raw).unwrap();
            assert_eq!(fingerprint(&raw), d.bundled_fingerprint(), "{}", d.file_name());
            assert_eq!(fingerprint_len(&d.bundled_fingerprint()), raw.len());
        }
        assert_eq!(decode(b"\xef\xbb\xbf{}".to_vec()), "{}");
        assert_eq!(decode(b"a\xffb".to_vec()), "a\u{fffd}b");
        let id = snapshot_id(NativeSet::Rdr3);
        assert!(id.starts_with("v1;overrides=") && id.contains(";cfx=") && id.contains(";rdr3_docs="), "{id}");
        assert!(!snapshot_id(NativeSet::Server).contains("gta5="));
        let html = groups(NativeSet::Server, |_| Some("<html></html>")).map(|g| g.len());
        assert_eq!(html.map_err(|(d, e)| (d, e.0.starts_with("invalid JSON"))), Err((Database::Cfx, true)));
        let overrides = Overrides::bundled();
        assert_eq!(overrides.return_types.len(), 17);
        assert_eq!(overrides.param_types["REGISTER_RAW_KEYMAP"]["onKeyUp"], "fun()|nil");
        assert_eq!(overrides.string_param_types["playerSrc"], "string|integer");
    }
}
