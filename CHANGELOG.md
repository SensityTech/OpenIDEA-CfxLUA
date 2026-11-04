# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/) (0.x while OpenIDEA's plugin API is a preview).

## [Unreleased]

### Added

- **A signed marketplace.** Every release is also published, signed by the `sensity` publisher, with the `index.json`
  OpenIDEA reads in **Settings › Plugins › Available**: installs from there are checked against the publisher's key,
  and new versions are offered as updates. The package file of a release stays unsigned. The marketplace copy is
  signed on Windows: on Linux and macOS OpenIDEA refuses it until
  [l0ng-ai/openidea#1258](https://github.com/l0ng-ai/openidea/issues/1258) is fixed.
- A daily workflow tests every new OpenIDEA release with OpenIDEA's own `openidea-plugin check`, and updates the
  documented requirement once one can install the plugin.

### Fixed

- **The OpenIDEA requirement.** The plugin needs plugin API 1.0, which no OpenIDEA release has yet (0.1.35 ships
  plugin API 0.1): OpenIDEA must be built from source at `4c5b6de` or later, not "0.1.35 or newer"
  ([#1](https://github.com/SensityTech/OpenIDEA-CfxLUA/issues/1)).

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

[Unreleased]: https://github.com/SensityTech/OpenIDEA-CfxLUA/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.1
[0.1.0]: https://github.com/SensityTech/OpenIDEA-CfxLUA/releases/tag/v0.1.0
