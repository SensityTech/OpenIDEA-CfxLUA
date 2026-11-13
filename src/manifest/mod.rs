pub mod catalog;
mod checks;
mod complete;
pub mod convert;
pub mod edits;
pub mod files;
pub mod generate;
mod snippets;
pub mod view;

use crate::feature::{Ctx, Feature, Problem};
use crate::text::parent;
use catalog::Role;
use files::{Tree, is_url, split_resource_path};
use openidea_plugin::providers::CompletionItem;
use openidea_plugin::{Location, Range};
use view::View;

#[derive(Default)]
pub struct ManifestFeature;

impl Feature for ManifestFeature {
    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !cx.settings.enabled() || !cx.doc.is_manifest() {
            return Vec::new();
        }
        complete::completion(cx, at)
    }

    fn definition(&self, cx: &Ctx, at: usize) -> Vec<Location> {
        if !cx.settings.enabled() || !cx.doc.is_manifest() {
            return Vec::new();
        }
        definition(cx, at)
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !cx.settings.enabled() || !cx.doc.is_manifest() {
            return Vec::new();
        }
        checks::problems(cx)
    }
}

fn definition(cx: &Ctx, at: usize) -> Vec<Location> {
    let src = &cx.doc.src;
    let Some(i) = src.tok_at(at).filter(|&i| src.toks[i].is_string()) else { return Vec::new() };
    let (start, end) = src.toks[i].content_range(&src.text);
    if at < start || at > end {
        return Vec::new();
    }
    let view = View::new(src);
    let Some((decl, arg, value)) = view.value_at(i) else { return Vec::new() };
    let text = value.text.as_str();
    if text.trim().is_empty() {
        return Vec::new();
    }
    let file = |dir: &str, rel: &str| Location { path: format!("{dir}/{rel}"), range: Range::default() };
    match catalog::role(decl.name(), arg) {
        Role::Path(kind) => {
            if kind.urls() && is_url(text) {
                return Vec::new();
            }
            let (dirs, path): (Vec<&str>, &str) = match split_resource_path(text) {
                Some(_) if !kind.other_resources() => return Vec::new(),
                Some((res, path)) => {
                    (cx.ws.resources().filter(|r| r.name == res).map(|r| r.dir.as_str()).collect(), path)
                }
                None => (vec![parent(&cx.doc.path)], text),
            };
            dirs.into_iter()
                .flat_map(|dir| {
                    Tree::cached(cx.ws, dir)
                        .matches(path, kind)
                        .into_iter()
                        .filter(|m| !m.dir)
                        .map(move |m| file(dir, &m.rel))
                })
                .collect()
        }
        Role::Dependency if !text.starts_with('/') => cx
            .ws
            .resources()
            .filter(|r| r.name == text.trim())
            .map(|r| Location { path: r.manifest_path(), range: Range::default() })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::catalog::RDR3_WARNING;
    use crate::manifest::edits::apply;
    use crate::testing::Fixture;
    use openidea_plugin::Severity;

    const FILES: &[(&str, &str)] = &[
        ("res/client/main.lua", ""),
        ("res/client/utils.lua", ""),
        ("res/client/sub/deep.lua", ""),
        ("res/server/main.lua", ""),
        ("res/audio/game.dat151.rel", ""),
        ("res/html/index.html", ""),
        ("other/fxmanifest.lua", "fx_version 'cerulean'"),
        ("other/init.lua", ""),
    ];

    fn fixture(name: &str, manifest: &str) -> (Fixture, String) {
        let mut files = FILES.to_vec();
        let rel = format!("res/{name}");
        files.push((rel.as_str(), manifest));
        (Fixture::new(&files), rel)
    }

    fn definitions(manifest: &str) -> Vec<String> {
        let (fx, rel) = fixture("fxmanifest.lua", manifest);
        let (doc, at) = fx.caret(&rel);
        ManifestFeature.definition(&fx.cx(&doc), at).into_iter().map(|l| l.path).collect()
    }

    #[test]
    fn navigation() {
        assert_eq!(definitions("client_script 'client/ma<caret>in.lua'"), ["/p/res/client/main.lua"]);
        assert_eq!(definitions("client_script './server/<caret>main.lua'"), ["/p/res/server/main.lua"]);
        assert_eq!(
            definitions("client_scripts { 'client/*<caret>.lua' }"),
            ["/p/res/client/main.lua", "/p/res/client/utils.lua"]
        );
        assert_eq!(definitions("client_scripts { 'client/**/*<caret>.lua' }").len(), 3);
        assert_eq!(definitions("data_file 'AUDIO_GAMEDATA' 'audio/game<caret>.dat'"), ["/p/res/audio/game.dat151.rel"]);
        assert!(definitions("data_file 'AUDIO_WAVEPACK' 'au<caret>dio'").is_empty(), "directories are not opened");
        assert_eq!(definitions("shared_script '@other/in<caret>it.lua'"), ["/p/other/init.lua"]);
        assert!(definitions("shared_script '@ox_lib/in<caret>it.lua'").is_empty());
        assert_eq!(definitions("dependency 'oth<caret>er'"), ["/p/other/fxmanifest.lua"]);
        assert!(definitions("dependencies { '/one<caret>sync' }").is_empty());
        assert!(definitions("ui_page 'https://exa<caret>mple.com/x.html'").is_empty());
        assert!(definitions("description 'client/<caret>main.lua'").is_empty());
        assert!(definitions("client_script<caret> 'client/main.lua'").is_empty());
    }

    fn problems(name: &str, manifest: &str) -> Vec<(&'static str, Severity, String, String)> {
        let (fx, rel) = fixture(name, manifest);
        let doc = fx.doc(&rel);
        ManifestFeature
            .problems(&fx.cx(&doc))
            .into_iter()
            .map(|p| (p.code, p.severity, p.message.clone(), doc.text()[p.start..p.end].to_owned()))
            .collect()
    }

    fn messages(manifest: &str) -> Vec<String> {
        problems("fxmanifest.lua", manifest).into_iter().map(|p| p.2).collect()
    }

    fn fixed(name: &str, manifest: &str, message: &str, title: &str) -> String {
        let (fx, rel) = fixture(name, manifest);
        let doc = fx.doc(&rel);
        let problems = ManifestFeature.problems(&fx.cx(&doc));
        let p = problems
            .iter()
            .find(|p| p.message.starts_with(message))
            .unwrap_or_else(|| panic!("no {message}: {problems:?}"));
        let f = p.fixes.iter().find(|f| f.title == title).unwrap_or_else(|| panic!("no fix {title}: {:?}", p.fixes));
        apply(doc.text(), &f.edits)
    }

    const OK: &str = "fx_version 'cerulean'\ngame 'gta5'\n";

    #[test]
    fn fx_version_checks() {
        let p = problems("fxmanifest.lua", "-- my resource\ngame 'gta5'\n");
        assert_eq!(p[0].0, checks::FX_VERSION);
        assert_eq!(p[0].2, "Missing fx_version: FXServer does not start a resource without a manifest version");
        assert_eq!(p[0].3, "game");
        assert_eq!(
            fixed("fxmanifest.lua", "-- my resource\ngame 'gta5'\n", "Missing fx_version", "Add fx_version 'cerulean'"),
            "-- my resource\nfx_version 'cerulean'\ngame 'gta5'\n"
        );
        let text = "fx_version 'cerulan'\ngame 'gta5'\nfx_version 'cerulean'\n";
        let p = problems("fxmanifest.lua", text);
        assert_eq!(
            (p[0].1, p[0].2.as_str(), p[0].3.as_str()),
            (
                Severity::Error,
                "Unknown fx_version 'cerulan': FXServer accepts adamant, bodacious, cerulean",
                "'cerulan'"
            )
        );
        assert_eq!(
            (p[1].1, p[1].2.as_str()),
            (Severity::Warning, "Duplicate fx_version: a resource has a single manifest version")
        );
        assert_eq!(
            fixed("fxmanifest.lua", text, "Unknown fx_version", "Replace with 'cerulean'"),
            text.replacen("cerulan", "cerulean", 1)
        );
        assert_eq!(
            fixed("fxmanifest.lua", text, "Duplicate fx_version", "Remove the duplicate fx_version"),
            "fx_version 'cerulan'\ngame 'gta5'\n"
        );
        assert!(problems("__resource.lua", "game 'gta5'").iter().all(|p| p.0 != checks::FX_VERSION));
        assert!(messages(OK).is_empty(), "{:?}", messages(OK));
    }

    #[test]
    fn game_checks() {
        let text = "fx_version 'cerulean'\nauthor 'me'";
        let p = problems("fxmanifest.lua", text);
        assert_eq!(
            (p[0].2.as_str(), p[0].3.as_str()),
            ("Missing game: FXServer only starts resources that declare the game they run on", "fx_version")
        );
        assert_eq!(
            fixed("fxmanifest.lua", text, "Missing game", "Add game 'gta5' (FiveM)"),
            "fx_version 'cerulean'\ngame 'gta5'\nauthor 'me'"
        );
        assert_eq!(
            fixed("fxmanifest.lua", text, "Missing game", "Add game 'rdr3' (RedM)"),
            format!("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning '{RDR3_WARNING}'\nauthor 'me'")
        );
        assert_eq!(
            fixed("fxmanifest.lua", text, "Missing game", "Add games { 'gta5', 'rdr3' } (FiveM and RedM)"),
            format!("fx_version 'cerulean'\ngames {{ 'gta5', 'rdr3' }}\nrdr3_warning '{RDR3_WARNING}'\nauthor 'me'")
        );
        assert!(problems("__resource.lua", "client_script 'client/main.lua'").iter().all(|p| p.0 != checks::GAME));
        let m = messages("fx_version 'cerulean'\ngames { 'gta6', 'common', 'gta5', 'gta5' }\n");
        assert_eq!(
            m,
            [
                "Unknown game 'gta6': use gta5 (FiveM), rdr3 (RedM) or common",
                "'common' cannot be combined with a specific game: FXServer refuses the resource as ill-formed",
                "Duplicate game 'gta5'",
            ]
        );
        let text = "fx_version 'cerulean'\ngame 'gta6'\n";
        assert_eq!(
            fixed("fxmanifest.lua", text, "Unknown game", "Replace with 'rdr3'"),
            "fx_version 'cerulean'\ngame 'rdr3'\n"
        );
        let m = messages("fx_version 'cerulean'\ngames 'gta5'\n");
        assert!(
            m.contains(&"Missing game: FXServer only starts resources that declare the game they run on".to_owned()),
            "{m:?}"
        );
        assert!(m.contains(
            &"'games' with a single string is recorded as 'games' and ignored: use 'game' or a table".to_owned()
        ));
    }

    #[test]
    fn rdr3_warning_checks() {
        let p = problems("fxmanifest.lua", "fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\n");
        assert_eq!(
            (p[0].1, p[0].2.as_str(), p[0].3.as_str()),
            (
                Severity::Error,
                "RedM resources must declare rdr3_warning with the pre-release acknowledgement, or FXServer refuses to start them",
                "'rdr3'"
            )
        );
        assert_eq!(
            fixed(
                "fxmanifest.lua",
                "fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\nauthor 'x'\n",
                "RedM resources",
                "Add rdr3_warning"
            ),
            format!("fx_version 'cerulean'\ngames {{ 'gta5', 'rdr3' }}\nrdr3_warning '{RDR3_WARNING}'\nauthor 'x'\n")
        );
        let m = messages("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning {}\n");
        assert_eq!(m, ["rdr3_warning needs the pre-release acknowledgement as its value"]);
        let text = "fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning 'I acknowledge'\n";
        assert_eq!(messages(text), [format!("rdr3_warning must be exactly '{RDR3_WARNING}'")]);
        assert_eq!(
            fixed("fxmanifest.lua", text, "rdr3_warning must", "Replace with the required acknowledgement"),
            format!("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning '{RDR3_WARNING}'\n")
        );
        assert!(messages(&format!("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning \"{RDR3_WARNING}\"\n")).is_empty());
        assert!(problems("__resource.lua", "game 'rdr3'").iter().all(|p| p.0 != checks::RDR3));
    }

    #[test]
    fn deprecated_checks() {
        let legacy = problems(
            "__resource.lua",
            "resource_manifest_version '44febabe-d386-4d18-afbe-5e627f4af937'\nclient_script 'client/main.lua'\n",
        );
        assert_eq!(legacy.len(), 1);
        assert_eq!(
            (legacy[0].2.as_str(), legacy[0].3.as_str()),
            (
                "Legacy __resource.lua manifest: rename it to fxmanifest.lua and declare fx_version and game",
                "resource_manifest_version"
            )
        );
        let mut files = FILES.to_vec();
        files.push(("res/fxmanifest.lua", OK));
        files.push(("res/__resource.lua", "client_script 'client/main.lua'"));
        let fx = Fixture::new(&files);
        let doc = fx.doc("res/__resource.lua");
        let p = ManifestFeature.problems(&fx.cx(&doc));
        assert_eq!(
            p[0].message,
            "This __resource.lua is ignored: FXServer loads the fxmanifest.lua of the same folder"
        );
        let text = format!("{OK}lua54 'yes'\nreplace_level_meta 'map'\nbefore_level_meta 'x'\n");
        assert_eq!(
            messages(&text),
            [
                "lua54 has no effect: every Lua script runs on Lua 5.4 since June 2025",
                "before_level_meta is deprecated: use data files (data_file) instead"
            ]
        );
        assert_eq!(
            fixed("fxmanifest.lua", &text, "lua54", "Remove lua54"),
            format!("{OK}replace_level_meta 'map'\nbefore_level_meta 'x'\n")
        );
        let (fx, rel) = fixture("fxmanifest.lua", &text);
        let doc = fx.doc(&rel);
        assert!(ManifestFeature.problems(&fx.cx(&doc)).iter().all(|p| p.deprecated));
        assert!(!p[0].deprecated);
        let text = "game 'gta5'\nresource_manifest_version '44febabe-d386-4d18-afbe-5e627f4af937'\n";
        let m = "resource_manifest_version belongs to legacy __resource.lua manifests: fxmanifest.lua uses fx_version";
        assert_eq!(
            fixed("fxmanifest.lua", text, m, "Replace with fx_version 'cerulean'"),
            "game 'gta5'\nfx_version 'cerulean'\n"
        );
        let text = format!("{OK}resource_manifest_version '44febabe-d386-4d18-afbe-5e627f4af937'\n");
        assert_eq!(fixed("fxmanifest.lua", &text, m, "Remove resource_manifest_version"), OK);
    }

    #[test]
    fn unknown_directives() {
        let text = format!(
            "{OK}client_scirpts {{ 'client/main.lua' }}\nfx_verison 'x'\npizza_topping 'cheese'\nrepository 'x'\nclient_scripts 'client/main.lua'\nlocal t = {{ client_scirpts 'x' }}\n"
        );
        let p = problems("fxmanifest.lua", &text);
        let unknown: Vec<(&str, &str)> =
            p.iter().filter(|p| p.0 == checks::UNKNOWN).map(|p| (p.2.as_str(), p.3.as_str())).collect();
        assert_eq!(
            unknown,
            [
                ("Unknown directive 'client_scirpts': did you mean 'client_scripts'?", "client_scirpts"),
                ("Unknown directive 'fx_verison': did you mean 'fx_version'?", "fx_verison"),
                (
                    "'client_scripts' with a single string is recorded as 'client_scripts' and ignored: use 'client_script' or a table",
                    "client_scripts"
                ),
            ]
        );
        assert_eq!(
            fixed("fxmanifest.lua", &text, "Unknown directive 'client_scirpts'", "Rename to 'client_scripts'"),
            text.replacen("client_scirpts", "client_scripts", 1)
        );
        assert_eq!(
            fixed("fxmanifest.lua", &text, "'client_scripts' with", "Rename to 'client_script'"),
            text.replacen("client_scripts 'client", "client_script 'client", 1)
        );
        assert!(messages(&format!("{OK}dependencies {{ 'other' }}\nfiles {{ 'html/index.html' }}\n")).is_empty());
    }

    #[test]
    fn file_templates_are_clean() {
        for template in
            [include_str!("../../templates/fxmanifest-fivem.lua"), include_str!("../../templates/fxmanifest-redm.lua")]
        {
            let manifest = template.replace("$END$", "");
            let fx = Fixture::new(&[
                ("res/fxmanifest.lua", manifest.as_str()),
                ("res/shared/config.lua", include_str!("../../templates/config.lua")),
                ("res/client/main.lua", include_str!("../../templates/client.lua")),
                ("res/server/main.lua", include_str!("../../templates/server.lua")),
            ]);
            let doc = fx.doc("res/fxmanifest.lua");
            assert!(ManifestFeature.problems(&fx.cx(&doc)).is_empty(), "{manifest}");
        }
    }

    #[test]
    fn missing_files() {
        let text = format!(
            "{OK}client_scripts {{ 'client/*.lua', 'client/nope/*.lua', 'client/gone.lua', 'client', '@other/init.lua', '@other/nope.lua', '@other/x/*.lua', '@ox_lib/init.lua' }}\n\
             files {{ 'client', 'html/style.css', 'stream/*', 'missing.html' }}\nui_page 'https://cfx.re/x.html'\nloadscreen 'html/gone.html'\n\
             data_file 'AUDIO_GAMEDATA' 'audio/game.dat'\nescrow_ignore {{ 'config.lua' }}\n"
        );
        let p = problems("fxmanifest.lua", &text);
        let missing: Vec<(&str, &str)> =
            p.iter().filter(|p| p.0 == checks::MISSING_FILE).map(|p| (p.2.as_str(), p.3.as_str())).collect();
        assert_eq!(
            missing,
            [
                ("'client/nope/*.lua' matches no file of the resource", "'client/nope/*.lua'"),
                ("File 'client/gone.lua' not found in the resource", "'client/gone.lua'"),
                ("File 'client' not found in the resource", "'client'"),
                ("File 'nope.lua' not found in resource 'other'", "'@other/nope.lua'"),
                ("'x/*.lua' matches no file of resource 'other'", "'@other/x/*.lua'"),
                ("File 'missing.html' not found in the resource", "'missing.html'"),
                ("File 'html/gone.html' not found in the resource", "'html/gone.html'"),
                ("File 'config.lua' not found in the resource", "'config.lua'"),
            ]
        );
        let p = &problems("fxmanifest.lua", &text)[0];
        assert_eq!(p.1, Severity::Warning);
    }

    #[test]
    fn missing_files_among_many_globs() {
        let mut paths: Vec<String> = (0..40)
            .flat_map(|i| [format!("res/client/m{i}/main.lua"), format!("res/client/m{i}/sub/deep/x.lua")])
            .collect();
        paths.extend(["res/html/index.html".to_owned(), "res/web/node_modules/a/index.js".to_owned()]);
        let globs: Vec<String> = (0..40).map(|i| format!("'client/m{i}/**/*.lua'")).collect();
        let manifest = format!(
            "{OK}client_scripts {{ {}, 'client/*/sub/deep/*.lua', 'client/m40/*.lua', 'CLIENT/M7/*.LUA', 'web/*/*.js', '**/*.js' }}\n\
             files {{ 'html/*.html', 'html/*.htm', 'client/m3' }}\n",
            globs.join(", ")
        );
        let mut files: Vec<(&str, &str)> = paths.iter().map(|p| (p.as_str(), "")).collect();
        files.push(("res/fxmanifest.lua", &manifest));
        let mut fx = Fixture::new(&files);
        let missing = |fx: &Fixture| -> Vec<String> {
            let doc = fx.doc("res/fxmanifest.lua");
            let problems = ManifestFeature.problems(&fx.cx(&doc));
            problems.into_iter().filter(|p| p.code == checks::MISSING_FILE).map(|p| p.message).collect()
        };
        let expected = [
            "'client/m40/*.lua' matches no file of the resource",
            "'web/*/*.js' matches no file of the resource",
            "'**/*.js' matches no file of the resource",
            "'html/*.htm' matches no file of the resource",
        ];
        assert_eq!(missing(&fx), expected);
        assert_eq!(missing(&fx), expected, "same answer from the cached tree");
        fx.ws.set("/p/res/client/m40/new.lua", Some(""));
        assert_eq!(missing(&fx), expected[1..], "a new file is seen");
        fx.ws.remove("/p/res/client/m40/new.lua");
        assert_eq!(missing(&fx), expected, "a removed file is seen");
    }
}
