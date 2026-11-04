-- Copyright (c) 2026. SensityTech
--
-- Licensed under the Apache License, Version 2.0 (the "License"); you may not
-- use this file except in compliance with the License. You may obtain a copy of
-- the License at
--
-- http://www.apache.org/licenses/LICENSE-2.0
--
-- Unless required by applicable law or agreed to in writing, software
-- distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
-- WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
-- License for the specific language governing permissions and limitations under
-- the License.

-- Cfx.re resource manifest (`fxmanifest.lua`, legacy `__resource.lua`)
-- directives. These names only exist in the separate runtime that evaluates the
-- manifest, so this file must only apply to manifest files.
--
-- In a manifest every unknown global is a directive: `key 'value'` adds the
-- metadata entry `key`; `key { 'a', 'b' }` adds one entry per element and
-- strips a trailing `s` from the key (`client_scripts { ... }` adds
-- `client_script` entries), so plural forms only take tables. Every directive
-- call returns a function that stores an extra value as `<key>_extra`, which is
-- how `data_file 'TYPE' 'path'` works. The `exports { ... }` table form of
-- `export` is not declared here because `exports` is the runtime exports
-- object in scripts.

---
--- Sets the FXv2 manifest version of the resource. Required in
--- `fxmanifest.lua`; `cerulean` is the current version.
---
--- * `adamant` (2019-12): requires `game`, mandatory for RedM.
--- * `bodacious` (2020-02): implies `clr_disable_task_scheduler`.
--- * `cerulean` (2020-05): NUI pages load in a secure context (`https://`
---   callbacks).
---
--- ```lua
--- fx_version 'cerulean'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/resource-manifest/)
---@param version string|'"cerulean"'|'"bodacious"'|'"adamant"' @The manifest version.
function fx_version(version) end

---
--- Sets the game the resource runs on: `gta5` (FiveM), `rdr3` (RedM),
--- `common` (any game, CitizenFX APIs only) or `gta4` (LibertyM).
---
--- ```lua
--- game 'gta5'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/resource-manifest/)
---@param name string|string[]|'"gta5"'|'"rdr3"'|'"common"'|'"gta4"' @The game name, or a list of game names.
function game(name) end

---
--- Sets the games the resource runs on (table form of `game`).
---
--- ```lua
--- games { 'gta5', 'rdr3' }
--- ```
---@param names string[] @The game names: `gta5`, `rdr3`, `common` or `gta4`.
function games(names) end

---
--- Acknowledgement required by resources that target RedM (`game 'rdr3'`).
--- The value must be exactly:
--- `I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.`
---@param acknowledgement string|'"I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships."' @The acknowledgement text.
function rdr3_warning(acknowledgement) end

---
--- Sets the legacy (FXv1) manifest version GUID, used in `__resource.lua`.
---@deprecated Use fxmanifest.lua with fx_version instead.
---@param guid string|'"44febabe-d386-4d18-afbe-5e627f4af937"'|'"05cfa83c-a124-4cfa-a768-c24a5811d8f9"'|'"f15e72ec-3972-4fe4-9c7d-afc5394ae207"'|'"77731fab-63ca-442c-a67b-abc70f28dfa5"' @The manifest version GUID.
function resource_manifest_version(guid) end

---
--- Free-form metadata: the resource author.
---@param name string @The author.
function author(name) end

---
--- Free-form metadata: the resource description.
---@param text string @The description.
function description(text) end

---
--- Free-form metadata: the resource version.
---@param text string @The version.
function version(text) end

---
--- Free-form metadata: the resource repository URL.
---@param url string @The repository URL.
function repository(url) end

---
--- Adds a script loaded on the client and included in the resource package.
--- Supports globbing (`'client/*.lua'`, `'**/cl_*.lua'`) and files of other
--- resources (`'@resource/file.lua'`). The extension selects the runtime:
--- `.lua`, `.js` or `.net.dll`.
---
--- ```lua
--- client_script 'client.lua'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/resource-manifest/)
---@param path string|string[] @The script path (or a list of paths).
function client_script(path) end

---
--- Adds scripts loaded on the client (table form of `client_script`).
---
--- ```lua
--- client_scripts {
---     'config.lua',
---     'client/*.lua',
--- }
--- ```
---@param paths string[] @The script paths; globbing is supported.
function client_scripts(paths) end

---
--- Adds a script loaded on the server. Supports globbing and files of other
--- resources (`'@oxmysql/lib/MySQL.lua'`).
---
--- ```lua
--- server_script 'server.lua'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/resource-manifest/)
---@param path string|string[] @The script path (or a list of paths).
function server_script(path) end

---
--- Adds scripts loaded on the server (table form of `server_script`).
---@param paths string[] @The script paths; globbing is supported.
function server_scripts(paths) end

---
--- Adds a script loaded on both the client and the server. Supports globbing.
---
--- ```lua
--- shared_script 'config.lua'
--- ```
---@param path string|string[] @The script path (or a list of paths).
function shared_script(path) end

---
--- Adds scripts loaded on both sides (table form of `shared_script`).
---@param paths string[] @The script paths; globbing is supported.
function shared_scripts(paths) end

---
--- Adds a file to the resource package downloaded by clients (NUI files, data
--- files, ...). Supports globbing.
---
--- ```lua
--- file 'html/index.html'
--- ```
---@param path string|string[] @The file path (or a list of paths).
function file(path) end

---
--- Adds files to the resource package (table form of `file`).
---
--- ```lua
--- files {
---     'html/index.html',
---     'html/**/*',
--- }
--- ```
---@param paths string[] @The file paths; globbing is supported.
function files(paths) end

---
--- Groups files into a separate client package named `<name>.rpf`. Call the
--- returned function with the list of file patterns (globbing is supported).
---
--- ```lua
--- file_set 'maps' {
---     'stream/maps/**',
--- }
--- ```
---@param name string @The package name, without the `.rpf` extension.
---@return fun(patterns:string[]) @Function taking the file patterns.
function file_set(name) end

---
--- Exports a global function of the client scripts, callable from other
--- resources with `exports.<resource>:<name>(...)`. Prefer calling
--- `exports('name', fn)` in the script.
---@param name string|string[] @The name of the global function (or a list of names).
function export(name) end

---
--- Exports a global function of the server scripts, callable from other
--- server resources with `exports.<resource>:<name>(...)`.
---@param name string|string[] @The name of the global function (or a list of names).
function server_export(name) end

---
--- Exports global functions of the server scripts (table form of
--- `server_export`).
---@param names string[] @The names of the global functions.
function server_exports(names) end

---
--- Sets the NUI page of the resource: a file of the resource (also listed
--- with `file`) or an absolute URL.
---
--- ```lua
--- ui_page 'html/index.html'
--- ```
---@param path string @The page path or URL.
function ui_page(path) end

---
--- When present, the NUI page is loaded as soon as the resource starts.
---@param value string|'"yes"' @Any value.
function ui_page_preload(value) end

---
--- Makes NUI callbacks of this resource only accept requests coming from its
--- own UI page.
---@param value string|'"true"' @`'true'` to enable the strict mode.
function nui_callback_strict_mode(value) end

---
--- Sets the HTML file shown as the game loading screen (also list it with
--- `file`).
---@param path string @The loading screen page.
function loadscreen(path) end

---
--- Keeps the loading screen open after the game has loaded, until
--- `ShutdownLoadingScreenNui` is called.
---@param value string|'"yes"' @`'yes'` to enable.
function loadscreen_manual_shutdown(value) end

---
--- Shows the mouse cursor on the loading screen.
---@param value string|'"yes"' @`'yes'` to enable.
function loadscreen_cursor(value) end

---
--- Adds a data file of the given type to the game content system. Call the
--- returned function with the file path (globbing is supported); the file must
--- also be listed with `file`.
---
--- ```lua
--- data_file 'HANDLING_FILE' 'data/handling.meta'
--- data_file 'VEHICLE_METADATA_FILE' 'data/vehicles.meta'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/game-references/data-files/)
---@param fileType string @The data file type, for example `'VEHICLE_METADATA_FILE'`.
---@return fun(path:string) @Function taking the data file path.
function data_file(fileType) end

---
--- Marks the resource as a map: the map storage is reloaded when it starts.
---@param value string|'"yes"' @Any value.
function this_is_a_map(value) end

---
--- Marks the resource as server-only: clients do not download it.
---@param value string|'"yes"' @Any value.
function server_only(value) end

---
--- Loads a level meta file of the resource before the primary level meta.
---@deprecated Use data files instead.
---@param path string @The level meta file.
function before_level_meta(path) end

---
--- Loads a level meta file of the resource after the primary level meta.
---@deprecated Use data files instead.
---@param path string @The level meta file.
function after_level_meta(path) end

---
--- Replaces the level meta (`common:/data/levels/gta5/gta5.meta`) with a file
--- of the resource, given without its `.meta` extension and listed with
--- `file`.
---@param path string @The level meta file, without extension.
function replace_level_meta(path) end

---
--- Adds a meta file of the resource to the meta files the game loads at
--- initialization (also list it with `file`). Low-level: prefer data files.
---@param path string @The meta file.
function init_meta(path) end

---
--- RedM only: replaces the default train tracks with a file of the resource
--- (also listed with `file`).
---@param path string @The train track file.
function replace_traintrack_file(path) end

---
--- Requires another resource to be started before this one, or a runtime
--- constraint: `'/server:<build>'`, `'/policy:<name>'`, `'/onesync'`,
--- `'/gameBuild:<build>'` or `'/native:<hash>'`.
---
--- ```lua
--- dependency 'oxmysql'
--- ```
---@param name string|string[] @The resource name or constraint (or a list).
function dependency(name) end

---
--- Requires resources or runtime constraints (table form of `dependency`).
---
--- ```lua
--- dependencies {
---     'oxmysql',
---     '/server:5848',
---     '/onesync',
--- }
--- ```
---@param names string[] @The resource names or constraints.
function dependencies(names) end

---
--- Declares the resource as a replacement for another one: it starts instead
--- of that resource when another resource depends on it.
---@param name string|string[] @The replaced resource name (or a list).
function provide(name) end

---
--- Enables Lua 5.4 for the resource.
---@deprecated Lua 5.4 is always used since June 2025; this directive has no effect.
---@param value string|'"yes"' @`'yes'`.
function lua54(value) end

---
--- Enables the experimental "one argument list" native calling convention for
--- Lua (corrected native return types, faster calls). Vector arguments are no
--- longer expanded: pass `coords.x, coords.y, coords.z` instead of `coords`.
---@param value string|'"yes"' @`'yes'` to enable.
function use_experimental_fxv2_oal(value) end

---
--- Selects the Node.js version of the server JavaScript runtime.
---@param value string|'"16"'|'"22"' @`'16'` (default) or `'22'`.
function node_version(value) end

---
--- Disables the custom C# task scheduler on the server (implied by
--- `fx_version 'bodacious'` and later).
---@param value string|'"yes"' @`'yes'` to disable the scheduler.
function clr_disable_task_scheduler(value) end

---
--- Lists files that Asset Escrow must not encrypt.
---
--- ```lua
--- escrow_ignore {
---     'config.lua',
---     'locales/*.lua',
--- }
--- ```
---@param paths string|string[] @The ignored files; globbing is supported.
function escrow_ignore(paths) end

---
--- Adds a group of convars to the FxDK "Project Settings" page. Call the
--- returned function with `{ title, { { label, convar, type, default, ... }, ... } }`
--- where `type` is `'CV_STRING'`, `'CV_BOOL'`, `'CV_INT'`, `'CV_SLIDER'`,
--- `'CV_COMBI'`, `'CV_PASSWORD'` or `'CV_MULTI'`.
---
--- ```lua
--- convar_category 'MyResource' {
---     'My resource settings',
---     {
---         { 'Debug mode', 'myresource_debug', 'CV_BOOL', 'false' },
---     },
--- }
--- ```
---@param name string @The category name.
---@return fun(definition:table) @Function taking the category definition.
function convar_category(name) end
