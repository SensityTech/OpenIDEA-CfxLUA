# CfxLua syntax

FiveM and RedM run **CfxLua**, a fork of Lua 5.4 maintained by Cfx.re (`citizenfx/lua`, based on LuaGLM). It is standard Lua 5.4 plus a small set of syntax extensions and first-class vector types. Client and server, FiveM and RedM all use the same grammar.

This guide lists every extension, what it does exactly, and the traps around it. It also lists popular syntax from other languages that does **not** exist in CfxLua.

## Summary

| Feature | Example | Status |
|---|---|---|
| Compound assignment | `x += 1`, `mask &= ~4` | Supported: `+= -= *= /= <<= >>= &= \|= ^=` only |
| Safe navigation | `a?.b?.c`, `obj?:method()` | Supported |
| `in` unpacking | `local x, y in coords` | Supported |
| Set constructors | `{ .admin, .moderator }` | Supported |
| C-style block comments | `/* ... */` | Supported (with a line-number caveat) |
| Compile-time hashes | `` `adder` `` | Supported |
| Vectors and quaternions | `vector3(1, 2, 3)` | Supported (runtime types, see `cfxdoc:vectors-and-math`) |
| `++`, `--` operators | `i++` | **Not supported** (`--` is a comment) |
| `//=`, `%=`, `..=` | `s ..= 'x'` | **Not supported** |
| `!=`, `!`, `??`, ternary `a ? b : c` | | **Not supported** |
| `continue` | | **Not supported** (use `goto`) |
| `defer ... end` statement | | **Not supported** (use `defer(fn)` with `<close>`) |
| `//` line comment | `// note` | **Not supported** (`//` is integer division) |

## Compound assignment

```lua
local count = 10
count += 5        -- 15
count -= 1        -- 14
count *= 2        -- 28
count /= 4        -- 7.0 (/ is always float division)

local flags = 0
flags |= 1 << 3   -- set bit 3
flags &= ~(1 << 3) -- clear bit 3
flags <<= 1
flags >>= 1
flags ^= 0xFF     -- XOR, not power
```

`v op= expr` behaves like `v = v op (expr)`. The right-hand side is a complete expression, so it is evaluated first as a whole:

```lua
local a = 2
a *= 2 + 1   -- a = a * (2 + 1) = 6, not a * 2 + 1
```

Targets can be locals, upvalues, globals, fields and indexed elements. For an indexed target, the table and the key are evaluated only once:

```lua
local stats = { kills = 0 }
stats.kills += 1

local calls = 0
local function key()
    calls += 1
    return 'kills'
end
stats[key()] += 1 -- key() runs once
```

Exactly nine operators exist: `+= -= *= /= <<= >>= &= |= ^=`.

### Pitfalls

- **`^=` is bitwise XOR.** `^` alone is exponentiation, but `x ^= 2` computes `x ~ 2`. With `x = 3` the result is `1`, not `9`. Write `x = x ^ 2` for a power.
- **No `//=`, `%=` or `..=`.** Write `s = s .. 'x'`, `n = n // 2`, `n = n % 3`.
- **Single target only.** `a, b += 1, 1` is a syntax error.
- **It is a statement, not an expression.** `print(a += 1)` and `if (a += 1) then` do not compile.
- **The operator must be written without spaces.** `a + = 1` is a syntax error.
- **`<const>` is not protected.** The runtime does not reject `c += 1` on a `<const>` local: it is either a silent no-op or it really modifies the value. Never do it; OpenIDEA-CfxLUA reports it as an error.

## Safe navigation `?`

Put `?` in front of any suffix of an expression: a field (`?.name`), an index (`?[key]`), a method call (`?:name(...)`), or a call (`?(...)`, `?"str"`, `?{...}`).

```lua
local config = { vehicle = { plate = 'CFX 001' } }

local plate = config?.vehicle?.plate      -- 'CFX 001'
local color = config?.paint?.primary      -- nil, no error
local first = config?.list?[1]            -- nil
local upper = config.vehicle.plate?:upper() -- 'CFX 001'
local onDone = config.onDone
onDone?('finished')                        -- not called because onDone is nil
```

### Exact semantics

- When the value before `?` is **`nil` or `false`**, evaluation jumps to the end of the **whole** chain, and the chain's value is that `nil` or `false`. Any other value continues normally.
- Only `nil` and `false` short-circuit. `local n = 0; n?.x` still raises "attempt to index a number value".
- The `?` covers the rest of the chain: in `a?.b.c`, if `a` is `nil` the result is `nil`; if `a` exists but `a.b` is `nil`, indexing `.c` still raises an error. Add a `?` at each level that may be missing: `a?.b?.c`.
- Inside a larger expression, the whole chain is one operand: `a?.b or 10` gives `10` when `a` is `nil`; `not t?.x` and `#t?.list` work as expected.

### In statements

A safe-navigation chain can start a statement. If it short-circuits, the **entire statement** is skipped, including the evaluation of the right-hand side:

```lua
local player = nil
player?.stats.kills = 10        -- skipped, no error
player?:notify('hello')         -- skipped
player?.stats.kills += 1        -- skipped
```

In a multiple assignment, only the first target may contain `?`: `x?.a, y = 1, 2` compiles but `y, x?.a = 1, 2` does not.

### Not supported

There is no `??` (null coalescing) and no ternary `a ? b : c`. Use the usual Lua idioms:

```lua
local value = nil
local name = value or 'default'
local label = (value ~= nil) and 'set' or 'unset'
```

Beware that `cond and a or b` returns `b` when `a` is `false` or `nil`.

## `in` unpacking

`local a, b, c in t` declares locals initialised from the fields of the same name:

```lua
local coords = { x = 215.7, y = -810.1, z = 30.7 }
local x, y, z in coords        -- same as: local x, y, z = coords.x, coords.y, coords.z
```

The source expression is evaluated **once**, which is handy with function calls:

```lua
local function getSettings()
    return { volume = 0.5, language = 'en' }
end

local volume, language in getSettings()
```

Attributes are allowed: `local a <const>, b in t` keeps `a` constant.

The non-local form assigns existing variables or fields. The key used for each target is its name: a local, upvalue or global name; the field name for `obj.name`; the integer for `obj[1]`.

```lua
local data = { health = 200, armour = 50 }
local health, armour = 0, 0
health, armour in data         -- health = data.health, armour = data.armour
```

### Pitfalls

- **The source must be a name, an index, a call or a parenthesised expression.** A literal table needs parentheses: `local a in { a = 1 }` is a syntax error, `local a in ({ a = 1 })` works.
- **Only one source.** `local a, b in t1, t2` does not compile.
- **`<close>` is silently ignored with `in`.** `local f <close> in t` never calls `__close`. OpenIDEA-CfxLUA reports this.
- **It reads fields by name, not by position.** `local a, b in { 10, 20 }` gives `nil, nil`. Use `table.unpack` for arrays.
- Not to be confused with `for k, v in pairs(t) do`, which is the regular generic `for`.

## Set constructors

Inside a table constructor, `.name` without a value means `name = true`, and `[expr]` without a value means `[expr] = true`:

```lua
local allowedJobs = { .police, .ambulance, ['tow-truck'] }
-- same as { police = true, ambulance = true, ['tow-truck'] = true }

if allowedJobs['police'] then
    print('police may use this')
end
```

You can mix forms, and `.name = value` is also accepted as an alternative spelling of `name = value`:

```lua
local perms = { .read, .write = false, admin = true, 'positional' }
```

### Pitfalls

- `{ .5 }` is the number `0.5` in an array, not a set entry: a dot followed by a digit is a number.
- Keywords cannot be used: `{ .end }` is a syntax error. Use `{ ['end'] }`.
- Only a single name is allowed: `{ .a.b }` is a syntax error.

## C-style block comments

```lua
local speed = 10 /* m/s */ * 3.6
/*
    Multi-line comments work too.
*/
```

- They do **not** nest: in `/* a /* b */ c */`, the comment ends at the first `*/` and `c */` is parsed as code.
- An unterminated `/*` is an error ("unfinished long comment").
- **Line numbers drift.** The runtime does not count newlines inside `/* */`, so every error message and stack trace after a multi-line C comment reports a line number that is too small. Prefer `--[[ ]]` for multi-line comments.
- `//` is **not** a comment, it is integer division. `local x = 10 // comment` divides `10` by a global named `comment`.

## Compile-time hashes (backticks)

A string between backticks is replaced at compile time by its Jenkins one-at-a-time hash (the hash used by `GetHashKey`):

```lua
local ADDER <const> = `adder`          -- -1216765807
print(`adder` == `ADDER`)              -- true, ASCII is lowercased first
print(`adder` & 0xFFFFFFFF)            -- 3078201489, unsigned view
```

- The result is a **signed 32-bit integer**, the same representation natives return for hashes, so `` GetEntityModel(veh) == `adder` `` works.
- It costs nothing at runtime (it is a constant) and works with `<const>`.
- A backtick literal can be passed as a call argument without parentheses, like a string: ``RequestModel`adder` ``. Writing `` RequestModel(`adder`) `` is clearer.
- Escapes work as in strings; the literal must stay on one line.
- Use `joaat(str)` (or `GetHashKey(str)`) when the string is only known at runtime. See `cfxdoc:cfxlua-std-extensions`.

## Increment and decrement: not supported

There is no `++` or `--` operator, and there will not be: `--` is the comment marker.

```lua
local i = 0
i += 1      -- increment
i -= 1      -- decrement
```

The dangerous case is `i--`: it compiles as `i` followed by a comment, so the rest of the line disappears silently. `local x = i-- 1` sets `x` to `i` and ignores ` 1`. `a-=1` is fine (compound subtraction), but `a--=1` is a comment.

## Lua 5.4 features

CfxLua is Lua 5.4 (5.4.8 at the time of writing; `_VERSION` reports `LuaGLM 5.4`). Lua 5.3 support was removed in June 2025, so every script runs on 5.4. The manifest flag `lua54 'yes'` is deprecated and has no effect.

### Integers and floats

Numbers have two subtypes. Integer arithmetic stays integer, `/` always produces a float, `//` is floor division:

```lua
print(math.type(1), math.type(1.0))   -- integer  float
print(7 / 2, 7 // 2, 7 // 2.0)        -- 3.5  3  3.0
print(-7 // 2, 7 % -3)                -- -4  -2
print(math.maxinteger + 1 == math.mininteger) -- true: integers wrap around
print(10 == 10.0)                     -- true
print(math.tointeger(3.0))            -- 3
```

### Bitwise operators

`&` (and), `|` (or), `~` (binary xor), `~x` (unary not), `<<`, `>>` work on integers:

```lua
local FLAG_A <const> = 1 << 0
local FLAG_B <const> = 1 << 1
local state = FLAG_A | FLAG_B
local hasB = (state & FLAG_B) ~= 0
state = state & ~FLAG_A
```

`~=` is still "not equal"; binary `~` between two values is XOR.

### `<const>` and `<close>`

```lua
local MAX_PLAYERS <const> = 48

do
    local resource <close> = setmetatable({}, {
        __close = function()
            print('released')
        end,
    })
    -- work with the resource
end -- prints 'released', even if an error was raised inside the block
```

A `<close>` value must have a `__close` metamethod (or be `nil`/`false`). The CfxLua helper `defer(fn)` creates such a value from a function:

```lua
local function withLock(lock, fn)
    lock.busy = true
    local _ <close> = defer(function()
        lock.busy = false
    end)
    return fn()
end
```

### `goto` and labels

Lua has no `continue`. Use a label at the end of the loop body:

```lua
for _, item in ipairs({ 1, 2, 3, 4 }) do
    if item % 2 == 0 then
        goto continue
    end
    print(item)
    ::continue::
end
```

## Other syntax that does not exist

| You may write | Use instead |
|---|---|
| `a != b` | `a ~= b` |
| `!flag` | `not flag` |
| `a ?? b` | `a == nil and b or a` (or `a or b` when `false` is not a valid value) |
| `cond ? a : b` | `cond and a or b` |
| `x => x * 2`, `\|x\| x * 2` | `function(x) return x * 2 end` |
| `continue` | `goto continue` + `::continue::` |
| `defer ... end` | `local _ <close> = defer(function() ... end)` |
| `t::field` | `t.field` |
| `10LL`, `1_000` | `10`, `1000` |

## See also

- `cfxdoc:cfxlua-std-extensions`: `joaat`, `defer`, `each`, `table.*` and `string.*` additions
- `cfxdoc:vectors-and-math`: vector and quaternion types
- `cfxdoc:plugin-guide`: the inspections built on these rules

## Sources

- CfxLua runtime source (README, "Power Patches"): https://github.com/citizenfx/lua/tree/luaglm-548
- Cfx.re Lua runtime manual: https://docs.fivem.net/docs/scripting-manual/runtimes/lua/
- Removal of Lua 5.3 support (forum announcement): https://forum.cfx.re/t/removal-of-lua-5-3-support/5335232
- Lua 5.4 reference manual: https://www.lua.org/manual/5.4/manual.html
