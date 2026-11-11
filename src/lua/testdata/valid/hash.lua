local model = `adder`
local same = `ADDER`
local empty = ``
local escaped = `a\tb\x41\65`
local function GetHashKey(s) return s end
local v = GetHashKey`prop_bench_01a`
local c <const> = `weapon_pistol` & 0xFFFFFFFF
local t = { [`blista`] = true, .car }
print(`adder` == -1216765807)
