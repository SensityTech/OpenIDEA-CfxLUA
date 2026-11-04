# RedM and FiveM: what differs

FiveM (GTA V) and RedM (Red Dead Redemption 2) are built from the same Cfx.re code base. The Lua language, the scheduler, events, exports, state bags, NUI, `json`/`msgpack` and the server are the same. What differs is the game: its natives, its data, and a few platform features.

## What is identical

- The CfxLua dialect: compound assignments, safe navigation, backtick hashes, vectors... (`cfxdoc:cfxlua-syntax`).
- The runtime API: `CreateThread`, `Wait`, events, exports, state bags, `promise`, `json`, `msgpack`.
- The server (FXServer) and all **server natives**: the same server script runs for both games, as long as it does not depend on game data.
- NUI (`SendNUIMessage`, `RegisterNUICallback`, `SetNuiFocus`).
- `RegisterCommand`, ACE permissions, convars.

## Manifest

RedM resources must declare the game and acknowledge the pre-release status, with this exact sentence:

```lua
fx_version 'cerulean'
game 'rdr3'
rdr3_warning 'I acknowledge that this is a prerelease build of RedM, and I am aware my resources *will* become incompatible once RedM ships.'
```

Without the correct `rdr3_warning`, the server refuses to start the resource on RedM. A resource that supports both games uses `games { 'gta5', 'rdr3' }` together with `rdr3_warning`. See `cfxdoc:fxmanifest`.

## Natives

| Topic | FiveM | RedM |
|---|---|---|
| Game natives | GTA V natives, documented at docs.fivem.net/natives | RDR3 natives, documented by community databases such as alloc8or's RDR3 NativeDB |
| Unnamed natives | fewer | many more; `Citizen.InvokeNative(0x...)` and `N_0x...` are common |
| Cfx client natives | GTA-specific ones (marked `gta5`) plus shared ones | RedM-specific ones (marked `rdr3`) plus shared ones |

### Names that do not exist at runtime

RedM's Lua natives are generated from the definition file the RedM runtime ships with. Community databases are updated more often and sometimes **rename** natives (or name previously unnamed ones). Such a newer name is `nil` in game.

When a name from a database does not resolve:

1. Look the native up by hash in the **Natives** tab of the CfxLua tool window; it shows the name that exists at runtime (or `N_0x...`) and mentions the database name.
2. Call it by that name, or by hash:

```lua
-- the database name may not exist in the runtime; the hash always works
local function callByHash(hash, ...)
    return Citizen.InvokeNative(hash, ...)
end
```

The plugin's RedM stubs use the runtime names, so completion only proposes callable names.

### Same name, different signature

Many natives exist in both games under the same name, but with different parameters. Never copy a call from GTA V code into RedM code (or the reverse) without checking the signature for the target game:

| Native | FiveM (GTA V) | RedM (RDR3) |
|---|---|---|
| `GetEntityCoords` | `GetEntityCoords(entity, alive)` | `GetEntityCoords(entity, alive, realCoords)` |
| `GetCurrentPedWeapon` | `GetCurrentPedWeapon(ped, p2)` returns `retval, weaponHash` | `GetCurrentPedWeapon(ped, p2, attachPoint, p4)` returns `retval, weaponHash` |

```lua
-- RedM client
local ped = PlayerPedId()
local pos = GetEntityCoords(ped, false, false)
local hasWeapon, weaponHash = GetCurrentPedWeapon(ped, true, 0, false)
if hasWeapon then
    print(('holding %d at %.1f, %.1f'):format(weaponHash, pos.x, pos.y))
end
```

With the project's game set correctly, completion and its documentation show the signature of the game the file targets.

### Platform natives that differ

| Feature | FiveM | RedM |
|---|---|---|
| Rebindable key mappings | `RegisterKeyMapping` | not available; use `RegisterRawKeymap` (RedM only) or poll controls |
| `gameEventTriggered`, `entityDamaged`, `populationPedCreating` client events | available | not raised |
| Controls | `IsControlJustPressed(0, 38)` with numeric control ids | controls identified by hash values, `IsControlJustPressed(0, hash)` |

## Server-side game events (OneSync)

The server parses some game events sent by clients and raises script events for them. The list differs per game:

| Event | FiveM | RedM |
|---|---|---|
| `weaponDamageEvent`, `explosionEvent`, `startProjectileEvent`, `clearPedTasksEvent`, `respawnPlayerPedEvent` | yes | yes (with different data fields) |
| `fireEvent`, `giveWeaponEvent`, `removeWeaponEvent`, `removeAllWeaponsEvent`, `ptFxEvent`, `vehicleComponentControlEvent` | yes | no |
| `lightningEvent`, `endLootEvent`, carriable events (`pickupCarriableEvent`, ...) | no | yes |

The Events tab of the CfxLua tool window lists each event with its game.

## Writing code for both games

Check the game at runtime with `GetGameName()`, which returns `'fivem'` or `'redm'` on clients and `'fxserver'` on the server:

```lua
local IS_REDM <const> = GetGameName() == 'redm'

local function notify(message)
    if IS_REDM then
        print(('[notify] %s'):format(message)) -- replace with a RedM UI of your choice
    else
        BeginTextCommandThefeedPost('STRING')
        AddTextComponentSubstringPlayerName(message)
        EndTextCommandThefeedPostTicker(false, false)
    end
end
```

A per-game configuration table keeps the differences in one place:

```lua
local GAME <const> = GetGameName()

local CONFIG <const> = { -- example values
    fivem = { spawn = vector3(-268.4, -957.3, 31.2), model = `mp_m_freemode_01` },
    redm = { spawn = vector3(-283.0, 795.0, 118.0), model = `mp_male` },
}

local function getConfig()
    return CONFIG[GAME] or CONFIG.fivem
end
```

Tips:

- Keep game-specific code in separate files (`client/gta5.lua`, `client/rdr3.lua`) and shared logic in common files.
- On the server, `GetGameName()` returns `'fxserver'` for both games. A RedM server is started with the convar `gamename` set to `rdr3`, so server code can check `GetConvar('gamename', 'gta5') == 'rdr3'`.
- Set the project's game to **Both** in Settings > Languages & Frameworks > CfxLua (or let Auto read `games { 'gta5', 'rdr3' }`), so that both native sets are known to the IDE.
- Hashes, model names and coordinates are different between the games; keep them in per-game configuration tables.

## Sources

- RedM: https://redm.net/
- Setting up a server (`gamename`): https://docs.fivem.net/docs/server-manual/setting-up-a-server-vanilla/
- RDR3 NativeDB: https://alloc8or.re/rdr3/nativedb/
- FiveM natives: https://docs.fivem.net/natives/
- GET_GAME_NAME: https://docs.fivem.net/natives/?_0xE8EAA18B
- RedM manifest check (`ServerResources.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-server-impl/src/ServerResources.cpp
- Server game events per game (`ServerGameState.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-server-impl/src/state/ServerGameState.cpp
