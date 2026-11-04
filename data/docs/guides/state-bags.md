# State bags

A state bag is a key/value store attached to the server (global state), to a player or to an entity. Values set on the server are replicated to clients automatically, and late joiners receive the current values. State bags are often a better fit than events for data that describes "how things are" (a player's job, a vehicle's fuel, whether a door is locked).

Entity state bags require OneSync (the server's "state awareness" mode).

## The four entry points

| Expression | Bag name | Available on |
|---|---|---|
| `GlobalState` | `global` | server and client |
| `Player(serverId).state` | `player:<serverId>` | server and client |
| `LocalPlayer.state` | `player:<your serverId>` | client only |
| `Entity(handle).state` | `entity:<netId>` (networked) or `localEntity:<handle>` (client, not networked) | server and client |

```lua
-- server
GlobalState.weather = 'RAIN'
Player(source).state.job = 'police'

local vehicle = GetVehiclePedIsIn(GetPlayerPed(source), false)
if vehicle ~= 0 then
    Entity(vehicle).state.fuel = 75.0
end
```

```lua
-- client
print(GlobalState.weather)
print(LocalPlayer.state.job)

local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
if vehicle ~= 0 then
    print(Entity(vehicle).state.fuel)
end
```

Reading a key that was never set returns `nil`.

## Writing and replication

| Who writes | Default | Override |
|---|---|---|
| Server (`bag.key = value`) | replicated to clients | `bag:set(key, value, false)` keeps it server-side |
| Client (`bag.key = value`) | **not** replicated, local only | `bag:set(key, value, true)` sends it to the server, if allowed |

```lua
-- server: keep an internal value out of the clients' reach
Player(source).state:set('lastAntiCheatScore', 12, false)

-- client: tell the server something (only works if the policy allows it)
LocalPlayer.state:set('isTyping', true, true)
```

`bag:set(key, value, replicated)` is the only method on a bag; every other key is a value. When `replicated` is omitted it means `false`.

### Who may write

By default:

- **global** state: only the server;
- **player** state: the server and that player;
- **entity** state: the server and the client that owns the entity.

The server convar `sv_stateBagStrictMode` set to `true` restricts every bag to server-only writes. Values written by a client, even when allowed, must be treated as untrusted input (see `cfxdoc:security`).

## Values are copied, not referenced

Each read deserializes the stored value and each write serializes the whole value. Changing a nested field of a value you read does nothing:

```lua
-- server
local state = Player(source).state

state.inventory = { water = 1 }

local inv = state.inventory
inv.water = 5            -- changes only the local copy
print(state.inventory.water) -- still 1

inv.bread = 2
state.inventory = inv    -- write the whole value back: now replicated
```

Practical consequences:

- Prefer several small keys (`state['inventory:water'] = 5`) over one large nested table that you rewrite often.
- Read a value once into a local instead of indexing the bag repeatedly in a loop.
- Values go through MessagePack: numbers, strings, booleans, tables and vectors are fine; functions are not meaningful. Keep values small, every change is sent to every client that can see the bag.

## Reacting to changes

`AddStateBagChangeHandler(keyFilter, bagFilter, handler)` calls `handler` whenever a matching key changes. Pass `nil` as a filter to match everything.

```lua
-- client: react when any player's "isTyping" changes
local cookie = AddStateBagChangeHandler('isTyping', nil, function(bagName, key, value, _reserved, replicated)
    local player = GetPlayerFromStateBagName(bagName) -- client: player handle, 0 if unknown
    if player == 0 then return end
    print(('player %d typing: %s'):format(GetPlayerServerId(player), tostring(value)))
end)
```

```lua
-- server: react to fuel changes on any entity
AddStateBagChangeHandler('fuel', nil, function(bagName, key, value)
    local entity = GetEntityFromStateBagName(bagName) -- 0 if the entity is unknown
    if entity == 0 then return end
    print(('entity %d fuel is now %.1f'):format(entity, value))
end)
```

Handler arguments:

| Argument | Meaning |
|---|---|
| `bagName` | the bag that changed: `global`, `player:<id>`, `entity:<netId>`, `localEntity:<handle>` |
| `key` | the key that changed |
| `value` | the **new** value. While the handler runs, the bag still holds the old value |
| `reserved` | unused |
| `replicated` | whether the write was meant to be replicated |

- `GetPlayerFromStateBagName(bagName)` returns the player's server id on the server and the player handle on the client (`0` if none).
- `GetEntityFromStateBagName(bagName)` returns the entity handle on this machine (`0` if none).
- `RemoveStateBagChangeHandler(cookie)` removes a handler.
- A handler cannot reject a change.
- To filter one bag, pass its name as `bagFilter`, for example `('player:%d'):format(serverId)`.

### Entities that are not loaded yet

On the client, a change can arrive for an entity that is not streamed in. Wait for it, with a deadline:

```lua
AddStateBagChangeHandler('locked', nil, function(bagName, key, value)
    local entity = GetEntityFromStateBagName(bagName)
    local deadline = GetGameTimer() + 2000
    while entity == 0 and GetGameTimer() < deadline do
        Wait(100)
        entity = GetEntityFromStateBagName(bagName)
    end
    if entity == 0 then return end
    SetVehicleDoorsLocked(entity, value and 2 or 1)
end)
```

## Low-level natives

The objects above are thin wrappers over natives you can also call directly:

| Native | Purpose |
|---|---|
| `GetStateBagValue(bagName, key)` | read a value |
| `GetStateBagKeys(bagName)` | list the keys of a bag |
| `StateBagHasKey(bagName, key)` | test a key |
| `SetStateBagValue(bagName, key, packedValue, length, replicated)` | write a msgpack-encoded value |
| `EnsureEntityStateBag(entity)` | make sure an entity has a bag |

## When to use what

| Need | Use |
|---|---|
| Persistent fact about a player or entity, needed by many resources and late joiners | state bag |
| One-off notification ("show this message", "play this sound") | event |
| Large data, rarely needed | event or export on demand, not a state bag |
| Secret or authoritative server data | a server-side Lua table, or `bag:set(key, value, false)` |

## Sources

- State bags manual: https://docs.fivem.net/docs/scripting-manual/networking/state-bags/
- ADD_STATE_BAG_CHANGE_HANDLER: https://docs.fivem.net/natives/?_0x5BA35AAF
- Server commands (`sv_stateBagStrictMode`): https://docs.fivem.net/docs/server-manual/server-commands/
- State bag wrappers (`scheduler.lua`): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/lua/scheduler.lua
