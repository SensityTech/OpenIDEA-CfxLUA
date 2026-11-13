use crate::project::is_manifest_name;
use crate::text::{file_name, slash};
use crate::workspace::Workspace;
use openidea_plugin::FilesChanged;

pub const DEFAULT_DELAY_MS: i64 = 400;
pub const MAX_DELAY_MS: i64 = 60_000;

const REFRESH: &str = "refresh";
const ENSURE: &str = "ensure";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Content,
    Structure,
}

impl Change {
    pub fn of_content_change(file_name: &str) -> Self {
        if is_manifest_name(file_name) { Self::Structure } else { Self::Content }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReloadBatch {
    pub refresh: bool,
    pub resources: Vec<String>,
}

impl ReloadBatch {
    pub fn commands(&self) -> Vec<String> {
        let refresh = self.refresh.then(|| REFRESH.to_owned());
        refresh.into_iter().chain(self.resources.iter().map(|r| format!("{ENSURE} {r}"))).collect()
    }

    /// The batch without the resources `ws` no longer holds, `None` when none is left.
    pub fn within(mut self, ws: &Workspace) -> Option<Self> {
        self.resources.retain(|r| ws.resource_named(r).is_some());
        (!self.resources.is_empty()).then_some(self)
    }
}

struct Pending {
    resource: String,
    deadline: u64,
    structural: bool,
}

pub struct ReloadPlanner {
    delay: u64,
    pending: Vec<Pending>,
}

impl ReloadPlanner {
    pub fn new(delay_ms: i64) -> Self {
        let mut planner = Self { delay: 0, pending: Vec::new() };
        planner.set_delay_ms(delay_ms);
        planner
    }

    pub fn set_delay_ms(&mut self, delay_ms: i64) {
        self.delay = delay_ms.clamp(0, MAX_DELAY_MS).unsigned_abs();
    }

    pub fn record(&mut self, resource: &str, change: Change, now: u64, excluded: &[String]) -> bool {
        if resource.trim().is_empty() || excluded.iter().any(|e| e == resource) {
            return false;
        }
        let at = match self.pending.iter().position(|p| p.resource == resource) {
            Some(at) => at,
            None => {
                self.pending.push(Pending { resource: resource.to_owned(), deadline: now, structural: false });
                self.pending.len() - 1
            }
        };
        let entry = &mut self.pending[at];
        entry.deadline = now.saturating_add(self.delay);
        entry.structural |= change == Change::Structure;
        true
    }

    pub fn next_deadline(&self) -> Option<u64> {
        self.pending.iter().map(|p| p.deadline).min()
    }

    pub fn cancel(&mut self, resource: &str) {
        self.pending.retain(|p| p.resource != resource);
    }

    pub fn drain(&mut self, now: u64) -> Option<ReloadBatch> {
        let (due, waiting): (Vec<Pending>, Vec<Pending>) =
            std::mem::take(&mut self.pending).into_iter().partition(|p| p.deadline <= now);
        self.pending = waiting;
        if due.is_empty() {
            return None;
        }
        let refresh = due.iter().any(|p| p.structural);
        Some(ReloadBatch { refresh, resources: due.into_iter().map(|p| p.resource).collect() })
    }
}

/// The resources a `files-changed` batch touched, read from `ws` once it holds that batch.
pub fn changes<'a>(ws: &'a Workspace, batch: &FilesChanged) -> Vec<(&'a str, Change)> {
    let created = batch.created.iter().map(|p| (p, Change::Structure));
    let modified = batch.modified.iter().map(|p| (p, Change::of_content_change(file_name(&slash(p)))));
    let removed = batch.removed.iter().map(|p| (p, Change::Structure));
    created
        .chain(modified)
        .chain(removed)
        .filter_map(|(path, change)| Some((ws.resource_of(path)?.name.as_str(), change)))
        .collect()
}
