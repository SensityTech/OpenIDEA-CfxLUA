# The resource manifest (fxmanifest.lua)

Every resource is a folder with a `fxmanifest.lua` file at its root. The manifest declares the resource's metadata: target game, which scripts run on the client and the server, which files clients download, dependencies, and more. The older name `__resource.lua` (with `resource_manifest_version`) is deprecated.

## How the manifest is read

The manifest is Lua, but it runs in its own, separate environment before the resource starts. Each unknown global name is a **directive**: calling it records a metadata entry.

```lua
author 'Jane Doe'                 -- one entry: author = Jane Doe
client_script 'client.lua'        -- one entry: client_script = client.lua
client_scripts {                  -- table form: one entry per element,
    'client/main.lua',            -- the trailing "s" is removed from the key
    'client/ui.lua',
}
```

- A string argument adds one entry.
- A table argument adds one entry per element. If the directive name ends with `s`, that `s` is dropped: `client_scripts { ... }` adds `client_script` entries, `files { ... }` adds `file` entries.
- Any name works: custom keys (`my_config 'value'`) are allowed and readable from scripts with `GetNumResourceMetadata` / `GetResourceMetadata`.
- A second call with a table adds extra data: `my_data 'one' { two = 42 }` stores `my_data = one` and `my_data_extra = {"two":42}`. This is how `data_file 'TYPE' 'path'` works.

```lua
-- reading metadata from a script
local resource = GetCurrentResourceName()
local version = GetResourceMetadata(resource, 'version', 0)
local count = GetNumResourceMetadata(resource, 'client_script')
for i = 0, count - 1 do
    print(GetResourceMetadata(resource, 'client_script', i))
end
```

## Minimal manifests

FiveM:

```lua
fx_version 'cerulean'
game 'gta5'

author 'Jane Doe'
description 'Example resource'
version '1.0.0'

shared_script 'config.lua'
client_script 'client/*.lua'
server_script 'server/*.lua'
```

RedM:

```lua
fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'

client_script 'client.lua'
server_script 'server.lua'
```

Both games:

```lua
fx_version 'cerulean'
games { 'gta5', 'rdr3' }
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'

shared_script 'shared.lua'
client_script 'client.lua'
server_script 'server.lua'
```

## Core directives

### fx_version

Required. The manifest format version, one of:

| Value | Introduced | Notable behaviour |
|---|---|---|
| `adamant` | 2019-12 | Requires a `game` entry. Minimum for RedM. |
| `bodacious` | 2020-02 | Implies `clr_disable_task_scheduler`; JS contexts do not define `window`. |
| `cerulean` | 2020-05 | NUI pages load in a secure context: NUI callbacks use `https://`. **Current version, use it.** |

Each version includes the behaviour of the previous ones.

### game / games

Required with every `fx_version`. Which game(s) the resource supports:

| Value | Meaning |
|---|---|
| `gta5` | FiveM |
| `rdr3` | RedM (also requires `rdr3_warning`) |
| `common` | Any game, but only Cfx APIs, no game natives |

```lua
game 'gta5'
games { 'gta5', 'rdr3' }
```

### rdr3_warning

Required when the resource runs on RedM. The value must be exactly this sentence, otherwise the server refuses to start the resource:

```lua
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'
```

### Metadata: author, description, version

Free text, shown in tools and readable with `GetResourceMetadata`.

## Scripts and files

### client_script(s), server_script(s), shared_script(s)

| Directive | Runs on |
|---|---|
| `client_script` / `client_scripts` | each client |
| `server_script` / `server_scripts` | the server |
| `shared_script` / `shared_scripts` | both |

Scripts run in the order they are listed, shared scripts first on each side. The extension selects the runtime: `.lua` for Lua, `.js` for JavaScript, `.net.dll` for C#.

These directives support **globbing**:

| Pattern | Matches |
|---|---|
| `*.lua` | Lua files in the resource root, not in subfolders |
| `client/*.lua` | Lua files directly in `client/` |
| `**/*.lua` or `**.lua` | Lua files in every folder, recursively |
| `**/cl_*.lua` | Every file starting with `cl_`, recursively |

A file from another resource can be included with `@resourceName/path`:

```lua
shared_script '@my_library/init.lua'
```

OpenIDEA-CfxLUA uses these entries to decide whether a file is client, server or shared code (see `cfxdoc:plugin-guide`).

### file(s)

Files sent to clients without being executed: NUI pages and assets, data files, JSON configuration read with `LoadResourceFile`. Client scripts are added automatically, everything else that the client needs must be listed.

```lua
files {
    'html/index.html',
    'html/*.js',
    'data/items.json',
}
```

### ui_page, loadscreen

```lua
ui_page 'html/index.html'               -- NUI page of this resource (or an https:// URL)
loadscreen 'loading/index.html'         -- use this page as the loading screen
loadscreen_manual_shutdown 'yes'        -- keep it until ShutdownLoadingScreenNui() is called
loadscreen_cursor 'yes'                 -- show the mouse cursor on the loading screen
ui_page_preload 'yes'                   -- create the NUI frame as soon as the resource starts
nui_callback_strict_mode 'true'         -- only this resource's page may call its NUI callbacks
```

The page and its assets must also be listed in `files`. See `cfxdoc:nui`.

### export, server_export

Export **global** functions by name (client and server respectively). Calling `exports('name', fn)` in a script is the modern alternative. See `cfxdoc:exports`.

```lua
export 'getWidget'
server_exports { 'getStock', 'setStock' }
```

## Dependencies

### dependency / dependencies

Resources that must be started before this one:

```lua
dependency 'my_library'

dependencies {
    'my_library',
    'my_database',
}
```

Entries starting with `/` are runtime constraints instead of resource names:

| Constraint | Requires |
|---|---|
| `'/server:4500'` | server build 4500 or newer |
| `'/policy:subdir_file_mapping'` | the server key to be granted this policy |
| `'/onesync'` | OneSync to be enabled |
| `'/gameBuild:h4'` | the server's enforced game build to be at least this one (same values as `sv_enforceGameBuild`) |
| `'/native:0xE27C97A0'` | the server to support this native |

### provide

Declares that this resource replaces another one: it starts when the other is required, and acts as it.

```lua
provide 'mysql-async'
```

## Game data and maps (FiveM)

### data_file

Registers a game data file with the extra content system. Syntax: `data_file 'TYPE' 'path'`. The path supports globbing, and the file must also be listed in `files`.

```lua
files {
    'data/vehicles.meta',
    'data/carvariations.meta',
    'data/handling/*.meta',
}

data_file 'VEHICLE_METADATA_FILE' 'data/vehicles.meta'
data_file 'VEHICLE_VARIATION_FILE' 'data/carvariations.meta'
data_file 'HANDLING_FILE' 'data/handling/*.meta'
```

Frequent types: `VEHICLE_METADATA_FILE`, `VEHICLE_VARIATION_FILE`, `CARCOLS_FILE`, `HANDLING_FILE`, `VEHICLE_LAYOUTS_FILE`, `WEAPONINFO_FILE`, `WEAPON_METADATA_FILE`, `WEAPON_ANIMATIONS_FILE`, `WEAPONCOMPONENTSINFO_FILE`, `DLC_ITYP_REQUEST`, `AUDIO_WAVEPACK`, `AUDIO_GAMEDATA`, `AUDIO_SOUNDDATA`, `PED_METADATA_FILE`, `SHOP_PED_APPAREL_META_FILE`, `TIMECYCLEMOD_FILE`, `SCALEFORM_DLC_FILE`. The full list is in the data files reference linked below.

### this_is_a_map

Marks the resource as a map and reloads map storage when it loads. Any value works.

```lua
this_is_a_map 'yes'
```

### replace_level_meta, before_level_meta, after_level_meta

`replace_level_meta 'name'` replaces the level meta with `name.meta` from the resource (list the file in `files`). `before_level_meta` and `after_level_meta` load an extra level meta before or after the main one; they are **deprecated**, use data files instead.

## Server and runtime options

| Directive | Value | Effect |
|---|---|---|
| `server_only` | any | Clients download nothing from this resource. |
| `lua54` | `'yes'` | **Deprecated, no effect.** All Lua runs on 5.4 since June 2025. Safe to remove. |
| `use_experimental_fxv2_oal` | `'yes'` | Experimental faster native calls for Lua with corrected return types (BOOL out-values become booleans). **Disables vector unrolling** in native calls, and can break natives with wrong documented types. |
| `node_version` | `'16'` (default) or `'22'` | Node.js version for server-side JavaScript. |
| `clr_disable_task_scheduler` | `'yes'` | Disables the custom C# task scheduler on the server (implied by `bodacious` and later). |
| `escrow_ignore` | list of paths/globs | Files that Asset Escrow must leave unencrypted (configuration files, for example). |
| `convar_category` | see below | Shows convars of this resource on FxDK's Project Settings page. |

```lua
escrow_ignore {
    'config.lua',
    'locales/en.lua',
}
```

```lua
convar_category 'MyResource' {
    'MyResource settings',
    {
        { 'Enable debug logs', 'myres_debug', 'CV_BOOL', false },
        { 'Max players per job', 'myres_max', 'CV_INT', 10, 1, 64 },
        { 'Mode', 'myres_mode', 'CV_MULTI', {
            { 'Easy', 'easy' },
            { 'Hard', 'hard' },
        } },
    }
}
```

Convar entry types: `CV_STRING`, `CV_BOOL`, `CV_INT`, `CV_SLIDER`, `CV_COMBI`, `CV_PASSWORD`, `CV_MULTI`. Prefix the convar name with `$` for replicated convars (`setr`) and `#` for server-info convars (`sets`).

## Deprecated: resource_manifest_version

Before `fx_version`, manifests were named `__resource.lua` and declared a GUID:

```lua
resource_manifest_version '44febabe-d386-4d18-afbe-5e627f4af937'
```

Use `fxmanifest.lua` with `fx_version 'cerulean'` instead.

## A complete example

```lua
fx_version 'cerulean'
game 'gta5'

author 'Jane Doe'
description 'Player garages with NUI'
version '2.1.0'

dependencies {
    '/onesync',
    'my_library',
}

shared_scripts {
    '@my_library/init.lua',
    'config.lua',
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
    'web/assets/*.js',
    'web/assets/*.css',
}

escrow_ignore {
    'config.lua',
}
```

The resource's name is the name of its folder; `GetCurrentResourceName()` returns it.

## Sources

- Resource manifest reference: https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
- Data files reference: https://docs.fivem.net/docs/game-references/data-files/
- Manifest loader (`resource_init.lua`): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/resource_init.lua
- RedM manifest check (`ServerResources.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-server-impl/src/ServerResources.cpp
- Loading screens: https://docs.fivem.net/docs/scripting-manual/nui-development/loading-screens/
- Removal of Lua 5.3 support: https://forum.cfx.re/t/removal-of-lua-5-3-support/5335232
