# JSON and MessagePack

Two serialization libraries are always available as globals, on the client and on the server:

| Global | Format | Typical use |
|---|---|---|
| `json` | JSON text | Config files, HTTP APIs, NUI messages, databases, KVP storage |
| `msgpack` | MessagePack binary | Used by the runtime itself for events, exports and state bags |

Both are fast C implementations. The official docs still mention the pure-Lua libraries dkjson and lua-MessagePack; the runtime actually ships C rewrites with compatible APIs.

## json

### Encoding

```lua
local data = {
    name = 'Garage A',
    slots = 12,
    open = true,
    vehicles = { 'adder', 'zentorno' },
}

local text = json.encode(data)
print(text) -- {"name":"Garage A","slots":12,"open":true,"vehicles":["adder","zentorno"]} (key order may vary)

local pretty = json.encode(data, { indent = true, sort_keys = true })
```

Rules to keep in mind:

- A table whose keys are exactly `1..n` becomes a JSON array; a table with string keys becomes an object. Keep tables pure: either a list or a map.
- **Empty tables encode as `[]`.** The runtime configures the library this way. If you need `{}`, mark the table as an object:

```lua
local empty = setmetatable({}, { __jsontype = 'object' })
print(json.encode({ list = {}, map = empty })) -- {"list":[],"map":{}} (key order may vary)
```

  `json.object()` and `json.array()` return empty tables already marked as object or array.
- A `nil` field is simply absent from the output. Use `json.null` to write an explicit `null`.
- Use string keys for objects. Numeric map keys are not portable across JSON consumers.
- Vectors are not JSON values. Convert them first:

```lua
local function vecToTable(v)
    return { x = v.x, y = v.y, z = v.z }
end

local function tableToVec(t)
    return vector3(t.x, t.y, t.z)
end
```

### Decoding

```lua
local text = '{"name":"Garage A","slots":12}'
local data = json.decode(text)
print(data.name, data.slots) -- Garage A 12
```

`json.decode` does **not** raise on invalid input: it returns `nil` followed by error information. Always check the result:

```lua
local function decodeOrNil(text)
    if type(text) ~= 'string' or text == '' then
        return nil
    end
    local value = json.decode(text)
    if value == nil then
        print('invalid JSON')
    end
    return value
end
```

### Reading and writing resource files

```lua
-- shared or client: read a JSON file shipped with the resource
local raw = LoadResourceFile(GetCurrentResourceName(), 'data/items.json')
local items = raw and json.decode(raw) or {}
```

```lua
-- server only: write a JSON file into the resource folder
local function saveJson(fileName, value)
    local text = json.encode(value, { indent = true })
    return SaveResourceFile(GetCurrentResourceName(), fileName, text, -1) -- -1: use the string length
end
```

On the client, `LoadResourceFile` can only read files the client downloaded, so list them in `files` in the manifest. Writing files is server-only.

### Key-value storage

Resource KVP stores strings persistently (on the server, or locally on the player's machine for client scripts). JSON turns tables into strings:

```lua
local function saveSettings(settings)
    SetResourceKvp('settings', json.encode(settings))
end

local function loadSettings()
    local raw = GetResourceKvpString('settings')
    return raw and json.decode(raw) or {}
end
```

### NUI

`SendNUIMessage(tbl)` encodes the table with `json` for you, and NUI callbacks receive the page's JSON body already decoded. See `cfxdoc:nui`.

## msgpack

```lua
local packed = msgpack.pack({ id = 7, tags = { 'a', 'b' } })
print(type(packed), #packed) -- string, a few bytes

local value = msgpack.unpack(packed)
print(value.id, value.tags[2]) -- 7 b

local multi = msgpack.pack_args(1, 'two', true) -- pack several values as one list
```

You rarely call `msgpack` yourself, but it decides what survives when data crosses a boundary:

| Boundary | Serialization |
|---|---|
| Event arguments (local and network) | msgpack |
| Export arguments and results | msgpack |
| State bag values | msgpack |
| `SendNUIMessage`, HTTP bodies, files | json (done by you or by the helper) |

What this means in practice:

- **Tables are copied.** The receiver gets a new table; changes are not shared.
- **Empty tables arrive as empty arrays** (the runtime packs them that way).
- **Sparse tables** (arrays with `nil` holes) are packed as maps: values keep their keys, but do not rely on `#` or `ipairs` on the result.
- **Vectors and quaternions** have dedicated MessagePack types and arrive as vectors.
- **Functions** are packed as function references: the receiver gets a callable object that calls back into the original resource.
- **Metatables are lost.** Objects with methods arrive as plain tables; send data, not objects.
- **Entity handles** are plain numbers and only make sense on the machine that produced them. Send network IDs instead (see `cfxdoc:natives`).

## Choosing between them

| Need | Use |
|---|---|
| Human-readable, external systems (web APIs, files, databases, NUI) | `json` |
| Data passed between resources or over the network | let the runtime use msgpack: just pass Lua values |
| Storing a table in a string (KVP, database column) | `json` (portable, debuggable) |

## Sources

- Lua runtime manual (bundled libraries): https://docs.fivem.net/docs/scripting-manual/runtimes/lua/
- lua-rapidjson (Cfx fork used by the runtime): https://github.com/citizenfx/lua-rapidjson
- lua-cmsgpack (Cfx fork used by the runtime): https://github.com/citizenfx/lua-cmsgpack
- Runtime configuration of both libraries (`scheduler.lua`): https://github.com/citizenfx/fivem/blob/master/data/shared/citizen/scripting/lua/scheduler.lua
- LOAD_RESOURCE_FILE: https://docs.fivem.net/natives/?_0x76A9EE1F
