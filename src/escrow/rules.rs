//! Asset Escrow's rules: which files of a resource it encrypts (Lua, YFT, YDD
//! and YDR files that `escrow_ignore` does not match), the files buyers expect
//! to edit (configuration, translations), and the files scripts load from
//! their path at runtime (`LoadResourceFile`, ox_lib's module loaders).

use crate::lua::Source;
use crate::manifest::files::normalize;
use crate::project::{Resource, is_manifest_name};
use crate::text::file_name;
use crate::workspace::Workspace;

/// Extensions escrow encrypts.
const ENCRYPTED_EXTENSIONS: [&str; 4] = ["lua", "yft", "ydd", "ydr"];
/// NUI files, which escrow never encrypts.
pub const NUI_EXTENSIONS: [&str; 5] = ["html", "htm", "js", "mjs", "css"];
/// First bytes of a file escrow encrypted.
const MAGIC: &str = "FXAP";
/// The key file of a protected build, in the resource directory.
const KEY_FILE: &str = ".fxap";
const SKIPPED_DIRS: [&str; 5] = ["node_modules", ".git", ".svn", ".idea", ".vscode"];
const MAX_FILES: usize = 20_000;
const CONFIG_NAMES: [&str; 5] = ["config", "configs", "cfg", "conf", "settings"];
const CONFIG_DIRS: [&str; 5] = ["config", "configs", "configuration", "cfg", "settings"];
const LOCALE_DIRS: [&str; 8] = ["locales", "locale", "lang", "langs", "languages", "language", "translations", "i18n"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Encrypted,
    /// Listed in `escrow_ignore`.
    Ignored,
    /// A type escrow does not encrypt (or a manifest).
    Unprotected,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Encrypted => "escrow: encrypted",
            Self::Ignored | Self::Unprotected => "escrow: open",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Encrypted => "Asset Escrow encrypts this file: buyers can neither read nor edit it",
            Self::Ignored => "Listed in escrow_ignore: shipped readable and editable",
            Self::Unprotected => "Asset Escrow does not encrypt this file type: shipped readable",
        }
    }
}

/// The text after the last `.` of a name, lowercased (`""` without one).
pub fn extension(name: &str) -> String {
    file_name(name).rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// A Lua file escrow already encrypted (its text starts with `FXAP`).
pub fn is_protected(text: &str) -> bool {
    text.starts_with(MAGIC)
}

pub fn has_key_file(ws: &Workspace, res: &Resource) -> bool {
    ws.exists(&format!("{}/{KEY_FILE}", res.dir))
}

/// The resource declares `escrow_ignore` or holds a `.fxap` key.
pub fn uses_escrow(ws: &Workspace, res: &Resource) -> bool {
    res.escrow_ignore.is_some() || has_key_file(ws, res)
}

/// Status of the resource file `rel` (relative to the resource directory).
pub fn status(res: &Resource, rel: &str) -> Status {
    let name = file_name(rel);
    if is_manifest_name(name) || !ENCRYPTED_EXTENSIONS.contains(&extension(name).as_str()) {
        Status::Unprotected
    } else if res.is_escrow_ignored(rel) {
        Status::Ignored
    } else {
        Status::Encrypted
    }
}

/// A file of a resource.
#[derive(Clone, Debug)]
pub struct EscrowFile {
    pub path: String,
    pub rel: String,
    pub status: Status,
}

/// The files escrow packs, sorted by path: nested resources, tool folders and the key file left out.
pub fn files_of(ws: &Workspace, res: &Resource) -> Vec<EscrowFile> {
    let prefix = format!("{}/", res.dir);
    let nested: Vec<String> =
        ws.resources().filter(|r| r.dir.starts_with(&prefix)).map(|r| format!("{}/", r.dir)).collect();
    ws.files_under(&res.dir)
        .filter(|f| !nested.iter().any(|n| f.path.starts_with(n)))
        .filter_map(|f| {
            let rel = f.path.strip_prefix(&prefix)?;
            let mut segs: Vec<&str> = rel.split('/').collect();
            let name = segs.pop()?;
            if name == KEY_FILE || segs.iter().any(|s| SKIPPED_DIRS.iter().any(|d| s.eq_ignore_ascii_case(d))) {
                return None;
            }
            Some(EscrowFile { path: f.path.clone(), rel: rel.to_owned(), status: status(res, rel) })
        })
        .take(MAX_FILES)
        .collect()
}

/// The `escrow_ignore` pattern that keeps a configuration or translation file open, `None` for other files.
pub fn editable_pattern(rel: &str) -> Option<String> {
    if extension(rel) != "lua" {
        return None;
    }
    let segs: Vec<&str> = rel.split('/').collect();
    let dirs = &segs[..segs.len() - 1];
    let dir_pattern = |i: usize| {
        let dir = dirs[..=i].join("/");
        if i + 1 == dirs.len() { format!("{dir}/*.lua") } else { format!("{dir}/**/*.lua") }
    };
    let in_dirs = |names: &[&str]| dirs.iter().position(|d| names.contains(&d.to_ascii_lowercase().as_str()));
    if let Some(i) = in_dirs(&LOCALE_DIRS).or_else(|| in_dirs(&CONFIG_DIRS)) {
        return Some(dir_pattern(i));
    }
    let name = segs[segs.len() - 1];
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    stem.split(['_', '.', '-'])
        .any(|w| CONFIG_NAMES.contains(&w.to_ascii_lowercase().as_str()))
        .then(|| rel.to_owned())
}

/// Encrypted configuration and translation files, grouped by the pattern that keeps them open (first appearance first).
pub fn editable_groups(files: &[EscrowFile]) -> Vec<(String, Vec<&EscrowFile>)> {
    let mut out: Vec<(String, Vec<&EscrowFile>)> = Vec::new();
    for f in files.iter().filter(|f| f.status == Status::Encrypted) {
        let Some(pattern) = editable_pattern(&f.rel) else { continue };
        match out.iter_mut().find(|(p, _)| *p == pattern) {
            Some((_, group)) => group.push(f),
            None => out.push((pattern, vec![f])),
        }
    }
    out
}

/// Whose file `LoadResourceFile` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Owner {
    /// `GetCurrentResourceName()`, ox_lib's `cache.resource` (and every module loader).
    This,
    /// A resource named by a string.
    Named(String),
}

/// A file loaded from its path at runtime.
#[derive(Clone, Debug)]
pub struct Load {
    /// `None` for `LoadResourceFile`, else the module loader as written (`require`, `lib.require`, `lib.load`).
    pub module: Option<&'static str>,
    owner: Owner,
    /// The path or module name.
    pub value: String,
    /// The literal's byte span.
    pub start: usize,
    pub end: usize,
}

impl Load {
    /// Paths relative to the resource named `resource`, in lookup order.
    pub fn candidates(&self, resource: &str) -> Vec<String> {
        if self.module.is_none() {
            if matches!(&self.owner, Owner::Named(n) if n != resource) {
                return Vec::new();
            }
            let p = normalize(&self.value);
            return if p.is_empty() { Vec::new() } else { vec![p] };
        }
        let mut name = self.value.trim();
        if let Some(rest) = name.strip_prefix('@') {
            let Some(end) = rest.find(['.', '/']) else { return Vec::new() };
            if &rest[..end] != resource {
                return Vec::new();
            }
            name = &rest[end + 1..];
        }
        let p = normalize(&name.replace('.', "/"));
        if p.is_empty() {
            return Vec::new();
        }
        vec![format!("{p}.lua"), format!("{p}/init.lua")]
    }
}

/// The loads of a Lua source: `LoadResourceFile(<this resource>, 'path')`, `require 'm'`, `lib.require 'm'`, `lib.load 'm'`.
pub fn loads(src: &Source) -> Vec<Load> {
    // Most scripts load nothing: skip the call scan (it runs on every new version of the edited file).
    if !src.text.contains("LoadResourceFile") && !src.text.contains("require") && !src.text.contains("lib.load") {
        return Vec::new();
    }
    let chains = src.chains();
    let mut out = Vec::new();
    for call in src.calls(&chains) {
        let (module, owner, arg) = match call.callee.as_str() {
            "LoadResourceFile" if call.args.len() >= 2 => {
                let Some(owner) = owner(src, call.args[0]) else { continue };
                (None, owner, call.args[1])
            }
            callee @ ("require" | "lib.require" | "lib.load") => {
                let Some(&arg) = call.args.first() else { continue };
                let module = match callee {
                    "require" => "require",
                    "lib.require" => "lib.require",
                    _ => "lib.load",
                };
                (Some(module), Owner::This, arg)
            }
            _ => continue,
        };
        let Some(tok) = src.string_arg(arg) else { continue };
        let t = src.toks[tok];
        out.push(Load { module, owner, value: t.string_value(&src.text), start: t.start, end: t.end });
    }
    out
}

/// The owner argument of `LoadResourceFile`, when it names a resource.
fn owner(src: &Source, (a, b): (usize, usize)) -> Option<Owner> {
    if let Some(tok) = src.string_arg((a, b)) {
        return Some(Owner::Named(src.toks[tok].string_value(&src.text)));
    }
    let texts: Vec<&str> = (a..b).map(|i| src.tok_text(i)).collect();
    matches!(texts.as_slice(), ["GetCurrentResourceName", "(", ")"] | ["cache", ".", "resource"]).then_some(Owner::This)
}

/// The resource file a load reads (as written in the project) and its status.
pub fn target(ws: &Workspace, res: &Resource, load: &Load) -> Option<(String, Status)> {
    let prefix = format!("{}/", res.dir);
    load.candidates(&res.name).into_iter().find_map(|c| {
        let rel = if ws.exists(&format!("{prefix}{c}")) {
            c
        } else {
            ws.files_under(&res.dir).filter_map(|f| f.path.strip_prefix(&prefix)).find(|r| r.eq_ignore_ascii_case(&c))?.to_owned()
        };
        let status = status(res, &rel);
        Some((rel, status))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        let ws = Workspace::from_files(
            "/p",
            &[("res/fxmanifest.lua", "escrow_ignore { 'config.lua', 'locales/*.lua', 'stream/**/*.yft', 'client/open_*.lua' }")],
        );
        let res = ws.resource("/p/res").unwrap();
        let cases = [
            ("config.lua", Status::Ignored),
            ("client/main.lua", Status::Encrypted),
            ("client/open_api.lua", Status::Ignored),
            ("locales/en.lua", Status::Ignored),
            ("locales/extra/fr.lua", Status::Encrypted),
            ("stream/car.yft", Status::Ignored),
            ("stream/parts/wheel.yft", Status::Ignored),
            ("stream/car.ydr", Status::Encrypted),
            ("stream/car.ytd", Status::Unprotected),
            ("html/index.html", Status::Unprotected),
            ("data/items.json", Status::Unprotected),
            ("fxmanifest.lua", Status::Unprotected),
            ("sub/__resource.lua", Status::Unprotected),
        ];
        for (rel, want) in cases {
            assert_eq!(status(res, rel), want, "{rel}");
        }
        assert_eq!(Status::Ignored.label(), "escrow: open");
    }

    #[test]
    fn editable_files() {
        for (rel, want) in [
            ("config.lua", Some("config.lua")),
            ("sh_config.lua", Some("sh_config.lua")),
            ("settings.lua", Some("settings.lua")),
            ("config.example.lua", Some("config.example.lua")),
            ("cfg.lua", Some("cfg.lua")),
            ("configure.lua", None),
            ("myconfig.lua", None),
            ("config.json", None),
            ("locales/en.lua", Some("locales/*.lua")),
            ("config/jobs/police.lua", Some("config/**/*.lua")),
            ("shared/Lang/fr.lua", Some("shared/Lang/*.lua")),
        ] {
            assert_eq!(editable_pattern(rel).as_deref(), want, "{rel}");
        }
    }

    #[test]
    fn files_skip_nested_resources_and_tools() {
        let ws = Workspace::from_files(
            "/p",
            &[
                ("res/fxmanifest.lua", ""),
                ("res/.fxap", ""),
                ("res/client/main.lua", ""),
                ("res/node_modules/x/a.js", ""),
                ("res/inner/fxmanifest.lua", ""),
                ("res/inner/x.lua", ""),
            ],
        );
        let files = files_of(&ws, ws.resource("/p/res").unwrap());
        assert_eq!(files.iter().map(|f| f.rel.as_str()).collect::<Vec<_>>(), ["client/main.lua", "fxmanifest.lua"]);
    }

    #[test]
    fn loaders() {
        let src = Source::new(
            "load(LoadResourceFile(GetCurrentResourceName(), 'data/items.lua'))()\nLoadResourceFile('res', './data/items.lua')\n\
             LoadResourceFile(cache.resource, 'a.lua')\nLoadResourceFile(other, 'b.lua')\nLoadResourceFile('other_resource', 'c.lua')\n\
             require 'modules.utils'\nlib.require('@res.modules.menu')\nlib.load '@res/modules/utils'\nrequire '@ox_core.modules.utils'\nx.require 'y'\n",
        );
        let all: Vec<(Option<&str>, Vec<String>)> = loads(&src).iter().map(|l| (l.module, l.candidates("res"))).collect();
        let c = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            all,
            [
                (None, c(&["data/items.lua"])),
                (None, c(&["data/items.lua"])),
                (None, c(&["a.lua"])),
                (None, c(&[])),
                (Some("require"), c(&["modules/utils.lua", "modules/utils/init.lua"])),
                (Some("lib.require"), c(&["modules/menu.lua", "modules/menu/init.lua"])),
                (Some("lib.load"), c(&["modules/utils.lua", "modules/utils/init.lua"])),
                (Some("require"), c(&[])),
            ]
        );
    }
}
