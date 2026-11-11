local a <const> = 10
local f <close> = nil
local b = 7 // 2
local c = (a & 3) | (b ~ 1) << 2 >> 1
local d = ~c
for i = 1, 3 do
    if i == 2 then goto continue end
    print(i)
    ::continue::
end
local t = setmetatable({}, { __index = function(_, k) return k end })
local s = [[long
string]] .. [==[another]==]
local function va(...) local n = select('#', ...) return n end
while true do break end
repeat local x = 1 until x == 1
local m = {}
function m.f(x) return x end
function m:g(x) return self, x end
m:g(1)
m.f "s"
m.f { 1 }
print(#t, -b, not a)
