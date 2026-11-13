use super::planner::{Change, ReloadBatch, ReloadPlanner, changes};
use crate::workspace::Workspace;
use openidea_plugin::FilesChanged;

const NONE: &[String] = &[];

fn commands(batch: Option<ReloadBatch>) -> Vec<String> {
    batch.map(|b| b.commands()).unwrap_or_default()
}

#[test]
fn a_single_save_is_ensured_after_the_delay() {
    let mut planner = ReloadPlanner::new(400);
    assert!(planner.record("cfx_demo", Change::Content, 1000, NONE));
    assert_eq!(planner.next_deadline(), Some(1400));
    assert_eq!(planner.drain(1399), None);
    assert_eq!(commands(planner.drain(1400)), ["ensure cfx_demo"]);
    assert_eq!(planner.next_deadline(), None);
    assert_eq!(planner.drain(5000), None);
}

#[test]
fn rapid_saves_are_debounced() {
    let mut planner = ReloadPlanner::new(400);
    planner.record("cfx_demo", Change::Content, 0, NONE);
    planner.record("cfx_demo", Change::Content, 300, NONE);
    planner.record("cfx_demo", Change::Content, 600, NONE);
    assert_eq!(planner.drain(900), None);
    assert_eq!(commands(planner.drain(1000)), ["ensure cfx_demo"]);
}

#[test]
fn a_structural_change_refreshes_first() {
    let mut planner = ReloadPlanner::new(400);
    planner.record("cfx_demo", Change::Content, 0, NONE);
    planner.record("cfx_demo", Change::Structure, 100, NONE);
    planner.record("cfx_demo", Change::Content, 200, NONE);
    let batch = planner.drain(600).expect("due");
    assert!(batch.refresh);
    assert_eq!(batch.commands(), ["refresh", "ensure cfx_demo"]);
}

#[test]
fn resources_are_coalesced_separately() {
    let mut planner = ReloadPlanner::new(400);
    planner.record("a", Change::Content, 0, NONE);
    planner.record("b", Change::Structure, 100, NONE);
    planner.record("a", Change::Content, 300, NONE);
    assert_eq!(planner.drain(500), Some(ReloadBatch { refresh: true, resources: vec!["b".to_owned()] }));
    assert_eq!(planner.next_deadline(), Some(700));
    assert_eq!(planner.drain(700), Some(ReloadBatch { refresh: false, resources: vec!["a".to_owned()] }));
}

#[test]
fn due_resources_share_one_refresh_in_first_change_order() {
    let mut planner = ReloadPlanner::new(400);
    planner.record("b", Change::Content, 0, NONE);
    planner.record("a", Change::Structure, 10, NONE);
    planner.record("c", Change::Content, 20, NONE);
    assert_eq!(commands(planner.drain(1000)), ["refresh", "ensure b", "ensure a", "ensure c"]);
}

#[test]
fn excluded_resources_are_ignored() {
    let mut planner = ReloadPlanner::new(400);
    let excluded = ["big_map".to_owned()];
    assert!(!planner.record("big_map", Change::Content, 0, &excluded));
    assert!(planner.record("cfx_demo", Change::Content, 0, &excluded));
    assert_eq!(commands(planner.drain(400)), ["ensure cfx_demo"]);
    assert!(!planner.record("  ", Change::Content, 0, NONE));
}

#[test]
fn cancel_forgets_pending_changes() {
    let mut planner = ReloadPlanner::new(400);
    planner.record("cfx_demo", Change::Structure, 0, NONE);
    planner.cancel("cfx_demo");
    assert_eq!(planner.next_deadline(), None);
    assert_eq!(planner.drain(1000), None);
}

#[test]
fn the_delay_is_configurable_and_bounded() {
    let mut planner = ReloadPlanner::new(1500);
    planner.record("nui", Change::Content, 0, NONE);
    assert_eq!(planner.drain(1499), None);
    planner.set_delay_ms(-5);
    planner.record("other", Change::Content, 2000, NONE);
    assert_eq!(planner.next_deadline(), Some(1500));
    assert_eq!(commands(planner.drain(2000)), ["ensure nui", "ensure other"]);

    planner.set_delay_ms(i64::MAX);
    planner.record("nui", Change::Content, 0, NONE);
    assert_eq!(planner.next_deadline(), Some(60_000));
}

#[test]
fn a_manifest_content_change_is_structural() {
    assert_eq!(Change::of_content_change("fxmanifest.lua"), Change::Structure);
    assert_eq!(Change::of_content_change("__resource.lua"), Change::Structure);
    assert_eq!(Change::of_content_change("client.lua"), Change::Content);
    assert_eq!(Change::of_content_change("index.html"), Change::Content);
}

const ROOT: &str = "/p";
const DEMO: &str = "resources/[local]/cfx_demo";

fn project() -> Workspace {
    Workspace::from_files(
        ROOT,
        &[
            (
                "resources/[local]/cfx_demo/fxmanifest.lua",
                "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\n",
            ),
            ("resources/[local]/cfx_demo/client.lua", "print('a')\n"),
            ("resources/big_map/fxmanifest.lua", "fx_version 'cerulean'\n"),
            ("resources/big_map/map.lua", "-- map\n"),
            ("tools/build.lua", "-- not a resource\n"),
        ],
    )
}

fn path(rel: &str) -> String {
    format!("{ROOT}/{rel}")
}

fn batch(created: &[&str], modified: &[&str], removed: &[&str]) -> FilesChanged {
    let paths = |rels: &[&str]| rels.iter().map(|r| path(r)).collect();
    FilesChanged { created: paths(created), modified: paths(modified), removed: paths(removed) }
}

struct Reload {
    ws: Workspace,
    planner: ReloadPlanner,
    excluded: Vec<String>,
    now: u64,
}

impl Reload {
    fn new() -> Self {
        Self { ws: project(), planner: ReloadPlanner::new(50), excluded: Vec::new(), now: 0 }
    }

    fn changed(&mut self, batch: &FilesChanged) {
        self.now += 10;
        for (resource, change) in changes(&self.ws, batch) {
            self.planner.record(resource, change, self.now, &self.excluded);
        }
    }

    fn sent(&mut self) -> Vec<String> {
        self.now += 50;
        commands(self.planner.drain(self.now).and_then(|b| b.within(&self.ws)))
    }
}

#[test]
fn a_saved_script_ensures_its_resource() {
    let mut reload = Reload::new();
    reload.changed(&batch(&[], &[&format!("{DEMO}/client.lua")], &[]));
    assert_eq!(reload.sent(), ["ensure cfx_demo"]);
}

#[test]
fn rapid_saves_of_a_resource_are_coalesced() {
    let mut reload = Reload::new();
    let client = format!("{DEMO}/client.lua");
    for _ in 0..3 {
        reload.changed(&batch(&[], &[&client], &[]));
    }
    assert_eq!(reload.sent(), ["ensure cfx_demo"]);
    assert!(reload.sent().is_empty());
}

#[test]
fn a_new_file_a_manifest_change_or_a_rename_refreshes_first() {
    let mut reload = Reload::new();
    let extra = format!("{DEMO}/extra.lua");
    reload.ws.set(&path(&extra), Some(""));
    reload.changed(&batch(&[&extra], &[], &[]));
    assert_eq!(reload.sent(), ["refresh", "ensure cfx_demo"]);

    let manifest = format!("{DEMO}/fxmanifest.lua");
    reload.ws.set(
        &path(&manifest),
        Some("fx_version 'cerulean'\ngame 'gta5'\nclient_scripts { 'client.lua', 'extra.lua' }\n"),
    );
    reload.changed(&batch(&[], &[&manifest], &[]));
    assert_eq!(reload.sent(), ["refresh", "ensure cfx_demo"]);

    let (client, main) = (format!("{DEMO}/client.lua"), format!("{DEMO}/main.lua"));
    reload.ws.remove(&path(&client));
    reload.ws.set(&path(&main), Some("print('a')\n"));
    reload.changed(&batch(&[&main], &[], &[&client]));
    assert_eq!(reload.sent(), ["refresh", "ensure cfx_demo"]);
}

#[test]
fn a_deleted_file_refreshes_but_a_deleted_resource_is_not_ensured() {
    let mut reload = Reload::new();
    reload.ws.remove(&path("resources/big_map/map.lua"));
    reload.changed(&batch(&[], &[], &["resources/big_map/map.lua"]));
    assert_eq!(reload.sent(), ["refresh", "ensure big_map"]);

    reload.ws.remove(&path("resources/big_map/fxmanifest.lua"));
    reload.changed(&batch(&[], &[], &["resources/big_map", "resources/big_map/fxmanifest.lua"]));
    assert!(reload.sent().is_empty());
}

#[test]
fn a_resource_deleted_while_waiting_is_not_ensured() {
    let mut reload = Reload::new();
    reload.changed(&batch(&[], &["resources/big_map/map.lua"], &[]));
    reload.ws.remove(&path("resources/big_map/map.lua"));
    reload.ws.remove(&path("resources/big_map/fxmanifest.lua"));
    reload.changed(&batch(&[], &[], &["resources/big_map"]));
    assert!(reload.sent().is_empty());
}

#[test]
fn excluded_resources_and_files_outside_resources_are_not_reloaded() {
    let mut reload = Reload::new();
    reload.excluded = vec!["big_map".to_owned()];
    reload.changed(&batch(&[], &["resources/big_map/map.lua", "tools/build.lua"], &[]));
    assert!(reload.sent().is_empty());

    reload.excluded.clear();
    reload.changed(&batch(&[], &["resources/big_map/map.lua"], &[]));
    assert_eq!(reload.sent(), ["ensure big_map"]);
}
