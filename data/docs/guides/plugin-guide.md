# Using Sensity DevKit

Sensity DevKit turns IntelliJ-based IDEs into a FiveM and RedM (Cfx.re) Lua workbench. It understands the CfxLua dialect, knows every native function of the game you target, and separates client code from server code so that completion only offers what can actually run where you are typing.

This guide describes what the plugin does and how to configure it. The language and runtime themselves are covered in the other guides, starting with `cfxdoc:cfxlua-syntax` and `cfxdoc:natives`.

## Quick start

1. Open the folder that contains your resources (a single resource, or a whole `resources/` tree).
2. Make sure every resource has an `fxmanifest.lua`. The plugin reads it to find out the target game and which files run on the client, the server or both.
3. Open any `.lua` file. Natives, the Cfx runtime API (`Citizen`, events, exports, state bags, vectors, `json`, `msgpack`, `promise`) and the CfxLua syntax extensions are available immediately.

The first time the IDE starts, the plugin builds the native stubs in the background. While that runs you can already edit; completion for natives appears as soon as the stubs are ready.

## Project settings

Open **Settings > Languages & Frameworks > CfxLua**. Settings are stored per project.

| Setting | Values | Effect |
|---|---|---|
| Game | Auto, FiveM, RedM, Both | Selects which client natives are loaded. **Auto** reads `game` / `games` in your manifests. |
| Client natives | on / off | Loads the client native set of the selected game(s). |
| Server natives | on / off | Loads the FXServer natives (identical for FiveM and RedM). |
| Runtime API | on / off | Loads the hand-written stubs for `Citizen`, events, exports, state bags, NUI, vectors, `json`, `msgpack`, `promise` and the CfxLua standard library extensions. |
| Completion options | various | Fine-tunes what completion offers, for example whether deprecated native aliases are suggested. Deprecated names always stay resolvable, so existing code is never flagged as an unknown global. |

### How Auto detects the game

With **Auto**, the plugin scans every `fxmanifest.lua` (and legacy `__resource.lua`) in the project:

| Manifest content | Native set used |
|---|---|
| `game 'gta5'` | FiveM client natives (GTA V) |
| `game 'rdr3'` | RedM client natives (RDR3) |
| `games { 'gta5', 'rdr3' }` | Both sets |
| `game 'common'` | No game natives, only Cfx natives and the runtime API |

Every modern `fx_version` (`adamant` and later) requires a `game` or `games` entry, so a manifest without one is worth fixing anyway. If a project mixes FiveM and RedM resources, pick **Both** or let Auto do it. Choose a fixed value when you want to force one game regardless of the manifests.

## Natives: automatic updates

Native definitions are not frozen inside the plugin. At IDE start, Sensity DevKit checks the official Cfx.re native databases:

- GTA V natives and Cfx natives (client, server and shared) from the files that power `docs.fivem.net/natives`;
- RedM natives from the definition file the RedM runtime itself is built from, with descriptions joined from the RDR3 NativeDB by hash.

The check runs in the background, at most once per IDE session, and uses conditional HTTP requests, so nothing is downloaded when the data has not changed. The stubs are regenerated only when a source actually changed.

When the network is unavailable, the plugin falls back, in order, to:

1. the copy downloaded during a previous session;
2. a snapshot bundled with the plugin.

You can always force a refresh with the **Sensity DevKit: Update Natives** action (press `Ctrl+Shift+A` / `Cmd+Shift+A` and type the name). A notification tells you where the data came from (remote, cache or bundled snapshot) and how many functions were generated.

## Client, server and shared files

FiveM and RedM run the same Lua dialect on both sides, but the available functions differ: `TriggerServerEvent` only exists on the client, `TriggerClientEvent` only on the server, game natives such as `PlayerPedId` are client-only, and server natives such as `GetPlayerIdentifierByType` are server-only.

Sensity DevKit decides the side of each file as follows:

1. **Manifest entries.** Files matched by `client_script(s)`, `server_script(s)` or `shared_script(s)` in the resource's `fxmanifest.lua` get that side. Globs are honoured (`client/*.lua`, `**/*.lua`, `**/cl_*.lua`).
2. **Heuristics** for files not listed in a manifest (for example while you are still writing it):
   - a folder named `client`, `server` or `shared` somewhere in the path;
   - a file name prefix `cl_`, `sv_` or `sh_`.
3. Files that cannot be classified are treated as **shared**.

Completion, resolution and quick documentation follow the side: a client file sees client natives and the client runtime API, a server file sees server natives and the server runtime API. A shared file sees both, so guard side-specific calls:

```lua
if IsDuplicityVersion() then
    -- server-only code
    print(('running on the server, %d players'):format(#GetPlayers()))
else
    -- client-only code
    print(('my server id is %d'):format(GetPlayerServerId(PlayerId())))
end
```

`fxmanifest.lua` itself is recognized as a manifest: completion offers the manifest directives (`fx_version`, `client_scripts`, `ui_page`, `data_file`, ...) with their allowed values. See `cfxdoc:fxmanifest`.

## The Sensity tool window

Its button is on the left tool window bar (the window is docked there, pinned, the first time a project is opened; drag the button elsewhere if you prefer, the IDE remembers it). You can also open it with **View > Tool Windows > Sensity**, **Tools > Sensity DevKit: Search Natives...** or **Help > Sensity DevKit Documentation**. It has six tabs:

| Tab | Content |
|---|---|
| Natives | Search natives by Lua name (`GetEntityCoords`), native name (`GET_ENTITY_COORDS`), camel humps (`GEC`) or hash (`0x3FEF770D40960D5A`), filtered by set (the project's sets by default), namespace and side. Shows the Lua call with its return values, the parameter and return types (Lua and native), the description, the example, the aliases and the links. |
| Runtime API | The non-native API by part (shared, client, server, manifest): `Citizen`, scheduler, events, exports, state bags, NUI, vectors, `json`, `msgpack`, `promise`. Double click opens the declaration. |
| Events | The catalog of built-in events (resource lifecycle, player connection, OneSync game events, ...) with their parameters, side, game and whether they can be canceled. |
| Manifest | The `fxmanifest.lua` directives with their allowed values and examples. |
| Syntax | The CfxLua syntax extensions with examples and pitfalls. |
| Guides | These guides. |

In the lists, the arrow keys move the selection from the search field, `Enter` inserts the selected item at the caret of the active editor (a native call with placeholders for its arguments and return values, an event handler, a directive, an example) and `Ctrl+C` / `Cmd+C` copies its name. The toolbar above the documentation has the other actions: copy the signature or the hash, go to the declaration, open the online documentation.

Right-click a native call or a built-in event name in the editor and choose **Show in Sensity Docs** to open it in the tool window.

Reading comfort:

- When the tool window is wide (700 pixels and more), the list and the documentation are side by side; narrower, the list is above. Each layout remembers where you put the divider.
- The **Decrease / Increase Documentation Font Size** buttons (magnifiers) of the toolbar change the size of the text, for the tool window and the editor tabs; the **Reset** button restores it. The size is remembered, and shortcuts can be assigned in **Settings > Keymap**.
- **Open in Editor Tab** (toolbar) opens the selected native, event, directive, syntax feature, guide or runtime declaration in an editor tab, for wide reading; a double click on a guide does the same. Each item has one tab, reused when you open it again.
- The documentation follows the editor color scheme and the IDE zoom. It is displayed by the embedded browser when available, by a built-in renderer otherwise.

## Finding and reading natives

- **Search Everywhere** (`Shift` twice) has a **Natives** tab that finds natives by their Lua name (`GetEntityCoords`), their original name (`GET_ENTITY_COORDS`), their camel humps (`GEC`) or their hash (`0x3FEF770D40960D5A`). `Enter` opens the generated stub, `Shift+Enter` the Sensity tool window.
- **Quick Documentation** (`Ctrl+Q`, macOS `F1`) on a native shows its typed signature, the Lua and native type of each parameter and return value (including out-parameters, see `cfxdoc:natives`), its namespace, side, game, hash and original name, the description, a Lua example when one exists, and links to the tool window and the online reference. On a backtick hash literal it shows the signed, unsigned and hexadecimal values; on a built-in event name (`AddEventHandler('onResourceStart', ...)`) the event documentation.
- **Completion** inside the event name of `AddEventHandler`, `RegisterNetEvent`, `TriggerEvent`, `TriggerServerEvent` and `TriggerClientEvent` offers the built-in events of the script's side and the event names already used in the project (and, as described in Network events below, the keys of the project's own event helpers).
- **Parameter Info** (`Ctrl+P`, macOS `Cmd+P`) shows the Lua parameters while you type the arguments. Out-pointer parameters are not listed because Lua callers do not pass them.
- **Go to Declaration** (`Ctrl+B` / `Cmd+B`) opens the generated stub.

Deprecated names (old `_NAMES` and `N_0x...` hash aliases) are still resolved, are shown struck through, and the documentation points to the current name.

## Network events

Events tie resources together through plain strings. The plugin indexes every call that registers, handles, triggers or aliases an event whose name is written literally, in all the resources of the project, and connects them:

- **Ctrl+click** (Go to Declaration) on the event name of a trigger opens the handlers it reaches; on a handler, the triggers reaching it. `TriggerServerEvent` leads to server handlers, `TriggerClientEvent` to client handlers, `TriggerEvent` to handlers of the same side.
- **Gutter icons** show "N handlers" next to a trigger and "N triggers" next to a handler; click one to list them.
- **Find Usages** (`Alt+F7`) on an event name lists its handlers, triggers, `RegisterNetEvent` registrations and aliases.
- **Rename** (`Shift+F6`) on an event name renames it in every resource. It is refused, with the place that prevents it, when one occurrence is not a literal written at the call (a variable, a computed name).
- **Quick Documentation** (`Ctrl+Q`) on an event name shows where it is handled and triggered, on which side, and how many values are sent and taken.
- **Completion** of event names in the arguments of the event functions and of the project's helpers.
- **Search Everywhere** has an **Events** tab listing the events of the project.

Names built at runtime (`'shop:' .. action`) are never guessed: the features above ignore them and the checks below stay silent on the events they may stand for.

### Event helpers

Many projects wrap the runtime functions in their own helpers, with keys made of several arguments, network names computed or obfuscated by the helper, or values passed to the callback before the event values. The plugin detects such helpers by following the parameters of each function to the event name of `AddEventHandler`, `RegisterNetEvent` and `Trigger*Event`, through local variables, string concatenations, closures, and other helpers:

```lua
Events = {}

-- detected: a handler helper whose callback receives the player first;
-- Events.On('shop', 'buy', ...) handles the network event 'evt:shop:buy'
function Events.On(category, name, callback)
    RegisterNetEvent('evt:' .. category .. ':' .. name, function(...)
        callback(source, ...)
    end)
end

-- detected: a trigger helper sending to the server
function Events.Send(category, name, ...)
    TriggerServerEvent('evt:' .. category .. ':' .. name, ...)
end

-- used as
Events.On('shop', 'buy', function(playerId, item) end)
Events.Send('shop', 'buy', 'bread')
```

Methods (`Player:TriggerEvent(name, ...)`), keys at any position (`Zone:TriggerEvent(area, category, name, ...)`) and project redefinitions of the runtime functions (a client `function TriggerServerEvent(category, name, ...)`) are followed too; a redefinition applies to the files of its resource and of the resources loading it with `@resource/file.lua`. Calls of an alias function such as `RegisterEventAlias('legacy_name', 'shop', 'buy')` declare that a network name and a key are the same event.

When a helper can't be detected (its transport is outside of the project) or is detected wrongly, describe it with a `---@cfx.event` annotation:

```lua
Bridge = {}

---@cfx.event handler server key(category, name) callback(cb) inject(1)
function Bridge.Listen(category, name, cb)
    exports.bridge:Listen(category .. '/' .. name, cb)
end
```

The words after `---@cfx.event` are: the kind (`handler`, `trigger`, `register` or `alias`); `net` or `local`; `server`, `client` or `both` (for a handler, where it is used; for a trigger, where its handlers run); `key(...)` the parameters forming the key (one parameter is a network name, several form a key whose network name the helper computes); `callback(p)` the handler function; `inject(n)` the number of values the helper passes to the callback before the event values; `target(p)` the target player of a trigger; `payload(p)` its first value sent (`payload(...)` for the vararg); `wire(p)` the network name of an alias. Parameters are given by name or 1-based position.

The same can be declared without touching the code in **Settings > Languages & Frameworks > CfxLua > Events**, which also lists the helpers detected in the project with their `---@cfx.event` description, can turn the detection off, and holds the list of alias functions.

### Event checks

Under **Settings > Editor > Inspections > CfxLua events**:

- **Event triggered but never handled**: a trigger with no handler in the project. Only events of a namespace the project handles are reported (`shop:buyy` when `shop:buy` is handled), so events of frameworks and of the game stay quiet.
- **Network event handled without RegisterNetEvent**: `AddEventHandler` for an event triggered from the other side, which the runtime drops ("event was not safe for net"). The quick fix uses `RegisterNetEvent(name, handler)`.
- **Event function called from the wrong side**: `TriggerServerEvent` in a server script, `TriggerClientEvent` in a client script.
- **Event values sent and taken differ**: the number of values sent differs from the parameters of every handler, values injected by a helper excluded. Only reported when both are known.
- **Event handled twice in a resource**: a second handler of a requested event in the same resource and side.

## NUI bridge

A NUI page and its client script only talk through strings (see `cfxdoc:nui`): the page calls Lua callbacks by name, and Lua sends messages whose `action` (or `type`) the page compares. The plugin reads both sides, Lua and the page's JavaScript, TypeScript, JSX, Vue, Svelte and HTML scripts, and connects them within each resource:

```lua
-- client.lua
RegisterNUICallback('buyItem', function(data, cb)
    TriggerServerEvent('shop:buy', data.item)
    cb(true)
end)

SendNUIMessage({ action = 'openShop', items = {} })
```

```ts
// web/src/App.tsx
useNuiEvent('openShop', (data) => setItems(data.items))
const buy = (item: string) => fetchNui<boolean>('buyItem', { item })
```

- **Ctrl+click** on a callback name goes from `RegisterNUICallback('buyItem')` to the page's calls (`fetchNui('buyItem')`, `fetch('https://${GetParentResourceName()}/buyItem')`, `$.post('https://my_resource/buyItem')`) and back; on a message name, from `SendNUIMessage({ action = 'openShop' })` to the listeners (`useNuiEvent('openShop')`, `case 'openShop':`, `event.data.action === 'openShop'`) and back.
- **Gutter icons** on the Lua side show "N calls from the page" and "N listeners in the page".
- **Find Usages** and **Rename** work on both sides at once, also from `AddEventHandler('__cfx_nui:buyItem', ...)`. Rename is refused, with the place that prevents it, when an occurrence is not written as is. A name written in a file outside of any resource is only renamed in the files outside of the resources.
- **Completion**: the page's callback names in `RegisterNUICallback('|')`, its message names in `SendNUIMessage({ action = '|' })`; with the JavaScript plugin, the Lua callbacks in `fetchNui('|')` and callback URLs, the Lua messages in `useNuiEvent('|')`, and in `case '|':` and `data.action === '|'` inside a `message` listener (a reducer's `case` strings are left to the language).

The JavaScript side is read from the text of the files, so navigation, gutter icons, Find Usages, rename and the Lua completion work without the JavaScript plugin; completion and checks inside the page's files need it. `node_modules` and minified bundles are skipped. A UI project kept outside of the resource folder is matched through the output folder of its `vite.config` / `webpack.config` (`outDir: '../my_resource/html'`, `outDir: path.resolve(__dirname, '../my_resource/html')`).

### Wrappers

Wrappers are detected from their code, in Lua and in the page:

- a function building the callback URL from `GetParentResourceName()` and one of its parameters is a callback wrapper (`fetchNui(eventName, data)`), as is a function passing its parameter to one (`callLua(name)` calling `request(name)`);
- a function whose `message` listener compares the message to one of its parameters is a message wrapper (`useNuiEvent(action, handler)`);
- a Lua function passing its parameter to `RegisterNUICallback` or to the `action` of `SendNUIMessage` is a Lua wrapper:

```lua
local function SendReactMessage(action, data)
    SendNUIMessage({ action = action, data = data })
end

SendReactMessage('openShop', { items = {} })
```

Calls are matched to the page's wrappers through imports (`import { fetchNui as nui } from './utils'`), and to the functions of plain scripts by name. Wrappers the plugin can't see (imported from a package, decorators such as `@OnNuiEvent('open')`) are declared in **Settings > Languages & Frameworks > CfxLua > NUI**, which also sets the message keys (`action` and `type` by default), the dev server port, and lists the wrappers detected in the project.

### NUI checks

Under **Settings > Editor > Inspections > CfxLua > NUI**:

- **NUI callback not answered on every path**: the function of `RegisterNUICallback` can end, or `return`, without calling `cb(...)`, so the page's request waits until it times out. Silent when `cb` is passed on, stored or called later from a nested function. The quick fix inserts `cb('ok')` (`respond({ status = 200, body = '' })` for `RegisterRawNUICallback`) before the end and before each such `return`, and adds the missing parameters as `_, cb`.
- **NUI callback not registered by a client script** (JavaScript plugin): the page calls a callback no client script of the resource registers. Silent when the resource registers callbacks with computed names, or none the plugin can see.
- **ui_page or its build not listed in files**: the page, or files of its build folder (`web/build/assets/*.css`), are not covered by `files`, and the NUI stays white. Hidden files, source maps, sources (`.scss`, `.ts`, `.vue`), notes and build manifests are not required. The quick fix adds the folder glob (`'web/build/**'`).

### Dev server

On the `ui_page` line of `fxmanifest.lua`, the intention **Switch ui_page to the dev server** (`Alt+Enter`) points the page at the Vite dev server (the `port` of the `server` block of `vite.config`, else the setting) for hot reload in game, keeping the build path in a comment; **Switch ui_page back to the build** restores the line exactly:

```lua
ui_page 'http://localhost:5173' -- ui_page build: 'web/build/index.html'
```

## CfxLua inspections

The plugin adds inspections for mistakes that the runtime accepts silently or reports confusingly. They are listed under **Settings > Editor > Inspections > CfxLua**.

### Compound assignment on a `<const>` variable

The runtime does not reject `+=` on a constant: depending on how the constant was initialised, the assignment either does nothing or really changes the "constant". Both are bugs, so the plugin reports an error.

```lua
local MAX <const> = 10
MAX += 1 -- reported: compound assignment to a <const> variable
```

### `<close>` combined with `in` unpacking

`local a <close> in t` compiles, but the runtime ignores the `<close>` attribute in this form: the value is never closed. Use a regular declaration instead.

```lua
local handle <close> in resources        -- reported: <close> is ignored here
local handle2 <close> = resources.handle  -- closed when the scope ends
```

### `^=` is bitwise XOR

In CfxLua `^` alone is exponentiation, but `^=` is **XOR**. `x ^= 2` sets `x` to `x ~ 2`, not `x ^ 2`. The plugin warns on `^=` so that the intent is explicit; when you want a power, write `x = x ^ 2`.

```lua
local area = 3
area ^= 2   -- area is now 1 (3 XOR 2), not 9
```

### `i--` starts a comment

There is no decrement operator. `--` always starts a comment, so `i--` is the expression `i` followed by a comment, and any code after it on the same line is ignored.

```lua
local i = 5
local j = i-- this is a comment, j == 5 and i is unchanged
i -= 1 -- the correct way
```

### `++` is not supported

`i++` and `++i` are syntax errors in CfxLua. The plugin reports them with an explicit message instead of a generic parse error; write `i += 1` instead.

### Asset Escrow

For resources sold through Cfx Asset Escrow (manifests declaring `escrow_ignore`), the Project view shows whether each file will be encrypted or stay open, inspections report configuration files and files loaded with `LoadResourceFile` or `require` that would be encrypted, and **Tools > Sensity DevKit: Check Resource Before Escrow Upload** produces a pre-publish report. Encrypted files of protected builds (content starting with `FXAP`) open as read-only plain text instead of failing to parse. See `cfxdoc:escrow`.

## Coordinates toolbox

### Pasting coordinates

Paste coordinates copied from the game, txAdmin, a Discord message or a web tool where a Lua value is expected (after `=`, in a table, in an operation) and they become a vector:

| Clipboard | Pasted |
|---|---|
| `123.4, 56.7, 8.9` | `vector3(123.4, 56.7, 8.9)` |
| `123.4, 56.7, 8.9, 90.0` | `vector4(123.4, 56.7, 8.9, 90.0)` |
| `{"x": 1.0, "y": 2.0, "z": 3.0, "heading": 90.0}` | `vector4(1.0, 2.0, 3.0, 90.0)` |
| `x: 1.0 y: 2.0 z: 3.0`, `{x = 1.0, y = 2.0, z = 3.0}`, `Vector3(1.0, 2.0, 3.0)` | `vector3(1.0, 2.0, 3.0)` |

Several lines of coordinates pasted into a table become one entry per line. In the x, y, z arguments of a native (`SetEntityCoords(ped, |)`), a pasted vector becomes the separate numbers, with the heading only when the native takes one next (`CreatePed`, `CreateVehicle`). Nothing changes inside strings and comments, for text that isn't clearly coordinates (integers alone, two numbers, a sentence), or for a bare number list pasted where several values make sense (`local x, y, z = |`, function arguments). One **Undo** gives back the clipboard text; **Paste as Plain Text** (`Ctrl+Alt+Shift+V` / `Cmd+Alt+Shift+V`) never converts. Turn it off in **Settings > Languages & Frameworks > CfxLua > Coordinates**.

### Intentions

Press `Alt+Enter` on a vector or a coordinates table to convert it between `vector3(…)` / `vector4(…)`, `{x = …, y = …, z = …}` and `{x, y, z}`, add or remove the heading, round the numbers (2 decimals by default, set in the same settings page) or copy them as JSON. On a vector passed where a native takes x, y and z, **Spread into x, y, z arguments** writes the components; on `GetDistanceBetweenCoords` and `Vdist`, **Replace with #(a - b)** uses vector math:

```lua
local ped = PlayerPedId()
local coords = vector3(-268.4, -957.3, 31.2)
SetEntityCoords(ped, coords.x, coords.y, coords.z, false, false, false, false)
local distance = #(GetEntityCoords(ped) - coords)
print(('%.1f m away'):format(distance))
```

### Checks

Three weak warnings, under **Settings > Editor > Inspections > Lua > CfxLua > Coordinates**:

- a world position with `z = 0.0`, usually a forgotten height (offsets, rotations, scales and colors are left alone);
- coordinates outside of the map of the resource's game (approximate and generous areas, islands included), which catches typos and FiveM positions in a RedM resource;
- the same coordinates twice in a `Config` table for the same key, usually a copied entry never updated (the table names are an option of the inspection).

### Zone names

With a zones file, an inlay hint shows the GTA V zone after literal coordinates (`vector3(213.8, -808.5, 31.0)` followed by `≈ Pillbox Hill`). No zone data ships with the plugin: download a `zones.json` (for example the one of the community GTA V data dumps), choose it in **Settings > Languages & Frameworks > CfxLua > Coordinates**, then turn on **Settings > Editor > Inlay Hints > Other > Lua > GTA V zone names**. Zones are bounding boxes, so names are approximate near borders.

## Performance inspections

A second group, **Settings > Editor > Inspections > CfxLua > Performance**, finds what makes a resource expensive in `resmon`. It only reports patterns that are clearly wasteful: a loop calling code the plugin can't follow (exports, framework functions it can't resolve, `TriggerEvent`) is left alone. What a native does (drawing, control input, `*ThisFrame`, "must be called every frame" in its documentation, reading or changing the game state) comes from the natives catalog; before the natives are generated, natives are classified by their name.

| Inspection | Reports | Quick fix |
|---|---|---|
| Loop running every frame without per-frame work | `Wait(0)` loops that only read state, compare distances or send events | Use adaptive sleep, Wait longer between iterations |
| Endless loop that never waits | `while true` loops where an iteration can end without `Wait`, `while not HasModelLoaded(m)` loops without `Wait` (both freeze) | Insert `Wait(0)` |
| Native called repeatedly in the same frame | `PlayerPedId()`, `GetEntityCoords(ped)`... called twice in one iteration, `GetPlayerPed(-1)` | Cache in a local, Replace with `PlayerPedId()` |
| Distance computed with a native | `GetDistanceBetweenCoords`, `Vdist` | Replace with `#(a - b)` (`vector2` when `useZ` is false) |
| Logging, serialization or network call every frame | `print`, `json.encode`, `TriggerServerEvent`, KVP, HTTP or database calls on every frame | Remove the `print` call |
| Handler or thread registered repeatedly | `AddEventHandler` or an endless `CreateThread` inside an event handler, a command or a loop | |

The **adaptive sleep** fix keeps the loop running every frame near the position its distance checks compare to, and makes it sleep (1000 ms by default) when the player is far away. It is offered, without highlighting, on loops whose per-frame work (a marker, a key press) only runs near a position:

```lua
local shop = vector3(25.7, -1347.3, 29.5)

CreateThread(function()
    while true do
        local sleep = 1000
        local dist = #(GetEntityCoords(PlayerPedId()) - shop)
        if dist < 10.0 then
            sleep = 0
            DrawMarker(1, shop.x, shop.y, shop.z - 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 255, 0, 0, 100, false, false, 2, false, nil, nil, false)
        end
        Wait(sleep)
    end
end)
```

The options of **Loop running every frame without per-frame work** set the sleep and throttle delays and list the functions of your framework that only read state (for example `ESX.GetPlayerData`), so that loops calling them are checked too.

Long-running loops (`while true`, `while active`, `repeat ... until false`) also show their cost in the gutter: every frame, adaptive, throttled or event-driven; hover the icon for the reason. Turn the icons off in **Settings > Editor > General > Gutter Icons > CfxLua loop cost**. See `cfxdoc:performance` for the patterns behind these checks.

## Annotating your code with EmmyDoc

Sensity DevKit inherits the EmmyLua type system. Type annotations are Lua comments that start with three dashes, so they never change runtime behaviour. They drive completion, parameter info, inspections and quick documentation for your own code.

### `---@class` and `---@field`

```lua
---@class PlayerData
---@field source integer @server id
---@field name string
---@field job string
---@field cash integer
local PlayerData = {}

---@class Garage
---@field private vehicles table<string, integer>
---@field capacity integer
```

A class can extend another one with `---@class Child : Parent`.

### `---@param` and `---@return`

```lua
---@param src integer @server id of the player
---@param amount integer
---@param reason string|nil @optional reason, written to the log
---@return boolean, string @success, error message
local function removeCash(src, amount, reason)
    if amount <= 0 then
        return false, 'invalid amount'
    end
    return true, ''
end
```

The text after `@` is a free comment shown in the documentation. Optional parameters are written as a union with `nil` (`string|nil`), or LuaCATS style with `---@param reason? string` (see below).

### `---@type`

```lua
---@type table<integer, PlayerData>
local players = {}

---@type vector3
local spawnPoint = vector3(-268.4, -957.3, 31.2)
```

### `---@alias`

```lua
---@alias JobName string
---@alias PlayerMap table<integer, PlayerData>

---@param job JobName
local function setJob(job) end
```

### `---@overload`

```lua
---@param model string|integer
---@param coords vector3
---@return integer
---@overload fun(model: string|integer, x: number, y: number, z: number): integer
local function spawnVehicle(model, coords) end
```

### `---@generic`

```lua
---@generic T
---@param list T[]
---@param predicate fun(value: T): boolean
---@return T|nil
local function find(list, predicate)
    for _, value in ipairs(list) do
        if predicate(value) then
            return value
        end
    end
    return nil
end
```

### `---@deprecated`

```lua
---@deprecated use removeCash instead
local function takeMoney(src, amount) end
```

Calls to a deprecated function are shown struck through.

### Typing exports and events

Annotations are especially useful at the boundaries between resources, where nothing else tells the IDE what a value is:

```lua
---@class InventoryItem
---@field name string
---@field count integer

---@param src integer
---@return InventoryItem[]
local function getItems(src)
    return {}
end

exports('getItems', getItems)

RegisterNetEvent('myinv:requestItems', function()
    ---@type integer
    local src = source
    TriggerClientEvent('myinv:items', src, getItems(src))
end)
```

## LuaCATS annotations (Lua Language Server)

Frameworks such as ox_lib, ox_inventory, qbx_core or ESX ship their API annotated for the Lua Language Server, in the LuaCATS dialect. Sensity DevKit reads LuaCATS next to EmmyDoc: the types of these libraries work as soon as their files are in the project, and you can use either style in your own code.

```lua
---@alias NotifyPosition 'top' | 'top-right' | 'bottom'

---@alias NotifyType
---| 'inform' # Blue
---| 'error' # Red
---|>'success' # The default

---@class NotifyProps
---@field title? string
---@field position? NotifyPosition
---@field type? NotifyType

---@enum Keybind
local Keybind = { PRESS = 0, HOLD = 1 }

---@param data NotifyProps
---@param duration? integer Milliseconds, 3000 when omitted
---@return boolean shown
---@return string? reason Why it was not shown
local function notify(data, duration)
    return true, nil
end

---@param mode Keybind
local function bind(mode) end

local shown, reason = notify({ title = 'Saved', position = 'top' })
bind(Keybind.HOLD)
```

What the plugin does with these annotations:

- **Optional parameters and fields** (`name?`, or a type ending with `?`): calls may omit them without a missing-argument report, and parameter info shows `duration?`.
- **Returns**: each `---@return` line is one returned value, so `local shown, reason = notify(...)` types both variables. Return names (`shown`, `reason`) are shown in the quick documentation.
- **String literal types and aliases**: inside a string argument, completion offers the allowed values (`'top'`, `'top-right'`, ...), also for the fields of an option table (`notify({ position = '' })`). The `---|` lines of an alias or of a parameter list the values, `#` starts their description.
- **Option tables**: when a parameter is a class, completion inside the table constructor offers its fields (`notify({ })` offers `title = `, `position = `, ...), without the keys already set.
- **Enums**: `---@enum Name` on a table makes its fields the values of the type: where a `Keybind` is expected, completion offers `Keybind.PRESS` and `Keybind.HOLD`. With `---@enum (key) Name`, the values are the keys, as strings.
- **Several parents**: `---@class Child: Base, Mixin` gives `Child` the members of every parent.
- **Index signatures**: with `---@field [string] Item` (or the type `{ [string]: Item }`), `t.anything` and `t[key]` are typed `Item`.
- **Casts**: `---@cast name Type` changes the type of the variable after the annotation; `+Type` and `-Type` add or remove a type.
- **Definition files**: a file annotated with `---@meta` only declares an API: unused parameters, missing returns and globals that could be local are not reported there.
- Literal types (`1`, `true`, `'name'`), `integer`, tuples (`[string, integer]`), `fun(x?: T, ...: U): R` and `---@class (exact)` are understood. `---@async`, `---@nodiscard`, `---@diagnostic`, `---@package`, `---@version`, `---@source`, `---@operator` and `---@module 'name'` are accepted without error but have no effect yet.

```lua
---@param value string|number
local function describe(value)
    if type(value) == 'string' then
        ---@cast value string
        return value:upper()
    end
    return tostring(value)
end
```

## Troubleshooting

| Symptom | What to check |
|---|---|
| Natives are unknown globals | Settings > CfxLua: client/server natives enabled? Is the file classified on the expected side? Check the event log for the last natives update. |
| RedM natives missing | Set Game to RedM or Both, or add `game 'rdr3'` to the manifest. |
| A RedM native from NativeDB is "unknown" | Its NativeDB name may not exist in the RedM runtime. See `cfxdoc:redm-vs-fivem`. |
| Client natives offered in a server file | The file is matched by a client or shared entry, or by a heuristic. Add it to `server_scripts` in the manifest. |
| Stale native documentation | Run **Sensity DevKit: Update Natives**. |

## See also

- `cfxdoc:cfxlua-syntax` for the language extensions behind the inspections
- `cfxdoc:natives` for how natives are called from Lua
- `cfxdoc:fxmanifest` for manifest directives

## Sources

- EmmyLua documentation (annotations): https://emmylua.github.io
- LuaCATS annotations (Lua Language Server): https://luals.github.io/wiki/annotations/
- FiveM native reference: https://docs.fivem.net/natives/
- RDR3 NativeDB: https://alloc8or.re/rdr3/nativedb/
- Resource manifest reference: https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
