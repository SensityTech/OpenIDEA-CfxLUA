use super::command_line::{LaunchSettings, command_line};
use crate::project::Game;
use crate::workspace::Workspace;

const CFG: &str = "endpoint_add_tcp \"0.0.0.0:30120\"\n";
const MANIFEST: &str = "fx_version 'cerulean'\n";

fn project() -> Workspace {
    Workspace::from_files(
        "/p",
        &[
            ("artifacts/FXServer.exe", ""),
            ("server-data/server.cfg", CFG),
            ("server-data/resources/cfx_demo/fxmanifest.lua", MANIFEST),
        ],
    )
}

fn settings(data_folder: &str, config: &str, game: Game, arguments: &str) -> LaunchSettings {
    LaunchSettings {
        data_folder: data_folder.to_owned(),
        config: config.to_owned(),
        game,
        arguments: arguments.to_owned(),
    }
}

fn fivem(data_folder: &str, config: &str) -> LaunchSettings {
    settings(data_folder, config, Game::Gta5, "")
}

fn args(ws: &Workspace, settings: &LaunchSettings) -> Vec<String> {
    command_line(ws, settings).unwrap_or_else(|problem| panic!("{problem}"))
}

fn problem(ws: &Workspace, settings: &LaunchSettings) -> String {
    command_line(ws, settings).expect_err("a problem")
}

#[test]
fn a_fivem_server_executes_its_config_from_the_project_root() {
    assert_eq!(args(&project(), &fivem("server-data", "server.cfg")), ["+exec", "server-data/server.cfg"]);
}

#[test]
fn redm_sets_the_game_name_before_the_config() {
    let redm = settings("server-data", "server.cfg", Game::Rdr3, "");
    assert_eq!(args(&project(), &redm), ["+set", "gamename", "rdr3", "+exec", "server-data/server.cfg"]);
}

#[test]
fn extra_arguments_are_split_like_a_shell() {
    let extra = settings("server-data", "server.cfg", Game::Gta5, "+set sv_hostname \"My dev server\" +set onesync on");
    assert_eq!(
        args(&project(), &extra),
        ["+exec", "server-data/server.cfg", "+set", "sv_hostname", "My dev server", "+set", "onesync", "on"]
    );
    let quotes =
        settings("server-data", "server.cfg", Game::Gta5, "  +set a \"\"  +set b \\\"x\\\" +set c \"say \\\"hi\\\"\" ");
    assert_eq!(args(&project(), &quotes)[2..], ["+set", "a", "", "+set", "b", "\"x\"", "+set", "c", "say \"hi\""]);
}

#[test]
fn the_data_folder_can_be_the_root_or_an_absolute_path_in_the_project() {
    let root = Workspace::from_files("/p", &[("server.cfg", CFG)]);
    assert_eq!(args(&root, &fivem(".", "server.cfg")), ["+exec", "server.cfg"]);
    assert_eq!(args(&project(), &fivem("/p/server-data", "server.cfg")), ["+exec", "server-data/server.cfg"]);
    assert_eq!(args(&project(), &fivem("server-data\\", ".\\server.cfg")), ["+exec", "server-data/server.cfg"]);
}

#[test]
fn a_blank_data_folder_is_detected_with_resources_first_then_the_shallowest() {
    let ws = Workspace::from_files(
        "/p",
        &[
            ("server.cfg", CFG),
            ("txData/a/b/server.cfg", CFG),
            ("txData/a/b/resources/x/fxmanifest.lua", MANIFEST),
            ("txData/c/server.cfg", CFG),
            ("txData/c/resources/y/fxmanifest.lua", MANIFEST),
        ],
    );
    assert_eq!(args(&ws, &fivem("", "server.cfg")), ["+exec", "txData/c/server.cfg"]);
    assert_eq!(args(&project(), &fivem(" ", "server.cfg")), ["+exec", "server-data/server.cfg"]);
}

#[test]
fn validation_names_the_first_problem() {
    let ws = project();
    let empty = Workspace::from_files("/p", &[("resources/cfx_demo/fxmanifest.lua", MANIFEST)]);
    assert_eq!(
        problem(&empty, &fivem("", "server.cfg")),
        "Specify the server data folder (the folder holding server.cfg and resources/)"
    );
    assert_eq!(problem(&ws, &fivem("nope", "server.cfg")), "Server data folder 'nope' not found");
    assert_eq!(problem(&ws, &fivem("nope", "")), "Server data folder 'nope' not found");
    assert_eq!(problem(&ws, &fivem("server-data", " ")), "Specify the config file executed at startup (server.cfg)");
    assert_eq!(problem(&ws, &fivem("server-data", "other.cfg")), "Config file 'other.cfg' not found in 'server-data'");
    assert_eq!(problem(&ws, &fivem("", "other.cfg")), "Config file 'other.cfg' not found in 'server-data'");
    let root = Workspace::from_files("/p", &[("server.cfg", CFG)]);
    assert_eq!(problem(&root, &fivem("", "other.cfg")), "Config file 'other.cfg' not found in '.'");
}
