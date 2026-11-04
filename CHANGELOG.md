# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/) (0.x while OpenIDEA's plugin API is a preview).

## [Unreleased]

### Fixed

- **No natives in completion on big projects.** The catalogs were loaded once the manifests were read, which a big
  project now does over several calls, and every natives update check dropped them. They are loaded at activation and
  kept until an update changes them.
- **The remaining "exceeded its CPU budget" crashes** on big projects: opening a `fxmanifest.lua`, the **Escrow** tab
  and its check, **CfxLua: Update Natives** when the database changed, and edits that rebuilt project-wide indexes.
  Calls made while you type never load a catalog or rebuild an index, natives updates are spread over several calls
  and convert only the project's games, and escrow reports are built from facts gathered while indexing. On a
  5,000-file project with the tool window open, no call uses more than 36% of the budget.
- **lua-language-server stalled** (one core at 100%, empty completion) while the generated definitions file was open:
  about 40,000 of its lines ended with a space, and lua-language-server's trailing-space check is very slow on a file
  that size, so its semantic colors never came either. Run **CfxLua: Generate lua-language-server Definitions** again
  to rewrite the file: natives and the Cfx runtime API then take the library color, apart from your own functions,
  parameters and fields.
- Ctrl+click on a native said "Cannot find declaration to go to": native declarations now come from
  lua-language-server and the generated definitions, without reading that file on every hover.
- **Natives listed up to four times in completion, and "Choose Declaration (3 found)" on Ctrl+click,** once the
  generated definitions were loaded: this plugin listed the native, and lua-language-server once per game signature.
  A native of several games now has one signature in the definitions (parameters some games lack are optional), and
  this plugin leaves the natives and runtime API lua-language-server loads to it. The file's header says which games
  and runtime parts it holds; this plugin still completes the natives of a game it lacks.
- **Natives listed twice again after a restart.** OpenIDEA does not list hidden folders such as `.cfxlua/`, so the
  plugin only knew the definitions once their file was saved. It now reads them by name when it starts (the files the
  command wrote, and `natives.lua` in each library folder) and after a repository change, and reads `.luarc.json`
  again when you switch editor, so an edit made outside the IDE is followed.
- A native of FiveM and RedM could get two parameters of the same name in the definitions (four of them, when both
  games are generated together): lua-language-server then reported valid calls as type errors.
- Server natives whose client name is an alias (`GetVehicleDashboardColour`…) were marked deprecated.
- The status bar shows the side of the first file opened.
- **CfxLua: Convert Coordinates** stays enabled when run from Find Action.

### Changed

- The **lua-language-server config (.luarc.json)** template ignores `.cfxlua` as project code (it stays a library)
  and lets completion list up to 1,000 items: lua-language-server stops at 100, which could hide the native you type.
  In an existing `.luarc.json`, add `".cfxlua"` to `workspace.ignoreDir` and `"completion.maxSuggestCount": 1000`.
- **CfxLua: Generate lua-language-server Definitions** warns when no `.luarc.json` at the root of the project loads
  the file's folder: lua-language-server would then see the natives only while the file is open. The `.luarc.json`
  is read in every form lua-language-server accepts: `Lua.`-prefixed or nested keys, comments and trailing commas,
  `.luarc.jsonc`, `${workspaceFolder}`, a nested folder or the file itself.
- The **Escrow** tab checks the resource you pick, not the first one listed. While the project is still being read,
  a report says which files are not checked yet.
- A loop that runs every frame gets the regular gutter icon: the green Run triangle read as a button that runs it.
- Project events in completion have the event icon, as built-in events do.
- **CfxLua: Show Documentation** separates the signature from the description with a dash: the notification shows
  line breaks as spaces.
- The tool window tabs go onto a second line when the window is narrow, instead of being cut off; the Asset Escrow
  badge is blue instead of red, which read as an error.

### Added

- Messages chosen by `and` / `or` (`action = bOpen and 'open' or 'close'`) link Lua and the NUI page both ways.
- Side icons for `client_*.lua`, `*_client.lua`, `*_cl.lua` and their server and shared equivalents.

## [0.1.3] - 2026-11-04

### Fixed

- **Big projects crashed the plugin at start** ("activation exceeded its CPU budget"): it read and indexed the
  whole project in one call, past the CPU budget OpenIDEA gives each call (5·10⁹ wasm instructions). The scan now runs
  in slices counted in files read and bytes indexed, over the calls the IDE makes (opening and focusing files, the
  tool window, commands), the resource you open first; derived indexes catch up between slices. On a 5,000-file
  project, no call uses more than half of the budget, and completion stays under 10 ms while indexing.
- Files over 1 MB (generated data, bundles) are no longer indexed.

## [0.1.2] - 2026-11-04

### Changed

- **Built on the final shape of plugin API 1.0.** OpenIDEA changed plugin API 1.0 types before releasing it
  ([l0ng-ai/openidea#1245](https://github.com/l0ng-ai/openidea/pull/1245)): 0.1.1 installs in OpenIDEA builds from
  that change on but does not start ("expected `string` found `enum`"). 0.1.2 is built against OpenIDEA
  [`859f634`](https://github.com/l0ng-ai/openidea/commit/859f63454eed53ae97eb5881d5f076504848a6aa), and needs an
  OpenIDEA built at that revision until a release ships plugin API 1.0.
- The manifest declares its marketplace categories: frameworks, linters, tools.

### Added

- Deprecated manifest directives (`lua54`, `resource_manifest_version` in an fxmanifest, `before_level_meta`,
  `after_level_meta`) are struck through.
- **A signed marketplace.** Every release is also published, signed by the `sensity` publisher, with the `index.json`
  OpenIDEA reads from **Settings › Plugins** › gear menu › **Plugin Repository…**: installs from there are checked
  against the publisher's key, and new versions are offered as updates. The package file of a release stays unsigned. The marketplace copy is
  signed on Windows: on Linux and macOS OpenIDEA refuses it until
  [l0ng-ai/openidea#1258](https://github.com/l0ng-ai/openidea/issues/1258) is fixed.
- A daily workflow installs the latest release of the plugin with every new OpenIDEA release (its own
  `openidea-plugin check`, then a command run by its plugin host), updates the documented requirement once one runs
  it, and opens an issue when one does not.

### Fixed

- **The OpenIDEA requirement.** The plugin needs plugin API 1.0, which no OpenIDEA release has yet (0.1.35 ships
  plugin API 0.1), not "OpenIDEA 0.1.35 or newer" ([#1](https://github.com/SensityTech/OpenIDEA-CfxLUA/issues/1)).
- `change-notes` of the manifest still described 0.1.0.

## [0.1.1] - 2026-11-04

### Added

- **CfxLua: Generate lua-language-server Definitions**: fills an empty `.lua` file with a `---@meta` library of the
  natives of the project's games and the server (documentation, parameters, returns, per-game overloads) and of the
  Cfx runtime API, so that lua-language-server checks native calls and shows their documentation.
- A **lua-language-server config (.luarc.json)** New file template: CfxLua syntax, the `.cfxlua` library, manifests
  left to the plugin.
- An **Escrow** tab in the tool window: every resource of the project, those using Asset Escrow first, with the full
  pre-publish report of the selected one and a **Copy Report** button. **CfxLua: Check Resource Before Escrow Upload**
  opens it on the checked resource.
- An **Update Natives Now** button in the Natives tab.
- A weekly workflow refreshes the bundled native databases (`scripts/update-natives.sh`).
- An **FXServer (server.cfg)** run configuration in `server-data` projects (optional `process:FXServer` permission).

### Changed

- lua-language-server is now listed as a requirement next to OpenIDEA.
- Typing in a project with many events no longer rebuilds the whole event database: only the edited file is resolved
  again, unless it changes an event helper.

### Fixed

- The `string.blob_pack` overload of the runtime API used an invalid vararg annotation.
- Safe navigation with spaces (`x ? . y`) and safe calls with a long string (`f?[[text]]`) are parsed as CfxLua does.
- Go to Definition on `AddEventHandler('__cfx_nui:…')` only lists the page's NUI callbacks, not event sites.
- The escrow report says "0 files", not "0 file".

## [0.1.0] - 2026-11-04

First preview release.

### Added

- **Client / server awareness**: every Lua script is classified as client, server or shared from its resource's
  manifest (globs included) or the naming conventions; shown in the status bar, with file icons for the conventions.
- **Natives**: the Cfx.re databases for FiveM (GTA V + CFX), RedM (RDR3 + CFX) and FXServer, filtered by game and
  side, with completion, full documentation, out-parameters returned as Lua does, a wrong-side check, and updates
  (manual and daily) stored as compact deltas.
- **Cfx runtime API**: completion and documentation of `Citizen`, events, exports, state bags, `json`, `msgpack`,
  `promise`, vectors and the CfxLua standard library extensions.
- **State bags**: keys indexed across the project, completion with types and write sites, navigation, unknown-key
  check with suggestions.
- **Events**: handlers and triggers matched across the project (helpers and `---@cfx.event` included), built-in
  events, completion, navigation, gutter icons and six checks.
- **fxmanifest**: completion of directives, values, paths and dependencies, navigation, six checks with quick fixes,
  snippets and New file templates.
- **NUI bridge** between Lua client scripts and pages in JavaScript, TypeScript, JSX/TSX, Vue, Svelte and HTML, with
  completion, navigation, gutter icons, three checks and a dev-server switch.
- **CfxLua syntax checks**: `i--`, `++`, `^=`, compound assignment to `<const>`, `<close>` in `in` unpacking, and
  syntax from other languages (`!=`, `!x`, `&&`, `||`, `//=`, `%=`, `..=`), with fixes.
- **Readable magic values** (controls, hashes, blips, markers, HUD colours, explosions, animation flags),
  **coordinate tools** and a **performance linter** with loop-cost gutter markers.
- **Asset Escrow**: per-file encryption status, checks and a pre-upload checklist.
- **The CfxLua tool window**: natives browser, runtime API, events, manifest directives, syntax features and guides.

[Unreleased]: https://github.com/SensityTech/OpenIDEA-CfxLUA/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.3
[0.1.2]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.2
[0.1.1]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.1
[0.1.0]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.0
