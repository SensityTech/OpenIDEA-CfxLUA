# Exports

An export is a function that a resource makes callable from other resources running on the **same side** (client exports for client scripts, server exports for server scripts). Exports are the usual way to build shared libraries and APIs between resources.

## Defining an export

Call the global `exports` with a name and a function:

```lua
-- resource "bank", server script
local balances = {}

exports('getBalance', function(playerId)
    return balances[playerId] or 0
end)

exports('addMoney', function(playerId, amount)
    if math.type(amount) ~= 'integer' or amount <= 0 then
        return false
    end
    balances[playerId] = (balances[playerId] or 0) + amount
    return true
end)
```

A client script defines client exports, a server script defines server exports. A shared script defines the export on both sides.

### Manifest exports (legacy)

The manifest can also export **global** functions by name. The function is looked up in the resource's global table when it is called:

```lua
-- fxmanifest.lua
fx_version 'cerulean'
game 'gta5'

client_script 'client.lua'
server_script 'server.lua'

export 'getWidget'          -- client export
server_export 'getStock'    -- server export
```

```lua
-- client.lua
local lastWidget

function getWidget()
    return lastWidget
end
```

Manifest exports become available only after the resource's first scheduler tick, and they require global functions. Prefer `exports('name', fn)`, which works with locals and is visible to the IDE.

## Calling an export

Use the resource name, then the export name, with a **colon**:

```lua
-- another resource, server script
local balance = exports.bank:getBalance(source)

-- resource names with dashes or other special characters need brackets
local ok = exports['my-bank']:addMoney(source, 100)
```

The colon matters. The function behind an export expects a dummy first argument (the `exports.bank` table). With a dot, your first real argument is swallowed:

```lua
exports.bank:getBalance(playerId)   -- correct
exports.bank.getBalance(playerId)   -- WRONG: playerId is dropped, the export receives nothing
exports.bank.getBalance(nil, playerId) -- works, but use the colon form
```

## Behaviour to know

- **The target resource must be started.** Otherwise the call raises `No such export <name> in resource <resource>`. Check with `GetResourceState`:

```lua
local function safeBalance(playerId)
    if GetResourceState('bank') ~= 'started' then
        return nil
    end
    return exports.bank:getBalance(playerId)
end
```

- **Errors propagate.** An error inside the export is re-raised in the caller, with a message naming the export and the resource. Use `pcall` when the other resource is optional:

```lua
local ok, result = pcall(function()
    return exports.bank:getBalance(1)
end)
if not ok then
    print(('bank unavailable: %s'):format(result))
end
```

- **Arguments and results are serialized** with MessagePack, like event arguments. Tables are copied, not shared: modifying a table returned by an export does not change the other resource's data. Functions become callable references. See `cfxdoc:json-and-msgpack`.
- **Multiple return values** are supported.
- **Exports can wait.** If the export calls `Wait` (or awaits something), the caller is suspended until it returns. The caller must then run inside a thread, an event handler or another export.
- **Same side only.** A server script cannot call a client export, and vice versa. Use events to cross sides.
- **Lookups are cached** per resource and export name, and the cache is cleared when that resource stops.
- **Exports cannot be assigned.** `exports.bank = {}` or `exports.bank.x = fn` raises an error. Define exports with `exports('name', fn)`.

## Knowing who called

Inside an export (or an event handler), `GetInvokingResource()` returns the name of the resource that invoked it, when there is one (it can be `nil`). Use it to restrict sensitive exports to known callers:

```lua
local TRUSTED <const> = { .admin, .jobs }

exports('setJob', function(playerId, job)
    local caller = GetInvokingResource()
    if not caller or not TRUSTED[caller] then
        print(('setJob refused for resource %s'):format(tostring(caller)))
        return false
    end
    -- apply the job
    return true
end)
```

## Typing exports for the IDE

Annotate the function you export, so that callers in the same project get completion and signature help from lua-language-server:

```lua
---@param playerId integer
---@return integer @balance in dollars
local function getBalance(playerId)
    return 0
end

exports('getBalance', getBalance)
```

## Performance

An export call serializes its arguments and results and goes through the runtime. It is fast enough for normal use, but avoid:

- calling exports every frame in a tight loop: cache the result and refresh it periodically or on an event;
- passing very large tables back and forth: send ids, keep the data on one side.

## Sources

- Lua runtime manual, "Using exports": https://docs.fivem.net/docs/scripting-manual/runtimes/lua/
- Resource manifest, `export` / `server_export`: https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
- GET_INVOKING_RESOURCE: https://docs.fivem.net/natives/?_0x4D52FE5B
- Exports implementation (`scheduler.lua`): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/lua/scheduler.lua
