# Events

Events are how resources talk to each other and how clients talk to the server. An event has a name and a list of arguments; any number of handlers, in any resource, can listen to it.

## Local and network events

| Kind | Trigger | Received by |
|---|---|---|
| Local | `TriggerEvent(name, ...)` | Handlers on the **same machine** (same client, or the server), in every resource |
| Client to server | `TriggerServerEvent(name, ...)` (client) | Server handlers registered with `RegisterNetEvent` |
| Server to client | `TriggerClientEvent(name, target, ...)` (server) | Client handlers registered with `RegisterNetEvent` on the target(s) |
| Latent (large payloads) | `TriggerLatentServerEvent(name, bps, ...)`, `TriggerLatentClientEvent(name, target, bps, ...)` | Same as above, sent progressively |

There is no direct client-to-client event: relay through the server.

## Listening

```lua
-- a local event: any resource on this machine can trigger it
AddEventHandler('weather:changed', function(weatherType, transition)
    print(('weather is now %s'):format(weatherType))
end)

-- a network event: must be registered as safe for the network
RegisterNetEvent('weather:sync', function(weatherType)
    SetWeatherTypeNowPersist(weatherType)
end)
```

- `AddEventHandler(name, fn)` adds a handler and returns a handle table (`{ key, name }`).
- `RemoveEventHandler(handle)` removes it.
- `RegisterNetEvent(name)` marks the event as allowed to arrive from the network. Passing a function as second argument also adds it as a handler (and returns the handle). Without this registration, an event coming from the other side is dropped and the console prints `event <name> was not safe for net`.
- On the server, `RegisterServerEvent` is an older alias of `RegisterNetEvent`.

A handler can be removed later:

```lua
local handle = AddEventHandler('demo:once', function()
    print('only reacts until removed')
end)

RemoveEventHandler(handle)
```

Each handler call runs in its own scheduler coroutine, so `Wait` is allowed inside handlers (see `cfxdoc:threads-and-timing`).

## Triggering

```lua
-- local
TriggerEvent('weather:changed', 'RAIN', 15.0)

-- client -> server
TriggerServerEvent('bank:deposit', 500)

-- server -> one client
TriggerClientEvent('weather:sync', playerId, 'RAIN')

-- server -> every client
TriggerClientEvent('weather:sync', -1, 'RAIN')
```

The target of `TriggerClientEvent` is a player's server id (number or string), or `-1` for all connected players. To reach a subset, loop over it:

```lua
local function notifyMany(playerIds, message)
    for _, id in ipairs(playerIds) do
        TriggerClientEvent('hud:notify', id, message)
    end
end
```

### Latent events

Regular network events are sent in one piece on the reliable channel. A big payload (several kilobytes and more) blocks other traffic and can make a client time out. Latent events are sent in chunks at a given rate in bytes per second; `0` or `-1` selects the default rate.

```lua
-- server: send a large table to one player at 50 KB/s
TriggerLatentClientEvent('mdt:records', playerId, 50000, records)
```

With target `-1`, the rate applies to each player, so the server sends `bps * players` bytes per second in total.

## Arguments and serialization

Arguments are serialized with MessagePack, even for local events:

- numbers, strings, booleans, tables, vectors and quaternions arrive intact;
- tables are **copied**: the handler gets a new table, changes do not affect the sender;
- functions are turned into callable function references;
- a `nil` argument followed by other arguments is not reliably delivered: the arguments after it can be lost. Pass `false`, or group optional values in a table;
- avoid tables with `nil` holes (sparse arrays) and tables that mix array and hash keys: send either a list or a keyed map;
- entity handles are plain integers and mean nothing on another machine: send network IDs (see `cfxdoc:natives`).

## `source`

During a handler, the global `source` tells you who triggered the event:

| Situation | `source` |
|---|---|
| Server handler of an event from a client | the player's server id (number) |
| Handler of a local event | not a player id; do not use it |
| Built-in player events (`playerConnecting`, `playerJoining`, `playerDropped`) | the player concerned (a temporary id during `playerConnecting`) |

`source` is a global that is overwritten for every event dispatched. **Copy it into a local first**, before any `Wait`, callback or function call that could run another event:

```lua
RegisterNetEvent('bank:deposit', function(amount)
    local src = source -- always the first line
    Wait(100)          -- 'source' may already belong to another event here
    print(('player %d deposits %s'):format(src, tostring(amount)))
end)
```

On the client, `source` does not identify a player and should not be used.

## Canceling events

`CancelEvent()` marks the current event as canceled. It does **not** stop other handlers from running; it only tells the code that triggered the event:

```lua
AddEventHandler('inventory:beforeUse', function(item)
    if item == 'contraband' then
        CancelEvent()
    end
end)

CreateThread(function()
    TriggerEvent('inventory:beforeUse', 'contraband')
    if WasEventCanceled() then
        print('use was blocked by another resource')
    end
end)
```

- `WasEventCanceled()` reports whether the last local event you triggered was canceled. Network events cannot be canceled by the receiver in a way the sender sees.
- Several built-in events act on cancellation: `playerConnecting` refuses the player, `onResourceStarting` keeps the resource stopped, `entityCreating` deletes the entity, OneSync game events such as `explosionEvent` or `weaponDamageEvent` are not forwarded to other players. The catalog in the **Events** tab says which built-in events are cancelable.

## Built-in events

The runtime raises many events by itself. The most used ones:

| Event | Side | Arguments |
|---|---|---|
| `onResourceStart` | both | `resourceName` |
| `onResourceStop` | both | `resourceName` |
| `onClientResourceStart` | client | `resourceName` |
| `onServerResourceStart` | server | `resourceName` |
| `playerConnecting` | server | `name, setKickReason, deferrals` |
| `playerJoining` | server | `oldID` (the temporary id; `source` is the final id) |
| `playerDropped` | server | `reason, resourceName, clientDropReason` |
| `entityCreated`, `entityRemoved` | server | `entity` |
| `gameEventTriggered` | client (FiveM) | `name, args` |

Resource events fire for **every** resource, so filter on the name:

```lua
AddEventHandler('onResourceStart', function(resourceName)
    if resourceName ~= GetCurrentResourceName() then return end
    print('this resource just started')
end)
```

A connection check with deferrals (server):

```lua
AddEventHandler('playerConnecting', function(name, setKickReason, deferrals)
    local src = source
    deferrals.defer()
    Wait(0) -- required before update/done

    deferrals.update(('Checking %s...'):format(name))
    local license
    for _, identifier in ipairs(GetPlayerIdentifiers(src)) do
        if identifier:sub(1, 8) == 'license:' then
            license = identifier
            break
        end
    end
    Wait(0)

    if not license then
        deferrals.done('No license identifier found.')
    else
        deferrals.done()
    end
end)
```

The complete list, with parameters, sides, games and cancelability, is in the **Events** tab of the CfxLua tool window.

## Event security

Any client can trigger any network event registered on the server, with any arguments: modified clients and executors do this routinely. Treat every server-side net event as a public API endpoint.

1. **Never trust the payload.** Check types and ranges, reject anything unexpected.
2. **Never trust the client's identity claims.** Use `source`, never a player id passed as an argument.
3. **Decide on the server.** The client asks ("I want to buy item X"); the server checks price, money, distance and stock, then acts. Never expose events like `giveMoney(amount)` or `setJob(job)`.
4. **Rate-limit** events that cost something.
5. **Check permissions on the server** with ACE (`IsPlayerAceAllowed`) for admin actions.

```lua
-- server
local PRICES <const> = { water = 5, bread = 8 }
local SHOP_POS <const> = vector3(25.7, -1347.3, 29.5)
local lastBuy = {}

RegisterNetEvent('shop:buy', function(itemName, quantity)
    local src = source

    if type(itemName) ~= 'string' or math.type(quantity) ~= 'integer' then return end
    if quantity < 1 or quantity > 10 then return end

    local price = PRICES[itemName]
    if not price then return end

    local now = GetGameTimer()
    if lastBuy[src] and now - lastBuy[src] < 1000 then return end
    lastBuy[src] = now

    local ped = GetPlayerPed(src)
    if #(GetEntityCoords(ped) - SHOP_POS) > 5.0 then return end

    -- charge price * quantity and give the item using your own server-side logic
    print(('player %d buys %d x %s for $%d'):format(src, quantity, itemName, price * quantity))
end)

AddEventHandler('playerDropped', function()
    lastBuy[source] = nil
end)
```

More in `cfxdoc:security`.

## A request/response pattern

Events are one-way. For a request that expects an answer, pair two events with a request id and a promise:

```lua
-- client
local pending = {}
local nextId = 0

RegisterNetEvent('rpc:response', function(requestId, ...)
    local p = pending[requestId]
    if p then
        pending[requestId] = nil
        p:resolve(table.pack(...))
    end
end)

local function callServer(name, ...)
    nextId += 1
    local p = promise.new()
    pending[nextId] = p
    TriggerServerEvent('rpc:request', nextId, name, ...)
    local result = Citizen.Await(p)
    return table.unpack(result, 1, result.n)
end

CreateThread(function()
    local balance = callServer('getBalance')
    print(('balance: %s'):format(tostring(balance)))
end)
```

```lua
-- server
local handlers = {
    getBalance = function(src)
        return 1000
    end,
}

RegisterNetEvent('rpc:request', function(requestId, name, ...)
    local src = source
    local handler = type(name) == 'string' and handlers[name]
    if not handler or math.type(requestId) ~= 'integer' then return end
    TriggerClientEvent('rpc:response', src, requestId, handler(src, ...))
end)
```

In real code, add a timeout so a lost response does not block the caller forever (see "Awaiting with a timeout" in `cfxdoc:threads-and-timing`).

## Naming conventions

- Prefix event names with your resource name: `myresource:action`. This avoids collisions between resources.
- Do not reuse the `__cfx_` prefix: it is reserved for internal events.
- Keep client-to-server events few and specific; they are your attack surface.

## Sources

- Listening for events: https://docs.fivem.net/docs/scripting-manual/working-with-events/listening-for-events/
- Triggering events: https://docs.fivem.net/docs/scripting-manual/working-with-events/triggering-events/
- Canceling events: https://docs.fivem.net/docs/scripting-manual/working-with-events/canceling-events/
- Server events reference: https://docs.fivem.net/docs/scripting-reference/events/server-events/
- Client events reference: https://docs.fivem.net/docs/scripting-reference/events/client-events/
- Scheduler source (event dispatch): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/lua/scheduler.lua
