local allowed = { .police, .ambulance, ["mechanic"], vip = false }
local key = "taxi"
local s = { [key], .a = 1, .b, 2, }
local numbers = { .5, 1.5 }
local function varargs(...) return { ... } end
local nested = { .outer, inner = { .x; .y } }
local spaced = { . spacedName }
