# Using OpenIDEA-CfxLUA

OpenIDEA-CfxLUA turns OpenIDEA into a FiveM and RedM (Cfx.re) Lua workbench. It knows every native of the game you
target, separates client code from server code so that completion only offers what can run where you are typing,
links events, state bags and NUI messages across your resources, and checks the mistakes the Cfx.re runtime does not
report. This guide describes what it does and where to find it.

## Getting set up

OpenIDEA-CfxLUA needs two things:

- OpenIDEA built from source at revision `859f634`: the plugin needs plugin API 1.0, which no OpenIDEA release has yet and later revisions may still change;
- [lua-language-server](https://luals.github.io/#install) on your `PATH` (restart OpenIDEA after installing it).

The plugin brings the Cfx.re knowledge; lua-language-server brings the Lua engine (see *Working with
lua-language-server* below).

1. Install the plugin: **Settings › Plugins › Install Plugin from Package File…** and pick the `.oiplugin` file.
   Or, in **Settings › Plugins**, gear menu › **Plugin Repository…**, set the marketplace index URL to
   `https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/download/marketplace/index.json` and install
   **OpenIDEA-CfxLUA** from **Available**: that copy is signed by `sensity`, and new versions are offered as
   updates. OpenIDEA reads one marketplace at a time; leave the URL empty to go back to its default one.
2. Accept the permissions. `project.read`, `editor.read`, `editor.write`, `storage` and `events` are needed;
   `net:static.cfx.re` and `net:raw.githubusercontent.com` are only used to download natives updates, and
   `clipboard` only by the copy buttons of the tool window.
3. Open a folder that contains resources — a whole `server-data` folder works. The plugin reads the manifests, the
   Lua scripts and the NUI pages in the background (*CfxLua: indexing resources*), the resource you open first. In a
   big project this goes on while you work: events, NUI callbacks and state bags of the whole project fill in within
   the first minutes; natives are there at once. Files over 1 MB (generated data, bundles) are not indexed.

Every option is in **Settings › Plugins › OpenIDEA-CfxLUA** and can be overridden per project. The master switch,
*Enable FiveM / RedM support*, turns everything off for projects that are not resources.

## The game and the natives

The plugin bundles the Cfx.re native databases: GTA V and CFX client natives for FiveM, RDR3 and CFX client natives
for RedM, and the FXServer natives for server scripts. Each native has its real Lua name (`GetGroundZFor_3dCoord`,
`N_0x…` for unnamed ones), typed parameters, its return values — out-parameters are returned the way Lua does it, see
`cfxdoc:natives` — its description, its Lua example and a link to the official reference.

**Game.** The *Game* setting is *auto* by default: the plugin reads the `game` / `games` directives of every manifest
of the project (FiveM when none says), and a resource that declares its own game only sees that game's natives. You
can force *FiveM*, *RedM* or *both*. A FiveM project never sees RedM natives, and the other way round.

**Completion.** Type a few letters of a native in a Lua script: the list shows the natives your file can call, each
with its signature and its full documentation. Camel humps work: `ggz` finds `GetGroundZFor_3dCoord`. Old names
(deprecated aliases) and unnamed `N_0x…` natives are hidden unless you turn them on in the settings; existing scripts
that use them are never flagged.

**Updates.** **CfxLua: Update Natives** (Tools menu, Find Action, or the update button of the Natives tab) downloads
the databases again — conditional requests, so nothing is downloaded when nothing changed — and a background check
does it once a day (*Check for new natives automatically*). Offline, the bundled snapshot is used. A notification says where the natives
came from and how many functions each set has.

## Client, server and shared files

Every Lua file of a resource is client, server or shared, decided like this:

1. the resource's `fxmanifest.lua` (or `__resource.lua`): `client_script(s)`, `server_script(s)` and `shared_script(s)`,
   with globs such as `client/*.lua` or `**/cl_*.lua`. The nearest manifest above the file wins; a file listed as both
   client and server is shared;
2. otherwise the file name: `client.lua`, `cl_*.lua` are client, `server.lua`, `sv_*.lua` server, `shared.lua`,
   `sh_*.lua` shared;
3. otherwise the nearest folder inside the resource: `client/`, `cl/`, `server/`, `sv/`, `shared/`, `sh/`.

A file that matches none of these is treated as shared: everything is offered.

The **status bar** shows the side of the focused file, its resource and its game — `Client script · shop · FiveM` —
and its tooltip says why (listed in the manifest, or a naming convention). Clicking it runs
**CfxLua: Show Documentation**. In the
project view, file icons mark manifests and client, server and shared scripts named by convention.

The side drives completion: a client script is never offered server natives or the server runtime API, and the other
way round. Calling a server native from a client script (or a client native from a server script) is reported.

## The CfxLua tool window

The **CfxLua** tool window (its button is on the left tool window bar) has seven tabs:

| Tab | What it shows |
|---|---|
| Natives | Every native of the sets your project uses, or all sets. Search by name, `NATIVE_NAME`, hash (`0xC906…`) or camel humps; filter by namespace and side. The sheet lists parameters with their Lua and native types, returns, description, example and details. |
| Runtime API | `Citizen`, events, exports, state bags, `json`, `msgpack`, `promise`, vectors and the CfxLua standard library extensions, as a tree with their documentation. |
| Events | The built-in events (`onResourceStart`, `playerConnecting`, `gameEventTriggered`…) with their parameters, side, game and whether they can be cancelled. |
| Manifest | Every `fxmanifest.lua` directive with its values and an example. |
| Syntax | The CfxLua syntax features and their pitfalls. |
| Guides | These guides. |
| Escrow | The resources of the project, those using Asset Escrow first, with the pre-publish report of the selected one. |

**Insert** buttons put a call, an event handler, a directive or an example at the caret of the editor you were in.
**Copy** buttons copy a name, a signature, a hash or an example. Online documentation links show their address with a
copy button, because the tool window cannot open a browser. **A-** / **A+** change the reading size.

**CfxLua: Show Documentation** (editor context menu, Find Action, or a key you bind in **Settings › Keymap**) shows
in the tool window the native, built-in event, backtick hash literal or manifest directive at the caret, and sums it
up in a notification.

## Events

Event handlers and triggers are matched across the project: `AddEventHandler`, `RegisterNetEvent`,
`RegisterServerEvent`, `TriggerEvent`, `TriggerServerEvent`, `TriggerClientEvent` and their latent variants, plus the
helpers of your own code that forward an event name (detected automatically, or declared in the settings).

- Inside the event name string, completion lists the events of the project and the built-in events that the receiving
  side can get.
- **Go to Definition** (Ctrl+B) on an event name lists its handlers or its triggers; gutter icons on both ends jump to
  the other side.
- Checks report an event that is triggered but never handled, a network event triggered from the other side without
  `RegisterNetEvent` (with a quick fix), a trigger function called on the wrong side, handlers ignoring values that
  are sent, duplicate handlers, and mistakes in `---@cfx.event` annotations.

See `cfxdoc:events`.

## State bags

Keys written anywhere in the project — `Player(src).state:set('job', …)`, `GlobalState.weather = …`,
`Entity(ent).state.locked = …`, `LocalPlayer.state:set(…)` — are indexed with the type of the value written.

- After `.state.` (or in `state:set('`), completion lists the keys of that kind of bag with where they are set.
- **Go to Definition** on a key jumps to the places that write it.
- A key that is read but never written is reported, with up to three *Replace with* fixes for close names
  (`.state.jbo` → `job`).

See `cfxdoc:state-bags`.

## fxmanifest

In `fxmanifest.lua` and `__resource.lua`:

- completion of directive names with their documentation, of values (`fx_version 'cerulean'`, `game 'gta5'`, every
  `data_file` type…), of file paths and globs relative to the resource, of `@other_resource/…` paths and of dependency
  names; snippets for the usual blocks;
- **Go to Definition** on a path or a glob lists every file it matches; on a dependency, opens that resource's manifest;
- checks with quick fixes: missing `fx_version` or `game`, missing or wrong `rdr3_warning`, deprecated directives
  (`lua54`), misspelled directives, a single string given to a list directive, files that do not exist.

The **New** menu has templates for a FiveM `fxmanifest.lua`, a RedM one, a client script, a server script and a
shared config. See `cfxdoc:fxmanifest`.

## NUI

Lua client scripts and the page — JavaScript, TypeScript, JSX/TSX, Vue, Svelte or HTML, also when the UI project
lives outside the resource and builds into it (vite or webpack output folder) — are linked both ways:

- `RegisterNUICallback('close', …)` ↔ `` fetch(`https://${GetParentResourceName()}/close`) ``, `fetchNui('close')`;
- `SendNUIMessage({ action = 'open' })` ↔ `case 'open':`, `if (data.action === 'open')`, `useNuiEvent('open')`.

Completion offers the names of the other side, **Go to Definition** jumps across, and gutter icons in Lua show how many
calls or listeners the page has. Checks report a callback the page calls but no client script registers, a callback
that does not answer on every path (with a fix adding the missing `cb(…)` calls), and a `ui_page` or its build missing
from `files`. On the `ui_page` line, a quick action switches between the build and the dev server. The message keys,
wrapper functions and dev server port are in the settings. See `cfxdoc:nui`.

## CfxLua syntax checks

lua-language-server does not know CfxLua, so the plugin checks what matters:

| You write | What happens | Reported |
|---|---|---|
| `i--` | starts a comment | warning, fix to `i -= 1` |
| `i++` / `++i` | syntax error | fix to `i += 1` |
| `x ^= 2` | bitwise XOR, not a power | fix to `x = x ^ (2)` |
| `c += 1` on a `<const>` or `<close>` local | ignored or corrupted by the runtime | error |
| `local f <close> in t` | `<close>` is ignored | warning |
| `!=`, `!x`, `&&`, `||`, `//=`, `%=`, `..=` | do not exist | error, fix to the valid form |

See `cfxdoc:cfxlua-syntax`.

## Readable magic values

Numbers passed to natives are named in hints on the line: `IsControlJustPressed(0, 38)` gets
`Control 38: INPUT_PICKUP (E / LB)`, and hashes, blip sprites and colours, marker types, HUD colours, explosion types
and animation flags are decoded the same way. Where a native expects one of these values, completion lists them by
name. Quick actions add the name as a comment, or replace a hash number with its `` `name` `` literal. Each family can
be turned off in the settings.

## Coordinates

On coordinates at the caret, quick actions convert between `vector3(…)`, keyed tables, arrays and spread arguments,
round them, and add or remove a heading. **CfxLua: Convert Coordinates** converts the selected text (pasted from a
dev tool, a JSON dump or a chat message) in place. Checks report coordinates at z = 0, outside the map, and duplicates
in config tables; with a zones file set in the settings, coordinates get the name of their zone as a hint.

## Performance

Loops that run every frame (`while true do … Wait(0) end`) get a gutter icon with their cost, and checks suggest an
adaptive sleep, a longer wait, caching calls in a local, `PlayerPedId()` instead of `GetPlayerPed(-1)`, vector math
instead of `GetDistanceBetweenCoords`, and removing prints from per-frame code — each with a quick fix. See
`cfxdoc:performance`.

## Asset Escrow

In resources that declare `escrow_ignore`, each Lua file shows on its first line whether it will be encrypted or stay
open; checks report configuration and translation files, and files loaded at runtime, that buyers could not edit.
**CfxLua: Check Resource Before Escrow Upload** runs the pre-publish checklist and opens its full report in the
**Escrow** tab of the tool window, which lists every resource. Encrypted `FXAP` files are never parsed.
See `cfxdoc:escrow`.

## Working with lua-language-server

OpenIDEA runs lua-language-server for Lua itself: the types of your own code and its `---@param` / `---@class`
annotations, hover, signature help, references. On its own it does not know CfxLua syntax or the natives, and reports
`+=`, backtick hashes and every native as errors. Two steps teach it:

1. **The configuration.** At the root of the project, **New › lua-language-server config (.luarc.json)**, named
   `.luarc`. It enables the CfxLua syntax (`+=`, `` `hash` ``, `/* */`), loads the `.cfxlua` folder as a library
   without checking it as project code, leaves `fxmanifest.lua` files to this plugin and lets completion list every
   matching native.
2. **The definitions.** Create an empty `.cfxlua/natives.lua`, keep it focused and run **CfxLua: Generate
   lua-language-server Definitions**, then save. The file receives every native of your games and of the server,
   with their documentation, parameters, returns and per-game signatures, and the Cfx runtime API (`Citizen`, events,
   exports, state bags, `json`, `msgpack`, `promise`, vectors).

lua-language-server then checks calls to natives: a missing argument, a wrong type or one argument too many is
reported, and hover shows the native's documentation. Trailing flags may be left out, as the runtime allows. Run the
command again after **CfxLua: Update Natives** or a change of game; the folder can stay out of version control.

Once lua-language-server loads the file, it completes those natives and the runtime API itself (one entry each),
Ctrl+click goes to their declaration and they are colored as library functions. This plugin then stops listing them,
so that nothing shows twice, but still offers the natives of a game the file lacks and still reports a native called
from the wrong side.

## Running FXServer

In a `server-data` folder (a `server.cfg` at the root of the project), the run configurations offer **FXServer
(server.cfg)**, which starts `FXServer +exec server.cfg` there with its console in the Run tool window. Grant the
optional `process:FXServer` permission in **Settings › Plugins** once `FXServer` is on your `PATH`: on Windows the
folder of `FXServer.exe`, on Linux an executable `FXServer` script that runs the artifacts' `run.sh`. After an artifacts
update, approve the program again.

## Troubleshooting

| Symptom | What to do |
|---|---|
| Nothing happens in Lua files | Check that *Enable FiveM / RedM support* is on and that the plugin is listed as active in **Settings › Plugins**. |
| A file has the wrong side | Hover the status bar item: it says why. List the file in the manifest, or follow a naming convention. |
| RedM natives in a FiveM project | Set *Game* to *FiveM*, or add `game 'gta5'` to the resource's manifest. |
| A new native is missing | Run **CfxLua: Update Natives**. |
| Files created outside the IDE are not seen | Save them once in the IDE, or reload the plugin in **Settings › Plugins**. |
