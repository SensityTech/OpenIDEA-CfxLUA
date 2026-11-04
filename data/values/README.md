# Sensity DevKit magic values data: sources and attribution

Compact tables used by the "magic values decoder" (`com.tang.intellij.lua.cfx.values`): inlay hints, quick
documentation, completion and intentions that give a name to the numbers passed to FiveM / RedM natives.
Only facts are bundled (numeric ids, game identifiers, default key bindings, colour components): no prose, no
images. Every table links back to its source in the quick documentation.

| File | Content | Source | License / terms |
| --- | --- | --- | --- |
| `controls.json` | GTA V control ids: `[id, name, default keyboard key, default Xbox pad button]` | Cfx.re documentation, [Controls](https://docs.fivem.net/docs/game-references/controls/) (`citizenfx/fivem-docs`, `content/docs/game-references/controls.md`) | The repository has no license file and the site shows no copyright notice or terms of reuse; the table is a factual listing of the game's control enumeration. Attribution kept. |
| `blip-sprites.json` | GTA V blip sprite ids: `[id, name]` (the sprite images are **not** bundled, the documentation links to the page) | Cfx.re documentation, [Blips](https://docs.fivem.net/docs/game-references/blips/) | Same as above. |
| `blip-colours.json` | GTA V blip colour ids: `[id, name, #rrggbb]` (the friendly / enemy variants of the page are not kept) | Cfx.re documentation, [Blips: Blip colors](https://docs.fivem.net/docs/game-references/blips/#blip-colors) | Same as above. |
| `markers.json` | GTA V `DRAW_MARKER` types: `[id, name]` (images not bundled) | Cfx.re documentation, [Markers](https://docs.fivem.net/docs/game-references/markers/) | Same as above. |
| `hud-colours.json` | GTA V HUD colours: `[id, name, r, g, b, a]` | Cfx.re documentation, [HUD Colors](https://docs.fivem.net/docs/game-references/hud-colors/) (`common:/data/ui/hudcolor.dat`) | Same as above. |
| `explosions.json` | `eExplosionTag` values: `gta5` and `rdr3`, `[id, name]` (`EXP_TAG_` prefix removed) | GTA V: documentation of `ADD_EXPLOSION` in the Cfx.re native database (`https://static.cfx.re/natives/natives.json`, from `citizenfx/natives`); RDR3: documentation of `ADD_EXPLOSION` in the RDR3 NativeDB (`alloc8or/rdr3-nativedb-data`) | Both databases are already bundled with the plugin (`cfx/natives-snapshot`); only the enumeration names and values are extracted. |
| `anim-flags.json` | GTA V `eScriptedAnimFlags` bits (`TASK_PLAY_ANIM` flag): `[bit, name]` | Documentation of `TASK_PLAY_ANIM` in the Cfx.re native database (as above) | As above. The RDR3 flags are not bundled: the only public list (`Halen84/RDR3-Native-Flags-And-Enums`) has no license. |
| `hash-names.txt.gz` | Names whose joaat hash is decoded out of the box: vehicles, peds, weapons, weapon components (one name per line, `#category` headers). Hashes are computed at load time with `CfxHash.joaat`; every name was checked against the hash of the source database when the file was extracted. | [calamity-inc/gta-v-joaat-hash-db](https://github.com/calamity-inc/gta-v-joaat-hash-db) (`out/vehicles`, `out/peds`, `out/weapons`, `out/weapon_components`) | [The Unlicense](https://github.com/calamity-inc/gta-v-joaat-hash-db/blob/senpai/LICENSE) (public domain). |

## Optional download (never bundled)

The full GTA V hash dictionary of `calamity-inc/gta-v-joaat-hash-db` (`out/dictionary-dec_signed.tsv`, about 7.6 MB,
The Unlicense) is only downloaded on an explicit user action (*Tools > Download GTA V Hash Names*), into the IDE
system directory (`<system>/cfxlua/hashes/`), and can be deleted from the same menu.

## RedM

RDR3 control ids are joaat hashes of the control names (`IS_CONTROL_JUST_PRESSED(0, 0xCEFD9220)` is
`` `INPUT_ENTER` ``): the decoder recognizes the RDR3 controls whose name is also a GTA V control name by hashing
the names of `controls.json`, so no RDR3 control table is bundled. The community RDR3 tables
(`femga/rdr3_discoveries`) have no license and are not bundled.

## Updating

The tables were extracted with a small script from the sources above (October 2026): markdown tables / HTML of the
documentation pages, `enum` blocks of the native documentation, CSV files of the hash database. Keep the formats
(compact JSON arrays, one entry per line) and this file in sync when updating.
