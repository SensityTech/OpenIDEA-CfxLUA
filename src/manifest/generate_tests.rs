use super::*;
use crate::feature::Feature;
use crate::lua::tree::Tree;
use crate::manifest::ManifestFeature;
use crate::manifest::catalog::RDR3_WARNING;
use crate::testing::Fixture;

fn paths(files: &[(&'static str, String)]) -> Vec<&'static str> {
    files.iter().map(|(path, _)| *path).collect()
}

fn text<'a>(files: &'a [(&'static str, String)], path: &str) -> &'a str {
    files.iter().find(|(p, _)| *p == path).map(|(_, t)| t.as_str()).unwrap_or_else(|| panic!("no {path}"))
}

fn assert_clean(files: &[(&'static str, String)]) {
    let rels: Vec<(String, &str)> = files.iter().map(|(path, text)| (format!("res/{path}"), text.as_str())).collect();
    let refs: Vec<(&str, &str)> = rels.iter().map(|(rel, text)| (rel.as_str(), *text)).collect();
    let fx = Fixture::new(&refs);
    let doc = fx.doc("res/fxmanifest.lua");
    let problems = ManifestFeature.problems(&fx.cx(&doc));
    assert!(problems.is_empty(), "{problems:?}");
    for (path, text) in files.iter().filter(|(path, _)| path.ends_with(".lua")) {
        assert!(!Tree::parse(text).has_errors(), "syntax error in {path}");
    }
}

#[test]
fn a_fivem_resource_with_a_shared_config_and_an_author() {
    let options = Options { shared: true, author: "Jane".to_owned(), ..Options::new("garage", Target::FiveM) };
    let files = files(&options);
    assert_eq!(paths(&files), ["fxmanifest.lua", "shared/config.lua", "client/main.lua", "server/main.lua"]);
    assert_eq!(
        text(&files, "fxmanifest.lua"),
        "fx_version 'cerulean'
game 'gta5'

author 'Jane'
description 'garage'
version '1.0.0'

shared_scripts {
    'shared/*.lua',
}

client_scripts {
    'client/*.lua',
}

server_scripts {
    'server/*.lua',
}
"
    );
    assert!(!text(&files, "client/main.lua").contains("RegisterNUICallback"));
    assert_clean(&files);
}

#[test]
fn a_redm_resource_with_a_nui_page() {
    let options = Options {
        client: false,
        server: false,
        nui: true,
        description: "Player's HUD".to_owned(),
        ..Options::new("hud", Target::RedM)
    };
    let files = files(&options);
    assert_eq!(
        paths(&files),
        ["fxmanifest.lua", "client/main.lua", "web/index.html", "web/style.css", "web/script.js"]
    );
    assert_eq!(
        text(&files, "fxmanifest.lua"),
        format!(
            "fx_version 'cerulean'
game 'rdr3'
rdr3_warning '{RDR3_WARNING}'

description 'Player\\'s HUD'
version '1.0.0'

client_scripts {{
    'client/*.lua',
}}

ui_page 'web/index.html'

files {{
    'web/index.html',
    'web/style.css',
    'web/script.js',
}}
"
        )
    );
    let client = text(&files, "client/main.lua");
    assert!(client.contains("RegisterNUICallback('close'"), "{client}");
    assert!(client.contains("RegisterCommand('hud', function()"), "{client}");
    assert!(text(&files, "web/script.js").contains("https://${GetParentResourceName()}/close"));
    assert!(text(&files, "web/index.html").contains("<title>hud</title>"));
    assert_clean(&files);
}

#[test]
fn a_resource_for_both_games() {
    let files = files(&Options::new("shared_lib", Target::Both));
    assert!(
        text(&files, "fxmanifest.lua").starts_with(&format!(
            "fx_version 'cerulean'\ngames {{ 'gta5', 'rdr3' }}\nrdr3_warning '{RDR3_WARNING}'\n"
        )),
        "{}",
        text(&files, "fxmanifest.lua")
    );
    assert_clean(&files);
}

#[test]
fn every_part_together_is_clean() {
    let options = Options::from_answers(" all ", Target::Both, " a\\b ", "", &Script::ALL);
    assert_eq!((options.name.as_str(), options.author.as_str()), ("all", "a\\b"));
    let files = files(&options);
    assert!(text(&files, "fxmanifest.lua").contains("author 'a\\\\b'\ndescription 'all'\n"));
    assert_clean(&files);
}

#[test]
fn a_nui_page_implies_the_client_script() {
    let options = Options::from_answers("ui", Target::FiveM, "", "", &[Script::Nui]);
    assert!(options.client && options.nui && !options.server && !options.shared);
    assert_eq!(Script::DEFAULTS.map(Script::label), ["Client", "Server", "Shared config"]);
}

#[test]
fn the_target_follows_the_project_games() {
    assert_eq!(Target::of(&[]), Target::FiveM);
    assert_eq!(Target::of(&[Game::Rdr3]), Target::RedM);
    assert_eq!(Target::of(&[Game::Rdr3, Game::Gta5]), Target::Both);
    assert_eq!(Target::ALL.map(Target::label), ["FiveM (gta5)", "RedM (rdr3)", "FiveM and RedM"]);
}

#[test]
fn name_validation() {
    let existing = |name: &str| name == "existing";
    assert_eq!(validate_name("my-resource_2.0", existing), None);
    assert_eq!(validate_name("", existing).as_deref(), Some("Enter the resource name"));
    assert_eq!(validate_name("   ", existing).as_deref(), Some("Enter the resource name"));
    let charset = "Use letters, digits, '_', '-' and '.' only: the folder name is the resource name";
    assert_eq!(validate_name("my resource", existing).as_deref(), Some(charset));
    assert_eq!(validate_name("[category]", existing).as_deref(), Some(charset));
    assert_eq!(validate_name("café", existing).as_deref(), Some(charset));
    assert_eq!(validate_name(" existing ", existing).as_deref(), Some("'existing' already exists"));
}
