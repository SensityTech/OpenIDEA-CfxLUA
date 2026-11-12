# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/) (0.x while OpenIDEA's plugin API is a preview).

## [Unreleased]

### Added

- **Semantic colours without lua-language-server.** The plugin colours the names of a Lua file itself, the way the
  JetBrains plugin does: parameters, locals read from a nested function (underlined), globals, functions and methods
  (in italics when called through a dot), fields, `self`, hash literals, and natives, the Cfx runtime API and the Lua
  standard library as library names. Where lua-language-server colours the same name, the plugin's colour is shown.
- **Documentation comments are coloured**, the way the JetBrains plugin colours them: the tag names (`@param`,
  `@class`, `@return`…), the types, the names of classes, aliases, fields and parameters, `fun` and `vararg`. EmmyDoc
  and the LuaCATS annotations of lua-language-server are both read.
- **Syntax errors of CfxLua itself.** A missing `end`, an unexpected symbol, a missing expression, a statement that
  is only an expression or a method name without its arguments are reported by the plugin's own parser, which knows
  the CfxLua syntax (compound assignments, safe navigation, hash literals, set constructors, `in` unpacking, C
  comments).
- **Folding, structure and occurrences from the plugin**, for a project without lua-language-server: blocks, tables,
  long comments, long strings and `--region` fold; the structure view lists functions, methods, variables and table
  fields, each under what is written around it (a method declared after its table is listed as `Table:method()`);
  the caret on a name highlights its uses in the file (written or read), on `return` the exits of the function, on
  `break` the ends of its loop.
- **A slow editor call is named in the IDE log.** OpenIDEA restarts a plugin whose editor call runs for 500 ms and
  does not say which call it was: the plugin now writes `<call> took <n> ms on <size> KB` when one of its own takes
  more than 250 ms.
- **Completion of the members of a table, a class or an object, without lua-language-server.** After a `.` or a
  `:` the plugin lists what the expression holds, from its own type engine (a port of the JetBrains plugin's): the
  fields of a table and what is assigned to it in any file of the project, the fields and methods of an annotated
  class (`---@class`, `---@field`) and of the classes it inherits from, what a function returns (`---@return`, or
  read from its body), `self`, the values of a generic function, the file a `require` loads. A function shows its
  parameters and what it returns, a field its type. After a colon only what can take the object is listed; a
  `private` member is listed inside its class only.
- **Completion of names, without lua-language-server.** While a name is typed the plugin lists the locals,
  parameters and loop variables in sight with their type, the globals the project declares (by their start or by
  the starts of their words: `gsc` finds `GetShopCount`), the keywords, and in a method the methods of its object
  as `self:name`. `function` typed where a value is expected inserts `function() end`. Locals come first, then
  the globals of the project, the natives and the runtime API, the keywords, and last the words of the file. A
  name whose first letters read as a keyword is still completed: `in` lists `index`, `do` lists `door`.
- **Completion of what a place asks for.** Where a typed annotation expects it, the plugin offers the strings of
  a literal type (`'top'|'bottom'`, an alias, `---|` value lines), inside a string or with their quotes; the
  members of an expected `---@enum` (`Mode.PRESS`) and the keys of an `---@enum (key)`; and in a table, the
  fields of the class it is expected to be (`lib.notify({ ti` offers `title = `), without those already set.
- **Completion of the methods to define again.** After `function Shop:` the methods of the classes `Shop`
  inherits from, which it does not define yet, are offered with their parameters and their `end`.
- **Completion inside documentation comments**: the tags after `---@`, the parameters of the documented function
  after `@param`, the classes, aliases and built-in types where a type is written, `public` / `protected` and
  the fields of the class after `@field`, the members after `@see Class#`.
- **Completion of `require` paths and of string arguments.** Inside `require('...')` the Lua files of the
  resource are offered as the names they are required by (`modules.cart.items`). Inside any other string
  argument, the strings the project gives to a function of the same name at the same position are offered,
  marked "History".
- **Documentation on hover, without lua-language-server.** Hovering a name shows how it is declared and what its
  comment says, from the plugin's own type engine: a local with its type, a parameter with its `@param`, a
  function or a method with its parameters and what it returns (`@return`, or read from its body), a field with
  the class it belongs to, a class with its parents and its fields, an alias with its values. The text of the
  comment is shown as Markdown, then what its tags tell: parameters and their values, returned values, overloads,
  `@see`, `@deprecated`, `@since`. A class or an alias named in an annotation, and a parameter named by a
  `@param`, show the same. Where lua-language-server answers too, the plugin's text comes below its own.
- **Parameter info, without lua-language-server.** Inside the parentheses of a call (after `(` or `,`, or with
  Parameter Info) the parameters of the called function are shown, the one being typed in bold: each signature of
  the function, its `---@overload`s included, with the object first when a method is called with a dot.
- **Go to declaration from the type engine.** Go to Declaration on a name leads to the local, the parameter or the
  global it stands for, in any file of the project; on a field, to the member of its class (the `---@field`, the
  method or the assignment); on `self`, to what the method is declared on; on a class or an alias named in an
  annotation, to its `---@class` or `---@alias`; on the name after `@param`, to the parameter; on the label of a
  `goto`, to that label; on the string of a `require`, to the file it loads. Go to Type Declaration leads to the
  classes of the type of a name.
- **Inlay hints of parameter names.** Before a number, a string, a table, a function or an operation given to a
  function the plugin knows, the name of the parameter is written: `open(door: 2, fast: true)`. Three more hints
  can be turned on in the settings: the types of local variables, the types of parameters, and what functions
  return.
- **The Lua standard library and the Cfx runtime are known to the type engine.** `print`, `string.format`,
  `Citizen.Wait` or `vector3` have a hover (their documentation), parameter info and parameter hints, and what
  they return is typed. After `string.` or `('x'):` the string functions are listed, and the names of Lua itself
  (`print`, `pairs`, `math`…) are completed. A script only sees the runtime API of its side: a server script is
  not offered `SendNUIMessage`, a client script is not offered `GetPlayers`.
- **Lua inspections, without lua-language-server.** The plugin reports, the way the JetBrains plugin does: a
  local, a parameter, a loop variable or a local function that is never used (faded, with a fix that removes it
  or renames it to `_`); a local that hides another local of the same name; `local a = nil`; a `for`, a `while`
  or a `do` with an empty body; the statements written after a `return`; a table indexed at `0`. Each has a
  setting and a quick fix where one makes sense. A `---@suppress <id>` comment above a statement turns an
  inspection off in it (`---@suppress RemoveUnusedLocal`), for the plugin's CfxLua checks too, and a `---@meta`
  file is not told about its unused names.
- **Inspections that read the whole project.** Once a project has been read, the plugin also reports a
  `---@class` that is declared twice, with the file of the other declaration; the use of a function, a field or
  a class that a `---@deprecated` comment marks, struck through; a type written in a comment that no class, no
  alias and no generic parameter declares; and a `---@param` that names no parameter of the function it
  documents, with a fix that removes it. Each has a setting.
- **Type safety inspections, off by default.** Four inspections of the JetBrains plugin can be turned on in the
  settings: arguments that do not match the `---@param` of the function they are given to (type, missing or
  extra argument, a method its class does not declare), `return` values that do not match `---@return`, a value
  assigned to a variable or a field of another type, and a name that nothing declares. On code that is not
  annotated they report a lot, which is why they wait for a setting.
- **Lua intentions.** Where the caret stands, the plugin offers the rewrites of the JetBrains plugin: add or
  remove the parentheses of a call that takes a single string or table; invert a boolean; replace an operation
  between constants by its value (with the caret on its operator); add `---@param` for a parameter, `---@type`
  for a local, `---@return` or a whole documentation comment for a function, each written with the type the
  plugin infers; set the visibility of a method; keep a parameter of a method in a field (`self.name = name`).
  On a selection, the statements it covers can be surrounded by a `--region` or a `--{{{` pair.
- **Postfix templates.** After an expression and a dot, completion offers the templates of the JetBrains plugin:
  `count.if` becomes `if count then ... end`, and likewise `if_not`, `if_nil`, `if_not_nil`, `for`, `for_i`,
  `for_p`, `return`, `print`, `increase`, `decrease` and `var` for an expression alone on its line; `par`,
  `tostring` and `tonumber` wrap an expression wherever it stands.
- **Lua live templates.** `func`, `lfunc`, `for`, `fori`, `forp`, `if`, `elseif`, `ifelse` and `closure` expand
  into their statement, with tab stops; they are listed under Settings › Editor › Live Templates.
- **Extend Selection knows CfxLua.** The selection grows from the word at the caret to what a string holds,
  then to each expression, statement (with its documentation comment) and block around it; inside a
  documentation comment it grows through the type, the tag and the comment.
- **Strings of another language.** A `---@language JSON` comment above a statement, or above a field of a
  table, makes the strings of that statement JSON (or SQL, HTML, JavaScript…): OpenIDEA colours them as such.
- **Gutter markers of classes and calls.** A method that overrides one of a parent class, or that a subclass
  overrides, has a marker that leads there; a `---@class` shows whether it has subclasses; a recursive call and
  a call that is returned (a tail call) are marked. Each kind has a setting.
- **A Lua formatter, the one of the JetBrains plugin.** Once the plugin is chosen for `lua` in **Settings ›
  Languages & Frameworks › Plugin Formatters**, Reformat Code (a file or a selection) and Format on save indent
  blocks, tables and what continues a line, put statements and the bodies of blocks on their own lines, write the
  spaces around operators, `=` and commas and inside the braces of a table, line up the parameters written on
  several lines and the dots of a chained call, keep one blank line at most between two statements, and take out
  the semicolons no statement needs. Only white space changes. The indent is the editor's; a line that continues another is
  indented twice, as IntelliJ does, unless a setting gives its width. The other options of the JetBrains plugin
  are settings with IntelliJ's defaults: keeping a block of one statement on one line, aligning arguments, the
  `=` of consecutive assignments and of table fields, and each of the spaces. A semicolon that separates table
  fields, or that keeps a statement starting with a parenthesis from being read as a call, stays. A file over
  1 MiB is not formatted, and Reformat Code says so.
- **Natives have parameter info, hints, a hover, and type what they return.** A native shows its parameters while
  its arguments are typed and names them before literals; `GetEntityCoords(ped).` lists `x`, `y` and `z`; hovering
  a native shows its documentation: description, example, parameters, hash, side and game.
- **The memory the plugin holds is written in the IDE log** when it has read a project
  (`resources indexed (N MiB)`): OpenIDEA gives a plugin 256 MiB.

### Fixed

- **Completion that came too late in big files.** Every keystroke made the plugin index the edited file again for
  all its features (events, NUI, state bags, performance facts) before it listed anything, and the completions of
  state bag keys and of enumerated values each analysed the whole file first. A name being typed needs none of it.
  The file is now indexed by the first request that reads those indexes, once the typing pauses, and those two
  completions analyse the file only where they apply. In a dense 450 KB file the list took the 50 ms the editor
  waits and now comes in under 10 ms; in files close to 1 MB it never came and now takes about 20 ms.
- **Completion that came too late on big projects.** Every keystroke in a file that calls events made the plugin
  detect the event functions of the whole project again before it answered: on a 5,000-file project the editor had
  stopped waiting and listed nothing. They are now detected again only when an edit touches one of them, and a
  name being typed no longer waits for the event index: only a completion inside a string reads it.
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
- A comment on a line that ends with a lone carriage return (old Mac line endings) ran over the lines after it, so
  nothing was found in them.
- Ctrl+click on a native said "Cannot find declaration to go to": native declarations now come from
  lua-language-server and the generated definitions, without reading that file on every hover.
- **Natives listed up to four times in completion, and "Choose Declaration (3 found)" on Ctrl+click,** once the
  generated definitions were loaded: this plugin listed the native, and lua-language-server once per game signature.
  A native of several games now has one signature in the definitions (parameters some games lack are optional), and
  this plugin leaves the natives and runtime API lua-language-server loads to it. The file's header says which games
  and runtime parts it holds; this plugin still completes the natives of a game it lacks.
- **Natives listed twice again after a restart.** OpenIDEA does not list a `.cfxlua/` that Git ignores (before
  0.1.39, any hidden folder), so the plugin only knew the definitions once their file was saved. It now reads them by name when it starts (the files the
  command wrote, and `natives.lua` in each library folder) and after a repository change, and reads `.luarc.json`
  again when you switch editor, so an edit made outside the IDE is followed.
- A native of FiveM and RedM could get two parameters of the same name in the definitions (four of them, when both
  games are generated together): lua-language-server then reported valid calls as type errors.
- Server natives whose client name is an alias (`GetVehicleDashboardColour`…) were marked deprecated.
- The status bar shows the side of the first file opened.
- **CfxLua: Convert Coordinates** stays enabled when run from Find Action.
- **Enable FiveM / RedM support turned off, then on again,** left completion, hover, colours and every other
  editor feature of the plugin off until one of its commands was run or its tool window used: while it is off,
  the plugin now reads the setting again at each request of the editor.

### Changed

- **OpenIDEA 0.1.40 or newer is required.** The plugin is built against plugin API 1.4 (it was 1.0); its features are
  unchanged.
- **One CfxLua parser reads every file**, the one of the JetBrains plugin ported to Rust: events, state bags, NUI,
  performance, coordinates, magic values and syntax checks used to read Lua through three partial parsers that could
  disagree. What they report is unchanged; code being typed is read the same way by all of them.
- **Indexing a project takes about a tenth less time** (measured on 5,000 files): what the features read from a Lua
  file is built once and released with the file, instead of being looked up again by each of them.
- **The component is compiled for speed** instead of size: through OpenIDEA it answers about 30% faster and reads a
  5,000-file project in 9 s instead of 11.5 s, for a package a tenth bigger (2.6 MB).
- **The plugin holds less than half the memory on big projects.** OpenIDEA gives a plugin 256 MiB, and a 5,500-file
  project took 209 of them once it was read: it now takes about 90. The text of a file is released once it is
  indexed and read again when something needs it, and what the events, performance, magic values, coordinates and
  NUI features and the type engine keep of each file is packed, each name kept once instead of once per use. What
  they report is unchanged.
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
