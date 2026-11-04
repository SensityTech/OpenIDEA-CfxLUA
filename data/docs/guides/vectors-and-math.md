# Vectors, quaternions and math

CfxLua has built-in vector and quaternion types. They are real value types, like numbers and strings: `type(vector3(1, 2, 3))` is `'vector3'`. Natives return them for positions, rotations and directions, and accept them as arguments.

## Constructors

| Function | Result |
|---|---|
| `vector2(x, y)` / `vec2(x, y)` | `vector2` |
| `vector3(x, y, z)` / `vec3(x, y, z)` | `vector3` |
| `vector4(x, y, z, w)` / `vec4(x, y, z, w)` | `vector4` |
| `vec(...)` / `vector(...)` | size inferred from the number of values (2 to 4) |
| `quat(...)` | quaternion, see below |

```lua
local spawn = vector3(-268.4, -957.3, 31.2)
local point = vec2(10, 20)
local tint = vector4(255, 0, 0, 128)

local ones = vec3(1)              -- a single value fills every component: vector3(1, 1, 1)
local pos3 = vec3(point, 5)       -- vectors are flattened: vector3(10, 20, 5)
local fromList = vec3({ 1, 2, 3 }) -- arrays are flattened too
local auto = vec(1, 2, 3, 4)      -- vector4
```

A sized constructor needs exactly 1 or N values: `vec3(1, 2)` raises an error.

Components are stored as **32-bit floats**. `vector3(0.1, 0, 0).x` is `0.10000000149011612`, not `0.1`. Avoid exact equality tests on computed coordinates; compare distances with a tolerance.

## Reading components

| Access | Result |
|---|---|
| `v.x`, `v.y`, `v.z`, `v.w` | a component (number) |
| `v.r`, `v.g`, `v.b`, `v.a` | the same components, colour naming |
| `v[1]` .. `v[4]` | the same components, by index |
| `v.xy`, `v.zyx`, `v.xxyy` | **swizzle**: 2 to 4 letters build a new vector |
| `v.n`, `v.dim` | the number of components |
| `#v` | the **length** (magnitude) of the vector |

```lua
local v = vector3(1, 2, 3)

print(v.x, v.y, v.z)    -- 1.0 2.0 3.0
print(v[3])             -- 3.0
local r = v.zyx         -- vector3(3, 2, 1)
local flat = v.xy       -- vector2(1, 2)
local all = v.xxxx      -- vector4(1, 1, 1, 1)
print(v.n)              -- 3
print(#vector3(3, 4, 0)) -- 5.0

local x, y, z = table.unpack(v)
for key, value in pairs(v) do
    print(key, value)   -- x 1.0, y 2.0, z 3.0
end
```

Swizzle rules:

- Multi-letter swizzles only use **`x y z w`**. `v.rg` or `v.rgb` return `nil`; single-letter `r g b a` work.
- A letter beyond the vector's size makes the swizzle fail: `vector2(1, 2).xyz` is `nil`.
- Unknown fields return `nil` rather than raising an error, so a typo such as `v.X` silently gives `nil`. The IDE completes the valid names.

## Vectors are immutable

You cannot change a component in place: `v.x = 5` raises an error. Build a new vector instead:

```lua
local pos = vector3(10.0, 20.0, 30.0)
pos = vector3(pos.x, pos.y, pos.z + 1.0) -- new value
pos = pos + vector3(0.0, 0.0, 1.0)       -- same idea
pos += vector3(0.0, 0.0, 1.0)            -- compound assignment rebinds the variable
```

Because they are values, vectors can be compared with `==` and used as table keys:

```lua
local visited = {}
visited[vector2(4, 5)] = true
print(visited[vector2(4, 5)]) -- true
print(vector3(1, 2, 3) == vector3(1, 2, 3)) -- true
```

`<`, `<=`, `>` and `>=` are not defined for vectors.

## Arithmetic

Operators work component by component, between two vectors of the same size or between a vector and a number:

```lua
local a = vector3(1, 2, 3)
local b = vector3(4, 5, 6)

local sum = a + b        -- vector3(5, 7, 9)
local diff = a - b       -- vector3(-3, -3, -3)
local prod = a * b       -- vector3(4, 10, 18)
local scaled = a * 2     -- vector3(2, 4, 6)
local scaled2 = 2 * a    -- vector3(2, 4, 6)
local half = b / 2       -- vector3(2, 2.5, 3)
local neg = -a           -- vector3(-1, -2, -3)
local floored = (b / 2) // 1.0 -- vector3(2, 2, 3), floor of each component
```

### Common recipes

```lua
-- distance between two points
local function distance(p1, p2)
    return #(p1 - p2)
end

-- horizontal (2D) distance, ignoring height
local function distance2d(p1, p2)
    return #(p1.xy - p2.xy)
end

-- linear interpolation
local function lerp(p1, p2, t)
    return p1 + (p2 - p1) * t
end

-- a point `dist` units in front of a position with a GTA heading (degrees)
local function inFront(pos, heading, dist)
    local rad = math.rad(heading)
    return pos + vector3(-math.sin(rad), math.cos(rad), 0.0) * dist
end
```

`#(a - b)` is the idiomatic and fastest way to get a distance in Lua; there is no need for a native call.

## Global math helpers

| Function | Description |
|---|---|
| `dot(a, b)` | dot product (number) |
| `cross(a, b)` | cross product: `vector3` for two `vector3`, a number for two `vector2` |
| `norm(v)` | normalized vector (length 1), or normalized quaternion |
| `inv(q)` | inverse of a quaternion (or matrix) |
| `slerp(a, b, t)` | spherical interpolation |

```lua
local forward = norm(vector3(3, 4, 0))      -- vector3(0.6, 0.8, 0)
local facing = dot(forward, vector3(0, 1, 0)) -- about 0.8: cosine of the angle
local up = cross(vector3(1, 0, 0), vector3(0, 1, 0)) -- vector3(0, 0, 1)
```

## Quaternions

| Constructor | Meaning |
|---|---|
| `quat()` | identity (no rotation) |
| `quat(w, x, y, z)` | from components |
| `quat(angleDegrees, axis)` | rotation of `angleDegrees` around the `vector3` axis |
| `quat(from, to)` | rotation that turns direction `from` into direction `to` |

```lua
local turn = quat(90.0, vector3(0, 0, 1))    -- 90 degrees around the up axis
local rotated = turn * vector3(1, 0, 0)       -- rotate a direction: about vector3(0, 1, 0)
local combined = turn * turn                  -- 180 degrees
local undo = inv(turn)

local degrees = turn.angle -- about 90 (degrees)
local axis = turn.axis     -- vector3(0, 0, 1)
```

`type(q)` is `'quat'`. Swizzling a quaternion with four letters keeps it a quaternion only if the result is still normalized.

Most game natives use Euler angles in degrees (`GetEntityRotation`, `SetEntityRotation`), as a `vector3`, not quaternions. Quaternions are useful for your own interpolation and composition.

## The `glm` library

A large binding of the GLM mathematics library is available on demand:

```lua
local glm = require 'glm'

local a = vector3(0, 0, 0)
local b = vector3(3, 4, 0)

local len = glm.length(b)          -- 5
local dist = glm.distance(a, b)    -- 5
local dir = glm.normalize(b)       -- vector3(0.6, 0.8, 0)
local clamped = glm.clamp(15, 0, 10) -- 10
local mid = glm.mix(a, b, 0.5)     -- vector3(1.5, 2, 0)
local rad = glm.radians(180)       -- 3.14159...
```

- `glm` is not a global; load it with `require 'glm'` in each file that needs it. `require` cannot load your own files: besides the built-in libraries it only knows a few special modules such as `glm`.
- It covers most GLM functions (`length`, `distance`, `normalize`, `clamp`, `mix`, `rotate`, `eulerAngles`, `quatLookAt`, ...) plus game-friendly helpers such as `moveTowards`, and geometry sub-libraries (`glm.aabb`, `glm.ray`, `glm.sphere`, `glm.plane`, ...).
- Loading `glm` installs it as the metatable of vector values. After that, the quaternion fields `q.angle` and `q.axis` are no longer available as fields; call `glm.angle(q)` and `glm.axis(q)` instead.

## Vectors and natives

- Natives that return a `Vector3` return a `vector3`: `GetEntityCoords`, `GetEntityRotation`, `GetEntityForwardVector`, `GetOffsetFromEntityInWorldCoords`...
- A vector passed to a native is unrolled into consecutive arguments, so `SetEntityCoords(ped, pos, false, false, false, false)` works where the native expects `x, y, z`. This does not work with `use_experimental_fxv2_oal`, and not for natives with hidden out-parameters after the coordinates. See `cfxdoc:natives`.
- Vectors survive events, exports and state bags: they are serialized as vectors, not tables.

```lua
-- client: spawn a prop 2 m in front of the player
local ped = PlayerPedId()
local where = GetOffsetFromEntityInWorldCoords(ped, 0.0, 2.0, 0.0)
print(('prop goes to %.1f, %.1f, %.1f'):format(where.x, where.y, where.z))
```

## Sources

- vector3 reference: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector3/
- quat reference: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/quat/
- Lua runtime manual (vectors and quaternions): https://docs.fivem.net/docs/scripting-manual/runtimes/lua/
- CfxLua (LuaGLM) README: https://github.com/citizenfx/lua/blob/luaglm-548/README.md
- GLM manual: https://github.com/g-truc/glm
