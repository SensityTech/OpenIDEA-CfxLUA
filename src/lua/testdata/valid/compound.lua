local count, mask, flags, ratio = 0, 0xFF, 0, 10
count += 1
count -= 2
count *= 3 + 1
ratio /= 2
mask &= ~0x4
flags |= 1 << 3
flags ^= 0x10
flags <<= 2
flags >>= 1
local t = { n = 0, list = { 1, 2 } }
t.n += 1
t["n"] -= 1
t.list[1] *= 2
local function f() return t end
f().n += 5
self = { count = 0 }
self.count += 1;
