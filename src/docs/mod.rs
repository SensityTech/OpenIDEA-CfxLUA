pub mod data;
pub mod html;
mod insert;
pub mod pages;
pub mod runtime_pages;
pub mod search;
pub mod tabs;
pub(crate) mod target;

use crate::escrow;
use crate::feature::{CommandCx, CommandResult, Feature};
use crate::natives::{self, NativeSet};
use crate::runtime::runtime;
use crate::settings::keys;
use openidea_plugin::views::{View, ViewEvent, Viewport};
use openidea_plugin::{Error, LogLevel, NotificationLevel, host};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use tabs::{Filters, Listing, Tab};

// The page protocol is listed at the top of web/app.js.
pub const VIEW: &str = "docs";
pub const SHOW_DOCS: &str = "show-docs";
const PREFS: &str = "docs.prefs";
const PREFS_MAX: usize = 64 * 1024;
const PENDING: Duration = Duration::from_secs(120);
const WHERE: &str = "The documentation is in the CfxLua tool window (its button is on the left tool window bar). \
                     Put the caret on a native, a hash literal, a built-in event or a manifest directive to show its page there.";

#[derive(Default)]
pub struct Docs {
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    natives: Option<(String, Listing)>,
    pending: Option<(Instant, Value)>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    Init,
    Search(Search),
    Rows {
        tab: String,
        #[serde(default)]
        seq: u64,
        #[serde(default)]
        text: String,
        #[serde(default)]
        filters: Filters,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
    },
    Detail {
        tab: String,
        id: String,
    },
    Insert {
        tab: String,
        id: String,
        variant: String,
    },
    Prefs {
        prefs: Value,
    },
    /// The page is a sandboxed frame: it opens nothing by itself.
    OpenUrl {
        tab: String,
        url: String,
    },
    UpdateNatives,
}

#[derive(Deserialize)]
struct Search {
    tab: String,
    #[serde(default)]
    seq: u64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    filters: Filters,
    #[serde(default)]
    selected: Option<String>,
    #[serde(default)]
    select: Option<String>,
    #[serde(default = "page_rows")]
    limit: usize,
}

fn page_rows() -> usize {
    200
}

impl Docs {
    pub fn render(&self, _cx: &CommandCx, _view: &str, _viewport: Option<Viewport>) -> Result<View, Error> {
        Ok(View::message("The documentation is shown by the plugin's page."))
    }

    /// Answers the page; returns the command the page asked for, which the caller runs, then calls [`Self::refresh`].
    pub fn handle(&self, cx: &CommandCx, view: &str, event: ViewEvent) -> Result<Option<&'static str>, Error> {
        let ViewEvent::Message(text) = event else { return Ok(None) };
        if view != VIEW {
            return Ok(None);
        }
        let reply = match serde_json::from_str::<Request>(&text) {
            Ok(Request::UpdateNatives) => return Ok(Some(crate::natives::UPDATE)),
            Ok(request) => self.reply(cx, request),
            Err(e) => {
                host::log(LogLevel::Debug, &format!("docs page message {text:?}: {e}"));
                None
            }
        };
        if let Some(reply) = reply {
            post(&reply);
        }
        Ok(None)
    }

    pub fn refresh(&self, command: &str) {
        if command == crate::natives::UPDATE {
            post(&json!({ "op": "refresh", "tab": Tab::Natives.id() }));
        }
    }

    /// The first scan ended: tabs listing what it found show it.
    pub fn indexed(&self) {
        for tab in [Tab::Events, Tab::Escrow] {
            post(&json!({ "op": "refresh", "tab": tab.id() }));
        }
    }

    fn reply(&self, cx: &CommandCx, request: Request) -> Option<Value> {
        match request {
            Request::Init => {
                let prefs =
                    host::storage::get(PREFS).ok().flatten().and_then(|s| serde_json::from_str::<Value>(&s).ok());
                let open = self
                    .state
                    .borrow_mut()
                    .pending
                    .take()
                    .filter(|(at, _)| at.elapsed() < PENDING)
                    .map(|(_, open)| open);
                Some(json!({ "op": "init", "prefs": prefs.unwrap_or_else(|| json!({})), "open": open }))
            }
            Request::Search(search) => {
                let tab = Tab::parse(&search.tab)?;
                Some(self.search(cx, tab, search))
            }
            Request::Rows { tab, seq, text, filters, offset, limit } => {
                let tab = Tab::parse(&tab)?;
                let rows = self.with_listing(cx, tab, &text, &filters, |l| l.rows(cx.natives, offset, limit.min(2000)));
                Some(json!({ "op": "rows", "tab": tab.id(), "seq": seq, "offset": offset, "rows": rows }))
            }
            Request::Detail { tab, id } => {
                let tab = Tab::parse(&tab)?;
                let mut detail = detail(cx, tab, &id);
                detail.insert("op".into(), "detail".into());
                Some(Value::Object(detail))
            }
            Request::Insert { tab, id, variant } => {
                let tab = Tab::parse(&tab)?;
                let (text, error) = match insert_text(cx, tab, &id, &variant) {
                    None => ("Nothing to insert for this item.".to_owned(), true),
                    Some(make) => match insert::at_caret(cx.inv, make) {
                        Ok(file) => (format!("Inserted in {file}."), false),
                        Err(message) => (message, true),
                    },
                };
                Some(json!({ "op": "notice", "text": text, "error": error }))
            }
            Request::Prefs { prefs } => {
                let text = prefs.to_string();
                if prefs.is_object()
                    && text.len() <= PREFS_MAX
                    && let Err(e) = host::storage::set(PREFS, &text)
                {
                    host::log(LogLevel::Debug, &format!("docs preferences: {e}"));
                }
                None
            }
            Request::OpenUrl { tab, url } => {
                let tab = Tab::parse(&tab)?;
                // An address the IDE does not open is given back to the page, which shows it to be copied.
                let refused = host::open_url(&url).err()?;
                host::log(LogLevel::Debug, &format!("docs page link: {refused}"));
                Some(json!({ "op": "url", "tab": tab.id(), "url": url }))
            }
            Request::UpdateNatives => None,
        }
    }

    fn search(&self, cx: &CommandCx, tab: Tab, request: Search) -> Value {
        let Search { seq, mut text, mut filters, selected, select, limit, .. } = request;
        let (selected, select) = (selected.as_deref(), select.as_deref());
        let project = project_sets(cx);
        // A native of a set the filter leaves out: search every set.
        if tab == Tab::Natives
            && let Some(set) = select.and_then(|id| id.split_once(':')).and_then(|(s, _)| NativeSet::parse(s))
            && !filters.sets(&project).contains(&set)
        {
            filters =
                Filters { set: "all".into(), aliases: filters.aliases, unnamed: filters.unnamed, ..Filters::default() };
        }
        let rt = runtime();
        let mut results = tab_results(cx, tab, rt, &project, &text, &filters);
        let mut index = select.and_then(|id| results.listing.position(id));
        if select.is_some() && index.is_none() && !text.is_empty() {
            text.clear();
            results = tab_results(cx, tab, rt, &project, &text, &filters);
            index = select.and_then(|id| results.listing.position(id));
        }
        // An escrow report checks a whole resource: only for the row the page asks for.
        let first = if tab == Tab::Escrow { None } else { results.listing.first_selectable() };
        let index = index.or_else(|| selected.and_then(|id| results.listing.position(id))).or(first);
        let detail = index.and_then(|i| results.listing.id(i)).map(|id| Value::Object(detail(cx, tab, &id)));
        let rows = results.listing.rows(cx.natives, 0, limit);
        let reply = json!({
            "op": "items",
            "tab": tab.id(),
            "seq": seq,
            "text": text,
            "filters": filters,
            "total": results.listing.len(),
            "count": results.count,
            "empty": results.empty,
            "offset": 0,
            "rows": rows,
            "selected": index,
            "detail": detail,
            "namespaces": results.namespaces,
        });
        if tab == Tab::Natives {
            self.state.borrow_mut().natives = Some((listing_key(&text, &filters, &project), results.listing));
        }
        reply
    }

    fn with_listing<R>(
        &self,
        cx: &CommandCx,
        tab: Tab,
        text: &str,
        filters: &Filters,
        f: impl FnOnce(&Listing) -> R,
    ) -> R {
        let project = project_sets(cx);
        let key = listing_key(text, filters, &project);
        if tab == Tab::Natives
            && let Some((k, listing)) = self.state.borrow().natives.as_ref()
            && *k == key
        {
            return f(listing);
        }
        let results = tab_results(cx, tab, runtime(), &project, text, filters);
        let out = f(&results.listing);
        if tab == Tab::Natives {
            self.state.borrow_mut().natives = Some((key, results.listing));
        }
        out
    }

    fn open(&self, tab: Tab, id: &str) {
        let message = json!({ "op": "open", "tab": tab.id(), "id": id });
        post(&message);
        self.state.borrow_mut().pending = Some((Instant::now(), message));
    }

    fn show_docs(&self, cx: &CommandCx) -> CommandResult {
        let found = cx.doc.filter(|_| cx.settings.enabled()).and_then(|doc| {
            let selection = cx.inv.editor()?.selections().ok()?.first().copied()?;
            let at = doc.offset(selection.end);
            let detected = cx.ws.map(|w| w.detected_games()).unwrap_or_default();
            let games = cx.ws.and_then(|w| w.resource_of(&doc.path)).and_then(|r| r.games.clone());
            let sets = natives::visible_sets(cx.settings, &detected, doc.side, games.as_deref());
            target::find(doc, at, cx.natives, &sets).map(|t| target::summary(&t))
        });
        match found {
            Some((tab, id, title, body)) => {
                self.open(tab, &id);
                host::notify(NotificationLevel::Info, &title, &body);
            }
            None => host::notify(NotificationLevel::Info, "CfxLua", WHERE),
        }
        Ok(None)
    }
}

impl Feature for Docs {
    fn command(&self, cx: &CommandCx, id: &str, _arg: Option<&str>) -> Option<CommandResult> {
        match id {
            SHOW_DOCS => Some(self.show_docs(cx)),
            escrow::CHECK => {
                if let Some(dir) = escrow::check_resources(cx).first() {
                    self.open(Tab::Escrow, dir);
                }
                Some(Ok(None))
            }
            _ => None,
        }
    }
}

fn post(message: &Value) {
    if let Err(e) = host::post_view_message(VIEW, &message.to_string()) {
        host::log(LogLevel::Debug, &format!("docs page: {e}"));
    }
}

fn tab_results(
    cx: &CommandCx,
    tab: Tab,
    rt: &crate::runtime::Runtime,
    project: &[NativeSet],
    text: &str,
    filters: &Filters,
) -> tabs::Results {
    match (tab, cx.ws.filter(|_| cx.settings.enabled())) {
        (Tab::Escrow, Some(ws)) => tabs::escrow_list(&escrow::resources(ws, cx.settings), &ws.root, text),
        _ => tabs::list(tab, cx.natives, rt, project, text, filters),
    }
}

fn listing_key(text: &str, filters: &Filters, project: &[NativeSet]) -> String {
    format!("{text}\u{0}{filters:?}\u{0}{project:?}")
}

fn project_sets(cx: &CommandCx) -> Vec<NativeSet> {
    let Some(ws) = cx.ws.filter(|_| cx.settings.enabled()) else { return Vec::new() };
    let mut sets = Vec::new();
    if cx.settings.bool(keys::CLIENT_NATIVES, true) {
        sets.extend(natives::project_games(cx.settings, &ws.detected_games()).into_iter().map(NativeSet::for_game));
    }
    if cx.settings.bool(keys::SERVER_NATIVES, true) {
        sets.push(NativeSet::Server);
    }
    sets
}

fn detail(cx: &CommandCx, tab: Tab, id: &str) -> Map<String, Value> {
    let mut copy = Map::new();
    let mut url = None;
    let mut insert: Vec<&str> = Vec::new();
    let html = match tab {
        Tab::Natives => match id.split_once(':').and_then(|(s, n)| cx.natives.catalog(NativeSet::parse(s)?).get(n)) {
            Some(n) => {
                copy.insert("name".into(), n.lua_name.clone().into());
                copy.insert("signature".into(), n.call_statement().into());
                copy.insert("hash".into(), n.hash_display().into());
                if let Some(original) = &n.original_name {
                    copy.insert("nativeName".into(), original.clone().into());
                }
                url = Some(n.doc_url.clone());
                insert.push("call");
                pages::native(n)
            }
            None => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Events => match data::event(id) {
            Some(e) => {
                copy.insert("name".into(), e.name.clone().into());
                url = Some(e.doc_url.clone());
                insert.extend(["handler", "netHandler"]);
                pages::event(e)
            }
            None => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Manifest => match data::directive(id) {
            Some(d) => {
                copy.insert("name".into(), d.name.clone().into());
                url = Some(d.doc_url.clone());
                insert.push("directive");
                pages::directive(d)
            }
            None => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Syntax => match data::syntax_features().iter().find(|f| f.id == id) {
            Some(f) => {
                copy.insert("example".into(), f.example.clone().into());
                insert.push("example");
                pages::syntax(f)
            }
            None => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Escrow => match cx.ws.and_then(|ws| escrow::report(ws, id)) {
            Some(report) => {
                let page = html::markdown(&report);
                copy.insert("report".into(), report.into());
                page
            }
            None => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Guides => match (id.strip_prefix("category:"), data::guide(id)) {
            (Some(category), _) => pages::category(category),
            (None, Some(g)) => {
                copy.insert("link".into(), format!("cfxdoc:{}", g.id).into());
                pages::guide(g)
            }
            (None, None) => pages::NOTHING_SELECTED.to_owned(),
        },
        Tab::Runtime => {
            let rt = runtime();
            match id.strip_prefix("type:") {
                Some(class) => runtime_pages::class(rt, class),
                None => match tabs::runtime_node(rt, id) {
                    Some(tabs::RuntimeNode::Part(p)) => runtime_pages::part(rt, p),
                    Some(tabs::RuntimeNode::Decl(d)) => runtime_pages::decl(rt, rt.decl(d)),
                    None => runtime_pages::NOTHING_SELECTED.to_owned(),
                },
            }
        }
    };
    let mut out = Map::new();
    out.insert("tab".into(), tab.id().into());
    out.insert("id".into(), id.into());
    out.insert("html".into(), html.into());
    out.insert("copy".into(), Value::Object(copy));
    out.insert("url".into(), url.filter(|u| !u.is_empty()).map_or(Value::Null, Value::from));
    out.insert("insert".into(), json!(insert));
    out
}

type InsertText = Box<dyn Fn(&str) -> String>;

fn insert_text(cx: &CommandCx, tab: Tab, id: &str, variant: &str) -> Option<InsertText> {
    match (tab, variant) {
        (Tab::Natives, "call") => {
            let (set, name) = id.split_once(':')?;
            let text = insert::native_call(cx.natives.catalog(NativeSet::parse(set)?).get(name)?);
            Some(Box::new(move |_| text.clone()))
        }
        (Tab::Events, "handler" | "netHandler") => {
            let e = data::event(id)?;
            let network = variant == "netHandler";
            Some(Box::new(move |indent| insert::handler(e, network, indent)))
        }
        (Tab::Manifest, "directive") => {
            let text = insert::directive(data::directive(id)?);
            Some(Box::new(move |_| text.clone()))
        }
        (Tab::Syntax, "example") => {
            let text = insert::example(data::syntax_features().iter().find(|f| f.id == id)?);
            Some(Box::new(move |_| text.clone()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::search::tests::fixture;
    use crate::natives::Natives;
    use crate::settings::Settings;
    use crate::workspace::Workspace;
    use openidea_plugin::Invocation;
    use openidea_plugin::mock::MockHost;

    struct Env {
        mock: MockHost,
        ws: Workspace,
        settings: Settings,
        natives: Natives,
    }

    impl Env {
        fn new() -> Self {
            Self {
                mock: MockHost::install(),
                ws: Workspace::from_files(
                    "/project",
                    &[("res/fxmanifest.lua", "game 'gta5'\nclient_script 'client/*.lua'")],
                ),
                settings: Settings::defaults(),
                natives: Natives::from_natives(fixture()),
            }
        }

        fn send(&self, docs: &Docs, doc: Option<&crate::feature::Doc>, message: Value) -> Vec<Value> {
            let inv: Invocation = self.mock.invocation();
            let cx = CommandCx { ws: Some(&self.ws), settings: &self.settings, natives: &self.natives, doc, inv: &inv };
            let before = self.mock.view_messages().len();
            docs.handle(&cx, VIEW, ViewEvent::Message(message.to_string())).unwrap();
            self.mock.view_messages()[before..]
                .iter()
                .map(|(v, m)| {
                    assert_eq!(v, VIEW);
                    serde_json::from_str(m).unwrap()
                })
                .collect()
        }

        fn one(&self, docs: &Docs, message: Value) -> Value {
            self.send(docs, None, message).pop().expect("a reply")
        }
    }

    #[test]
    fn search_pages_and_details() {
        let env = Env::new();
        let docs = Docs::default();
        let items = env.one(&docs, json!({ "op": "search", "tab": "natives", "seq": 3, "text": "", "limit": 4 }));
        assert_eq!(
            (items["op"].as_str(), items["seq"].as_u64(), items["total"].as_u64()),
            (Some("items"), Some(3), Some(9))
        );
        assert_eq!(items["count"], "9 natives");
        assert_eq!(items["rows"].as_array().unwrap().len(), 4);
        assert_eq!(items["selected"], 0);
        assert_eq!(items["namespaces"], json!(["CFX", "ENTITY", "MISC", "PLAYER"]));
        assert!(items["detail"]["html"].as_str().unwrap().starts_with("<div class='title'>"));
        let rows =
            env.one(&docs, json!({ "op": "rows", "tab": "natives", "seq": 3, "text": "", "offset": 4, "limit": 100 }));
        assert_eq!(rows["rows"].as_array().unwrap().len(), 5);
        let detail = env.one(&docs, json!({ "op": "detail", "tab": "natives", "id": "GTA5:GetGroundZFor_3dCoord" }));
        assert_eq!(detail["copy"]["signature"], "local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, ignoreWater)");
        assert_eq!(detail["copy"]["hash"], "0xC906A7DAB05C8D2B");
        assert_eq!(detail["copy"]["nativeName"], "GET_GROUND_Z_FOR_3D_COORD");
        assert_eq!(detail["insert"], json!(["call"]));
        assert!(detail["url"].as_str().unwrap().starts_with("https://docs.fivem.net/natives/"));
        let unnamed = env.one(&docs, json!({ "op": "detail", "tab": "natives", "id": "GTA5:N_0x0b40ed49d7d6ff84" }));
        assert!(unnamed["copy"].get("nativeName").is_none());
        let guide = env.one(&docs, json!({ "op": "detail", "tab": "guides", "id": "natives" }));
        assert_eq!(guide["copy"]["link"], "cfxdoc:natives");
        let class = env.one(&docs, json!({ "op": "detail", "tab": "runtime", "id": "type:StateBag" }));
        assert!(class["html"].as_str().unwrap().contains("class <b>StateBag</b>"));
        let missing = env.one(&docs, json!({ "op": "detail", "tab": "events", "id": "nope" }));
        assert_eq!(missing["html"], pages::NOTHING_SELECTED);
        assert!(env.send(&docs, None, json!({ "op": "bogus" })).is_empty());
        assert!(env.send(&docs, None, json!({ "op": "detail", "tab": "nope", "id": "x" })).is_empty());
    }

    #[test]
    fn selecting_widens_the_set_and_clears_the_search() {
        let env = Env::new();
        let docs = Docs::default();
        let request = json!({ "op": "search", "tab": "natives", "text": "Ground", "select": "RDR3:GetEntityCoordsRdr",
                              "filters": { "set": "project", "namespace": "MISC", "side": "", "game": "", "aliases": true, "unnamed": true } });
        let items = env.one(&docs, request);
        assert_eq!(items["filters"]["set"], "all");
        assert_eq!(items["filters"]["namespace"], "");
        assert_eq!(items["text"], "");
        assert_eq!(items["total"], 10);
        let index = items["selected"].as_u64().unwrap() as usize;
        assert_eq!(items["rows"][index]["id"], "RDR3:GetEntityCoordsRdr");
        assert_eq!(items["detail"]["id"], "RDR3:GetEntityCoordsRdr");
        let kept = env
            .one(&docs, json!({ "op": "search", "tab": "events", "text": "resource", "selected": "onResourceStop" }));
        assert_eq!(kept["rows"][kept["selected"].as_u64().unwrap() as usize]["id"], "onResourceStop");
        let guides = env.one(&docs, json!({ "op": "search", "tab": "guides", "text": "" }));
        assert_eq!(
            (guides["selected"].as_u64(), guides["count"].as_str()),
            (Some(1), Some(format!("{} guides", data::guides().len()).as_str()))
        );
        let runtime = env.one(
            &docs,
            json!({ "op": "search", "tab": "runtime", "text": "", "select": "PerformHttpRequest", "limit": 5000 }),
        );
        let row = &runtime["rows"][runtime["selected"].as_u64().unwrap() as usize];
        assert!(row["label"].as_str().unwrap().starts_with("PerformHttpRequest("), "{row}");
    }

    #[test]
    fn prefs_open_and_inserts() {
        let env = Env::new();
        let docs = Docs::default();
        let init = env.one(&docs, json!({ "op": "init" }));
        assert_eq!((init["prefs"].clone(), init["open"].clone()), (json!({}), Value::Null));
        assert!(env.send(&docs, None, json!({ "op": "prefs", "prefs": { "fontPercent": 120 } })).is_empty());
        assert_eq!(env.mock.storage()[PREFS], "{\"fontPercent\":120}");
        assert_eq!(env.one(&docs, json!({ "op": "init" }))["prefs"]["fontPercent"], 120);

        let text = "local x = GetEntityCoords(PlayerPedId())\n";
        env.mock.open_editor("/project/res/client/main.lua", "lua", text, &[(0, 15, 0, 15)]);
        let doc = crate::feature::Doc::new(&env.ws, "/project/res/client/main.lua", "lua", 1, text);
        let inv = env.mock.invocation();
        let cx =
            CommandCx { ws: Some(&env.ws), settings: &env.settings, natives: &env.natives, doc: Some(&doc), inv: &inv };
        assert!(docs.command(&cx, "other", None).is_none());
        docs.command(&cx, SHOW_DOCS, None).unwrap().unwrap();
        let open: Value = serde_json::from_str(&env.mock.view_messages().last().unwrap().1).unwrap();
        assert_eq!(open, json!({ "op": "open", "tab": "natives", "id": "GTA5:GetEntityCoords" }));
        let note = env.mock.notifications().pop().unwrap();
        assert_eq!(note.title, "GetEntityCoords");
        assert!(note.body.ends_with("in the CfxLua tool window."));
        assert_eq!(env.one(&docs, json!({ "op": "init" }))["open"], open, "a page loading later still opens it");
        assert_eq!(env.one(&docs, json!({ "op": "init" }))["open"], Value::Null);

        let notice =
            env.one(&docs, json!({ "op": "insert", "tab": "natives", "id": "GTA5:PlayerPedId", "variant": "call" }));
        assert_eq!((notice["text"].as_str(), notice["error"].as_bool()), (Some("Inserted in main.lua."), Some(false)));
        assert_eq!(env.mock.editor_text(), "local x = GetEnlocal retval = PlayerPedId()tityCoords(PlayerPedId())\n");
        let bad = env.one(&docs, json!({ "op": "insert", "tab": "guides", "id": "natives", "variant": "call" }));
        assert_eq!(bad["error"], true);

        let elsewhere = MockHost::install();
        let inv = elsewhere.invocation();
        let cx = CommandCx { ws: None, settings: &env.settings, natives: &env.natives, doc: None, inv: &inv };
        docs.command(&cx, SHOW_DOCS, None).unwrap().unwrap();
        assert!(elsewhere.notifications()[0].body.starts_with("The documentation is in the CfxLua tool window"));
        assert_eq!(project_sets(&cx), Vec::<NativeSet>::new());
    }

    #[test]
    fn a_web_page_is_opened_in_the_browser_and_any_other_address_is_given_back() {
        let env = Env::new();
        let docs = Docs::default();
        let detail = env.one(&docs, json!({ "op": "detail", "tab": "natives", "id": "GTA5:GetGroundZFor_3dCoord" }));
        let online = detail["url"].as_str().unwrap();
        assert!(env.send(&docs, None, json!({ "op": "openUrl", "tab": "natives", "url": online })).is_empty());
        assert_eq!(env.mock.opened_urls(), [online]);
        let mail = json!({ "op": "openUrl", "tab": "guides", "url": "mailto:someone@example.com" });
        let shown = json!({ "op": "url", "tab": "guides", "url": "mailto:someone@example.com" });
        assert_eq!(env.one(&docs, mail), shown, "the page shows it to be copied");
        assert!(env.send(&docs, None, json!({ "op": "openUrl", "tab": "nope", "url": online })).is_empty());
        assert_eq!(env.mock.opened_urls(), [online]);
    }

    #[test]
    fn the_page_asks_for_a_natives_update_and_is_refreshed() {
        let env = Env::new();
        let docs = Docs::default();
        let inv: Invocation = env.mock.invocation();
        let cx = CommandCx { ws: Some(&env.ws), settings: &env.settings, natives: &env.natives, doc: None, inv: &inv };
        let request = ViewEvent::Message(json!({ "op": "updateNatives" }).to_string());
        assert_eq!(docs.handle(&cx, VIEW, request).unwrap(), Some(crate::natives::UPDATE));
        assert!(env.mock.view_messages().is_empty(), "the caller runs the update first");
        docs.refresh(crate::natives::UPDATE);
        let (_, message) = env.mock.view_messages().pop().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&message).unwrap(), json!({ "op": "refresh", "tab": "natives" }));
    }

    #[test]
    fn the_escrow_check_opens_its_report_in_the_escrow_tab() {
        let env = Env::new();
        let docs = Docs::default();
        let inv: Invocation = env.mock.invocation();
        let cx = CommandCx { ws: Some(&env.ws), settings: &env.settings, natives: &env.natives, doc: None, inv: &inv };
        assert_eq!(docs.command(&cx, escrow::CHECK, None).unwrap().unwrap(), None);
        let open: Value = serde_json::from_str(&env.mock.view_messages().last().unwrap().1).unwrap();
        assert_eq!(open, json!({ "op": "open", "tab": "escrow", "id": "/project/res" }));
        let items = env.one(
            &docs,
            json!({ "op": "search", "tab": "escrow", "seq": 1, "text": "", "select": "/project/res", "limit": 10 }),
        );
        assert_eq!((items["rows"][0]["label"].as_str(), items["count"].as_str()), (Some("res"), Some("1 resource")));
        assert!(items["detail"]["html"].as_str().unwrap().contains("Escrow pre-publish check: res"));
        assert!(items["detail"]["copy"]["report"].as_str().unwrap().starts_with("# Escrow pre-publish check: res"));
        let none = env.one(&docs, json!({ "op": "search", "tab": "escrow", "seq": 2, "text": "zzz", "limit": 10 }));
        assert_eq!(none["empty"], "Nothing matches 'zzz'");
    }

    #[test]
    fn the_escrow_tab_checks_only_the_resource_asked_for() {
        let env = Env::new();
        let docs = Docs::default();
        let listed = env.one(&docs, json!({ "op": "search", "tab": "escrow", "seq": 1, "text": "", "limit": 10 }));
        assert_eq!(listed["rows"][0]["id"], "/project/res");
        assert_eq!((&listed["selected"], &listed["detail"]), (&Value::Null, &Value::Null));
        let kept = env.one(
            &docs,
            json!({ "op": "search", "tab": "escrow", "seq": 2, "text": "res", "selected": "/project/res", "limit": 10 }),
        );
        assert_eq!(kept["selected"], 0);
        assert!(kept["detail"]["copy"]["report"].as_str().unwrap().starts_with("# Escrow pre-publish check: res"));
    }
}
