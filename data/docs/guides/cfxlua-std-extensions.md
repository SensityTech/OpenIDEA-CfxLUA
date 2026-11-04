# CfxLua standard library extensions

On top of the Lua 5.4 standard library, CfxLua adds a few global functions and extends `table`, `string`, `math` and `utf8`. The runtime also removes or restricts some standard functions. This guide lists the additions that exist in the FiveM/RedM runtime, with their exact behaviour.

## Global functions

### joaat(str [, keepCase])

Computes the Jenkins one-at-a-time hash of a string at runtime, the same hash as backtick literals and `GetHashKey`, returned as a **signed 32-bit integer**.

```lua
local model = 'adder'
print(joaat(model) == `adder`)    -- true
print(joaat('ADDER') == `adder`)  -- true: ASCII is lowercased by default
print(joaat('Adder', true) == `adder`) -- false: true means "hash the bytes as they are"
print(joaat(model) & 0xFFFFFFFF)  -- unsigned view: 3078201489
```

Prefer a backtick literal when the string is a constant: it is computed at compile time. Use `joaat` for strings known only at runtime; it does not cross into the game engine like `GetHashKey` does.

### defer(fn)

Returns a value that calls `fn` when it is closed. Combined with a `<close>` local, it runs cleanup code when the block exits, normally or through an error:

```lua
local function withCamera(fn)
    local cam = CreateCam('DEFAULT_SCRIPTED_CAMERA', true)
    local _ <close> = defer(function()
        DestroyCam(cam, false)
    end)
    return fn(cam) -- the camera is destroyed after fn returns or raises
end
```

There is **no** `defer ... end` statement in CfxLua; only this function.

### each(t)

An iterator that respects a custom `__iter` metamethod and otherwise behaves like `pairs`:

```lua
for key, value in each({ a = 1, b = 2 }) do
    print(key, value)
end

local Range = {}
Range.__iter = function(self)
    local i = self.from - 1
    return function()
        i += 1
        if i <= self.to then
            return i
        end
    end
end

for n in each(setmetatable({ from = 1, to = 3 }, Range)) do
    print(n) -- 1 2 3
end
```

### scrub(...)

Returns its arguments, with every value that is not a number, boolean or string replaced by `nil`:

```lua
print(scrub(1, 'two', {}, true, print)) -- 1  two  nil  true  nil
```

### ipairs and `__ipairs`

`ipairs` honours an `__ipairs` metamethod (removed from standard Lua 5.4, restored in CfxLua).

## table

| Function | Description |
|---|---|
| `table.create(narray, nhash)` / `table.new` | New empty table with preallocated space for `narray` array items and `nhash` hash items. Both arguments are required. |
| `table.wipe(t)` / `table.clear` | Removes every entry of `t` and returns `t` (same table object). |
| `table.clone(t [, dest])` | Shallow copy of `t` into a new table (or into `dest`). Nested tables are shared, not copied. |
| `table.type(t)` | `'empty'`, `'array'`, `'hash'` or `'mixed'`, based on the table's internal layout. |

```lua
local cache = table.create(64, 0)
for i = 1, 64 do
    cache[i] = i * i
end

local copy = table.clone(cache)
copy[1] = -1
print(cache[1], copy[1])     -- 1  -1

print(table.type({}))             -- empty
print(table.type({ 1, 2, 3 }))    -- array
print(table.type({ a = 1 }))      -- hash
print(table.type({ 1, a = 1 }))   -- mixed

table.wipe(cache)
print(next(cache))           -- nil
```

`table.type` reflects how the table is stored, not just its content: a table that was once mixed may stay `'mixed'` after you remove keys. Use it as a hint (for example before serialization), not as a strict validator.

## string

| Function | Description |
|---|---|
| `string.strsplit(delimiters, str [, limit])` | Splits `str` and returns the pieces as **multiple values**. Every character of `delimiters` is a separator. `limit` caps the number of pieces. |
| `string.strjoin(delimiter, ...)` | Joins the strings (or numbers) with `delimiter`. |
| `string.strtrim(str [, chars])` | Removes leading and trailing characters found in `chars` (default: whitespace). |
| `string.strconcat(...)` | Concatenates all arguments. |
| `string.tostringall(...)` | Returns every argument converted with `tostring`. |

```lua
local a, b, c = string.strsplit(',', 'red,green,blue')
print(a, b, c)                               -- red  green  blue

local parts = { string.strsplit(' ', 'a b  c') }
print(#parts)                                -- 4: consecutive separators give empty strings

local key, rest = string.strsplit('=', 'name=value=x', 2)
print(key, rest)                             -- name  value=x

print(string.strjoin(', ', 'a', 'b', 3))     -- a, b, 3
print(string.strtrim('  padded  '))          -- padded
print(string.strtrim('--title--', '-'))      -- title
```

The delimiter comes **first**. Method syntax therefore does not work as you might expect: `s:strsplit(',')` passes `s` as the delimiter. Always write `string.strsplit(',', s)`.

Binary blobs are also available (`string.blob`, `string.isblob`, `string.blob_pack`, `string.blob_unpack`): a string variant stored by reference, that `string.blob_pack` can write into using `string.pack` formats. Most scripts never need them.

## math

Lua 5.3 compatibility functions and C99 functions are present in addition to the 5.4 library:

| Group | Functions |
|---|---|
| Lua 5.3 compatibility | `math.pow`, `math.atan2`, `math.cosh`, `math.sinh`, `math.tanh`, `math.frexp`, `math.ldexp`, `math.log10` |
| C99 | `math.round`, `math.trunc`, `math.hypot`, `math.cbrt`, `math.isnan`, `math.isinf`, `math.isfinite`, `math.copysign`, `math.exp2`, `math.log1p`, ... |
| Extra | `math.clamp(x, min, max)` |

```lua
print(math.round(2.5), math.round(-2.5)) -- 3.0  -3.0 (halfway rounds away from zero)
print(math.trunc(-2.7))                  -- -2.0
local limited = math.clamp(15, 0, 10)    -- 10
print(math.hypot(3, 4))                  -- 5.0
```

`math.round` and `math.trunc` return floats; wrap them in `math.tointeger` when you need an integer.

## utf8

| Function | Description |
|---|---|
| `utf8.strlenutf8(s)` | Alias of `utf8.len`. |
| `utf8.strcmputf8i(a, b)` | Case-insensitive comparison accounting for UTF-8 characters: negative, zero or positive. |

## What the runtime removes or changes

| Item | Status |
|---|---|
| `dofile`, `loadfile` | Removed (`nil`). Read files with `LoadResourceFile` and compile with `load` if you really need to. |
| `require` | Restricted: returns built-in libraries (`require 'json'`) and special modules such as `glm`. It cannot load your own Lua files; list them in the manifest instead. |
| `package` | Not available. |
| `io`, `os` | **Server only.** On the client they are `nil`: `os.time()` and `os.date()` fail in client scripts. |
| `debug` | Sandboxed: only `getinfo`, `getmetatable`, `setmetatable`, `getupvalue` and `traceback`. |
| `print` | Prints to the console with the resource name as prefix. |
| `warn` | Prints a warning to the console. |

On the client, use natives for time:

```lua
local now = GetCloudTimeAsInt()   -- client: Unix time from the game
local uptime = GetGameTimer()     -- client or server: milliseconds since start
```

On the server, `os.time()`, `os.date()` and `os.clock()` work, and the server `os` library adds high resolution timers such as `os.microtime()` and `os.nanotime()`.

## Sources

- CfxLua (LuaGLM) README, "Power Patches" and "Extended API": https://github.com/citizenfx/lua/blob/luaglm-548/README.md
- Lua runtime bootstrap (libraries, `require`, `print`): https://github.com/citizenfx/fivem/blob/master/code/components/citizen-scripting-lua/src/LuaScriptRuntime.cpp
- Build configuration (enabled extensions): https://github.com/citizenfx/fivem/blob/master/code/vendor/lua.lua
- Lua 5.4 reference manual: https://www.lua.org/manual/5.4/manual.html
