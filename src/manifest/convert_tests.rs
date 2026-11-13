use super::*;

#[test]
fn convert_a_legacy_manifest() {
    let legacy = "-- legacy resource\nresource_manifest_version '44febabe-d386-4d18-afbe-5e627f4af937'\nclient_script 'client.lua'\n";
    assert_eq!(convert(legacy), "-- legacy resource\nfx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\n");
}

#[test]
fn convert_a_legacy_manifest_without_version() {
    assert_eq!(
        convert("client_script 'client.lua'\ngame 'gta5'\n"),
        "fx_version 'cerulean'\nclient_script 'client.lua'\ngame 'gta5'\n"
    );
}

#[test]
fn the_other_versions_are_removed() {
    let legacy = "resource_manifest_version 'a'\nclient_script 'client.lua'\n  resource_manifest_version 'b'\ngames { 'rdr3' }\n";
    assert_eq!(convert(legacy), "fx_version 'cerulean'\nclient_script 'client.lua'\ngames { 'rdr3' }\n");
}

#[test]
fn an_existing_fx_version_is_kept() {
    assert_eq!(
        convert("-- header\nfx_version 'bodacious'\nclient_script 'client.lua'"),
        "-- header\nfx_version 'bodacious'\ngame 'gta5'\nclient_script 'client.lua'"
    );
}

#[test]
fn an_empty_legacy_manifest() {
    assert_eq!(convert(""), "fx_version 'cerulean'\ngame 'gta5'\n");
    assert_eq!(convert("-- nothing yet"), "-- nothing yet\nfx_version 'cerulean'\ngame 'gta5'\n");
}

#[test]
fn a_games_string_is_not_a_game_declaration() {
    assert_eq!(convert("fx_version 'cerulean'\ngames 'rdr3'\n"), "fx_version 'cerulean'\ngame 'gta5'\ngames 'rdr3'\n");
}
