local player = { ped = { coords = { x = 1 } }, getName = function(self) return "p" end }
local x = player?.ped?.coords?.x
local y = player?["ped"]?.coords
local name = player?:getName()
local cb = player?.callback
cb?(1, 2)
local r = cb?"str"
local s = cb?{ 1 }
local h = cb?`hash`
local v = player ? . ped
local z = player?.missing or 10
local nz = not player?.ped
local len = #player?.ped
player?.ped = {}
player?.ped.coords.x += 1
player?:getName()
player?.ped, y = 1, 2
player?.ped.coords?.x = 3
