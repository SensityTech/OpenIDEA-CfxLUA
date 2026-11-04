//! The directive catalog (`data/docs/manifest/directives.json`) as completion
//! and the checks read it: the forms of each directive (`client_script` /
//! `client_scripts`), their one-line summary and value kind, and the role of
//! each string value (an enumeration, a path, a dependency).

use crate::docs::data::{Directive, directives};
use std::sync::OnceLock;

/// The acknowledgement FXServer requires, character for character, from RedM resources.
pub const RDR3_WARNING: &str = "I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.";
pub const CURRENT_FX_VERSION: &str = "cerulean";
/// Oldest first.
pub const FX_VERSIONS: [&str; 3] = ["adamant", "bodacious", "cerulean"];
/// Games FXServer accepts (`gta4`: LibertyM, never offered in completion).
pub const GAMES: [&str; 4] = ["gta5", "rdr3", "common", "gta4"];
/// Known but undocumented directives: never reported as unknown.
pub const EXTRA_KNOWN: [&str; 5] = ["file_set", "init_meta", "replace_traintrack_file", "is_cfxv2", "repository"];
pub const FX_VERSION_LINE: &str = "fx_version 'cerulean'";
/// Runtime constraints of `dependency` / `dependencies`, with their description.
pub const CONSTRAINTS: [(&str, &str); 7] = [
    ("/onesync", "OneSync must be enabled"),
    ("/server:", "minimum server build, e.g. /server:4500"),
    ("/gameBuild:", "minimum enforced game build (sv_enforceGameBuild values)"),
    ("/gameBuild:h4", "game build 2189 or newer"),
    ("/policy:", "a policy granted to the server key"),
    ("/policy:subdir_file_mapping", "clothing support granted to the server key"),
    ("/native:", "a native the server supports, e.g. /native:0xE27C97A0"),
];

/// `rdr3_warning '<the acknowledgement>'`.
pub fn rdr3_warning_line() -> String {
    format!("rdr3_warning '{RDR3_WARNING}'")
}

/// The directive `name` is a form of: `client_scripts` → `client_script`.
pub fn find(name: &str) -> Option<&'static Directive> {
    let all = directives();
    all.iter().find(|d| d.name == name).or_else(|| {
        let singular = name.strip_suffix('s')?;
        all.iter().find(|d| d.name == singular && d.plural)
    })
}

pub fn is_known(name: &str) -> bool {
    find(name).is_some() || EXTRA_KNOWN.contains(&name)
}

/// Whether `name` is the table form of a directive (FXServer strips the `s` of table arguments only).
pub fn is_table_form(name: &str) -> bool {
    if name == "dependencies" {
        return true;
    }
    if directives().iter().any(|d| d.name == name) {
        return false;
    }
    name.strip_suffix('s').is_some_and(|s| directives().iter().any(|d| d.name == s && d.plural))
}

/// `dependencies` → `dependency`, `client_scripts` → `client_script`.
pub fn singular_of(name: &str) -> &str {
    if name == "dependencies" { "dependency" } else { name.strip_suffix('s').unwrap_or(name) }
}

/// Every form of every directive, catalog order (singular before plural).
pub fn forms() -> &'static [(String, &'static Directive)] {
    static FORMS: OnceLock<Vec<(String, &'static Directive)>> = OnceLock::new();
    FORMS.get_or_init(|| {
        let mut out = Vec::new();
        for d in directives() {
            out.push((d.name.clone(), d));
            if d.plural {
                out.push((format!("{}s", d.name), d));
            }
        }
        out
    })
}

/// The description without backticks, up to its first sentence end (a `.` before whitespace or the end).
pub fn summary(d: &Directive) -> String {
    let text = d.description.replace('`', "");
    let bytes = text.as_bytes();
    let end = (0..bytes.len())
        .find(|&i| bytes[i] == b'.' && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace()))
        .unwrap_or(text.len());
    text[..end].to_owned()
}

/// What the value looks like: `'yes'`, `enum`, `path / glob`, `path / URL`, `list`, `flag`, `string`, …
pub fn value_kind(d: &Directive) -> String {
    let t = d.value_type.as_str();
    if let [v] = d.values.as_slice()
        && matches!(t, "enum" | "flag")
        && v.chars().count() <= 8
    {
        return format!("'{v}'");
    }
    match t {
        "glob" => "path / glob".into(),
        "path|url" => "path / URL".into(),
        "string[]" => "list".into(),
        _ => t.to_owned(),
    }
}

/// What a path value may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// `client_script(s)`, `server_script(s)`, `shared_script(s)`.
    Script,
    /// `file(s)`, `escrow_ignore`.
    File,
    /// `ui_page`, `loadscreen`.
    Page,
    /// The path of `data_file 'TYPE' 'path'`.
    DataFile,
}

impl PathKind {
    /// Extensions offered in completion (empty: every file).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Script => &["lua", "js", "dll"],
            Self::Page => &["html", "htm"],
            Self::File | Self::DataFile => &[],
        }
    }

    /// Whether a directory satisfies the path.
    pub fn directories(self) -> bool {
        matches!(self, Self::File | Self::DataFile)
    }

    /// Whether `@resource/path` names a file of another resource.
    pub fn other_resources(self) -> bool {
        self == Self::Script
    }

    /// Whether an absolute URL is allowed (and never checked).
    pub fn urls(self) -> bool {
        self == Self::Page
    }

    /// Whether a path without its extension matches (`audio/game.dat` → `audio/game.dat151.rel`).
    pub fn prefix(self) -> bool {
        self == Self::DataFile
    }
}

/// The role of a string value of a directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    None,
    Enum(&'static [String]),
    Path(PathKind),
    Dependency,
}

/// Role of a value of directive `name` in argument `arg` (0: the value, 1: the chained extra value).
pub fn role(name: &str, arg: usize) -> Role {
    let Some(d) = find(name) else { return Role::None };
    if d.name == "data_file" {
        return match arg {
            0 => Role::Enum(&d.values),
            1 => Role::Path(PathKind::DataFile),
            _ => Role::None,
        };
    }
    if arg != 0 {
        return Role::None;
    }
    match d.name.as_str() {
        "dependency" | "dependencies" => Role::Dependency,
        "client_script" | "server_script" | "shared_script" => Role::Path(PathKind::Script),
        "file" | "escrow_ignore" => Role::Path(PathKind::File),
        "ui_page" | "loadscreen" => Role::Path(PathKind::Page),
        _ if matches!(d.value_type.as_str(), "enum" | "flag") && !d.values.is_empty() => Role::Enum(&d.values),
        _ => Role::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(name: &str) -> &'static Directive {
        find(name).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn forms_and_lookups() {
        assert_eq!(forms().len(), 34 + 7, "34 directives, 7 with a plural form");
        assert_eq!(d("games").name, "game");
        assert_eq!(d("client_scripts").name, "client_script");
        assert!(find("escrow_ignores").is_none());
        assert!(find("authors").is_none());
        assert!(is_known("repository") && is_known("file_set") && !is_known("pizza_topping"));
        for name in ["client_scripts", "server_scripts", "shared_scripts", "files", "games", "exports", "server_exports", "dependencies"] {
            assert!(is_table_form(name), "{name}");
        }
        assert!(!is_table_form("client_script") && !is_table_form("escrow_ignore") && !is_table_form("dependency"));
        assert_eq!(singular_of("dependencies"), "dependency");
        assert_eq!(singular_of("games"), "game");
    }

    #[test]
    fn summaries_and_value_kinds() {
        assert_eq!(summary(d("fx_version")), "Required");
        assert_eq!(summary(d("game")), "Required with every fx_version");
        assert_eq!(summary(d("file")), "File(s) downloaded by clients without being executed: NUI pages and assets, data files, JSON read with LoadResourceFile");
        assert_eq!(
            summary(d("replace_level_meta")),
            "Replaces the level meta with the given file of the resource (name without the .meta extension)"
        );
        assert!(summary(d("convar_category")).ends_with("{ title, { { label, convar, type, default, .."));
        let kinds: Vec<(&str, String)> = ["fx_version", "rdr3_warning", "author", "client_script", "ui_page", "ui_page_preload", "nui_callback_strict_mode", "loadscreen", "data_file", "dependencies", "convar_category", "this_is_a_map"]
            .iter()
            .map(|n| (*n, value_kind(d(n))))
            .collect();
        let expect = ["enum", "enum", "string", "path / glob", "path / URL", "'yes'", "'true'", "path", "enum", "list", "table", "'yes'"];
        for ((name, kind), want) in kinds.iter().zip(expect) {
            assert_eq!(kind, want, "{name}");
        }
    }

    #[test]
    fn roles() {
        assert_eq!(role("client_scripts", 0), Role::Path(PathKind::Script));
        assert_eq!(role("files", 0), Role::Path(PathKind::File));
        assert_eq!(role("escrow_ignore", 0), Role::Path(PathKind::File));
        assert_eq!(role("ui_page", 0), Role::Path(PathKind::Page));
        assert_eq!(role("data_file", 1), Role::Path(PathKind::DataFile));
        assert!(matches!(role("data_file", 0), Role::Enum(v) if v.len() == 86));
        assert_eq!(role("dependencies", 0), Role::Dependency);
        assert!(matches!(role("games", 0), Role::Enum(v) if v == ["gta5", "rdr3", "common"]));
        assert!(matches!(role("lua54", 0), Role::Enum(_)));
        assert_eq!(role("replace_level_meta", 0), Role::None);
        assert_eq!(role("convar_category", 1), Role::None);
        assert_eq!(role("description", 0), Role::None);
        assert_eq!(role("pizza", 0), Role::None);
    }
}
