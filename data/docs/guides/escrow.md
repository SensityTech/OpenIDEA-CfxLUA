# Selling resources with Asset Escrow

Cfx Asset Escrow protects the code of resources sold on Tebex: you upload the resource as a zip on the Cfx.re portal, the portal encrypts it, and buyers receive a protected build that only starts on servers whose license key owns the asset. This guide covers what escrow encrypts, how to keep configuration editable with `escrow_ignore`, the mistakes that only show up after the upload, and the OpenIDEA-CfxLUA tools that catch them before.

## What escrow encrypts

| Files | In the protected build |
|---|---|
| `.lua` scripts | encrypted, unless `escrow_ignore` matches them |
| `.yft`, `.ydd`, `.ydr` models | encrypted, unless `escrow_ignore` matches them |
| `fxmanifest.lua` | always readable: FXServer reads it to load the resource |
| NUI (`.html`, `.js`, `.css`) | shipped as is: NUI is not supported by escrow |
| JavaScript and C# scripts | shipped as is: escrow only protects Lua |
| everything else (`.ytd`, `.ymap`, `.meta`, `.json`, images, audio) | shipped as is |

Encrypted files keep their name and place. Their content becomes a binary blob that starts with the bytes `FXAP`, and the folder gets a hidden `.fxap` key file. Only FXServer decrypts them, in memory, after checking that the server's license key is entitled to the asset; otherwise the console prints "You lack the required entitlement" and the resource does not start.

Obfuscating escrowed code is unnecessary: buyers never see it.

## Keeping files editable: escrow_ignore

Buyers expect to edit the configuration and the translations of what they buy. Escrow encrypts every Lua file that `escrow_ignore` does not match, `config.lua` included:

```lua
fx_version 'cerulean'
game 'gta5'

shared_scripts {
    'config.lua',
    'locales/*.lua',
}
client_scripts { 'client/*.lua' }
server_scripts { 'server/*.lua' }

escrow_ignore {
    'config.lua',          -- one file
    'locales/*.lua',       -- every .lua file right in locales/
    'bridge/**/*.lua',     -- every .lua file of bridge/ and its subfolders
    'stream/**/*.yft',     -- works for models too
}
```

- Paths are relative to the resource folder, with `/` separators.
- `*` matches inside one folder: `locales/*.lua` does not match `locales/extra/fr.lua`.
- `**` crosses folders: `stream/**/*.yft` matches `stream/car.yft` and `stream/parts/wheel.yft`.
- A pattern that matches nothing is not an error, it just protects nothing: check the spelling.

What to keep open, typically: `config.lua` (or `shared/config.lua`, `config/*.lua`), translations (`locales/`, `lang/`), framework bridges and other files you document as "editable". Keep secrets (webhook URLs, API keys) out of open files: read them from convars on the server instead (see `cfxdoc:security`).

## Files loaded at runtime

`LoadResourceFile` returns a file as it is stored in the resource. In the protected build, a Lua file loaded that way and compiled with `load` holds encrypted bytes that `load` cannot compile. The module loaders of ox_lib (`require`, `lib.require`, `lib.load`) read modules exactly this way. Cfx does not document an exception for them: keep every Lua file loaded from its path in `escrow_ignore`, or list it as a script of the manifest.

```lua
-- data/items.lua must be listed in escrow_ignore
local chunk = LoadResourceFile(GetCurrentResourceName(), 'data/items.lua')
local items = load(chunk, '@@my_resource/data/items.lua', 't')()
```

Data that buyers edit fits better in JSON, which escrow never encrypts:

```lua
local items = json.decode(LoadResourceFile(GetCurrentResourceName(), 'data/items.json'))
```

## Before uploading

- [ ] `fxmanifest.lua` sits at the root of the resource folder in the zip, and parses: a syntax error fails the upload with "error loading manifest file".
- [ ] `fx_version` and `game` are declared (and `rdr3_warning` for RedM): otherwise the resource never starts. See `cfxdoc:fxmanifest-essentials`.
- [ ] `escrow_ignore` lists the configuration, the translations and every file loaded with `LoadResourceFile` or `require`.
- [ ] No syntax error in the files that will be encrypted: buyers cannot fix them.
- [ ] The zip is under 1 GB.
- [ ] You upload your sources, not a protected build downloaded from the portal (files starting with `FXAP`, a `.fxap` file).
- [ ] After the upload, download the protected build from the portal and start it on a server whose license key belongs to your account.

The 2021 announcement of escrow asked for `lua54 'yes'` in the manifest. Every script runs on Lua 5.4 since June 2025 and the flag has no effect any more; add it only if the portal asks for it.

## What OpenIDEA-CfxLUA does for vendors

**Status of each file.** Every Lua file of a resource that declares `escrow_ignore` shows a hint on its first line, `escrow: encrypted` or `escrow: open`, following the rules above. **Settings › Plugins › OpenIDEA-CfxLUA** turns the status off (*Show the Asset Escrow status of resource files*), or shows it for every resource (*Treat every resource as escrowed*).

**Checks**, shown in the editor and the Problems view, in resources declaring `escrow_ignore`:

| Inspection | Reports | Quick fix |
|---|---|---|
| Configuration or translation file encrypted by Asset Escrow | `config.lua`, `*_config.lua`, `settings.lua`, files of `config/`, `locales/`, `lang/`, `i18n/` not matched by `escrow_ignore` | add the file, or `locales/*.lua`, to `escrow_ignore` |
| File loaded at runtime encrypted by Asset Escrow | `LoadResourceFile(GetCurrentResourceName(), 'x.lua')`, `require`, `lib.require`, `lib.load` of a file that will be encrypted | add the file to `escrow_ignore` |
| Script that Asset Escrow cannot protect | `.js` and `.dll` scripts of the manifest | |

**Pre-publish check.** **CfxLua: Check Resource Before Escrow Upload** (Tools menu, or Find Action) checks the resource of the focused file, or every resource of the project when no file is open. Its notification lists the blockers (manifest syntax, missing `fx_version` / `game` / `rdr3_warning`, legacy `__resource.lua`), the warnings (missing `escrow_ignore`, configuration and loaded files that will be encrypted, `escrow_ignore` patterns matching nothing, syntax errors in encrypted files, protected builds) and the notes (readable JavaScript and NUI files). On the manifest, the quick action *Add N patterns to escrow_ignore* (Alt+Enter) adds every suggested pattern in one step.

**Protected files.** A Lua file whose content starts with `FXAP` is an encrypted file of a protected build. OpenIDEA-CfxLUA does not parse it and says so with a single note, "This file is protected by Cfx Asset Escrow", instead of reporting hundreds of syntax errors.

## Shipping protected builds

- Keep the `.fxap` file: some FTP clients skip hidden files, and the resource then fails to verify. Upload the zip and extract it on the server when possible.
- Do not edit encrypted files: the resource no longer starts.
- Escrowed resources need a server build 5181 or newer.

## See also

- `cfxdoc:fxmanifest` for every manifest directive, `escrow_ignore` included
- `cfxdoc:fxmanifest-essentials` for the checklist of a manifest
- `cfxdoc:nui` for NUI pages, which escrow does not protect
- `cfxdoc:security` for keeping secrets out of client and open files

## Sources

- Asset Escrow documentation: https://docs.fivem.net/docs/server-manual/asset-escrow/
- Escrow announcement (Lua 5.4, server 5181, `.fxap` file, NUI): https://forum.cfx.re/t/introducing-asset-escrow-for-your-resources/4777151
- Manifest errors at upload: https://forum.cfx.re/t/help-error-finding-fxmanifest-lua-when-uploading-asset-to-keymaster/4813129
- Removal of Lua 5.3 support: https://forum.cfx.re/t/removal-of-lua-5-3-support/5335232
- ox_lib module loading (`LoadResourceFile` + `load`): https://github.com/overextended/ox_lib/blob/master/imports/require/shared.lua
