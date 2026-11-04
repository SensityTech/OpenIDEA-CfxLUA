# NUI: HTML user interfaces

NUI lets a resource display a web page (HTML, CSS, JavaScript) on top of the game. Each resource gets one full-screen, transparent page. The page and the resource's **client** scripts talk to each other through messages (Lua to page) and callbacks (page to Lua).

NUI exists on the client only, in FiveM and RedM.

## Declaring the page

```lua
-- fxmanifest.lua
fx_version 'cerulean'
game 'gta5'

client_script 'client.lua'

ui_page 'html/index.html'

files {
    'html/index.html',
    'html/style.css',
    'html/app.js',
}
```

- `ui_page` points to the page. It can also be an absolute `https://` URL.
- Every file the page loads (HTML, CSS, JS, images, fonts) must be listed in `files` so that clients download it. Globs are accepted, for example `'html/*.js'` or `'html/**/*.png'`.
- Inside the page, reference files with relative paths, or with the `https://cfx-nui-<resourceName>/path` scheme. The older `nui://` scheme is deprecated.

A minimal page:

```html
<!DOCTYPE html>
<html>
<head>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="panel" style="display: none">
        <h1 id="title"></h1>
        <button id="close">Close</button>
    </div>
    <script src="app.js"></script>
</body>
</html>
```

## Lua to page: `SendNUIMessage`

`SendNUIMessage(table)` encodes the table as JSON and posts it to your resource's page. The page receives it as a `message` event; the table is in `event.data`.

```lua
-- client.lua
local function openPanel(title)
    SendNUIMessage({ action = 'open', title = title })
    SetNuiFocus(true, true)
end

RegisterCommand('panel', function()
    openPanel('Hello from Lua')
end, false)
```

```js
// html/app.js
window.addEventListener('message', (event) => {
    const data = event.data;
    if (data.action === 'open') {
        document.getElementById('title').textContent = data.title;
        document.getElementById('panel').style.display = 'block';
    }
});
```

`SendNuiMessage(jsonString)` is the underlying native; it takes an already encoded JSON string.

## Page to Lua: NUI callbacks

The page calls Lua with an HTTP `POST` to `https://<resourceName>/<callbackName>`. `GetParentResourceName()` (a function available inside NUI pages) returns the resource name.

```js
// html/app.js
async function post(name, data = {}) {
    const response = await fetch(`https://${GetParentResourceName()}/${name}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json; charset=UTF-8' },
        body: JSON.stringify(data),
    });
    return response.json();
}

document.getElementById('close').addEventListener('click', async () => {
    document.getElementById('panel').style.display = 'none';
    await post('close', { reason: 'button' });
});
```

On the Lua side, register the callback. The handler receives the decoded body and a function `cb` that sends the response back to the page:

```lua
-- client.lua
RegisterNUICallback('close', function(data, cb)
    SetNuiFocus(false, false)
    print(('panel closed: %s'):format(tostring(data.reason)))
    cb({ ok = true })
end)
```

**Always call `cb`**, even with an empty table (`cb({})`). The page's `fetch` waits for it, and a callback that never answers leaves the request hanging.

Two registration functions exist:

| Function | Notes |
|---|---|
| `RegisterNUICallback(name, fn)` | Classic form. Internally it listens to the event `__cfx_nui:<name>`. |
| `RegisterNuiCallback(name, fn)` | Wrapper over the newer native. Same handler signature `(data, cb)`. |

Both run the handler inside a scheduler coroutine, so `Wait` and `Citizen.Await` are allowed before calling `cb`. Errors in a handler are printed as "error during NUI callback".

## Focus

`SetNuiFocus(hasFocus, hasCursor)` gives your page the keyboard focus and/or shows the mouse cursor:

| Call | Effect |
|---|---|
| `SetNuiFocus(true, true)` | Page gets keyboard and mouse; game controls are blocked |
| `SetNuiFocus(true, false)` | Keyboard focus without cursor |
| `SetNuiFocus(false, false)` | Give control back to the game |

`SetNuiFocusKeepInput(true)` keeps passing input to the game while the page has focus (for HUD-like overlays you can type into while driving, for example). Use it sparingly: the player can type into the page and move at the same time.

Always release focus when your UI closes, including when the resource stops:

```lua
AddEventHandler('onResourceStop', function(resourceName)
    if resourceName == GetCurrentResourceName() then
        SetNuiFocus(false, false)
    end
end)
```

A common bug is a UI that cannot be closed because the page lost track of its state. Give players a way out (an Escape handler in the page that calls a `close` callback).

```js
window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
        post('close', { reason: 'escape' });
    }
});
```

## Manifest options

| Directive | Effect |
|---|---|
| `ui_page 'path-or-url'` | The page of this resource. |
| `ui_page_preload 'yes'` | Create the page frame as soon as the resource starts, instead of only preparing it. |
| `nui_callback_strict_mode 'true'` | Only your own page may call your NUI callbacks; requests from other resources' pages are refused. |
| `loadscreen 'path'` | Use an HTML file as the loading screen (see `cfxdoc:fxmanifest`). |
| `loadscreen_manual_shutdown 'yes'` | Keep the loading screen until a script calls `ShutdownLoadingScreenNui()`. |
| `loadscreen_cursor 'yes'` | Show the mouse cursor on the loading screen. |

## Debugging

- Open the NUI developer tools with the client console command `nui_devTools`, or browse to `http://localhost:13172/` while the game runs.
- `console.log` output from the page appears in those developer tools, not in the game console.

## Security

Everything that comes from the page runs on the player's machine and can be forged: the callback data is **client input**. If a callback leads to a server action (buy, withdraw, spawn), send a request to the server and validate it there, exactly like any other network event (see `cfxdoc:security`).

## Sources

- NUI overview and full-screen NUI: https://docs.fivem.net/docs/scripting-manual/nui-development/full-screen-nui/
- NUI callbacks: https://docs.fivem.net/docs/scripting-manual/nui-development/nui-callbacks/
- Loading screens: https://docs.fivem.net/docs/scripting-manual/nui-development/loading-screens/
- SET_NUI_FOCUS: https://docs.fivem.net/natives/?_0x5B98AE30
- Resource manifest (`ui_page`, `files`): https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
- NUI callback strict mode and preload (`ResourceUI.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/nui-resources/src/ResourceUI.cpp
