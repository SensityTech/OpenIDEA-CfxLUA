local src = { a = 1, b = 2, x = 3, [1] = "one", c = { d = 4 } }
local a, b in src
local x <const> in src
local c in (src)
local d in src.c
local e in src?.c
local function get() return src end
local a2 in get()
a, b in src
x2 in src
local t = {}
t.a, t["b"], t[1] in src
src?.c.d in src
