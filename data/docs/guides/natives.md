# Calling natives from Lua

Natives are the functions exposed by the game engine (GTA V or RDR3) and by the Cfx.re platform. In Lua they are ordinary global functions such as `PlayerPedId()` or `GetEntityCoords(ped)`. They are loaded lazily: the first time a script reads an unknown global, the runtime looks it up in the native table and installs it.

This guide explains how a native's C signature becomes a Lua call: names, arguments, return values, and the quirks that regularly cause bugs.

## Where to find natives

| Set | Contents | Reference |
|---|---|---|
| GTA V client | Game natives for FiveM | https://docs.fivem.net/natives/ |
| RDR3 client | Game natives for RedM | https://alloc8or.re/rdr3/nativedb/ (descriptions; see the RedM notes below) |
| Cfx (namespace `CFX`) | Platform natives: events, resources, state bags, NUI, server player management... Each has an API set: `client`, `server` or `shared` | https://docs.fivem.net/natives/ (namespace CFX) |

In the IDE, the **Natives** tab of the CfxLua tool window and completion list them all, filtered by your project's game and by the side of the current file (see `cfxdoc:plugin-guide`).

## Naming rules

Natives are documented in `UPPER_SNAKE_CASE`. The Lua name is derived mechanically:

1. Lowercase the name, and turn a hash name `0x...` into `n_0x...`.
2. Remove each underscore that is followed by a **letter**, and uppercase that letter.
3. Uppercase the first letter.

An underscore followed by a **digit** is kept. That is why some names contain an underscore:

| Native name | Lua name |
|---|---|
| `GET_ENTITY_COORDS` | `GetEntityCoords` |
| `GET_GROUND_Z_FOR_3D_COORD` | `GetGroundZFor_3dCoord` |
| `DRAW_DEBUG_TEXT_2D` | `DrawDebugText_2d` |
| `_GET_ENTITY_BONE_POSITION_2` | `GetEntityBonePosition_2` |
| `0x0B40ED49D7D6FF84` (no name) | `N_0x0b40ed49d7d6ff84` |

The same rule written in Lua:

```lua
local function toLuaName(nativeName)
    local s = nativeName:lower():gsub('0x', 'n_0x')
    s = s:gsub('_(%a)', string.upper)
    s = s:gsub('^(%a)', string.upper)
    return s
end

print(toLuaName('GET_GROUND_Z_FOR_3D_COORD')) -- GetGroundZFor_3dCoord
print(toLuaName('0x0B40ED49D7D6FF84'))        -- N_0x0b40ed49d7d6ff84
```

Notes:

- Native names that start with `_` were named by the community. They are regular names, not deprecated ones; the Lua name just drops the leading underscore.
- Unnamed natives are only callable as `N_0x<hash>` with the hash in **lowercase** hexadecimal.
- With the experimental `use_experimental_fxv2_oal 'yes'` manifest flag, `GetGroundZFor3dCoord` (without the underscore) also resolves. The underscore form works in both modes, so use it.

## Arguments

| C type in the reference | What to pass from Lua |
|---|---|
| `int`, `Any`, entity handles (`Entity`, `Ped`, `Vehicle`, `Object`, `Blip`, `Cam`, ...) | integer |
| `float` | number |
| `BOOL` | boolean (`true` / `false`; numbers are also accepted) |
| `char*` | string (`nil` passes a null pointer) |
| `Hash` | integer **or string**: strings are hashed for you |
| `Vector3` (by value, Cfx natives) | `vector3` |
| `func` (Cfx natives) | Lua function |
| `object` (Cfx natives) | any Lua value; it is serialized with msgpack |
| `Player` on the **server** | the player's server id, as a string or a number (`source` works directly) |
| pointer `T*` (other than `char*`) | usually nothing: see out-parameters below |

### Hashes

Model names, weapon names and similar identifiers are Jenkins hashes. Three ways to produce one:

```lua
RequestModel('adder')                -- string, hashed by the runtime on each call
RequestModel(`adder`)                -- compile-time hash, free at runtime (preferred)
RequestModel(GetHashKey('adder'))    -- explicit runtime hash
local name = 'adder'
RequestModel(joaat(name))            -- runtime hash without a native call
```

Hash values returned by natives are **signed 32-bit integers** (values above `0x7FFFFFFF` come back negative), exactly like backtick hashes and `joaat`, so comparisons work directly:

```lua
local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
if vehicle ~= 0 and GetEntityModel(vehicle) == `adder` then
    print('nice car')
end
```

To display a hash the way most databases list it, mask it: `` `adder` & 0xFFFFFFFF ``.

## Return values and out-parameters

Many natives write results through pointer parameters (`float* groundZ`, `Vector3* coords`, `Entity* entity`). In Lua these parameters are **removed** from the argument list and their values are **returned**, after the native's own return value, in declaration order.

```lua
-- C: BOOL GET_GROUND_Z_FOR_3D_COORD(float x, float y, float z, float* groundZ, BOOL includeWater)
local coords = GetEntityCoords(PlayerPedId())
local found, groundZ = GetGroundZFor_3dCoord(coords.x, coords.y, coords.z, false)
if found then
    print(('ground at %.2f'):format(groundZ))
end

-- C: BOOL GET_CURRENT_PED_WEAPON(Ped ped, Hash* weaponHash, BOOL p2)
local hasWeapon, weaponHash = GetCurrentPedWeapon(PlayerPedId(), true)

-- C: int GET_SHAPE_TEST_RESULT(int handle, BOOL* hit, Vector3* endCoords, Vector3* surfaceNormal, Entity* entityHit)
local function raycast(from, to, ignore)
    local handle = StartShapeTestLosProbe(from.x, from.y, from.z, to.x, to.y, to.z, -1, ignore, 4)
    local status, hit, endCoords, surfaceNormal, entityHit
    repeat
        Wait(0)
        status, hit, endCoords, surfaceNormal, entityHit = GetShapeTestResult(handle)
    until status ~= 1 -- 1 means "still running"
    return status == 2 and hit == 1, endCoords, surfaceNormal, entityHit
end
```

A `void` native with pointers returns only the pointer values.

### The single trailing pointer exception

If a native has **exactly one** pointer parameter and it is the **last** parameter, it stays in the Lua signature (as an initial value) and its final value is also returned:

```lua
-- C: void DELETE_ENTITY(Entity* entity)
local function removeEntity(entity)
    if DoesEntityExist(entity) then
        SetEntityAsMissionEntity(entity, true, true)
        DeleteEntity(entity) -- the handle is passed in, as you would expect
    end
end

-- C: BOOL GET_PED_LAST_WEAPON_IMPACT_COORD(Ped ped, Vector3* coords)
local hasImpact, impact = GetPedLastWeaponImpactCoord(PlayerPedId())
```

For a trailing `Vector3*` or `BOOL*`, the value you pass is ignored; only the returned value matters.

The documentation of a native (in completion, or with **CfxLua: Show Documentation**) lists the exact Lua return values, so you never need to work this out by hand.

## The BOOL quirks

Two behaviours of the default runtime regularly cause bugs:

1. **Natives returning `BOOL` return `false` or a non-zero integer** (usually `1`), not `true`.
2. **`BOOL*` out-parameters come back as the integers `0` or `1`.**

Since every number, including `0`, is truthy in Lua:

```lua
local ped = PlayerPedId()

-- BOOL return: test truthiness, never compare with true
if IsPedInAnyVehicle(ped, false) then end          -- correct
if IsPedInAnyVehicle(ped, false) == true then end  -- WRONG: always false

-- BOOL* out-parameter: compare with 1, never test truthiness
local from = GetEntityCoords(ped)
local to = from + vector3(0.0, 0.0, -10.0)
local handle = StartShapeTestLosProbe(from.x, from.y, from.z, to.x, to.y, to.z, -1, ped, 4)
Wait(0)
local status, hit = GetShapeTestResult(handle)
if status == 2 and hit == 1 then end -- correct
if hit then end                      -- WRONG: 0 is truthy, always true
```

To get a real boolean, write `local inVehicle = IsPedInAnyVehicle(ped, false) and true or false`, or `hit == 1`.

## Entity handles and network IDs

- Entity handles (`Ped`, `Vehicle`, `Object`, `Entity`) are **integers local to the machine** that obtained them. The same car has a different handle on each client and on the server. Never send a handle to another machine.
- To refer to a networked entity across machines, send its **network ID**:

```lua
-- client
local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
if vehicle ~= 0 and NetworkGetEntityIsNetworked(vehicle) then
    TriggerServerEvent('garage:store', NetworkGetNetworkIdFromEntity(vehicle))
end
```

```lua
-- server
RegisterNetEvent('garage:store', function(netId)
    local src = source
    if type(netId) ~= 'number' then return end
    local vehicle = NetworkGetEntityFromNetworkId(netId)
    if vehicle == 0 or not DoesEntityExist(vehicle) then return end
    print(('player %d stores vehicle %d'):format(src, vehicle))
end)
```

- A handle of `0` means "no entity". Always check `~= 0` and, after any wait, `DoesEntityExist(handle)`: entities can be deleted or leave your scope at any time.
- Server-side entity handles and the server entity natives require OneSync to be enabled on the server.

## Vectors

Natives that return a `Vector3` return a `vector3` value:

```lua
local pos = GetEntityCoords(PlayerPedId())
print(pos.x, pos.y, pos.z, type(pos)) -- ... vector3
```

When you pass a `vector2`, `vector3`, `vector4` or `quat` as an argument, the runtime **unrolls** it into 2, 3 or 4 consecutive arguments. So `x, y, z` parameters can receive one vector:

```lua
local ped = PlayerPedId()
local target = vector3(-75.0, -818.0, 326.0)
SetEntityCoords(ped, target, false, false, false, false)
-- is the same as
SetEntityCoords(ped, target.x, target.y, target.z, false, false, false, false)
```

Unrolling is positional, so it only works when the native's Lua parameters really are consecutive numbers. It breaks for natives whose Lua wrapper inserts hidden out-parameters after the coordinates (for example `GetGroundZFor_3dCoord`), and it is disabled entirely when the manifest enables `use_experimental_fxv2_oal`. When in doubt, pass the components. See `cfxdoc:vectors-and-math`.

## Calling a native by hash: `Citizen.InvokeNative`

Use `Citizen.InvokeNative(hash, ...)` for natives that are missing from the generated wrappers or whose wrapper is wrong. You describe the result type and out-parameters yourself with marker values:

| Marker | Meaning |
|---|---|
| `Citizen.ResultAsInteger()` | return value is an int |
| `Citizen.ResultAsLong()` | return value is a 64-bit integer |
| `Citizen.ResultAsFloat()` | return value is a float |
| `Citizen.ResultAsString()` | return value is a `char*` |
| `Citizen.ResultAsVector()` | return value is a `Vector3` (returned as `vector3`) |
| `Citizen.ReturnResultAnyway()` | return the raw result (used for `BOOL`) |
| `Citizen.PointerValueInt()` | an `int*` / handle / `BOOL*` out-parameter |
| `Citizen.PointerValueFloat()` | a `float*` out-parameter |
| `Citizen.PointerValueVector()` | a `Vector3*` out-parameter |
| `Citizen.PointerValueIntInitialized(v)` | an `int*` in/out parameter starting at `v` |

Results come back in this order: the main result (if a result marker was given), then one value per pointer marker.

```lua
local ped = PlayerPedId()

-- GET_ENTITY_COORDS
local coords = Citizen.InvokeNative(0x3FEF770D40960D5A, ped, false, Citizen.ResultAsVector())

-- GET_GROUND_Z_FOR_3D_COORD: BOOL result + float* out-parameter
local found, groundZ = Citizen.InvokeNative(0xC906A7DAB05C8D2B,
    coords.x, coords.y, coords.z, Citizen.PointerValueFloat(), false,
    Citizen.ReturnResultAnyway())
```

- GTA V and RDR3 natives use 64-bit hashes (`0x3FEF770D40960D5A`). Cfx natives use 32-bit hashes (`0x4D97BCC7`).
- Without a result marker and without pointers, the result is a 32-bit integer or `false`.
- `Citizen.PointerValueFloatInitialized(v)` currently ignores `v` and starts at `0`; do not rely on the initial value.
- `Citizen.GetNative(hash)` (client only) returns a callable for a native hash.
- This style is very common in RedM code, because many RDR3 natives are unnamed.

## Aliases and deprecated names

Natives get renamed as research progresses. Old names stay callable:

- old names, usually starting with `_` (for example `_GET_RAYCAST_RESULT`, now `GET_SHAPE_TEST_RESULT`, so `GetRaycastResult` still works);
- hash aliases, callable as `N_0x...`.

OpenIDEA-CfxLUA hides them from completion unless you ask for them, and their documentation names the current native. Prefer the current name in new code.

## Client, server and RPC natives

Each native is available on one side only, unless it is a `shared` Cfx native:

| Side | Examples |
|---|---|
| Client | `PlayerPedId`, `GetEntityCoords`, `DrawRect`, `SetNuiFocus`, `RegisterKeyMapping` (FiveM) |
| Server | `GetPlayerIdentifierByType`, `DropPlayer`, `GetPlayerPed`, `SetPlayerRoutingBucket`, `IsPlayerAceAllowed` |
| Shared | `RegisterCommand`, `GetCurrentResourceName`, `GetResourceState`, `AddStateBagChangeHandler`, `GetGameName` |

The server has its own versions of some entity natives (`GetEntityCoords`, `DoesEntityExist`, `DeleteEntity`...), sometimes with fewer parameters: server `GetEntityCoords(entity)` takes only the entity.

**RPC natives** are server-side equivalents of client natives such as `CreateVehicle`, `CreatePed` or `SetPlayerModel`. The server asks a client to execute them. For vehicles, `CreateVehicleServerSetter(model, type, x, y, z, heading)` creates the entity directly on the server and is the more reliable choice:

```lua
-- server
local function spawnHeli(src)
    local ped = GetPlayerPed(src)
    local pos = GetEntityCoords(ped) + vector3(0.0, 0.0, 15.0)
    local heli = CreateVehicleServerSetter(`seasparrow`, 'heli', pos.x, pos.y, pos.z, 0.0)
    return heli ~= 0 and NetworkGetNetworkIdFromEntity(heli) or nil
end
```

On the server, `Player` parameters are strings, but the runtime converts numbers for you, so `GetPlayerName(source)` works.

## RedM specifics

- RedM natives are named after the definition file the RedM runtime is generated from. Community databases (alloc8or's RDR3 NativeDB, VORP) sometimes use **newer names that do not exist at runtime**. If a name from a database is `nil` in game, call the native by hash: `N_0x...` or `Citizen.InvokeNative(0x..., ...)`.
- More natives are unnamed than in GTA V, which is why RedM code uses `Citizen.InvokeNative` so often.
- Cfx natives marked `gta5` (such as `RegisterKeyMapping`) do not exist in RedM; some are RedM-only (such as `RegisterRawKeymap`).

See `cfxdoc:redm-vs-fivem`.

## Performance tips

- Every native call crosses from Lua into the engine. It is cheap, but not free: in a loop that runs every frame, call each native once and keep the result in a local.

```lua
CreateThread(function()
    while true do
        local ped = PlayerPedId()                 -- once per iteration
        local pos = GetEntityCoords(ped)
        if #(pos - vector3(441.8, -982.0, 30.7)) < 2.0 then
            -- draw a prompt, check a key, ...
            Wait(0)
        else
            Wait(500)
        end
    end
end)
```

- Natives are looked up through the global table on first access. For very hot code, a local alias avoids repeated global lookups: `local GetEntityCoords = GetEntityCoords`.
- Use backtick hashes instead of strings for `Hash` parameters inside loops.
- Prefer `#(a - b)` over distance natives for vector distances (see `cfxdoc:vectors-and-math`).

More in `cfxdoc:performance`.

## Sources

- FiveM native reference: https://docs.fivem.net/natives/
- About native functions: https://docs.fivem.net/docs/scripting-manual/introduction/about-native-functions/
- Citizen.InvokeNative: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.InvokeNative/
- Lua wrapper generator (naming and out-parameter rules): https://github.com/citizenfx/fivem/tree/master/ext/natives
- RDR3 NativeDB: https://alloc8or.re/rdr3/nativedb/
