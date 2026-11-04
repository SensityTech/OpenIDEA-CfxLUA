use super::*;
use crate::testing::Fixture;
use serde_json::Value;

const SHOP_MAIN: &str = "local function SendReactMessage(action, data)
    SendNUIMessage({ action = action, data = data })
end

RegisterNUICallback('buyItem', function(data, cb)
    TriggerServerEvent('shop:buy', data.item)
    cb(true)
end)

RegisterNUICallback('closeShop', function(_, cb)
    SetNuiFocus(false, false)
    cb('ok')
end)

RegisterCommand('shop', function()
    SetNuiFocus(true, true)
    SendReactMessage('openShop', { items = {} })
end, false)

AddEventHandler('onResourceStop', function()
    SendNUIMessage({ action = 'closeShop' })
end)
";

const SHOP_MANIFEST: &str = "fx_version 'cerulean'
game 'gta5'

ui_page 'web/build/index.html'

client_scripts {
    'client/*.lua',
}

files {
    'web/build/index.html',
    'web/build/assets/*.js',
}
";

const SHOP_APP: &str = "import { useState } from 'react'
import { fetchNui } from './utils/fetchNui'
import { useNuiEvent } from './hooks/useNuiEvent'

interface ShopData {
  items: string[]
}

export function App() {
  const [data, setData] = useState<ShopData | null>(null)
  const [visible, setVisible] = useState(false)

  useNuiEvent<ShopData>('openShop', (shop) => {
    setData(shop)
    setVisible(true)
  })
  useNuiEvent('closeShop', () => setVisible(false))

  const buy = (item: string) => fetchNui<boolean>('buyItem', { item })
  const close = () => fetchNui('closeShop')
  const refund = () => fetchNui('refundItem', { id: 1 })

  return visible && data ? <div onClick={close}>{data.items.length} items, {String(buy)} {String(refund)}</div> : null
}
";

const SHOP_HOOK: &str = "import { useEffect, useRef } from 'react'

type MessageCallback<P> = (payload: P) => void

/** Runs onMessage for each message of the client script named messageName. */
export function useNuiEvent<P = unknown>(messageName: string, onMessage: MessageCallback<P>) {
  const latest = useRef(onMessage)
  latest.current = onMessage

  useEffect(() => {
    function receive(msg: MessageEvent) {
      const { action: kind, data: payload } = msg.data ?? {}
      if (kind !== messageName) return
      latest.current(payload as P)
    }
    window.addEventListener('message', receive)
    return () => window.removeEventListener('message', receive)
  }, [messageName])
}
";

const SHOP_FETCH: &str = "const nuiWindow = window as any

/** Posts body to the Lua callback callbackName of this resource and returns its answer. */
export async function fetchNui<R = unknown>(callbackName: string, body?: unknown): Promise<R> {
  const resource = nuiWindow.GetParentResourceName?.() ?? 'shop'
  const response = await fetch(`https://${resource}/${callbackName}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
  return (await response.json()) as R
}
";

const SHOP_VITE: &str = "import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: './',
  build: {
    outDir: 'build',
  },
  server: {
    port: 3000,
  },
})
";

const LEGACY_CLIENT: &str = "local display = false

function SetDisplay(bool)
    display = bool
    SetNuiFocus(bool, bool)
    SendNUIMessage({
        type = \"ui\",
        status = bool,
    })
end

RegisterNUICallback(\"exit\", function(data, cb)
    SetDisplay(false)
    cb(\"ok\")
end)

RegisterNUICallback(\"main\", function(data, cb)
    print(data.text)
    SetDisplay(false)
    cb(\"ok\")
end)

RegisterCommand(\"nui\", function()
    SetDisplay(not display)
end, false)
";

const LEGACY_MANIFEST: &str = "fx_version 'cerulean'
game 'gta5'

client_script 'client.lua'

ui_page 'html/index.html'

files {
    'html/index.html',
    'html/script.js',
    'html/style.css',
}
";

const LEGACY_SCRIPT: &str = "$(function () {
    function display(bool) {
        if (bool) {
            $(\"#container\").show();
        } else {
            $(\"#container\").hide();
        }
    }

    display(false);

    window.addEventListener('message', function (event) {
        var item = event.data;
        if (item.type === \"ui\") {
            display(item.status == true);
        }
    });

    document.onkeyup = function (data) {
        if (data.which == 27) {
            $.post('https://legacy/exit', JSON.stringify({}));
        }
    };

    $(\"#submit\").click(function () {
        $.post(`https://${GetParentResourceName()}/main`, JSON.stringify({ text: $(\"#input\").val() }));
    });
});
";

const VUE_MENU: &str = "<template>
  <ul>
    <li v-for=\"item in items\" :key=\"item\">{{ item }}</li>
    <button @click=\"close\">Don't keep it open</button>
  </ul>
</template>

<script setup lang=\"ts\">
import { ref } from 'vue'
import { callLua as nui, onLuaMessage } from './utils/nui'

const items = ref<string[]>([])

onLuaMessage<{ items: string[] }>('setMenu', (data) => {
  items.value = data.items
})

const close = () => nui('closeMenu')
</script>
";

const VUE_NUI: &str = "import { onMounted, onUnmounted } from 'vue'

export const callLua = async <T>(name: string, payload?: unknown): Promise<T> => {
  const response = await fetch(`https://${GetParentResourceName()}/${name}`, {
    method: 'POST',
    body: JSON.stringify(payload ?? {}),
  })
  return response.json()
}

export function onLuaMessage<T>(action: string, handler: (data: T) => void) {
  const listener = (event: MessageEvent) => {
    if (event.data?.action !== action) return
    handler(event.data)
  }
  onMounted(() => window.addEventListener('message', listener))
  onUnmounted(() => window.removeEventListener('message', listener))
}
";

fn files(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, String)> {
    let bundle = format!(
        "function n(e,t){{return fetch(`https://${{GetParentResourceName()}}/${{e}}`,{{method:'post',body:JSON.stringify(t)}})}};{}",
        "n('buyItem',{});n('closeShop');".repeat(60)
    );
    let mut out: Vec<(&str, String)> = vec![
        ("shop/fxmanifest.lua", SHOP_MANIFEST.into()),
        ("shop/client/main.lua", SHOP_MAIN.into()),
        ("shop/web/package.json", "{ \"name\": \"shop-web\" }".into()),
        ("shop/web/vite.config.ts", SHOP_VITE.into()),
        ("shop/web/src/App.tsx", SHOP_APP.into()),
        ("shop/web/src/hooks/useNuiEvent.ts", SHOP_HOOK.into()),
        ("shop/web/src/utils/fetchNui.ts", SHOP_FETCH.into()),
        ("shop/web/build/index.html", "<!doctype html>\n<html lang=\"en\">\n<head><script type=\"module\" crossorigin src=\"./assets/index-4f2a.js\"></script></head>\n<body><div id=\"root\"></div></body>\n</html>\n".into()),
        ("shop/web/build/assets/index-4f2a.js", bundle),
        ("shop/web/build/assets/index-4f2a.css", "#root{display:flex}\n".into()),
        ("legacy/fxmanifest.lua", LEGACY_MANIFEST.into()),
        ("legacy/client.lua", LEGACY_CLIENT.into()),
        ("legacy/html/index.html", "<!DOCTYPE html>\n<html>\n<body>\n    <script src=\"script.js\"></script>\n</body>\n</html>\n".into()),
        ("legacy/html/script.js", LEGACY_SCRIPT.into()),
        ("legacy/html/style.css", "#container { display: none; }\n".into()),
        ("other/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\n\nclient_script 'client.lua'\n".into()),
        ("other/client.lua", "RegisterNUICallback('closeShop', function(_, cb)\n    cb('ok')\nend)\n".into()),
        ("vueres/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\n\nclient_script 'client/*.lua'\n\nui_page 'dist/index.html'\n\nfiles {\n    'dist/**',\n}\n".into()),
        ("vueres/client/cl.lua", "RegisterNUICallback('closeMenu', function(_, cb)\n    SetNuiFocus(false, false)\n    cb({})\nend)\n\nRegisterCommand('menu', function()\n    SetNuiFocus(true, true)\n    SendNUIMessage({ action = 'setMenu', items = { 'a', 'b' } })\nend, false)\n".into()),
        ("vueres/dist/index.html", "<!doctype html>\n<html><head><script type=\"module\" src=\"./assets/index.js\"></script></head><body><div id=\"app\"></div></body></html>\n".into()),
        ("vueres/ui/src/Menu.vue", VUE_MENU.into()),
        ("vueres/ui/src/utils/nui.ts", VUE_NUI.into()),
        ("dynamic/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\n\nclient_script 'client.lua'\n\nui_page 'web/index.html'\n\nfiles {\n    'web/index.html',\n    'web/app.js',\n}\n".into()),
        ("dynamic/client.lua", "local actions = { 'accept', 'decline' }\n\nfor _, name in ipairs(actions) do\n    RegisterNUICallback(name, function(data, cb)\n        TriggerServerEvent('dialog:' .. name, data)\n        cb('ok')\n    end)\nend\n".into()),
        ("dynamic/web/index.html", "<!doctype html>\n<html><body><script src=\"app.js\"></script></body></html>\n".into()),
        ("dynamic/web/app.js", "document.getElementById('accept').addEventListener('click', () => fetchNui('accept'))\ndocument.getElementById('maybe').addEventListener('click', () => fetchNui('maybe'))\n".into()),
    ];
    for (p, t) in extra {
        out.retain(|(q, _)| q != p);
        out.push((p, (*t).to_owned()));
    }
    out
}

fn project(extra: &[(&'static str, &'static str)]) -> (Fixture, NuiFeature) {
    let all = files(extra);
    let refs: Vec<(&str, &str)> = all.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let fx = Fixture::new(&refs);
    let feature = NuiFeature::default();
    fx.index(&feature);
    (fx, feature)
}

fn rel(path: &str) -> &str {
    path.strip_prefix("/p/").unwrap_or(path)
}

fn describe(s: &Site) -> String {
    let role = match s.role {
        Role::Registration => "REGISTRATION",
        Role::Fetch => "FETCH",
        Role::Send => "SEND",
        Role::Listener => "LISTENER",
    };
    format!("{role} {}:{}", rel(&s.file), s.at.range.start.line + 1)
}

fn sites_of(fx: &Fixture, f: &NuiFeature, channel: Channel, name: &str) -> Vec<String> {
    let bridge = f.bridge(&fx.ws, &fx.settings);
    let mut out: Vec<String> =
        bridge.sites.iter().filter(|s| s.role.channel() == channel && s.name == name).map(describe).collect();
    out.sort();
    out
}

fn definitions(fx: &Fixture, f: &NuiFeature, path: &str, marker: &str, shift: usize) -> Vec<String> {
    let doc = fx.doc(path);
    let at = doc.text().find(marker).unwrap_or_else(|| panic!("{marker} not in {path}")) + shift;
    let mut out: Vec<String> =
        f.definition(&fx.cx(&doc), at).iter().map(|l| format!("{}:{}", rel(&l.path), l.range.start.line + 1)).collect();
    out.sort();
    out
}

fn completions(fx: &Fixture, f: &NuiFeature, path: &str) -> Vec<String> {
    let (doc, at) = fx.caret(path);
    let mut items = f.completion(&fx.cx(&doc), at, None);
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items.into_iter().map(|i| i.label).collect()
}

fn problems(fx: &Fixture, f: &NuiFeature, path: &str) -> Vec<String> {
    let doc = fx.doc(path);
    let text = doc.text();
    f.problems(&fx.cx(&doc)).iter().map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
}

fn apply(text: &str, fix: &crate::feature::Fix) -> String {
    let mut out = text.to_owned();
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    for (s, e, t) in edits {
        out.replace_range(s..e, &t);
    }
    out
}

#[test]
fn callbacks_and_messages_of_a_react_ui_and_its_client_script() {
    let (fx, f) = project(&[]);
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "buyItem"),
        ["FETCH shop/web/src/App.tsx:19", "REGISTRATION shop/client/main.lua:5"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "closeShop"),
        ["FETCH shop/web/src/App.tsx:20", "REGISTRATION other/client.lua:1", "REGISTRATION shop/client/main.lua:10"]
    );
    assert_eq!(sites_of(&fx, &f, Channel::Callback, "refundItem"), ["FETCH shop/web/src/App.tsx:21"]);
    // SendReactMessage forwards its parameter to the action of SendNUIMessage.
    assert_eq!(
        sites_of(&fx, &f, Channel::Message, "openShop"),
        ["LISTENER shop/web/src/App.tsx:13", "SEND shop/client/main.lua:17"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Message, "closeShop"),
        ["LISTENER shop/web/src/App.tsx:17", "SEND shop/client/main.lua:21"]
    );
}

#[test]
fn both_sides_are_matched_within_a_resource() {
    let (fx, f) = project(&[]);
    let bridge = f.bridge(&fx.ws, &fx.settings);
    let fetch = bridge.sites.iter().find(|s| s.role == Role::Fetch && s.name == "closeShop").unwrap();
    assert_eq!(
        bridge.counterparts(fetch).iter().map(|s| describe(s)).collect::<Vec<_>>(),
        ["REGISTRATION shop/client/main.lua:10"]
    );
    let other =
        bridge.sites.iter().find(|s| s.role == Role::Registration && rel(&s.file) == "other/client.lua").unwrap();
    assert!(bridge.counterparts(other).is_empty());
}

#[test]
fn a_jquery_page_and_a_vue_component() {
    let (fx, f) = project(&[]);
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "exit"),
        ["FETCH legacy/html/script.js:21", "REGISTRATION legacy/client.lua:12"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "main"),
        ["FETCH legacy/html/script.js:26", "REGISTRATION legacy/client.lua:17"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Message, "ui"),
        ["LISTENER legacy/html/script.js:14", "SEND legacy/client.lua:7"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "closeMenu"),
        ["FETCH vueres/ui/src/Menu.vue:18", "REGISTRATION vueres/client/cl.lua:1"]
    );
    assert_eq!(
        sites_of(&fx, &f, Channel::Message, "setMenu"),
        ["LISTENER vueres/ui/src/Menu.vue:14", "SEND vueres/client/cl.lua:8"]
    );
}

#[test]
fn ui_projects_outside_of_the_resource_are_matched_through_their_build_folder() {
    let (fx, f) = project(&[
        (
            "uires/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nui_page 'html/index.html'\nfiles { 'html/**' }",
        ),
        ("uires/client.lua", "RegisterNUICallback('fromOutside', function(_, cb) cb('ok') end)"),
        ("anotherres/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'"),
        ("anotherres/client.lua", "RegisterNUICallback('fromOutside', function(_, cb) cb('ok') end)"),
        ("ui/vite.config.ts", "export default defineConfig({ build: { outDir: '../uires/html', emptyOutDir: true } })"),
        ("ui/src/main.ts", "fetchNui('fromOutside')"),
        (
            "hud/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nui_page 'html/index.html'",
        ),
        ("hud/client.lua", "RegisterNUICallback('hudClose', function(_, cb) cb('ok') end)"),
        ("hud/html/index.html", "<html></html>"),
        (
            "hud-ui/vite.config.ts",
            "import path from 'path'\nexport default defineConfig({ build: { outDir: path.resolve(__dirname, '../hud/html') } })",
        ),
        ("hud-ui/src/main.ts", "fetchNui('hudClose')"),
    ]);
    let bridge = f.bridge(&fx.ws, &fx.settings);
    let fetch = bridge.sites.iter().find(|s| s.role == Role::Fetch && s.name == "fromOutside").unwrap();
    assert_eq!(fetch.resource.as_deref(), Some("/p/uires"));
    assert_eq!(
        bridge.counterparts(fetch).iter().map(|s| describe(s)).collect::<Vec<_>>(),
        ["REGISTRATION uires/client.lua:1"]
    );
    let hud = bridge.sites.iter().find(|s| s.role == Role::Fetch && s.name == "hudClose").unwrap();
    assert_eq!(hud.resource.as_deref(), Some("/p/hud"));
}

#[test]
fn hooks_comparing_another_field_and_generated_bundles_are_left_out() {
    let (fx, f) = project(&[(
        "shop/web/src/reply.ts",
        "export function waitForReply(id: string) {\n  window.addEventListener('message', (e) => {\n    if (e.data.requestId === id) console.log(e.data)\n  })\n}\nwaitForReply('openShop')",
    )]);
    let bridge = f.bridge(&fx.ws, &fx.settings);
    assert!(bridge.sites.iter().all(|s| rel(&s.file) != "shop/web/src/reply.ts"));
    assert!(bridge.sites.iter().all(|s| !rel(&s.file).starts_with("shop/web/build")));
}

#[test]
fn lua_wrappers_of_wrappers() {
    let (fx, f) = project(&[(
        "shop/client/ui.lua",
        "function SendUi(action, payload)\n    SendNUIMessage({ action = action, payload = payload })\nend\n\n\
         function ShowPanel(action)\n    SendUi(action, {})\nend\n\n\
         function RegisterUiCallback(name, handler)\n    RegisterNUICallback(name, function(data, cb)\n        handler(data, cb)\n    end)\nend\n\n\
         ShowPanel('showFromChain')\nRegisterUiCallback('fromWrapper', function(data, cb) cb(1) end)\n",
    )]);
    assert_eq!(sites_of(&fx, &f, Channel::Message, "showFromChain"), ["SEND shop/client/ui.lua:15"]);
    assert_eq!(sites_of(&fx, &f, Channel::Callback, "fromWrapper"), ["REGISTRATION shop/client/ui.lua:16"]);
}

#[test]
fn javascript_client_scripts_registering_callbacks() {
    let (fx, f) = project(&[(
        "legacy/client.js",
        "RegisterNuiCallbackType('jsCallback')\non('__cfx_nui:jsCallback', (data, cb) => cb({}))",
    )]);
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "jsCallback"),
        ["REGISTRATION legacy/client.js:1", "REGISTRATION legacy/client.js:2"]
    );
}

#[test]
fn callbacks_registered_with_computed_names() {
    let (fx, f) = project(&[]);
    let bridge = f.bridge(&fx.ws, &fx.settings);
    assert!(bridge.registers_dynamically(Some("/p/dynamic")));
    assert!(!bridge.registers_dynamically(Some("/p/shop")));
    // The default fetchNui setting covers a wrapper the plugin can't see.
    assert_eq!(sites_of(&fx, &f, Channel::Callback, "maybe"), ["FETCH dynamic/web/app.js:2"]);
}

#[test]
fn settings_change_the_message_keys_and_declare_wrappers() {
    let (fx, f) = project(&[("shop/web/src/Panel.tsx", "import { send } from '@studio/nui'\nsend('buyItem', {})")]);
    let fx = fx.with_settings(Settings::defaults().with(keys::MESSAGE_KEYS, Value::from("action")));
    assert!(sites_of(&fx, &f, Channel::Message, "ui").is_empty());
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "buyItem"),
        ["FETCH shop/web/src/App.tsx:19", "REGISTRATION shop/client/main.lua:5"]
    );
    let fx = fx.with_settings(Settings::defaults().with(keys::FETCH_FUNCTIONS, Value::from("fetchNui, send")));
    assert_eq!(
        sites_of(&fx, &f, Channel::Callback, "buyItem"),
        ["FETCH shop/web/src/App.tsx:19", "FETCH shop/web/src/Panel.tsx:2", "REGISTRATION shop/client/main.lua:5"]
    );
}

#[test]
fn the_bridge_follows_edits_of_the_page() {
    let (mut fx, f) =
        project(&[("shop/web/src/Late.tsx", "import { fetchNui } from './utils/fetchNui'\nfetchNui('lateCall')")]);
    assert_eq!(sites_of(&fx, &f, Channel::Callback, "lateCall"), ["FETCH shop/web/src/Late.tsx:2"]);
    let path = Fixture::path("shop/web/src/Late.tsx");
    let text = "import { fetchNui } from './utils/fetchNui'\nfetchNui('renamedCall')";
    fx.ws.set(&path, Some(text));
    f.index_file(&fx.ws, &path, FileKind::Web, &Source::plain(text));
    f.indexed(&fx.ws);
    assert!(sites_of(&fx, &f, Channel::Callback, "lateCall").is_empty());
    assert_eq!(sites_of(&fx, &f, Channel::Callback, "renamedCall"), ["FETCH shop/web/src/Late.tsx:2"]);
    f.forget_file(&path);
    assert!(sites_of(&fx, &f, Channel::Callback, "renamedCall").is_empty());
}

#[test]
fn definitions_from_lua_to_the_page_and_back() {
    let (fx, f) = project(&[("shop/client/other.lua", "print('buyItem')")]);
    assert_eq!(definitions(&fx, &f, "shop/client/main.lua", "'buyItem'", 1), ["shop/web/src/App.tsx:19"]);
    assert_eq!(
        definitions(&fx, &f, "shop/client/main.lua", "'buyItem'", 9),
        ["shop/web/src/App.tsx:19"],
        "right after the quote"
    );
    assert_eq!(definitions(&fx, &f, "shop/web/src/App.tsx", "'buyItem'", 3), ["shop/client/main.lua:5"]);
    // The callback of the page goes to its resource only, not to the message of the same name.
    assert_eq!(definitions(&fx, &f, "shop/web/src/App.tsx", "fetchNui('closeShop'", 12), ["shop/client/main.lua:10"]);
    assert_eq!(
        definitions(&fx, &f, "shop/web/src/App.tsx", "useNuiEvent('closeShop'", 15),
        ["shop/client/main.lua:21"]
    );
    assert_eq!(definitions(&fx, &f, "shop/client/main.lua", "'openShop'", 1), ["shop/web/src/App.tsx:13"]);
    assert_eq!(definitions(&fx, &f, "legacy/client.lua", "\"ui\"", 1), ["legacy/html/script.js:14"]);
    assert_eq!(definitions(&fx, &f, "legacy/html/script.js", "legacy/exit", 8), ["legacy/client.lua:12"]);
    assert_eq!(definitions(&fx, &f, "legacy/html/script.js", "/main`", 2), ["legacy/client.lua:17"]);
    assert_eq!(definitions(&fx, &f, "vueres/client/cl.lua", "'closeMenu'", 1), ["vueres/ui/src/Menu.vue:18"]);
    assert!(definitions(&fx, &f, "shop/client/other.lua", "buyItem", 3).is_empty(), "plain strings are no NUI names");
}

#[test]
fn gutter_markers_count_the_calls_and_listeners_of_the_page() {
    let (fx, f) = project(&[]);
    let doc = fx.doc("shop/client/main.lua");
    let markers = f.line_markers(&fx.cx(&doc));
    assert_eq!(markers.iter().map(|m| m.line).collect::<Vec<_>>(), [4, 9, 16, 20]);
    let buy = &markers[0];
    assert_eq!(buy.line, 4);
    assert_eq!(buy.icon.as_deref(), Some("implemented"));
    assert_eq!(buy.tooltip, "1 call from the page 'buyItem'\nshop/web/src/App.tsx:19");
    assert_eq!(
        buy.target.as_ref().map(|t| (rel(&t.path).to_owned(), t.range.start.line)),
        Some(("shop/web/src/App.tsx".into(), 18))
    );
    let open = markers.iter().find(|m| m.tooltip.contains("'openShop'")).unwrap();
    assert_eq!(open.icon.as_deref(), Some("implementing"));
    assert!(open.tooltip.starts_with("1 listener in the page"));
    assert!(f.line_markers(&fx.cx(&fx.doc("shop/web/src/App.tsx"))).is_empty(), "Lua files only");
}

#[test]
fn completion_of_the_page_names_in_lua() {
    let (fx, f) = project(&[
        ("shop/client/extra.lua", "RegisterNUICallback('<caret>', function(data, cb) cb('ok') end)"),
        ("shop/client/extra2.lua", "SendNUIMessage({ action = 'op<caret>' })"),
        ("shop/client/extra3.lua", "SendNuiMessage(json.encode({ action = '<caret>' }))"),
        ("shop/client/extra4.lua", "SendReactMessage('<caret>')"),
        ("shop/client/ui.lua", "function ShowUi(action) SendNUIMessage({ action = action }) end"),
        ("shop/client/extra5.lua", "ShowUi('<caret>')"),
    ]);
    assert_eq!(
        completions(&fx, &f, "shop/client/extra5.lua"),
        ["closeShop", "openShop"],
        "a global wrapper of another file"
    );
    let names = completions(&fx, &f, "shop/client/extra.lua");
    assert_eq!(names, ["buyItem", "closeShop", "refundItem"], "not closeMenu of vueres");
    assert_eq!(completions(&fx, &f, "shop/client/extra2.lua"), ["openShop"]);
    assert_eq!(completions(&fx, &f, "shop/client/extra3.lua"), ["closeShop", "openShop"]);
    // SendReactMessage is a local wrapper of main.lua: not visible from another file.
    assert!(completions(&fx, &f, "shop/client/extra4.lua").is_empty());
    let (doc, at) = fx.caret("shop/client/extra2.lua");
    let item = f.completion(&fx.cx(&doc), at, None).remove(0);
    assert_eq!(item.detail.as_deref(), Some("message handler (App.tsx)"));
    assert_eq!(item.replace, Some(doc.range(at - 2, at)));
}

#[test]
fn completion_of_the_lua_names_in_the_page() {
    let (fx, f) = project(&[
        ("shop/web/src/Extra.tsx", "import { fetchNui } from './utils/fetchNui'\nfetchNui<boolean>('<caret>')"),
        (
            "shop/web/src/Hook.tsx",
            "import { useNuiEvent } from './hooks/useNuiEvent'\nuseNuiEvent('<caret>', () => {})",
        ),
        (
            "legacy/html/extra.js",
            "window.addEventListener('message', (e) => { switch (e.data.type) { case '<caret>': break } })",
        ),
        ("legacy/html/url.js", "$.post('https://shop/<caret>', '{}')"),
        ("legacy/html/own.js", "$.post(`https://${GetParentResourceName()}/<caret>`, '{}')"),
        ("shop/client/hud.lua", "SendNUIMessage({ action = 'luaOnlyMessage' })"),
        (
            "shop/web/src/listener.ts",
            "window.addEventListener('message', (e) => { if (e.data.action === '<caret>') {} })",
        ),
        (
            "shop/web/src/reducer.ts",
            "export function reducer(state: number, action: { type: string }) {\n  switch (action.type) {\n    case '<caret>': return state\n  }\n}",
        ),
    ]);
    assert_eq!(completions(&fx, &f, "shop/web/src/Extra.tsx"), ["buyItem", "closeShop"], "not closeMenu");
    assert_eq!(completions(&fx, &f, "shop/web/src/Hook.tsx"), ["closeShop", "luaOnlyMessage", "openShop"]);
    assert_eq!(completions(&fx, &f, "legacy/html/extra.js"), ["ui"]);
    assert_eq!(completions(&fx, &f, "legacy/html/url.js"), ["buyItem", "closeShop"]);
    assert_eq!(completions(&fx, &f, "legacy/html/own.js"), ["exit", "main"]);
    assert!(completions(&fx, &f, "shop/web/src/listener.ts").contains(&"luaOnlyMessage".to_owned()));
    assert!(completions(&fx, &f, "shop/web/src/reducer.ts").is_empty(), "a reducer's strings are no message names");
}

#[test]
fn completion_in_a_ui_project_kept_outside_of_the_resource() {
    let (fx, f) = project(&[
        (
            "uires/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nui_page 'html/index.html'",
        ),
        ("uires/client.lua", "RegisterNUICallback('uiOnly', function(_, cb) cb('ok') end)"),
        ("uires/html/index.html", "<html></html>"),
        ("ui/vite.config.ts", "export default defineConfig({ build: { outDir: '../uires/html' } })"),
        ("ui/src/main.ts", "fetchNui('<caret>')"),
    ]);
    assert_eq!(completions(&fx, &f, "ui/src/main.ts"), ["uiOnly"]);
}

#[test]
fn callbacks_no_client_script_registers() {
    let (fx, f) = project(&[
        (
            "legacy/html/extra.js",
            "$.post('https://legacy/missing', '{}')\n$.post(`https://${GetParentResourceName()}/main`, '{}')",
        ),
        (
            "csharp_ui/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'Client.net.dll'\nui_page 'web/index.html'",
        ),
        ("csharp_ui/web/app.ts", "fetchNui('handledInCSharp')"),
    ]);
    assert_eq!(
        problems(&fx, &f, "shop/web/src/App.tsx"),
        [
            "refundItem: NUI callback 'refundItem' is not registered by the client scripts of 'shop' (RegisterNUICallback): the request fails"
        ]
    );
    assert!(problems(&fx, &f, "legacy/html/script.js").is_empty());
    assert_eq!(
        problems(&fx, &f, "legacy/html/extra.js"),
        [
            "missing: NUI callback 'missing' is not registered by the client scripts of 'legacy' (RegisterNUICallback): the request fails"
        ]
    );
    assert!(problems(&fx, &f, "dynamic/web/app.js").is_empty(), "registered with computed names");
    assert!(problems(&fx, &f, "csharp_ui/web/app.ts").is_empty(), "no visible registration");
    let doc = fx.doc("shop/web/src/App.tsx");
    assert_eq!(f.problems(&fx.cx(&doc))[0].severity, Severity::Info);
}

#[test]
fn callbacks_not_answered_in_lua_files() {
    let (fx, f) = project(&[(
        "shop/client/bad.lua",
        "RegisterNUICallback('close', function()\n    SetNuiFocus(false, false)\nend)",
    )]);
    assert_eq!(problems(&fx, &f, "shop/client/bad.lua").len(), 1);
    assert!(problems(&fx, &f, "shop/client/main.lua").is_empty());
}

#[test]
fn build_files_left_out_of_files() {
    let (fx, f) = project(&[]);
    assert_eq!(
        problems(&fx, &f, "shop/fxmanifest.lua"),
        [
            "'web/build/index.html': 1 file of 'web/build' not covered by files (assets/index-4f2a.css): the page loads without them"
        ]
    );
    let doc = fx.doc("shop/fxmanifest.lua");
    let p = f.problems(&fx.cx(&doc)).remove(0);
    assert_eq!(p.severity, Severity::Warning);
    assert_eq!(p.fixes[0].title, "Add 'web/build/**' to files");
    let fixed = apply(doc.text(), &p.fixes[0]);
    assert!(
        fixed.contains("files {\n    'web/build/index.html',\n    'web/build/assets/*.js',\n    'web/build/**',\n}"),
        "{fixed}"
    );
    assert!(problems(&fx, &f, "legacy/fxmanifest.lua").is_empty());
    assert!(problems(&fx, &f, "vueres/fxmanifest.lua").is_empty());
}

#[test]
fn a_page_missing_from_files() {
    let (fx, f) = project(&[
        ("pageres/html/index.html", "<html></html>"),
        ("pageres/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\n\nui_page 'html/index.html'"),
        ("single/html/index.html", "<html></html>"),
        ("single/html/app.js", "console.log('x')"),
        (
            "single/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nui_page 'html/index.html'\nfile 'html/index.html'\n",
        ),
        ("inline/html/index.html", "<html></html>"),
        ("inline/fxmanifest.lua", "ui_page \"html/index.html\"\nfiles { \"x.lua\" }"),
    ]);
    assert_eq!(
        problems(&fx, &f, "pageres/fxmanifest.lua"),
        [
            "'html/index.html': 'html/index.html' is not covered by files: FXServer does not send the page to the game and the NUI stays white"
        ]
    );
    let fix_of = |path: &str| {
        let doc = fx.doc(path);
        let fix = f.problems(&fx.cx(&doc)).remove(0).fixes.remove(0);
        (fix.title.clone(), apply(doc.text(), &fix))
    };
    assert_eq!(
        fix_of("pageres/fxmanifest.lua"),
        (
            "Add 'html/**' to files".into(),
            "fx_version 'cerulean'\ngame 'gta5'\n\nui_page 'html/index.html'\nfiles {\n    'html/**',\n}".into()
        )
    );
    assert_eq!(problems(&fx, &f, "single/fxmanifest.lua").len(), 1);
    assert_eq!(
        fix_of("single/fxmanifest.lua").1,
        "fx_version 'cerulean'\ngame 'gta5'\nui_page 'html/index.html'\nfile 'html/index.html'\nfile 'html/**'\n"
    );
    assert_eq!(fix_of("inline/fxmanifest.lua").1, "ui_page \"html/index.html\"\nfiles { \"x.lua\", \"html/**\" }");
}

#[test]
fn files_the_page_does_not_load_are_not_required() {
    let (fx, f) = project(&[
        ("legacy/html/.DS_Store", "x"),
        ("legacy/html/style.scss", "x"),
        ("legacy/html/script.js.map", "x"),
        ("legacy/html/README.md", "x"),
        ("legacy/html/scripts.LICENSE.txt", "x"),
        ("legacy/html/.cache/data.bin", "x"),
    ]);
    assert!(problems(&fx, &f, "legacy/fxmanifest.lua").is_empty());
    let (fx, f) = project(&[("legacy/html/logo.png", "png")]);
    assert_eq!(
        problems(&fx, &f, "legacy/fxmanifest.lua"),
        ["'html/index.html': 1 file of 'html' not covered by files (logo.png): the page loads without them"]
    );
}

#[test]
fn dev_servers_missing_builds_and_root_pages_stay_silent() {
    let (fx, f) = project(&[
        ("devres/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nui_page 'http://localhost:5173'\n"),
        (
            "notbuilt/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nui_page 'web/dist/index.html'\nfiles { 'web/dist/**' }\n",
        ),
        ("rootpage/index.html", "<html></html>"),
        ("rootpage/client.lua", "print('client')"),
        (
            "rootpage/fxmanifest.lua",
            "fx_version 'cerulean'\ngame 'gta5'\nui_page 'index.html'\nfiles { 'index.html' }\n",
        ),
    ]);
    for m in ["devres", "notbuilt", "rootpage"] {
        assert!(problems(&fx, &f, &format!("{m}/fxmanifest.lua")).is_empty(), "{m}");
    }
}

fn intentions(fx: &Fixture, f: &NuiFeature, path: &str, text: &str, marker: &str) -> Vec<(String, String)> {
    let doc = Doc::new(&fx.ws, &Fixture::path(path), "lua", 1, text);
    let at = text.find(marker).unwrap() + 1;
    f.intentions(&fx.cx(&doc), at, at).into_iter().map(|i| (i.fix.title.clone(), apply(text, &i.fix))).collect()
}

#[test]
fn dev_mode_round_trip_with_the_port_of_vite_config() {
    let (fx, f) = project(&[]);
    let original = SHOP_MANIFEST;
    let dev = intentions(&fx, &f, "shop/fxmanifest.lua", original, "ui_page");
    assert_eq!(dev.len(), 1);
    assert_eq!(dev[0].0, "Switch ui_page to the dev server (http://localhost:3000)");
    assert!(
        dev[0].1.contains("ui_page 'http://localhost:3000' -- ui_page build: 'web/build/index.html'\n"),
        "{}",
        dev[0].1
    );
    let back = intentions(&fx, &f, "shop/fxmanifest.lua", &dev[0].1, "ui_page");
    assert_eq!(back[0].0, "Switch ui_page back to the build ('web/build/index.html')");
    assert_eq!(back[0].1, original);
}

#[test]
fn dev_mode_round_trip_of_a_call_with_a_comment_and_the_port_setting() {
    let (fx, f) = project(&[]);
    let fx = fx.with_settings(Settings::defaults().with(keys::DEV_SERVER_PORT, Value::from(8080)));
    let original = "fx_version 'cerulean'\ngame 'gta5'\nui_page(\"html/index.html\") -- the menu\nfiles { 'html/**' }";
    let dev = intentions(&fx, &f, "menu/fxmanifest.lua", original, "html/index");
    assert_eq!(dev[0].0, "Switch ui_page to the dev server (http://localhost:8080)");
    assert_eq!(
        dev[0].1,
        "fx_version 'cerulean'\ngame 'gta5'\nui_page(\"http://localhost:8080\") -- the menu -- ui_page build: \"html/index.html\"\nfiles { 'html/**' }"
    );
    let back = intentions(&fx, &f, "menu/fxmanifest.lua", &dev[0].1, "ui_page");
    assert_eq!(back, [("Switch ui_page back to the build (\"html/index.html\")".to_owned(), original.to_owned())]);
}

#[test]
fn dev_mode_is_not_offered_elsewhere() {
    let (fx, f) = project(&[]);
    let text = "fx_version 'cerulean'\ngame 'gta5'\nui_page 'https://cdn.example.com/ui.html'\nfiles { 'html/**' }";
    assert!(intentions(&fx, &f, "elsewhere/fxmanifest.lua", text, "ui_page").is_empty());
    assert!(intentions(&fx, &f, "elsewhere/fxmanifest.lua", text, "files").is_empty());
    let doc = Doc::new(&fx.ws, &Fixture::path("elsewhere/client.lua"), "lua", 1, "local ui_page = 'html/index.html'");
    assert!(f.intentions(&fx.cx(&doc), 8, 8).is_empty());
}

#[test]
fn nothing_when_the_plugin_is_disabled() {
    let (fx, f) = project(&[]);
    let fx = fx.with_settings(Settings::defaults().with("enabled", Value::from(false)));
    let doc = fx.doc("shop/client/main.lua");
    let at = doc.text().find("'buyItem'").unwrap() + 1;
    assert!(f.definition(&fx.cx(&doc), at).is_empty());
    assert!(f.line_markers(&fx.cx(&doc)).is_empty());
    assert!(f.problems(&fx.cx(&fx.doc("shop/web/src/App.tsx"))).is_empty());
    assert!(f.problems(&fx.cx(&fx.doc("shop/fxmanifest.lua"))).is_empty());
}

#[test]
fn nui_callback_events_go_to_the_page() {
    let (fx, f) =
        project(&[("shop/client/events.lua", "AddEventHandler('__cfx_nui:closeShop', function(_, cb) cb('ok') end)")]);
    let found = definitions(&fx, &f, "shop/client/events.lua", "closeShop", 1);
    assert!(found.iter().any(|l| l.starts_with("shop/web/src/App.tsx:")), "{found:?}");
}
