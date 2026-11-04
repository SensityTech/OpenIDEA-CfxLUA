//! Asset Escrow: which files are encrypted, checks, the pre-upload report.
//!
//! In a resource that uses escrow (it declares `escrow_ignore` or holds a
//! `.fxap` key; every resource with `escrow.assumeEscrow`): a hint at the top
//! of each Lua file says whether escrow encrypts it (the Project view badges
//! of the original are not possible), the manifest reports the configuration
//! and translation files escrow would encrypt and the scripts it cannot
//! protect, and Lua files report the files they load at runtime that escrow
//! would encrypt. Files escrow already encrypted (`FXAP…`) get a notice only.

mod check;
mod edits;
mod rules;

use crate::feature::{CommandCx, CommandResult, Ctx, Feature, Fix, Intention, Problem};
use crate::lua::Source;
use crate::manifest::view::View;
use crate::project::glob::Glob;
use crate::project::{CASE_SENSITIVE, FileKind, Resource};
use crate::text::{file_name, parent};
use crate::workspace::Workspace;
use openidea_plugin::Severity;
use rules::{Load, Status};
use std::cell::RefCell;
use std::collections::HashMap;

/// Settings keys.
pub mod keys {
    /// Show whether escrow encrypts each Lua file (a hint at the top of the file).
    pub const SHOW_STATUS: &str = "escrow.showStatus";
    /// Treat every resource as escrowed.
    pub const ASSUME_ESCROW: &str = "escrow.assumeEscrow";
}

pub const EDITABLE: &str = "CfxEscrowEditableFile";
pub const LOADED: &str = "CfxEscrowLoadedFile";
pub const UNPROTECTED: &str = "CfxEscrowUnprotectedScript";
pub const STATUS: &str = "CfxEscrowStatus";
pub const PROTECTED: &str = "CfxEscrowProtected";

const SCRIPT_DIRECTIVES: [&str; 6] = ["client_script", "client_scripts", "server_script", "server_scripts", "shared_script", "shared_scripts"];

#[derive(Default)]
pub struct EscrowFeature {
    /// The runtime loads of each Lua file that has some (for the manifest's "add every pattern" action).
    loads: RefCell<HashMap<String, Vec<Load>>>,
}

/// `Add 'x' to escrow_ignore`.
fn add_title(pattern: &str) -> String {
    format!("Add '{pattern}' to escrow_ignore")
}

/// The resource `cx`'s document belongs to, when escrow applies to it.
fn escrow_resource<'a>(cx: &Ctx<'a>) -> Option<&'a Resource> {
    let res = cx.ws.resource_of(&cx.doc.path)?;
    (cx.settings.bool(keys::ASSUME_ESCROW, false) || rules::uses_escrow(cx.ws, res)).then_some(res)
}

/// Whether `path` is the manifest FXServer reads in its folder (not a `__resource.lua` beside an `fxmanifest.lua`).
fn is_main_manifest(ws: &Workspace, path: &str) -> bool {
    !file_name(path).eq_ignore_ascii_case("__resource.lua") || !ws.exists(&format!("{}/fxmanifest.lua", parent(path)))
}

impl Feature for EscrowFeature {
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        if kind != FileKind::Lua {
            return;
        }
        let loads = if rules::is_protected(&src.text) { Vec::new() } else { rules::loads(src) };
        let mut map = self.loads.borrow_mut();
        if loads.is_empty() {
            map.remove(path);
        } else {
            map.insert(path.to_owned(), loads);
        }
    }

    fn forget_file(&self, path: &str) {
        self.loads.borrow_mut().remove(path);
    }

    fn command(&self, cx: &CommandCx, id: &str, _arg: Option<&str>) -> Option<CommandResult> {
        (id == check::COMMAND).then(|| check::run(cx))
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        let doc = cx.doc;
        if !cx.settings.enabled() || !matches!(doc.kind, FileKind::Lua | FileKind::Manifest) {
            return Vec::new();
        }
        if doc.kind == FileKind::Lua && rules::is_protected(doc.text()) {
            let message = "This file is protected by Cfx Asset Escrow: its content is encrypted, only FXServer can decrypt it. Sensity DevKit does not parse it.";
            return vec![Problem::new(PROTECTED, Severity::Info, 0, 4, message)];
        }
        let Some(res) = escrow_resource(cx) else { return Vec::new() };
        let mut out = Vec::new();
        if cx.settings.bool(keys::SHOW_STATUS, true)
            && let Some(rel) = res.relative(&doc.path)
        {
            let status = rules::status(res, rel);
            out.push(Problem::new(STATUS, Severity::Hint, 0, 0, format!("{} ({})", status.label(), status.tooltip())));
        }
        if doc.is_manifest() {
            if parent(&doc.path) == res.dir && is_main_manifest(cx.ws, &doc.path) {
                let view = View::new(&doc.src);
                editable_files(cx, res, &view, &mut out);
                unprotected_scripts(&view, &mut out);
            }
        } else {
            loaded_files(cx, res, &mut out);
        }
        out
    }

    fn intentions(&self, cx: &Ctx, _start: usize, _end: usize) -> Vec<Intention> {
        let doc = cx.doc;
        if !cx.settings.enabled() || !doc.is_manifest() || !is_main_manifest(cx.ws, &doc.path) {
            return Vec::new();
        }
        let Some(res) = escrow_resource(cx).filter(|r| r.dir == parent(&doc.path)) else { return Vec::new() };
        let patterns = self.patterns(cx.ws, res);
        let Some((start, end, text)) = edits::pattern_edit(doc.text(), &patterns) else { return Vec::new() };
        let title = match patterns.len() {
            1 => "Add 1 pattern to escrow_ignore".to_owned(),
            n => format!("Add {n} patterns to escrow_ignore"),
        };
        vec![Intention { fix: Fix::new(title).replace(start, end, text), kind: "quickfix" }]
    }
}

impl EscrowFeature {
    /// The patterns the pre-upload check would add: encrypted configuration and
    /// translation files, then encrypted files loaded at runtime (distinct, in that order).
    fn patterns(&self, ws: &Workspace, res: &Resource) -> Vec<String> {
        let files = rules::files_of(ws, res);
        let mut out: Vec<String> = rules::editable_groups(&files).into_iter().map(|(p, _)| p).collect();
        let loads = self.loads.borrow();
        for f in files.iter().filter(|f| rules::extension(&f.rel) == "lua") {
            for load in loads.get(&f.path).into_iter().flatten() {
                if let Some((target, Status::Encrypted)) = rules::target(ws, res, load)
                    && !out.contains(&target)
                {
                    out.push(target);
                }
            }
        }
        out
    }
}

/// Configuration and translation files escrow would encrypt, one problem per `escrow_ignore` pattern to add.
fn editable_files(cx: &Ctx, res: &Resource, view: &View, out: &mut Vec<Problem>) {
    let text = cx.doc.text();
    let files = rules::files_of(cx.ws, res);
    for (pattern, group) in rules::editable_groups(&files) {
        let message = if pattern == group[0].rel {
            format!("'{pattern}' will be encrypted by Asset Escrow: buyers will not be able to edit it")
        } else if group.len() == 1 {
            format!("'{pattern}' matches a file that will be encrypted by Asset Escrow: buyers will not be able to edit it")
        } else {
            format!("'{pattern}' matches {} files that will be encrypted by Asset Escrow: buyers will not be able to edit them", group.len())
        };
        // The value listing one of the files, else `escrow_ignore`, else the first directive, else the file.
        let listing = view.decls.iter().filter(|d| d.name() != "escrow_ignore").flat_map(|d| d.values()).find(|v| {
            Glob::new(&v.text, CASE_SENSITIVE).is_some_and(|g| group.iter().any(|f| g.matches(&f.rel)))
        });
        let (start, end) = match listing {
            Some(v) => (v.start, v.end),
            None => view
                .first_named("escrow_ignore")
                .or(view.decls.first())
                .map_or((0, text.len()), |d| (d.entry.name_start, d.entry.name_end)),
        };
        let mut p = Problem::new(EDITABLE, Severity::Warning, start, end, message);
        if let Some((s, e, t)) = edits::pattern_edit(text, std::slice::from_ref(&pattern)) {
            p = p.fix(Fix::new(add_title(&pattern)).replace(s, e, t));
        }
        out.push(p);
    }
}

/// JavaScript and C# scripts, which escrow ships as they are.
fn unprotected_scripts(view: &View, out: &mut Vec<Problem>) {
    for v in view.decls.iter().filter(|d| SCRIPT_DIRECTIVES.contains(&d.name())).flat_map(|d| d.values()) {
        if v.text.starts_with('@') {
            continue;
        }
        let message = match rules::extension(&v.text).as_str() {
            "js" => format!("Asset Escrow does not encrypt JavaScript: '{}' ships readable", v.text),
            "dll" => format!("Asset Escrow does not encrypt C# assemblies: '{}' ships as is", v.text),
            _ => continue,
        };
        out.push(Problem::new(UNPROTECTED, Severity::Info, v.start, v.end, message));
    }
}

/// Files of the resource the document loads at runtime that escrow would encrypt.
fn loaded_files(cx: &Ctx, res: &Resource, out: &mut Vec<Problem>) {
    let manifest_path = res.manifest_path();
    let manifest = cx.ws.text(&manifest_path);
    for load in rules::loads(&cx.doc.src) {
        let Some((target, Status::Encrypted)) = rules::target(cx.ws, res, &load) else { continue };
        let message = match load.module {
            Some(callee) => format!(
                "'{target}' will be encrypted by Asset Escrow: {callee} reads it with LoadResourceFile and cannot load it in the protected build"
            ),
            None => format!("'{target}' will be encrypted by Asset Escrow: LoadResourceFile returns its encrypted bytes in the protected build"),
        };
        let mut p = Problem::new(LOADED, Severity::Warning, load.start, load.end, message);
        // The fix edits the manifest, another file than the problem's.
        if let Some(edit) = manifest.and_then(|m| edits::pattern_text_edit(m, std::slice::from_ref(&target))) {
            let mut fix = Fix::new(add_title(&target));
            fix.other.push((manifest_path.clone(), vec![edit]));
            p = p.fix(fix);
        }
        out.push(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::edits::apply;
    use crate::settings::Settings;
    use crate::testing::Fixture;
    use serde_json::Value;

    /// Problems of `rel` (code, severity, message, highlighted text).
    fn problems(fx: &Fixture, rel: &str) -> Vec<(&'static str, Severity, String, String)> {
        let doc = fx.doc(rel);
        let feature = EscrowFeature::default();
        fx.index(&feature);
        feature
            .problems(&fx.cx(&doc))
            .into_iter()
            .map(|p| (p.code, p.severity, p.message.clone(), doc.text()[p.start..p.end].to_owned()))
            .collect()
    }

    fn without_status(p: Vec<(&'static str, Severity, String, String)>) -> Vec<(&'static str, Severity, String, String)> {
        p.into_iter().filter(|p| p.0 != STATUS).collect()
    }

    fn fixed_manifest(fx: &Fixture, message: &str) -> String {
        let doc = fx.doc("res/fxmanifest.lua");
        let p = EscrowFeature::default().problems(&fx.cx(&doc));
        let p = p.iter().find(|p| p.message.starts_with(message)).unwrap_or_else(|| panic!("{message}: {p:?}"));
        apply(doc.text(), &p.fixes[0].edits)
    }

    #[test]
    fn editable_files_in_the_manifest() {
        let fx = Fixture::new(&[
            ("res/fxmanifest.lua", "shared_script 'config.lua'\nescrow_ignore {\n    'client/open.lua',\n}\n"),
            ("res/config.lua", ""),
            ("res/client/open.lua", ""),
        ]);
        let p = without_status(problems(&fx, "res/fxmanifest.lua"));
        assert_eq!(
            p,
            [(EDITABLE, Severity::Warning, "'config.lua' will be encrypted by Asset Escrow: buyers will not be able to edit it".to_owned(), "'config.lua'".to_owned())]
        );
        assert_eq!(fixed_manifest(&fx, "'config.lua'"), "shared_script 'config.lua'\nescrow_ignore {\n    'client/open.lua',\n    'config.lua',\n}\n");

        let manifest = "shared_scripts { 'locales/*.lua', 'shared/*.lua', 'config/**/*.lua' }\nescrow_ignore 'client/main.lua'\n";
        let fx = Fixture::new(&[
            ("res/fxmanifest.lua", manifest),
            ("res/locales/en.lua", ""),
            ("res/locales/fr.lua", ""),
            ("res/shared/sh_config.lua", ""),
            ("res/config/jobs/police.lua", ""),
            ("res/client/configure.lua", ""),
        ]);
        let p: Vec<(String, String)> = without_status(problems(&fx, "res/fxmanifest.lua")).into_iter().map(|p| (p.2, p.3)).collect();
        let want = [
            ("'config/**/*.lua' matches a file that will be encrypted by Asset Escrow: buyers will not be able to edit it", "'config/**/*.lua'"),
            ("'locales/*.lua' matches 2 files that will be encrypted by Asset Escrow: buyers will not be able to edit them", "'locales/*.lua'"),
            ("'shared/sh_config.lua' will be encrypted by Asset Escrow: buyers will not be able to edit it", "'shared/*.lua'"),
        ];
        assert_eq!(p, want.map(|(a, b)| (a.to_owned(), b.to_owned())));
        assert_eq!(
            fixed_manifest(&fx, "'locales/*.lua'"),
            manifest.replace("escrow_ignore 'client/main.lua'", "escrow_ignore { 'client/main.lua', 'locales/*.lua' }")
        );

        let fx = Fixture::new(&[("res/fxmanifest.lua", "fx_version 'cerulean'\nescrow_ignore {}\n"), ("res/settings.lua", "")]);
        let p = without_status(problems(&fx, "res/fxmanifest.lua"));
        assert_eq!(p[0].3, "escrow_ignore");
        assert_eq!(fixed_manifest(&fx, "'settings.lua'"), "fx_version 'cerulean'\nescrow_ignore { 'settings.lua' }\n");
    }

    #[test]
    fn only_in_escrowed_resources() {
        let files = [
            ("res/fxmanifest.lua", "fx_version 'cerulean'\nshared_script 'config.lua'\nclient_script 'ui.js'\n"),
            ("res/config.lua", ""),
            ("res/ui.js", ""),
        ];
        assert!(problems(&Fixture::new(&files), "res/fxmanifest.lua").is_empty());
        let fx = Fixture::new(&files).with_settings(Settings::defaults().with(keys::ASSUME_ESCROW, Value::Bool(true)));
        let p = without_status(problems(&fx, "res/fxmanifest.lua"));
        assert_eq!(p.len(), 2);
        assert_eq!((p[1].0, p[1].1, p[1].2.as_str()), (UNPROTECTED, Severity::Info, "Asset Escrow does not encrypt JavaScript: 'ui.js' ships readable"));
        assert_eq!(
            fixed_manifest(&fx, "'config.lua'"),
            "fx_version 'cerulean'\nshared_script 'config.lua'\nclient_script 'ui.js'\n\nescrow_ignore {\n    'config.lua',\n}\n"
        );
        let fx = Fixture::new(&[
            ("res/fxmanifest.lua", "escrow_ignore {}\nserver_scripts { 'server/Bank.net.dll', '@x/y.js' }\n"),
            ("res/.fxap", ""),
        ]);
        let p = without_status(problems(&fx, "res/fxmanifest.lua"));
        assert_eq!(p.iter().map(|p| p.2.as_str()).collect::<Vec<_>>(), ["Asset Escrow does not encrypt C# assemblies: 'server/Bank.net.dll' ships as is"]);
    }

    #[test]
    fn status_hints() {
        let fx = Fixture::new(&[
            ("res/fxmanifest.lua", "escrow_ignore { 'config.lua' }\n"),
            ("res/config.lua", ""),
            ("res/client.lua", ""),
            ("plain/fxmanifest.lua", ""),
            ("plain/client.lua", ""),
        ]);
        let status = |rel: &str| problems(&fx, rel).into_iter().find(|p| p.0 == STATUS).map(|p| (p.1, p.2));
        assert_eq!(
            status("res/client.lua"),
            Some((Severity::Hint, "escrow: encrypted (Asset Escrow encrypts this file: buyers can neither read nor edit it)".to_owned()))
        );
        assert_eq!(status("res/config.lua").unwrap().1, "escrow: open (Listed in escrow_ignore: shipped readable and editable)");
        assert_eq!(status("res/fxmanifest.lua").unwrap().1, "escrow: open (Asset Escrow does not encrypt this file type: shipped readable)");
        assert_eq!(status("plain/client.lua"), None);
        let fx = Fixture::new(&[("res/fxmanifest.lua", "escrow_ignore {}"), ("res/client.lua", "")])
            .with_settings(Settings::defaults().with(keys::SHOW_STATUS, Value::Bool(false)));
        assert!(problems(&fx, "res/client.lua").is_empty());
    }

    #[test]
    fn protected_files() {
        let fx = Fixture::new(&[("res/fxmanifest.lua", "escrow_ignore {}"), ("res/client.lua", "FXAP\u{1}LoadResourceFile(x)"), ("res/b.lua", "-- FXAP")]);
        let p = problems(&fx, "res/client.lua");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].0, p[0].1, p[0].3.as_str()), (PROTECTED, Severity::Info, "FXAP"));
        assert!(problems(&fx, "res/b.lua").iter().all(|p| p.0 != PROTECTED));
    }

    const LOADING: &str = "local items = load(LoadResourceFile(GetCurrentResourceName(), 'data/items.lua'))()\n\
        LoadResourceFile(GetCurrentResourceName(), 'data/items.json')\nLoadResourceFile('res', './data/items.lua')\n\
        LoadResourceFile(GetCurrentResourceName(), 'config.lua')\nLoadResourceFile('other_resource', 'data/items.lua')\n\
        LoadResourceFile(GetCurrentResourceName(), 'missing.lua')\nrequire 'modules.utils'\nlib.require('modules.open')\n\
        lib.require('@res.modules.menu')\nlib.load '@res/modules/utils'\nrequire '@ox_core.modules.utils'\nrequire 'json'\n";

    fn loading_fixture() -> Fixture {
        Fixture::new(&[
            ("res/fxmanifest.lua", "escrow_ignore {\n    'config.lua', -- edited by buyers\n    'modules/open.lua',\n}\n"),
            ("res/client/main.lua", LOADING),
            ("res/config.lua", ""),
            ("res/data/items.lua", ""),
            ("res/data/items.json", ""),
            ("res/modules/utils.lua", ""),
            ("res/modules/open.lua", ""),
            ("res/modules/menu/init.lua", ""),
        ])
    }

    #[test]
    fn loaded_files_in_scripts() {
        let fx = loading_fixture();
        let p: Vec<(String, String)> = without_status(problems(&fx, "res/client/main.lua")).into_iter().map(|p| (p.2, p.3)).collect();
        let file = "will be encrypted by Asset Escrow: LoadResourceFile returns its encrypted bytes in the protected build";
        let module = |callee: &str| format!("will be encrypted by Asset Escrow: {callee} reads it with LoadResourceFile and cannot load it in the protected build");
        assert_eq!(
            p,
            [
                (format!("'data/items.lua' {file}"), "'data/items.lua'".to_owned()),
                (format!("'data/items.lua' {file}"), "'./data/items.lua'".to_owned()),
                (format!("'modules/utils.lua' {}", module("require")), "'modules.utils'".to_owned()),
                (format!("'modules/menu/init.lua' {}", module("lib.require")), "'@res.modules.menu'".to_owned()),
                (format!("'modules/utils.lua' {}", module("lib.load")), "'@res/modules/utils'".to_owned()),
            ]
        );
        let doc = fx.doc("res/client/main.lua");
        let problems = EscrowFeature::default().problems(&fx.cx(&doc));
        let fix = &problems.iter().find(|p| p.code == LOADED).unwrap().fixes[0];
        assert_eq!(fix.title, "Add 'data/items.lua' to escrow_ignore");
        assert!(fix.edits.is_empty());
        let (path, edits) = &fix.other[0];
        assert_eq!(path, "/p/res/fxmanifest.lua");
        let manifest = fx.ws.text(path).unwrap();
        let lines = crate::text::LineIndex::new(manifest);
        let (s, e) = (lines.offset(manifest, edits[0].range.start), lines.offset(manifest, edits[0].range.end));
        assert_eq!(
            apply(manifest, &[(s, e, edits[0].text.clone())]),
            "escrow_ignore {\n    'config.lua', -- edited by buyers\n    'modules/open.lua',\n    'data/items.lua',\n}\n"
        );
    }

    #[test]
    fn add_every_pattern() {
        let mut files = vec![
            ("res/fxmanifest.lua", "escrow_ignore {\n    'modules/open.lua',\n}\n"),
            ("res/client/main.lua", LOADING),
            ("res/locales/en.lua", ""),
        ];
        files.extend(loading_fixture_files());
        let fx = Fixture::new(&files);
        let feature = EscrowFeature::default();
        fx.index(&feature);
        let doc = fx.doc("res/fxmanifest.lua");
        let intentions = feature.intentions(&fx.cx(&doc), 0, 0);
        assert_eq!(intentions[0].fix.title, "Add 4 patterns to escrow_ignore");
        assert_eq!(
            apply(doc.text(), &intentions[0].fix.edits),
            "escrow_ignore {\n    'modules/open.lua',\n    'config.lua',\n    'locales/*.lua',\n    'data/items.lua',\n    'modules/utils.lua',\n}\n"
        );
    }

    #[test]
    fn check_command() {
        let host = openidea_plugin::mock::MockHost::install();
        let inv = host.invocation();
        let fx = loading_fixture();
        let run = |doc: Option<&crate::feature::Doc>, ws: Option<&Workspace>| {
            let cx = CommandCx { ws, settings: &fx.settings, natives: &fx.natives, doc, inv: &inv };
            EscrowFeature::default().command(&cx, "escrow.check", None).expect("owned").expect("runs")
        };
        let doc = fx.doc("res/client/main.lua");
        let report = run(Some(&doc), Some(&fx.ws)).expect("a report");
        assert!(report.starts_with("# Escrow pre-publish check: res\n"));
        let n = host.notifications();
        assert_eq!((n[0].level, n[0].title.as_str()), (openidea_plugin::NotificationLevel::Error, "Escrow check of res"));
        assert!(n[0].body.starts_with("2 blockers, 5 warnings\nBlockers\n"), "{}", n[0].body);
        assert!(run(None, None).is_none());
        assert_eq!(host.notifications()[1].title, "Escrow check: no resource (folder with an fxmanifest.lua) found");
        assert!(EscrowFeature::default().command(&CommandCx { ws: None, settings: &fx.settings, natives: &fx.natives, doc: None, inv: &inv }, "other", None).is_none());
    }

    fn loading_fixture_files() -> Vec<(&'static str, &'static str)> {
        vec![
            ("res/config.lua", ""),
            ("res/data/items.lua", ""),
            ("res/modules/utils.lua", ""),
            ("res/modules/open.lua", ""),
        ]
    }
}
