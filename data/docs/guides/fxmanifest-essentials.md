# fxmanifest essentials: what every resource must define

A resource is a folder with an `fxmanifest.lua` at its root. FXServer reads that file before anything else: it decides whether the resource may start at all, which files run on the client and on the server, in which order, and what clients download. This guide is the checklist of what every manifest must get right. The reference of every directive is in `cfxdoc:fxmanifest`.

## The required lines

Three directives decide whether FXServer starts the resource. When one is missing or wrong, the server prints a warning and does **not** start it.

| Directive | Required when | Without it |
|---|---|---|
| `fx_version 'cerulean'` | always | "does not specify an `fx_version`": the resource is not started |
| `game 'gta5'`, `game 'rdr3'` or `games { ... }` | always | "does not support the current game": the resource is not started |
| `rdr3_warning '...'` | the resource lists `rdr3` | "does not contain the RedM pre-release warning": not started on RedM |

```lua
fx_version 'cerulean'
game 'gta5'
```

- `fx_version` is one of `adamant`, `bodacious` or `cerulean`. Use `cerulean`, the current version: among other things it loads NUI pages in a secure context, so NUI callbacks use `https://`.
- `game` is `gta5` (FiveM), `rdr3` (RedM) or `common` (only Cfx APIs, no game natives). A resource for both games uses the table form `games { 'gta5', 'rdr3' }`. Mixing `common` with a specific game is refused as ill-formed.
- `rdr3_warning` must be **exactly** this sentence, character for character:

```lua
fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'
```

## Recommended metadata

`author`, `description` and `version` are free text. Nothing requires them, but tools display them and scripts can read them:

```lua
author 'Jane Doe <jane@example.com>'
description 'Player garages'
version '1.2.0'
```

```lua
-- in a script of the resource
local version = GetResourceMetadata(GetCurrentResourceName(), 'version', 0)
print(('garages %s loaded'):format(version))
```

Any other name is accepted as custom metadata (`repository 'https://...'`, `my_setting 'value'`) and is readable the same way.

## Scripts: which side, which order

| Directive | Runs on |
|---|---|
| `shared_script` / `shared_scripts` | the client **and** the server, before the other scripts |
| `client_script` / `client_scripts` | each client |
| `server_script` / `server_scripts` | the server |

On each side, shared scripts run first, then the side's scripts, in the order they are listed. Put what other files use at the top: configuration, locales, libraries.

```lua
shared_scripts {
    'config.lua',        -- defines Config, used by everything below
    'locales/*.lua',
}

client_scripts {
    'client/utils.lua',  -- helpers used by main.lua
    'client/main.lua',
}

server_scripts {
    'server/*.lua',
}
```

The extension selects the runtime: `.lua`, `.js` or `.net.dll`.

### Singular or table form, not both

`client_script 'main.lua'` adds one script and `client_scripts { 'a.lua', 'b.lua' }` adds several. The trailing `s` is only removed for a **table** argument: `client_scripts 'main.lua'` records an entry named `client_scripts`, which FXServer never reads, so the script silently does not load. The same applies to `files`, `games`, `dependencies` and the other table forms.

```lua
client_script 'client/main.lua'      -- one file
client_scripts { 'client/main.lua' } -- a table, even with one file
```

## Globs

Paths of scripts, `files` and `data_file` may be patterns, relative to the resource folder:

| Pattern | Matches |
|---|---|
| `client/*.lua` | Lua files directly in `client/`, not in its subfolders |
| `*.lua` | Lua files at the root of the resource |
| `client/**/*.lua` | Lua files in `client/` and all its subfolders |
| `**/cl_*.lua` | files starting with `cl_` anywhere in the resource |

When the load order between two files matters, list them explicitly instead of relying on a glob.

A script of another resource is included with `@resourceName/path`. Declare that resource as a dependency so it is started first:

```lua
dependency 'ox_lib'
shared_script '@ox_lib/init.lua'
```

## What clients download: NUI, data files, streaming

Client and shared scripts are sent to clients automatically. Every **other** file a client needs must be listed in `files`: NUI pages and their assets, JSON read on the client with `LoadResourceFile`, game data files. Files only the server reads need no entry.

```lua
ui_page 'web/index.html'

files {
    'web/index.html',
    'web/assets/*.js',
    'web/assets/*.css',
    'data/items.json',
}
```

The `ui_page` file is a file of the resource like any other, so it also appears in `files`. See `cfxdoc:nui` for the scripting side.

Game data files need both a `files` entry and a `data_file` line naming their type:

```lua
files {
    'data/vehicles.meta',
    'data/handling.meta',
}

data_file 'VEHICLE_METADATA_FILE' 'data/vehicles.meta'
data_file 'HANDLING_FILE' 'data/handling.meta'
```

Streamed assets (models, textures, maps) go in a folder named `stream` at the root of the resource: they are streamed to clients without any manifest entry.

## Dependencies and constraints

`dependency` / `dependencies` list resources that must be started before this one. Entries starting with `/` are runtime constraints the server must satisfy:

| Entry | Requires |
|---|---|
| `'/server:4500'` | server build 4500 or newer |
| `'/onesync'` | OneSync enabled |
| `'/gameBuild:h4'` | an enforced game build of at least this one (same values as `sv_enforceGameBuild`) |
| `'/policy:subdir_file_mapping'` | this policy granted to the server key |
| `'/native:0xE27C97A0'` | a server supporting this native |

```lua
dependencies {
    '/onesync',
    '/server:4500',
    'ox_lib',
    'oxmysql',
}
```

## Common mistakes and how the plugin catches them

Sensity DevKit checks manifests as you type (**Settings > Editor > Inspections > Lua > CfxLua > fxmanifest**). Most problems come with a quick fix (Alt+Enter).

| Mistake | What FXServer does | Plugin |
|---|---|---|
| No `fx_version`, or an unknown one | does not start the resource | error, adds `fx_version 'cerulean'` |
| No `game` / `games` | does not start the resource | error, adds `game 'gta5'`, `game 'rdr3'` or both |
| `common` with a specific game | refuses the ill-formed resource | error |
| RedM resource without the exact `rdr3_warning` | does not start it on RedM | error, adds or corrects the sentence |
| `client_scripts 'main.lua'` (table form, single string) | ignores the entry | warning, renames to `client_script` |
| Misspelled directive (`client_scirpts`) | keeps it as custom metadata, loads nothing | warning with "did you mean", renames it |
| Path or glob matching no file | prints "could not find" when starting | warning on the path |
| `lua54 'yes'` | nothing: Lua 5.4 is always used | marked deprecated, removes the line |
| Legacy `__resource.lua` | legacy format; ignored next to an `fxmanifest.lua` | warning, converts it to `fxmanifest.lua` |

While writing a manifest, completion (Ctrl+Space) offers the directive names with their value shape, the known values (`fx_version`, `game`, `data_file` types, `node_version`, ...), the files and folders of the resource with ready-made globs, `@resource/` paths of the other resources of the project, and dependency names and constraints. Ctrl+click on a path opens the file, on a glob lists every matched file, and on a dependency opens that resource's manifest.

To start a resource from scratch, use **New > CfxLua Resource** in the Project view: it asks for the name, the game and the parts (client, server, shared config, NUI page) and generates a manifest that passes every check. In a manifest, the live templates `fxm` (FiveM), `fxmr` (RedM) and `fxmb` (both) insert the required header, and `shs`, `cls`, `svs`, `fls`, `nui`, `deps`, `dataf` and `escrow` the common blocks.

## Templates

A FiveM resource with a shared configuration, client and server scripts and a NUI page:

```lua
fx_version 'cerulean'
game 'gta5'

author 'Jane Doe'
description 'Player garages'
version '1.0.0'

dependencies {
    'ox_lib',
}

shared_scripts {
    '@ox_lib/init.lua',
    'shared/config.lua',
}

client_scripts {
    'client/*.lua',
}

server_scripts {
    'server/*.lua',
}

ui_page 'web/index.html'

files {
    'web/index.html',
    'web/style.css',
    'web/script.js',
}
```

A RedM resource:

```lua
fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'

author 'Jane Doe'
description 'Stables'
version '1.0.0'

shared_script 'shared/config.lua'
client_script 'client/*.lua'
server_script 'server/*.lua'
```

A resource for both games lists them with `games { 'gta5', 'rdr3' }` and keeps the `rdr3_warning` line; see `cfxdoc:redm-vs-fivem` for the code differences.

## Sources

- Resource manifest reference: https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
- Manifest loader, table forms and the trailing `s` (`resource_init.lua`): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/resource_init.lua
- Game, `fx_version` and `rdr3_warning` checks (`ServerResources.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-server-impl/src/ServerResources.cpp
- Data files reference: https://docs.fivem.net/docs/game-references/data-files/
- Streaming assets from the `stream` folder: https://docs.fivem.net/docs/assets-manual/beginner-series/part-4/
