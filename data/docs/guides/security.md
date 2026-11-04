# Security

The single rule behind every item in this guide: **the client is not trusted.** Client scripts are downloaded to players' machines, where they can be read, modified and replaced. Modified clients can trigger any network event with any arguments, call your NUI callbacks, write their own player state, and lie about positions and values. Everything that matters is decided and checked on the server.

## Server events are public endpoints

Every event registered with `RegisterNetEvent` on the server can be triggered by any connected client, at any rate, with any arguments.

### Validate everything

```lua
-- server
RegisterNetEvent('garage:rename', function(plate, newName)
    local src = source

    if type(plate) ~= 'string' or #plate > 8 then return end
    if type(newName) ~= 'string' or #newName == 0 or #newName > 32 then return end
    if newName:find('[^%w %-]') then return end -- letters, digits, spaces, dashes

    -- also check that `src` owns the vehicle with this plate (server-side data)
    print(('player %d renames %s to %s'):format(src, plate, newName))
end)
```

Check, in this order: types, sizes and ranges, ownership and permissions, game-state plausibility (distance, cooldown, current activity).

### Use `source`, never a client-provided id

```lua
-- BAD: the client chooses who gets paid
RegisterNetEvent('job:pay', function(playerId, amount) end)

-- GOOD: the server knows who is asking, and decides the amount
RegisterNetEvent('job:finishDelivery', function(deliveryId)
    local src = source
    -- look up the delivery assigned to `src`, verify it is complete, pay the server-side reward
end)
```

### Keep authority on the server

Design events as **requests**, not commands. The client says what it wants to do; the server checks whether it is allowed and computes the result.

| Avoid | Prefer |
|---|---|
| `addMoney(amount)` | `sellItem(itemName)` with the price looked up on the server |
| `setJob(job)` | `applyForJob(jobName)` checked against server rules |
| `spawnVehicle(model)` | `takeVehicleFromGarage(garageId, vehicleId)` checked against ownership |
| `teleport(x, y, z)` | `useElevator(elevatorId, floor)` with server-side coordinates |

### Rate-limit

```lua
-- server
local lastCall = {}

local function allow(src, key, intervalMs)
    local now = GetGameTimer()
    lastCall[src] = lastCall[src] or {}
    local previous = lastCall[src][key]
    if previous and now - previous < intervalMs then
        return false
    end
    lastCall[src][key] = now
    return true
end

RegisterNetEvent('shop:buy', function(item)
    local src = source
    if not allow(src, 'shop:buy', 500) then return end
    -- ...
end)

AddEventHandler('playerDropped', function()
    lastCall[source] = nil
end)
```

### Check plausibility

The server knows player positions (with OneSync):

```lua
local function isNear(src, point, maxDistance)
    local ped = GetPlayerPed(src)
    if ped == 0 then return false end
    return #(GetEntityCoords(ped) - point) <= maxDistance
end
```

## Permissions

- Check permissions on the server with ACE: `IsPlayerAceAllowed(source, 'myresource.admin')`. Client-side checks are only cosmetic (hiding a button).
- Register admin commands on the server with `restricted = true`; they then require `command.<name>`.
- Identify players with server-side identifiers (`GetPlayerIdentifierByType(source, 'license')`), never with names or data the client sends.

See `cfxdoc:commands-and-keymaps`.

## Secrets

- **Never put secrets in client or shared scripts**: API keys, webhook URLs, database credentials, admin passwords. Clients download those files.
- Store secrets in server convars and read them in server scripts with `GetConvar('my_api_key', '')`.
- Use `set` for secret convars. `setr` **replicates** the value to every client, and `sets` publishes it in the server info.

```
# server.cfg
set my_api_key "..."
```

```lua
-- server
local API_KEY <const> = GetConvar('my_api_key', '')
if API_KEY == '' then
    print('^1my_api_key is not configured^7')
end
```

- Make outgoing HTTP calls with credentials only from the server (`PerformHttpRequest` is server-only).

## Client-writable data

### State bags

By default a client can write its own player state and the state of entities it owns (when it sets them with `replicated = true`). Treat these values as client input:

- Read authoritative values (money, job, permissions) from your server-side data, not from `Player(src).state`.
- Set authoritative state from the server; consider `setr sv_stateBagStrictMode true` so that only the server can write state bags.

### NUI callbacks

NUI callback data comes from the page running on the player's machine. A callback that should change something on the server must trigger a server event, which then validates the request like any other.

### Entities

With OneSync, clients create and own many networked entities. The server can intercept them:

```lua
-- server: block a list of models from being created by clients
local BLOCKED_MODELS <const> = { [`rhino`] = true, [`hydra`] = true }

AddEventHandler('entityCreating', function(entity)
    if BLOCKED_MODELS[GetEntityModel(entity)] then
        CancelEvent() -- the entity is deleted
    end
end)
```

Several OneSync game events reported by clients can be inspected and canceled on the server so that they are not forwarded to other players: `explosionEvent`, `weaponDamageEvent`, `startProjectileEvent`, `clearPedTasksEvent` and, on FiveM, `giveWeaponEvent`, `removeWeaponEvent`, `removeAllWeaponsEvent`, `fireEvent`, `ptFxEvent`. The sender's id is passed as the first argument (a string):

```lua
-- server: block explosions near a safe zone
local SAFE_ZONE <const> = vector3(-543.0, -204.0, 38.0)

AddEventHandler('explosionEvent', function(sender, ev)
    local pos = vector3(ev.posX, ev.posY, ev.posZ)
    if #(pos - SAFE_ZONE) < 100.0 then
        print(('blocked explosion type %d from player %s'):format(ev.explosionType, sender))
        CancelEvent()
    end
end)
```

## Exports and local events

- Any resource on the same side can call your exports and trigger your local events. Use `GetInvokingResource()` to restrict sensitive exports to known resources (`cfxdoc:exports`).
- Local events triggered by `TriggerEvent` cannot come from the network, but a client-side local event can still be triggered by any client-side code, including injected code. Only server-side local events are trustworthy.

## Data handling

- Never build SQL queries by concatenating client input. Use the parameterized queries of your database library.
- Limit sizes: strings, arrays and nested tables from clients can be huge. Reject oversized payloads early.
- Log refused requests with the player id, so you can spot abuse.

## Checklist for every server net event

- [ ] `local src = source` on the first line.
- [ ] Argument types, lengths and ranges validated.
- [ ] Permission or ownership checked against server-side data.
- [ ] Plausibility checked (distance, cooldown, state).
- [ ] Rate-limited if it has a cost.
- [ ] No amount, price, id or permission taken from the client.

## Sources

- Server commands (ACL, convars, `sv_stateBagStrictMode`, `sv_filterRequestControl`): https://docs.fivem.net/docs/server-manual/server-commands/
- State bags (policy): https://docs.fivem.net/docs/scripting-manual/networking/state-bags/
- Intercepting game events with OneSync: https://docs.fivem.net/docs/cookbook/2019/08/19/onesync-intercepting-game-events-such-as-explosions/
- Server events reference: https://docs.fivem.net/docs/scripting-reference/events/server-events/
- Server game event handlers (`ServerGameState.cpp`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-server-impl/src/state/ServerGameState.cpp
