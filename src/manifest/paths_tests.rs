use super::followed;
use crate::lua::Source;
use crate::manifest::edits::apply;
use crate::testing::Fixture;

const MANIFEST: &str = "res/fxmanifest.lua";

fn fixture(manifest: &str) -> Fixture {
    Fixture::new(&[
        (MANIFEST, manifest),
        ("res/client/utils.lua", ""),
        ("res/client/main.lua", ""),
        ("res/html/index.html", ""),
        ("res/audio/game.dat151.rel", ""),
        ("other/fxmanifest.lua", "fx_version 'cerulean'\n"),
        ("other/init.lua", ""),
        ("user/fxmanifest.lua", "shared_script '@other/init.lua'\nclient_script 'init.lua'\n"),
    ])
}

/// The manifest `rel` once `file` is `to`.
fn after_in(fx: &Fixture, rel: &str, file: &str, to: &str) -> String {
    let path = Fixture::path(rel);
    let text = fx.ws.text(&path).unwrap_or_else(|| panic!("no fixture file {rel}")).to_owned();
    let edits = followed(&fx.ws, &path, &Source::new(&text), &Fixture::path(file), &Fixture::path(to));
    apply(&text, &edits)
}

fn after(manifest: &str, file: &str, to: &str) -> String {
    after_in(&fixture(manifest), MANIFEST, file, to)
}

#[test]
fn a_renamed_file_takes_its_new_name_and_a_glob_stays() {
    // `CfxManifestReferenceTest.testRenameFileUpdatesManifest`
    assert_eq!(
        after(
            "client_script 'client/utils.lua'\nclient_script 'client/*.lua'\n",
            "res/client/utils.lua",
            "res/client/helpers.lua"
        ),
        "client_script 'client/helpers.lua'\nclient_script 'client/*.lua'\n"
    );
}

#[test]
fn what_is_written_before_the_name_is_kept() {
    let manifest =
        "client_scripts {\n    \"./client/utils.lua\",\n    'client/main.lua',\n}\nfile 'client/utils.lua'\n";
    assert_eq!(
        after(manifest, "res/client/utils.lua", "res/client/helpers.lua"),
        "client_scripts {\n    \"./client/helpers.lua\",\n    'client/main.lua',\n}\nfile 'client/helpers.lua'\n"
    );
}

#[test]
fn a_moved_file_is_written_again_from_its_resource() {
    let manifest = "client_script './client/utils.lua'\nui_page 'html/index.html'\n";
    assert_eq!(
        after(manifest, "res/client/utils.lua", "res/shared/lib/utils.lua"),
        "client_script 'shared/lib/utils.lua'\nui_page 'html/index.html'\n"
    );
    assert_eq!(
        after(manifest, "res/html/index.html", "res/web/index.html"),
        "client_script './client/utils.lua'\nui_page 'web/index.html'\n"
    );
    assert_eq!(
        after(manifest, "res/client/utils.lua", "other/utils.lua"),
        manifest,
        "out of the resource it is read from"
    );
}

#[test]
fn a_path_into_another_resource_keeps_its_resource() {
    let fx = fixture("");
    let user = "user/fxmanifest.lua";
    assert_eq!(
        after_in(&fx, user, "other/init.lua", "other/start.lua"),
        "shared_script '@other/start.lua'\nclient_script 'init.lua'\n",
        "the file of the same name in its own resource is another file"
    );
    assert_eq!(
        after_in(&fx, user, "other/init.lua", "other/lib/init.lua"),
        "shared_script '@other/lib/init.lua'\nclient_script 'init.lua'\n"
    );
    assert_eq!(after_in(&fx, "other/fxmanifest.lua", "other/init.lua", "other/start.lua"), "fx_version 'cerulean'\n");
}

#[test]
fn what_names_no_file_is_left_alone() {
    let manifest = "description 'client/utils.lua'\nui_page 'https://example.com/client/utils.lua'\n\
                    file '@other/init.lua'\nclient_script 'client/missing.lua'\ndependency 'other'\n";
    assert_eq!(after(manifest, "res/client/utils.lua", "res/client/helpers.lua"), manifest);
    assert_eq!(after(manifest, "other/init.lua", "other/start.lua"), manifest, "only a script reads another resource");
}

#[test]
fn a_data_file_named_by_its_start_takes_the_whole_new_name() {
    let manifest = "data_file 'AUDIO_GAMEDATA' 'audio/game.dat'\n";
    assert_eq!(
        after(manifest, "res/audio/game.dat151.rel", "res/audio/sounds.dat151.rel"),
        "data_file 'AUDIO_GAMEDATA' 'audio/sounds.dat151.rel'\n"
    );
}

#[test]
fn a_path_written_with_an_escape_is_left_alone() {
    let manifest = "client_script 'client/\\117tils.lua'\n";
    assert_eq!(after(manifest, "res/client/utils.lua", "res/client/helpers.lua"), manifest);
}
